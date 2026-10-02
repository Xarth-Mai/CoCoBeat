//! Canonical audio frames, distinct from render, device, and musical clocks.

/// One frame contains every channel at one sample instant.
pub const CANONICAL_SAMPLE_RATE: u32 = 48_000;

/// Signed canonical 48 kHz frames; negative values represent pre-roll.
///
/// Precision of representation is not a claim about hardware timing accuracy.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SongTime(i64);

impl SongTime {
    pub const ZERO: Self = Self(0);

    pub const fn from_frames(frames: i64) -> Self {
        Self(frames)
    }

    pub const fn frames(self) -> i64 {
        self.0
    }

    /// Exact conversion, rejecting overflow rather than wrapping or saturating.
    pub fn from_seconds(seconds: i64) -> Option<Self> {
        seconds
            .checked_mul(i64::from(CANONICAL_SAMPLE_RATE))
            .map(Self)
    }

    /// Exact conversion: one millisecond is 48 canonical frames.
    pub fn from_millis(millis: i64) -> Option<Self> {
        millis
            .checked_mul(i64::from(CANONICAL_SAMPLE_RATE / 1_000))
            .map(Self)
    }

    /// Floating-point conversion for import/UI boundaries only.
    /// Rounds to the nearest frame, with half frames away from zero.
    /// Rejects non-finite values and values outside the signed frame range.
    pub fn try_from_seconds_f64(seconds: f64) -> Option<Self> {
        let frames = (seconds * f64::from(CANONICAL_SAMPLE_RATE)).round();
        // i64::MAX rounds to 2^63 in f64, so the upper bound is exclusive.
        if !frames.is_finite() || frames < i64::MIN as f64 || frames >= -(i64::MIN as f64) {
            return None;
        }
        Some(Self(frames as i64))
    }

    /// Display/diagnostic conversion; do not feed this back into game truth.
    pub fn as_seconds_f64(self) -> f64 {
        self.0 as f64 / f64::from(CANONICAL_SAMPLE_RATE)
    }

    pub fn checked_add_frames(self, frames: i64) -> Option<Self> {
        self.0.checked_add(frames).map(Self)
    }

    pub fn checked_frames_since(self, earlier: Self) -> Option<i64> {
        self.0.checked_sub(earlier.0)
    }
}

/// Musical-grid position. A tempo map and tick resolution are required to map it
/// to SongTime; it is never implicitly converted to an audio-frame count.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct MusicalTick(pub i64);

/// Session generation identifier. Restarting a session invalidates the old epoch.
/// This is an identity, not a wall-clock timestamp or a SongTime offset.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SessionEpoch(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_units_and_preroll() {
        assert_eq!(
            SongTime::from_seconds(1),
            Some(SongTime::from_frames(48_000))
        );
        assert_eq!(SongTime::from_millis(1), Some(SongTime::from_frames(48)));
        assert_eq!(
            SongTime::from_seconds(-2),
            Some(SongTime::from_frames(-96_000))
        );
        assert_eq!(
            SongTime::from_seconds(64),
            Some(SongTime::from_frames(3_072_000))
        );
        assert_eq!(SongTime::from_frames(-48_000).as_seconds_f64(), -1.0);
    }

    #[test]
    fn float_boundaries_have_explicit_rounding_and_rejection() {
        for (seconds, frames) in [
            (0.0, 0),
            (0.5 / 48_000.0, 1),
            (-0.5 / 48_000.0, -1),
            (1.25, 60_000),
        ] {
            assert_eq!(
                SongTime::try_from_seconds_f64(seconds),
                Some(SongTime::from_frames(frames))
            );
        }
        for seconds in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            i64::MAX as f64 / 48_000.0,
        ] {
            assert_eq!(SongTime::try_from_seconds_f64(seconds), None);
        }
        assert_eq!(
            SongTime::try_from_seconds_f64(i64::MIN as f64 / 48_000.0),
            Some(SongTime::from_frames(i64::MIN))
        );
    }

    #[test]
    fn arithmetic_never_wraps() {
        assert_eq!(SongTime::from_seconds(i64::MAX), None);
        assert_eq!(SongTime::from_millis(i64::MIN), None);
        assert_eq!(SongTime::from_frames(i64::MAX).checked_add_frames(1), None);
        assert_eq!(SongTime::from_frames(i64::MIN).checked_add_frames(-1), None);
        assert_eq!(
            SongTime::from_frames(i64::MAX).checked_frames_since(SongTime::from_frames(-1)),
            None
        );
        assert_eq!(
            SongTime::from_frames(-48).checked_frames_since(SongTime::ZERO),
            Some(-48)
        );
    }

    #[test]
    fn ten_hours_of_blocks_have_no_accumulated_drift() {
        let mut clock = SongTime::ZERO;
        // Literal oracle independent of conversion helpers: 10 h at 48 kHz.
        for _ in 0..3_375_000 {
            clock = clock.checked_add_frames(512).unwrap();
        }
        assert_eq!(clock.frames(), 1_728_000_000);
    }
}
