//! Network I/O and package decoding stay off the presentation thread

use crate::content::{self, SongContent};
use crate::{audio::AudioOutput, session::Session};
use cocobeat_net::{LiveCommand, LiveConfig, LiveEvent, LiveRole, LiveSession};
use cocobeat_schema::{DuoEvent, PlayerId, SessionEpoch, SongTime};
use kira::sound::static_sound::StaticSoundData;
use std::{collections::VecDeque, path::PathBuf, sync::mpsc, time::Instant};

#[path = "recovery.rs"]
mod recovery;
use recovery::{PhaseMaintenance, Recovery};

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
    loading_thread: Option<std::thread::JoinHandle<()>>,
    pub player: Option<PlayerId>,
    pub spent: bool,
    pub started: bool,
    pub local_ended: bool,
    pub deadline: Option<Instant>,
    terminal: bool,
    recovery: Option<Recovery>,
    source_phase: Option<PhaseMaintenance>,
    phase_observations: Option<Vec<serde_json::Value>>,
    pub(crate) network_clock: Option<cocobeat_net::clock::ClockSync>,
    network_clock_round: u64,
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
                    LiveRole::Join { package, .. } => LiveRole::Join {
                        package: package.clone(),
                        invite,
                    },
                    LiveRole::Receive {
                        package_destination,
                        ..
                    } => LiveRole::Receive {
                        package_destination: package_destination.clone(),
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

    pub fn can_start_next(&self) -> bool {
        self.terminal
            && self.spent
            && self.is_finished()
            && !self.next_rounds.is_empty()
            && !self.waiting_for_next_invite()
    }

    pub fn waiting_for_next_invite(&self) -> bool {
        if !self.terminal || !self.spent || !self.is_finished() {
            return false;
        }
        let Some(LiveConfig {
            role: LiveRole::Join { invite, .. } | LiveRole::Receive { invite, .. },
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
                "The previous network round must finish before starting a queued round".into(),
            );
        }
        let mut config = self.next_rounds.pop_front().unwrap();
        if let LiveRole::Receive {
            package_destination,
            invite,
        } = &config.role
        {
            match std::fs::symlink_metadata(package_destination) {
                Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
                    config.role = LiveRole::Join {
                        package: package_destination.clone(),
                        invite: invite.clone(),
                    };
                }
                Ok(_) => return Err("Received package target must be a real directory".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("Inspect received package target: {error}")),
            }
        }

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

    /// Preserve the actual exchange; future source comparisons must still query freshness
    pub fn maintain_clock(
        &mut self,
        epoch: SessionEpoch,
        round: u64,
        exchange: cocobeat_net::clock::ClockExchange,
    ) -> Result<(), String> {
        if !self.started
            || self.terminal
            || self.player.is_none()
            || exchange.epoch != epoch
            || round == 0
            || round <= self.network_clock_round
        {
            return Err("Clock maintenance does not belong to this running network round".into());
        }
        if self.network_clock.is_none() {
            self.network_clock = Some(
                cocobeat_net::clock::ClockSync::new(epoch, Default::default())
                    .map_err(|error| error.to_string())?,
            );
        }
        self.network_clock
            .as_mut()
            .ok_or("Network clock disappeared")?
            .observe(exchange)
            .map_err(|error| error.to_string())?;
        self.network_clock_round = round;
        Ok(())
    }

    pub(super) fn enable_phase_observations(&mut self) {
        self.phase_observations = Some(Vec::new());
    }

    pub(super) fn phase_observations(&self) -> &[serde_json::Value] {
        self.phase_observations.as_deref().unwrap_or_default()
    }

    pub fn recovering(&self) -> bool {
        self.recovery.as_ref().is_some_and(Recovery::active)
            || self
                .source_phase
                .as_ref()
                .is_some_and(PhaseMaintenance::correcting)
    }

    /// RecoveryReady releases input only when no original Phase remains unresolved
    pub fn recovery_event(
        &mut self,
        event: LiveEvent,
        session: &mut Session,
        audio: &mut AudioOutput,
        end: SongTime,
        input_origin: Instant,
    ) -> Result<bool, String> {
        if !self.started || self.local_ended || self.terminal || self.player.is_none() {
            return Err("Recovery requires a started, unfinished network round".into());
        }
        let (epoch, attempt) = match &event {
            LiveEvent::RecoveryPausing { epoch, attempt, .. }
            | LiveEvent::RecoveryScheduled { epoch, attempt, .. }
            | LiveEvent::RecoverySampling { epoch, attempt, .. }
            | LiveEvent::RecoveryReady { epoch, attempt, .. } => (*epoch, *attempt),
            _ => return Err("Expected a network recovery event".into()),
        };
        if epoch != session.epoch() || attempt != 1 {
            return Err("Recovery epoch or attempt differs from the live session".into());
        }
        if let LiveEvent::RecoveryPausing {
            deadline,
            phase_round,
            ..
        } = event
        {
            if self.recovery.is_some() {
                return Err("This network round already attempted recovery".into());
            }
            let source = audio
                .source_observation()
                .ok_or("Recovery needs its original coherent publication")?;
            let now = Instant::now();
            let phase = self
                .source_phase
                .as_mut()
                .ok_or("Recovery needs its previously announced original phase source")?;
            if phase.end() != end {
                return Err("Recovery changed its original content duration".into());
            }
            let (recovery, snapshot) = phase.reconnect(
                session,
                (source, audio.state()),
                now,
                input_origin,
                deadline,
                phase_round,
            )?;
            if recovery.requests_pause() {
                audio.pause();
            }
            self.recovery = Some(recovery);
            if let Some(snapshot) = snapshot {
                self.send(snapshot)?;
            }
            return Ok(false);
        }
        let recovery = self.recovery.as_mut().ok_or("Recovery has not paused")?;
        match event {
            LiveEvent::RecoveryScheduled {
                deadline,
                verify_at,
                common_frame,
                ..
            } => {
                let source = audio.source_observation();
                let now = Instant::now();
                let state = audio.state();
                recovery.schedule(source, state, now, deadline, verify_at, common_frame)?;
                audio.resume_at(deadline)?;
                recovery.start_sampler(audio.source_sampler()?)?;
                self.send(LiveCommand::RecoveryArmed { epoch, attempt })?;
                Ok(false)
            }
            LiveEvent::RecoverySampling { not_before, .. } => {
                recovery.sampling(not_before, Instant::now())?;
                Ok(false)
            }
            LiveEvent::RecoveryReady { phase_round, .. } => {
                let source = audio.source_observation();
                let now = Instant::now();
                if let (Some(phase), Some(source)) = (&mut self.source_phase, source) {
                    phase.observe(source, now)?;
                }
                recovery.ready(source, audio.state(), now)?;
                self.source_phase
                    .as_mut()
                    .ok_or("Recovery lost its original phase source")?
                    .reconnect_ready(phase_round, now)
            }
            _ => unreachable!("recovery event was checked above"),
        }
    }

    /// Catchup records only real source-driven watermarks through the original fact FIFO
    pub fn update_recovery(
        &mut self,
        session: &mut Session,
        audio: &mut AudioOutput,
        input_origin: Instant,
    ) -> Result<Vec<DuoEvent>, String> {
        if !self.recovery.as_ref().is_some_and(Recovery::active)
            && self
                .source_phase
                .as_ref()
                .is_some_and(PhaseMaintenance::correcting)
        {
            return self.update_phase(session, audio, input_origin);
        }
        let player = self.player.ok_or("Recovery has no local player")?;
        let recovery = self.recovery.as_mut().ok_or("Recovery has not paused")?;
        let source = audio.source_observation();
        let now = Instant::now();
        if let (Some(phase), Some(source)) = (&mut self.source_phase, source) {
            phase.observe(source, now)?;
        }
        let was_waiting = recovery.waiting_for_resume();
        let (events, commands) =
            recovery.update(session, player, source, audio.state(), now, input_origin)?;
        if was_waiting && recovery.requests_pause() {
            audio.pause();
        }
        for command in commands {
            self.send(command)?;
        }
        Ok(events)
    }

    /// Announce the first actual Playing publication once; preserve identity across reconnect
    pub fn announce_phase_source(
        &mut self,
        session: &Session,
        audio: &AudioOutput,
        end: SongTime,
    ) -> Result<(), String> {
        if !self.started
            || self.local_ended
            || self.terminal
            || self.player.is_none()
            || audio.state() != Some(kira::sound::PlaybackState::Playing)
        {
            return Err("Phase source requires the unfinished acknowledged Playing round".into());
        }
        let source = audio
            .source_observation()
            .ok_or("Phase source needs its actual coherent publication")?;
        let now = Instant::now();
        if let Some(phase) = &mut self.source_phase {
            phase.observe(source, now)?;
        } else {
            let phase = PhaseMaintenance::new(session.epoch(), end, source, now)?;
            self.send(LiveCommand::PhaseSource {
                epoch: session.epoch(),
                generation: source.generation,
                source_id: source.source_id,
                publication: cocobeat_net::PhasePublication {
                    sequence: source.sequence,
                    position_seconds_bits: source.position_seconds.to_bits(),
                    published_between: source.published_between,
                },
            })?;
            self.source_phase = Some(phase);
        }
        Ok(())
    }

    /// Ordinary checks keep input open; correction and reconnect verification retain the gate
    pub fn phase_event(
        &mut self,
        event: LiveEvent,
        session: &mut Session,
        audio: &mut AudioOutput,
        input_origin: Instant,
    ) -> Result<bool, String> {
        if !self.started
            || self.local_ended
            || self.terminal
            || self.player.is_none()
            || self.recovery.as_ref().is_some_and(Recovery::active)
        {
            return Err("Phase requires an unfinished round outside authenticated recovery".into());
        }
        let (epoch, round, attempt) = match &event {
            LiveEvent::PhaseSampling {
                epoch,
                round,
                attempt,
                ..
            }
            | LiveEvent::PhasePausing {
                epoch,
                round,
                attempt,
            }
            | LiveEvent::PhaseScheduled {
                epoch,
                round,
                attempt,
                ..
            }
            | LiveEvent::PhaseReady {
                epoch,
                round,
                attempt,
            }
            | LiveEvent::PhaseRebound {
                epoch,
                round,
                attempt,
                ..
            } => (*epoch, *round, *attempt),
            _ => return Err("Expected an original-source phase event".into()),
        };
        if epoch != session.epoch() || round == 0 || round > 128 {
            return Err("Phase event differs from the original epoch or round budget".into());
        }
        let source = audio
            .source_observation()
            .ok_or("Phase event needs its original coherent publication")?;
        let now = Instant::now();
        let state = audio.state();
        let phase = self
            .source_phase
            .as_mut()
            .ok_or("Phase source has not been announced")?;
        if !matches!(event, LiveEvent::PhaseRebound { .. }) {
            phase.expect_attempt(round, attempt)?;
        }
        let observation = self.phase_observations.as_ref().map(|_| {
            let nanos = |at: Instant| {
                u64::try_from(at.duration_since(input_origin).as_nanos()).unwrap_or(u64::MAX)
            };
            let mut row = serde_json::json!({
                "epoch": epoch.0, "round": round, "attempt": attempt,
                "observed_ns": nanos(now),
                "source_generation": source.generation, "source_id": source.source_id,
                "source_sequence": source.sequence,
                "position_seconds_bits": source.position_seconds.to_bits(),
                "publication_before_ns": nanos(source.published_between[0]),
                "publication_after_ns": nanos(source.published_between[1]),
            });
            match &event {
                LiveEvent::PhaseSampling {
                    verification,
                    not_before,
                    common_at,
                    until,
                    ..
                } => {
                    row["event"] = "sampling".into();
                    row["verification"] = (*verification).into();
                    row["not_before_ns"] = nanos(*not_before).into();
                    row["common_at_ns"] = nanos(*common_at).into();
                    row["until_ns"] = nanos(*until).into();
                }
                LiveEvent::PhaseRebound { deadline, .. } => {
                    row["event"] = "rebound".into();
                    row["deadline_ns"] = nanos(*deadline).into();
                }
                LiveEvent::PhaseReady { .. } => row["event"] = "ready".into(),
                LiveEvent::PhasePausing { .. } => row["event"] = "pausing".into(),
                LiveEvent::PhaseScheduled { .. } => row["event"] = "scheduled".into(),
                _ => unreachable!("phase event was checked above"),
            }
            row
        });
        match event {
            LiveEvent::PhaseRebound { deadline, .. } => {
                phase.rebound(round, attempt, deadline, source, state, now)?;
            }
            LiveEvent::PhaseSampling {
                verification,
                not_before,
                common_at,
                until,
                ..
            } => {
                phase.sampling(
                    round,
                    verification,
                    [not_before, common_at, until],
                    (source, state),
                    audio.source_sampler()?,
                    now,
                )?;
            }
            LiveEvent::PhasePausing { .. } => {
                phase.pausing(round, session, source, state, now, input_origin)?;
                audio.pause();
            }
            LiveEvent::PhaseScheduled {
                deadline,
                verify_at,
                common_frame,
                ..
            } => {
                phase.schedule(
                    round,
                    source,
                    state,
                    now,
                    [deadline, verify_at],
                    common_frame,
                )?;
                audio.resume_at(deadline)?;
                self.send(LiveCommand::PhaseArmed {
                    epoch,
                    round,
                    attempt,
                })?;
            }
            LiveEvent::PhaseReady { .. } => {
                phase.ready(round, source, state, now)?;
            }
            _ => unreachable!("phase event was checked above"),
        }
        if let Some(row) = observation {
            let observations = self.phase_observations.as_mut().unwrap();
            if observations.len() >= 16 {
                return Err("Native phase observation exceeded its bound".into());
            }
            observations.push(row);
        }
        Ok(self
            .source_phase
            .as_ref()
            .is_some_and(PhaseMaintenance::correcting))
    }

    pub fn update_phase(
        &mut self,
        session: &mut Session,
        audio: &AudioOutput,
        input_origin: Instant,
    ) -> Result<Vec<DuoEvent>, String> {
        if self.terminal || self.local_ended || self.recovery.as_ref().is_some_and(Recovery::active)
        {
            return Err("Phase update cannot overlap End or authenticated recovery".into());
        }
        let player = self.player.ok_or("Phase has no local player")?;
        let phase = self
            .source_phase
            .as_mut()
            .ok_or("Phase original source is missing")?;
        let (events, commands) = phase.update(
            session,
            player,
            audio.source_observation(),
            audio.state(),
            Instant::now(),
            input_origin,
        )?;
        for command in commands {
            self.send(command)?;
        }
        Ok(events)
    }

    pub fn end_phase_sampling(&mut self) {
        if let Some(phase) = &mut self.source_phase {
            phase.stop_sampling();
        }
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

    pub fn load(&mut self, event: LiveEvent) -> Result<(), String> {
        let LiveEvent::Prepared {
            epoch,
            player,
            package_path: path,
            content_id,
            canonical_frames,
            stage_compiler_version,
            final_through,
        } = event
        else {
            return Err("Expected a validated network preparation event".into());
        };
        if self.loading_thread.is_some() || self.player.is_some() {
            return Err("Duplicate network package preparation".into());
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("cocobeat-song-decode".into())
            .spawn(move || {
                let result = content::load_package(&path).and_then(|(content, sound)| {
                    if content.content_id != content_id
                        || content.end.frames() as u64 != canonical_frames
                        || content.stage.as_ref().map(|stage| stage.compiler_version())
                            != Some(stage_compiler_version)
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
        self.loading_thread = Some(thread);
        Ok(())
    }

    pub fn cancel_worker(&mut self) {
        self.end_phase_sampling();
        if let Some(recovery) = &mut self.recovery {
            recovery.stop_sampling();
        }
        if let Some(worker) = &mut self.worker {
            worker.cancel();
        }
    }

    pub fn stop(&mut self) {
        self.cancel_worker();
        self.terminal = true;
        self.loading = None;
    }

    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(LiveSession::is_finished)
            && self
                .loading_thread
                .as_ref()
                .is_none_or(std::thread::JoinHandle::is_finished)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_network_clock_keeps_actual_samples_and_rejects_stale_rounds() {
        use cocobeat_net::clock::{ClockError, ClockExchange};
        let mut round = OnlineRound {
            started: true,
            player: Some(PlayerId::P1),
            ..Default::default()
        };
        let exchange = ClockExchange {
            epoch: SessionEpoch(9),
            guest_send_ns: 1_000_000_000,
            host_receive_ns: 1_015_000_000,
            host_send_ns: 1_017_000_000,
            guest_receive_ns: 1_022_000_000,
        };
        round.maintain_clock(SessionEpoch(9), 1, exchange).unwrap();
        assert!(round.maintain_clock(SessionEpoch(9), 1, exchange).is_err());
        assert!(
            round
                .maintain_clock(
                    SessionEpoch(10),
                    2,
                    ClockExchange {
                        epoch: SessionEpoch(10),
                        ..exchange
                    }
                )
                .is_err()
        );
        assert_eq!(round.network_clock_round, 1);
        let next = ClockExchange {
            guest_send_ns: exchange.guest_send_ns + 1_000_000_000,
            host_receive_ns: exchange.host_receive_ns + 1_000_000_000,
            host_send_ns: exchange.host_send_ns + 1_000_000_000,
            guest_receive_ns: exchange.guest_receive_ns + 1_000_000_000,
            ..exchange
        };
        round.maintain_clock(SessionEpoch(9), 2, next).unwrap();
        assert_eq!(round.network_clock_round, 2);
        let clock = round.network_clock.as_mut().unwrap();
        let actual = clock.estimate(next.guest_receive_ns).unwrap();
        assert_eq!(actual.epoch, SessionEpoch(9));
        assert_eq!(actual.guest_ns, next.guest_receive_ns);
        assert_eq!(
            clock.estimate(next.guest_receive_ns + 2_000_000_001),
            Err(ClockError::Stale)
        );
    }

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
                    LiveRole::Receive {
                        package_destination,
                        invite,
                    } => {
                        assert_eq!(expected_package, "received");
                        (package_destination, invite)
                    }
                };
                assert_eq!(package, &PathBuf::from(expected_package));
                assert_eq!(actual_invite, &PathBuf::from(invite));
            }
        }
    }

    #[test]
    fn terminal_rounds_advance_and_a_failed_start_consumes_one_config() {
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
        assert!(round.can_start_next());
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
        assert!(round.poll().unwrap().is_none());
        assert!(round.start_next().is_err());
        assert_eq!(round.remaining_rounds(), 1);
    }

    #[test]
    fn the_last_completed_round_has_no_next_action() {
        let mut round = OnlineRound::default();
        round.stop();
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
            round.spent = true;
            round.stop();
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

    #[test]
    fn failed_workers_reenter_only_after_decoder_exit_and_keep_old_evidence() {
        fn wait(round: &OnlineRound) {
            let deadline = Instant::now() + std::time::Duration::from_secs(5);
            while !round.is_finished() && Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(round.is_finished());
        }
        let root = std::env::temp_dir().join(format!(
            "cocobeat-reentry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let destination = root.join("package");
        let invite = root.join("second-invite");
        std::fs::write(&invite, b"invalid fresh invitation").unwrap();
        let mut round = OnlineRound::new(
            LiveConfig {
                role: LiveRole::Receive {
                    package_destination: destination.clone(),
                    invite: root.join("missing-first-invite"),
                },
                output: root.join("first-output"),
            },
            vec![(invite.clone(), root.join("second-output"))],
        );
        round.start().unwrap();
        wait(&round);
        assert!(matches!(
            round.poll().unwrap(),
            Some(Update::Network(LiveEvent::Failed(_)))
        ));
        let first = std::fs::read(root.join("first-output/status.json")).unwrap();
        let (release, held) = mpsc::sync_channel(1);
        let (sender, receiver) = mpsc::sync_channel(1);
        round.loading = Some(receiver);
        round.loading_thread = Some(std::thread::spawn(move || {
            let _ = held.recv();
            let _ = sender.send(Err("discarded old decoder result".into()));
        }));
        round.stop();
        assert!(!round.is_finished());
        assert!(round.start_next().is_err());
        assert_eq!(round.remaining_rounds(), 1);
        release.send(()).unwrap();
        wait(&round);
        assert!(round.poll().unwrap().is_none());
        round.start_next().unwrap();
        assert_eq!(round.remaining_rounds(), 0);
        assert!(round.loading_thread.is_none());
        wait(&round);
        assert!(matches!(
            round.poll().unwrap(),
            Some(Update::Network(LiveEvent::Failed(_)))
        ));
        let second: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("second-output/status.json")).unwrap())
                .unwrap();
        assert_eq!(second["status"], "FAILED");
        assert_eq!(second["facts"], serde_json::json!([0, 0]));
        assert!(!destination.exists());
        assert_eq!(
            std::fs::read(root.join("first-output/status.json")).unwrap(),
            first
        );
        drop(round);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn received_targets_are_preserved_and_invalid_published_packages_never_ready() {
        let root = std::env::temp_dir().join(format!(
            "cocobeat-reentry-targets-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let invite = root.join("invite");
        std::fs::write(&invite, b"invalid invitation").unwrap();
        let directory = root.join("directory");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("keep"), b"invalid published package").unwrap();
        let file = root.join("file");
        std::fs::write(&file, b"preserved existing file").unwrap();
        let targets = vec![(directory.clone(), true), (file.clone(), false)];
        #[cfg(unix)]
        let targets = {
            let mut targets = targets;
            let link = root.join("link");
            std::os::unix::fs::symlink(&directory, &link).unwrap();
            targets.push((link, false));
            targets
        };
        for (index, (target, directory)) in targets.into_iter().enumerate() {
            let output = root.join(format!("output-{index}"));
            let mut round = OnlineRound::new(
                LiveConfig {
                    role: LiveRole::Receive {
                        package_destination: target.clone(),
                        invite: invite.clone(),
                    },
                    output: root.join("unused-original"),
                },
                vec![(invite.clone(), output.clone())],
            );
            round.spent = true;
            round.stop();
            let result = round.start_next();
            if directory {
                result.unwrap();
                let deadline = Instant::now() + std::time::Duration::from_secs(5);
                while !round.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                assert!(round.is_finished());
                assert!(matches!(
                    round.poll().unwrap(),
                    Some(Update::Network(LiveEvent::Failed(_)))
                ));
                let status: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(output.join("status.json")).unwrap())
                        .unwrap();
                assert_eq!(status["status"], "FAILED");
                assert_eq!(
                    std::fs::read(target.join("keep")).unwrap(),
                    b"invalid published package"
                );
            } else {
                assert!(result.is_err());
                assert!(!output.exists());
                assert!(std::fs::symlink_metadata(&target).is_ok());
            }
            assert_eq!(round.remaining_rounds(), 0);
            assert_eq!(std::fs::read(&file).unwrap(), b"preserved existing file");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
