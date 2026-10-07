//! Software observations from public Kira hooks, not device or speaker timestamps

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::SeqCst},
    },
    time::{Duration, Instant},
};

use kira::{
    Frame,
    effect::Effect,
    info::Info,
    sound::{
        Sound, SoundData,
        static_sound::{StaticSoundData, StaticSoundHandle},
    },
    track::MainTrackBuilder,
};

/// A main-mix hook reached inside a backend callback, after source publication
/// The current callback is not sealed until the next hook invocation
#[derive(Debug, Clone, Copy)]
pub struct CallbackObservation {
    /// Application observation lifetime, not a native stream identifier
    pub generation: u64,
    pub sequence: u64,
    pub observed_at: Instant,
    pub previous_observed_at: Option<Instant>,
    /// Sum of internal chunk lengths in the previous completed callback
    /// Dividing by output_sample_rate gives its rendered duration, not wall time or latency
    pub previous_frames: Option<u64>,
    pub output_sample_rate: u32,
}

/// One coherent Kira source-position publication and the Instant interval containing it
/// PlaybackState changes elsewhere and is deliberately not part of this snapshot
#[derive(Debug, Clone, Copy)]
pub struct SourceObservation {
    pub generation: u64,
    /// Unique within the generation, including replacements of the same song
    pub source_id: u64,
    pub sequence: u64,
    pub position_seconds: f64,
    pub published_between: [Instant; 2],
}

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

pub(super) struct AudioObservations {
    shared: Arc<Shared>,
    next_source: u64,
}

struct Shared {
    origin: Instant,
    generation: u64,
    valid: AtomicBool,
    sequence: AtomicU64,
    observed_ns: AtomicU64,
    previous_ns: AtomicU64,
    previous_frames: AtomicU64,
    sample_rate: AtomicU32,
}

impl AudioObservations {
    pub(super) fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                origin: Instant::now(),
                generation: NEXT_GENERATION.fetch_add(1, SeqCst),
                valid: AtomicBool::new(true),
                sequence: AtomicU64::new(0),
                observed_ns: AtomicU64::new(0),
                previous_ns: AtomicU64::new(0),
                previous_frames: AtomicU64::new(0),
                sample_rate: AtomicU32::new(0),
            }),
            next_source: 0,
        }
    }

    pub(super) fn main_track(&self) -> MainTrackBuilder {
        let mut track = MainTrackBuilder::new();
        track.add_built_effect(Box::new(CallbackObserver {
            shared: self.shared.clone(),
            frames: 0,
        }));
        track
    }

    pub(super) fn sound(&mut self, data: StaticSoundData) -> ObservedSoundData {
        self.next_source += 1;
        ObservedSoundData {
            data,
            source: Arc::new(SourceShared {
                owner: self.shared.clone(),
                id: self.next_source,
                sequence: AtomicU64::new(0),
                before_ns: AtomicU64::new(0),
                after_ns: AtomicU64::new(0),
            }),
        }
    }

    /// Invalid for the remainder of this AudioOutput lifetime, even if playback recovers
    pub(super) fn invalidate(&self) {
        self.shared.valid.store(false, SeqCst);
    }

    pub(super) fn callback(&self) -> Option<CallbackObservation> {
        let shared = &self.shared;
        shared.read(&shared.sequence, |sequence| CallbackObservation {
            generation: shared.generation,
            sequence,
            observed_at: shared.instant(shared.observed_ns.load(SeqCst)),
            previous_observed_at: (sequence > 1)
                .then(|| shared.instant(shared.previous_ns.load(SeqCst))),
            previous_frames: (sequence > 1).then(|| shared.previous_frames.load(SeqCst)),
            output_sample_rate: shared.sample_rate.load(SeqCst),
        })
    }
}

impl Shared {
    fn elapsed_ns(&self) -> u64 {
        self.origin.elapsed().as_nanos().min(u64::MAX as u128) as u64
    }

    fn instant(&self, elapsed_ns: u64) -> Instant {
        self.origin + Duration::from_nanos(elapsed_ns)
    }

    // All shared fields and Kira's position are SeqCst atomics; no non-atomic seqlock access
    // A busy writer gives an unavailable observation after three attempts, never a blocking read
    fn read<T>(&self, sequence: &AtomicU64, read: impl Fn(u64) -> T) -> Option<T> {
        for _ in 0..3 {
            let before = sequence.load(SeqCst);
            if before == 0 || !before.is_multiple_of(2) || !self.valid.load(SeqCst) {
                continue;
            }
            let value = read(before / 2);
            if before == sequence.load(SeqCst) && self.valid.load(SeqCst) {
                return Some(value);
            }
        }
        None
    }
}

struct CallbackObserver {
    shared: Arc<Shared>,
    frames: u64,
}

