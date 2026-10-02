//! Original brand PCM is CC0-1.0; this generator source remains MPL-2.0

use std::f32::consts::TAU;

use kira::{
    Frame,
    sound::static_sound::{StaticSoundData, StaticSoundSettings},
};

use crate::brand_intro::BrandImpact;

/// Generate once per impact and cache the result in the existing audio owner
pub fn sound(impact: BrandImpact) -> StaticSoundData {
    let (milliseconds, start_hz, end_hz, attack_seconds, harmonic) = match impact {
        BrandImpact::Co1 => (280, 440.0, 330.0, 0.006, 0.20),
        BrandImpact::Co2 => (320, 587.33, 440.0, 0.006, 0.20),
        BrandImpact::Beat => (620, 160.0, 70.0, 0.018, 0.12),
    };
    let sample_rate = 48_000;
    let length = sample_rate as usize * milliseconds / 1_000;
    let duration = (length - 1) as f32 / sample_rate as f32;
    let sweep = (end_hz - start_hz) / duration;
    let frames: Vec<_> = (0..length)
        .map(|index| {
            let time = index as f32 / sample_rate as f32;
            // Integrate the frequency ramp so the pitch falls continuously
            let phase = TAU * (start_hz * time + 0.5 * sweep * time * time);
            let attack = smoothstep(time / attack_seconds);
            let release = smoothstep((duration - time) / 0.024);
            let envelope = attack * release * (-5.0 * time / duration).exp();
            Frame::from_mono((phase.sin() + harmonic * (phase * 2.0).sin()) * envelope * 0.25)
        })
        .collect();
    StaticSoundData {
        sample_rate,
        frames: frames.into(),
        settings: StaticSoundSettings::default(),
        slice: None,
    }
}

fn smoothstep(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impacts_are_distinct_bounded_and_fade_before_unlock() {
        let sounds = [
            sound(BrandImpact::Co1),
            sound(BrandImpact::Co2),
            sound(BrandImpact::Beat),
        ];
        for (sound, length) in sounds.iter().zip([13_440, 15_360, 29_760]) {
            assert_eq!(sound.sample_rate, 48_000);
            assert_eq!(sound.frames.len(), length);
            assert_eq!(sound.frames.first(), Some(&Frame::ZERO));
            assert_eq!(sound.frames.last(), Some(&Frame::ZERO));
            assert!(sound.frames.iter().all(|frame| {
                frame.left.is_finite() && frame.left.abs() <= 0.30 && frame.left == frame.right
            }));
            assert!(sound.frames.iter().any(|frame| frame.left.abs() > 0.10));
            assert!(
                sound
                    .frames
                    .windows(2)
                    .all(|pair| { (pair[1].left - pair[0].left).abs() < 0.03 })
            );
            assert!(
                sound.frames[..48]
                    .iter()
                    .all(|frame| frame.left.abs() < 0.02)
            );
            assert!(
                sound.frames[length - 480..]
                    .iter()
                    .all(|frame| frame.left.abs() < 0.001)
            );
        }
        let crossings: Vec<_> = sounds
            .iter()
            .map(|sound| {
                sound.frames[..4_800]
                    .windows(2)
                    .filter(|pair| pair[0].left <= 0.0 && pair[1].left > 0.0)
                    .count()
            })
            .collect();
        assert!(crossings[1] > crossings[0]);
        assert!(crossings[0] > crossings[2] * 2);
        assert_eq!(sounds[0].frames, sound(BrandImpact::Co1).frames);
        assert!(sounds[2].frames.len() as f32 / 48_000.0 <= 0.65);
    }
}
