//! Process-monotonic clock probes and a future headless software start

use std::time::Duration;

use cocobeat_schema::{PlayerId, SessionEpoch};
use quinn::Connection;
use serde::Serialize;
use tokio::time::Instant;

use crate::{
    PROTOCOL_VERSION,
    clock::{ClockConfig, ClockEstimate, ClockExchange, ClockSync},
    session::ControlIo,
    wire::Control,
};

const MAX_PROBES: u64 = 8;
const MAX_PACKETS: u8 = 64;
const CLOCK_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);
const START_LEAD_NS: u64 = 2_000_000_000;
const MINIMUM_LEAD_NS: u64 = 100_000_000;
const PACKET_BYTES: usize = 48;

#[derive(Clone, Debug, Serialize)]
pub struct ClockSample {
    pub probe_id: u64,
    pub guest_sample_ns: u64,
    pub offset_ns: i64,
    pub uncertainty_ns: u64,
    pub round_trip_min_ns: u64,
    pub round_trip_max_ns: u64,
}

/// Only observed software coordinates; no device-output or acoustic uncertainty
#[derive(Clone, Debug, Default, Serialize)]
pub struct NetworkTiming {
    pub probes_sent: u8,
    pub received_datagrams: u8,
    pub ignored_datagrams: u8,
    pub clock: Option<ClockSample>,
    pub host_start_ns: Option<u64>,
    pub local_start_ns: Option<u64>,
    pub start_uncertainty_ns: Option<u64>,
    pub software_start_observed_ns: Option<u64>,
    pub software_start_lateness_ns: Option<u64>,
}

impl NetworkTiming {
    fn record_clock(&mut self, id: u64, estimate: ClockEstimate) {
        self.clock = Some(ClockSample {
            probe_id: id,
            guest_sample_ns: estimate.guest_ns,
            offset_ns: estimate.offset_ns,
            uncertainty_ns: estimate.uncertainty_ns,
            round_trip_min_ns: estimate.round_trip_min_ns,
            round_trip_max_ns: estimate.round_trip_max_ns,
        });
    }