impl Effect for CallbackObserver {
    fn init(&mut self, sample_rate: u32, _: usize) {
        self.shared.sample_rate.store(sample_rate, SeqCst);
    }

    fn on_change_sample_rate(&mut self, _: u32) {
        self.shared.valid.store(false, SeqCst);
    }

    fn on_start_processing(&mut self) {
        let shared = &self.shared;
        shared.sequence.fetch_add(1, SeqCst);
        shared
            .previous_ns
            .store(shared.observed_ns.load(SeqCst), SeqCst);
        shared.observed_ns.store(shared.elapsed_ns(), SeqCst);
        shared.previous_frames.store(self.frames, SeqCst);
        self.frames = 0;
        shared.sequence.fetch_add(1, SeqCst);
    }

    fn process(&mut self, input: &mut [Frame], _: f64, _: &Info) {
        self.frames += input.len() as u64;
    }
}

struct SourceShared {
    owner: Arc<Shared>,
    id: u64,
    sequence: AtomicU64,
    before_ns: AtomicU64,
    after_ns: AtomicU64,
}

#[derive(Clone)]
pub(super) struct SourceReader(Arc<SourceShared>);

impl SourceReader {
    pub(super) fn read(&self, handle: &StaticSoundHandle) -> Option<SourceObservation> {
        let shared = &self.0;
        shared
            .owner
            .read(&shared.sequence, |sequence| SourceObservation {
                generation: shared.owner.generation,
                source_id: shared.id,
                sequence,
                position_seconds: handle.position(),
                published_between: [
                    shared.owner.instant(shared.before_ns.load(SeqCst)),
                    shared.owner.instant(shared.after_ns.load(SeqCst)),
                ],
            })
    }
}

pub(super) struct ObservedSoundData {
    data: StaticSoundData,
    source: Arc<SourceShared>,
}

impl SoundData for ObservedSoundData {
    type Error = <StaticSoundData as SoundData>::Error;
    type Handle = (StaticSoundHandle, SourceReader);

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let (inner, handle) = self.data.into_sound()?;
        let reader = SourceReader(self.source.clone());
        Ok((
            Box::new(ObservedSound {
                inner,
                source: self.source,
            }),
            (handle, reader),
        ))
    }
}

struct ObservedSound {
    inner: Box<dyn Sound>,
    source: Arc<SourceShared>,
}

impl Sound for ObservedSound {
    fn on_start_processing(&mut self) {
        let shared = &self.source;
        shared.sequence.fetch_add(1, SeqCst);
        shared.before_ns.store(shared.owner.elapsed_ns(), SeqCst);
        self.inner.on_start_processing();
        shared.after_ns.store(shared.owner.elapsed_ns(), SeqCst);
        shared.sequence.fetch_add(1, SeqCst);
    }

    fn process(&mut self, out: &mut [Frame], dt: f64, info: &Info) {
        self.inner.process(out, dt, info);
    }

