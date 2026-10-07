//! Network I/O and package decoding stay off the presentation thread

use crate::content::{self, SongContent};
use cocobeat_net::{LiveCommand, LiveConfig, LiveEvent, LiveRole, LiveSession};
use cocobeat_schema::{PlayerId, SessionEpoch};
use kira::sound::static_sound::StaticSoundData;
use std::{collections::VecDeque, path::PathBuf, sync::mpsc, time::Instant};

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
    next_rounds: VecDeque<LiveConfig>,
    worker: Option<LiveSession>,
    loading: Option<mpsc::Receiver<Result<PreparedSong, String>>>,
    pub player: Option<PlayerId>,
    pub spent: bool,
    pub started: bool,
    pub local_ended: bool,
    pub deadline: Option<Instant>,
    terminal: bool,
    completed: bool,
}

impl OnlineRound {
    pub fn new(config: LiveConfig, next_rounds: Vec<(PathBuf, PathBuf)>) -> Self {
        let next_rounds = next_rounds
            .into_iter()
            .map(|(invite, output)| LiveConfig {
                role: match &config.role {
                    LiveRole::Host { package, bind, .. } => LiveRole::Host {
                        package: package.clone(),
                        bind: *bind,
                        invite,
                    },
                    LiveRole::Join { package, .. }
                    | LiveRole::Receive {
                        package_destination: package,
                        ..
                    } => LiveRole::Join {
                        package: package.clone(),
                        invite,
                    },
                },
                output,
            })
            .collect();
        Self {
            config: Some(config),
            next_rounds,
            ..Self::default()
        }
    }

    pub fn complete(&mut self) {
        self.stop();
        self.completed = true;
    }

    pub fn can_start_next(&self) -> bool {
        self.completed
            && self.is_finished()
            && !self.next_rounds.is_empty()
            && !self.waiting_for_next_invite()
    }

    pub fn waiting_for_next_invite(&self) -> bool {
        if !self.completed || !self.is_finished() {
            return false;
        }
        let Some(LiveConfig {
            role: LiveRole::Join { invite, .. },
            ..
        }) = self.next_rounds.front()
        else {
            return false;
        };
        std::fs::symlink_metadata(invite)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    }

    pub fn start_next(&mut self) -> Result<(), String> {
        if !self.can_start_next() {
            return Err(
                "The completed network round must finish before starting a queued round".into(),
            );
        }
        let config = self.next_rounds.pop_front().unwrap();
        *self = Self {
            config: Some(config),
            next_rounds: std::mem::take(&mut self.next_rounds),
            ..Self::default()
        };
        self.start()
    }

    pub fn remaining_rounds(&self) -> usize {
        self.next_rounds.len()
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
        path: PathBuf,
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
        self.completed = false;
        self.loading = None;
    }

    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(LiveSession::is_finished)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_configs_keep_the_package_and_use_separate_invitations_and_outputs() {
        let bind = "127.0.0.1:4200".parse().unwrap();
        for (role, expected_package, host) in [
            (
                LiveRole::Host {
                    package: "source".into(),
                    bind,
                    invite: "first-invite".into(),
                },
                "source",
                true,
            ),
            (
                LiveRole::Join {
                    package: "source".into(),
                    invite: "first-invite".into(),
                },
                "source",
                false,
            ),
            (
                LiveRole::Receive {
                    package_destination: "received".into(),
                    invite: "first-invite".into(),
                },
                "received",
                false,
            ),
        ] {
            let mut round = OnlineRound::new(
                LiveConfig {
                    role,
                    output: "first-output".into(),
                },
                vec![
                    ("second-invite".into(), "second-output".into()),
                    ("third-invite".into(), "third-output".into()),
                ],
            );
            assert_eq!(round.remaining_rounds(), 2);
            let original = round.config.take().unwrap();
            assert_eq!(original.output, PathBuf::from("first-output"));
            assert!(round.config.is_none());
            for (next, (invite, output)) in round.next_rounds.iter().zip([
                ("second-invite", "second-output"),
                ("third-invite", "third-output"),
            ]) {
                assert_eq!(next.output, PathBuf::from(output));
                let (package, actual_invite) = match &next.role {
                    LiveRole::Host {
                        package,
                        bind: actual_bind,
                        invite,
                    } => {
                        assert!(host);
                        assert_eq!(*actual_bind, bind);
                        (package, invite)
                    }
                    LiveRole::Join { package, invite } => {
                        assert!(!host);
                        (package, invite)
                    }
                    LiveRole::Receive { .. } => panic!("A received package must be reused by Join"),
                };
                assert_eq!(package, &PathBuf::from(expected_package));
                assert_eq!(actual_invite, &PathBuf::from(invite));
            }
        }
    }

