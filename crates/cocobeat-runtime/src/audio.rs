//! Kira playback cursors describe rendered audio, not measured speaker output

use std::{
    f32::consts::TAU,
    time::{Duration, Instant},
};

use cocobeat_schema::{PlayerId, SongTime};
use kira::{
    AudioManager, AudioManagerSettings, DefaultBackend, Frame, Tween,
    backend::cpal::{
        CpalBackendSettings,
        cpal::{
            self,
            traits::{DeviceTrait, HostTrait},
        },
    },
    sound::{
        PlaybackState,
        static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings},
    },
};

use crate::{brand_audio, brand_intro::BrandImpact, dev_song};

#[path = "audio_observation.rs"]
mod observation;

use observation::{AudioObservations, SourceReader};
pub use observation::{CallbackObservation, SourceObservation};

pub struct AudioOutput {
    manager: AudioManager<DefaultBackend>,
    song: Option<StaticSoundData>,
    music: Option<ControlledSound>,
    observations: AudioObservations,
    music_source: Option<SourceReader>,
    hits: [Option<StaticSoundData>; 2],
    brand_sounds: [StaticSoundData; 3],
    brand_handles: [Option<ControlledSound>; 3],
    discarded_errors: u64,
    output_info: String,
}

impl AudioOutput {
    pub fn new(song: Option<StaticSoundData>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("No default audio output device")?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("Cannot query audio output configuration: {error}"))?;
        let config = supported.config();
        let output_info = format!(
            "audio_host={:?}\ndevice_id={:?}\ndevice_description={:?}\nrequested_output_sample_rate={}\nrequested_output_channels={}\nrequested_output_buffer={:?}\ndefault_supported_sample_format={:?}\nactual_callback_size=NOT MEASURED AT INITIALIZATION (callback_observation reports previous completed batch)\n",
            host.id(),
            device.id(),
            device.description(),
            config.sample_rate,
            config.channels,
            config.buffer_size,
            supported.sample_format(),
        );
        let observations = AudioObservations::new();
        let manager = AudioManager::new(AudioManagerSettings {
            backend_settings: CpalBackendSettings {
                // Fix initial selection; Kira may still rebuild on another device after an error
                device: Some(device),
                config: Some(config),
            },
            main_track_builder: observations.main_track(),
            ..Default::default()
        })
        .map_err(|error| format!("Cannot initialize audio output: {error}"))?;
        Ok(Self {
            manager,
            song,
            music: None,
            observations,
            music_source: None,
            hits: [None, None],
            brand_sounds: [BrandImpact::Co1, BrandImpact::Co2, BrandImpact::Beat]
                .map(brand_audio::sound),
            brand_handles: [None, None, None],
            discarded_errors: 0,
            output_info,
        })
    }

    pub fn start(&mut self) -> Result<(), String> {
        self.start_at(SongTime::ZERO)
    }

    /// Starts a new canonical PCM instance at a source frame, without changing any clock mapping
    pub fn start_at(&mut self, position: SongTime) -> Result<(), String> {
        let song = self.song.as_ref().ok_or("No song has been loaded")?.clone();
        let song = sound_at(song, position)?;
        self.stop();
        self.play_music(song)
            .map_err(|error| format!("Cannot start music: {error}"))
    }

    /// Kira starts this delay when its callback consumes the sound, not at a measured device time
    pub fn schedule(&mut self, deadline: Instant) -> Result<(), String> {
        let song = self.song.as_ref().ok_or("No song has been loaded")?.clone();
        schedule_delay(deadline, Instant::now())?;
        self.stop();
        let song = song.start_time(schedule_delay(deadline, Instant::now())?);
        self.play_music(song)
            .map_err(|error| format!("Cannot schedule music: {error}"))
    }

    fn play_music(&mut self, song: StaticSoundData) -> Result<(), kira::PlaySoundError<()>> {
        let (handle, source) = self.manager.play(self.observations.sound(song))?;
        self.music = Some(ControlledSound::new(handle));
        self.music_source = Some(source);
        Ok(())
    }

    /// Replacing decoded content cancels pending playback before changing the song
    pub fn replace_song(&mut self, song: StaticSoundData) {
        self.stop();
        self.song = Some(song);
    }

    pub(crate) fn output_info(&self) -> &str {
        &self.output_info
    }

    /// Returns the enqueue time, excluding PCM generation; only an explicit probe calls this
    pub(crate) fn start_click_probe(&mut self, duration_seconds: u32) -> Result<Instant, String> {
        let frames = probe_duration_frames(duration_seconds)?;
        self.stop();
        // ponytail: static PCM is capped at 230 MB; stream only if probes exceed ten minutes
        let data = sound_data((0..frames).map(probe_sample).collect());
        let requested = Instant::now();
        self.play_music(data)
            .map_err(|error| format!("Cannot start audio probe: {error}"))?;
        Ok(requested)
    }

    /// Records the final frame intent; `reconcile_playback` enqueues it after control processing
    pub fn pause(&mut self) {
        if let Some(music) = &mut self.music {
            music.paused = true;
        }
    }

    /// Records the final frame intent; inspect `state` for callback acknowledgment
    pub fn resume(&mut self) {
        if let Some(music) = &mut self.music {
            music.paused = false;
        }
    }

    /// Enqueues a stop and discards the handle; the audio callback applies it asynchronously
    pub fn stop(&mut self) {
        self.music_source = None;
        if let Some(mut music) = self.music.take() {
            music.handle.stop(immediate());
        }
        self.stop_brand();
    }

    /// Each impact plays at most once until `stop_brand` clears the startup handles
    pub fn play_brand(&mut self, impact: BrandImpact) -> Result<(), String> {
        let index = match impact {
            BrandImpact::Co1 => 0,
            BrandImpact::Co2 => 1,
            BrandImpact::Beat => 2,
        };
        if self.brand_handles[index].is_some() {
            return Ok(());
        }
        match self.manager.play(self.brand_sounds[index].clone()) {
            Ok(handle) => self.brand_handles[index] = Some(ControlledSound::new(handle)),
            Err(error) => {
                self.stop_brand();
                return Err(format!("Cannot play brand impact: {error}"));
            }
        }
        Ok(())
    }

    /// Startup effects use the same acknowledged control path as music
    pub fn pause_brand(&mut self) {
        for handle in self.brand_handles.iter_mut().flatten() {
            handle.paused = true;
        }
    }

    pub fn resume_brand(&mut self) {
        for handle in self.brand_handles.iter_mut().flatten() {
            handle.paused = false;
        }
    }

    pub fn stop_brand(&mut self) {
        for handle in &mut self.brand_handles {
            if let Some(mut handle) = handle.take() {
                handle.handle.stop(immediate());
            }
        }
    }

    /// Call once after all frame controls so only their final intent reaches the callback
    pub fn reconcile_playback(&mut self) {
        for sound in self
            .music
            .iter_mut()
            .chain(self.brand_handles.iter_mut().flatten())
        {
            sound.reconcile();
        }
    }

    /// Source seconds last published by an audio callback, with unknown device buffering
    /// Natural completion can leave the last cursor short of the content length; inspect `state`
    pub fn position(&self) -> Option<f64> {
        self.music.as_ref().map(|music| music.handle.position())
    }

    /// Last coherent main-mix observation, unavailable before its first hook or during a write
    /// Backend errors or sample-rate changes invalidate this AudioOutput observation lifetime
    /// These are historical software observations, not future callback or device latency bounds
    pub fn callback_observation(&self) -> Option<CallbackObservation> {
        self.observations.callback()
    }

    /// Source position paired with its publication interval, never an unobserved initial cursor
    /// Unavailable before publication, during contention, after stop or observation invalidation
    /// A successful read may be old; consumers must bound publication age for their own policy
    pub fn source_observation(&self) -> Option<SourceObservation> {
        self.music_source
            .as_ref()?
            .read(&self.music.as_ref()?.handle)
    }

    /// Includes pending control intent as Pausing/Resuming until the callback acknowledges it
    /// Stopped always takes precedence over any remaining intent
    pub fn state(&self) -> Option<PlaybackState> {
        self.music.as_ref().map(ControlledSound::state)
    }

    pub fn hit(&mut self, player: PlayerId) -> Result<(), String> {
        let sound = self.hits[player.index()].get_or_insert_with(|| hit_sound(player));
        self.manager
            .play(sound.clone())
            .map(|_| ())
            .map_err(|error| format!("Cannot play local feedback: {error}"))
    }

    /// The two player tones form a shared open fifth when core confirms synchronization
    pub fn sync(&mut self) -> Result<(), String> {
        self.hit(PlayerId::P1)?;
        self.hit(PlayerId::P2)
    }

    /// Every backend error invalidates the caller's timing calibration, including recovered errors
    pub fn take_error(&mut self) -> Option<String> {
        let backend = self.manager.backend_mut();
        if let Some(error) = backend.pop_error() {
            self.observations.invalidate();
            self.stop_brand();
            return Some(format!("Audio output error: {error}"));
        }
        let discarded = backend.num_stream_errors_discarded().unwrap_or(0);
        if discarded != self.discarded_errors {
            self.observations.invalidate();
            self.discarded_errors = discarded;
            self.stop_brand();
            return Some(format!(
                "Audio output discarded {discarded} stream errors; timing is invalid"
            ));
        }
        None
    }
}