    fn finished(&self) -> bool {
        self.inner.finished()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::{
        AudioManager, AudioManagerSettings, Tween,
        backend::mock::{MockBackend, MockBackendSettings},
        info::MockInfoBuilder,
    };

    fn mock_audio(observations: &AudioObservations) -> AudioManager<MockBackend> {
        AudioManager::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: 48_000,
            },
            main_track_builder: observations.main_track(),
            internal_buffer_size: 128,
            ..Default::default()
        })
        .unwrap()
    }

    fn data() -> StaticSoundData {
        super::super::sound_data(
            (0..4_800)
                .map(|frame| Frame::from_mono(frame as f32 / 4_800.0))
                .collect(),
        )
    }

    #[test]
    fn mock_callbacks_seal_all_chunks_and_pair_source_publication_with_its_cursor() {
        let mut observations = AudioObservations::new();
        let mut audio = mock_audio(&observations);
        let (handle, source) = audio.play(observations.sound(data())).unwrap();
        assert!(observations.callback().is_none());
        assert!(source.read(&handle).is_none());
        assert_eq!(handle.position(), 0.0);

        let before = Instant::now();
        audio.backend_mut().on_start_processing();
        let first = observations.callback().unwrap();
        assert_eq!(first.sequence, 1);
        assert_eq!(first.previous_frames, None);
        assert_eq!(first.previous_observed_at, None);
        let published = source.read(&handle).unwrap();
        assert_eq!(published.generation, first.generation);
        assert_eq!(published.position_seconds, handle.position());
        assert!(before <= published.published_between[0]);
        assert!(published.published_between[0] <= published.published_between[1]);
        assert!(published.published_between[1] <= first.observed_at);
        for _ in 0..3 {
            audio.backend_mut().process();
        }
        // Rendering has no source-position publication, and the current callback stays unsealed
        assert_eq!(source.read(&handle).unwrap().sequence, published.sequence);
        assert_eq!(observations.callback().unwrap().previous_frames, None);
        audio.backend_mut().on_start_processing();
        let second = observations.callback().unwrap();
        assert_eq!(second.sequence, 2);
        assert_eq!(second.previous_observed_at, Some(first.observed_at));
        assert_eq!(second.previous_frames, Some(384));
        assert_eq!(second.output_sample_rate, 48_000);
        let published = source.read(&handle).unwrap();
        assert_eq!(published.sequence, 2);
        assert_eq!(published.position_seconds, handle.position());
        assert!(published.position_seconds > 0.0);

        // The main mix still observes output after this sound is paused
        let mut handle = handle;
        handle.pause(Tween {
            duration: Duration::ZERO,
            ..Default::default()
        });
        audio.backend_mut().process();
        audio.backend_mut().on_start_processing();
        audio.backend_mut().process();
        assert_eq!(handle.state(), kira::sound::PlaybackState::Paused);
        audio.backend_mut().on_start_processing();
        assert_eq!(observations.callback().unwrap().previous_frames, Some(128));
    }

    #[test]
    fn source_instances_are_independent_and_busy_or_invalidated_snapshots_are_unavailable() {
        let mut observations = AudioObservations::new();
        let mut audio = mock_audio(&observations);
        let (first, first_source) = audio.play(observations.sound(data())).unwrap();
        audio.backend_mut().on_start_processing();
        audio.backend_mut().process();
        let first_id = first_source.read(&first).unwrap().source_id;
        let (second, second_source) = audio.play(observations.sound(data())).unwrap();
        assert!(second_source.read(&second).is_none());
        audio.backend_mut().on_start_processing();
        assert_ne!(second_source.read(&second).unwrap().source_id, first_id);
        assert_eq!(second_source.read(&second).unwrap().sequence, 1);
        assert_eq!(first_source.read(&first).unwrap().sequence, 2);

        second_source.0.sequence.fetch_add(1, SeqCst);
        assert!(second_source.read(&second).is_none());
        second_source.0.sequence.fetch_add(1, SeqCst);
        observations.shared.sequence.fetch_add(1, SeqCst);
        assert!(observations.callback().is_none());
        observations.shared.sequence.fetch_add(1, SeqCst);
        observations.invalidate();
        audio.backend_mut().process();
        audio.backend_mut().on_start_processing();
        assert!(observations.callback().is_none());
        assert!(first_source.read(&first).is_none());
        assert!(second_source.read(&second).is_none());
        assert!(first.position() > 0.0);
        assert_ne!(
            AudioObservations::new().shared.generation,
            observations.shared.generation
        );
    }

    #[test]
    fn sample_rate_changes_invalidate_the_observation_lifetime() {
        let mut observations = AudioObservations::new();
        let mut audio = mock_audio(&observations);
        let (handle, source) = audio.play(observations.sound(data())).unwrap();
        audio.backend_mut().on_start_processing();
        assert!(observations.callback().is_some());
        assert!(source.read(&handle).is_some());
        audio.backend_mut().set_sample_rate(44_100);
        audio.backend_mut().on_start_processing();
        assert!(observations.callback().is_none());
        assert!(source.read(&handle).is_none());
    }

    #[test]
    fn delegates_preserve_pcm_timing_controls_and_completion() {
        let mut observations = AudioObservations::new();
        let data = data()
            .start_position(0.01)
            .start_time(Duration::from_millis(5));
        let (mut plain, mut plain_handle) = data.clone().into_sound().unwrap();
        let (mut observed, (mut observed_handle, _)) =
            observations.sound(data).into_sound().unwrap();
        let info = MockInfoBuilder::new().build();
        let immediate = Tween {
            duration: Duration::ZERO,
            ..Default::default()
        };
        for index in 0..60 {
            if index == 8 {
                plain_handle.pause(immediate);
                observed_handle.pause(immediate);
            }
            if index == 12 {
                plain_handle.resume(immediate);
                observed_handle.resume(immediate);
            }
            plain.on_start_processing();
            observed.on_start_processing();
            let mut expected = [Frame::ZERO; 128];
            let mut actual = expected;
            plain.process(&mut expected, 1.0 / 48_000.0, &info);
            observed.process(&mut actual, 1.0 / 48_000.0, &info);
            assert_eq!(actual, expected);
            assert_eq!(plain_handle.position(), observed_handle.position());
            assert_eq!(plain_handle.state(), observed_handle.state());
            assert_eq!(plain.finished(), observed.finished());
        }
        assert!(plain.finished());
        let mut effect = CallbackObserver {
            shared: observations.shared,
            frames: 0,
        };
        let mut pcm = [Frame::from_mono(0.375); 128];
        effect.process(&mut pcm, 1.0 / 48_000.0, &info);
        assert_eq!(pcm, [Frame::from_mono(0.375); 128]);
    }
}