    fn receive_packet(&mut self) -> Result<(), String> {
        if self.received_datagrams >= MAX_PACKETS {
            return Err("clock datagram receive limit exceeded".into());
        }
        self.received_datagrams += 1;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClockPacket {
    kind: u8,
    epoch: u64,
    attempt: u8,
    id: u64,
    guest_send_ns: u64,
    host_receive_ns: u64,
    host_send_ns: u64,
}

impl ClockPacket {
    fn encode(self) -> [u8; PACKET_BYTES] {
        let mut bytes = [0; PACKET_BYTES];
        bytes[..4].copy_from_slice(b"CBCK");
        bytes[4] = PROTOCOL_VERSION as u8;
        bytes[5] = self.kind;
        bytes[6] = self.attempt;
        for (index, value) in [
            self.epoch,
            self.id,
            self.guest_send_ns,
            self.host_receive_ns,
            self.host_send_ns,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[8 + index * 8..16 + index * 8].copy_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != PACKET_BYTES
            || &bytes[..4] != b"CBCK"
            || bytes[4] != PROTOCOL_VERSION as u8
            || ![1, 2].contains(&bytes[5])
            || bytes[6] > 1
            || bytes[7] != 0
        {
            return None;
        }
        let mut values = [0; 5];
        for (index, value) in values.iter_mut().enumerate() {
            *value = u64::from_be_bytes(bytes[8 + index * 8..16 + index * 8].try_into().ok()?);
        }
        let [epoch, id, guest_send_ns, host_receive_ns, host_send_ns] = values;
        if id >= MAX_PROBES || (bytes[5] == 1 && (host_receive_ns != 0 || host_send_ns != 0)) {
            return None;
        }
        Some(Self {
            kind: bytes[5],
            epoch,
            attempt: bytes[6],
            id,
            guest_send_ns,
            host_receive_ns,
            host_send_ns,
        })
    }
}

pub(crate) fn now_ns(origin: Instant) -> Result<u64, String> {
    u64::try_from(origin.elapsed().as_nanos())
        .map_err(|_| "process monotonic clock overflow".into())
}

pub(crate) fn local_instant(origin: Instant, nanos: u64) -> Result<Instant, String> {
    origin
        .checked_add(Duration::from_nanos(nanos))
        .ok_or_else(|| "scheduled local instant overflow".into())
}

async fn guest_clock(
    connection: &Connection,
    control: &mut ControlIo,
    epoch: SessionEpoch,
    attempt: u8,
    origin: Instant,
    timing: &mut NetworkTiming,
) -> Result<ClockSync, String> {
    let mut clock =
        ClockSync::new(epoch, ClockConfig::default()).map_err(|error| error.to_string())?;
    for id in 0..MAX_PROBES {
        let guest_send_ns = now_ns(origin)?;
        timing.probes_sent += 1;
        connection
            .send_datagram(
                ClockPacket {
                    kind: 1,
                    epoch: epoch.0,
                    attempt,
                    id,
                    guest_send_ns,
                    host_receive_ns: 0,
                    host_send_ns: 0,
                }
                .encode()
                .to_vec()
                .into(),
            )
            .map_err(|_| "send clock probe failed")?;
        let deadline = Instant::now() + PROBE_TIMEOUT;
        loop {
            let bytes = match tokio::time::timeout_at(deadline, connection.read_datagram()).await {
                Err(_) => break,
                Ok(result) => result.map_err(|_| "receive clock reply failed")?,
            };
            let guest_receive_ns = now_ns(origin)?;
            timing.receive_packet()?;
            if let Some(reply) = ClockPacket::decode(&bytes)
                && reply.kind == 2
                && reply.epoch == epoch.0
                && reply.attempt == attempt
                && reply.id == id
                && reply.guest_send_ns == guest_send_ns
            {
                let exchange = ClockExchange {
                    epoch,
                    guest_send_ns,
                    host_receive_ns: reply.host_receive_ns,
                    host_send_ns: reply.host_send_ns,
                    guest_receive_ns,
                };
                if let Ok(estimate) = clock.observe(exchange) {
                    timing.record_clock(id, estimate);
                    control
                        .send(Control::ClockSynced {
                            epoch: epoch.0,
                            attempt,
                            id,
                            guest_send_ns,
                            host_receive_ns: reply.host_receive_ns,
                            host_send_ns: reply.host_send_ns,
                            guest_receive_ns,
                        })
                        .await?;
                    return Ok(clock);
                }
            }
            timing.ignored_datagrams += 1;
        }
    }
    Err("no valid clock sample after eight probes".into())
}

async fn host_clock(
    connection: &Connection,
    control: &mut ControlIo,
    epoch: SessionEpoch,
    attempt: u8,
    origin: Instant,
    timing: &mut NetworkTiming,
) -> Result<ClockSync, String> {
    let mut replies = [None; MAX_PROBES as usize];
    let message = {
        // Preserve one in-flight length-prefixed read across datagram arrivals
        // Cancelling a partial reliable message remains terminal for this session
        let synced = control.recv();
        tokio::pin!(synced);
        loop {
            tokio::select! {
                message = &mut synced => break message?,
                bytes = connection.read_datagram() => {
                    let bytes = bytes.map_err(|_| "receive clock probe failed")?;
                    let host_receive_ns = now_ns(origin)?;
                    timing.receive_packet()?;
                    if let Some(probe) = ClockPacket::decode(&bytes)
                        && probe.kind == 1 && probe.epoch == epoch.0 && probe.attempt == attempt
                        && replies[probe.id as usize].is_none()
                    {
                        let reply = ClockPacket { kind: 2, host_receive_ns, host_send_ns: now_ns(origin)?, ..probe };
                        connection.send_datagram(reply.encode().to_vec().into()).map_err(|_| "send clock reply failed")?;
                        replies[probe.id as usize] = Some(reply);
                    } else {
                        timing.ignored_datagrams += 1;
                    }
                }
            }
        }
    };
    match message {
        Control::ClockSynced {
            epoch: actual,
            attempt: actual_attempt,
            id,
            guest_send_ns,
            host_receive_ns,
            host_send_ns,
            guest_receive_ns,
        } if actual == epoch.0 && actual_attempt == attempt && id < MAX_PROBES => {
            if replies[id as usize]
                != Some(ClockPacket {
                    kind: 2,
                    epoch: actual,
                    attempt,
                    id,
                    guest_send_ns,
                    host_receive_ns,
                    host_send_ns,
                })
            {
                return Err("ClockSynced does not match an actual clock reply".into());
            }
            let mut clock =
                ClockSync::new(epoch, ClockConfig::default()).map_err(|error| error.to_string())?;
            let estimate = clock
                .observe(ClockExchange {
                    epoch,
                    guest_send_ns,
                    host_receive_ns,
                    host_send_ns,
                    guest_receive_ns,
                })
                .map_err(|error| error.to_string())?;
            timing.record_clock(id, estimate);
            Ok(clock)
        }
        _ => Err("expected ClockSynced for this epoch and probe".into()),
    }
}

pub(crate) async fn capture_clock(
    connection: &Connection,
    control: &mut ControlIo,
    epoch: SessionEpoch,
    attempt: u8,
    player: PlayerId,
    origin: Instant,
    timing: &mut NetworkTiming,
) -> Result<ClockSync, String> {
    tokio::time::timeout(CLOCK_TIMEOUT, async {
        if player == PlayerId::P1 {
            host_clock(connection, control, epoch, attempt, origin, timing).await
        } else {
            guest_clock(connection, control, epoch, attempt, origin, timing).await
        }
    })
    .await
    .map_err(|_| "clock probe phase timed out")?
}

/// Both clocks are process-local software clocks; no input/audio mapping occurs
pub(crate) async fn arm_start(
    connection: &Connection,
    control: &mut ControlIo,
    epoch: SessionEpoch,
    player: PlayerId,
    origin: Instant,
    timing: &mut NetworkTiming,
) -> Result<Instant, String> {
    let mut clock = capture_clock(connection, control, epoch, 0, player, origin, timing).await?;
    if player == PlayerId::P2 {
        control.send(Control::Ready { epoch: epoch.0 }).await?;
    }
    if control.recv().await? != (Control::Ready { epoch: epoch.0 }) {
        return Err("expected the peer Ready after clock synchronization".into());
    }
    if player == PlayerId::P1 {
        control.send(Control::Ready { epoch: epoch.0 }).await?;
        let host_start_ns = now_ns(origin)?
            .checked_add(START_LEAD_NS)
            .ok_or("host start overflow")?;
        timing.host_start_ns = Some(host_start_ns);
        timing.local_start_ns = Some(host_start_ns);
        timing.start_uncertainty_ns = Some(0);
        control
            .send(Control::ScheduleStart {
                epoch: epoch.0,
                host_start_ns,
            })
            .await?;
        let deadline = local_instant(origin, host_start_ns - MINIMUM_LEAD_NS)?;
        let ack = tokio::time::timeout_at(deadline, control.recv())
            .await
            .map_err(|_| "ScheduleStartAck missed the arm deadline")??;
        match ack {
            Control::ScheduleStartAck {
                epoch: actual,
                host_start_ns: echoed,
                guest_now_ns,
                guest_start_ns,
                uncertainty_ns,
            } if actual == epoch.0 && echoed == host_start_ns => {
                let expected = clock
                    .schedule_start(epoch, host_start_ns, guest_now_ns, MINIMUM_LEAD_NS)
                    .map_err(|error| error.to_string())?;
                if guest_start_ns != expected.guest_start_ns
                    || uncertainty_ns != expected.uncertainty_ns
                {
                    return Err("ScheduleStartAck clock mapping differs".into());
                }
            }
            _ => return Err("ScheduleStartAck does not match this epoch and host deadline".into()),
        }
    } else {
        let host_start_ns = match control.recv().await? {
            Control::ScheduleStart {
                epoch: actual,
                host_start_ns,
            } if actual == epoch.0 => host_start_ns,
            _ => return Err("expected ScheduleStart for this epoch".into()),
        };
        timing.host_start_ns = Some(host_start_ns);
        let guest_now_ns = now_ns(origin)?;
        let start = clock
            .schedule_start(epoch, host_start_ns, guest_now_ns, MINIMUM_LEAD_NS)
            .map_err(|error| error.to_string())?;
        timing.local_start_ns = Some(start.guest_start_ns);
        timing.start_uncertainty_ns = Some(start.uncertainty_ns);
        control
            .send(Control::ScheduleStartAck {
                epoch: epoch.0,
                host_start_ns,
                guest_now_ns,
                guest_start_ns: start.guest_start_ns,
                uncertainty_ns: start.uncertainty_ns,
            })
            .await?;
    }
    let local_start_ns = timing
        .local_start_ns
        .ok_or("local start was not scheduled")?;
    let uncertainty = timing
        .start_uncertainty_ns
        .ok_or("start uncertainty was not established")?;
    if now_ns(origin)?
        .checked_add(MINIMUM_LEAD_NS)
        .ok_or("arm deadline overflow")?
        > local_start_ns
            .checked_sub(uncertainty)
            .ok_or("start interval precedes local origin")?
    {
        return Err("scheduled start no longer leaves the minimum arm lead".into());
    }
    local_instant(origin, local_start_ns)
}

pub(crate) async fn wait_start(
    connection: &Connection,
    start: Instant,
    origin: Instant,
    timing: &mut NetworkTiming,
) -> Result<Instant, String> {
    let local_start_ns = timing
        .local_start_ns
        .ok_or("local start was not scheduled")?;
    tokio::select! {
        biased;
        _ = connection.closed() => return Err("connection closed before scheduled software start".into()),
        _ = tokio::time::sleep_until(start) => {}
    }
    let observed_ns = now_ns(origin)?;
    timing.software_start_observed_ns = Some(observed_ns);
    let late = observed_ns
        .checked_sub(local_start_ns)
        .ok_or("software start woke before its deadline")?;
    timing.software_start_lateness_ns = Some(late);
    if late > MINIMUM_LEAD_NS {
        return Err("software start missed its deadline by more than 100 milliseconds".into());
    }
    Ok(start)
}

pub(crate) async fn ready_start(
    connection: &Connection,
    control: &mut ControlIo,
    epoch: SessionEpoch,
    player: PlayerId,
    origin: Instant,
    timing: &mut NetworkTiming,
) -> Result<Instant, String> {
    let start = arm_start(connection, control, epoch, player, origin, timing).await?;
    wait_start(connection, start, origin, timing).await
}

pub(crate) const RESUME_ATTEMPT: u8 = 1;
pub(crate) const RESUME_GUARD_FRAMES: i64 = 2_400;
const VERIFY_LEAD_NS: u64 = 100_000_000;

pub(crate) struct ResumePlan {
    pub deadline: Instant,
    pub verify_at: Instant,
    pub host_verify_ns: u64,
    pub common_frame: i64,
    pub start_uncertainties_ns: [u64; 2],
    pub phase_deadline: Option<Instant>,
    pub timing: NetworkTiming,
}

pub(crate) fn catchup_ns(common: i64, paused: i64) -> Result<u64, String> {
    let frames = u64::try_from(common.checked_sub(paused).ok_or("catchup frame overflow")?)
        .map_err(|_| "catchup must not rewind the source")?;
    u64::try_from((u128::from(frames) * 1_000_000_000).div_ceil(48_000))
        .map_err(|_| "catchup duration overflow".into())
}

pub(crate) async fn arm_resume(
    connection: &Connection,
    control: &mut ControlIo,
    epoch: SessionEpoch,
    player: PlayerId,
    origin: Instant,
    continuation: ([i64; 2], Option<u64>),
    end: i64,
) -> Result<ResumePlan, String> {
    let (paused, host_deadline_ns) = continuation;
    let common_frame = *paused.iter().max().ok_or("missing paused frames")?;
    if paused.iter().any(|frame| !(0..end).contains(frame))
        || common_frame
            .checked_add(4_800)
            .is_none_or(|verify| verify >= end)
    {
        return Err("resume needs valid sources and more than 100 ms before song end".into());
    }
    let delays = [
        catchup_ns(common_frame, paused[0])?,
        catchup_ns(common_frame, paused[1])?,
    ];
    let mut timing = NetworkTiming::default();
    let mut clock = capture_clock(
        connection,
        control,
        epoch,
        RESUME_ATTEMPT,
        player,
        origin,
        &mut timing,
    )
    .await?;
    let phase_deadline = if let Some(host_deadline) = host_deadline_ns {
        let local_deadline = if player == PlayerId::P1 {
            host_deadline
        } else {
            clock
                .conservative_deadline(epoch, host_deadline, now_ns(origin)?, MINIMUM_LEAD_NS)
                .map_err(|error| error.to_string())?
        };
        Some(local_instant(origin, local_deadline)?)
    } else {
        None
    };
    let deadline = phase_deadline.unwrap_or(Instant::now() + Duration::from_secs(30));
    tokio::time::timeout_at(deadline, async {
        if player == PlayerId::P2 {
            control
                .send(Control::ResumeReady {
                    epoch: epoch.0,
                    attempt: RESUME_ATTEMPT,
                })
                .await?;
        }
        if control.recv().await?
            != (Control::ResumeReady {
                epoch: epoch.0,
                attempt: RESUME_ATTEMPT,
            })
        {
            return Err("expected peer resume Ready for this attempt".into());
        }
        let (
            host_common_ns,
            host_verify_ns,
            local_resume_ns,
            local_verify_ns,
            uncertainty,
            start_uncertainties_ns,
        ) = if player == PlayerId::P1 {
            control
                .send(Control::ResumeReady {
                    epoch: epoch.0,
                    attempt: RESUME_ATTEMPT,
                })
                .await?;
            let common = now_ns(origin)?
                .checked_add(START_LEAD_NS)
                .and_then(|time| time.checked_add(*delays.iter().max()?))
                .ok_or("resume common time overflow")?;
            let verify = common
                .checked_add(VERIFY_LEAD_NS)
                .ok_or("resume verification time overflow")?;
            control
                .send(Control::ResumeSchedule {
                    epoch: epoch.0,
                    attempt: RESUME_ATTEMPT,
                    host_common_ns: common,
                    host_verify_ns: verify,
                    common_frame,
                    host_paused_frame: paused[0],
                    guest_paused_frame: paused[1],
                })
                .await?;
            let guest_uncertainty = match control.recv().await? {
                Control::ResumeScheduleAck {
                    epoch: actual,
                    attempt: RESUME_ATTEMPT,
                    host_common_ns: echoed,
                    common_frame: echoed_frame,
                    guest_now_ns,
                    guest_resume_ns,
                    guest_common_ns,
                    guest_verify_ns,
                    uncertainty_ns,
                } if actual == epoch.0 && echoed == common && echoed_frame == common_frame => {
                    let expected_resume = clock
                        .schedule_start(
                            epoch,
                            common
                                .checked_sub(delays[1])
                                .ok_or("guest resume time overflow")?,
                            guest_now_ns,
                            MINIMUM_LEAD_NS,
                        )
                        .map_err(|error| error.to_string())?;
                    let expected_common = clock
                        .schedule_start(epoch, common, guest_now_ns, MINIMUM_LEAD_NS)
                        .map_err(|error| error.to_string())?;
                    let expected_verify = clock
                        .schedule_start(epoch, verify, guest_now_ns, MINIMUM_LEAD_NS)
                        .map_err(|error| error.to_string())?;
                    if guest_resume_ns != expected_resume.guest_start_ns
                        || guest_common_ns != expected_common.guest_start_ns
                        || guest_verify_ns != expected_verify.guest_start_ns
                        || uncertainty_ns != expected_resume.uncertainty_ns
                    {
                        return Err(
                            "resume schedule Ack differs from the complete clock mapping".into(),
                        );
                    }
                    uncertainty_ns
                }
                _ => return Err("resume schedule Ack identity or attempt differs".into()),
            };
            (
                common,
                verify,
                common
                    .checked_sub(delays[0])
                    .ok_or("host resume time overflow")?,
                verify,
                0,
                [0, guest_uncertainty],
            )
        } else {
            let (common, verify) = match control.recv().await? {
                Control::ResumeSchedule {
                    epoch: actual,
                    attempt: RESUME_ATTEMPT,
                    host_common_ns,
                    host_verify_ns,
                    common_frame: actual_frame,
                    host_paused_frame,
                    guest_paused_frame,
                } if actual == epoch.0
                    && actual_frame == common_frame
                    && [host_paused_frame, guest_paused_frame] == paused
                    && host_common_ns.checked_add(VERIFY_LEAD_NS) == Some(host_verify_ns) =>
                {
                    (host_common_ns, host_verify_ns)
                }
                _ => return Err("resume schedule differs from frozen sources or attempt".into()),
            };
            let guest_now_ns = now_ns(origin)?;
            let resume = clock
                .schedule_start(
                    epoch,
                    common
                        .checked_sub(delays[1])
                        .ok_or("guest resume time overflow")?,
                    guest_now_ns,
                    MINIMUM_LEAD_NS,
                )
                .map_err(|error| error.to_string())?;
            let reached = clock
                .schedule_start(epoch, common, guest_now_ns, MINIMUM_LEAD_NS)
                .map_err(|error| error.to_string())?;
            let observed = clock
                .schedule_start(epoch, verify, guest_now_ns, MINIMUM_LEAD_NS)
                .map_err(|error| error.to_string())?;
            control
                .send(Control::ResumeScheduleAck {
                    epoch: epoch.0,
                    attempt: RESUME_ATTEMPT,
                    host_common_ns: common,
                    common_frame,
                    guest_now_ns,
                    guest_resume_ns: resume.guest_start_ns,
                    guest_common_ns: reached.guest_start_ns,
                    guest_verify_ns: observed.guest_start_ns,
                    uncertainty_ns: resume.uncertainty_ns,
                })
                .await?;
            (
                common,
                verify,
                resume.guest_start_ns,
                observed.guest_start_ns,
                resume.uncertainty_ns,
                [0, resume.uncertainty_ns],
            )
        };
        if now_ns(origin)?
            .checked_add(MINIMUM_LEAD_NS)
            .ok_or("resume arm time overflow")?
            > local_resume_ns
                .checked_sub(uncertainty)
                .ok_or("resume interval precedes origin")?
        {
            return Err("resume no longer leaves the full minimum arm lead".into());
        }
        timing.host_start_ns = Some(host_common_ns);
        timing.local_start_ns = Some(local_resume_ns);
        timing.start_uncertainty_ns = Some(uncertainty);
        Ok(ResumePlan {
            deadline: local_instant(origin, local_resume_ns)?,
            verify_at: local_instant(origin, local_verify_ns)?,
            host_verify_ns,
            common_frame,
            start_uncertainties_ns,
            phase_deadline,
            timing,
        })
    })
    .await
    .map_err(|_| "resume scheduling exceeded the original phase deadline")?
}

#[derive(Clone, Copy)]
pub(crate) struct GateWindow {
    pub paused: [i64; 2],
    pub sources: [(u64, u64); 2],
    pub end: i64,
    pub common_frame: i64,
    pub host_verify_ns: u64,
}

/// A past publication bracket, under ClockSync's explicit monotonic-rate assumption
/// This does not extrapolate the sound or constrain any future backend callback
pub(crate) fn resume_gate(
    clock: &mut ClockSync,
    sample_guest_ns: u64,
    evidence: &[crate::wire::GateEvidence; 2],
    window: GateWindow,
) -> Result<[[i64; 2]; 2], String> {
    let GateWindow {
        paused,
        sources,
        end,
        common_frame,
        host_verify_ns,
    } = window;
    if paused.iter().any(|frame| !(0..end).contains(frame))
        || paused.iter().max().copied() != Some(common_frame)
    {
        return Err("gate paused sources differ from the frozen common frame".into());
    }
    let mut intervals = [[0; 2]; 2];
    for (index, (rows, expected_source)) in evidence.iter().zip(sources).enumerate() {
        if (rows.generation, rows.source_id) != expected_source
            || expected_source.0 == 0
            || expected_source.1 == 0
            || rows.progress_sequence == 0
            || !(2..=64).contains(&rows.observations.len())
        {
            return Err("resume source identity or publication count differs".into());
        }
        let mut previous = None;
        let mut before = None;
        let mut after = None;
        for row in &rows.observations {
            if row.sequence <= rows.progress_sequence
                || !(paused[index] + 1..end).contains(&row.frame)
                || row.publication_before_ns > row.publication_after_ns
                || previous.is_some_and(|old: &crate::wire::GateObservation| {
                    row.sequence <= old.sequence
                        || row.frame < old.frame
                        || row.publication_before_ns < old.publication_after_ns
                })
            {
                return Err("resume publications cross a discontinuity or move backwards".into());
            }
            previous = Some(row);
            if index == 1 && row.publication_before_ns < sample_guest_ns {
                continue;
            }
            let map = |clock: &mut ClockSync, nanos: u64| -> Result<[u64; 2], String> {
                if index == 0 {
                    return Ok([nanos; 2]);
                }
                let estimate = clock.estimate(nanos).map_err(|error| error.to_string())?;
                let midpoint = i128::from(nanos) + i128::from(estimate.offset_ns);
                let error = i128::from(estimate.uncertainty_ns);
                Ok([
                    u64::try_from(midpoint - error).map_err(|_| "resume interval before origin")?,
                    u64::try_from(midpoint + error).map_err(|_| "resume interval overflow")?,
                ])
            };
            let lo = map(clock, row.publication_before_ns)?;
            let hi = map(clock, row.publication_after_ns)?;
            if hi[1] <= host_verify_ns {
                before = Some(row.frame);
            }
            if after.is_none() && lo[0] >= host_verify_ns {
                after = Some(row.frame);
            }
        }
        intervals[index] = [
            before
                .ok_or("resume lacks a publication before the fixed common point")?
                .checked_sub(1)
                .ok_or("resume quantization underflow")?,
            after
                .ok_or("resume lacks a publication after the fixed common point")?
                .checked_add(1)
                .ok_or("resume quantization overflow")?,
        ];
    }
    let expected = common_frame
        .checked_add(4_800)
        .ok_or("resume expected frame overflow")?;
    let low = expected
        .checked_sub(RESUME_GUARD_FRAMES)
        .ok_or("resume guard underflow")?;
    let high = expected
        .checked_add(RESUME_GUARD_FRAMES)
        .ok_or("resume guard overflow")?;
    if intervals
        .iter()
        .any(|range| range[0] < low || range[1] > high || range[0] > range[1])
        || (i128::from(intervals[0][0]) - i128::from(intervals[1][1])).abs()
            > i128::from(RESUME_GUARD_FRAMES)
        || (i128::from(intervals[0][1]) - i128::from(intervals[1][0])).abs()
            > i128::from(RESUME_GUARD_FRAMES)
    {
        let diagnostic = serde_json::json!({
            "intervals_frames": intervals,
            "expected_frame": expected,
            "low_frame": low,
            "high_frame": high,
            "paused_frames": paused,
            "common_frame": common_frame,
            "host_verify_ns": host_verify_ns,
            "sources_generation_id": sources,
        });
        return Err(format!(
            "actual resume publications exceed the frozen 50 millisecond guard; diagnostics={diagnostic}"
        ));
    }
    Ok(intervals)
}

// Active-connection maintenance is a separate packet domain and round budget
// Initial admission and the one authenticated continuation keep CBCK attempt 0/1
const MAINTENANCE_BYTES: usize = 56;
pub(crate) const MAX_MAINTENANCE_ROUNDS: u64 = 1_024;
const MAINTENANCE_PERIOD_NS: u64 = 1_000_000_000;
const MAINTENANCE_DEADLINE_NS: u64 = 250_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MaintenancePacket {
    kind: u8,
    epoch: u64,
    round: u64,
    guest_send_ns: u64,
    host_receive_ns: u64,
    host_send_ns: u64,
    guest_receive_ns: u64,
}

impl MaintenancePacket {
    fn encode(self) -> [u8; MAINTENANCE_BYTES] {
        let mut bytes = [0; MAINTENANCE_BYTES];
        bytes[..4].copy_from_slice(b"CBMC");
        bytes[4] = PROTOCOL_VERSION as u8;
        bytes[5] = self.kind;
        for (index, value) in [
            self.epoch,
            self.round,
            self.guest_send_ns,
            self.host_receive_ns,
            self.host_send_ns,
            self.guest_receive_ns,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[8 + index * 8..16 + index * 8].copy_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != MAINTENANCE_BYTES
            || &bytes[..4] != b"CBMC"
            || bytes[4] != PROTOCOL_VERSION as u8
            || !(1..=3).contains(&bytes[5])
            || bytes[6..8] != [0, 0]
        {
            return None;
        }
        let mut values = [0; 6];
        for (index, value) in values.iter_mut().enumerate() {
            *value = u64::from_be_bytes(bytes[8 + index * 8..16 + index * 8].try_into().ok()?);
        }
        let [
            epoch,
            round,
            guest_send_ns,
            host_receive_ns,
            host_send_ns,
            guest_receive_ns,
        ] = values;
        if !(1..=MAX_MAINTENANCE_ROUNDS).contains(&round)
            || (bytes[5] == 1
                && (host_receive_ns != 0 || host_send_ns != 0 || guest_receive_ns != 0))
            || (bytes[5] == 2 && guest_receive_ns != 0)
        {
            return None;
        }
        Some(Self {
            kind: bytes[5],
            epoch,
            round,
            guest_send_ns,
            host_receive_ns,
            host_send_ns,
            guest_receive_ns,
        })
    }

    fn exchange(self) -> ClockExchange {
        ClockExchange {
            epoch: SessionEpoch(self.epoch),
            guest_send_ns: self.guest_send_ns,
            host_receive_ns: self.host_receive_ns,
            host_send_ns: self.host_send_ns,
            guest_receive_ns: self.guest_receive_ns,
        }
    }
}

/// A single datagram reader drives this state beside the live fact FIFO
/// No partial ControlIo read is cancelled and no sample is assumed at creation
pub(crate) struct ClockMaintenance {
    epoch: SessionEpoch,
    player: PlayerId,
    pub(crate) clock: ClockSync,
    round: u64,
    pending: Option<(MaintenancePacket, u64)>,
    next_probe_ns: u64,
    last_now_ns: u64,
    last_valid_local_ns: u64,
    received: u8,
    accepted: Option<MaintainedClock>,
}

#[derive(Clone, Copy)]
pub(crate) struct MaintainedClock {
    pub round: u64,
    pub exchange: ClockExchange,
    pub estimate: ClockEstimate,
}

pub(crate) struct MaintenanceResult {
    pub reply: Option<[u8; MAINTENANCE_BYTES]>,
    pub sample: Option<MaintainedClock>,
}

impl ClockMaintenance {
    /// Carry `last_round` across the authenticated reconnect, separately from used
    pub(crate) fn new(
        epoch: SessionEpoch,
        player: PlayerId,
        last_round: u64,
        now_ns: u64,
    ) -> Result<Self, String> {
        if last_round >= MAX_MAINTENANCE_ROUNDS {
            return Err("clock maintenance round budget exhausted".into());
        }
        Ok(Self {
            epoch,
            player,
            clock: ClockSync::new(epoch, ClockConfig::default())
                .map_err(|error| error.to_string())?,
            round: last_round,
            pending: None,
            next_probe_ns: now_ns,
            last_now_ns: now_ns,
            // This is only a startup deadline; ClockSync remains Uncalibrated
            last_valid_local_ns: now_ns,
            received: 0,
            accepted: None,
        })
    }

    pub(crate) fn sample(&self) -> Option<MaintainedClock> {
        self.accepted
    }

    pub(crate) fn reconnect(&mut self, now_ns: u64) -> Result<(), String> {
        if now_ns < self.last_now_ns || self.round >= MAX_MAINTENANCE_ROUNDS {
            return Err("clock continuation rewinds time or exhausts its original rounds".into());
        }
        self.pending = None;
        self.accepted = None;
        self.received = 0;
        self.last_now_ns = now_ns;
        // Startup freshness is not a sample; sample() remains None until actual new CBMC
        self.last_valid_local_ns = now_ns;
        self.next_probe_ns = now_ns;
        Ok(())
    }

    pub(crate) fn round(&self) -> u64 {
        self.round
    }

    pub(crate) fn next_deadline_ns(&self) -> Result<u64, String> {
        let stale = self
            .last_valid_local_ns
            .checked_add(ClockConfig::default().max_sample_age_ns)
            .and_then(|time| time.checked_add(1))
            .ok_or("clock maintenance age overflow")?;
        let pending = self.pending.map_or(stale, |(_, deadline)| deadline);
        Ok(stale.min(pending).min(if self.player == PlayerId::P2 {
            self.next_probe_ns
        } else {
            stale
        }))
    }

    pub(crate) fn check_local_time(&self, now_ns: u64) -> Result<(), crate::clock::ClockError> {
        if now_ns < self.last_now_ns {
            return Err(crate::clock::ClockError::NonMonotonic);
        }
        if now_ns - self.last_valid_local_ns > ClockConfig::default().max_sample_age_ns {
            return Err(crate::clock::ClockError::Stale);
        }
        Ok(())
    }

    fn check_now(&mut self, now_ns: u64) -> Result<(), String> {
        self.check_local_time(now_ns)
            .map_err(|error| error.to_string())?;
        self.last_now_ns = now_ns;
        if self.pending.is_some_and(|(_, deadline)| now_ns >= deadline) {
            self.pending = None;
        }
        Ok(())
    }

    pub(crate) fn tick(&mut self, now_ns: u64) -> Result<Option<[u8; MAINTENANCE_BYTES]>, String> {
        self.check_now(now_ns)?;
        if self.player != PlayerId::P2 || now_ns < self.next_probe_ns {
            return Ok(None);
        }
        let round = self
            .round
            .checked_add(1)
            .ok_or("clock maintenance round overflow")?;
        if round > MAX_MAINTENANCE_ROUNDS {
            return Err("clock maintenance round budget exhausted".into());
        }
        let deadline = now_ns
            .checked_add(MAINTENANCE_DEADLINE_NS)
            .ok_or("clock maintenance deadline overflow")?;
        let next = now_ns
            .checked_add(MAINTENANCE_PERIOD_NS)
            .ok_or("clock maintenance cadence overflow")?;
        let probe = MaintenancePacket {
            kind: 1,
            epoch: self.epoch.0,
            round,
            guest_send_ns: now_ns,
            host_receive_ns: 0,
            host_send_ns: 0,
            guest_receive_ns: 0,
        };
        self.round = round;
        self.pending = Some((probe, deadline));
        self.next_probe_ns = next;
        self.received = 0;
        Ok(Some(probe.encode()))
    }

    pub(crate) fn receive(
        &mut self,
        bytes: &[u8],
        now_ns: u64,
        host_send_ns: u64,
    ) -> Result<MaintenanceResult, String> {
        self.check_now(now_ns)?;
        if self.received >= MAX_PACKETS {
            return Err("clock maintenance datagram receive limit exceeded".into());
        }
        self.received += 1;
        let ignored = MaintenanceResult {
            reply: None,
            sample: None,
        };
        let Some(packet) = MaintenancePacket::decode(bytes) else {
            return Ok(ignored);
        };
        if packet.epoch != self.epoch.0 {
            return Err("clock maintenance epoch differs from the active session".into());
        }
        if self.player == PlayerId::P1 && packet.kind == 1 {
            if packet.round <= self.round || now_ns < self.next_probe_ns {
                return Ok(ignored);
            }
            let deadline = now_ns
                .checked_add(MAINTENANCE_DEADLINE_NS)
                .ok_or("clock maintenance deadline overflow")?;
            // Bound accepted probe rate while allowing 250 ms path jitter
            let next = now_ns
                .checked_add(MAINTENANCE_PERIOD_NS - MAINTENANCE_DEADLINE_NS * 2)
                .ok_or("clock maintenance cadence overflow")?;
            if host_send_ns < now_ns || host_send_ns >= deadline {
                return Err("clock maintenance host processing missed its deadline".into());
            }
            let reply = MaintenancePacket {
                kind: 2,
                host_receive_ns: now_ns,
                host_send_ns,
                ..packet
            };
            self.round = packet.round;
            self.pending = Some((reply, deadline));
            self.next_probe_ns = next;
            self.received = 0;
            return Ok(MaintenanceResult {
                reply: Some(reply.encode()),
                sample: None,
            });
        }
        let Some((pending, _)) = self.pending else {
            return Ok(ignored);
        };
        let exchange = if self.player == PlayerId::P2 && packet.kind == 2 {
            if pending.kind != 1
                || packet.round != pending.round
                || packet.guest_send_ns != pending.guest_send_ns
            {
                return Ok(ignored);
            }
            MaintenancePacket {
                kind: 3,
                guest_receive_ns: now_ns,
                ..packet
            }
        } else if self.player == PlayerId::P1 && packet.kind == 3 {
            if (MaintenancePacket {
                kind: 2,
                guest_receive_ns: 0,
                ..packet
            }) != pending
            {
                return Ok(ignored);
            }
            packet
        } else {
            return Ok(ignored);
        };
        let estimate = self
            .clock
            .observe(exchange.exchange())
            .map_err(|error| error.to_string())?;
        self.pending = None;
        self.last_valid_local_ns = if self.player == PlayerId::P1 {
            // The actual probe receipt precedes the fourth timestamp and confirmation
            exchange.host_receive_ns
        } else {
            exchange.guest_receive_ns
        };
        let sample = MaintainedClock {
            round: exchange.round,
            exchange: exchange.exchange(),
            estimate,
        };
        self.accepted = Some(sample);
        Ok(MaintenanceResult {
            reply: (self.player == PlayerId::P2).then(|| exchange.encode()),
            sample: Some(sample),
        })
    }
}

const PHASE_SOURCE_MAX_AGE_NS: u64 = 50_000_000;
const PHASE_WINDOW_NS: u64 = 1_000_000_000;
const PHASE_DELIVERY_MAX_AGE_NS: u64 = 250_000_000;
pub(crate) const MAX_PHASE_ROUNDS: u16 = 128;
pub(crate) const MAX_PHASE_CORRECTIONS: u8 = 64;

#[derive(Clone, Copy)]
pub(crate) struct PhaseWindow {
    pub epoch: SessionEpoch,
    pub round: u16,
    /// False for the check window, true only after actual pause and resume
    pub verification: bool,
    pub attempt: u8,
    pub reconnecting: bool,
    pub sources: [(u64, u64); 2],
    pub previous: [Option<crate::wire::PhasePublication>; 2],
    pub end: i64,
    pub host_point_ns: u64,
    /// Actual host receipt of both reliable evidence messages
    pub host_now_ns: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhaseBounds {
    epoch: SessionEpoch,
    round: u16,
    host_point_ns: u64,
    host_received_ns: u64,
    stage: crate::wire::PhaseProofStage,
    source_frames: [[i64; 2]; 2],
    guest_minus_host_frames: [i64; 2],
    last_publications: [crate::wire::PhasePublication; 2],
}

impl PhaseBounds {
    pub(crate) fn source_frames(self) -> [[i64; 2]; 2] {
        self.source_frames
    }
    pub(crate) fn difference(self) -> [i64; 2] {
        self.guest_minus_host_frames
    }
    pub(crate) fn last_publications(self) -> [crate::wire::PhasePublication; 2] {
        self.last_publications
    }

    pub(crate) fn within_guard(self) -> bool {
        self.guest_minus_host_frames[0] <= self.guest_minus_host_frames[1]
            && self
                .guest_minus_host_frames
                .iter()
                .all(|difference| difference.unsigned_abs() <= RESUME_GUARD_FRAMES as u64)
    }
    pub(crate) fn supports_guard(self) -> bool {
        self.guest_minus_host_frames[1]
            .checked_sub(self.guest_minus_host_frames[0])
            .is_some_and(|width| (0..=2 * RESUME_GUARD_FRAMES).contains(&width))
            && self.source_frames.iter().all(|range| {
                range[1]
                    .checked_sub(range[0])
                    .is_some_and(|width| (0..=2 * RESUME_GUARD_FRAMES).contains(&width))
            })
    }

    pub(crate) fn verify_reconnect_resume(
        self,
        paused: [crate::wire::PhasePublication; 2],
        common_frame: i64,
        host_common_ns: u64,
        start_uncertainties_ns: [u64; 2],
    ) -> Result<(), String> {
        let elapsed = self
            .host_point_ns
            .checked_sub(host_common_ns)
            .ok_or("reconnected phase point precedes the actual common frame")?;
        let advance = u128::from(elapsed) * 48_000;
        let expected_low = common_frame
            .checked_add(
                i64::try_from(advance / 1_000_000_000)
                    .map_err(|_| "reconnected phase progress overflow")?,
            )
            .ok_or("reconnected expected frame overflow")?;
        let expected_high = common_frame
            .checked_add(
                i64::try_from(advance.div_ceil(1_000_000_000))
                    .map_err(|_| "reconnected phase progress overflow")?,
            )
            .ok_or("reconnected expected frame overflow")?;
        if self.stage != crate::wire::PhaseProofStage::ReconnectVerification || !self.within_guard()
        {
            return Err(
                "reconnected phase requires its new original-source proof within guard".into(),
            );
        }
        for index in 0..2 {
            let nanos = u128::from(start_uncertainties_ns[index])
                + (u128::from(elapsed) * u128::from(ClockConfig::default().max_drift_ppm))
                    .div_ceil(1_000_000);
            let uncertainty = i64::try_from((nanos * 48_000).div_ceil(1_000_000_000))
                .map_err(|_| "reconnected progress uncertainty overflow")?
                .checked_add(1)
                .ok_or("reconnected quantization overflow")?;
            if uncertainty > RESUME_GUARD_FRAMES {
                return Err("reconnected progress uncertainty exceeds unchanged guard".into());
            }
            let low = expected_low
                .checked_sub(RESUME_GUARD_FRAMES)
                .and_then(|n| n.checked_sub(uncertainty))
                .ok_or("reconnected progress guard underflow")?;
            let high = expected_high
                .checked_add(RESUME_GUARD_FRAMES)
                .and_then(|n| n.checked_add(uncertainty))
                .ok_or("reconnected progress guard overflow")?;
            let last = self.last_publications[index];
            let frozen = paused[index];
            if self.source_frames[index][0] < low
                || self.source_frames[index][1] > high
                || last.sequence <= frozen.sequence
                || f64::from_bits(last.position_seconds_bits)
                    <= f64::from_bits(frozen.position_seconds_bits)
                || last.publication_before_ns < frozen.publication_after_ns
            {
                return Err("reconnected original source fails the full pause floor or bounded common-frame progress".into());
            }
        }
        Ok(())
    }

    pub(crate) fn verify_original_resume(
        self,
        paused: [crate::wire::PhasePublication; 2],
        common_frame: i64,
    ) -> Result<(), String> {
        let expected = common_frame
            .checked_add(4_800)
            .ok_or("phase expected frame overflow")?;
        let low = expected
            .checked_sub(RESUME_GUARD_FRAMES)
            .ok_or("phase resume guard underflow")?;
        let high = expected
            .checked_add(RESUME_GUARD_FRAMES)
            .ok_or("phase resume guard overflow")?;
        if self.stage != crate::wire::PhaseProofStage::Correction
            || !self.within_guard()
            || self
                .source_frames
                .iter()
                .any(|range| range[0] < low || range[1] > high)
            || self
                .last_publications
                .iter()
                .zip(paused)
                .any(|(last, frozen)| {
                    last.sequence <= frozen.sequence
                        || f64::from_bits(last.position_seconds_bits)
                            <= f64::from_bits(frozen.position_seconds_bits)
                        || last.publication_before_ns < frozen.publication_after_ns
                })
        {
            return Err("phase original sources did not reach the scheduled common frame".into());
        }
        Ok(())
    }
}

/// Bound a common past point, never extrapolate source positions or choose midpoint
/// The accepted CBMC exchange is frozen for this phase round, not recomputed from data
pub(crate) fn source_phase_bounds(
    clock: &mut ClockSync,
    actual_exchange: ClockExchange,
    evidence: &[crate::wire::PhaseEvidence; 2],
    window: PhaseWindow,
) -> Result<PhaseBounds, String> {
    if window.attempt > 1
        || (window.reconnecting && (window.attempt != 1 || !window.verification))
        || window.round == 0
        || window.round > MAX_PHASE_ROUNDS
        || window.end <= 0
        || window.end as u64 > cocobeat_schema::MAX_CANONICAL_FRAMES
        || window.host_point_ns > window.host_now_ns
    {
        return Err("phase epoch, song extent or common past point is invalid".into());
    }
    if clock.original_exchange() != Some(actual_exchange) || actual_exchange.epoch != window.epoch {
        return Err("phase clock anchor differs from the actual accepted sample".into());
    }
    let sample_guest_ns = actual_exchange.guest_receive_ns;
    clock
        .estimate(sample_guest_ns)
        .map_err(|error| error.to_string())?;
    let mut intervals = [[0; 2]; 2];
    let mut last_publications = [None; 2];
    for (index, rows) in evidence.iter().enumerate() {
        rows.validate()?;
        if (rows.generation, rows.source_id) != window.sources[index] {
            return Err("phase source identity differs from the original source".into());
        }
        let first = rows
            .publications
            .first()
            .ok_or("phase publications missing")?;
        let last = rows
            .publications
            .last()
            .ok_or("phase publications missing")?;
        if window.previous[index].is_some_and(|previous| {
            first.sequence <= previous.sequence
                || f64::from_bits(first.position_seconds_bits)
                    < f64::from_bits(previous.position_seconds_bits)
                || first.publication_before_ns < previous.publication_after_ns
        }) {
            return Err(
                "phase source publications repeat or rewind an already accepted window".into(),
            );
        }
        last_publications[index] = Some(*last);
        if rows
            .collected_at_ns
            .checked_sub(last.publication_before_ns)
            .is_none_or(|age| age > PHASE_SOURCE_MAX_AGE_NS)
            || rows
                .collected_at_ns
                .checked_sub(first.publication_before_ns)
                .is_none_or(|age| age > PHASE_WINDOW_NS)
            || (index == 1 && first.publication_before_ns < sample_guest_ns)
        {
            return Err("phase publication is stale or outside the accepted clock window".into());
        }
        let mut before = None;
        let mut after = None;
        for row in &rows.publications {
            let frame = cocobeat_schema::SongTime::try_from_seconds_f64(f64::from_bits(
                row.position_seconds_bits,
            ))
            .filter(|frame| (0..window.end).contains(&frame.frames()))
            .ok_or("phase source lies outside the unfinished song")?
            .frames();
            let mut map = |nanos: u64| -> Result<[u64; 2], String> {
                if index == 0 {
                    return Ok([nanos; 2]);
                }
                let estimate = clock.estimate(nanos).map_err(|error| error.to_string())?;
                let midpoint = i128::from(nanos) + i128::from(estimate.offset_ns);
                let error = i128::from(estimate.uncertainty_ns);
                Ok([
                    u64::try_from(midpoint - error)
                        .map_err(|_| "phase time precedes host origin")?,
                    u64::try_from(midpoint + error).map_err(|_| "phase time overflow")?,
                ])
            };
            let lo = map(row.publication_before_ns)?;
            let hi = map(row.publication_after_ns)?;
            if hi[1] <= window.host_point_ns {
                before = Some(frame);
            }
            if after.is_none() && lo[0] >= window.host_point_ns {
                after = Some(frame);
            }
        }
        let collected_low = if index == 0 {
            rows.collected_at_ns
        } else {
            let estimate = clock
                .estimate(rows.collected_at_ns)
                .map_err(|error| error.to_string())?;
            u64::try_from(
                i128::from(rows.collected_at_ns) + i128::from(estimate.offset_ns)
                    - i128::from(estimate.uncertainty_ns),
            )
            .map_err(|_| "phase collection predates host origin")?
        };
        // Actual reliable receipt supplies causality; the lower mapping bounds maximum age
        if window
            .host_now_ns
            .checked_sub(collected_low)
            .is_none_or(|age| age > PHASE_DELIVERY_MAX_AGE_NS)
        {
            return Err("phase evidence delivery is stale or precedes collection".into());
        }
        intervals[index] = [
            before
                .ok_or("phase lacks a publication before the common point")?
                .checked_sub(1)
                .ok_or("phase quantization underflow")?,
            after
                .ok_or("phase lacks a publication after the common point")?
                .checked_add(1)
                .ok_or("phase quantization overflow")?,
        ];
        if intervals[index][0] > intervals[index][1] {
            return Err("phase source bracket moved backwards".into());
        }
    }
    Ok(PhaseBounds {
        epoch: window.epoch,
        round: window.round,
        host_point_ns: window.host_point_ns,
        host_received_ns: window.host_now_ns,
        stage: if window.reconnecting {
            crate::wire::PhaseProofStage::ReconnectVerification
        } else if window.verification {
            crate::wire::PhaseProofStage::Correction
        } else {
            crate::wire::PhaseProofStage::Check
        },
        source_frames: intervals,
        last_publications: last_publications
            .map(|row| row.expect("both source windows were validated")),
        guest_minus_host_frames: [
            intervals[1][0]
                .checked_sub(intervals[0][1])
                .ok_or("phase difference overflow")?,
            intervals[1][1]
                .checked_sub(intervals[0][0])
                .ok_or("phase difference overflow")?,
        ],
    })
}

/// Source-maintenance rounds never spend or reset the authenticated reconnect budget
#[derive(Default)]
pub(crate) struct PhaseRounds {
    round: u16,
    corrections: u8,
    // Fixed deadline, original epoch, expected common point, post-correction proof
    active: Option<(u64, SessionEpoch, u64, crate::wire::PhaseProofStage)>,
    sealed: Option<(u64, SessionEpoch, u64, crate::wire::PhaseProofStage)>,
    rebound: bool,
    last_now_ns: u64,
}

impl PhaseRounds {
    pub(crate) fn begin(
        &mut self,
        round: u16,
        epoch: SessionEpoch,
        host_point_ns: u64,
        now_ns: u64,
    ) -> Result<(), String> {
        if now_ns < self.last_now_ns
            || self.active.is_some()
            || round == 0
            || round > MAX_PHASE_ROUNDS
            || self.round.checked_add(1) != Some(round)
        {
            return Err("phase round is replayed, out of order or exhausted".into());
        }
        let deadline = now_ns
            .checked_add(30_000_000_000)
            .ok_or("phase deadline overflow")?;
        if host_point_ns <= now_ns || host_point_ns >= deadline {
            return Err("phase check point must be future and inside its fixed deadline".into());
        }
        self.round = round;
        self.active = Some((
            deadline,
            epoch,
            host_point_ns,
            crate::wire::PhaseProofStage::Check,
        ));
        self.last_now_ns = now_ns;
        Ok(())
    }

    fn check(
        &self,
        round: u16,
        now_ns: u64,
    ) -> Result<(u64, SessionEpoch, u64, crate::wire::PhaseProofStage), String> {
        let active = self.active.ok_or("no active source-maintenance round")?;
        if round != self.round || now_ns < self.last_now_ns || now_ns >= active.0 {
            return Err("phase round identity or fixed deadline differs".into());
        }
        Ok(active)
    }

    pub(crate) fn correct(
        &mut self,
        round: u16,
        verification_point_ns: u64,
        now_ns: u64,
    ) -> Result<(), String> {
        let (deadline, epoch, check_point, correcting) = self.check(round, now_ns)?;
        if correcting != crate::wire::PhaseProofStage::Check
            || self.corrections >= MAX_PHASE_CORRECTIONS
        {
            return Err("phase correction repeated or budget exhausted".into());
        }
        if verification_point_ns <= check_point
            || verification_point_ns <= now_ns
            || verification_point_ns >= deadline
        {
            return Err("phase verification must be later and inside the original deadline".into());
        }
        self.corrections += 1;
        self.active = Some((
            deadline,
            epoch,
            verification_point_ns,
            crate::wire::PhaseProofStage::Correction,
        ));
        self.last_now_ns = now_ns;
        Ok(())
    }

    pub(crate) fn rebind(
        &mut self,
        round: u16,
        epoch: SessionEpoch,
        host_point_ns: u64,
        host_deadline_ns: u64,
        now_ns: u64,
        corrections: u8,
    ) -> Result<(), String> {
        let original = self
            .active
            .or(self.sealed)
            .ok_or("no unresolved or last-sealed phase to rebind")?;
        if self.rebound
            || round != self.round
            || epoch != original.1
            || host_deadline_ns != original.0
            || now_ns < self.last_now_ns
            || now_ns >= original.0
            || host_point_ns <= now_ns
            || host_point_ns <= original.2
            || host_point_ns
                .checked_add(80_000_000)
                .is_none_or(|end| end >= original.0)
            || corrections < self.corrections
            || corrections > MAX_PHASE_CORRECTIONS
        {
            return Err(
                "phase rebind differs from its original identity, fixed deadline or spent budgets"
                    .into(),
            );
        }
        self.rebound = true;
        self.corrections = corrections;
        self.active = Some((
            original.0,
            epoch,
            host_point_ns,
            crate::wire::PhaseProofStage::ReconnectVerification,
        ));
        self.last_now_ns = now_ns;
        Ok(())
    }

    pub(crate) fn complete(
        &mut self,
        round: u16,
        now_ns: u64,
        verified: PhaseBounds,
    ) -> Result<(), String> {
        let (deadline, epoch, point, correcting) = self.check(round, now_ns)?;
        if verified.epoch != epoch
            || verified.round != round
            || verified.host_point_ns != point
            || verified.stage != correcting
            || verified.host_received_ns < point
            || verified.host_received_ns > now_ns
            || verified.host_received_ns >= deadline
        {
            return Err(
                "phase proof differs from the active epoch, round or verification point".into(),
            );
        }
        if !verified.within_guard() {
            return Err("source-maintenance cannot reopen input outside the phase guard".into());
        }
        self.sealed = self.active.take();
        self.last_now_ns = now_ns;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase_from_exchange(
        exchange: ClockExchange,
        evidence: &[crate::wire::PhaseEvidence; 2],
        window: PhaseWindow,
    ) -> Result<PhaseBounds, String> {
        let mut clock = ClockSync::new(exchange.epoch, ClockConfig::default()).unwrap();
        clock.observe(exchange).unwrap();
        source_phase_bounds(&mut clock, exchange, evidence, window)
    }

    fn phase_model(
        point: u64,
        source_origins: [u64; 2],
        paused_frames: [i64; 2],
        ppm: [i32; 2],
        path: (u64, u64),
    ) -> (ClockExchange, [crate::wire::PhaseEvidence; 2], PhaseWindow) {
        let sent = point - 500_000_000;
        let exchange = ClockExchange {
            epoch: SessionEpoch(9),
            guest_send_ns: sent,
            host_receive_ns: sent + path.0,
            host_send_ns: sent + path.0 + 2_000_000,
            guest_receive_ns: sent + path.0 + 2_000_000 + path.1,
        };
        let evidence = std::array::from_fn(|index| crate::wire::PhaseEvidence {
            generation: 11 + index as u64,
            source_id: 21 + index as u64,
            collected_at_ns: point + 170_000_000,
            publications: (0..64)
                .filter(|sample| point - 150_000_000 + sample * 5_000_000 >= source_origins[index])
                .map(|sample| {
                    let at = point - 150_000_000 + sample * 5_000_000;
                    let elapsed = at.checked_sub(source_origins[index]).unwrap();
                    // Independent integer oscillator, not a source-phase estimate
                    let progress =
                        i128::from(elapsed) * 48_000 * (1_000_000 + i128::from(ppm[index]))
                            / 1_000_000_000_000_000;
                    let frame = paused_frames[index] + i64::try_from(progress).unwrap();
                    crate::wire::PhasePublication {
                        sequence: at / 1_000_000 + 1,
                        position_seconds_bits: (frame as f64 / 48_000.0).to_bits(),
                        publication_before_ns: at - 50_000,
                        publication_after_ns: at + 50_000,
                    }
                })
                .collect(),
        });
        (
            exchange,
            evidence,
            PhaseWindow {
                epoch: SessionEpoch(9),
                round: 1,
                verification: false,
                attempt: 0,
                reconnecting: false,
                sources: [(11, 21), (12, 22)],
                previous: [None; 2],
                end: cocobeat_schema::MAX_CANONICAL_FRAMES as i64,
                host_point_ns: point,
                host_now_ns: point + 177_000_000,
            },
        )
    }

    #[test]
    fn phase_verification_reaches_the_actual_scheduled_frame_and_original_pause_floor() {
        let point = 20_000_000_000;
        let common_ns = point - 100_000_000;
        let common_frame = 24_000;
        let (exchange, evidence, mut window) = phase_model(
            point,
            [common_ns; 2],
            [common_frame; 2],
            [0; 2],
            (1_000_000, 1_000_000),
        );
        let paused = std::array::from_fn(|_| crate::wire::PhasePublication {
            sequence: (common_ns - 1_000_000_000) / 1_000_000 + 1,
            position_seconds_bits: (common_frame as f64 / 48_000.0).to_bits(),
            publication_before_ns: common_ns - 1_000_050_000,
            publication_after_ns: common_ns - 999_950_000,
        });
        window.verification = true;
        window.previous = paused.map(Some);
        let bounds = phase_from_exchange(exchange, &evidence, window).unwrap();
        assert!(bounds.supports_guard() && bounds.within_guard());
        bounds.verify_original_resume(paused, common_frame).unwrap();
        assert!(
            bounds
                .verify_original_resume(paused, common_frame + 10_000)
                .is_err(),
            "aligned sources alone do not prove arrival at the scheduled frame"
        );
        let newer_pause = bounds.last_publications;
        assert!(
            bounds
                .verify_original_resume(newer_pause, common_frame)
                .is_err(),
            "verification must really advance beyond the pause publications"
        );
        window.verification = false;
        let check = phase_from_exchange(exchange, &evidence, window).unwrap();
        assert!(check.verify_original_resume(paused, common_frame).is_err());
    }

    #[test]
    fn phase_six_hundred_second_song_bounds_independent_opposite_source_oscillators() {
        for ppm in [
            [100, -100],
            [-100, 100],
            [1000, -1000],
            [-1000, 1000],
            [0, 0],
        ] {
            let mut final_guard = false;
            // The fastest 600 s source reaches EOF before wall-time 600 s
            // Observe through 599 s; EOF must be rejected rather than fabricate ongoing Playing
            for second in 1..600 {
                let point = 3_000_000_000 + second * 1_000_000_000;
                let (exchange, evidence, window) = phase_model(
                    point,
                    [3_000_000_000; 2],
                    [0; 2],
                    ppm,
                    (1_000_000, 1_000_000),
                );
                let bounds = phase_from_exchange(exchange, &evidence, window).unwrap();
                let actual: [i64; 2] = ppm.map(|rate| {
                    i64::try_from(
                        i128::from(second) * 48_000 * (1_000_000 + i128::from(rate)) / 1_000_000,
                    )
                    .unwrap()
                });
                for (index, frame) in actual.iter().enumerate() {
                    assert!(
                        (bounds.source_frames[index][0]..=bounds.source_frames[index][1])
                            .contains(frame)
                    );
                }
                let difference = actual[1] - actual[0];
                assert!(
                    (bounds.guest_minus_host_frames[0]..=bounds.guest_minus_host_frames[1])
                        .contains(&difference)
                );
                final_guard = bounds.within_guard();
            }
            assert_eq!(final_guard, ppm == [0, 0]);
        }
    }

    #[test]
    fn phase_asymmetric_path_cannot_be_replaced_by_midpoint_precision() {
        let (exchange, evidence, window) = phase_model(
            10_000_000_000,
            [0; 2],
            [0; 2],
            [0; 2],
            (10_000_000, 90_000_000),
        );
        let bounds = phase_from_exchange(exchange, &evidence, window).unwrap();
        assert!(bounds.guest_minus_host_frames[0] <= 0 && bounds.guest_minus_host_frames[1] >= 0);
        assert!(
            !bounds.within_guard(),
            "50 ms path ambiguity alone cannot prove the phase guard"
        );
        assert!(
            !bounds.supports_guard(),
            "wide mapping must fail before an unprovable correction"
        );
        let short: [_; 2] = evidence.clone().map(|mut rows| {
            rows.publications.truncate(30);
            rows.collected_at_ns = rows.publications.last().unwrap().publication_after_ns;
            rows
        });
        assert!(
            phase_from_exchange(exchange, &short, window).is_err(),
            "missing whole-interval bracket cannot report synchronous sources"
        );
    }

    #[test]
    fn guest_deadline_mapping_keeps_check_and_rebound_raw_proofs_queryable() {
        for reconnecting in [false, true] {
            let (exchange, evidence, mut window) = phase_model(
                10_000_000_000,
                [0; 2],
                [0; 2],
                [0; 2],
                (1_000_000, 1_000_000),
            );
            window.attempt = u8::from(reconnecting);
            window.verification = reconnecting;
            window.reconnecting = reconnecting;
            let original = phase_from_exchange(exchange, &evidence, window).unwrap();
            let mut clock = ClockSync::new(exchange.epoch, ClockConfig::default()).unwrap();
            clock.observe(exchange).unwrap();
            let now = exchange.guest_receive_ns + 20_000_000;
            let earliest = clock
                .conservative_deadline(exchange.epoch, now + 30_000_000_000, now, 100_000_000)
                .unwrap();
            assert!(earliest > now);
            assert_eq!(clock.original_exchange(), Some(exchange));
            assert_eq!(
                source_phase_bounds(&mut clock, exchange, &evidence, window).unwrap(),
                original
            );
            assert!(clock.estimate(exchange.guest_receive_ns).is_err());
        }
    }

    #[test]
    fn phase_rejects_identity_replay_stale_loss_bad_cursor_and_missing_brackets() {
        let (exchange, evidence, window) = phase_model(
            10_000_000_000,
            [0; 2],
            [0; 2],
            [0; 2],
            (1_000_000, 1_000_000),
        );
        let valid = phase_from_exchange(exchange, &evidence, window).unwrap();
        let mut queried = ClockSync::new(exchange.epoch, ClockConfig::default()).unwrap();
        queried.observe(exchange).unwrap();
        queried.estimate(evidence[1].collected_at_ns).unwrap();
        assert!(
            source_phase_bounds(&mut queried, exchange, &evidence, window).is_err(),
            "phase must not clear last_query to retroactively query publications"
        );

        let cases: Vec<_> = (0..9)
            .map(|case| {
                let mut rows = evidence.clone();
                match case {
                    0 => rows[1].generation += 1,
                    1 => rows[1].source_id += 1,
                    2 => rows[1].publications[1].sequence = rows[1].publications[0].sequence,
                    3 => rows[1].publications[1].position_seconds_bits = f64::NAN.to_bits(),
                    4 => rows[1].publications[1].position_seconds_bits = (-1.0_f64).to_bits(),
                    5 => {
                        rows[1].publications[1].publication_before_ns =
                            rows[1].publications[0].publication_after_ns - 1
                    }
                    6 => rows[1].collected_at_ns += 60_000_000,
                    7 => {
                        rows[1].publications[0].publication_before_ns =
                            exchange.guest_receive_ns - 1
                    }
                    _ => rows[1].publications.clear(),
                }
                rows
            })
            .collect();
        for (index, rows) in cases.iter().enumerate() {
            assert!(
                phase_from_exchange(exchange, rows, window).is_err(),
                "accepted bad source case {index}"
            );
        }
        assert!(
            phase_from_exchange(
                exchange,
                &evidence,
                PhaseWindow {
                    previous: valid.last_publications.map(Some),
                    ..window
                }
            )
            .is_err()
        );
        assert!(
            phase_from_exchange(
                exchange,
                &evidence,
                PhaseWindow {
                    host_now_ns: window.host_now_ns + 300_000_000,
                    ..window
                }
            )
            .is_err()
        );
        assert!(
            phase_from_exchange(
                exchange,
                &evidence,
                PhaseWindow {
                    host_now_ns: window.host_point_ns - 1,
                    ..window
                }
            )
            .is_err()
        );
        let old_clock = ClockExchange {
            guest_send_ns: exchange.guest_send_ns - 3_000_000_000,
            guest_receive_ns: exchange.guest_receive_ns - 3_000_000_000,
            host_receive_ns: exchange.host_receive_ns - 3_000_000_000,
            host_send_ns: exchange.host_send_ns - 3_000_000_000,
            ..exchange
        };
        assert!(
            phase_from_exchange(old_clock, &evidence, window).is_err(),
            "lost maintenance cannot refresh ClockSync by copying source data"
        );
        let first = evidence[0].publications[0];
        let mut previous = first;
        previous.sequence -= 1;
        previous.publication_before_ns = first.publication_before_ns - 10;
        previous.publication_after_ns = first.publication_before_ns - 1;
        previous.position_seconds_bits =
            (f64::from_bits(first.position_seconds_bits) + 1.0).to_bits();
        assert!(
            phase_from_exchange(
                exchange,
                &evidence,
                PhaseWindow {
                    previous: [Some(previous), None],
                    ..window
                }
            )
            .is_err(),
            "higher sequence does not permit cross-round original cursor rewind"
        );
        previous.position_seconds_bits = first.position_seconds_bits;
        previous.publication_after_ns = first.publication_before_ns + 1;
        assert!(
            phase_from_exchange(
                exchange,
                &evidence,
                PhaseWindow {
                    previous: [Some(previous), None],
                    ..window
                }
            )
            .is_err(),
            "higher sequence does not permit cross-round publication overlap"
        );
        let mut eof = evidence.clone();
        eof[0]
            .publications
            .last_mut()
            .unwrap()
            .position_seconds_bits = (window.end as f64 / 48_000.0).to_bits();
        assert!(phase_from_exchange(exchange, &eof, window).is_err());
    }

    #[test]
    fn phase_two_same_source_natural_catchups_use_separate_bounded_rounds() {
        let mut budget = PhaseRounds::default();
        let mut previous = [None; 2];
        let mut origins = [59_000_000_000; 2];
        let mut source_frames = [100_000_i64, 104_000_i64];
        let rates = [1000, -1000];
        let progress = |elapsed: u64, ppm: i32| {
            i64::try_from(
                i128::from(elapsed) * 48_000 * (1_000_000 + i128::from(ppm))
                    / 1_000_000_000_000_000,
            )
            .unwrap()
        };
        for round in 1..=2 {
            let point = u64::from(round) * 60_000_000_000;
            let (exchange, rows, mut window) =
                phase_model(point, origins, source_frames, rates, (1_000_000, 1_000_000));
            window.previous = previous;
            window.round = round;
            let before = phase_from_exchange(exchange, &rows, window).unwrap();
            assert!(!before.within_guard());
            budget
                .begin(round, window.epoch, point, point - 500_000_000)
                .unwrap();
            let pause_ns = point + 200_000_000;
            let paused: [i64; 2] = std::array::from_fn(|index| {
                source_frames[index] + progress(pause_ns - origins[index], rates[index])
            });
            let common_frame = *paused.iter().max().unwrap();
            let common_ns = point + 3_000_000_000;
            let resume_ns =
                paused.map(|frame| common_ns - catchup_ns(common_frame, frame).unwrap());
            let verify_ns = common_ns + 300_000_000;
            budget.correct(round, verify_ns, pause_ns).unwrap();
            let (exchange, rows, mut window) =
                phase_model(verify_ns, resume_ns, paused, rates, (1_000_000, 1_000_000));
            window.previous = before.last_publications.map(Some);
            window.round = round;
            window.verification = true;
            let after = phase_from_exchange(exchange, &rows, window).unwrap();
            assert!(
                after.within_guard(),
                "same original source naturally advances, no seek or playback-rate fit"
            );
            for (index, range) in after.source_frames.iter().enumerate() {
                assert!(range[0] >= paused[index]);
                assert_eq!(
                    (rows[index].generation, rows[index].source_id),
                    window.sources[index]
                );
            }
            budget
                .complete(round, verify_ns + 200_000_000, after)
                .unwrap();
            previous = after.last_publications.map(Some);
            source_frames = std::array::from_fn(|index| {
                paused[index] + progress(verify_ns - resume_ns[index], rates[index])
            });
            origins = [verify_ns; 2];
            assert!(
                budget
                    .begin(
                        round,
                        window.epoch,
                        verify_ns + 300_000_000,
                        verify_ns + 201_000_000
                    )
                    .is_err()
            );
        }
        assert_eq!(budget.corrections, 2);
        assert_eq!(
            RESUME_ATTEMPT, 1,
            "source-maintenance rounds do not alter reconnect admission"
        );
    }

    #[test]
    fn phase_rounds_bind_original_proof_and_refuse_guard_deadline_order_and_budget_exhaustion() {
        let point = 10_000_000_000;
        let (exchange, evidence, window) =
            phase_model(point, [0; 2], [0; 2], [0; 2], (1_000_000, 1_000_000));
        let valid = phase_from_exchange(exchange, &evidence, window).unwrap();
        let begin = point - 500_000_000;
        let receipt = window.host_now_ns;
        let mut rounds = PhaseRounds::default();
        rounds.begin(1, window.epoch, point, begin).unwrap();
        assert!(rounds.begin(2, window.epoch, point, begin + 1).is_err());
        assert!(rounds.correct(2, point + 1_000_000_000, receipt).is_err());
        assert!(
            rounds
                .complete(
                    1,
                    receipt,
                    PhaseBounds {
                        guest_minus_host_frames: [-2401, 0],
                        ..valid
                    }
                )
                .is_err()
        );
        assert!(rounds.correct(1, point + 1_000_000_000, begin - 1).is_err());
        assert!(rounds.complete(1, begin + 30_000_000_000, valid).is_err());
        for bad in [
            PhaseWindow {
                epoch: SessionEpoch(10),
                ..window
            },
            PhaseWindow { round: 2, ..window },
            PhaseWindow {
                host_point_ns: point + 1_000_000,
                ..window
            },
            PhaseWindow {
                verification: true,
                ..window
            },
        ] {
            let mut ex = exchange;
            ex.epoch = bad.epoch;
            let proof = phase_from_exchange(ex, &evidence, bad).unwrap();
            assert!(rounds.complete(1, receipt, proof).is_err());
        }
        assert!(
            rounds.complete(1, receipt - 1, valid).is_err(),
            "proof cannot arrive in the caller's future"
        );
        rounds.complete(1, receipt, valid).unwrap();
        rounds
            .begin(2, window.epoch, point + 1_000_000_000, receipt)
            .unwrap();
        assert!(
            rounds.complete(2, receipt + 1, valid).is_err(),
            "old round proof cannot release a new round"
        );

        let mut rounds = PhaseRounds::default();
        rounds.begin(1, window.epoch, point, begin).unwrap();
        assert!(rounds.correct(1, point, receipt).is_err());
        assert!(rounds.correct(1, begin + 30_000_000_000, receipt).is_err());
        rounds.correct(1, point + 1_000_000_000, receipt).unwrap();
        assert!(
            rounds.complete(1, receipt + 1, valid).is_err(),
            "check proof cannot replace post-correction verification"
        );
        let (ex, rows, mut post) = phase_model(
            point + 1_000_000_000,
            [0; 2],
            [0; 2],
            [0; 2],
            (1_000_000, 1_000_000),
        );
        post.verification = true;
        let proof = phase_from_exchange(ex, &rows, post).unwrap();
        rounds.complete(1, post.host_now_ns, proof).unwrap();

        // Every budget fixture recomputes source bounds with this round's original metadata
        let mut rounds = PhaseRounds::default();
        for round in 1..=MAX_PHASE_ROUNDS {
            let point = 10_000_000_000 + u64::from(round) * 2_000_000_000;
            let now = point - 500_000_000;
            rounds.begin(round, window.epoch, point, now).unwrap();
            let correcting = round <= u16::from(MAX_PHASE_CORRECTIONS);
            if correcting {
                rounds
                    .correct(round, point + 500_000_000, point + 200_000_000)
                    .unwrap();
                assert!(
                    rounds
                        .correct(round, point + 600_000_000, point + 200_000_001)
                        .is_err()
                );
            } else {
                assert!(
                    rounds
                        .correct(round, point + 500_000_000, point + 200_000_000)
                        .is_err()
                );
            }
            let verification = if correcting {
                point + 500_000_000
            } else {
                point
            };
            let (ex, rows, mut fixture) =
                phase_model(verification, [0; 2], [0; 2], [0; 2], (1_000_000, 1_000_000));
            fixture.round = round;
            fixture.verification = correcting;
            let proof = phase_from_exchange(ex, &rows, fixture).unwrap();
            rounds.complete(round, fixture.host_now_ns, proof).unwrap();
        }
        assert!(
            rounds
                .begin(
                    MAX_PHASE_ROUNDS + 1,
                    window.epoch,
                    400_000_000_000,
                    399_000_000_000
                )
                .is_err()
        );
        assert_eq!(rounds.corrections, MAX_PHASE_CORRECTIONS);
    }

    #[test]
    fn phase_anchor_matches_original_four_timestamps_not_projected_query() {
        let (exchange, evidence, window) = phase_model(
            10_000_000_000,
            [0; 2],
            [0; 2],
            [0; 2],
            (1_000_000, 1_000_000),
        );
        let mut clock = ClockSync::new(exchange.epoch, ClockConfig::default()).unwrap();
        clock.observe(exchange).unwrap();
        assert_eq!(clock.original_exchange(), Some(exchange));
        for field in 0..4 {
            let mut forged = exchange;
            match field {
                0 => forged.guest_send_ns += 100_000_000,
                1 => forged.host_receive_ns += 100_000_000,
                2 => forged.host_send_ns += 100_000_000,
                _ => forged.guest_receive_ns += 100_000_000,
            }
            assert!(source_phase_bounds(&mut clock, forged, &evidence, window).is_err());
            assert_eq!(clock.original_exchange(), Some(exchange));
        }
        source_phase_bounds(&mut clock, exchange, &evidence, window).unwrap();
        assert_eq!(clock.original_exchange(), Some(exchange));
        assert!(
            source_phase_bounds(&mut clock, exchange, &evidence, window).is_err(),
            "the same original anchor does not bypass last_query"
        );
    }

    #[test]
    fn maintenance_packet_domain_is_fixed_strict_and_separate_from_admission() {
        let probe = MaintenancePacket {
            kind: 1,
            epoch: 9,
            round: MAX_MAINTENANCE_ROUNDS,
            guest_send_ns: 1_000,
            host_receive_ns: 0,
            host_send_ns: 0,
            guest_receive_ns: 0,
        };
        let bytes = probe.encode();
        assert_eq!(MaintenancePacket::decode(&bytes), Some(probe));
        assert_eq!(ClockPacket::decode(&bytes), None);
        assert_eq!(MaintenancePacket::decode(&bytes[..55]), None);
        assert_eq!(MaintenancePacket::decode(&[0; 57]), None);
        for index in [0, 4, 5, 6, 7, 32, 40, 48] {
            let mut bad = bytes;
            bad[index] ^= 0xff;
            assert_eq!(
                MaintenancePacket::decode(&bad),
                None,
                "accepted byte {index}"
            );
        }
        for round in [0, MAX_MAINTENANCE_ROUNDS + 1] {
            assert_eq!(
                MaintenancePacket::decode(&MaintenancePacket { round, ..probe }.encode()),
                None
            );
        }
        let reply = MaintenancePacket {
            kind: 2,
            host_receive_ns: 2_000,
            host_send_ns: 2_100,
            ..probe
        };
        assert_eq!(MaintenancePacket::decode(&reply.encode()), Some(reply));
        assert_eq!(
            MaintenancePacket::decode(
                &MaintenancePacket {
                    guest_receive_ns: 3_000,
                    ..reply
                }
                .encode()
            ),
            None
        );
        let confirmation = MaintenancePacket {
            kind: 3,
            guest_receive_ns: 3_000,
            ..reply
        };
        assert_eq!(
            MaintenancePacket::decode(&confirmation.encode()),
            Some(confirmation)
        );
    }

    #[test]
    fn maintenance_six_hundred_rounds_keep_four_actual_timestamps_and_asymmetric_bounds() {
        for ppm in [-1_000_i128, 0, 1_000] {
            let host_time = |guest: u64| {
                u64::try_from(50_000_000 + i128::from(guest) * (1_000_000 + ppm) / 1_000_000)
                    .unwrap()
            };
            for (outbound, inbound) in [(5_000_000, 5_000_000), (10_000_000, 90_000_000)] {
                let start = 1_000_000_000;
                let mut guest =
                    ClockMaintenance::new(SessionEpoch(9), PlayerId::P2, 0, start).unwrap();
                let mut host =
                    ClockMaintenance::new(SessionEpoch(9), PlayerId::P1, 0, host_time(start))
                        .unwrap();
                for round in 1..=600 {
                    let sent = start + (round - 1) * MAINTENANCE_PERIOD_NS;
                    let probe = guest.tick(sent).unwrap().unwrap();
                    let reply = host
                        .receive(
                            &probe,
                            host_time(sent + outbound),
                            host_time(sent + outbound + 2_000_000),
                        )
                        .unwrap();
                    assert!(reply.sample.is_none());
                    let received = sent + outbound + 2_000_000 + inbound;
                    let guest_result = guest
                        .receive(&reply.reply.unwrap(), received, received)
                        .unwrap();
                    let confirmation = guest_result.reply.unwrap();
                    let host_result = host
                        .receive(
                            &confirmation,
                            host_time(received + outbound),
                            host_time(received + outbound),
                        )
                        .unwrap();
                    assert!(host_result.reply.is_none());
                    let guest_sample = guest_result.sample.unwrap();
                    let host_sample = host_result.sample.unwrap();
                    assert_eq!(guest_sample.round, round);
                    assert_eq!(host_sample.round, round);
                    assert_eq!(guest_sample.exchange, host_sample.exchange);
                    assert_eq!(guest_sample.estimate, host_sample.estimate);
                    assert_eq!(guest_sample.exchange.guest_receive_ns, received);
                    assert_eq!(
                        guest_sample.exchange.host_send_ns,
                        host_time(sent + outbound + 2_000_000)
                    );
                    let query = received + 400_000_000;
                    for clock in [&mut guest.clock, &mut host.clock] {
                        let estimate = clock.estimate(query).unwrap();
                        let actual_offset = i128::from(host_time(query)) - i128::from(query);
                        assert!(
                            (i128::from(estimate.offset_ns) - actual_offset).unsigned_abs()
                                <= u128::from(estimate.uncertainty_ns)
                        );
                    }
                    assert_eq!(guest.round(), round);
                    assert_eq!(host.round(), round);
                }
                let sample_ns =
                    start + 599 * MAINTENANCE_PERIOD_NS + outbound + 2_000_000 + inbound;
                assert_eq!(
                    guest.clock.estimate(sample_ns + 2_000_000_001),
                    Err(crate::clock::ClockError::Stale)
                );
                assert_eq!(
                    host.clock.estimate(sample_ns + 2_000_000_001),
                    Err(crate::clock::ClockError::Stale)
                );
            }
        }
    }

    #[test]
    fn maintenance_replay_loss_identity_and_monotonic_fail_without_fabricating_freshness() {
        let start = 1_000_000_000;
        let mut guest = ClockMaintenance::new(SessionEpoch(9), PlayerId::P2, 0, start).unwrap();
        let mut host = ClockMaintenance::new(SessionEpoch(9), PlayerId::P1, 0, start).unwrap();
        assert_eq!(
            guest.clock.estimate(start),
            Err(crate::clock::ClockError::Uncalibrated)
        );
        let probe = guest.tick(start).unwrap().unwrap();
        let reply = host
            .receive(&probe, start + 10, start + 20)
            .unwrap()
            .reply
            .unwrap();
        assert!(
            host.receive(&probe, start + 21, start + 21)
                .unwrap()
                .reply
                .is_none()
        );
        let mut bad_reply = MaintenancePacket::decode(&reply).unwrap();
        bad_reply.guest_send_ns += 1;
        assert!(
            guest
                .receive(&bad_reply.encode(), start + 30, start + 30)
                .unwrap()
                .sample
                .is_none()
        );
        let confirmed = guest.receive(&reply, start + 31, start + 31).unwrap();
        let confirmation = confirmed.reply.unwrap();
        assert!(
            guest
                .receive(&reply, start + 32, start + 32)
                .unwrap()
                .sample
                .is_none()
        );
        let mut bad_confirmation = MaintenancePacket::decode(&confirmation).unwrap();
        bad_confirmation.host_send_ns += 1;
        assert!(
            host.receive(&bad_confirmation.encode(), start + 40, start + 40)
                .unwrap()
                .sample
                .is_none()
        );
        assert_eq!(
            host.clock.estimate(start + 40),
            Err(crate::clock::ClockError::Uncalibrated)
        );
        let actual = host
            .receive(&confirmation, start + 41, start + 41)
            .unwrap()
            .sample
            .unwrap();
        assert_eq!(actual.exchange.guest_receive_ns, start + 31);
        assert!(
            host.receive(&confirmation, start + 42, start + 42)
                .unwrap()
                .sample
                .is_none()
        );
        assert!(
            host.receive(&probe, start + 43, start + 43)
                .unwrap()
                .sample
                .is_none()
        );
        assert!(guest.tick(start + 30).is_err());
        let mut wrong_epoch = MaintenancePacket::decode(&probe).unwrap();
        wrong_epoch.epoch += 1;
        wrong_epoch.round += 1;
        assert!(
            host.receive(
                &wrong_epoch.encode(),
                start + 500_000_000,
                start + 500_000_000
            )
            .is_err()
        );
        assert_eq!(host.round(), 1);
        assert!(host.tick(start + 2_000_000_011).is_err());
        // Lost confirmations leave the host Uncalibrated and cannot reset source age
        let mut loss = ClockMaintenance::new(SessionEpoch(9), PlayerId::P1, 0, start).unwrap();
        let mut probe = MaintenancePacket::decode(&probe).unwrap();
        for round in 1..=2 {
            probe.round = round;
            probe.guest_send_ns = start + (round - 1) * MAINTENANCE_PERIOD_NS;
            let received = probe.guest_send_ns + 10;
            assert!(
                loss.receive(&probe.encode(), received, received + 10)
                    .unwrap()
                    .sample
                    .is_none()
            );
        }
        assert_eq!(
            loss.clock.estimate(start + MAINTENANCE_PERIOD_NS),
            Err(crate::clock::ClockError::Uncalibrated)
        );
        assert!(loss.tick(start + 2_000_000_001).is_err());
    }

    #[test]
    fn maintenance_round_deadline_and_packet_budgets_are_independent_of_resume_attempt() {
        let start = 1_000_000_000;
        let mut guest = ClockMaintenance::new(
            SessionEpoch(9),
            PlayerId::P2,
            MAX_MAINTENANCE_ROUNDS - 1,
            start,
        )
        .unwrap();
        let mut host = ClockMaintenance::new(
            SessionEpoch(9),
            PlayerId::P1,
            MAX_MAINTENANCE_ROUNDS - 1,
            start,
        )
        .unwrap();
        let probe = guest.tick(start).unwrap().unwrap();
        let reply = host
            .receive(&probe, start + 10, start + 11)
            .unwrap()
            .reply
            .unwrap();
        let confirmation = guest
            .receive(&reply, start + 20, start + 20)
            .unwrap()
            .reply
            .unwrap();
        assert!(
            host.receive(&confirmation, start + 30, start + 30)
                .unwrap()
                .sample
                .is_some()
        );
        assert!(guest.tick(start + MAINTENANCE_PERIOD_NS).is_err());
        assert!(
            ClockMaintenance::new(SessionEpoch(9), PlayerId::P1, MAX_MAINTENANCE_ROUNDS, start)
                .is_err()
        );
        // Reconnect may retain distinct seen/sent floors after a lost probe
        let mut reconnected_host =
            ClockMaintenance::new(SessionEpoch(9), PlayerId::P1, 400, start).unwrap();
        let mut reconnected_guest =
            ClockMaintenance::new(SessionEpoch(9), PlayerId::P2, 401, start).unwrap();
        let probe = reconnected_guest.tick(start).unwrap().unwrap();
        assert!(
            reconnected_host
                .receive(&probe, start + 10, start + 11)
                .unwrap()
                .reply
                .is_some()
        );
        assert_eq!(reconnected_guest.round(), 402);
        assert_eq!(reconnected_host.round(), 402);
        assert_eq!(RESUME_ATTEMPT, 1);
        let mut deadline = ClockMaintenance::new(SessionEpoch(9), PlayerId::P2, 0, start).unwrap();
        let probe = deadline.tick(start).unwrap().unwrap();
        let mut reply = MaintenancePacket::decode(&probe).unwrap();
        reply.kind = 2;
        reply.host_receive_ns = start + 10;
        reply.host_send_ns = start + 11;
        assert!(
            deadline
                .receive(
                    &reply.encode(),
                    start + MAINTENANCE_DEADLINE_NS,
                    start + MAINTENANCE_DEADLINE_NS
                )
                .unwrap()
                .sample
                .is_none()
        );
        assert_eq!(
            deadline.clock.estimate(start + MAINTENANCE_DEADLINE_NS),
            Err(crate::clock::ClockError::Uncalibrated)
        );
        let mut bounded = ClockMaintenance::new(SessionEpoch(9), PlayerId::P1, 0, start).unwrap();
        for _ in 0..MAX_PACKETS {
            assert!(bounded.receive(&[], start, start).unwrap().sample.is_none());
        }
        assert!(bounded.receive(&[], start, start).is_err());
    }

    #[test]
    fn clock_datagrams_are_fixed_bounded_and_strict() {
        let probe = ClockPacket {
            kind: 1,
            epoch: u64::MAX,
            attempt: 0,
            id: 7,
            guest_send_ns: u64::MAX,
            host_receive_ns: 0,
            host_send_ns: 0,
        };
        let bytes = probe.encode();
        assert_eq!(ClockPacket::decode(&bytes), Some(probe));
        assert_eq!(ClockPacket::decode(&bytes[..47]), None);
        assert_eq!(ClockPacket::decode(&[0; 49]), None);
        for index in [0, 4, 5, 7, 40] {
            let mut bad = bytes;
            bad[index] ^= 0xff;
            assert_eq!(ClockPacket::decode(&bad), None);
        }
        assert_eq!(
            ClockPacket::decode(&ClockPacket { id: 8, ..probe }.encode()),
            None
        );
        let mut stale = bytes;
        stale[6] = 2;
        assert_eq!(ClockPacket::decode(&stale), None);
        assert_eq!(
            ClockPacket::decode(
                &ClockPacket {
                    attempt: 1,
                    ..probe
                }
                .encode()
            )
            .unwrap()
            .attempt,
            1
        );
        let reply = ClockPacket {
            kind: 2,
            host_receive_ns: u64::MAX - 1,
            host_send_ns: u64::MAX,
            ..probe
        };
        assert_eq!(ClockPacket::decode(&reply.encode()), Some(reply));
    }
    fn gate_fixture() -> (ClockSync, [crate::wire::GateEvidence; 2], GateWindow) {
        let mut clock = ClockSync::new(SessionEpoch(9), ClockConfig::default()).unwrap();
        clock
            .observe(ClockExchange {
                epoch: SessionEpoch(9),
                guest_send_ns: 1_000_000_000,
                host_receive_ns: 1_006_000_000,
                host_send_ns: 1_006_000_000,
                guest_receive_ns: 1_002_000_000,
            })
            .unwrap();
        let evidence = std::array::from_fn(|player| crate::wire::GateEvidence {
            generation: player as u64 + 10,
            source_id: player as u64 + 1,
            progress_sequence: 1,
            observations: [1_010_000_000_u64, 1_030_000_000]
                .into_iter()
                .enumerate()
                .map(|(index, guest_ns)| {
                    // Independent integer oscillator: 48k frames/s at an exact +5ms host offset
                    let host_ns = guest_ns + 5_000_000;
                    crate::wire::GateObservation {
                        sequence: index as u64 + 2,
                        frame: 14_800
                            + ((i128::from(host_ns) - 1_025_000_000) * 48_000 / 1_000_000_000)
                                as i64,
                        publication_before_ns: if player == 0 { host_ns } else { guest_ns },
                        publication_after_ns: if player == 0 { host_ns } else { guest_ns },
                    }
                })
                .collect(),
        });
        (
            clock,
            evidence,
            GateWindow {
                paused: [10_000, 9_500],
                sources: [(10, 1), (11, 2)],
                end: 48_000,
                common_frame: 10_000,
                host_verify_ns: 1_025_000_000,
            },
        )
    }

    #[test]
    fn recovery_gate_uses_complete_past_intervals_and_rejects_late_or_unknown_sources() {
        let (mut clock, evidence, window) = gate_fixture();
        assert_eq!(
            resume_gate(&mut clock, 1_002_000_000, &evidence, window).unwrap(),
            [[14_319, 15_281]; 2]
        );
        let (mut clock, mut evidence, window) = gate_fixture();
        for row in &mut evidence[1].observations {
            row.frame += 1_438;
        }
        assert!(
            resume_gate(&mut clock, 1_002_000_000, &evidence, window).is_ok(),
            "exact worst-case 2400 frame bound is inclusive"
        );
        let (mut clock, mut evidence, window) = gate_fixture();
        for row in &mut evidence[1].observations {
            row.frame += 1_439;
        }
        assert!(resume_gate(&mut clock, 1_002_000_000, &evidence, window).is_err());
        let (mut clock, mut evidence, window) = gate_fixture();
        for rows in &mut evidence {
            for row in &mut rows.observations {
                row.frame -= 3_000;
            }
        }
        assert!(
            resume_gate(&mut clock, 1_002_000_000, &evidence, window).is_err(),
            "both late and mutually close still violate the fixed point"
        );
        let (mut clock, mut evidence, window) = gate_fixture();
        evidence[0].source_id += 1;
        assert!(resume_gate(&mut clock, 1_002_000_000, &evidence, window).is_err());
    }

    #[test]
    fn recovery_guard_diagnostics_report_the_actual_brackets_and_fixed_bounds() {
        for (shift, deltas) in [(0, [0, 1_439]), (137, [0, 1_439]), (1_000, [-3_000; 2])] {
            let (mut clock, mut evidence, mut window) = gate_fixture();
            for player in 0..2 {
                window.paused[player] += shift;
                window.sources[player] = (
                    10 + shift as u64 + player as u64,
                    1 + shift as u64 + player as u64,
                );
                evidence[player].generation = window.sources[player].0;
                evidence[player].source_id = window.sources[player].1;
                for row in &mut evidence[player].observations {
                    row.frame += shift + deltas[player];
                }
            }
            window.common_frame += shift;
            window.end += shift;
            let error = resume_gate(&mut clock, 1_002_000_000, &evidence, window).unwrap_err();
            let diagnostic: serde_json::Value =
                serde_json::from_str(error.split_once("; diagnostics=").unwrap().1).unwrap();
            // These rows bracket the independent fixture oscillator's fixed past point
            let brackets = evidence.each_ref().map(|rows| {
                [
                    rows.observations[0].frame - 1,
                    rows.observations[1].frame + 1,
                ]
            });
            let expected = window.paused.iter().copied().max().unwrap() + 48_000 / 10;
            assert_eq!(diagnostic["intervals_frames"], serde_json::json!(brackets));
            assert_eq!(diagnostic["expected_frame"], expected);
            assert_eq!(diagnostic["low_frame"], expected - 2_400);
            assert_eq!(diagnostic["high_frame"], expected + 2_400);
            assert_eq!(
                diagnostic["paused_frames"],
                serde_json::json!(window.paused)
            );
            assert_eq!(diagnostic["common_frame"], window.common_frame);
            assert_eq!(diagnostic["host_verify_ns"], window.host_verify_ns);
            assert_eq!(
                diagnostic["sources_generation_id"],
                serde_json::json!(window.sources)
            );
            if deltas[1] == 1_439 {
                assert_eq!(brackets[1][1] - brackets[0][0], 2_401);
                assert!(
                    brackets
                        .iter()
                        .all(|range| range[0] >= expected - 2_400 && range[1] <= expected + 2_400)
                );
            } else {
                assert!(brackets.iter().all(|range| range[0] < expected - 2_400));
                assert!(brackets[0][1] - brackets[1][0] <= 2_400);
            }
        }
    }

    #[test]
    fn recovery_gate_rejects_middle_regressions_stale_samples_and_missing_brackets() {
        let (mut clock, mut evidence, window) = gate_fixture();
        evidence[1].observations.insert(
            1,
            crate::wire::GateObservation {
                sequence: 3,
                frame: 14_000,
                publication_before_ns: 1_020_000_000,
                publication_after_ns: 1_020_000_000,
            },
        );
        evidence[1].observations[2].sequence = 4;
        assert!(resume_gate(&mut clock, 1_002_000_000, &evidence, window).is_err());
        let (mut clock, mut evidence, window) = gate_fixture();
        evidence[1].observations[0].publication_before_ns = 1_001_000_000;
        evidence[1].observations[0].publication_after_ns = 1_001_000_000;
        assert!(
            resume_gate(&mut clock, 1_002_000_000, &evidence, window).is_err(),
            "pre-sample publication cannot be mapped backwards"
        );
        let (mut clock, evidence, window) = gate_fixture();
        clock.estimate(1_040_000_000).unwrap();
        assert!(
            resume_gate(&mut clock, 1_002_000_000, &evidence, window).is_err(),
            "gate clock must not query current time before publications"
        );
        let (mut clock, mut evidence, mut window) = gate_fixture();
        for rows in &mut evidence {
            for row in &mut rows.observations {
                row.publication_before_ns += 3_000_000_000;
                row.publication_after_ns += 3_000_000_000;
            }
        }
        window.host_verify_ns += 3_000_000_000;
        assert!(
            resume_gate(&mut clock, 1_002_000_000, &evidence, window).is_err(),
            "2s sample age is not relaxed"
        );
        let (mut clock, mut evidence, window) = gate_fixture();
        evidence[0].observations[0].frame = window.paused[0];
        assert!(
            resume_gate(&mut clock, 1_002_000_000, &evidence, window).is_err(),
            "samples from a paused segment cannot bracket an advancing segment"
        );
        assert_eq!(catchup_ns(10_000, 9_500).unwrap(), 10_416_667);
        assert!(catchup_ns(9_500, 10_000).is_err());
    }
    #[test]
    fn reconnect_rebind_keeps_round_deadline_budget_and_requires_new_proof_stage() {
        let point = 60_000_000_000;
        let begin = point - 500_000_000;
        let epoch = SessionEpoch(9);
        let mut rounds = PhaseRounds::default();
        rounds.begin(1, epoch, point, begin).unwrap();
        rounds
            .correct(1, point + 1_000_000_000, point + 200_000_000)
            .unwrap();
        let deadline = begin + 30_000_000_000;
        assert!(
            rounds
                .rebind(
                    1,
                    epoch,
                    point + 3_000_000_000,
                    deadline + 1,
                    point + 2_000_000_000,
                    1
                )
                .is_err()
        );
        assert!(
            rounds
                .rebind(
                    2,
                    epoch,
                    point + 3_000_000_000,
                    deadline,
                    point + 2_000_000_000,
                    1
                )
                .is_err()
        );
        rounds
            .rebind(
                1,
                epoch,
                point + 3_000_000_000,
                deadline,
                point + 2_000_000_000,
                1,
            )
            .unwrap();
        assert_eq!(rounds.corrections, 1);
        assert_eq!(rounds.round, 1);
        assert_eq!(rounds.active.unwrap().0, deadline);
        assert!(
            rounds
                .correct(1, point + 4_000_000_000, point + 2_000_000_001)
                .is_err()
        );
        assert!(
            rounds
                .rebind(
                    1,
                    epoch,
                    point + 4_000_000_000,
                    deadline,
                    point + 2_000_000_001,
                    1
                )
                .is_err()
        );
        let (exchange, rows, mut window) = phase_model(
            point + 3_000_000_000,
            [0; 2],
            [0; 2],
            [0; 2],
            (1_000_000, 1_000_000),
        );
        window.verification = true;
        let old_stage = phase_from_exchange(exchange, &rows, window).unwrap();
        assert!(rounds.complete(1, window.host_now_ns, old_stage).is_err());
        window.attempt = 1;
        window.reconnecting = true;
        let fresh = phase_from_exchange(exchange, &rows, window).unwrap();
        rounds.complete(1, window.host_now_ns, fresh).unwrap();
        assert!(
            rounds
                .rebind(
                    1,
                    epoch,
                    point + 5_000_000_000,
                    deadline,
                    point + 4_000_000_000,
                    1
                )
                .is_err()
        );
    }

    #[test]
    fn last_sealed_round_rebind_is_once_only_and_never_extends_original_deadline() {
        let point = 60_000_000_000;
        let (exchange, rows, window) =
            phase_model(point, [0; 2], [0; 2], [0; 2], (1_000_000, 1_000_000));
        let proof = phase_from_exchange(exchange, &rows, window).unwrap();
        let begin = point - 500_000_000;
        let mut rounds = PhaseRounds::default();
        rounds.begin(1, window.epoch, point, begin).unwrap();
        rounds.complete(1, window.host_now_ns, proof).unwrap();
        let deadline = begin + 30_000_000_000;
        assert!(
            rounds
                .rebind(
                    1,
                    window.epoch,
                    deadline - 40_000_000,
                    deadline,
                    deadline - 100_000_000,
                    0
                )
                .is_err()
        );
        rounds
            .rebind(
                1,
                window.epoch,
                point + 1_000_000_000,
                deadline,
                point + 500_000_000,
                0,
            )
            .unwrap();
        assert_eq!(rounds.active.unwrap().0, deadline);
        assert_eq!(rounds.corrections, 0);
    }

    #[test]
    fn reconnected_progress_binds_frozen_bits_actual_common_time_and_full_uncertainty() {
        let point = 60_000_000_000;
        let common = point - 1_000_000_000;
        let frame = 100_000;
        let (exchange, rows, mut window) = phase_model(
            point,
            [common; 2],
            [frame; 2],
            [0; 2],
            (1_000_000, 1_000_000),
        );
        let frozen = std::array::from_fn(|_| crate::wire::PhasePublication {
            sequence: 1,
            position_seconds_bits: (frame as f64 / 48_000.0).to_bits(),
            publication_before_ns: common - 1_000_000_000,
            publication_after_ns: common - 999_000_000,
        });
        window.attempt = 1;
        window.reconnecting = true;
        window.verification = true;
        window.previous = frozen.map(Some);
        let proof = phase_from_exchange(exchange, &rows, window).unwrap();
        proof
            .verify_reconnect_resume(frozen, frame, common, [0, 2_000_000])
            .unwrap();
        assert!(proof.verify_original_resume(frozen, frame).is_err());
        assert!(
            proof
                .verify_reconnect_resume(frozen, frame + 10_000, common, [0, 2_000_000])
                .is_err()
        );
        assert!(
            proof
                .verify_reconnect_resume(frozen, frame, point + 1, [0, 2_000_000])
                .is_err()
        );
        assert!(
            proof
                .verify_reconnect_resume(proof.last_publications, frame, common, [0, 2_000_000])
                .is_err()
        );
        assert!(
            proof
                .verify_reconnect_resume(frozen, frame, common, [0, 100_000_000])
                .is_err()
        );
        window.reconnecting = false;
        let old = phase_from_exchange(exchange, &rows, window).unwrap();
        assert!(
            old.verify_reconnect_resume(frozen, frame, common, [0, 2_000_000])
                .is_err()
        );
    }

    #[test]
    fn reconnect_retains_original_clock_and_round_but_disallows_old_sample_as_new_evidence() {
        let mut clock =
            ClockMaintenance::new(SessionEpoch(9), PlayerId::P2, 7, 1_000_000_000).unwrap();
        let original = ClockExchange {
            epoch: SessionEpoch(9),
            guest_send_ns: 1_000_000_000,
            host_receive_ns: 1_001_000_000,
            host_send_ns: 1_002_000_000,
            guest_receive_ns: 1_003_000_000,
        };
        clock.clock.observe(original).unwrap();
        clock.tick(1_100_000_000).unwrap();
        let floor = clock.round();
        clock.reconnect(2_000_000_000).unwrap();
        assert_eq!(clock.clock.original_exchange(), Some(original));
        assert_eq!(clock.round(), floor);
        assert!(clock.sample().is_none());
        let probe =
            MaintenancePacket::decode(&clock.tick(2_000_000_000).unwrap().unwrap()).unwrap();
        assert!(probe.round > floor);
        assert!(clock.reconnect(1_999_999_999).is_err());
    }
}
