//! Audio-position observations mapped onto a monotonic clock, independently of rendering

use std::collections::VecDeque;

use cocobeat_schema::{CANONICAL_SAMPLE_RATE, SessionEpoch, SongTime};

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Nanoseconds from one process-local monotonic origin, never a wall-clock timestamp
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct MonotonicTime(u64);

impl MonotonicTime {
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    pub const fn nanos(self) -> u64 {
        self.0
    }
}

/// Audio frames since a playback origin, at the device's declared sample rate
///
/// A frame contains every channel at one sample instant
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct DeviceTime(u64);

impl DeviceTime {
    pub const fn from_frames(frames: u64) -> Self {
        Self(frames)
    }

    pub const fn frames(self) -> u64 {
        self.0
    }

    /// Floors to canonical frames; the caller accounts for this quantization in observations
    ///
    /// This only converts a playback-relative count, not an arbitrary device-clock origin
    pub fn to_song_time(self, sample_rate: u32) -> Option<SongTime> {
        let frames = u128::from(self.0)
            .checked_mul(u128::from(CANONICAL_SAMPLE_RATE))?
            .checked_div(u128::from(sample_rate))?;
        i64::try_from(frames).ok().map(SongTime::from_frames)
    }
}

/// A playback-position sample, retaining its original timestamp and uncertainty
///
/// `song_time` must come from audio playback, never elapsed render time
/// `uncertainty_frames` bounds observation, timestamp, and sample-rate-conversion error
/// Physical output latency is outside this model unless measured into that bound
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockObservation {
    pub epoch: SessionEpoch,
    pub monotonic: MonotonicTime,
    pub song_time: SongTime,
    pub uncertainty_frames: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockEstimate {
    pub epoch: SessionEpoch,
    pub monotonic: MonotonicTime,
    pub song_time: SongTime,
    pub uncertainty_frames: u64,
}

/// Bounds are assumptions to validate against measurements, not hardware accuracy claims
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockConfig {
    pub max_extrapolation_ns: u64,
    pub max_drift_ppm: u32,
    pub history_capacity: usize,
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self {
            max_extrapolation_ns: 250_000_000,
            max_drift_ppm: 1_000,
            history_capacity: 256,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockState {
    Uncalibrated,
    Running,
    Paused,
    DeviceLost,
    Expired,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockError {
    InvalidConfig,
    WrongEpoch,
    NonMonotonicObservation,
    NonMonotonicQuery,
    InvalidTransition,
    Uncalibrated,
    Paused,
    DeviceLost,
    CalibrationExpired,
    Discontinuity,
    OutsideHistory,
    Overflow,
}

impl std::fmt::Display for ClockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "clock bridge: {self:?}")
    }
}

impl std::error::Error for ClockError {}

#[derive(Clone, Copy, Debug)]
struct Anchor {
    observation: ClockObservation,
    end: Option<MonotonicTime>,
}

/// A bounded piecewise map anchored only by observed audio positions
///
/// Each anchor uses the nominal 48 kHz rate, with an error bound of the observation
/// error + ceil(elapsed nominal frames * max_drift_ppm / 1,000,000) + one frame
/// The bound holds only while playback progresses within the configured drift limit
/// Unobserved stalls/device changes cannot be detected before another observation
///
/// Capture queries use a recorded anchor at capture time; callers retain the returned
/// estimate alongside the raw input instead of rewriting facts when more samples arrive
/// `now` additionally clamps corrections to its last result and increases uncertainty
#[derive(Debug)]
pub struct ClockBridge {
    epoch: SessionEpoch,
    config: ClockConfig,
    state: ClockState,
    history: VecDeque<Anchor>,
    transition_at: MonotonicTime,
    last_now: Option<ClockEstimate>,
}

impl ClockBridge {
    pub fn new(epoch: SessionEpoch, config: ClockConfig) -> Result<Self, ClockError> {
        if config.max_extrapolation_ns == 0
            || config.max_drift_ppm >= 1_000_000
            || config.history_capacity == 0
        {
            return Err(ClockError::InvalidConfig);
        }
        Ok(Self {
            epoch,
            config,
            state: ClockState::Uncalibrated,
            history: VecDeque::new(),
            transition_at: MonotonicTime::default(),
            last_now: None,
        })
    }

    pub const fn epoch(&self) -> SessionEpoch {
        self.epoch
    }

    /// Expiration is evaluated by `now`; a state alone does not certify a future query
    pub const fn state(&self) -> ClockState {
        self.state
    }

    pub fn last_observation(&self) -> Option<ClockObservation> {
        self.history.back().map(|anchor| anchor.observation)
    }

    pub fn observe(&mut self, observation: ClockObservation) -> Result<(), ClockError> {
        if observation.epoch != self.epoch {
            return Err(ClockError::WrongEpoch);
        }
        self.require_observable()?;
        observation
            .uncertainty_frames
            .checked_add(1)
            .ok_or(ClockError::Overflow)?;
        if observation.monotonic < self.transition_at
            || self
                .last_observation()
                .is_some_and(|last| observation.monotonic <= last.monotonic)
        {
            return Err(ClockError::NonMonotonicObservation);
        }
        // Fresh observations after an expired interval re-anchor the map; they do not
        // retroactively certify the unobserved gap
        if let Some(last) = self.history.back().filter(|anchor| anchor.end.is_none()) {
            match self.project(last.observation, observation.monotonic) {
                Ok(predicted) => {
                    let difference = (i128::from(predicted.song_time.frames())
                        - i128::from(observation.song_time.frames()))
                    .unsigned_abs();
                    let bound = u128::from(predicted.uncertainty_frames)
                        + u128::from(observation.uncertainty_frames);
                    if difference > bound {
                        self.close_anchor(observation.monotonic);
                        self.state = ClockState::Invalidated;
                        self.transition_at = observation.monotonic;
                        return Err(ClockError::Discontinuity);
                    }
                }
                Err(ClockError::CalibrationExpired) => {}
                Err(error) => return Err(error),
            }
        }
        self.close_anchor(observation.monotonic);
        if self.history.len() == self.config.history_capacity {
            self.history.pop_front();
        }
        self.history.push_back(Anchor {
            observation,
            end: None,
        });
        self.state = ClockState::Running;
        Ok(())
    }

    /// Maps a captured timestamp without applying the presentation/current-time clamp
    pub fn estimate_song_time(
        &self,
        monotonic: MonotonicTime,
    ) -> Result<ClockEstimate, ClockError> {
        let anchor = self
            .history
            .iter()
            .rev()
            .find(|anchor| anchor.observation.monotonic <= monotonic)
            .ok_or(ClockError::OutsideHistory)?;
        if anchor.end.is_some_and(|end| monotonic >= end) {
            return Err(ClockError::OutsideHistory);
        }
        self.project(anchor.observation, monotonic)
    }

    /// Returns nondecreasing song positions within an epoch, with any clamp disclosed
    /// in the uncertainty; callers must not substitute this for raw input mapping
    pub fn now(&mut self, monotonic: MonotonicTime) -> Result<ClockEstimate, ClockError> {
        self.require_running()?;
        if monotonic < self.transition_at
            || self
                .last_observation()
                .is_some_and(|last| monotonic < last.monotonic)
            || self.last_now.is_some_and(|last| monotonic < last.monotonic)
        {
            return Err(ClockError::NonMonotonicQuery);
        }
        let mut estimate = match self.estimate_song_time(monotonic) {
            Err(ClockError::CalibrationExpired) => {
                self.state = ClockState::Expired;
                return Err(ClockError::CalibrationExpired);
            }
            result => result?,
        };
        if let Some(last) = self.last_now {
            let correction =
                i128::from(last.song_time.frames()) - i128::from(estimate.song_time.frames());
            if correction > 0 {
                estimate.uncertainty_frames = estimate
                    .uncertainty_frames
                    .checked_add(u64::try_from(correction).map_err(|_| ClockError::Overflow)?)
                    .ok_or(ClockError::Overflow)?;
                estimate.song_time = last.song_time;
            }
        }
        self.last_now = Some(estimate);
        Ok(estimate)
    }

    pub fn pause(&mut self, at: MonotonicTime) -> Result<(), ClockError> {
        self.require_observable()?;
        self.transition(ClockState::Paused, at)
    }

    /// Keeps the epoch and monotonic song-time floor, requiring a fresh observation
    pub fn resume(&mut self, at: MonotonicTime) -> Result<(), ClockError> {
        if self.state != ClockState::Paused {
            return Err(ClockError::InvalidTransition);
        }
        self.transition(ClockState::Uncalibrated, at)
    }

    pub fn invalidate_calibration(&mut self, at: MonotonicTime) -> Result<(), ClockError> {
        if matches!(self.state, ClockState::Paused | ClockState::DeviceLost) {
            return Err(ClockError::InvalidTransition);
        }
        self.transition(ClockState::Uncalibrated, at)
    }

    /// A replacement device requires a new epoch through `restart`
    pub fn device_lost(&mut self, at: MonotonicTime) -> Result<(), ClockError> {
        self.transition(ClockState::DeviceLost, at)
    }

    /// Epochs must increase so old inputs can never become current again
    pub fn restart(&mut self, epoch: SessionEpoch, at: MonotonicTime) -> Result<(), ClockError> {
        if epoch <= self.epoch {
            return Err(ClockError::WrongEpoch);
        }
        self.transition(ClockState::Uncalibrated, at)?;
        self.epoch = epoch;
        self.history.clear();
        self.last_now = None;
        Ok(())
    }

    fn project(
        &self,
        observation: ClockObservation,
        monotonic: MonotonicTime,
    ) -> Result<ClockEstimate, ClockError> {
        let elapsed = monotonic
            .nanos()
            .checked_sub(observation.monotonic.nanos())
            .ok_or(ClockError::OutsideHistory)?;
        if elapsed > self.config.max_extrapolation_ns {
            return Err(ClockError::CalibrationExpired);
        }
        let scaled_frames = u128::from(elapsed) * u128::from(CANONICAL_SAMPLE_RATE);
        let frames =
            i64::try_from(scaled_frames / NANOS_PER_SECOND).map_err(|_| ClockError::Overflow)?;
        let drift = (scaled_frames * u128::from(self.config.max_drift_ppm))
            .div_ceil(NANOS_PER_SECOND * 1_000_000);
        Ok(ClockEstimate {
            epoch: observation.epoch,
            monotonic,
            song_time: observation
                .song_time
                .checked_add_frames(frames)
                .ok_or(ClockError::Overflow)?,
            uncertainty_frames: observation
                .uncertainty_frames
                .checked_add(u64::try_from(drift).map_err(|_| ClockError::Overflow)?)
                .and_then(|bound| bound.checked_add(1))
                .ok_or(ClockError::Overflow)?,
        })
    }

    fn close_anchor(&mut self, at: MonotonicTime) {
        if let Some(anchor) = self
            .history
            .back_mut()
            .filter(|anchor| anchor.end.is_none())
        {
            anchor.end = Some(at);
        }
    }

    fn transition(&mut self, state: ClockState, at: MonotonicTime) -> Result<(), ClockError> {
        if at < self.transition_at
            || self
                .last_observation()
                .is_some_and(|last| at < last.monotonic)
            || self.last_now.is_some_and(|last| at < last.monotonic)
        {
            return Err(ClockError::NonMonotonicQuery);
        }
        self.close_anchor(at);
        self.transition_at = at;
        self.state = state;
        Ok(())
    }

    fn require_observable(&self) -> Result<(), ClockError> {
        match self.state {
            ClockState::Paused => Err(ClockError::Paused),
            ClockState::DeviceLost => Err(ClockError::DeviceLost),
            ClockState::Invalidated => Err(ClockError::Discontinuity),
            _ => Ok(()),
        }
    }

    fn require_running(&self) -> Result<(), ClockError> {
        self.require_observable()?;
        match self.state {
            ClockState::Uncalibrated => Err(ClockError::Uncalibrated),
            ClockState::Expired => Err(ClockError::CalibrationExpired),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono(nanos: u64) -> MonotonicTime {
        MonotonicTime::from_nanos(nanos)
    }

    fn observation(nanos: u64, frames: i64, uncertainty: u64) -> ClockObservation {
        ClockObservation {
            epoch: SessionEpoch(1),
            monotonic: mono(nanos),
            song_time: SongTime::from_frames(frames),
            uncertainty_frames: uncertainty,
        }
    }

    fn bridge() -> ClockBridge {
        ClockBridge::new(SessionEpoch(1), ClockConfig::default()).unwrap()
    }

    #[test]
    fn simulated_durations_and_drift_stay_inside_declared_bounds() {
        // Independent rational oscillator, with no accumulation or float round trips
        for seconds in [30, 64, 300, 600, 36_000] {
            for ppm in [-100_i128, 0, 100] {
                let origin = u64::MAX - 40_000_000_000_000;
                let truth = |nanos: u64| {
                    i64::try_from(
                        i128::from(nanos) * 48_000 * (1_000_000 + ppm) / 1_000_000_000_000_000,
                    )
                    .unwrap()
                        - 96_000
                };
                let mut clock = ClockBridge::new(
                    SessionEpoch(1),
                    ClockConfig {
                        max_extrapolation_ns: 1_000_000_000,
                        max_drift_ppm: 150,
                        history_capacity: 8,
                    },
                )
                .unwrap();
                let mut previous = SongTime::from_frames(i64::MIN);
                for second in 0..=seconds {
                    let elapsed = second * 1_000_000_000;
                    let noise = if second % 2 == 0 { -17 } else { 17 };
                    clock
                        .observe(observation(origin + elapsed, truth(elapsed) + noise, 17))
                        .unwrap();
                    for offset in [0, 500_000_000, 999_999_999] {
                        let estimate = clock.now(mono(origin + elapsed + offset)).unwrap();
                        let error =
                            (estimate.song_time.frames() - truth(elapsed + offset)).unsigned_abs();
                        assert!(error <= estimate.uncertainty_frames);
                        assert!(estimate.song_time >= previous);
                        previous = estimate.song_time;
                    }
                }
            }
        }
    }

    #[test]
    fn device_frames_keep_channel_count_and_precision_separate() {
        assert_eq!(
            DeviceTime::from_frames(44_100).to_song_time(44_100),
            SongTime::from_seconds(1)
        );
        assert_eq!(
            DeviceTime::from_frames(1_728_000_000).to_song_time(48_000),
            SongTime::from_seconds(36_000)
        );
        assert_eq!(DeviceTime::from_frames(u64::MAX).to_song_time(0), None);
        assert_eq!(DeviceTime::from_frames(u64::MAX).to_song_time(1), None);
        assert_eq!(
            DeviceTime::from_frames(1).to_song_time(96_000),
            Some(SongTime::ZERO)
        );
    }

    #[test]
    fn correction_clamps_now_but_preserves_raw_capture_mapping() {
        let mut clock = bridge();
        clock.observe(observation(0, 100, 10)).unwrap();
        let first = clock.now(mono(100_000)).unwrap();
        assert_eq!(first.song_time.frames(), 104);
        let noisy = observation(110_000, 99, 10);
        clock.observe(noisy).unwrap();
        let raw = clock.estimate_song_time(mono(110_000)).unwrap();
        let current = clock.now(mono(110_000)).unwrap();
        assert_eq!(clock.last_observation(), Some(noisy));
        assert_eq!(raw.song_time.frames(), 99);
        assert_eq!(current.song_time.frames(), 104);
        assert_eq!(current.uncertainty_frames, raw.uncertainty_frames + 5);
        assert_eq!(clock.now(mono(100_000)), Err(ClockError::NonMonotonicQuery));
        assert_eq!(clock.estimate_song_time(mono(100_000)).unwrap(), first);
    }

    #[test]
    fn stalls_expire_and_new_observations_do_not_certify_the_gap() {
        let mut clock = bridge();
        clock.observe(observation(0, 0, 0)).unwrap();
        assert!(clock.now(mono(250_000_000)).is_ok());
        assert_eq!(
            clock.now(mono(250_000_001)),
            Err(ClockError::CalibrationExpired)
        );
        assert_eq!(clock.state(), ClockState::Expired);
        clock
            .observe(observation(1_000_000_000, 48_000, 0))
            .unwrap();
        assert_eq!(clock.state(), ClockState::Running);
        assert_eq!(
            clock.now(mono(1_000_000_000)).unwrap().song_time.frames(),
            48_000
        );
        assert_eq!(
            clock.estimate_song_time(mono(500_000_000)),
            Err(ClockError::CalibrationExpired)
        );
    }

    #[test]
    fn observation_order_epochs_and_discontinuities_are_explicit() {
        let mut clock = bridge();
        let first = observation(1_000_000, 48, 0);
        clock.observe(first).unwrap();
        for nanos in [0, 1_000_000] {
            assert_eq!(
                clock.observe(observation(nanos, 48, 0)),
                Err(ClockError::NonMonotonicObservation)
            );
        }
        let mut wrong_epoch = observation(2_000_000, 96, 0);
        wrong_epoch.epoch = SessionEpoch(0);
        assert_eq!(clock.observe(wrong_epoch), Err(ClockError::WrongEpoch));
        assert_eq!(clock.last_observation(), Some(first));
        assert_eq!(
            clock.observe(observation(2_000_000, 0, 0)),
            Err(ClockError::Discontinuity)
        );
        assert_eq!(clock.state(), ClockState::Invalidated);
        assert_eq!(clock.now(mono(2_000_000)), Err(ClockError::Discontinuity));
        assert!(clock.estimate_song_time(mono(1_500_000)).is_ok());
        clock.invalidate_calibration(mono(2_000_000)).unwrap();
        clock.observe(observation(2_000_000, 0, 0)).unwrap();
        assert_eq!(
            clock.now(mono(2_000_000)).unwrap().song_time,
            SongTime::ZERO
        );
    }

    #[test]
    fn pause_resume_and_device_loss_close_mapping_segments() {
        let mut clock = bridge();
        assert_eq!(clock.now(mono(0)), Err(ClockError::Uncalibrated));
        clock.observe(observation(0, 0, 0)).unwrap();
        clock.now(mono(10_000_000)).unwrap();
        assert_eq!(
            clock.pause(mono(9_000_000)),
            Err(ClockError::NonMonotonicQuery)
        );
        clock.pause(mono(20_000_000)).unwrap();
        assert_eq!(clock.now(mono(20_000_000)), Err(ClockError::Paused));
        assert_eq!(
            clock.observe(observation(21_000_000, 960, 0)),
            Err(ClockError::Paused)
        );
        assert!(clock.estimate_song_time(mono(19_000_000)).is_ok());
        assert_eq!(
            clock.estimate_song_time(mono(20_000_000)),
            Err(ClockError::OutsideHistory)
        );
        clock.resume(mono(1_000_000_000)).unwrap();
        assert_eq!(
            clock.now(mono(1_000_000_000)),
            Err(ClockError::Uncalibrated)
        );
        clock.observe(observation(1_000_000_000, 960, 0)).unwrap();
        assert_eq!(clock.epoch(), SessionEpoch(1));
        assert_eq!(
            clock.now(mono(1_000_000_000)).unwrap().song_time.frames(),
            960
        );
        assert_eq!(
            clock.estimate_song_time(mono(500_000_000)),
            Err(ClockError::OutsideHistory)
        );
        clock.device_lost(mono(1_010_000_000)).unwrap();
        assert_eq!(clock.now(mono(1_010_000_000)), Err(ClockError::DeviceLost));
        assert_eq!(
            clock.observe(observation(1_010_000_000, 1_440, 0)),
            Err(ClockError::DeviceLost)
        );
        assert_eq!(
            clock.invalidate_calibration(mono(1_010_000_000)),
            Err(ClockError::InvalidTransition)
        );
        assert!(clock.estimate_song_time(mono(1_005_000_000)).is_ok());
        assert_eq!(
            clock.restart(SessionEpoch(1), mono(1_020_000_000)),
            Err(ClockError::WrongEpoch)
        );
        clock.restart(SessionEpoch(2), mono(1_020_000_000)).unwrap();
        assert_eq!(
            clock.estimate_song_time(mono(19_000_000)),
            Err(ClockError::OutsideHistory)
        );
        assert_eq!(
            clock.observe(observation(1_020_000_000, 0, 0)),
            Err(ClockError::WrongEpoch)
        );
        let mut restarted = observation(1_020_000_000, 0, 0);
        restarted.epoch = SessionEpoch(2);
        clock.observe(restarted).unwrap();
        assert_eq!(
            clock.now(mono(1_020_000_000)).unwrap().song_time,
            SongTime::ZERO
        );
    }

    #[test]
    fn history_and_overflow_fail_instead_of_fabricating_time() {
        let mut clock = ClockBridge::new(
            SessionEpoch(1),
            ClockConfig {
                history_capacity: 1,
                ..ClockConfig::default()
            },
        )
        .unwrap();
        clock.observe(observation(0, 0, 0)).unwrap();
        clock.observe(observation(1_000_000, 48, 0)).unwrap();
        assert_eq!(
            clock.estimate_song_time(mono(0)),
            Err(ClockError::OutsideHistory)
        );
        let mut clock = bridge();
        assert_eq!(
            clock.observe(observation(0, 0, u64::MAX)),
            Err(ClockError::Overflow)
        );
        assert_eq!(clock.last_observation(), None);
        clock.observe(observation(0, i64::MAX, 0)).unwrap();
        assert_eq!(clock.now(mono(1_000_000)), Err(ClockError::Overflow));
        assert_eq!(
            clock.last_observation().unwrap().song_time.frames(),
            i64::MAX
        );
        assert!(
            ClockBridge::new(
                SessionEpoch(0),
                ClockConfig {
                    history_capacity: 0,
                    ..ClockConfig::default()
                }
            )
            .is_err()
        );
    }
}
