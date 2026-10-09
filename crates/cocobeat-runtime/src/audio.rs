//! Kira playback cursors describe rendered audio, not measured speaker output

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, TryLockError},
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

use crate::{
    brand_audio,
    brand_intro::BrandImpact,
    dev_song,
    feedback_audio::{FeedbackContext, FeedbackPalette, MAX_FEEDBACK_VOICES},
};

#[path = "audio_observation.rs"]
mod observation;

use observation::{AudioObservations, SourceReader};
pub use observation::{CallbackObservation, SourceObservation};

pub struct AudioOutput {
    manager: AudioManager<DefaultBackend>,
    song: Option<StaticSoundData>,
    music: Option<Arc<Mutex<ControlledSound>>>,
    observations: AudioObservations,
    music_source: Option<SourceReader>,
    palette: FeedbackPalette,
    feedback_handles: VecDeque<StaticSoundHandle>,
    music_volume: u8,
    music_headroom: f32,
    feedback_volume: u8,
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
        let music_headroom = song.as_ref().map_or(0.4, music_headroom);
        Ok(Self {
            manager,
            song,
            music: None,
            observations,
            music_source: None,
            palette: FeedbackPalette::new(),
            feedback_handles: VecDeque::new(),
            music_volume: 100,
            music_headroom,
            feedback_volume: 80,
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
        let (handle, source) = self.manager.play(
            self.observations
                .sound(song.volume(music_decibels(self.music_volume, self.music_headroom))),
        )?;
        self.music = Some(Arc::new(Mutex::new(ControlledSound::new(handle))));
        self.music_source = Some(source);
        Ok(())
    }

    /// Replacing decoded content cancels pending playback before changing the song
    pub fn replace_song(&mut self, song: StaticSoundData) {
        self.stop();
        self.music_headroom = music_headroom(&song);
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
        self.stop_feedback();
        if let Some(music) = &self.music
            && let Ok(mut music) = music.lock()
        {
            music.paused = true;
        }
    }

    /// Records the final frame intent; inspect `state` for callback acknowledgment
    pub fn resume(&mut self) {
        if let Some(music) = &self.music
            && let Ok(mut music) = music.lock()
        {
            music.paused = false;
        }
    }

    /// Enqueues one delayed resume on the existing, acknowledged Paused source
    /// The caller verifies stable source publications and keeps input gated until RecoveryReady
    /// Kira counts the delay from command consumption; this is not a device-time guarantee
    /// Canceling an armed resume requires terminal stop, not another pause request
    pub fn resume_at(&mut self, deadline: Instant) -> Result<(), String> {
        self.source_observation()
            .ok_or("Scheduled resume requires an available current source observation")?;
        self.music
            .as_mut()
            .ok_or("No music is playing")?
            .lock()
            .map_err(|_| "Music control lock is poisoned")?
            .resume_at(deadline)
    }

    /// Enqueues a stop and discards the handle; the audio callback applies it asynchronously
    pub fn stop(&mut self) {
        reset_feedback(&mut self.palette, &mut self.feedback_handles);
        self.music_source = None;
        if let Some(music) = self.music.take() {
            // Poisoned state can only be used to enqueue terminal cleanup
            music
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .handle
                .stop(immediate());
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
        if let Some(music) = &self.music
            && let Ok(mut music) = music.lock()
        {
            music.reconcile();
        }
        for sound in self.brand_handles.iter_mut().flatten() {
            sound.reconcile();
        }
    }

    /// Source seconds last published by an audio callback, with unknown device buffering
    /// Natural completion can leave the last cursor short of the content length; inspect `state`
    pub fn position(&self) -> Option<f64> {
        Some(self.music.as_ref()?.lock().ok()?.handle.position())
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
            .read(&self.music.as_ref()?.try_lock().ok()?.handle)
    }

    pub(crate) fn source_sampler(&self) -> Result<SourceSampler, String> {
        Ok(SourceSampler {
            music: self
                .music
                .as_ref()
                .ok_or("No original music handle")?
                .clone(),
            source: self
                .music_source
                .as_ref()
                .ok_or("No original source reader")?
                .clone(),
        })
    }

    /// Includes pending control intent as Pausing/Resuming until the callback acknowledges it
    /// Stopped always takes precedence over any remaining intent
    pub fn state(&self) -> Option<PlaybackState> {
        Some(self.music.as_ref()?.lock().ok()?.state())
    }

    pub(crate) fn set_mix(&mut self, music: u8, feedback: u8) {
        let music = music.min(100);
        if self.music_volume != music {
            self.music_volume = music;
            if let Some(handle) = &self.music
                && let Ok(mut sound) = handle.lock()
            {
                sound
                    .handle
                    .set_volume(music_decibels(music, self.music_headroom), immediate());
            }
        }
        self.feedback_volume = feedback.min(100);
    }

    fn stop_feedback(&mut self) {
        stop_feedback(&mut self.feedback_handles);
    }

    pub(crate) fn play_prepared(&mut self, sounds: Vec<StaticSoundData>) -> Result<(), String> {
        play_feedback(
            &mut self.manager,
            &mut self.feedback_handles,
            sounds,
            self.feedback_volume,
        )
    }

    pub(crate) fn hit(&mut self, player: PlayerId, context: FeedbackContext) -> Result<(), String> {
        let sounds = self.prepare_hit(player, context);
        self.play_prepared(sounds)
    }

    pub(crate) fn sync(&mut self, context: FeedbackContext, precise: bool) -> Result<(), String> {
        let sounds = self.prepare_sync(context, precise);
        self.play_prepared(sounds)
    }

    pub(crate) fn prepare_hit(
        &mut self,
        player: PlayerId,
        context: FeedbackContext,
    ) -> Vec<StaticSoundData> {
        self.palette.hit(player, context)
    }

    pub(crate) fn prepare_sync(
        &mut self,
        context: FeedbackContext,
        precise: bool,
    ) -> Vec<StaticSoundData> {
        self.palette.duo(context, precise)
    }

    /// Every backend error invalidates the caller's timing calibration, including recovered errors
    pub fn take_error(&mut self) -> Option<String> {
        if self.music.as_ref().is_some_and(|music| music.is_poisoned()) {
            self.observations.invalidate();
            self.stop();
            return Some("Music control lock is poisoned; timing is invalid".into());
        }
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

/// Read-only worker access; raw state is independent of the coherent source publication
#[derive(Clone)]
pub(crate) struct SourceSampler {
    music: Arc<Mutex<ControlledSound>>,
    source: SourceReader,
}

impl SourceSampler {
    #[cfg(test)]
    pub(crate) fn music_owners(&self) -> usize {
        Arc::strong_count(&self.music)
    }

    /// Mock callback producer only; background samplers retain nonblocking `read`
    #[cfg(test)]
    pub(crate) fn read_witness(&self) -> Result<(SourceObservation, PlaybackState), String> {
        let music = self
            .music
            .lock()
            .map_err(|_| "Music sampler control lock is poisoned".to_owned())?;
        self.source
            .read(&music.handle)
            .map(|row| (row, music.handle.state()))
            .ok_or_else(|| "Mock callback has no coherent source witness".to_owned())
    }

    pub(crate) fn read(&self) -> Result<Option<(SourceObservation, PlaybackState)>, String> {
        match self.music.try_lock() {
            Ok(music) => Ok(self
                .source
                .read(&music.handle)
                .map(|row| (row, music.handle.state()))),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(_)) => Err("Music sampler control lock is poisoned".into()),
        }
    }
}

#[cfg(test)]
pub(crate) fn mock_source_sampler() -> (
    AudioManager<kira::backend::mock::MockBackend>,
    SourceSampler,
) {
    let mut observations = AudioObservations::new();
    let mut audio = AudioManager::new(AudioManagerSettings {
        backend_settings: kira::backend::mock::MockBackendSettings {
            sample_rate: 48_000,
        },
        main_track_builder: observations.main_track(),
        ..Default::default()
    })
    .unwrap();
    let (handle, source) = audio
        .play(observations.sound(sound_data(vec![Frame::ZERO; 48_000])))
        .unwrap();
    (
        audio,
        SourceSampler {
            music: Arc::new(Mutex::new(ControlledSound::new(handle))),
            source,
        },
    )
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

    fn resume_at(&mut self, deadline: Instant) -> Result<(), String> {
        if !self.paused
            || self.handle.state() != PlaybackState::Paused
            || self
                .pending
                .is_some_and(|pending| pending != PlaybackState::Paused)
        {
            return Err(
                "Scheduled resume requires acknowledged Paused music with no pending resume".into(),
            );
        }
        let delay = schedule_delay(deadline, Instant::now())?;
        self.handle.resume_at(delay.into(), immediate());
        self.paused = false;
        // WaitingToResume is not the final ACK, so reconcile must retain this delayed command
        self.pending = Some(PlaybackState::Playing);
        Ok(())
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

pub(crate) fn music_headroom(song: &StaticSoundData) -> f32 {
    // Source PCM and cubic resampling may exceed unity; reserve a half-scale music ceiling
    let peak = song.frames.iter().fold(1.0_f32, |peak, frame| {
        peak.max(frame.left.abs()).max(frame.right.abs())
    });
    0.4 / peak
}

pub(crate) fn music_decibels(volume: u8, headroom: f32) -> f32 {
    if volume == 0 {
        f32::NEG_INFINITY
    } else {
        20.0 * (f32::from(volume.min(100)) / 100.0 * headroom).log10()
    }
}

pub(crate) fn stop_feedback(handles: &mut VecDeque<StaticSoundHandle>) {
    for mut handle in handles.drain(..) {
        handle.stop(immediate());
    }
}

fn reset_feedback(palette: &mut FeedbackPalette, handles: &mut VecDeque<StaticSoundHandle>) {
    stop_feedback(handles);
    palette.reset();
}

pub(crate) fn play_feedback<B: kira::backend::Backend>(
    manager: &mut AudioManager<B>,
    handles: &mut VecDeque<StaticSoundHandle>,
    sounds: Vec<StaticSoundData>,
    volume: u8,
) -> Result<(), String> {
    handles.retain(|handle| handle.state() != PlaybackState::Stopped);
    let gain = music_decibels(volume, 1.0);
    for sound in sounds {
        // Validate before stealing a voice so malformed settings cannot interrupt valid sounds
        let kira::Value::Fixed(base) = sound.settings.volume else {
            return Err("Feedback palette requires a fixed gain".into());
        };
        if handles.len() >= MAX_FEEDBACK_VOICES {
            handles.pop_front().unwrap().stop(immediate());
        }
        // Cached bounds include cubic overshoot and panning: 8 * .055 + .5 < 1
        match manager.play(sound.volume(kira::Decibels(base.0 + gain))) {
            Ok(handle) => handles.push_back(handle),
            // Stopped slots retire on the audio callback; a peer burst may fill the pending queue
            Err(kira::PlaySoundError::SoundLimitReached) => break,
            Err(error) => return Err(format!("Cannot play feedback: {error}")),
        }
    }
    Ok(())
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
    fn music_headroom_covers_stereo_source_peaks_and_resampling() {
        for peak in [0.0, 0.5, 1.0, 3.0] {
            let song = sound_data(vec![Frame::new(peak * 0.7, -peak); 4]);
            let headroom = music_headroom(&song);
            assert!(headroom > 0.0 && headroom <= 0.4);
            assert!(peak * headroom * 1.25 <= 0.5 + f32::EPSILON);
            let at_max = kira::Decibels(music_decibels(100, headroom)).as_amplitude();
            assert!((at_max - headroom).abs() < 1e-6);
            assert_eq!(
                kira::Decibels(music_decibels(0, headroom)).as_amplitude(),
                0.0
            );
        }
    }

    #[test]
    fn feedback_burst_exhaustion_keeps_music_playing_and_recovers_after_callback() {
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: 48_000,
            },
            main_track_builder: kira::track::MainTrackBuilder::default().sound_capacity(128),
            ..Default::default()
        })
        .unwrap();
        let data = sound_data(vec![Frame::from_mono(0.03); 48_000]);
        let music = manager.play(data.clone()).unwrap();
        let mut handles = VecDeque::new();
        // No callback runs during the full peer batch; logical eviction cannot reclaim Kira slots yet
        for _ in 0..256 {
            play_feedback(&mut manager, &mut handles, vec![data.clone()], 80).unwrap();
            assert!(handles.len() <= MAX_FEEDBACK_VOICES);
        }
        assert_eq!(music.state(), PlaybackState::Playing);
        for _ in 0..3 {
            callback(&mut manager);
        }
        assert_eq!(music.state(), PlaybackState::Playing);
        assert!(music.position() > 0.0);
        play_feedback(&mut manager, &mut handles, vec![data], 80).unwrap();
        assert_eq!(handles.len(), MAX_FEEDBACK_VOICES);
    }

    #[test]
    fn replacement_stop_resets_palette_and_all_pending_feedback() {
        let context = FeedbackContext {
            event_id: 1,
            song_time: SongTime::ZERO,
            family: crate::feedback_audio::FeedbackTimbre::Wood,
            chord_mask: Some(0x091),
            short_tonal: false,
            energy: 0.7,
            density: 0.2,
            beat_seconds: 0.5,
        };
        let mut palette = FeedbackPalette::new();
        let mut manager = mock_audio();
        let mut handles = VecDeque::new();
        let first = palette.hit(PlayerId::P1, context);
        let initial = first[0].frames.clone();
        play_feedback(&mut manager, &mut handles, first, 80).unwrap();
        let next = palette.hit(PlayerId::P1, context);
        assert_ne!(initial.as_ref(), next[0].frames.as_ref());
        play_feedback(&mut manager, &mut handles, palette.duo(context, true), 80).unwrap();
        assert!(!handles.is_empty());
        // replace_song invokes stop, which uses this shared lifecycle operation
        reset_feedback(&mut palette, &mut handles);
        callback(&mut manager);
        assert!(handles.is_empty());
        assert_eq!(
            initial.as_ref(),
            palette.hit(PlayerId::P1, context)[0].frames.as_ref()
        );
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
    fn shared_music_sampler_reads_the_original_future_resume_without_control_access() {
        let (mut audio, sampler) = mock_source_sampler();
        callback(&mut audio);
        callback(&mut audio);
        let (initial, _) = sampler.read().unwrap().unwrap();
        {
            let mut music = sampler.music.lock().unwrap();
            music.paused = true;
            music.reconcile();
        }
        callback(&mut audio);
        callback(&mut audio);
        let (frozen, state) = sampler.read().unwrap().unwrap();
        assert_eq!(state, PlaybackState::Paused);
        sampler
            .music
            .lock()
            .unwrap()
            .resume_at(Instant::now() + Duration::from_secs(1))
            .unwrap();
        for _ in 0..100 {
            sampler.music.lock().unwrap().reconcile();
            callback(&mut audio);
            let (row, state) = sampler.read().unwrap().unwrap();
            assert_eq!(row.position_seconds, frozen.position_seconds);
            assert_eq!(state, PlaybackState::WaitingToResume);
        }
        for _ in 0..300 {
            sampler.music.lock().unwrap().reconcile();
            callback(&mut audio);
        }
        let (resumed, state) = sampler.read().unwrap().unwrap();
        assert_eq!(state, PlaybackState::Playing);
        assert_eq!(
            (resumed.generation, resumed.source_id),
            (initial.generation, initial.source_id)
        );
        assert!(resumed.sequence > frozen.sequence);
        assert!(resumed.position_seconds > frozen.position_seconds);
        let guard = sampler.music.lock().unwrap();
        assert!(sampler.read().unwrap().is_none());
        drop(guard);
    }

    #[test]
    fn delayed_resume_keeps_the_original_source_and_reconcile_preserves_its_delay() {
        let mut observations = AudioObservations::new();
        let mut audio = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: 48_000,
            },
            main_track_builder: observations.main_track(),
            ..Default::default()
        })
        .unwrap();
        let (handle, source) = audio
            .play(observations.sound(sound_data(vec![Frame::ZERO; 48_000])))
            .unwrap();
        let mut sound = ControlledSound::new(handle);
        callback(&mut audio);
        callback(&mut audio);
        sound.paused = true;
        sound.reconcile();
        callback(&mut audio);
        callback(&mut audio);
        let frozen = source.read(&sound.handle).unwrap();
        assert!(frozen.position_seconds > 0.0);
        assert_eq!(sound.handle.state(), PlaybackState::Paused);

        // An acknowledged pending pause does not need another UI frame to be cleared
        assert_eq!(sound.pending, Some(PlaybackState::Paused));
        sound
            .resume_at(Instant::now() + Duration::from_secs(1))
            .unwrap();
        assert_eq!(sound.pending, Some(PlaybackState::Playing));
        assert_eq!(sound.state(), PlaybackState::Resuming);
        assert!(
            sound
                .resume_at(Instant::now() + Duration::from_secs(1))
                .is_err()
        );
        for _ in 0..100 {
            sound.reconcile();
            callback(&mut audio);
            assert_eq!(sound.handle.state(), PlaybackState::WaitingToResume);
            assert_eq!(sound.state(), PlaybackState::Resuming);
            assert_eq!(sound.handle.position(), frozen.position_seconds);
        }
        for _ in 0..300 {
            sound.reconcile();
            callback(&mut audio);
        }
        sound.reconcile();
        let resumed = source.read(&sound.handle).unwrap();
        assert_eq!(sound.state(), PlaybackState::Playing);
        assert_eq!(sound.pending, None);
        assert_eq!(resumed.generation, frozen.generation);
        assert_eq!(resumed.source_id, frozen.source_id);
        assert!(resumed.sequence > frozen.sequence);
        assert!(resumed.position_seconds > frozen.position_seconds);
    }