fn schedule_delay(deadline: Instant, now: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(now)
        .filter(|delay| *delay >= Duration::from_millis(100))
        .ok_or_else(|| "Music scheduling needs at least 100 ms before the deadline".into())
}

struct ControlledSound {
    handle: StaticSoundHandle,
    paused: bool,
    pending: Option<PlaybackState>,
}

impl ControlledSound {
    fn new(handle: StaticSoundHandle) -> Self {
        Self {
            handle,
            paused: false,
            pending: None,
        }
    }

    fn desired_state(&self) -> PlaybackState {
        if self.paused {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        }
    }

    fn state(&self) -> PlaybackState {
        let observed = self.handle.state();
        if observed == PlaybackState::Stopped {
            return observed;
        }
        if observed != self.desired_state()
            || self.pending.is_some_and(|pending| pending != observed)
        {
            if self.paused {
                PlaybackState::Pausing
            } else {
                PlaybackState::Resuming
            }
        } else {
            observed
        }
    }

    fn reconcile(&mut self) {
        let observed = self.handle.state();
        if observed == PlaybackState::Stopped {
            self.pending = None;
            return;
        }
        // Kira consumes independent pause/resume slots in fixed order, not enqueue order
        // Await the previous command so an opposite request cannot overtake it
        if self.pending.is_some_and(|pending| pending != observed) {
            return;
        }
        self.pending = None;
        let desired = self.desired_state();
        if observed != desired {
            if self.paused {
                self.handle.pause(immediate());
            } else {
                self.handle.resume(immediate());
            }
            self.pending = Some(desired);
        }
    }
}

