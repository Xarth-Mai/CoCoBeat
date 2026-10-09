//! Cached contact and harmony layers; source music is never transposed

use std::{f32::consts::TAU, time::Duration};

use cocobeat_schema::{PlayerId, SongTime};
use kira::{
    Frame,
    sound::static_sound::{StaticSoundData, StaticSoundSettings},
};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_FEEDBACK_VOICES: usize = 8;
const RATE: u32 = 48_000;
const VARIANTS: usize = 4;
#[cfg(test)]
const PEAK: f32 = 0.055;
// Kira's cubic resampler can overshoot 1.25x and its pan law boosts one channel
const CACHE_PEAK: f32 = 0.036;
const BASE_MIDI: u8 = 72;
const MIN_MIDI: u8 = 60;
const LONG_TONAL_SECONDS: f32 = 0.38;
const SHORT_TONAL_SECONDS: f32 = 0.16;
// Lowest note is one octave below the cached root, so its tail lasts twice as long
pub(crate) const MAX_TONAL_SECONDS: f32 = LONG_TONAL_SECONDS * 2.0;
pub(crate) const MAX_SHORT_TONAL_SECONDS: f32 = SHORT_TONAL_SECONDS * 2.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FeedbackTimbre {
    Wood,
    Crisp,
    Drums,
    Plucks,
    Glass,
    #[default]
    Elastic,
}

impl FeedbackTimbre {
    pub(crate) const ALL: [Self; 6] = [
        Self::Wood,
        Self::Crisp,
        Self::Drums,
        Self::Plucks,
        Self::Glass,
        Self::Elastic,
    ];

