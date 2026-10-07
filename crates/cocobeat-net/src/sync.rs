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

fn now_ns(origin: Instant) -> Result<u64, String> {
    u64::try_from(origin.elapsed().as_nanos())
        .map_err(|_| "process monotonic clock overflow".into())
}

fn local_instant(origin: Instant, nanos: u64) -> Result<Instant, String> {
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
    pub timing: NetworkTiming,
}

fn catchup_ns(common: i64, paused: i64) -> Result<u64, String> {
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
    paused: [i64; 2],
    end: i64,
) -> Result<ResumePlan, String> {
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
    let (host_common_ns, host_verify_ns, local_resume_ns, local_verify_ns, uncertainty) =
        if player == PlayerId::P1 {
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
            match control.recv().await? {
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
                }
                _ => return Err("resume schedule Ack identity or attempt differs".into()),
            }
            (
                common,
                verify,
                common
                    .checked_sub(delays[0])
                    .ok_or("host resume time overflow")?,
                verify,
                0,
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
        timing,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