fn immediate() -> Tween {
    Tween {
        duration: Duration::ZERO,
        ..Default::default()
    }
}

pub fn sound_data(frames: Vec<Frame>) -> StaticSoundData {
    StaticSoundData {
        sample_rate: dev_song::SAMPLE_RATE,
        frames: frames.into(),
        settings: StaticSoundSettings::default(),
        slice: None,
    }
}

fn sound_at(song: StaticSoundData, position: SongTime) -> Result<StaticSoundData, String> {
    if song.sample_rate != 48_000 || !(0..song.frames.len() as i64).contains(&position.frames()) {
        return Err("Playback start must be inside canonical PCM".into());
    }
    Ok(song.start_position(position.frames() as f64 / 48_000.0))
}

pub(crate) fn probe_duration_frames(seconds: u32) -> Result<u32, String> {
    if !matches!(seconds, 30 | 64 | 300 | 600) {
        return Err("Audio probe duration must be 30, 64, 300, or 600 seconds".into());
    }
    Ok(seconds * dev_song::SAMPLE_RATE)
}

// Original CC0-1.0 probe signal: a 10 ms decaying 1 kHz pulse at each half-second offset
fn probe_sample(frame: u32) -> Frame {
    let within = frame % dev_song::SAMPLE_RATE;
    let Some(offset) = within.checked_sub(dev_song::SAMPLE_RATE / 2) else {
        return Frame::ZERO;
    };
    if offset >= 480 {
        return Frame::ZERO;
    }
    let polarity = if offset % 48 < 24 { 1.0 } else { -1.0 };
    Frame::from_mono(polarity * (480 - offset) as f32 / 480.0 * 0.16)
}