    #[test]
    fn only_completed_rounds_advance_and_a_failed_start_consumes_one_config() {
        let mut round = OnlineRound::new(
            LiveConfig {
                role: LiveRole::Host {
                    package: "package".into(),
                    bind: "127.0.0.1:4200".parse().unwrap(),
                    invite: "first-invite".into(),
                },
                output: "first-output".into(),
            },
            vec![
                // Missing file names fail before any filesystem or network worker work
                ("second-invite".into(), PathBuf::new()),
                ("third-invite".into(), "third-output".into()),
            ],
        );
        assert!(round.start_next().is_err());
        assert_eq!(round.remaining_rounds(), 2);
        assert!(round.config.is_some());
        round.config = None;
        round.spent = true;
        round.player = Some(PlayerId::P2);
        round.started = true;
        round.local_ended = true;
        round.deadline = Some(Instant::now());
        let (sender, receiver) = mpsc::sync_channel(1);
        assert!(sender.send(Err("old decoder result".into())).is_ok());
        round.loading = Some(receiver);
        round.stop();
        assert!(round.start_next().is_err());
        assert_eq!(round.remaining_rounds(), 2);
        round.complete();
        assert!(round.can_start_next());
        round.stop();
        assert!(!round.can_start_next());
        assert!(round.start_next().is_err());
        assert_eq!(round.remaining_rounds(), 2);
        round.complete();
        assert_eq!(
            round.start_next().unwrap_err(),
            "destination requires a file name"
        );
        assert_eq!(round.remaining_rounds(), 1);
        assert_eq!(round.next_rounds[0].output, PathBuf::from("third-output"));
        assert!(round.config.is_none());
        assert!(round.worker.is_none());
        assert!(round.loading.is_none());
        assert!(round.player.is_none());
        assert!(round.spent);
        assert!(!round.started);
        assert!(!round.local_ended);
        assert!(round.deadline.is_none());
        assert!(!round.terminal);
        assert!(!round.completed);
        assert!(round.poll().unwrap().is_none());
        assert!(round.start_next().is_err());
        assert_eq!(round.remaining_rounds(), 1);
    }

    #[test]
    fn the_last_completed_round_has_no_next_action() {
        let mut round = OnlineRound::default();
        round.complete();
        assert_eq!(round.remaining_rounds(), 0);
        assert!(!round.can_start_next());
        assert!(round.start_next().is_err());
        assert_eq!(round.remaining_rounds(), 0);
    }

    #[test]
    fn guests_wait_for_missing_invites_without_consuming_the_next_round() {
        let root = std::env::temp_dir().join(format!(
            "cocobeat-next-invite-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let invite = root.join("next-invite.json");
        let mut rounds = [
            LiveRole::Host {
                package: "package".into(),
                bind: "127.0.0.1:4200".parse().unwrap(),
                invite: "first-invite".into(),
            },
            LiveRole::Join {
                package: "package".into(),
                invite: "first-invite".into(),
            },
            LiveRole::Receive {
                package_destination: "received-package".into(),
                invite: "first-invite".into(),
            },
        ]
        .map(|role| {
            OnlineRound::new(
                LiveConfig {
                    role,
                    output: "first-output".into(),
                },
                vec![(invite.clone(), root.join("next-output"))],
            )
        });
        for (index, round) in rounds.iter_mut().enumerate() {
            assert!(!round.waiting_for_next_invite());
            round.complete();
            assert_eq!(round.waiting_for_next_invite(), index != 0);
            assert_eq!(round.can_start_next(), index == 0);
            if index != 0 {
                assert!(round.start_next().is_err());
            }
            assert_eq!(round.remaining_rounds(), 1);
        }

        // Existing content remains the network worker's validation responsibility
        std::fs::write(&invite, b"not an invitation").unwrap();
        for round in &rounds {
            assert!(!round.waiting_for_next_invite());
            assert!(round.can_start_next());
            assert_eq!(round.remaining_rounds(), 1);
        }
        #[cfg(unix)]
        {
            std::fs::remove_file(&invite).unwrap();
            std::os::unix::fs::symlink(root.join("missing-target"), &invite).unwrap();
            for round in &rounds {
                assert!(!round.waiting_for_next_invite());
                assert!(round.can_start_next());
                assert_eq!(round.remaining_rounds(), 1);
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
