//! Audition positions are callback source cursors, never measured device or input latency

use super::*;
use cocobeat_runtime::{AudioOutput, sound_data};
use kira::sound::{PlaybackState, static_sound::StaticSoundData};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Command {
    Play(Option<i64>),
    Pause,
    Stop,
}

#[derive(Default)]
pub(super) struct Audition {
    command: Option<Command>,
    pub playing: bool,
    pub position: Option<i64>,
    pub target: Option<i64>,
    pub status: &'static str,
    started: bool,
}

impl Audition {
    pub fn toggle(&mut self, cursor: i64, end: i64) -> Result<(), String> {
        if self.playing {
            self.pause();
        } else {
            let at = if !self.started || self.target.is_none() {
                let at = self.target.unwrap_or(cursor);
                require_frame(at, end)?;
                self.target = Some(at);
                Some(at)
            } else {
                None
            };
            self.command = Some(Command::Play(at));
            self.playing = true;
            self.status = "audition.resuming";
        }
        Ok(())
    }

    pub fn seek(&mut self, cursor: i64, end: i64) -> Result<(), String> {
        require_frame(cursor, end)?;
        self.target = Some(cursor);
        self.started = false;
        if self.playing {
            self.command = Some(Command::Play(Some(cursor)));
            self.status = "audition.resuming";
        }
        Ok(())
    }

    pub fn pause(&mut self) {
        if self.playing {
            self.playing = false;
            self.command = Some(Command::Pause);
            self.status = "audition.pausing";
        }
    }

    pub fn stop(&mut self) {
        self.playing = false;
        self.command = Some(Command::Stop);
        self.position = None;
        self.target = None;
        self.status = "audition.stopped";
        self.started = false;
    }

    fn fault(&mut self) {
        self.stop();
        self.command = None;
        self.status = "audition.failed";
    }

    pub fn status_key(&self) -> &'static str {
        if self.status.is_empty() {
            "audition.stopped"
        } else {
            self.status
        }
    }
}

fn require_frame(frame: i64, end: i64) -> Result<(), String> {
    if !(0..end).contains(&frame) {
        return Err("Audition selection must be inside the canonical song (0 <= frame < N)".into());
    }
    Ok(())
}

pub(super) struct Output {
    data: StaticSoundData,
    audio: Option<AudioOutput>,
}

impl Output {
    pub fn new(frames: Vec<kira::Frame>) -> Self {
        Self {
            data: sound_data(frames),
            audio: None,
        }
    }

    fn command(&mut self, command: Command) -> Result<(), String> {
        if matches!(command, Command::Play(Some(_))) && self.audio.is_none() {
            self.audio = Some(AudioOutput::new(Some(self.data.clone()))?);
        }
        if command == Command::Play(None)
            && self.audio.as_ref().and_then(AudioOutput::state).is_none()
        {
            return Err("No active audition to resume".into());
        }
        if let Some(audio) = &mut self.audio {
            match command {
                Command::Play(Some(frame)) => audio.start_at(SongTime::from_frames(frame))?,
                Command::Play(None) => audio.resume(),
                Command::Pause => audio.pause(),
                Command::Stop => audio.stop(),
            }
        }
        Ok(())
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if let Some(audio) = &mut self.audio {
            audio.stop();
        }
    }
}

pub(super) fn update(mut state: ResMut<Workbench>, mut output: NonSendMut<Output>) {
    if let Some(command) = state.audition.command.take() {
        if let Err(error) = output.command(command) {
            if let Some(audio) = &mut output.audio {
                audio.stop();
            }
            output.audio = None;
            state.audition.fault();
            state.error(error);
            return;
        }
        if matches!(command, Command::Play(Some(_))) {
            state.audition.started = true;
            // A new handle's initial position is not yet a callback observation
            state.audition.position = None;
        }
    }
    let Some(audio) = &mut output.audio else {
        state.audition.playing = false;
        state.audition.started = false;
        if state.audition.status != "audition.failed" {
            state.audition.status = "audition.stopped";
        }
        return;
    };
    if let Some(error) = audio.take_error() {
        audio.stop();
        output.audio = None;
        state.audition.fault();
        state.error(error);
        return;
    }
    audio.reconcile_playback();
    let playback = audio.state();
    if let Some(seconds) = audio.position()
        && seconds.is_finite()
        && seconds >= 0.0
        && (state.audition.position.is_some()
            || Some(PlaybackState::Playing) != playback
            || state
                .audition
                .target
                .is_some_and(|target| seconds > target as f64 / 48_000.0))
        && matches!(
            playback,
            Some(PlaybackState::Playing | PlaybackState::Paused | PlaybackState::Stopped)
        )
    {
        state.audition.position = Some((seconds * 48_000.0).floor() as i64);
    }
    state.audition.status = match playback {
        Some(PlaybackState::Playing) if state.audition.position.is_none() => "audition.resuming",
        Some(PlaybackState::Playing) => "audition.playing",
        Some(PlaybackState::Paused) => {
            state.audition.playing = false;
            "audition.paused"
        }
        Some(PlaybackState::Pausing) => "audition.pausing",
        Some(PlaybackState::Resuming) => "audition.resuming",
        Some(PlaybackState::Stopped) => {
            state.audition.playing = false;
            state.audition.started = false;
            state.audition.target = None;
            "audition.ended"
        }
        _ => {
            state.audition.playing = false;
            state.audition.started = false;
            "audition.stopped"
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audition_controls_keep_requested_seek_and_raw_diagnostics_separate() {
        let mut audition = Audition::default();
        for cursor in [-1, 48_000, i64::MAX] {
            assert!(audition.toggle(cursor, 48_000).is_err());
            assert!(audition.seek(cursor, 48_000).is_err());
            assert!(!audition.playing);
            assert!(audition.target.is_none());
        }
        audition.toggle(1_200, 48_000).unwrap();
        audition.pause();
        assert_eq!(audition.command.take(), Some(Command::Pause));
        audition.toggle(-1, 48_000).unwrap();
        assert_eq!(audition.command.take(), Some(Command::Play(Some(1_200))));
        audition.started = true;
        audition.position = Some(1_600);
        audition.pause();
        assert_eq!(audition.command.take(), Some(Command::Pause));
        audition.seek(24_000, 48_000).unwrap();
        assert!(audition.command.is_none());
        assert_eq!(audition.position, Some(1_600));
        audition.toggle(-1, 48_000).unwrap();
        assert_eq!(audition.command.take(), Some(Command::Play(Some(24_000))));
        audition.started = true;
        audition.pause();
        audition.command = None;
        audition.toggle(-1, 48_000).unwrap();
        assert_eq!(audition.command.take(), Some(Command::Play(None)));
        audition.stop();
        assert_eq!(audition.command, Some(Command::Stop));
        assert!(audition.target.is_none());
        assert!(audition.position.is_none());
        audition.fault();
        assert_eq!(audition.status_key(), "audition.failed");
    }
}