// Original short feedback tones are CC0-1.0; generator source remains MPL-2.0
fn hit_sound(player: PlayerId) -> StaticSoundData {
    let (frequency, harmonic, pan) = match player {
        PlayerId::P1 => (523.25, 2.0, -0.65),
        PlayerId::P2 => (783.99, 3.0, 0.65),
    };
    let length = dev_song::SAMPLE_RATE as usize * 80 / 1_000;
    let frames = (0..length)
        .map(|index| {
            let phase = TAU * frequency * index as f32 / dev_song::SAMPLE_RATE as f32;
            let attack = (index as f32 / 120.0).min(1.0);
            let decay = (length - 1 - index) as f32 / length as f32;
            Frame::from_mono(
                (phase.sin() + 0.25 * (phase * harmonic).sin()) * attack * decay * 0.16,
            )
        })
        .collect();
    sound_data(frames).panning(pan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::backend::mock::{MockBackend, MockBackendSettings};

    fn mock_audio() -> AudioManager<MockBackend> {
        AudioManager::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: 48_000,
            },
            ..Default::default()
        })
        .unwrap()
    }

    fn callback(audio: &mut AudioManager<MockBackend>) {
        audio.backend_mut().on_start_processing();
        audio.backend_mut().process();
    }

    #[test]
    fn scheduled_native_sound_waits_and_can_be_cancelled_before_replacement() {
        let now = Instant::now();
        for deadline in [
            now - Duration::from_secs(1),
            now,
            now + Duration::from_millis(99),
        ] {
            assert!(schedule_delay(deadline, now).is_err());
        }
        let delay = schedule_delay(now + Duration::from_millis(100), now).unwrap();
        assert_eq!(delay, Duration::from_millis(100));

        let mut audio = mock_audio();
        let data = sound_data(vec![Frame::from_mono(0.1); 48_000]);
        let mut scheduled = ControlledSound::new(audio.play(data.start_time(delay)).unwrap());
        // The mock callback renders 128 frames at 48 kHz per invocation
        for _ in 0..20 {
            callback(&mut audio);
            assert_eq!(scheduled.handle.position(), 0.0);
        }
        scheduled.handle.stop(immediate());
        callback(&mut audio);
        assert_eq!(scheduled.state(), PlaybackState::Stopped);
        assert_eq!(scheduled.handle.position(), 0.0);

        let replacement = sound_data(vec![Frame::from_mono(-0.1); 24_000]);
        let immediate = ControlledSound::new(audio.play(replacement).unwrap());
        for _ in 0..2 {
            callback(&mut audio);
        }
        assert!(immediate.handle.position() > 0.0);
        assert_eq!(scheduled.handle.position(), 0.0);

        let scheduled = ControlledSound::new(audio.play(data.start_time(delay)).unwrap());
        for _ in 0..20 {
            callback(&mut audio);
            assert_eq!(scheduled.handle.position(), 0.0);
        }
        for _ in 0..30 {
            callback(&mut audio);
        }
        assert!(scheduled.handle.position() > 0.0);
        assert_eq!(scheduled.state(), PlaybackState::Playing);
    }

    #[test]
    fn controls_coalesce_and_wait_for_opposite_command_acknowledgment() {
        let mut audio = mock_audio();
        let mut sound =
            ControlledSound::new(audio.play(sound_data(vec![Frame::ZERO; 48_000])).unwrap());
        callback(&mut audio);

        for paused in [true, false] {
            let (settled, opposite, transitioning) = if paused {
                (
                    PlaybackState::Paused,
                    PlaybackState::Playing,
                    PlaybackState::Pausing,
                )
            } else {
                (
                    PlaybackState::Playing,
                    PlaybackState::Paused,
                    PlaybackState::Resuming,
                )
            };
            sound.paused = paused;
            sound.reconcile();
            callback(&mut audio);
            sound.reconcile();
            assert_eq!(sound.state(), settled);

            // Two controls in one frame must not send both Kira commands
            sound.paused = !paused;
            sound.paused = paused;
            sound.reconcile();
            callback(&mut audio);
            assert_eq!(sound.handle.state(), settled);

            // A second frame can arrive before the first command's callback
            sound.paused = !paused;
            sound.reconcile();
            sound.paused = paused;
            sound.reconcile();
            assert_eq!(sound.handle.state(), settled);
            assert_eq!(sound.state(), transitioning);
            callback(&mut audio);
            assert_eq!(sound.handle.state(), opposite);
            assert_eq!(sound.state(), transitioning);
            sound.reconcile();
            callback(&mut audio);
            assert_eq!(sound.handle.state(), settled);
            assert_eq!(sound.state(), settled);
        }
    }

    #[test]
    fn stopped_sound_cannot_resume_and_new_handle_has_no_pending_control() {
        let mut audio = mock_audio();
        let data = sound_data(vec![Frame::ZERO; 48_000]);
        let mut sound = ControlledSound::new(audio.play(data.clone()).unwrap());
        callback(&mut audio);
        sound.paused = true;
        sound.reconcile();
        callback(&mut audio);
        sound.paused = false;
        sound.reconcile();
        sound.handle.stop(immediate());
        sound.paused = true;
        callback(&mut audio);
        assert_eq!(sound.state(), PlaybackState::Stopped);
        sound.reconcile();
        assert!(sound.pending.is_none());
        callback(&mut audio);
        assert_eq!(sound.handle.state(), PlaybackState::Stopped);

        sound = ControlledSound::new(audio.play(data).unwrap());
        sound.reconcile();
        callback(&mut audio);
        callback(&mut audio);
        assert_eq!(sound.state(), PlaybackState::Playing);
        assert!(sound.handle.position() > 0.0);
    }

    #[test]
    fn canonical_start_positions_reject_edges_and_render_from_requested_source_frame() {
        let mut audio = mock_audio();
        let data = sound_data(
            (0..48_000)
                .map(|i| Frame::from_mono(i as f32 / 48_000.0))
                .collect(),
        );
        for frame in [-1, 48_000, i64::MAX] {
            assert!(sound_at(data.clone(), SongTime::from_frames(frame)).is_err());
        }
        let mut noncanonical = data.clone();
        noncanonical.sample_rate = 44_100;
        assert!(sound_at(noncanonical, SongTime::ZERO).is_err());
        let mut sound = ControlledSound::new(
            audio
                .play(sound_at(data, SongTime::from_frames(24_000)).unwrap())
                .unwrap(),
        );
        assert_eq!(sound.handle.position(), 0.5);
        callback(&mut audio);
        callback(&mut audio);
        assert!(sound.handle.position() > 0.5);
        sound.paused = true;
        sound.reconcile();
        callback(&mut audio);
        assert_eq!(sound.state(), PlaybackState::Paused);
        let paused = sound.handle.position();
        callback(&mut audio);
        assert_eq!(sound.handle.position(), paused);
        sound.paused = false;
        sound.reconcile();
        callback(&mut audio);
        callback(&mut audio);
        assert!(sound.handle.position() > paused);
    }

    #[test]
    fn feedback_tones_are_short_distinct_and_bounded() {
        let tones = [hit_sound(PlayerId::P1), hit_sound(PlayerId::P2)];
        for tone in &tones {
            assert_eq!(tone.sample_rate, 48_000);
            assert_eq!(tone.frames.len(), 3_840);
            assert_eq!(tone.frames.first(), Some(&Frame::ZERO));
            assert_eq!(tone.frames.last(), Some(&Frame::ZERO));
            assert!(tone.frames.iter().all(|frame| {
                frame.left.is_finite()
                    && frame.right.is_finite()
                    && frame.left.abs() <= 0.2
                    && frame.right.abs() <= 0.2
            }));
            assert!(tone.frames.iter().any(|frame| frame.left.abs() > 0.05));
        }
        assert_ne!(tones[0].frames, tones[1].frames);
        assert_ne!(tones[0].settings.panning, tones[1].settings.panning);
    }

    #[test]
    fn probe_signal_has_explicit_onsets_and_bounded_duration() {
        for (seconds, frames) in [
            (30, 1_440_000),
            (64, 3_072_000),
            (300, 14_400_000),
            (600, 28_800_000),
        ] {
            assert_eq!(probe_duration_frames(seconds), Ok(frames));
        }
        for seconds in [0, 1, 31, 601, u32::MAX] {
            assert!(probe_duration_frames(seconds).is_err());
        }
        let active: Vec<_> = (0..96_000)
            .filter(|&frame| probe_sample(frame) != Frame::ZERO)
            .collect();
        assert_eq!(active.len(), 960);
        assert_eq!(active[0], 24_000);
        assert_eq!(active[479], 24_479);
        assert_eq!(active[480], 72_000);
        assert_eq!(active[959], 72_479);
        for frame in active {
            let sample = probe_sample(frame);
            assert!(sample.left.is_finite() && sample.left.abs() <= 0.16);
            assert_eq!(sample.left, sample.right);
        }
    }
}
