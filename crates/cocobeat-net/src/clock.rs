//! Bounded host-minus-guest monotonic offsets, independent of audio/device clocks

use cocobeat_schema::SessionEpoch;

const MILLION: u128 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockConfig {
    pub max_sample_age_ns: u64,
    pub max_exchange_ns: u64,
    /// Assumed relative host/guest monotonic rate error, requiring measurement
    pub max_drift_ppm: u32,
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self {
            max_sample_age_ns: 2_000_000_000,
            max_exchange_ns: 1_000_000_000,
            max_drift_ppm: 1_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockExchange {
    pub epoch: SessionEpoch,
    pub guest_send_ns: u64,
    pub host_receive_ns: u64,
    pub host_send_ns: u64,
    pub guest_receive_ns: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockEstimate {
    pub epoch: SessionEpoch,
    pub guest_ns: u64,
    /// Host monotonic time minus guest monotonic time
    pub offset_ns: i64,
    pub uncertainty_ns: u64,
    /// Path RTT interval after excluding host processing and allowing rate error
    pub round_trip_min_ns: u64,
    pub round_trip_max_ns: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduledStart {
    pub epoch: SessionEpoch,
    pub guest_start_ns: u64,
    pub uncertainty_ns: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockError {
    InvalidConfig,
    WrongEpoch,
    NonMonotonic,
    InvalidExchange,
    ExchangeTooSlow,
    Uncalibrated,
    Stale,
    StartTooSoon,
    Overflow,
}

impl std::fmt::Display for ClockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "network clock: {self:?}")
    }
}

impl std::error::Error for ClockError {}

/// One fresh four-timestamp sample with explicit path-asymmetry uncertainty
///
/// No one-way latency symmetry is assumed; all bounds also assume monotonic clocks
/// stay within `max_drift_ppm`, and timestamp capture overhead is accounted for by
/// the caller's timestamps rather than interpreted as speaker-output calibration
#[derive(Debug)]
pub struct ClockSync {
    epoch: SessionEpoch,
    config: ClockConfig,
    sample: Option<(ClockExchange, ClockEstimate)>,
    last_query_ns: Option<u64>,
}

impl ClockSync {
    pub fn new(epoch: SessionEpoch, config: ClockConfig) -> Result<Self, ClockError> {
        if config.max_sample_age_ns == 0
            || config.max_exchange_ns == 0
            || u128::from(config.max_drift_ppm) >= MILLION
        {
            return Err(ClockError::InvalidConfig);
        }
        Ok(Self {
            epoch,
            config,
            sample: None,
            last_query_ns: None,
        })
    }

    /// Exchanges are sequential; the protocol must match each response to its probe
    pub fn observe(&mut self, exchange: ClockExchange) -> Result<ClockEstimate, ClockError> {
        if exchange.epoch != self.epoch {
            return Err(ClockError::WrongEpoch);
        }
        let guest_elapsed = exchange
            .guest_receive_ns
            .checked_sub(exchange.guest_send_ns)
            .ok_or(ClockError::NonMonotonic)?;
        let host_elapsed = exchange
            .host_send_ns
            .checked_sub(exchange.host_receive_ns)
            .ok_or(ClockError::NonMonotonic)?;
        if self
            .last_query_ns
            .is_some_and(|last| exchange.guest_receive_ns < last)
            || self.sample.is_some_and(|(last, _)| {
                exchange.guest_send_ns < last.guest_receive_ns
                    || exchange.guest_receive_ns <= last.guest_receive_ns
                    || exchange.host_receive_ns < last.host_send_ns
            })
        {
            return Err(ClockError::NonMonotonic);
        }
        if guest_elapsed > self.config.max_exchange_ns {
            return Err(ClockError::ExchangeTooSlow);
        }
        let drift = drift_bound(guest_elapsed.max(host_elapsed), self.config.max_drift_ppm)?;
        let path = i128::from(guest_elapsed) - i128::from(host_elapsed);
        let rtt_max = path + i128::from(drift);
        if rtt_max < 0 {
            return Err(ClockError::InvalidExchange);
        }
        let lower = i128::from(exchange.host_send_ns)
            - i128::from(exchange.guest_receive_ns)
            - i128::from(drift);
        let upper = i128::from(exchange.host_receive_ns) - i128::from(exchange.guest_send_ns)
            + i128::from(drift);
        i64::try_from(lower).map_err(|_| ClockError::Overflow)?;
        i64::try_from(upper).map_err(|_| ClockError::Overflow)?;
        let width = u128::try_from(upper - lower).map_err(|_| ClockError::InvalidExchange)?;
        let estimate = ClockEstimate {
            epoch: self.epoch,
            guest_ns: exchange.guest_receive_ns,
            offset_ns: i64::try_from(lower + (width / 2) as i128)
                .map_err(|_| ClockError::Overflow)?,
            uncertainty_ns: u64::try_from(width.div_ceil(2)).map_err(|_| ClockError::Overflow)?,
            round_trip_min_ns: u64::try_from((path - i128::from(drift)).max(0))
                .map_err(|_| ClockError::Overflow)?,
            round_trip_max_ns: u64::try_from(rtt_max).map_err(|_| ClockError::Overflow)?,
        };
        self.sample = Some((exchange, estimate));
        Ok(estimate)
    }

    /// Original accepted four timestamps, without projecting or changing query order
    pub(crate) fn original_exchange(&self) -> Option<ClockExchange> {
        self.sample.map(|(exchange, _)| exchange)
    }

    pub fn estimate(&mut self, guest_now_ns: u64) -> Result<ClockEstimate, ClockError> {
        let estimate = self.project(guest_now_ns)?;
        self.last_query_ns = Some(guest_now_ns);
        Ok(estimate)
    }

    /// Require the entire possible start interval to leave the requested arm lead
    ///
    /// The interval includes drift until the deadline, not only sample age
    pub fn schedule_start(
        &mut self,
        epoch: SessionEpoch,
        host_start_ns: u64,
        guest_now_ns: u64,
        minimum_lead_ns: u64,
    ) -> Result<ScheduledStart, ClockError> {
        let scheduled = self.project_start(epoch, host_start_ns, guest_now_ns, minimum_lead_ns)?;
        self.last_query_ns = Some(guest_now_ns);
        Ok(scheduled)
    }

    /// Map an upper deadline to its earliest possible guest boundary without querying
    /// Source proof still consumes the accepted anchor and raw publications in order
    pub(crate) fn conservative_deadline(
        &self,
        epoch: SessionEpoch,
        host_deadline_ns: u64,
        guest_now_ns: u64,
        minimum_lead_ns: u64,
    ) -> Result<u64, ClockError> {
        let mapped = self.project_start(epoch, host_deadline_ns, guest_now_ns, minimum_lead_ns)?;
        mapped
            .guest_start_ns
            .checked_sub(mapped.uncertainty_ns)
            .ok_or(ClockError::StartTooSoon)
    }

    fn project_start(
        &self,
        epoch: SessionEpoch,
        host_start_ns: u64,
        guest_now_ns: u64,
        minimum_lead_ns: u64,
    ) -> Result<ScheduledStart, ClockError> {
        if epoch != self.epoch {
            return Err(ClockError::WrongEpoch);
        }
        let estimate = self.project(guest_now_ns)?;
        let guest_start_ns =
            u64::try_from(i128::from(host_start_ns) - i128::from(estimate.offset_ns))
                .map_err(|_| ClockError::Overflow)?;
        let horizon = guest_start_ns
            .checked_sub(guest_now_ns)
            .ok_or(ClockError::StartTooSoon)?;
        // Solve u >= sample_error + (horizon + u) * relative_drift
        let uncertainty = (u128::from(estimate.uncertainty_ns) * MILLION
            + u128::from(horizon) * u128::from(self.config.max_drift_ppm))
        .div_ceil(MILLION - u128::from(self.config.max_drift_ppm));
        let uncertainty_ns = u64::try_from(uncertainty).map_err(|_| ClockError::Overflow)?;
        let earliest = guest_start_ns
            .checked_sub(uncertainty_ns)
            .ok_or(ClockError::StartTooSoon)?;
        guest_start_ns
            .checked_add(uncertainty_ns)
            .ok_or(ClockError::Overflow)?;
        let ready_at = guest_now_ns
            .checked_add(minimum_lead_ns)
            .ok_or(ClockError::Overflow)?;
        if earliest <= guest_now_ns || earliest < ready_at {
            return Err(ClockError::StartTooSoon);
        }
        Ok(ScheduledStart {
            epoch,
            guest_start_ns,
            uncertainty_ns,
        })
    }

    fn project(&self, guest_now_ns: u64) -> Result<ClockEstimate, ClockError> {
        let (_, mut estimate) = self.sample.ok_or(ClockError::Uncalibrated)?;
        if self.last_query_ns.is_some_and(|last| guest_now_ns < last) {
            return Err(ClockError::NonMonotonic);
        }
        let age = guest_now_ns
            .checked_sub(estimate.guest_ns)
            .ok_or(ClockError::NonMonotonic)?;
        if age > self.config.max_sample_age_ns {
            return Err(ClockError::Stale);
        }
        estimate.guest_ns = guest_now_ns;
        estimate.uncertainty_ns = estimate
            .uncertainty_ns
            .checked_add(drift_bound(age, self.config.max_drift_ppm)?)
            .ok_or(ClockError::Overflow)?;
        Ok(estimate)
    }
}

fn drift_bound(elapsed_ns: u64, ppm: u32) -> Result<u64, ClockError> {
    u64::try_from((u128::from(elapsed_ns) * u128::from(ppm)).div_ceil(MILLION))
        .map_err(|_| ClockError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(ppm: u32) -> ClockSync {
        ClockSync::new(
            SessionEpoch(7),
            ClockConfig {
                max_drift_ppm: ppm,
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn exchange(offset: i64, outbound: u64, processing: u64, inbound: u64) -> ClockExchange {
        let guest_send_ns = 1_000_000_000;
        let host_receive_ns =
            u64::try_from(i128::from(guest_send_ns + outbound) + i128::from(offset)).unwrap();
        ClockExchange {
            epoch: SessionEpoch(7),
            guest_send_ns,
            host_receive_ns,
            host_send_ns: host_receive_ns + processing,
            guest_receive_ns: guest_send_ns + outbound + processing + inbound,
        }
    }

    #[test]
    fn asymmetric_paths_bound_offsets_without_claiming_midpoint_accuracy() {
        for offset in [-500_000_000, 0, 500_000_000] {
            for (outbound, inbound) in [
                (0, 0),
                (1, 2),
                (10_000_000, 90_000_000),
                (90_000_000, 10_000_000),
            ] {
                let mut clock = clock(0);
                let sample = exchange(offset, outbound, 5_000_000, inbound);
                let observed = clock.observe(sample).unwrap();
                assert_eq!(observed.round_trip_min_ns, outbound + inbound);
                assert_eq!(observed.round_trip_max_ns, outbound + inbound);
                assert!(
                    (i128::from(observed.offset_ns) - i128::from(offset)).unsigned_abs()
                        <= u128::from(observed.uncertainty_ns)
                );
                let scheduled = clock
                    .schedule_start(
                        SessionEpoch(7),
                        3_000_000_000,
                        sample.guest_receive_ns,
                        100_000_000,
                    )
                    .unwrap();
                let actual = 3_000_000_000_i128 - i128::from(offset);
                assert!(
                    (i128::from(scheduled.guest_start_ns) - actual).unsigned_abs()
                        <= u128::from(scheduled.uncertainty_ns)
                );
            }
        }
    }

    #[test]
    fn sample_age_and_deadline_horizon_both_expand_drift_bounds() {
        // Independent affine oscillators retain known true offsets and start times
        for ppm in [-1_000_i128, -100, 0, 100, 1_000] {
            let host = |guest: u64| {
                u64::try_from(5_000_000_000 + i128::from(guest) * (1_000_000 + ppm) / 1_000_000)
                    .unwrap()
            };
            for (outbound, inbound) in [(0, 0), (10_000_000, 90_000_000)] {
                let sample = ClockExchange {
                    epoch: SessionEpoch(7),
                    guest_send_ns: 1_000_000_000,
                    host_receive_ns: host(1_000_000_000 + outbound),
                    host_send_ns: host(1_005_000_000 + outbound),
                    guest_receive_ns: 1_005_000_000 + outbound + inbound,
                };
                let mut sync = clock(1_000);
                sync.observe(sample).unwrap();
                let now = sample.guest_receive_ns + 600_000_000;
                let estimated = sync.estimate(now).unwrap();
                let true_offset = i128::from(host(now)) - i128::from(now);
                assert!(
                    (i128::from(estimated.offset_ns) - true_offset).unsigned_abs()
                        <= u128::from(estimated.uncertainty_ns)
                );
                let true_start = now + 2_000_000_000;
                let scheduled = sync
                    .schedule_start(SessionEpoch(7), host(true_start), now, 100_000_000)
                    .unwrap();
                assert!(scheduled.guest_start_ns.abs_diff(true_start) <= scheduled.uncertainty_ns);
            }
        }
        let mut clock = clock(1_000);
        let sample = exchange(0, 10_000_000, 5_000_000, 10_000_000);
        let observed = clock.observe(sample).unwrap();
        let now = sample.guest_receive_ns + 1_000_000_000;
        let projected = clock.estimate(now).unwrap();
        assert_eq!(
            projected.uncertainty_ns,
            observed.uncertainty_ns + 1_000_000
        );
        let scheduled = clock
            .schedule_start(SessionEpoch(7), now + 2_000_000_000, now, 100_000_000)
            .unwrap();
        assert!(scheduled.uncertainty_ns >= projected.uncertainty_ns + 2_000_000);
        assert_eq!(
            clock.estimate(sample.guest_receive_ns + 2_000_000_001),
            Err(ClockError::Stale)
        );
    }

    #[test]
    fn deadline_mapping_preserves_the_anchor_query_floor_and_full_horizon_bounds() {
        let mut clock = clock(1_000);
        let sample = exchange(200_000_000, 10_000_000, 5_000_000, 90_000_000);
        let observed = clock.observe(sample).unwrap();
        let now = sample.guest_receive_ns + 600_000_000;
        let host_deadline = now + 30_000_000_000;
        let earliest = clock
            .conservative_deadline(SessionEpoch(7), host_deadline, now, 100_000_000)
            .unwrap();
        assert_eq!(clock.last_query_ns, None);
        assert_eq!(clock.estimate(sample.guest_receive_ns).unwrap(), observed);
        assert_eq!(clock.original_exchange(), Some(sample));
        let floor = clock.last_query_ns;
        assert_eq!(
            clock.conservative_deadline(SessionEpoch(8), host_deadline, now, 100_000_000),
            Err(ClockError::WrongEpoch)
        );
        assert_eq!(
            clock.conservative_deadline(
                SessionEpoch(7),
                host_deadline,
                sample.guest_receive_ns - 1,
                0
            ),
            Err(ClockError::NonMonotonic)
        );
        assert_eq!(
            clock.conservative_deadline(
                SessionEpoch(7),
                host_deadline,
                sample.guest_receive_ns + 2_000_000_001,
                0
            ),
            Err(ClockError::Stale)
        );
        assert_eq!(clock.last_query_ns, floor);
        let scheduled = clock
            .schedule_start(SessionEpoch(7), host_deadline, now, 100_000_000)
            .unwrap();
        assert_eq!(
            earliest,
            scheduled.guest_start_ns - scheduled.uncertainty_ns
        );
        assert!(scheduled.uncertainty_ns >= observed.uncertainty_ns + 30_000_000);
        assert_eq!(clock.last_query_ns, Some(now));
        assert_eq!(
            clock.estimate(sample.guest_receive_ns),
            Err(ClockError::NonMonotonic)
        );
    }

    #[test]
    fn invalid_exchanges_and_queries_preserve_the_last_valid_sample() {
        let mut clock = clock(0);
        assert_eq!(clock.estimate(0), Err(ClockError::Uncalibrated));
        let sample = exchange(0, 10, 5, 10);
        let saved = clock.observe(sample).unwrap();
        for (bad, expected) in [
            (
                ClockExchange {
                    epoch: SessionEpoch(8),
                    ..sample
                },
                ClockError::WrongEpoch,
            ),
            (
                ClockExchange {
                    guest_receive_ns: sample.guest_send_ns - 1,
                    ..sample
                },
                ClockError::NonMonotonic,
            ),
            (
                ClockExchange {
                    host_send_ns: sample.host_receive_ns - 1,
                    ..sample
                },
                ClockError::NonMonotonic,
            ),
            (sample, ClockError::NonMonotonic),
        ] {
            assert_eq!(clock.observe(bad), Err(expected));
            assert_eq!(clock.estimate(sample.guest_receive_ns).unwrap(), saved);
        }
        assert_eq!(
            clock.estimate(sample.guest_receive_ns - 1),
            Err(ClockError::NonMonotonic)
        );
        assert_eq!(
            clock.schedule_start(SessionEpoch(8), 2_000_000_000, sample.guest_receive_ns, 0),
            Err(ClockError::WrongEpoch)
        );
        assert_eq!(
            clock.schedule_start(
                SessionEpoch(7),
                sample.guest_receive_ns + 10,
                sample.guest_receive_ns,
                1
            ),
            Err(ClockError::StartTooSoon)
        );
        assert_eq!(clock.estimate(sample.guest_receive_ns).unwrap(), saved);
    }

    #[test]
    fn capacity_overflow_and_impossible_processing_are_rejected() {
        for config in [
            ClockConfig {
                max_sample_age_ns: 0,
                ..Default::default()
            },
            ClockConfig {
                max_exchange_ns: 0,
                ..Default::default()
            },
            ClockConfig {
                max_drift_ppm: 1_000_000,
                ..Default::default()
            },
        ] {
            assert_eq!(
                ClockSync::new(SessionEpoch(7), config).unwrap_err(),
                ClockError::InvalidConfig
            );
        }
        let sample = exchange(0, 0, 0, 0);
        assert_eq!(
            clock(0).observe(ClockExchange {
                guest_receive_ns: sample.guest_send_ns + 1_000_000_001,
                ..sample
            }),
            Err(ClockError::ExchangeTooSlow)
        );
        assert_eq!(
            clock(0).observe(ClockExchange {
                host_send_ns: sample.host_send_ns + 1,
                ..sample
            }),
            Err(ClockError::InvalidExchange)
        );
        assert_eq!(
            clock(0).observe(ClockExchange {
                host_receive_ns: u64::MAX,
                host_send_ns: u64::MAX,
                ..sample
            }),
            Err(ClockError::Overflow)
        );
        let mut clock = clock(0);
        let sample = exchange(-1_000, 0, 0, 0);
        clock.observe(sample).unwrap();
        assert_eq!(
            clock.schedule_start(SessionEpoch(7), u64::MAX, sample.guest_receive_ns, 0),
            Err(ClockError::Overflow)
        );
        assert_eq!(
            clock.schedule_start(
                SessionEpoch(7),
                2_000_000_000,
                sample.guest_receive_ns,
                u64::MAX
            ),
            Err(ClockError::Overflow)
        );
    }
}
