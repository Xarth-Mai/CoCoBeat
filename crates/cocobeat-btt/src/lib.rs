//! One fixed 48 kHz BTT instance with bounded borrowed mono input and no callbacks

pub const UPSTREAM_COMMIT: &str = "c039090f1af771092d95c3ffc402e557940f7384";
pub const BLOCK_FRAMES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoEstimate {
    pub bpm: f64,
    pub period_frames: u32,
    /// The original histogram value, not calibrated confidence
    pub native_certainty: f64,
}

pub struct Btt48000(ffi::Handle);

impl Btt48000 {
    pub fn new() -> Result<Self, &'static str> {
        ffi::Handle::new().map(Self)
    }

    /// Borrows at most 128 real samples; the caller supplies the actual short EOF block
    pub fn process(&mut self, samples: &mut [f32]) -> Result<TempoEstimate, &'static str> {
        validate_samples(samples)?;
        let (bpm, period, native_certainty) = self.0.process(samples);
        validate_estimate(bpm, period, native_certainty)
    }
}

fn validate_samples(samples: &[f32]) -> Result<(), &'static str> {
    if !(1..=BLOCK_FRAMES).contains(&samples.len())
        || samples.iter().any(|v| !v.is_finite() || v.abs() > 4.0)
    {
        return Err("BTT input requires 1..=128 finite mono frames with absolute peak <= 4");
    }
    Ok(())
}

fn validate_estimate(
    bpm: f64,
    period: i32,
    native_certainty: f64,
) -> Result<TempoEstimate, &'static str> {
    if !bpm.is_finite()
        || !native_certainty.is_finite()
        || native_certainty < 0.0
        || !((bpm == 0.0 && period == 0)
            || ((50.0..=200.0).contains(&bpm)
                && period > 0
                && period % BLOCK_FRAMES as i32 == 0
                && (bpm - 60.0 * 48_000.0 / f64::from(period)).abs() <= 1e-6 * bpm))
    {
        return Err("BTT output is non-finite or disagrees with its fixed period / tempo bounds");
    }
    Ok(TempoEstimate {
        bpm,
        period_frames: period as u32,
        native_certainty,
    })
}

// Unsafe is confined to one ownership wrapper; callers receive no raw handle
#[allow(unsafe_code)]
mod ffi {
    use std::{ffi::c_void, ptr::NonNull};

    unsafe extern "C" {
        fn cocobeat_btt_48000_new() -> *mut c_void;
        fn cocobeat_btt_48000_process(
            tracker: *mut c_void,
            samples: *mut f32,
            frames: u32,
            bpm: *mut f64,
            period: *mut i32,
            certainty: *mut f64,
        );
        fn cocobeat_btt_48000_drop(tracker: *mut c_void);
    }

    pub(super) struct Handle(NonNull<c_void>);

    impl Handle {
        pub(super) fn new() -> Result<Self, &'static str> {
            // The C constructor either owns a complete fixed instance or frees its partial state
            NonNull::new(unsafe { cocobeat_btt_48000_new() })
                .map(Self)
                .ok_or("BTT instance allocation failed")
        }

        pub(super) fn process(&mut self, samples: &mut [f32]) -> (f64, i32, f64) {
            let (mut bpm, mut period, mut certainty) = (0.0, 0, 0.0);
            // The sole caller validated 1..=128 finite samples; all outputs are live scalars
            // Exclusive self and sample borrows outlive the synchronous no-callback C call
            unsafe {
                cocobeat_btt_48000_process(
                    self.0.as_ptr(),
                    samples.as_mut_ptr(),
                    samples.len() as u32,
                    &mut bpm,
                    &mut period,
                    &mut certainty,
                );
            }
            (bpm, period, certainty)
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // NonNull is owned only here, never cloned, and freed once with its matching allocator
            unsafe { cocobeat_btt_48000_drop(self.0.as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_input_and_invalid_native_values_are_rejected() {
        for samples in [vec![], vec![0.0; 129], vec![f32::NAN], vec![4.01]] {
            assert!(validate_samples(&samples).is_err());
        }
        assert!(validate_samples(&[-4.0, 4.0]).is_ok());
        for (bpm, period, certainty) in [
            (f64::NAN, 0, 0.0),
            (0.0, -128, 0.0),
            (120.0, 0, 0.0),
            (120.0, 24_000, -1.0),
            (120.0, 24_000, f64::INFINITY),
            (120.0, 24_001, 1.0),
            (125.0, 24_064, 1.0),
        ] {
            assert!(validate_estimate(bpm, period, certainty).is_err());
        }
        let estimate = validate_estimate(60.0 * 48_000.0 / 24_064.0, 24_064, 19.75).unwrap();
        assert_eq!(estimate.native_certainty, 19.75);
    }

    #[test]
    fn native_short_silence_is_raw_zero_and_rejected_input_does_not_advance() {
        let mut fresh = Btt48000::new().unwrap();
        let mut other = Btt48000::new().unwrap();
        assert!(other.process(&mut [f32::INFINITY]).is_err());
        let zero = TempoEstimate {
            bpm: 0.0,
            period_frames: 0,
            native_certainty: 0.0,
        };
        assert_eq!(fresh.process(&mut [0.0; 17]).unwrap(), zero);
        assert_eq!(other.process(&mut [0.0; 17]).unwrap(), zero);
    }
}
