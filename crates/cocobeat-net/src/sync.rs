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
            || bytes[6..8] != [0, 0]
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
                        && probe.kind == 1 && probe.epoch == epoch.0
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
            id,
            guest_send_ns,
            host_receive_ns,
            host_send_ns,
            guest_receive_ns,
        } if actual == epoch.0 && id < MAX_PROBES => {
            if replies[id as usize]
                != Some(ClockPacket {
                    kind: 2,
                    epoch: actual,
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

/// Both clocks are process-local software clocks; no input/audio mapping occurs
pub(crate) async fn arm_start(
    connection: &Connection,
    control: &mut ControlIo,
    epoch: SessionEpoch,
    player: PlayerId,
    origin: Instant,
    timing: &mut NetworkTiming,
) -> Result<Instant, String> {
    let mut clock = tokio::time::timeout(CLOCK_TIMEOUT, async {
        if player == PlayerId::P1 {
            host_clock(connection, control, epoch, origin, timing).await
        } else {
            guest_clock(connection, control, epoch, origin, timing).await
        }
    })
    .await
    .map_err(|_| "clock probe phase timed out")??;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_datagrams_are_fixed_bounded_and_strict() {
        let probe = ClockPacket {
            kind: 1,
            epoch: u64::MAX,
            id: 7,
            guest_send_ns: u64::MAX,
            host_receive_ns: 0,
            host_send_ns: 0,
        };
        let bytes = probe.encode();
        assert_eq!(ClockPacket::decode(&bytes), Some(probe));
        assert_eq!(ClockPacket::decode(&bytes[..47]), None);
        assert_eq!(ClockPacket::decode(&[0; 49]), None);
        for index in [0, 4, 5, 6, 7, 40] {
            let mut bad = bytes;
            bad[index] ^= 0xff;
            assert_eq!(ClockPacket::decode(&bad), None);
        }
        assert_eq!(
            ClockPacket::decode(&ClockPacket { id: 8, ..probe }.encode()),
            None
        );
        let reply = ClockPacket {
            kind: 2,
            host_receive_ns: u64::MAX - 1,
            host_send_ns: u64::MAX,
            ..probe
        };
        assert_eq!(ClockPacket::decode(&reply.encode()), Some(reply));
    }
}
