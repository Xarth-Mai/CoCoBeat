//! One bounded continuation of the original audio source and rule history

use std::{
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use cocobeat_net::{
    LiveCommand, PhaseFrozen, PhaseObserved, PhasePublication, RecoveryFrozen, RecoveryObserved,
    RecoveryPublication,
};
use cocobeat_schema::{DuoEvent, PlayerId, SessionEpoch, SongTime};
use kira::sound::PlaybackState;

use crate::{
    audio::{SourceObservation, SourceSampler},
    clock::{ClockState, MonotonicTime},
    session::Session,
};

const SOURCE_MAX_AGE: Duration = Duration::from_millis(50);
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const SAMPLE_TAIL: Duration = Duration::from_millis(80);
const MAX_PUBLICATIONS: usize = 64;

enum Step {
    Pausing {
        stable: Option<RecoveryPublication>,
        paused_bits: Option<u64>,
    },
    Frozen {
        frame: SongTime,
        acknowledged_at: Instant,
        position_seconds_bits: u64,
    },
    Resuming(Resume),
    Ready,
}

struct Resume {
    frozen_frame: SongTime,
    acknowledged_at: Instant,
    verify_at: Instant,
    progress: Option<RecoveryPublication>,
    not_before: Option<Instant>,
    publications: Vec<RecoveryPublication>,
    observed: bool,
    sampler: Option<Sampler>,
}

#[derive(Clone, Copy)]
enum Purpose {
    Reconnect,
    SourceMaintenance { round: u16 },
}

pub(super) struct Recovery {
    purpose: Purpose,
    epoch: SessionEpoch,
    end: SongTime,
    started_at: Instant,
    generation: u64,
    source_id: u64,
    last: SourceObservation,
    step: Step,
}

fn monotonic(at: Instant, origin: Instant) -> Result<MonotonicTime, String> {
    let elapsed = at
        .checked_duration_since(origin)
        .ok_or("Source predates input origin")?;
    Ok(MonotonicTime::from_nanos(
        u64::try_from(elapsed.as_nanos()).map_err(|_| "Input clock nanoseconds overflow")?,
    ))
}

fn publication(
    source: SourceObservation,
    end: SongTime,
    now: Instant,
) -> Result<RecoveryPublication, String> {
    let frame = SongTime::try_from_seconds_f64(source.position_seconds)
        .filter(|frame| *frame >= SongTime::ZERO && *frame < end)
        .ok_or("Recovery source lies outside the unfinished song")?;
    if source.generation == 0
        || source.source_id == 0
        || source.sequence == 0
        || source.published_between[0] > source.published_between[1]
        || source.published_between[1] > now
        || now
            .checked_duration_since(source.published_between[0])
            .is_none_or(|age| age > SOURCE_MAX_AGE)
    {
        return Err("Recovery source publication is missing, stale or invalid".into());
    }
    Ok(RecoveryPublication {
        sequence: source.sequence,
        frame,
        published_between: source.published_between,
    })
}

impl Recovery {
    pub(super) fn begin(
        session: &mut Session,
        end: SongTime,
        source: Option<SourceObservation>,
        now: Instant,
        input_origin: Instant,
    ) -> Result<Self, String> {
        let source = source.ok_or("Recovery needs an observed original source")?;
        let row = publication(source, end, now)?;
        if session.clock.state() != ClockState::Running
            || session
                .clock
                .last_observation()
                .is_none_or(|last| row.frame < last.song_time)
        {
            return Err("Recovery requires a running, nondecreasing original clock".into());
        }
        session
            .clock
            .invalidate_calibration(monotonic(now, input_origin)?)
            .map_err(|error| format!("Recovery pause transition: {error:?}"))?;
        Ok(Self {
            purpose: Purpose::Reconnect,
            epoch: session.epoch(),
            end,
            started_at: now,
            generation: source.generation,
            source_id: source.source_id,
            last: source,
            step: Step::Pausing {
                stable: None,
                paused_bits: None,
            },
        })
    }

    pub(super) fn active(&self) -> bool {
        !matches!(self.step, Step::Ready)
    }

    fn check_time(&self, now: Instant) -> Result<(), String> {
        if now
            .checked_duration_since(self.started_at)
            .is_none_or(|age| age >= RECOVERY_TIMEOUT)
        {
            return Err("Recovery exceeded its fixed thirty-second window".into());
        }
        Ok(())
    }

    /// Every readable publication is checked, including samples outside the gate window
    fn read(
        &mut self,
        source: Option<SourceObservation>,
        now: Instant,
    ) -> Result<Option<RecoveryPublication>, String> {
        self.check_time(now)?;
        let Some(source) = source else {
            publication(self.last, self.end, now)?;
            return Ok(None);
        };
        let row = publication(source, self.end, now)?;
        if source.generation != self.generation
            || source.source_id != self.source_id
            || source.sequence < self.last.sequence
            || source.position_seconds < self.last.position_seconds
        {
            return Err("Recovery source changed identity or moved backwards".into());
        }
        if source.sequence == self.last.sequence {
            if source.position_seconds.to_bits() != self.last.position_seconds.to_bits()
                || source.published_between != self.last.published_between
            {
                return Err("Recovery source changed within one publication".into());
            }
            return Ok(None);
        }
        if source.published_between[0] < self.last.published_between[1] {
            return Err("Recovery publication intervals overlap or move backwards".into());
        }
        self.last = source;
        Ok(Some(row))
    }

    pub(super) fn schedule(
        &mut self,
        source: Option<SourceObservation>,
        state: Option<PlaybackState>,
        now: Instant,
        deadline: Instant,
        verify_at: Instant,
        common_frame: SongTime,
    ) -> Result<(), String> {
        let source = source.ok_or("Recovery scheduling needs a current coherent publication")?;
        self.read(Some(source), now)?;
        let Step::Frozen {
            frame,
            acknowledged_at,
            position_seconds_bits,
        } = self.step
        else {
            return Err("Recovery schedule requires a stable original pause".into());
        };
        if state != Some(PlaybackState::Paused)
            || publication(self.last, self.end, now)?.frame != frame
            || matches!(self.purpose, Purpose::SourceMaintenance { .. })
                && self.last.position_seconds.to_bits() != position_seconds_bits
            || deadline
                .checked_duration_since(now)
                .is_none_or(|lead| lead < Duration::from_millis(100))
            || verify_at <= deadline
            || verify_at
                .checked_duration_since(self.started_at)
                .is_none_or(|age| age + SAMPLE_TAIL >= RECOVERY_TIMEOUT)
            || common_frame < frame
            || common_frame >= self.end
        {
            return Err(
                "Recovery schedule no longer matches the frozen source or time window".into(),
            );
        }
        self.step = Step::Resuming(Resume {
            frozen_frame: frame,
            acknowledged_at,
            verify_at,
            progress: None,
            not_before: None,
            publications: Vec::new(),
            observed: false,
            sampler: None,
        });
        Ok(())
    }

    pub(super) fn start_sampler(&mut self, source: SourceSampler) -> Result<(), String> {
        let Step::Resuming(resume) = &mut self.step else {
            return Err("Recovery sampler requires the original resume schedule".into());
        };
        if resume.sampler.is_some() {
            return Err("Recovery sampler already started".into());
        }
        resume.sampler = Some(Sampler::start(
            source,
            self.last,
            self.end,
            Some(resume.frozen_frame),
            resume.verify_at + SAMPLE_TAIL,
        )?);
        Ok(())
    }

    pub(super) fn stop_sampling(&mut self) {
        if let Step::Resuming(resume) = &mut self.step {
            resume.sampler = None;
        }
    }

    pub(super) fn sampling(&mut self, not_before: Instant, now: Instant) -> Result<(), String> {
        self.check_time(now)?;
        let Step::Resuming(resume) = &mut self.step else {
            return Err("Recovery sampling arrived before its schedule".into());
        };
        if resume.not_before.is_some()
            || not_before < self.started_at
            || not_before > now
            || now >= resume.verify_at
        {
            return Err("Recovery sampling missed the fixed verification point".into());
        }
        if let Some(sampler) = &resume.sampler {
            sampler.sampling(not_before)?;
        }
        resume.not_before = Some(not_before);
        Ok(())
    }

    pub(super) fn ready(
        &mut self,
        source: Option<SourceObservation>,
        state: Option<PlaybackState>,
        now: Instant,
    ) -> Result<(), String> {
        let source = source.ok_or("Recovery Ready needs a current coherent publication")?;
        self.read(Some(source), now)?;
        if !matches!(&self.step, Step::Resuming(resume) if resume.observed)
            || state != Some(PlaybackState::Playing)
        {
            return Err("Recovery Ready arrived before observed continuous playback".into());
        }
        self.stop_sampling();
        self.step = Step::Ready;
        Ok(())
    }

    pub(super) fn update(
        &mut self,
        session: &mut Session,
        player: PlayerId,
        source: Option<SourceObservation>,
        state: Option<PlaybackState>,
        now: Instant,
        input_origin: Instant,
    ) -> Result<(Vec<DuoEvent>, Vec<LiveCommand>), String> {
        if session.epoch() != self.epoch {
            return Err("Recovery cannot replace the original session epoch".into());
        }
        let row = self.read(source, now)?;
        let mut commands = Vec::new();
        let mut events = Vec::new();
        match &mut self.step {
            Step::Pausing {
                stable,
                paused_bits,
            } => {
                if !matches!(state, Some(PlaybackState::Pausing | PlaybackState::Paused)) {
                    return Err("Recovery lost its requested original pause".into());
                }
                if stable.is_some() && state != Some(PlaybackState::Paused) {
                    return Err("Recovery source left its acknowledged pause".into());
                }
                if state == Some(PlaybackState::Paused)
                    && let Some(row) = row.filter(|row| row.published_between[0] >= self.started_at)
                {
                    if stable.is_some_and(|previous| previous.frame == row.frame)
                        && (matches!(self.purpose, Purpose::Reconnect)
                            || stable.is_some_and(|previous| previous.sequence < row.sequence)
                                && self.last.position_seconds.to_bits()
                                    == paused_bits.unwrap_or(u64::MAX))
                    {
                        let at = monotonic(row.published_between[1], input_origin)?;
                        session.observe_audio(self.last.position_seconds, at)?;
                        session.update_position(at)?;
                        session
                            .clock
                            .pause(at)
                            .map_err(|error| format!("Recovery acknowledged pause: {error:?}"))?;
                        commands.push(match self.purpose {
                            Purpose::Reconnect => LiveCommand::RecoveryFrozen {
                                epoch: self.epoch,
                                attempt: 1,
                                snapshot: Box::new(RecoveryFrozen {
                                    replay: session.replay.clone(),
                                    paused_frame: row.frame,
                                    source_generation: self.generation,
                                    source_id: self.source_id,
                                    paused_at: now,
                                }),
                            },
                            Purpose::SourceMaintenance { round } => LiveCommand::PhaseFrozen {
                                epoch: self.epoch,
                                round,
                                snapshot: Box::new(PhaseFrozen {
                                    replay: session.replay.clone(),
                                    paused_frame: row.frame,
                                    source_generation: self.generation,
                                    source_id: self.source_id,
                                    paused_at: now,
                                    publication: raw_publication(self.last),
                                }),
                            },
                        });
                        self.step = Step::Frozen {
                            frame: row.frame,
                            acknowledged_at: now,
                            position_seconds_bits: self.last.position_seconds.to_bits(),
                        };
                    } else {
                        *stable = Some(row);
                        *paused_bits = Some(self.last.position_seconds.to_bits());
                    }
                }
            }
            Step::Frozen {
                frame,
                position_seconds_bits,
                ..
            } => {
                if state != Some(PlaybackState::Paused)
                    || SongTime::try_from_seconds_f64(self.last.position_seconds) != Some(*frame)
                    || matches!(self.purpose, Purpose::SourceMaintenance { .. })
                        && self.last.position_seconds.to_bits() != *position_seconds_bits
                {
                    return Err("Recovery original source did not remain frozen".into());
                }
            }
            Step::Resuming(resume) => {
                #[cfg(not(test))]
                if resume.sampler.is_none() && matches!(self.purpose, Purpose::Reconnect) {
                    return Err("Recovery resume has no original source sampler".into());
                }
                if !matches!(
                    state,
                    Some(PlaybackState::Resuming | PlaybackState::Playing)
                ) || (resume.progress.is_some() && state != Some(PlaybackState::Playing))
                {
                    return Err("Recovery playback paused or stopped after scheduling".into());
                }
                if let Some(row) = row {
                    if resume.progress.is_none()
                        && state == Some(PlaybackState::Playing)
                        && row.frame > resume.frozen_frame
                    {
                        if row.published_between[0] < resume.acknowledged_at {
                            return Err(
                                "Recovery progress predates the pause acknowledgment".into()
                            );
                        }
                        session
                            .clock
                            .resume(monotonic(row.published_between[0], input_origin)?)
                            .map_err(|error| format!("Recovery real progress: {error:?}"))?;
                        resume.progress = Some(row);
                    }
                    if resume.progress.is_some() {
                        session.observe_audio(
                            self.last.position_seconds,
                            monotonic(row.published_between[1], input_origin)?,
                        )?;
                        // Synthetic state-machine checks supply their own publications
                        #[cfg(test)]
                        if matches!(self.purpose, Purpose::Reconnect)
                            && resume.sampler.is_none()
                            && !resume.observed
                            && resume
                                .progress
                                .is_some_and(|progress| row.sequence > progress.sequence)
                            && resume
                                .not_before
                                .is_some_and(|time| row.published_between[0] >= time)
                        {
                            if resume.publications.len() == MAX_PUBLICATIONS {
                                return Err(
                                    "Recovery publication limit reached before verification".into(),
                                );
                            }
                            resume.publications.push(row);
                        }
                    }
                }
                if resume.progress.is_some() {
                    session.update_position(monotonic(now, input_origin)?)?;
                    let (facts, confirmed) = session.advance_player(player)?;
                    commands.extend(facts.into_iter().map(LiveCommand::Fact));
                    events = confirmed;
                }
                if matches!(self.purpose, Purpose::Reconnect)
                    && !resume.observed
                    && now >= resume.verify_at + SAMPLE_TAIL
                {
                    let progress = resume
                        .progress
                        .ok_or("Recovery never acknowledged forward playback")?;
                    if let Some(sampler) = &mut resume.sampler {
                        let Some(rows) = sampler.result()? else {
                            return Ok((events, commands));
                        };
                        resume.publications = rows
                            .into_iter()
                            .map(|row| RecoveryPublication {
                                sequence: row.sequence,
                                frame: SongTime::try_from_seconds_f64(row.position_seconds)
                                    .expect("sampler already validated source frame"),
                                published_between: row.published_between,
                            })
                            .filter(|row| {
                                row.sequence > progress.sequence
                                    && resume
                                        .not_before
                                        .is_some_and(|time| row.published_between[0] >= time)
                            })
                            .collect();
                    }
                    if resume.publications.len() < 2
                        || !resume
                            .publications
                            .iter()
                            .any(|row| row.published_between[1] <= resume.verify_at)
                        || !resume
                            .publications
                            .iter()
                            .any(|row| row.published_between[0] >= resume.verify_at)
                    {
                        return Err(
                            "Recovery publications do not bracket the fixed verification point"
                                .into(),
                        );
                    }
                    commands.push(LiveCommand::RecoveryObserved {
                        epoch: self.epoch,
                        attempt: 1,
                        evidence: RecoveryObserved {
                            generation: self.generation,
                            source_id: self.source_id,
                            progress,
                            publications: std::mem::take(&mut resume.publications),
                        },
                    });
                    resume.observed = true;
                }
            }
            Step::Ready => return Err("Recovery already released its input gate".into()),
        }
        Ok((events, commands))
    }
}

fn raw_publication(source: SourceObservation) -> PhasePublication {
    PhasePublication {
        sequence: source.sequence,
        position_seconds_bits: source.position_seconds.to_bits(),
        published_between: source.published_between,
    }
}

/// Persistent original-source identity, separate from the one reconnect attempt
pub(super) struct PhaseMaintenance {
    epoch: SessionEpoch,
    end: SongTime,
    last: SourceObservation,
    last_round: u16,
    round: Option<MaintenanceRound>,
}

struct MaintenanceRound {
    number: u16,
    started_at: Instant,
    sample: Option<PhaseSample>,
    check_observed: bool,
    verification_observed: bool,
    correction: Option<Recovery>,
}

struct PhaseSample {
    verification: bool,
    common_at: Instant,
    until: Instant,
    not_before: Instant,
    sampler: Sampler,
}

impl PhaseMaintenance {
    pub(super) fn new(
        epoch: SessionEpoch,
        end: SongTime,
        source: SourceObservation,
        now: Instant,
    ) -> Result<Self, String> {
        publication(source, end, now)?;
        Ok(Self {
            epoch,
            end,
            last: source,
            last_round: 0,
            round: None,
        })
    }

    pub(super) fn active(&self) -> bool {
        self.round.is_some()
    }

    pub(super) fn correcting(&self) -> bool {
        self.round
            .as_ref()
            .is_some_and(|round| round.correction.is_some())
    }

    pub(super) fn observe(
        &mut self,
        source: SourceObservation,
        now: Instant,
    ) -> Result<(), String> {
        publication(source, self.end, now)?;
        if source.generation != self.last.generation
            || source.source_id != self.last.source_id
            || source.sequence < self.last.sequence
            || source.position_seconds < self.last.position_seconds
            || source.sequence == self.last.sequence
                && (source.position_seconds.to_bits() != self.last.position_seconds.to_bits()
                    || source.published_between != self.last.published_between)
            || source.sequence > self.last.sequence
                && source.published_between[0] < self.last.published_between[1]
        {
            return Err(
                "Phase maintenance changed the original source or publication order".into(),
            );
        }
        self.last = source;
        Ok(())
    }

    fn matching(&mut self, round: u16, now: Instant) -> Result<&mut MaintenanceRound, String> {
        let active = self.round.as_mut().ok_or("Phase round is not active")?;
        if active.number != round
            || now
                .checked_duration_since(active.started_at)
                .is_none_or(|age| age >= RECOVERY_TIMEOUT)
        {
            return Err("Phase round differs or exceeded its fixed deadline".into());
        }
        Ok(active)
    }

    pub(super) fn sampling(
        &mut self,
        round: u16,
        verification: bool,
        window: [Instant; 3],
        observed: (SourceObservation, Option<PlaybackState>),
        sampler: SourceSampler,
        now: Instant,
    ) -> Result<(), String> {
        let (source, state) = observed;
        self.observe(source, now)?;
        let [not_before, common_at, until] = window;
        if not_before > common_at
            || now >= common_at
            || common_at >= until
            || until.duration_since(common_at) != SAMPLE_TAIL
            || common_at.duration_since(not_before) > Duration::from_millis(40)
        {
            return Err("Phase sampling missed its fixed bounded window".into());
        }
        if !verification {
            if self.round.is_some()
                || round
                    != self
                        .last_round
                        .checked_add(1)
                        .ok_or("Phase round overflow")?
                || round > 128
                || state != Some(PlaybackState::Playing)
            {
                return Err(
                    "Phase check needs the next round and acknowledged original Playing source"
                        .into(),
                );
            }
            self.round = Some(MaintenanceRound {
                number: round,
                started_at: now,
                sample: None,
                check_observed: false,
                verification_observed: false,
                correction: None,
            });
        }
        let initial = self.last;
        let end = self.end;
        let active = self.matching(round, now)?;
        if active.sample.is_some()
            || (verification && active.verification_observed)
            || (!verification && active.check_observed)
            || until
                .checked_duration_since(active.started_at)
                .is_none_or(|age| age >= RECOVERY_TIMEOUT)
        {
            return Err(
                "Phase sampling is duplicate or outside the original round deadline".into(),
            );
        }
        let frozen = if verification {
            let recovery = active
                .correction
                .as_ref()
                .ok_or("Phase verification requires an actual correction")?;
            let Step::Resuming(resume) = &recovery.step else {
                return Err("Phase verification requires the armed original resume".into());
            };
            if !matches!(
                state,
                Some(PlaybackState::Resuming | PlaybackState::Playing)
            ) {
                return Err("Phase verification lost its original resume".into());
            }
            Some(resume.frozen_frame)
        } else {
            None
        };
        let sampler = Sampler::start(sampler, initial, end, frozen, until)?;
        sampler.sampling(not_before)?;
        active.sample = Some(PhaseSample {
            verification,
            common_at,
            until,
            not_before,
            sampler,
        });
        Ok(())
    }

    pub(super) fn pausing(
        &mut self,
        round: u16,
        session: &mut Session,
        source: SourceObservation,
        state: Option<PlaybackState>,
        now: Instant,
        input_origin: Instant,
    ) -> Result<(), String> {
        self.observe(source, now)?;
        let end = self.end;
        let active = self.matching(round, now)?;
        if !active.check_observed
            || active.sample.is_some()
            || active.correction.is_some()
            || state != Some(PlaybackState::Playing)
        {
            return Err(
                "Phase pause requires its completed original check and acknowledged Playing source"
                    .into(),
            );
        }
        let mut correction = Recovery::begin(session, end, Some(source), now, input_origin)?;
        correction.purpose = Purpose::SourceMaintenance { round };
        // Pausing does not restart the check's thirty-second budget
        correction.started_at = active.started_at;
        active.correction = Some(correction);
        Ok(())
    }

    pub(super) fn schedule(
        &mut self,
        round: u16,
        source: SourceObservation,
        state: Option<PlaybackState>,
        now: Instant,
        times: [Instant; 2],
        common_frame: SongTime,
    ) -> Result<(), String> {
        self.observe(source, now)?;
        self.matching(round, now)?
            .correction
            .as_mut()
            .ok_or("Phase correction has not paused")?
            .schedule(Some(source), state, now, times[0], times[1], common_frame)
    }

    pub(super) fn ready(
        &mut self,
        round: u16,
        source: SourceObservation,
        state: Option<PlaybackState>,
        now: Instant,
    ) -> Result<(), String> {
        self.observe(source, now)?;
        let active = self.matching(round, now)?;
        if active.sample.is_some()
            || state != Some(PlaybackState::Playing)
            || !active.check_observed
        {
            return Err("Phase Ready arrived without its original completed Playing check".into());
        }
        if let Some(correction) = &mut active.correction {
            if !active.verification_observed {
                return Err("Phase Ready arrived before its independent verification".into());
            }
            correction.ready(Some(source), state, now)?;
        }
        self.last_round = round;
        self.round = None;
        Ok(())
    }

    pub(super) fn stop_sampling(&mut self) {
        if let Some(active) = &mut self.round {
            active.sample = None;
            if let Some(correction) = &mut active.correction {
                correction.stop_sampling();
            }
        }
    }

    pub(super) fn update(
        &mut self,
        session: &mut Session,
        player: PlayerId,
        source: Option<SourceObservation>,
        state: Option<PlaybackState>,
        now: Instant,
        input_origin: Instant,
    ) -> Result<(Vec<DuoEvent>, Vec<LiveCommand>), String> {
        if session.epoch() != self.epoch {
            return Err("Phase cannot replace its original epoch".into());
        }
        let source = source.ok_or("Phase requires a coherent original source publication")?;
        self.observe(source, now)?;
        let Some(active) = &mut self.round else {
            return Ok((Vec::new(), Vec::new()));
        };
        if now
            .checked_duration_since(active.started_at)
            .is_none_or(|age| age >= RECOVERY_TIMEOUT)
        {
            return Err("Phase exceeded its fixed thirty-second budget".into());
        }
        let (events, mut commands) = if let Some(correction) = &mut active.correction {
            correction.update(session, player, Some(source), state, now, input_origin)?
        } else {
            if state != Some(PlaybackState::Playing) {
                return Err("Phase check lost acknowledged original Playing source".into());
            }
            (Vec::new(), Vec::new())
        };
        let Some(sample) = &mut active.sample else {
            return Ok((events, commands));
        };
        if now < sample.until {
            return Ok((events, commands));
        }
        let Some(rows) = sample.sampler.result()? else {
            return Ok((events, commands));
        };
        if !(2..=MAX_PUBLICATIONS).contains(&rows.len())
            || rows.iter().any(|row| {
                row.published_between[0] < sample.not_before
                    || row.published_between[1] > sample.until
            })
            || !rows
                .iter()
                .any(|row| row.published_between[1] <= sample.common_at)
            || !rows
                .iter()
                .any(|row| row.published_between[0] >= sample.common_at)
        {
            return Err("Phase raw publications do not bracket the fixed common point".into());
        }
        if sample.verification {
            let correction = active
                .correction
                .as_mut()
                .ok_or("Phase verification lost its correction")?;
            let Step::Resuming(resume) = &mut correction.step else {
                return Err("Phase verification lost its original resume".into());
            };
            if resume.progress.is_none()
                || state != Some(PlaybackState::Playing)
                || rows.iter().any(|row| {
                    row.position_seconds <= resume.frozen_frame.as_seconds_f64()
                        || row.published_between[0] < resume.acknowledged_at
                })
            {
                return Err(
                    "Phase verification has no acknowledged original forward progress".into(),
                );
            }
            resume.observed = true;
            active.verification_observed = true;
        } else {
            active.check_observed = true;
        }
        commands.push(LiveCommand::PhaseObserved {
            epoch: self.epoch,
            round: active.number,
            verification: sample.verification,
            evidence: PhaseObserved {
                generation: self.last.generation,
                source_id: self.last.source_id,
                collected_at: Instant::now(),
                publications: rows.into_iter().map(raw_publication).collect(),
            },
        });
        active.sample = None;
        Ok((events, commands))
    }
}

/// One finite read-only sampling job, disconnected and joined on every exit
struct Sampler {
    stop: Option<Sender<Instant>>,
    rows: Receiver<Result<Vec<SourceObservation>, String>>,
    thread: Option<JoinHandle<()>>,
}

impl Sampler {
    fn start(
        source: SourceSampler,
        initial: SourceObservation,
        end: SongTime,
        frozen: Option<SongTime>,
        until: Instant,
    ) -> Result<Self, String> {
        let (stop, canceled) = mpsc::channel();
        let (sender, rows) = mpsc::sync_channel(1);
        let thread = thread::Builder::new().name("recovery-source-sampler".into()).spawn(move || {
            let result = (|| {
                let mut last = initial;
                let mut rows = Vec::new();
                let mut progressing = frozen.is_none();
                let mut not_before: Option<Instant> = None;
                loop {
                    let current = source.read()?;
                    let now = Instant::now();
                    if let Some((current, state)) = current {
                        let row = publication(current, end, now)?;
                        if current.generation != initial.generation || current.source_id != initial.source_id
                            || current.sequence < last.sequence || current.position_seconds < last.position_seconds {
                            return Err("Recovery sampler source changed identity or moved backwards".into());
                        }
                        if current.sequence == last.sequence {
                            if current.position_seconds.to_bits() != last.position_seconds.to_bits()
                                || current.published_between != last.published_between {
                                return Err("Recovery sampler publication changed within its sequence".into());
                            }
                        } else {
                            if current.published_between[0] < last.published_between[1] {
                                return Err("Recovery sampler publication intervals overlap or move backwards".into());
                            }
                            if frozen.is_none_or(|frozen| row.frame > frozen) && state == PlaybackState::Playing {
                                progressing = true;
                                if not_before.is_some_and(|time| row.published_between[0] >= time)
                                    && row.published_between[1] <= until
                                {
                                    if rows.len() == MAX_PUBLICATIONS {
                                        return Err("Recovery sampler publication limit reached".into());
                                    }
                                    rows.push(current);
                                }
                            }
                            last = current;
                        }
                        // Raw state is a separate read, never a claimed atomically paired timestamp
                        if state == PlaybackState::Stopped || (progressing && state != PlaybackState::Playing) {
                            return Err("Recovery sampler source stopped or paused".into());
                        }
                    } else {
                        publication(last, end, now)?;
                    }
                    if now >= until {
                        return Ok(rows);
                    }
                    match canceled.recv_timeout(Duration::from_millis(2).min(until - now)) {
                        Ok(at) => {
                            if not_before.replace(at).is_some() {
                                return Err("Recovery sampler received a second sampling window".into());
                            }
                        },
                        Err(mpsc::RecvTimeoutError::Timeout) => {},
                        Err(mpsc::RecvTimeoutError::Disconnected) => return Err("Recovery sampler canceled".into()),
                    }
                }
            })();
            let _ = sender.try_send(result);
        }).map_err(|error| format!("Start recovery source sampler: {error}"))?;
        Ok(Self {
            stop: Some(stop),
            rows,
            thread: Some(thread),
        })
    }

    fn sampling(&self, not_before: Instant) -> Result<(), String> {
        self.stop
            .as_ref()
            .ok_or("Recovery sampler already joined")?
            .send(not_before)
            .map_err(|_| "Recovery sampler ended before its sampling window".into())
    }

    fn result(&mut self) -> Result<Option<Vec<SourceObservation>>, String> {
        match self.rows.try_recv() {
            Ok(result) => {
                self.join()?;
                result.map(Some)
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                Err("Recovery sampler ended without evidence".into())
            }
        }
    }

    fn join(&mut self) -> Result<(), String> {
        self.stop = None;
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| "Recovery sampler panicked")?;
        }
        Ok(())
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_schema::DuoInput;

    #[test]
    fn original_mock_source_sampler_keeps_actual_rows_for_slow_consumer_and_joins() {
        let (mut audio, source) = crate::audio::mock_source_sampler();
        audio.backend_mut().on_start_processing();
        audio.backend_mut().process();
        let (initial, state) = source.read().unwrap().unwrap();
        assert_eq!(state, PlaybackState::Playing);
        let until = Instant::now() + Duration::from_millis(100);
        let mut sampler = Sampler::start(
            source.clone(),
            initial,
            SongTime::from_frames(48_000),
            Some(SongTime::ZERO),
            until,
        )
        .unwrap();
        sampler.sampling(Instant::now()).unwrap();
        // The consumer waits while actual Kira callbacks keep publishing on the original source
        for _ in 0..45 {
            thread::sleep(Duration::from_millis(3));
            audio.backend_mut().on_start_processing();
            audio.backend_mut().process();
        }
        let rows = sampler.result().unwrap().unwrap();
        assert!(rows.len() >= 2);
        assert!(
            rows.windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence
                    && pair[0].position_seconds < pair[1].position_seconds
                    && pair[0].published_between[1] <= pair[1].published_between[0])
        );
        assert!(rows.iter().any(|row| row.published_between[1] <= until));
        let (current, _) = source.read().unwrap().unwrap();
        assert_eq!(
            (current.generation, current.source_id),
            (initial.generation, initial.source_id)
        );
        assert!(current.sequence > rows.last().unwrap().sequence);
        assert!(sampler.thread.is_none());
        assert_eq!(source.music_owners(), 1);
        let sampler = Sampler::start(
            source.clone(),
            current,
            SongTime::from_frames(48_000),
            Some(SongTime::ZERO),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
        drop(sampler);
        assert_eq!(source.music_owners(), 1);
    }

    #[test]
    fn sampler_checks_long_catchup_but_stores_only_the_signaled_window_and_keeps_limit() {
        for delayed_window in [true, false] {
            let (mut audio, source) = crate::audio::mock_source_sampler();
            audio.backend_mut().on_start_processing();
            audio.backend_mut().process();
            let (initial, _) = source.read().unwrap().unwrap();
            let until = Instant::now() + Duration::from_secs(1);
            let mut sampler = Sampler::start(
                source.clone(),
                initial,
                SongTime::from_frames(48_000),
                Some(SongTime::ZERO),
                until,
            )
            .unwrap();
            let mut callbacks_before_window = 0;
            if delayed_window {
                while Instant::now() < until - Duration::from_millis(100) {
                    thread::sleep(Duration::from_millis(3));
                    audio.backend_mut().on_start_processing();
                    audio.backend_mut().process();
                    callbacks_before_window += 1;
                }
                assert!(callbacks_before_window > MAX_PUBLICATIONS);
                assert!(sampler.result().unwrap().is_none());
            }
            let not_before = Instant::now();
            sampler.sampling(not_before).unwrap();
            let result = loop {
                thread::sleep(Duration::from_millis(3));
                audio.backend_mut().on_start_processing();
                audio.backend_mut().process();
                match sampler.result() {
                    Ok(None) => {}
                    other => break other,
                }
            };
            if delayed_window {
                let rows = result.unwrap().unwrap();
                assert!((2..=MAX_PUBLICATIONS).contains(&rows.len()));
                assert!(
                    rows.iter()
                        .all(|row| row.published_between[0] >= not_before)
                );
                assert!(rows.iter().any(|row| row.published_between[1] <= until));
            } else {
                assert!(result.unwrap_err().contains("publication limit reached"));
            }
            assert!(sampler.thread.is_none());
            assert_eq!(source.music_owners(), 1);
        }
    }

    // Synthetic publications exercise the runtime state machine, not a native audio device
    fn source(origin: Instant, sequence: u64, ms: u64, frame: i64) -> SourceObservation {
        SourceObservation {
            generation: 7,
            source_id: 9,
            sequence,
            position_seconds: SongTime::from_frames(frame).as_seconds_f64(),
            published_between: [origin + Duration::from_millis(ms); 2],
        }
    }

    fn begin(origin: Instant) -> (Session, Recovery) {
        let mut session = Session::new(SessionEpoch(42)).unwrap();
        session
            .observe_audio(1.0, MonotonicTime::from_nanos(1_000_000_000))
            .unwrap();
        session
            .update_position(MonotonicTime::from_nanos(1_000_000_000))
            .unwrap();
        session
            .hit(PlayerId::P1, 1_000_000_000, 1_000_000_000)
            .unwrap();
        let recovery = Recovery::begin(
            &mut session,
            SongTime::from_frames(3_072_000),
            Some(source(origin, 1, 1_000, 48_000)),
            origin + Duration::from_millis(1_001),
            origin,
        )
        .unwrap();
        (session, recovery)
    }

    fn tick(
        recovery: &mut Recovery,
        session: &mut Session,
        origin: Instant,
        seq: u64,
        ms: u64,
        frame: i64,
        state: PlaybackState,
    ) -> Result<(Vec<DuoEvent>, Vec<LiveCommand>), String> {
        recovery.update(
            session,
            PlayerId::P1,
            Some(source(origin, seq, ms, frame)),
            Some(state),
            origin + Duration::from_millis(ms + 1),
            origin,
        )
    }

    fn freeze(origin: Instant) -> (Session, Recovery) {
        let (mut session, mut recovery) = begin(origin);
        assert!(
            tick(
                &mut recovery,
                &mut session,
                origin,
                2,
                1_010,
                48_480,
                PlaybackState::Paused
            )
            .unwrap()
            .1
            .is_empty()
        );
        let (_, commands) = tick(
            &mut recovery,
            &mut session,
            origin,
            3,
            1_020,
            48_480,
            PlaybackState::Paused,
        )
        .unwrap();
        let [
            LiveCommand::RecoveryFrozen {
                epoch,
                attempt,
                snapshot,
            },
        ] = commands.as_slice()
        else {
            panic!("expected exactly one frozen snapshot");
        };
        assert_eq!((*epoch, *attempt), (SessionEpoch(42), 1));
        assert_eq!(
            snapshot.replay.encode().unwrap(),
            session.replay.encode().unwrap()
        );
        assert_eq!(snapshot.paused_frame.frames(), 48_480);
        assert_eq!(session.clock.state(), ClockState::Paused);
        (session, recovery)
    }

    fn schedule(recovery: &mut Recovery, origin: Instant) {
        recovery
            .schedule(
                Some(source(origin, 4, 1_030, 48_480)),
                Some(PlaybackState::Paused),
                origin + Duration::from_millis(1_031),
                origin + Duration::from_millis(1_500),
                origin + Duration::from_millis(1_600),
                SongTime::from_frames(48_480),
            )
            .unwrap();
    }

    #[test]
    fn original_epoch_and_facts_survive_real_progress_and_gate_once() {
        let origin = Instant::now();
        let (mut session, mut recovery) = freeze(origin);
        let original = session.replay.facts().to_vec();
        schedule(&mut recovery, origin);
        // Playing with the old nonzero cursor does not reopen the clock
        assert!(
            tick(
                &mut recovery,
                &mut session,
                origin,
                5,
                1_040,
                48_480,
                PlaybackState::Playing
            )
            .unwrap()
            .1
            .is_empty()
        );
        assert_eq!(session.clock.state(), ClockState::Paused);
        recovery
            .sampling(
                origin + Duration::from_millis(1_490),
                origin + Duration::from_millis(1_491),
            )
            .unwrap();
        let (_, progress_commands) = tick(
            &mut recovery,
            &mut session,
            origin,
            6,
            1_510,
            48_960,
            PlaybackState::Playing,
        )
        .unwrap();
        assert_eq!(session.clock.state(), ClockState::Running);
        assert!(
            progress_commands
                .iter()
                .all(|command| matches!(command, LiveCommand::Fact(DuoInput::Watermark { .. })))
        );
        tick(
            &mut recovery,
            &mut session,
            origin,
            7,
            1_540,
            50_400,
            PlaybackState::Playing,
        )
        .unwrap();
        tick(
            &mut recovery,
            &mut session,
            origin,
            8,
            1_610,
            53_760,
            PlaybackState::Playing,
        )
        .unwrap();
        let (_, commands) = tick(
            &mut recovery,
            &mut session,
            origin,
            9,
            1_680,
            57_120,
            PlaybackState::Playing,
        )
        .unwrap();
        let observed: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                LiveCommand::RecoveryObserved { evidence, .. } => Some(evidence),
                _ => None,
            })
            .collect();
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].progress.sequence, 6);
        assert_eq!(
            observed[0]
                .publications
                .iter()
                .map(|row| row.sequence)
                .collect::<Vec<_>>(),
            vec![7, 8, 9]
        );
        assert_eq!(session.epoch(), SessionEpoch(42));
        assert_eq!(
            &session.replay.facts()[..original.len()],
            original.as_slice()
        );
        assert!(
            session.replay.facts()[original.len()..]
                .iter()
                .all(|fact| matches!(fact, DuoInput::Watermark { .. }))
        );
        let (_, commands) = tick(
            &mut recovery,
            &mut session,
            origin,
            10,
            1_690,
            57_600,
            PlaybackState::Playing,
        )
        .unwrap();
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, LiveCommand::RecoveryObserved { .. }))
        );
        assert!(recovery.active());
        recovery
            .ready(
                Some(source(origin, 10, 1_690, 57_600)),
                Some(PlaybackState::Playing),
                origin + Duration::from_millis(1_691),
            )
            .unwrap();
        assert!(!recovery.active());
    }

    #[test]
    fn pause_needs_two_distinct_stable_publications_and_fresh_identity() {
        let origin = Instant::now();
        let (mut session, mut recovery) = begin(origin);
        for _ in 0..2 {
            assert!(
                tick(
                    &mut recovery,
                    &mut session,
                    origin,
                    2,
                    1_010,
                    48_480,
                    PlaybackState::Paused
                )
                .unwrap()
                .1
                .is_empty()
            );
        }
        assert!(matches!(recovery.step, Step::Pausing { .. }));
        assert!(
            tick(
                &mut recovery,
                &mut session,
                origin,
                3,
                1_020,
                48_481,
                PlaybackState::Paused
            )
            .unwrap()
            .1
            .is_empty()
        );
        let mut wrong = source(origin, 4, 1_030, 48_481);
        wrong.source_id += 1;
        assert!(
            recovery
                .read(Some(wrong), origin + Duration::from_millis(1_031))
                .is_err()
        );
        assert!(
            recovery
                .read(None, origin + Duration::from_millis(1_071))
                .is_err()
        );
        let mut future = source(origin, 4, 1_030, 48_481);
        future.published_between[1] = origin + Duration::from_millis(1_040);
        assert!(
            recovery
                .read(Some(future), origin + Duration::from_millis(1_031))
                .is_err()
        );
    }

    #[test]
    fn regression_is_rejected_even_before_sampling_and_ready_requires_observed() {
        let origin = Instant::now();
        let (mut session, mut recovery) = freeze(origin);
        assert!(
            recovery
                .schedule(
                    None,
                    Some(PlaybackState::Paused),
                    origin + Duration::from_millis(1_031),
                    origin + Duration::from_millis(1_500),
                    origin + Duration::from_millis(1_600),
                    SongTime::from_frames(48_480)
                )
                .is_err()
        );
        schedule(&mut recovery, origin);
        assert!(
            recovery
                .ready(
                    None,
                    Some(PlaybackState::Playing),
                    origin + Duration::from_millis(1_031)
                )
                .is_err()
        );
        tick(
            &mut recovery,
            &mut session,
            origin,
            5,
            1_510,
            48_960,
            PlaybackState::Playing,
        )
        .unwrap();
        assert!(
            recovery
                .ready(
                    Some(source(origin, 5, 1_510, 48_960)),
                    Some(PlaybackState::Playing),
                    origin + Duration::from_millis(1_511)
                )
                .is_err()
        );
        assert!(
            tick(
                &mut recovery,
                &mut session,
                origin,
                6,
                1_520,
                48_959,
                PlaybackState::Playing
            )
            .is_err()
        );
        assert!(
            tick(
                &mut recovery,
                &mut session,
                origin,
                6,
                1_520,
                49_440,
                PlaybackState::Paused
            )
            .is_err()
        );
    }

    #[test]
    fn verification_point_cannot_move_and_missing_before_side_fails() {
        let origin = Instant::now();
        let (mut session, mut recovery) = freeze(origin);
        schedule(&mut recovery, origin);
        assert!(
            recovery
                .sampling(
                    origin + Duration::from_millis(1_600),
                    origin + Duration::from_millis(1_600)
                )
                .is_err()
        );
        recovery
            .sampling(
                origin + Duration::from_millis(1_490),
                origin + Duration::from_millis(1_491),
            )
            .unwrap();
        assert!(
            recovery
                .sampling(
                    origin + Duration::from_millis(1_495),
                    origin + Duration::from_millis(1_496)
                )
                .is_err()
        );
        tick(
            &mut recovery,
            &mut session,
            origin,
            5,
            1_610,
            53_760,
            PlaybackState::Playing,
        )
        .unwrap();
        assert!(
            tick(
                &mut recovery,
                &mut session,
                origin,
                6,
                1_680,
                57_120,
                PlaybackState::Playing
            )
            .is_err()
        );
        assert!(
            recovery
                .check_time(origin + Duration::from_millis(31_001))
                .is_err()
        );
    }
    #[test]
    fn phase_check_keeps_original_callback_bits_and_never_pauses_or_changes_history() {
        let (mut audio, source) = crate::audio::mock_source_sampler();
        audio.backend_mut().on_start_processing();
        audio.backend_mut().process();
        let (initial, _) = source.read().unwrap().unwrap();
        let started = Instant::now();
        let mut phase = PhaseMaintenance::new(
            SessionEpoch(42),
            SongTime::from_frames(48_000),
            initial,
            started,
        )
        .unwrap();
        let mut session = Session::new(SessionEpoch(42)).unwrap();
        session
            .observe_audio(initial.position_seconds, MonotonicTime::from_nanos(1))
            .unwrap();
        let original = session.replay.encode().unwrap();
        let common_at = started + Duration::from_millis(50);
        let until = common_at + SAMPLE_TAIL;
        phase
            .sampling(
                1,
                false,
                [common_at - Duration::from_millis(40), common_at, until],
                (initial, Some(PlaybackState::Playing)),
                source.clone(),
                started,
            )
            .unwrap();
        let mut callbacks = std::collections::BTreeMap::new();
        while Instant::now() < until + Duration::from_millis(20) {
            thread::sleep(Duration::from_millis(3));
            audio.backend_mut().on_start_processing();
            let (row, state) = source.read().unwrap().unwrap();
            assert_eq!(state, PlaybackState::Playing);
            callbacks.insert(row.sequence, row);
            audio.backend_mut().process();
        }
        let (current, state) = source.read().unwrap().unwrap();
        let (events, commands) = phase
            .update(
                &mut session,
                PlayerId::P1,
                Some(current),
                Some(state),
                Instant::now(),
                started,
            )
            .unwrap();
        assert!(events.is_empty());
        let [
            LiveCommand::PhaseObserved {
                epoch,
                round,
                verification,
                evidence,
            },
        ] = commands.as_slice()
        else {
            panic!("expected one actual raw check");
        };
        assert_eq!(
            (*epoch, *round, *verification),
            (SessionEpoch(42), 1, false)
        );
        assert!((2..=MAX_PUBLICATIONS).contains(&evidence.publications.len()));
        for row in &evidence.publications {
            let original = callbacks.get(&row.sequence).unwrap();
            assert_eq!(
                row.position_seconds_bits,
                original.position_seconds.to_bits()
            );
            assert_eq!(row.published_between, original.published_between);
        }
        assert!(evidence.collected_at >= until);
        assert!(!phase.correcting());
        assert_eq!(session.clock.state(), ClockState::Running);
        assert_eq!(session.replay.encode().unwrap(), original);
        phase
            .ready(1, current, Some(PlaybackState::Playing), Instant::now())
            .unwrap();
        assert!(!phase.active());
        assert_eq!(phase.last_round, 1);
        assert_eq!(source.music_owners(), 1);
        assert!(
            phase
                .ready(1, current, Some(PlaybackState::Playing), Instant::now())
                .is_err()
        );
    }

    #[test]
    fn phase_pause_requires_exact_raw_stability_and_emits_its_own_fifo_snapshot() {
        let origin = Instant::now();
        let (mut session, mut correction) = begin(origin);
        correction.purpose = Purpose::SourceMaintenance { round: 3 };
        let prefix = session.replay.encode().unwrap();
        let mut last = None;
        for (sequence, subframe) in [(2, 0.1), (3, 0.2), (4, 0.2)] {
            let mut row = source(origin, sequence, 1_000 + sequence * 10, 48_480);
            row.position_seconds += subframe / 48_000.0;
            let (_, commands) = correction
                .update(
                    &mut session,
                    PlayerId::P1,
                    Some(row),
                    Some(PlaybackState::Paused),
                    origin + Duration::from_millis(1_001 + sequence * 10),
                    origin,
                )
                .unwrap();
            if sequence < 4 {
                assert!(commands.is_empty());
            } else {
                let [
                    LiveCommand::PhaseFrozen {
                        epoch,
                        round,
                        snapshot,
                    },
                ] = commands.as_slice()
                else {
                    panic!("expected phase snapshot, never RecoveryFrozen attempt1");
                };
                assert_eq!((*epoch, *round), (SessionEpoch(42), 3));
                assert_eq!(
                    snapshot.publication.position_seconds_bits,
                    row.position_seconds.to_bits()
                );
                assert_eq!(snapshot.publication.sequence, row.sequence);
                assert_eq!(
                    snapshot.publication.published_between,
                    row.published_between
                );
                assert_eq!(snapshot.replay.encode().unwrap(), prefix);
                assert_eq!((snapshot.source_generation, snapshot.source_id), (7, 9));
                last = Some(row);
            }
        }
        let mut shifted = last.unwrap();
        shifted.sequence += 1;
        shifted.position_seconds += 0.05 / 48_000.0;
        shifted.published_between = [origin + Duration::from_millis(1_050); 2];
        assert_eq!(
            SongTime::try_from_seconds_f64(shifted.position_seconds),
            Some(SongTime::from_frames(48_480))
        );
        assert!(
            correction
                .schedule(
                    Some(shifted),
                    Some(PlaybackState::Paused),
                    origin + Duration::from_millis(1_051),
                    origin + Duration::from_millis(1_500),
                    origin + Duration::from_millis(1_600),
                    SongTime::from_frames(48_480)
                )
                .is_err()
        );
    }

    #[test]
    fn phase_keeps_bit_identity_original_round_budget_and_independent_ready_domain() {
        let origin = Instant::now();
        let mut negative_zero = source(origin, 1, 0, 0);
        negative_zero.position_seconds = -0.0;
        let mut phase = PhaseMaintenance::new(
            SessionEpoch(42),
            SongTime::from_frames(48_000),
            negative_zero,
            origin,
        )
        .unwrap();
        let mut changed = negative_zero;
        changed.position_seconds = 0.0;
        assert!(phase.observe(changed, origin).is_err());
        assert!(
            phase
                .ready(1, negative_zero, Some(PlaybackState::Playing), origin)
                .is_err()
        );
        phase.round = Some(MaintenanceRound {
            number: 2,
            started_at: origin,
            sample: None,
            check_observed: true,
            verification_observed: false,
            correction: None,
        });
        assert!(phase.matching(1, origin).is_err());
        assert!(phase.matching(2, origin + RECOVERY_TIMEOUT).is_err());
        let (mut session, correction) = begin(origin);
        phase.round.as_mut().unwrap().correction = Some(correction);
        assert!(
            phase
                .ready(2, negative_zero, Some(PlaybackState::Playing), origin)
                .is_err()
        );
        let mut wrong_identity = negative_zero;
        wrong_identity.source_id += 1;
        assert!(
            phase
                .update(
                    &mut session,
                    PlayerId::P1,
                    Some(wrong_identity),
                    Some(PlaybackState::Playing),
                    origin,
                    origin
                )
                .is_err()
        );
        assert!(
            phase
                .update(
                    &mut session,
                    PlayerId::P1,
                    None,
                    Some(PlaybackState::Playing),
                    origin,
                    origin
                )
                .is_err()
        );
    }
}
