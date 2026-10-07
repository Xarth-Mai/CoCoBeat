//! Network I/O and package decoding stay off the presentation thread

use crate::content::{self, SongContent};
use cocobeat_net::{LiveCommand, LiveConfig, LiveEvent, LiveSession};
use cocobeat_schema::{PlayerId, SessionEpoch};
use kira::sound::static_sound::StaticSoundData;
use std::{sync::mpsc, time::Instant};

pub(crate) struct PreparedSong {
    pub epoch: SessionEpoch,
    pub player: PlayerId,
    pub final_through: i64,
    pub content: SongContent,
    pub sound: StaticSoundData,
}

pub(crate) enum Update {
    Network(LiveEvent),
    Song(Box<PreparedSong>),
}

#[derive(Default)]
pub(crate) struct OnlineRound {
    config: Option<LiveConfig>,
    worker: Option<LiveSession>,
    loading: Option<mpsc::Receiver<Result<PreparedSong, String>>>,
    pub player: Option<PlayerId>,
    pub spent: bool,
    pub started: bool,
    pub local_ended: bool,
    pub deadline: Option<Instant>,
    terminal: bool,
}

impl OnlineRound {
    pub fn new(config: LiveConfig) -> Self {
        Self {
            config: Some(config),
            ..Self::default()
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.is_some() || self.spent
    }

    pub fn start(&mut self) -> Result<(), String> {
        let config = self
            .config
            .take()
            .ok_or("This invitation has already been used")?;
        self.spent = true;
        self.worker = Some(LiveSession::spawn(config)?);
        Ok(())
    }

    pub fn send(&self, command: LiveCommand) -> Result<(), String> {
        self.worker
            .as_ref()
            .ok_or("No active network round")?
            .try_send(command)
            .map_err(|error| error.to_string())
    }

    pub fn poll(&mut self) -> Result<Option<Update>, String> {
        if self.terminal {
            return Ok(None);
        }
        if let Some(loader) = &self.loading {
            match loader.try_recv() {
                Ok(result) => {
                    self.loading = None;
                    return result.map(|song| Some(Update::Song(Box::new(song))));
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => return Err("Song decoder stopped".into()),
            }
        }
        let Some(worker) = &self.worker else {
            return Ok(None);
        };
        match worker.try_recv() {
            Ok(event) => Ok(Some(Update::Network(event))),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Network worker stopped without a result".into())
            }
        }
    }

    pub fn load(
        &mut self,
        epoch: SessionEpoch,
        player: PlayerId,
        path: std::path::PathBuf,
        content_id: String,
        canonical_frames: u64,
        final_through: i64,
    ) -> Result<(), String> {
        if self.loading.is_some() || self.player.is_some() {
            return Err("Duplicate network package preparation".into());
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("cocobeat-song-decode".into())
            .spawn(move || {
                let result = content::load_package(&path).and_then(|(content, sound)| {
                    if content.content_id != content_id
                        || content.end.frames() as u64 != canonical_frames
                    {
                        return Err(
                            "Playback package differs from the validated network identity".into(),
                        );
                    }
                    Ok(PreparedSong {
                        epoch,
                        player,
                        final_through,
                        content,
                        sound,
                    })
                });
                let _ = sender.send(result);
            })
            .map_err(|error| format!("Start song decoder: {error}"))?;
        self.loading = Some(receiver);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(worker) = &mut self.worker {
            worker.cancel();
        }
        self.terminal = true;
        self.loading = None;
    }

    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(LiveSession::is_finished)
    }
}