    fn companion(self) -> Self {
        match self {
            Self::Wood => Self::Plucks,
            Self::Crisp => Self::Glass,
            Self::Drums => Self::Elastic,
            Self::Plucks => Self::Wood,
            Self::Glass => Self::Crisp,
            Self::Elastic => Self::Drums,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FeedbackContext {
    pub event_id: u64,
    pub song_time: SongTime,
    pub family: FeedbackTimbre,
    /// Only a confident current chord belongs here; None produces percussion alone
    pub chord_mask: Option<u16>,
    pub energy: f32,
    pub density: f32,
    pub short_tonal: bool,
    pub beat_seconds: f32,
}

struct VoicePair {
    contact: StaticSoundData,
    tonal: StaticSoundData,
}

pub(crate) struct FeedbackPalette {
    voices: Vec<VoicePair>,
    last_variant: [[Option<usize>; 6]; 3],
    last_note: [u8; 2],
}

impl Default for FeedbackPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedbackPalette {
    pub(crate) fn new() -> Self {
        let mut voices = Vec::with_capacity(6 * 3 * VARIANTS * 2);
        for family in FeedbackTimbre::ALL {
            for velocity in 0..3 {
                for variant in 0..VARIANTS {
                    for dense in [false, true] {
                        voices.push(VoicePair {
                            contact: synthesize(family, velocity, variant, dense, false),
                            tonal: synthesize(family, velocity, variant, dense, true),
                        });
                    }
                }
            }
        }
        Self {
            voices,
            last_variant: [[None; 6]; 3],
            last_note: [67, 76],
        }
    }

    pub(crate) fn reset(&mut self) {
        self.last_variant = [[None; 6]; 3];
        self.last_note = [67, 76];
    }

    pub(crate) fn hit(
        &mut self,
        player: PlayerId,
        context: FeedbackContext,
    ) -> Vec<StaticSoundData> {
        let p = player.index();
        let family = if p == 0 {
            context.family
        } else {
            context.family.companion()
        };
        let variant = self.variant(p, family, context);
        let pan = if p == 0 { -0.48 } else { 0.48 };
        let bank = self.bank(family, variant, context, false);
        let mut sounds = vec![bank.contact.panning(pan)];
        if let Some(note) = nearest_note(context.chord_mask, self.last_note[p], None) {
            sounds.push(
                self.tonal(family, variant, context, note)
                    .panning(pan * 0.7),
            );
            self.last_note[p] = note;
        }
        sounds
    }

    /// A confirmed joint event adds a complementary answer, not two repeated hits
    pub(crate) fn duo(&mut self, context: FeedbackContext, precise: bool) -> Vec<StaticSoundData> {
        let family = context.family.companion();
        let variant = self.variant(2, family, context);
        let Some(first) = nearest_note(context.chord_mask, self.last_note[0] + 5, None) else {
            return vec![
                self.bank(family, variant, context, false)
                    .contact
                    .panning(0.0)
                    .volume(-2.0),
            ];
        };
        let octave_answer = if first <= 72 { first + 12 } else { first - 12 };
        let second = nearest_note(context.chord_mask, self.last_note[1], Some(first % 12))
            .unwrap_or(octave_answer);
        let subdivision = finite_unit(context.beat_seconds, 0.5, 0.2, 2.0) as f64 * 0.125;
        let density_gain = finite_unit(context.density, 0.0, 0.0, 1.0) * 6.0;
        let mut sounds = vec![
            self.tonal(family, variant, context, first)
                .panning(-0.3)
                .volume(-3.0 - density_gain),
            self.tonal(context.family, (variant + 1) % VARIANTS, context, second)
                .panning(0.3)
                .volume(-3.0 - density_gain)
                .start_time(Duration::from_secs_f64(subdivision)),
        ];
        if precise && finite_unit(context.density, 0.0, 0.0, 1.0) < 0.8 {
            sounds.push(
                self.tonal(family, (variant + 2) % VARIANTS, context, octave_answer)
                    .panning(0.0)
                    .volume(-6.0 - density_gain)
                    .start_time(Duration::from_secs_f64(subdivision * 2.0)),
            );
        }
        self.last_note = [first, second];
        sounds
    }

    fn variant(
        &mut self,
        player: usize,
        family: FeedbackTimbre,
        context: FeedbackContext,
    ) -> usize {
        let seed = context.event_id ^ ((player as u64 + 1) * 0x517cc1b727220a95);
        let mut variant =
            ((seed ^ (seed >> 27)).wrapping_mul(0x94d049bb133111eb) >> 32) as usize % VARIANTS;
        let previous = &mut self.last_variant[player][family as usize];
        if *previous == Some(variant) {
            variant = (variant + 1) % VARIANTS;
        }
        *previous = Some(variant);
        variant
    }

    fn bank(
        &self,
        family: FeedbackTimbre,
        variant: usize,
        context: FeedbackContext,
        short_tonal: bool,
    ) -> &VoicePair {
        let velocity = (finite_unit(context.energy, 0.4, 0.0, 1.0) * 2.99) as usize;
        let dense = usize::from(finite_unit(context.density, 0.0, 0.0, 1.0) > 0.55 || short_tonal);
        &self.voices[(((family as usize * 3 + velocity) * VARIANTS + variant) * 2) + dense]
    }

    fn tonal(
        &self,
        family: FeedbackTimbre,
        variant: usize,
        context: FeedbackContext,
        note: u8,
    ) -> StaticSoundData {
        self.bank(family, variant, context, context.short_tonal)
            .tonal
            .playback_rate(2.0_f64.powf((f64::from(note) - f64::from(BASE_MIDI)) / 12.0))
            .volume(-2.0 - finite_unit(context.density, 0.0, 0.0, 1.0) * 6.0)
    }
}

fn finite_unit(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn nearest_note(mask: Option<u16>, previous: u8, exclude: Option<u8>) -> Option<u8> {
    let mask = mask? & 0xfff;
    (MIN_MIDI..=84_u8)
        .filter(|note| mask & (1 << (note % 12)) != 0 && exclude != Some(note % 12))
        .min_by_key(|note| (note.abs_diff(previous), *note))
}

const WOOD: [&[u8]; 3] = [
    include_bytes!("../../../assets/audio/feedback/wood-soft.pcm"),
    include_bytes!("../../../assets/audio/feedback/wood-medium.pcm"),
    include_bytes!("../../../assets/audio/feedback/wood-hard.pcm"),
];
const CLAP: [&[u8]; 3] = [
    include_bytes!("../../../assets/audio/feedback/clap-soft.pcm"),
    include_bytes!("../../../assets/audio/feedback/clap-medium.pcm"),
    include_bytes!("../../../assets/audio/feedback/clap-hard.pcm"),
];
const GLASS: [&[u8]; 3] = [
    include_bytes!("../../../assets/audio/feedback/glass-soft.pcm"),
    include_bytes!("../../../assets/audio/feedback/glass-medium.pcm"),
    include_bytes!("../../../assets/audio/feedback/glass-hard.pcm"),
];

fn pcm(bytes: &[u8], position: f32) -> f32 {
    let index = position as usize;
    let sample = |i: usize| {
        bytes.get(i * 2..i * 2 + 2).map_or(0.0, |b| {
            f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0
        })
    };
    let a = sample(index);
    a + (sample(index + 1) - a) * position.fract()
}

fn synthesize(
    family: FeedbackTimbre,
    velocity: usize,
    variant: usize,
    dense: bool,
    tonal: bool,
) -> StaticSoundData {
    let duration = match (tonal, dense) {
        (true, false) => LONG_TONAL_SECONDS,
        (true, true) => SHORT_TONAL_SECONDS,
        (false, false) => 0.14,
        (false, true) => 0.085,
    };
    let count = (RATE as f32 * duration) as usize;
    let brightness = 0.65 + velocity as f32 * 0.25;
    let frequency = 523.2511;
    let mut noise_state = 0x9e3779b9_u32.wrapping_mul(variant as u32 + 1);
    let mut low_noise = 0.0;
    let mut previous_noise = 0.0;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let t = index as f32 / RATE as f32;
        let phase = TAU * frequency * t;
        noise_state ^= noise_state << 13;
        noise_state ^= noise_state >> 17;
        noise_state ^= noise_state << 5;
        let noise = noise_state as f32 / u32::MAX as f32 * 2.0 - 1.0;
        low_noise += (noise - low_noise) * (0.12 + brightness * 0.14);
        let high_noise = noise - previous_noise;
        previous_noise = noise;
        let sample = if tonal {
            match family {
                FeedbackTimbre::Wood => {
                    phase.sin() * (-t * 16.0).exp()
                        + 0.22 * brightness * (phase * 4.0).sin() * (-t * 65.0).exp()
                }
                FeedbackTimbre::Crisp => {
                    phase.sin() * (-t * 20.0).exp()
                        + 0.3 * brightness * (phase * 3.0).sin() * (-t * 32.0).exp()
                }
                FeedbackTimbre::Drums => {
                    phase.sin() * (-t * 13.0).exp() + 0.24 * (phase * 2.0).sin() * (-t * 35.0).exp()
                }
                FeedbackTimbre::Plucks => (1..=6)
                    .map(|h| {
                        (phase * h as f32).sin()
                            * (-t * (7.0 + h as f32 * 7.0)).exp()
                            * brightness.powi(h - 1)
                            / (h * h) as f32
                    })
                    .sum(),
                // Measured VCSL partial is 1053–1054 Hz; retune it to the common C5 bank root
                FeedbackTimbre::Glass => {
                    pcm(GLASS[velocity], index as f32 * frequency / 1053.5) * 8.0
                }
                FeedbackTimbre::Elastic => {
                    (phase + (phase * 2.0).sin() * (1.8 + variant as f32 * 0.2) * (-t * 22.0).exp())
                        .sin()
                        * (-t * 10.0).exp()
                }
            }
        } else {
            // Unknown harmony uses noisy contact transients, never a sustained pitched oscillator
            match family {
                FeedbackTimbre::Wood => {
                    pcm(WOOD[velocity], index as f32 * (0.9 + variant as f32 * 0.07))
                        + low_noise * (-t * 100.0).exp() * 0.006
                }
                FeedbackTimbre::Crisp => high_noise * (-t * (85.0 + variant as f32 * 9.0)).exp(),
                FeedbackTimbre::Drums => {
                    pcm(
                        CLAP[velocity],
                        index as f32 * (0.94 + variant as f32 * 0.04),
                    ) + low_noise * (-t * 40.0).exp() * 0.015
                }
                FeedbackTimbre::Plucks => {
                    low_noise * (-t * 110.0).exp() + high_noise * (-t * 210.0).exp() * 0.3
                }
                FeedbackTimbre::Glass => {
                    high_noise * (-t * 150.0).exp() + low_noise * (-t * 28.0).exp() * 0.06
                }
                FeedbackTimbre::Elastic => {
                    low_noise
                        * (-t * 45.0).exp()
                        * (1.0 + 0.35 * (TAU * (27.0 + variant as f32 * 5.0) * t).sin())
                }
            }
        };
        let attack = (t / 0.0007).min(1.0);
        let tail = ((duration - t) / if tonal { 0.065 } else { 0.025 }).clamp(0.0, 1.0);
        let articulation = if tonal {
            (-t * variant as f32 * 2.0).exp()
        } else {
            1.0
        };
        samples.push(sample * attack * tail * tail * articulation);
    }
    let peak = samples
        .iter()
        .copied()
        .map(f32::abs)
        .fold(0.0_f32, f32::max);
    let gain = CACHE_PEAK * [0.58, 0.8, 1.0][velocity] / peak.max(1e-9);
    StaticSoundData {
        sample_rate: RATE,
        frames: samples
            .into_iter()
            .map(|sample| Frame::from_mono(sample * gain))
            .collect::<Vec<_>>()
            .into(),
        settings: StaticSoundSettings::default(),
        slice: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{music_decibels, music_headroom, play_feedback, stop_feedback};
    use std::{collections::VecDeque, io::Write, path::Path};

    use kira::{
        AudioManager, AudioManagerSettings,
        backend::{Backend, Renderer},
        sound::static_sound::StaticSoundHandle,
    };

    fn context() -> FeedbackContext {
        FeedbackContext {
            event_id: 1,
            song_time: SongTime::ZERO,
            family: FeedbackTimbre::Elastic,
            chord_mask: None,
            energy: 0.8,
            density: 0.2,
            short_tonal: false,
            beat_seconds: 0.5,
        }
    }

    #[test]
    fn cached_palette_is_finite_bounded_audible_and_dense_tails_are_shorter() {
        let palette = FeedbackPalette::new();
        for (i, bank) in palette.voices.iter().enumerate() {
            for sound in [&bank.contact, &bank.tonal] {
                assert!(
                    sound
                        .frames
                        .iter()
                        .all(|f| f.left.is_finite() && f.left.abs() <= PEAK + 1e-6)
                );
                assert!(sound.frames.iter().any(|f| f.left.abs() > 0.018));
                assert!(sound.frames.first().unwrap().left.abs() < 1e-6);
                assert!(sound.frames.last().unwrap().left.abs() < 1e-5);
            }
            if i % 2 == 1 {
                assert!(bank.tonal.frames.len() < palette.voices[i - 1].tonal.frames.len());
            }
        }
        assert!(PEAK * MAX_FEEDBACK_VOICES as f32 + 0.5 < 1.0);
    }

    #[test]
    fn kira_interpolation_and_pan_preserve_the_rendered_voice_bound() {
        for signs in 0..16 {
            let frames: [Frame; 4] = std::array::from_fn(|i| {
                Frame::from_mono(if signs & (1 << i) == 0 {
                    -CACHE_PEAK
                } else {
                    CACHE_PEAK
                })
            });
            let data = StaticSoundData {
                sample_rate: RATE,
                frames: frames
                    .into_iter()
                    .cycle()
                    .take(64)
                    .collect::<Vec<_>>()
                    .into(),
                settings: StaticSoundSettings::default(),
                slice: None,
            };
            let mut manager = AudioManager::<CaptureBackend>::new(AudioManagerSettings {
                internal_buffer_size: 64,
                ..Default::default()
            })
            .unwrap();
            let _handle = manager
                .play(data.playback_rate(1.0 / 64.0).panning(-0.48))
                .unwrap();
            let renderer = manager.backend_mut().renderer.as_mut().unwrap();
            let mut output = vec![0.0; 8192];
            renderer.on_start_processing();
            renderer.process(&mut output, 2);
            assert!(output.iter().all(|sample| sample.abs() <= PEAK));
        }
    }

    #[test]
    fn unknown_harmony_is_percussive_and_trusted_chords_use_nearby_notes() {
        let mut palette = FeedbackPalette::new();
        assert_eq!(palette.hit(PlayerId::P1, context()).len(), 1);
        assert_eq!(palette.duo(context(), true).len(), 1);
        let chord_mask = (1 << 0) | (1 << 4) | (1 << 7);
        let chord = Some(chord_mask);
        let trusted = FeedbackContext {
            chord_mask: chord,
            ..context()
        };
        assert_eq!(palette.hit(PlayerId::P1, trusted).len(), 2);
        assert_eq!(palette.duo(trusted, true).len(), 3);
        for note in palette.last_note {
            assert!(chord_mask & (1 << (note % 12)) != 0);
        }
        assert_eq!(nearest_note(chord, 66, None), Some(67));
        assert_eq!(nearest_note(Some(0), 66, None), None);
        assert_eq!(nearest_note(Some(0xf000), 66, None), None);
    }

    #[test]
    fn short_tonal_reuses_existing_tail_without_changing_contact_or_density() {
        let mut palette = FeedbackPalette::new();
        let normal = FeedbackContext {
            chord_mask: Some(0x091),
            ..context()
        };
        let short = FeedbackContext {
            short_tonal: true,
            ..normal
        };
        let long_hit = palette.hit(PlayerId::P1, normal);
        palette.reset();
        let short_hit = palette.hit(PlayerId::P1, short);
        assert!(std::sync::Arc::ptr_eq(
            &long_hit[0].frames,
            &short_hit[0].frames
        ));
        assert_eq!(long_hit[0].frames.len(), short_hit[0].frames.len());
        assert!(short_hit[1].frames.len() < long_hit[1].frames.len());
        let (kira::Value::Fixed(long_gain), kira::Value::Fixed(short_gain)) =
            (long_hit[1].settings.volume, short_hit[1].settings.volume)
        else {
            panic!("palette gain must be fixed")
        };
        assert_eq!(long_gain, short_gain);
        assert_eq!(short.density, normal.density);
        assert_eq!(palette.voices.len(), 6 * 3 * VARIANTS * 2);

        let lowest = palette.tonal(short.family, 0, short, MIN_MIDI);
        let kira::Value::Fixed(rate) = lowest.settings.playback_rate else {
            panic!("palette rate must be fixed")
        };
        assert!(
            lowest.frames.len() as f64 / f64::from(RATE) / rate.0
                <= f64::from(MAX_SHORT_TONAL_SECONDS) + 1e-8
        );
        let mut manager =
            AudioManager::<CaptureBackend>::new(AudioManagerSettings::default()).unwrap();
        let _handle = manager.play(lowest).unwrap();
        let samples = capture(&mut manager, 24_000);
        let end = (MAX_SHORT_TONAL_SECONDS * RATE as f32) as usize;
        assert!(samples[end * 2..].iter().all(|sample| sample.abs() < 1e-7));

        palette.reset();
        let phrase = palette.duo(short, true);
        assert_eq!(phrase.len(), 3);
        for sound in phrase {
            assert_eq!(
                sound.frames.len(),
                (SHORT_TONAL_SECONDS * RATE as f32) as usize
            );
            let kira::Value::Fixed(rate) = sound.settings.playback_rate else {
                panic!("palette rate must be fixed")
            };
            assert!(
                sound.frames.len() as f64 / f64::from(RATE) / rate.0
                    <= f64::from(MAX_SHORT_TONAL_SECONDS) + 1e-8
            );
        }
        assert_eq!(MAX_FEEDBACK_VOICES, 8);
    }

    #[test]
    fn variants_do_not_repeat_and_restarting_reconstructs_sequence() {
        let mut palette = FeedbackPalette::new();
        let sequence = |palette: &mut FeedbackPalette| {
            (0..32)
                .map(|id| {
                    palette.variant(
                        0,
                        FeedbackTimbre::Wood,
                        FeedbackContext {
                            event_id: id,
                            ..context()
                        },
                    )
                })
                .collect::<Vec<_>>()
        };
        let first = sequence(&mut palette);
        assert!(first.windows(2).all(|pair| pair[0] != pair[1]));
        palette.reset();
        assert_eq!(first, sequence(&mut palette));
        palette.reset();
        assert_eq!(palette.last_note, [67, 76]);
        assert!(palette.last_variant.iter().flatten().all(Option::is_none));
    }

    #[test]
    fn event_identity_keeps_live_and_replay_articulation_equal() {
        let mut palette = FeedbackPalette::new();
        let live = palette.hit(
            PlayerId::P1,
            FeedbackContext {
                song_time: SongTime::from_frames(96_140),
                ..context()
            },
        );
        palette.reset();
        let replay = palette.hit(
            PlayerId::P1,
            FeedbackContext {
                song_time: SongTime::from_frames(96_000),
                ..context()
            },
        );
        assert_eq!(live[0].frames.as_ref(), replay[0].frames.as_ref());
        let rate = 2.0_f32.powf((f32::from(MIN_MIDI) - f32::from(BASE_MIDI)) / 12.0);
        assert!((LONG_TONAL_SECONDS / rate - MAX_TONAL_SECONDS).abs() < f32::EPSILON);
    }

    #[test]
    fn delayed_confirmation_preserves_local_articulation_history() {
        let mut palette = FeedbackPalette::new();
        palette.hit(
            PlayerId::P1,
            FeedbackContext {
                song_time: SongTime::from_frames(96_000),
                ..context()
            },
        );
        let local_variant = palette.last_variant[0];
        palette.duo(
            FeedbackContext {
                song_time: SongTime::from_frames(95_000),
                ..context()
            },
            true,
        );
        assert_eq!(palette.last_variant[0], local_variant);
        assert!(local_variant.iter().any(Option::is_some));
    }

    struct CaptureBackend {
        renderer: Option<Renderer>,
    }

    impl Backend for CaptureBackend {
        type Settings = ();
        type Error = ();

        fn setup(_: (), _: usize) -> Result<(Self, u32), ()> {
            Ok((Self { renderer: None }, RATE))
        }

        fn start(&mut self, renderer: Renderer) -> Result<(), ()> {
            self.renderer = Some(renderer);
            Ok(())
        }
    }

    fn capture(manager: &mut AudioManager<CaptureBackend>, frames: usize) -> Vec<f32> {
        let mut samples = vec![0.0; frames * 2];
        let renderer = manager.backend_mut().renderer.as_mut().unwrap();
        renderer.on_start_processing();
        renderer.process(&mut samples, 2);
        samples
    }

    #[test]
    fn production_mixer_caps_bursts_and_pause_cancels_delayed_phrases() {
        let mut palette = FeedbackPalette::new();
        let mut manager =
            AudioManager::<CaptureBackend>::new(AudioManagerSettings::default()).unwrap();
        let mut handles = VecDeque::new();
        for event_id in 0..16 {
            let sounds = palette.hit(
                PlayerId::P1,
                FeedbackContext {
                    event_id,
                    ..context()
                },
            );
            play_feedback(&mut manager, &mut handles, sounds, 100).unwrap();
        }
        assert_eq!(handles.len(), MAX_FEEDBACK_VOICES);
        let immediate = capture(&mut manager, 256);
        let onset = immediate
            .as_chunks::<2>()
            .0
            .iter()
            .position(|f| f[0].abs().max(f[1].abs()) > 1e-5)
            .unwrap();
        assert!(
            onset < 64,
            "contact added a scheduling delay: {onset} frames"
        );

        let original = StaticSoundData {
            sample_rate: RATE,
            frames: (0..48_000)
                .map(|i| {
                    Frame::new(
                        if i % 4 < 2 { 2.0 } else { -2.0 },
                        if i % 4 < 2 { -3.0 } else { 3.0 },
                    )
                })
                .collect::<Vec<_>>()
                .into(),
            settings: StaticSoundSettings::default(),
            slice: None,
        };
        let gain = music_headroom(&original);
        let mut music = manager
            .play(
                original
                    .volume(music_decibels(100, gain))
                    .playback_rate(1.0 / 64.0),
            )
            .unwrap();
        let mixed = capture(&mut manager, 4096);
        assert!(mixed.iter().all(|s| s.is_finite() && s.abs() < 1.0));
        music.stop(kira::Tween {
            duration: Duration::ZERO,
            ..Default::default()
        });
        stop_feedback(&mut handles);
        capture(&mut manager, 128);

        let phrase = palette.duo(
            FeedbackContext {
                chord_mask: Some((1 << 0) | (1 << 4) | (1 << 7)),
                beat_seconds: 2.0,
                ..context()
            },
            true,
        );
        play_feedback(&mut manager, &mut handles, phrase, 80).unwrap();
        assert!(capture(&mut manager, 960).iter().any(|s| s.abs() > 1e-5));
        // AudioOutput::pause uses the same production helper before pausing music
        stop_feedback(&mut handles);
        assert!(handles.is_empty());
        assert!(capture(&mut manager, 48_000).iter().all(|s| s.abs() < 1e-7));
    }

    #[derive(Deserialize)]
    struct PreviewEvent {
        event_id: u64,
        time_seconds: f64,
        kind: String,
        player: Option<String>,
        precise: Option<bool>,
        family: FeedbackTimbre,
        chord_mask: Option<u16>,
        energy: f32,
        density: f32,
        #[serde(default)]
        short_tonal: bool,
        beat_seconds: f32,
    }

    fn write_wav(path: &Path, samples: &[f32]) {
        let mut file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        let bytes = u32::try_from(samples.len() * 2).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(bytes + 36).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&2_u16.to_le_bytes()).unwrap();
        file.write_all(&RATE.to_le_bytes()).unwrap();
        file.write_all(&(RATE * 4).to_le_bytes()).unwrap();
        file.write_all(&4_u16.to_le_bytes()).unwrap();
        file.write_all(&16_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&bytes.to_le_bytes()).unwrap();
        for sample in samples {
            let quantized = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            file.write_all(&quantized.to_le_bytes()).unwrap();
        }
        file.flush().unwrap();
    }

    /// Offline native Kira mixer evidence, not a captured hardware or gameplay session
    #[test]
    #[ignore = "requires explicit song package or reference WAV, resolved presentation events and output directory"]
    fn export_reference_mix() {
        let source = std::env::var("COCOBEAT_FEEDBACK_PREVIEW_SOURCE").unwrap();
        let event_path = std::env::var("COCOBEAT_FEEDBACK_PREVIEW_EVENTS").unwrap();
        let output = std::env::var("COCOBEAT_FEEDBACK_PREVIEW_OUT").unwrap();
        let output = Path::new(&output);
        std::fs::create_dir_all(output).unwrap();
        let (original, source_kind, content_id) = if Path::new(&source).is_dir() {
            let (content, song) = crate::content::load_package(Path::new(&source)).unwrap();
            (song, "canonical_package", Some(content.content_id))
        } else {
            let wav = std::fs::read(&source).unwrap();
            assert_eq!(&wav[..4], b"RIFF");
            assert_eq!(&wav[8..16], b"WAVEfmt ");
            assert_eq!(u32::from_le_bytes(wav[16..20].try_into().unwrap()), 16);
            assert_eq!(u16::from_le_bytes(wav[20..22].try_into().unwrap()), 1);
            assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 2);
            assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), RATE);
            assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
            assert_eq!(&wav[36..40], b"data");
            let frames: Vec<Frame> = wav[44..]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| {
                    Frame::new(
                        f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0,
                        f32::from(i16::from_le_bytes([b[2], b[3]])) / 32768.0,
                    )
                })
                .collect();
            let song = StaticSoundData {
                sample_rate: RATE,
                frames: frames.into(),
                settings: StaticSoundSettings::default(),
                slice: None,
            };
            (song, "source_wav", None)
        };
        let event_bytes = std::fs::read(&event_path).unwrap();
        let events: Vec<PreviewEvent> = serde_json::from_slice(&event_bytes).unwrap();
        std::fs::write(output.join("events.json"), event_bytes).unwrap();
        assert!(
            events
                .windows(2)
                .all(|p| p[0].time_seconds <= p[1].time_seconds)
        );
        assert!(events.iter().all(|e| e.time_seconds.is_finite()
            && e.time_seconds >= 0.0
            && matches!(e.kind.as_str(), "hit" | "duo")));
        let mut stems = Vec::new();
        let mut voice_peak = 0;
        let music_gain = music_headroom(&original);
        let frame_count = original.frames.len() + RATE as usize;
        for with_music in [false, true] {
            let mut manager = AudioManager::<CaptureBackend>::new(AudioManagerSettings {
                internal_buffer_size: 64,
                ..Default::default()
            })
            .unwrap();
            let _music = with_music.then(|| {
                manager
                    .play(original.volume(music_decibels(100, music_gain)))
                    .unwrap()
            });
            let mut palette = FeedbackPalette::new();
            let mut handles: VecDeque<StaticSoundHandle> = VecDeque::new();
            let mut samples = Vec::with_capacity(frame_count * 2);
            let mut cursor = 0_usize;
            let mut event = 0_usize;
            let mut buffer = [0.0_f32; 128];
            while cursor < frame_count {
                while event < events.len()
                    && (events[event].time_seconds * f64::from(RATE)).round() as usize <= cursor
                {
                    let e = &events[event];
                    let context = FeedbackContext {
                        event_id: e.event_id,
                        song_time: SongTime::from_frames(cursor as i64),
                        family: e.family,
                        chord_mask: e.chord_mask,
                        energy: e.energy,
                        density: e.density,
                        short_tonal: e.short_tonal,
                        beat_seconds: e.beat_seconds,
                    };
                    let sounds = if e.kind == "duo" {
                        palette.duo(context, e.precise.unwrap_or(false))
                    } else {
                        let player = match e.player.as_deref() {
                            Some("P1") => PlayerId::P1,
                            Some("P2") => PlayerId::P2,
                            _ => panic!("hit must identify P1 or P2"),
                        };
                        palette.hit(player, context)
                    };
                    play_feedback(&mut manager, &mut handles, sounds, 80).unwrap();
                    voice_peak = voice_peak.max(handles.len());
                    event += 1;
                }
                let next_event = events.get(event).map_or(frame_count, |e| {
                    (e.time_seconds * f64::from(RATE)).round() as usize
                });
                let count = 64
                    .min(frame_count - cursor)
                    .min(next_event.saturating_sub(cursor).max(1));
                let renderer = manager.backend_mut().renderer.as_mut().unwrap();
                renderer.on_start_processing();
                renderer.process(&mut buffer[..count * 2], 2);
                samples.extend_from_slice(&buffer[..count * 2]);
                cursor += count;
            }
            assert_eq!(
                event,
                events.len(),
                "events must fit the reference duration"
            );
            assert!(
                samples.iter().all(|s| s.is_finite() && s.abs() < 1.0),
                "mixed samples clipped before quantization"
            );
            write_wav(
                &output.join(if with_music {
                    "mixed.wav"
                } else {
                    "feedback.wav"
                }),
                &samples,
            );
            stems.push(
                samples
                    .iter()
                    .copied()
                    .map(f32::abs)
                    .fold(0.0_f32, f32::max),
            );
        }
        if source_kind == "canonical_package" {
            let samples: Vec<f32> = original
                .frames
                .iter()
                .flat_map(|frame| [frame.left, frame.right])
                .collect();
            write_wav(&output.join("original.wav"), &samples);
        } else {
            std::fs::copy(&source, output.join("original.wav")).unwrap();
        }
        let receipt = serde_json::json!({ "scope": "Offline actual Kira Renderer and production feedback enqueue; resolved event fixture, not hardware capture", "sample_rate": RATE, "frames": frame_count, "events": events.len(), "feedback_peak": stems[0], "mixed_peak": stems[1], "voice_peak": voice_peak, "voice_cap": MAX_FEEDBACK_VOICES, "music_gain": music_gain, "feedback_gain": 0.8, "source": source, "source_kind": source_kind, "content_id": content_id, "event_source": event_path, "status": "PASS" });
        std::fs::write(
            output.join("receipt.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
    }
}