    #[test]
    fn delayed_resume_rejects_unacknowledged_controls_and_late_deadlines() {
        let mut audio = mock_audio();
        let mut sound =
            ControlledSound::new(audio.play(sound_data(vec![Frame::ZERO; 48_000])).unwrap());
        let deadline = || Instant::now() + Duration::from_secs(1);
        callback(&mut audio);
        assert!(sound.resume_at(deadline()).is_err());
        sound.paused = true;
        sound.reconcile();
        assert!(sound.resume_at(deadline()).is_err());
        callback(&mut audio);
        assert_eq!(sound.handle.state(), PlaybackState::Paused);
        for deadline in [
            Instant::now() - Duration::from_secs(1),
            Instant::now(),
            Instant::now() + Duration::from_millis(99),
        ] {
            assert!(sound.resume_at(deadline).is_err());
            assert!(sound.paused);
            assert_eq!(sound.pending, Some(PlaybackState::Paused));
        }
        // A queued immediate resume remains pending even if a later pause restores the intent
        sound.paused = false;
        sound.reconcile();
        sound.paused = true;
        assert_eq!(sound.handle.state(), PlaybackState::Paused);
        assert!(sound.resume_at(deadline()).is_err());
        assert_eq!(sound.pending, Some(PlaybackState::Playing));
    }

    #[test]
    fn terminal_stop_cancels_delayed_resume_before_and_after_callback_ack() {
        for consume_resume in [false, true] {
            let mut audio = mock_audio();
            let mut sound =
                ControlledSound::new(audio.play(sound_data(vec![Frame::ZERO; 48_000])).unwrap());
            callback(&mut audio);
            sound.paused = true;
            sound.reconcile();
            callback(&mut audio);
            sound
                .resume_at(Instant::now() + Duration::from_secs(1))
                .unwrap();
            if consume_resume {
                callback(&mut audio);
                assert_eq!(sound.handle.state(), PlaybackState::WaitingToResume);
            }
            sound.handle.stop(immediate());
            callback(&mut audio);
            let stopped_at = sound.handle.position();
            for _ in 0..400 {
                sound.reconcile();
                callback(&mut audio);
            }
            assert_eq!(sound.state(), PlaybackState::Stopped);
            assert_eq!(sound.pending, None);
            assert_eq!(sound.handle.position(), stopped_at);
            assert!(
                sound
                    .resume_at(Instant::now() + Duration::from_secs(1))
                    .is_err()
            );
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
