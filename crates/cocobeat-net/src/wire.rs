use std::{fmt, time::Duration};

use cocobeat_schema::{DuoInput, Hit, PlayerId, SessionEpoch, SongTime};
use quinn::{RecvStream, SendStream};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

const MAX_FRAME_BYTES: usize = 16 * 1024;
const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;
const FRAME_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Identity {
    pub content_id: String,
    pub canonical_frames: u64,
    pub content_schema: u32,
    pub ruleset_id: String,
    // A present null means a core-only session; missing fields are not v6
    #[serde(deserialize_with = "Option::deserialize")]
    pub stage_compiler_version: Option<u32>,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Control {
    Phase {
        epoch: u64,
        round: u16,
        attempt: u8,
        message: PhaseControl,
    },
    LiveFetch {
        protocol_version: u32,
        epoch: u64,
        player: u8,
        token: [u8; 32],
    },
    LiveHello {
        protocol_version: u32,
        epoch: u64,
        player: u8,
        token: [u8; 32],
        identity: Identity,
    },
    LiveWelcome {
        protocol_version: u32,
        epoch: u64,
        player: u8,
        identity: Identity,
        capability: [u8; 32],
    },
    LiveInstalled {
        epoch: u64,
        identity: Identity,
    },
    Armed {
        epoch: u64,
    },
    StartConfirmed {
        epoch: u64,
    },
    Fetch {
        protocol_version: u32,
        epoch: u64,
        player: u8,
        token: [u8; 32],
    },
    Resources {
        epoch: u64,
        objects: [ResourceObject; 4],
    },
    Installed {
        epoch: u64,
        identity: Identity,
        fact_count: u64,
    },
    Hello {
        protocol_version: u32,
        epoch: u64,
        player: u8,
        token: [u8; 32],
        identity: Identity,
        fact_count: u64,
    },
    Welcome {
        protocol_version: u32,
        epoch: u64,
        player: u8,
        identity: Identity,
        fact_count: u64,
    },
    InstalledAck {
        epoch: u64,
    },
    Ready {
        epoch: u64,
    },
    ClockSynced {
        epoch: u64,
        attempt: u8,
        id: u64,
        guest_send_ns: u64,
        host_receive_ns: u64,
        host_send_ns: u64,
        guest_receive_ns: u64,
    },
    ScheduleStart {
        epoch: u64,
        host_start_ns: u64,
    },
    ScheduleStartAck {
        epoch: u64,
        host_start_ns: u64,
        guest_now_ns: u64,
        guest_start_ns: u64,
        uncertainty_ns: u64,
    },
    ResumeHello {
        protocol_version: u32,
        epoch: u64,
        player: u8,
        identity: Identity,
        attempt: u8,
        pause_frame: i64,
        publication: PhasePublication,
        #[serde(deserialize_with = "Option::deserialize")]
        phase: Option<PhaseResume>,
        source_generation: u64,
        source_id: u64,
        owner_count: u64,
        started: bool,
        ended: bool,
        capability: [u8; 32],
    },
    ResumeWelcome {
        protocol_version: u32,
        epoch: u64,
        player: u8,
        identity: Identity,
        attempt: u8,
        pause_frame: i64,
        publication: PhasePublication,
        #[serde(deserialize_with = "Option::deserialize")]
        phase: Option<PhaseResume>,
        source_generation: u64,
        source_id: u64,
        owner_count: u64,
        started: bool,
        ended: bool,
    },
    ResumeTapeReady {
        epoch: u64,
        attempt: u8,
        counts: [u64; 2],
    },
    ResumeReady {
        epoch: u64,
        attempt: u8,
    },
    ResumeSchedule {
        epoch: u64,
        attempt: u8,
        host_common_ns: u64,
        host_verify_ns: u64,
        common_frame: i64,
        host_paused_frame: i64,
        guest_paused_frame: i64,
    },
    ResumeScheduleAck {
        epoch: u64,
        attempt: u8,
        host_common_ns: u64,
        common_frame: i64,
        guest_now_ns: u64,
        guest_resume_ns: u64,
        guest_common_ns: u64,
        guest_verify_ns: u64,
        uncertainty_ns: u64,
    },
    ResumeArmed {
        epoch: u64,
        attempt: u8,
    },
    ResumeConfirmed {
        epoch: u64,
        attempt: u8,
    },
    ResumeObserved {
        epoch: u64,
        attempt: u8,
        evidence: GateEvidence,
    },
    ResumeGate {
        epoch: u64,
        attempt: u8,
    },
    ResumeGateAck {
        epoch: u64,
        attempt: u8,
    },
    ResumeLive {
        epoch: u64,
        attempt: u8,
    },
    Finish {
        epoch: u64,
        bytes: u64,
        blake3: [u8; 32],
    },
    FinishAck {
        epoch: u64,
        blake3: [u8; 32],
    },
}

impl fmt::Debug for Control {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("Control");
        debug.field("kind", &std::mem::discriminant(self));
        match self {
            Self::LiveFetch { .. }
            | Self::LiveHello { .. }
            | Self::Fetch { .. }
            | Self::Hello { .. } => {
                debug.field("token", &"[REDACTED]");
            }
            Self::LiveWelcome { .. } | Self::ResumeHello { .. } => {
                debug.field("capability", &"[REDACTED]");
            }
            _ => {}
        }
        debug.finish_non_exhaustive()
    }
}

impl Control {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Phase {
                round,
                attempt,
                message,
                ..
            } => {
                if *attempt > 1
                    || (*round == 0
                        && matches!(message, PhaseControl::Source { .. })
                        && *attempt != 0)
                {
                    return Err("invalid phase connection attempt".into());
                }
                message.validate(*round)?;
            }
            Self::ClockSynced { attempt, .. } if *attempt > 1 => {
                return Err("clock attempt must be 0 or 1".into());
            }
            Self::ResumeHello {
                attempt,
                owner_count,
                publication,
                phase,
                ..
            }
            | Self::ResumeWelcome {
                attempt,
                owner_count,
                publication,
                phase,
                ..
            } => {
                validate_attempt(*attempt)?;
                validate_count(*owner_count)?;
                publication.validate()?;
                if let Some(phase) = phase {
                    phase.validate()?;
                }
            }
            Self::ResumeTapeReady {
                attempt, counts, ..
            } => {
                validate_attempt(*attempt)?;
                validate_count(
                    counts[0]
                        .checked_add(counts[1])
                        .ok_or("recovery fact count overflow")?,
                )?;
            }
            Self::ResumeObserved {
                attempt, evidence, ..
            } => {
                validate_attempt(*attempt)?;
                evidence.validate()?;
            }
            Self::ResumeReady { attempt, .. }
            | Self::ResumeSchedule { attempt, .. }
            | Self::ResumeScheduleAck { attempt, .. }
            | Self::ResumeArmed { attempt, .. }
            | Self::ResumeConfirmed { attempt, .. }
            | Self::ResumeGate { attempt, .. }
            | Self::ResumeGateAck { attempt, .. }
            | Self::ResumeLive { attempt, .. } => validate_attempt(*attempt)?,
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GateEvidence {
    // Local source identity; neither value is comparable across processes
    pub generation: u64,
    pub source_id: u64,
    pub progress_sequence: u64,
    pub observations: Vec<GateObservation>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GateObservation {
    pub sequence: u64,
    pub frame: i64,
    pub publication_before_ns: u64,
    pub publication_after_ns: u64,
}

impl GateEvidence {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !(1..=64).contains(&self.observations.len()) {
            return Err("recovery evidence must contain 1 to 64 observations".into());
        }
        for observation in &self.observations {
            if observation.sequence <= self.progress_sequence
                || observation.frame < 0
                || observation.publication_before_ns > observation.publication_after_ns
            {
                return Err("invalid recovery publication observation".into());
            }
        }
        for pair in self.observations.windows(2) {
            if pair[0].sequence >= pair[1].sequence
                || pair[0].frame > pair[1].frame
                || pair[0].publication_after_ns > pair[1].publication_before_ns
            {
                return Err("recovery observations are not monotonic".into());
            }
        }
        Ok(())
    }
}

fn validate_attempt(attempt: u8) -> Result<(), String> {
    if attempt != 1 {
        return Err("recovery attempt must be 1".into());
    }
    Ok(())
}

fn validate_count(count: u64) -> Result<(), String> {
    if count > cocobeat_replay::MAX_FACTS as u64 {
        return Err("recovery fact count exceeds replay limit".into());
    }
    Ok(())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResourceObject {
    pub bytes: u64,
    pub blake3: [u8; 32],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Fact {
    Hit { seq: u64, frame: i64 },
    Watermark { through: i64 },
}

impl Fact {
    pub fn into_input(self, epoch: SessionEpoch, player: PlayerId) -> DuoInput {
        match self {
            Self::Hit { seq, frame } => DuoInput::Hit(Hit {
                epoch,
                player,
                seq,
                song_time: SongTime::from_frames(frame),
            }),
            Self::Watermark { through } => DuoInput::Watermark {
                epoch,
                player,
                through: SongTime::from_frames(through),
            },
        }
    }

    pub fn from_input(input: DuoInput) -> Self {
        match input {
            DuoInput::Hit(hit) => Self::Hit {
                seq: hit.seq,
                frame: hit.song_time.frames(),
            },
            DuoInput::Watermark { through, .. } => Self::Watermark {
                through: through.frames(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PhaseAnchor {
    pub clock_round: u64,
    pub guest_send_ns: u64,
    pub host_receive_ns: u64,
    pub host_send_ns: u64,
    pub guest_receive_ns: u64,
}

impl PhaseAnchor {
    pub(crate) fn exchange(self, epoch: u64) -> crate::clock::ClockExchange {
        crate::clock::ClockExchange {
            epoch: SessionEpoch(epoch),
            guest_send_ns: self.guest_send_ns,
            host_receive_ns: self.host_receive_ns,
            host_send_ns: self.host_send_ns,
            guest_receive_ns: self.guest_receive_ns,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PhaseProofStage {
    Check,
    Correction,
    ReconnectVerification,
}

/// Fixed original round metadata, never an alternate recovery attempt
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PhaseResume {
    pub round: u16,
    pub host_deadline_ns: u64,
    pub host_point_ns: u64,
    pub stage: PhaseProofStage,
    pub corrections: u8,
    pub sealed: bool,
    pub anchor: PhaseAnchor,
    pub sources: [(u64, u64); 2],
    pub previous: [Option<PhasePublication>; 2],
    pub markers: [Option<u64>; 2],
}

impl PhaseResume {
    pub(crate) fn validate(self) -> Result<(), String> {
        if !(1..=crate::sync::MAX_PHASE_ROUNDS).contains(&self.round)
            || self.corrections > crate::sync::MAX_PHASE_CORRECTIONS
            || self.stage == PhaseProofStage::ReconnectVerification
            || self
                .sources
                .iter()
                .any(|&(generation, source_id)| generation == 0 || source_id == 0)
            || self.host_deadline_ns <= self.host_point_ns
            || self.host_point_ns <= self.anchor.host_send_ns
            || !(1..=crate::sync::MAX_MAINTENANCE_ROUNDS).contains(&self.anchor.clock_round)
            || self.anchor.guest_receive_ns <= self.anchor.guest_send_ns
            || self.anchor.host_send_ns < self.anchor.host_receive_ns
        {
            return Err("invalid original phase continuation metadata".into());
        }
        for publication in self.previous.into_iter().flatten() {
            publication.validate()?;
        }
        for count in self.markers.into_iter().flatten() {
            validate_count(count)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PhaseControl {
    Source {
        generation: u64,
        source_id: u64,
        publication: PhasePublication,
    },
    Check {
        anchor: PhaseAnchor,
        host_point_ns: u64,
        host_deadline_ns: u64,
    },
    Evidence {
        anchor: PhaseAnchor,
        host_point_ns: u64,
        verification: bool,
        evidence: PhaseEvidence,
    },
    Gate {
        anchor: PhaseAnchor,
        host_point_ns: u64,
        verification: bool,
        host_received_ns: u64,
        correcting: bool,
    },
    GateAck {
        anchor: PhaseAnchor,
        host_point_ns: u64,
        verification: bool,
    },
    Pause {},
    Frozen {
        frame: i64,
        generation: u64,
        source_id: u64,
        publication: PhasePublication,
        owner_count: u64,
    },
    Schedule {
        anchor: PhaseAnchor,
        host_common_ns: u64,
        host_verify_ns: u64,
        common_frame: i64,
        paused: [i64; 2],
    },
    ScheduleAck {
        anchor: PhaseAnchor,
        host_common_ns: u64,
        common_frame: i64,
        guest_now_ns: u64,
        guest_resume_ns: u64,
        guest_common_ns: u64,
        guest_verify_ns: u64,
        uncertainty_ns: u64,
    },
    Armed {},
    Confirmed {},
    Verify {
        anchor: PhaseAnchor,
        host_point_ns: u64,
    },
    Live {
        anchor: PhaseAnchor,
        host_point_ns: u64,
        verification: bool,
    },
    /// Drain the existing control reader before the unchanged Finish exchange
    Ended {
        owner_count: u64,
    },
}

impl PhaseControl {
    fn validate(&self, round: u16) -> Result<(), String> {
        let initial = matches!(self, Self::Source { .. } | Self::Ended { .. });
        if (initial && round != 0) || (!initial && !(1..=128).contains(&round)) {
            return Err("phase control round differs from its message domain".into());
        }
        match self {
            Self::Source {
                generation,
                source_id,
                publication,
            }
            | Self::Frozen {
                generation,
                source_id,
                publication,
                ..
            } => {
                let cursor = f64::from_bits(publication.position_seconds_bits);
                if *generation == 0
                    || *source_id == 0
                    || publication.sequence == 0
                    || !cursor.is_finite()
                    || cursor < 0.0
                    || publication.publication_before_ns > publication.publication_after_ns
                {
                    return Err("invalid phase original source publication".into());
                }
            }
            Self::Evidence { evidence, .. } => evidence.validate()?,
            _ => {}
        }
        match self {
            Self::Check { anchor, .. }
            | Self::Evidence { anchor, .. }
            | Self::Gate { anchor, .. }
            | Self::GateAck { anchor, .. }
            | Self::Schedule { anchor, .. }
            | Self::ScheduleAck { anchor, .. }
            | Self::Verify { anchor, .. }
            | Self::Live { anchor, .. }
                if !(1..=crate::sync::MAX_MAINTENANCE_ROUNDS).contains(&anchor.clock_round)
                    || anchor.guest_receive_ns <= anchor.guest_send_ns
                    || anchor.host_send_ns < anchor.host_receive_ns =>
            {
                return Err("invalid phase actual clock anchor".into());
            }
            _ => {}
        }
        if let Self::Frozen { owner_count, .. } | Self::Ended { owner_count } = self {
            validate_count(*owner_count)?;
        }
        Ok(())
    }
}

/// Per-direction lifetime phase budget, preserved across the one reconnect
#[derive(Clone, Default)]
pub(crate) struct PhaseBudget {
    messages: u16,
    bytes: u64,
    round: u16,
    round_messages: u8,
    source: bool,
    ended: bool,
}

impl PhaseBudget {
    /// Conservatively spend the one incomplete read if a connection is abandoned
    pub(crate) fn pending_read(&mut self) -> Result<(), String> {
        if self.ended || self.messages >= 2048 {
            return Err("phase read budget exhausted or ended".into());
        }
        self.bytes = self
            .bytes
            .checked_add((MAX_FRAME_BYTES + 4) as u64)
            .filter(|bytes| *bytes <= MAX_INPUT_BYTES)
            .ok_or("phase pending read byte budget exceeded")?;
        self.messages += 1;
        Ok(())
    }

    pub(crate) fn reserve(
        &mut self,
        round: u16,
        message: &PhaseControl,
        frame_bytes: u64,
    ) -> Result<(), String> {
        message.validate(round)?;
        if self.ended
            || self.messages >= 2048
            || !(5..=(MAX_FRAME_BYTES + 4) as u64).contains(&frame_bytes)
        {
            return Err("phase message budget exhausted or already ended".into());
        }
        let bytes = self
            .bytes
            .checked_add(frame_bytes)
            .filter(|bytes| *bytes <= MAX_INPUT_BYTES)
            .ok_or("phase lifetime byte budget exceeded")?;
        if round != 0 {
            if round < self.round || round > self.round.saturating_add(1) {
                return Err("phase message round replays or skips its bounded predecessor".into());
            }
            let count = if round == self.round {
                self.round_messages
            } else {
                0
            };
            if count >= 16 {
                return Err("phase per-round message budget exceeded".into());
            }
            self.round = round;
            self.round_messages = count + 1;
        } else if matches!(message, PhaseControl::Source { .. }) {
            if self.source || self.round != 0 {
                return Err("phase source identity repeated or late".into());
            }
            self.source = true;
        } else {
            self.ended = true;
        }
        self.messages += 1;
        self.bytes = bytes;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Input {
    PhasePaused {
        epoch: u64,
        round: u16,
        attempt: u8,
        fact_count: u64,
    },
    Open {
        epoch: u64,
        player: u8,
    },
    Facts {
        epoch: u64,
        facts: Vec<Fact>,
    },
    End {
        epoch: u64,
        fact_count: u64,
        final_through: i64,
    },
    ResumeOpen {
        epoch: u64,
        attempt: u8,
        player: u8,
        count: u64,
    },
    ResumeFacts {
        epoch: u64,
        attempt: u8,
        from_fact_index: u64,
        facts: Vec<Fact>,
    },
    ResumeEnd {
        epoch: u64,
        attempt: u8,
        count: u64,
        blake3: [u8; 32],
    },
}

impl Input {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::PhasePaused {
                round,
                attempt,
                fact_count,
                ..
            } => {
                if *attempt > 1 || !(1..=128).contains(round) {
                    return Err("invalid phase FIFO marker round".into());
                }
                validate_count(*fact_count)?;
            }
            Self::Facts { facts, .. } | Self::ResumeFacts { facts, .. }
                if !(1..=64).contains(&facts.len()) =>
            {
                return Err("input batch must contain 1 to 64 facts".into());
            }
            _ => {}
        }
        match self {
            Self::ResumeOpen {
                attempt,
                player,
                count,
                ..
            } => {
                validate_attempt(*attempt)?;
                validate_count(*count)?;
                if ![1, 2].contains(player) {
                    return Err("recovery player must be 1 or 2".into());
                }
            }
            Self::ResumeEnd { attempt, count, .. } => {
                validate_attempt(*attempt)?;
                validate_count(*count)?;
            }
            Self::ResumeFacts {
                attempt,
                from_fact_index,
                facts,
                ..
            } => {
                validate_attempt(*attempt)?;
                validate_count(
                    from_fact_index
                        .checked_add(facts.len() as u64)
                        .ok_or("recovery fact index overflow")?,
                )?;
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) enum LiveIoError {
    Transport(quinn::ConnectionError),
    Deadline,
    Invalid(String),
}

impl fmt::Display for LiveIoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(formatter, "connection lost: {error}"),
            Self::Deadline => formatter.write_str("frame I/O timed out"),
            Self::Invalid(error) => formatter.write_str(error),
        }
    }
}

impl From<String> for LiveIoError {
    fn from(error: String) -> Self {
        Self::Invalid(error)
    }
}

impl From<quinn::ReadExactError> for LiveIoError {
    fn from(error: quinn::ReadExactError) -> Self {
        match error {
            quinn::ReadExactError::ReadError(quinn::ReadError::ConnectionLost(error)) => {
                Self::Transport(error)
            }
            _ => Self::Invalid(format!("read frame: {error}")),
        }
    }
}

impl From<quinn::WriteError> for LiveIoError {
    fn from(error: quinn::WriteError) -> Self {
        match error {
            quinn::WriteError::ConnectionLost(error) => Self::Transport(error),
            _ => Self::Invalid(format!("write frame: {error}")),
        }
    }
}

pub(crate) async fn recv_control(
    stream: &mut RecvStream,
    count: &mut usize,
) -> Result<Control, String> {
    count_control(count)?;
    let message: Control = recv_frame(stream, None)
        .await
        .map_err(|error| error.to_string())?;
    message.validate()?;
    Ok(message)
}

pub(crate) async fn send_control(
    stream: &mut SendStream,
    count: &mut usize,
    message: &Control,
) -> Result<(), String> {
    count_control(count)?;
    message.validate()?;
    send_frame(stream, None, message)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn recv_input(stream: &mut RecvStream, bytes: &mut u64) -> Result<Input, String> {
    recv_live_input(stream, bytes)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn send_input(
    stream: &mut SendStream,
    bytes: &mut u64,
    message: &Input,
) -> Result<(), String> {
    send_live_input(stream, bytes, message)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn recv_live_input(
    stream: &mut RecvStream,
    bytes: &mut u64,
) -> Result<Input, LiveIoError> {
    let message: Input = recv_frame(stream, Some(bytes)).await?;
    message.validate()?;
    Ok(message)
}

pub(crate) async fn send_live_input(
    stream: &mut SendStream,
    bytes: &mut u64,
    message: &Input,
) -> Result<(), LiveIoError> {
    message.validate()?;
    send_frame(stream, Some(bytes), message).await
}

pub(crate) async fn ensure_eof(stream: &mut RecvStream) -> Result<(), String> {
    let mut byte = [0];
    match tokio::time::timeout(FRAME_TIMEOUT, stream.read(&mut byte))
        .await
        .map_err(|_| "stream EOF timed out".to_owned())?
        .map_err(|error| format!("read stream EOF: {error}"))?
    {
        None => Ok(()),
        Some(_) => Err("unexpected bytes after stream end message".into()),
    }
}

fn count_control(count: &mut usize) -> Result<(), String> {
    if *count >= 16 {
        return Err("control message limit exceeded".into());
    }
    *count += 1;
    Ok(())
}

fn reserve_frame(length: usize, bytes: Option<&mut u64>) -> Result<(), String> {
    if !(1..=MAX_FRAME_BYTES).contains(&length) {
        return Err("frame length must be 1 to 16384 bytes".into());
    }
    if let Some(bytes) = bytes {
        let next = bytes
            .checked_add(4 + length as u64)
            .filter(|next| *next <= MAX_INPUT_BYTES)
            .ok_or_else(|| "input byte limit exceeded".to_owned())?;
        *bytes = next;
    }
    Ok(())
}

async fn recv_frame<T: DeserializeOwned>(
    stream: &mut RecvStream,
    bytes: Option<&mut u64>,
) -> Result<T, LiveIoError> {
    let body = recv_body(stream, bytes).await?;
    serde_json::from_slice(&body)
        .map_err(|_| LiveIoError::Invalid("decode frame: invalid JSON message".into()))
}

async fn recv_body(
    stream: &mut RecvStream,
    bytes: Option<&mut u64>,
) -> Result<Vec<u8>, LiveIoError> {
    // A cancelled partial frame is terminal; the session must not reuse this stream
    tokio::time::timeout(FRAME_TIMEOUT, async {
        let mut prefix = [0; 4];
        stream
            .read_exact(&mut prefix)
            .await
            .map_err(LiveIoError::from)?;
        let length = u32::from_be_bytes(prefix) as usize;
        reserve_frame(length, bytes)?;
        let mut body = vec![0; length];
        stream
            .read_exact(&mut body)
            .await
            .map_err(LiveIoError::from)?;
        Ok(body)
    })
    .await
    .map_err(|_| LiveIoError::Deadline)?
}

pub(crate) async fn recv_phase_control(
    stream: &mut RecvStream,
    budget: &mut PhaseBudget,
) -> Result<Control, LiveIoError> {
    let body = recv_body(stream, None).await?;
    let message: Control = serde_json::from_slice(&body)
        .map_err(|_| LiveIoError::Invalid("invalid phase control JSON".into()))?;
    message.validate()?;
    match &message {
        Control::Phase { round, message, .. } => {
            budget.reserve(*round, message, 4 + body.len() as u64)?
        }
        _ => {
            return Err(LiveIoError::Invalid(
                "non-phase control before phase reader drained".into(),
            ));
        }
    }
    Ok(message)
}

pub(crate) async fn send_phase_control(
    stream: &mut SendStream,
    budget: &mut PhaseBudget,
    message: &Control,
) -> Result<(), String> {
    message.validate()?;
    let size = serde_json::to_vec(message)
        .map_err(|_| "encode phase control failed")?
        .len();
    match message {
        Control::Phase { round, message, .. } => {
            budget.reserve(*round, message, 4 + size as u64)?
        }
        _ => return Err("non-phase control passed to phase sender".into()),
    }
    send_frame(stream, None, message)
        .await
        .map_err(|error| error.to_string())
}

async fn send_frame<T: Serialize>(
    stream: &mut SendStream,
    bytes: Option<&mut u64>,
    message: &T,
) -> Result<(), LiveIoError> {
    let body = serde_json::to_vec(message).map_err(|error| format!("encode frame: {error}"))?;
    reserve_frame(body.len(), bytes)?;
    tokio::time::timeout(FRAME_TIMEOUT, async {
        stream
            .write_all(&(body.len() as u32).to_be_bytes())
            .await
            .map_err(LiveIoError::from)?;
        stream.write_all(&body).await.map_err(LiveIoError::from)
    })
    .await
    .map_err(|_| LiveIoError::Deadline)?
}

/// Original coherent source publication; f64 bits survive JSON without rounding
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PhasePublication {
    pub sequence: u64,
    pub position_seconds_bits: u64,
    pub publication_before_ns: u64,
    pub publication_after_ns: u64,
}

impl PhasePublication {
    pub(crate) fn validate(self) -> Result<(), String> {
        let position = f64::from_bits(self.position_seconds_bits);
        if self.sequence == 0
            || !position.is_finite()
            || position < 0.0
            || self.publication_before_ns > self.publication_after_ns
        {
            return Err("invalid original coherent phase publication".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PhaseEvidence {
    pub generation: u64,
    pub source_id: u64,
    pub collected_at_ns: u64,
    pub publications: Vec<PhasePublication>,
}

impl PhaseEvidence {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.generation == 0
            || self.source_id == 0
            || !(2..=64).contains(&self.publications.len())
        {
            return Err("phase source identity or publication count is invalid".into());
        }
        let mut previous: Option<PhasePublication> = None;
        for &row in &self.publications {
            let position = f64::from_bits(row.position_seconds_bits);
            if row.sequence == 0
                || !position.is_finite()
                || position < 0.0
                || row.publication_before_ns > row.publication_after_ns
                || row.publication_after_ns > self.collected_at_ns
                || previous.is_some_and(|last| {
                    row.sequence <= last.sequence
                        || row.publication_before_ns < last.publication_after_ns
                        || position < f64::from_bits(last.position_seconds_bits)
                })
            {
                return Err(
                    "phase source publication moved backwards or changed within one sequence"
                        .into(),
                );
            }
            previous = Some(row);
        }
        if f64::from_bits(self.publications.last().unwrap().position_seconds_bits)
            <= f64::from_bits(self.publications.first().unwrap().position_seconds_bits)
        {
            return Err("phase source has no observed progress".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_envelopes_bound_full_raw_cursor_batch_and_strict_identity() {
        let anchor = PhaseAnchor {
            clock_round: 1,
            guest_send_ns: 1,
            host_receive_ns: 2,
            host_send_ns: 3,
            guest_receive_ns: 4,
        };
        let rows = PhaseEvidence {
            generation: u64::MAX,
            source_id: u64::MAX,
            collected_at_ns: u64::MAX,
            publications: (0..64)
                .map(|index| PhasePublication {
                    sequence: u64::MAX - 64 + index,
                    position_seconds_bits: (index as f64 + 1.0).to_bits(),
                    publication_before_ns: u64::MAX - 128 + index * 2,
                    publication_after_ns: u64::MAX - 127 + index * 2,
                })
                .collect(),
        };
        let envelope = Control::Phase {
            epoch: u64::MAX,
            round: 128,
            attempt: 0,
            message: PhaseControl::Evidence {
                anchor,
                host_point_ns: u64::MAX,
                verification: true,
                evidence: rows.clone(),
            },
        };
        envelope.validate().unwrap();
        let bytes = serde_json::to_vec(&envelope).unwrap();
        assert!(
            bytes.len() <= MAX_FRAME_BYTES,
            "complete 64-row raw evidence fits one existing frame"
        );
        assert_eq!(serde_json::from_slice::<Control>(&bytes).unwrap(), envelope);
        let mut too_many = rows;
        too_many
            .publications
            .push(*too_many.publications.last().unwrap());
        assert!(
            Control::Phase {
                epoch: 9,
                round: 1,
                attempt: 0,
                message: PhaseControl::Evidence {
                    anchor,
                    host_point_ns: 1,
                    verification: false,
                    evidence: too_many
                }
            }
            .validate()
            .is_err()
        );
        for invalid in [
            r#"{"kind":"phase","epoch":9,"round":0,"attempt":0,"message":{"kind":"pause"}}"#,
            r#"{"kind":"phase","epoch":9,"round":129,"attempt":0,"message":{"kind":"pause"}}"#,
            r#"{"kind":"phase","epoch":9,"round":1,"attempt":0,"message":{"kind":"source","generation":1,"source_id":1,"publication":{"sequence":1,"position_seconds_bits":0,"publication_before_ns":1,"publication_after_ns":2}}}"#,
        ] {
            assert!(
                serde_json::from_str::<Control>(invalid)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        for invalid in [
            r#"{"kind":"phase","epoch":9,"round":1,"attempt":0,"message":{"kind":"pause","extra":0}}"#,
            r#"{"kind":"phase","epoch":9,"round":1,"attempt":0,"message":{"kind":"armed","extra":0}}"#,
            r#"{"kind":"phase","epoch":9,"round":1,"attempt":0,"message":{"kind":"confirmed","extra":0}}"#,
            r#"{"kind":"phase","epoch":9,"round":1,"attempt":0,"round":2,"message":{"kind":"pause"}}"#,
            r#"{"kind":"phase_paused","epoch":9,"round":1,"attempt":0,"fact_count":0,"extra":0}"#,
        ] {
            assert!(
                serde_json::from_str::<Control>(invalid).is_err(),
                "{invalid}"
            );
        }
        assert!(
            serde_json::from_str::<Input>(
                r#"{"kind":"phase_paused","epoch":9,"round":1,"attempt":0,"fact_count":0,"extra":0}"#
            )
            .is_err()
        );
        assert_eq!(
            serde_json::from_str::<Input>(
                r#"{"kind":"phase_paused","epoch":9,"round":1,"attempt":0,"fact_count":0}"#
            )
            .unwrap(),
            Input::PhasePaused {
                epoch: 9,
                round: 1,
                attempt: 0,
                fact_count: 0
            }
        );
        assert!(
            Input::PhasePaused {
                epoch: 9,
                round: 0,
                attempt: 0,
                fact_count: 0
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn phase_message_budget_preserves_base_sixteen_and_finite_rounds_bytes_end() {
        let pause = PhaseControl::Pause {};
        let mut base = 0;
        for _ in 0..16 {
            count_control(&mut base).unwrap();
        }
        assert!(count_control(&mut base).is_err());
        let mut budget = PhaseBudget::default();
        for round in 1..=128 {
            for _ in 0..16 {
                budget.reserve(round, &pause, 100).unwrap();
            }
            assert!(budget.reserve(round, &pause, 100).is_err());
        }
        assert_eq!(budget.messages, 2048);
        assert!(budget.reserve(128, &pause, 100).is_err());
        let mut budget = PhaseBudget::default();
        assert!(budget.reserve(2, &pause, 100).is_err());
        budget.reserve(1, &pause, 100).unwrap();
        budget.reserve(2, &pause, 100).unwrap();
        assert!(budget.reserve(1, &pause, 100).is_err());
        assert!(budget.reserve(2, &pause, 0).is_err());
        assert!(
            budget
                .reserve(2, &pause, MAX_FRAME_BYTES as u64 + 5)
                .is_err()
        );
        let mut near_cap = PhaseBudget {
            bytes: MAX_INPUT_BYTES - 5,
            ..Default::default()
        };
        assert!(near_cap.reserve(1, &pause, 6).is_err());
        near_cap.reserve(1, &pause, 5).unwrap();
        assert!(near_cap.reserve(1, &pause, 5).is_err());
        let mut budget = PhaseBudget::default();
        budget
            .reserve(0, &PhaseControl::Ended { owner_count: 0 }, 50)
            .unwrap();
        assert!(budget.reserve(1, &pause, 100).is_err());
        assert!(
            budget
                .reserve(0, &PhaseControl::Ended { owner_count: 0 }, 50)
                .is_err()
        );
    }

    #[test]
    fn phase_original_cursor_bits_and_identity_survive_reliable_json() {
        let evidence = PhaseEvidence {
            generation: 11,
            source_id: 21,
            collected_at_ns: 3_000,
            publications: vec![
                PhasePublication {
                    sequence: 1,
                    position_seconds_bits: 0.10000000000000003_f64.to_bits(),
                    publication_before_ns: 1_000,
                    publication_after_ns: 1_001,
                },
                PhasePublication {
                    sequence: 2,
                    position_seconds_bits: 0.20000000000000007_f64.to_bits(),
                    publication_before_ns: 2_000,
                    publication_after_ns: 2_001,
                },
            ],
        };
        evidence.validate().unwrap();
        let encoded = serde_json::to_vec(&evidence).unwrap();
        assert_eq!(
            serde_json::from_slice::<PhaseEvidence>(&encoded).unwrap(),
            evidence
        );
        let mut frozen = evidence.clone();
        frozen.publications[1].position_seconds_bits = frozen.publications[0].position_seconds_bits;
        assert!(frozen.validate().is_err());
        frozen.publications[0].position_seconds_bits = (-0.0_f64).to_bits();
        frozen.publications[1].position_seconds_bits = 0.0_f64.to_bits();
        assert!(
            frozen.validate().is_err(),
            "bit identity is not source progress"
        );
    }

    #[test]
    fn strict_messages_reject_ambiguous_or_unbounded_facts() {
        let ready = br#"{"kind":"ready","epoch":18446744073709551615}"#;
        assert_eq!(
            serde_json::from_slice::<Control>(ready).unwrap(),
            Control::Ready { epoch: u64::MAX }
        );
        for invalid in [
            r#"{"kind":"ready","epoch":1,"extra":0}"#,
            r#"{"kind":"ready","epoch":1,"epoch":2}"#,
            r#"{"kind":"ready","kind":"start","epoch":1}"#,
            r#"{"kind":"unknown","epoch":1}"#,
            r#"{"kind":"ready","epoch":-1}"#,
            r#"{"kind":"ready","epoch":1.0}"#,
            r#"{"kind":"ready","epoch":18446744073709551616}"#,
            r#"{"kind":"ready","epoch":1} {}"#,
            r#"{"kind":"ready","epoch":1"#,
        ] {
            assert!(
                serde_json::from_str::<Control>(invalid).is_err(),
                "{invalid}"
            );
        }
        for invalid in [
            r#"{"kind":"facts","epoch":1,"facts":[{"kind":"hit","seq":0,"frame":1,"player":1}]}"#,
            r#"{"kind":"facts","epoch":1,"facts":[{"kind":"hit","seq":0,"frame":1,"frame":2}]}"#,
            r#"{"kind":"facts","epoch":1,"facts":[{"kind":"watermark","through":9223372036854775808}]}"#,
            r#"{"kind":"open","epoch":1,"player":256}"#,
        ] {
            assert!(serde_json::from_str::<Input>(invalid).is_err(), "{invalid}");
        }
        for length in [0, 1, 64, 65] {
            let input = Input::Facts {
                epoch: 1,
                facts: vec![Fact::Watermark { through: -1 }; length],
            };
            let decoded: Input =
                serde_json::from_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
            assert_eq!(decoded.validate().is_ok(), (1..=64).contains(&length));
        }
        let hello = Control::Hello {
            protocol_version: 1,
            epoch: 2,
            player: 2,
            token: [7; 32],
            identity: Identity {
                content_id: "package".into(),
                canonical_frames: 48_000,
                content_schema: 1,
                ruleset_id: "duo-watermark-v1".into(),
                stage_compiler_version: None,
            },
            fact_count: 3,
        };
        let mut json = serde_json::to_value(&hello).unwrap();
        assert_eq!(
            serde_json::from_value::<Control>(json.clone()).unwrap(),
            hello
        );
        let mut missing_stage = json.clone();
        missing_stage["identity"]
            .as_object_mut()
            .unwrap()
            .remove("stage_compiler_version");
        assert!(serde_json::from_value::<Control>(missing_stage).is_err());
        json["identity"]["extra"] = 0.into();
        assert!(serde_json::from_value::<Control>(json).is_err());
        let mut json = serde_json::to_value(&hello).unwrap();
        json["token"].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<Control>(json).is_err());
        let resources = Control::Resources {
            epoch: 2,
            objects: [ResourceObject {
                bytes: 1,
                blake3: [0; 32],
            }; 4],
        };
        let valid = serde_json::to_value(&resources).unwrap();
        assert_eq!(
            serde_json::from_value::<Control>(valid.clone()).unwrap(),
            resources
        );
        for count in [0, 3, 5] {
            let mut invalid = valid.clone();
            invalid["objects"] = serde_json::json!(vec![
                ResourceObject {
                    bytes: 1,
                    blake3: [0; 32]
                };
                count
            ]);
            assert!(serde_json::from_value::<Control>(invalid).is_err());
        }
        let mut invalid = valid;
        invalid["objects"][0]["file_name"] = "../untrusted".into();
        assert!(serde_json::from_value::<Control>(invalid).is_err());
    }

    #[test]
    fn recovery_messages_bound_attempt_prefix_and_every_observation() {
        let identity = Identity {
            content_id: "package".into(),
            canonical_frames: 48_000,
            content_schema: 1,
            ruleset_id: "duo-watermark-v1".into(),
            stage_compiler_version: None,
        };
        let hello = Control::ResumeHello {
            protocol_version: 6,
            epoch: 9,
            player: 2,
            identity: identity.clone(),
            attempt: 1,
            pause_frame: 3_000,
            publication: PhasePublication {
                sequence: 1,
                position_seconds_bits: (3000.0_f64 / 48_000.0).to_bits(),
                publication_before_ns: 1,
                publication_after_ns: 2,
            },
            phase: None,
            source_generation: 7,
            source_id: 8,
            owner_count: 3,
            started: true,
            ended: false,
            capability: [211; 32],
        };
        let evidence = GateEvidence {
            generation: 7,
            source_id: 8,
            progress_sequence: 1,
            observations: (0..64)
                .map(|index| GateObservation {
                    sequence: index + 2,
                    frame: index as i64,
                    publication_before_ns: index * 10,
                    publication_after_ns: index * 10 + 1,
                })
                .collect(),
        };
        for message in [
            hello.clone(),
            Control::ResumeWelcome {
                protocol_version: 6,
                epoch: 9,
                player: 1,
                identity: identity.clone(),
                attempt: 1,
                pause_frame: 3_000,
                publication: PhasePublication {
                    sequence: 1,
                    position_seconds_bits: (3000.0_f64 / 48_000.0).to_bits(),
                    publication_before_ns: 1,
                    publication_after_ns: 2,
                },
                phase: None,
                source_generation: 7,
                source_id: 8,
                owner_count: 3,
                started: true,
                ended: false,
            },
            Control::ResumeTapeReady {
                epoch: 9,
                attempt: 1,
                counts: [3, 4],
            },
            Control::ResumeReady {
                epoch: 9,
                attempt: 1,
            },
            Control::ResumeSchedule {
                epoch: 9,
                attempt: 1,
                host_common_ns: 1_000_000_000,
                host_verify_ns: 1_100_000_000,
                common_frame: 4_000,
                host_paused_frame: 3_000,
                guest_paused_frame: 4_000,
            },
            Control::ResumeScheduleAck {
                epoch: 9,
                attempt: 1,
                host_common_ns: 1_000_000_000,
                common_frame: 4_000,
                guest_now_ns: 100,
                guest_resume_ns: 1_000_000_010,
                guest_common_ns: 1_000_000_010,
                guest_verify_ns: 1_100_000_010,
                uncertainty_ns: 10,
            },
            Control::ResumeArmed {
                epoch: 9,
                attempt: 1,
            },
            Control::ResumeConfirmed {
                epoch: 9,
                attempt: 1,
            },
            Control::ResumeObserved {
                epoch: 9,
                attempt: 1,
                evidence: evidence.clone(),
            },
            Control::ResumeGate {
                epoch: 9,
                attempt: 1,
            },
            Control::ResumeGateAck {
                epoch: 9,
                attempt: 1,
            },
            Control::ResumeLive {
                epoch: 9,
                attempt: 1,
            },
        ] {
            let json = serde_json::to_value(&message).unwrap();
            assert_eq!(
                serde_json::from_value::<Control>(json.clone()).unwrap(),
                message
            );
            assert!(message.validate().is_ok());
            for attempt in [0, 2, u8::MAX] {
                let mut invalid = json.clone();
                invalid["attempt"] = attempt.into();
                assert!(
                    serde_json::from_value::<Control>(invalid)
                        .unwrap()
                        .validate()
                        .is_err()
                );
            }
            let mut invalid = json;
            invalid["extra"] = 0.into();
            assert!(serde_json::from_value::<Control>(invalid).is_err());
        }
        for secret in [
            hello.clone(),
            Control::LiveWelcome {
                protocol_version: 6,
                epoch: 9,
                player: 1,
                identity: identity.clone(),
                capability: [211; 32],
            },
            Control::LiveHello {
                protocol_version: 6,
                epoch: 9,
                player: 2,
                identity,
                token: [211; 32],
            },
        ] {
            let debug = format!("{secret:?}");
            assert!(debug.contains("[REDACTED]"));
            assert!(!debug.contains("211"));
        }
        let mut missing = serde_json::to_value(&hello).unwrap();
        missing.as_object_mut().unwrap().remove("source_generation");
        assert!(serde_json::from_value::<Control>(missing).is_err());
        for length in [0, 1, 64, 65] {
            let input = Input::ResumeFacts {
                epoch: 9,
                attempt: 1,
                from_fact_index: 0,
                facts: vec![Fact::Watermark { through: -1 }; length],
            };
            let decoded: Input =
                serde_json::from_value(serde_json::to_value(input).unwrap()).unwrap();
            assert_eq!(decoded.validate().is_ok(), (1..=64).contains(&length));
        }
        for message in [
            Input::ResumeOpen {
                epoch: 9,
                attempt: 1,
                player: 1,
                count: 2,
            },
            Input::ResumeFacts {
                epoch: 9,
                attempt: 1,
                from_fact_index: 0,
                facts: vec![Fact::Watermark { through: -1 }],
            },
            Input::ResumeEnd {
                epoch: 9,
                attempt: 1,
                count: 2,
                blake3: [0; 32],
            },
        ] {
            let json = serde_json::to_value(&message).unwrap();
            assert_eq!(
                serde_json::from_value::<Input>(json.clone()).unwrap(),
                message
            );
            assert!(message.validate().is_ok());
            for attempt in [0, 2, u8::MAX] {
                let mut invalid = json.clone();
                invalid["attempt"] = attempt.into();
                assert!(
                    serde_json::from_value::<Input>(invalid)
                        .unwrap()
                        .validate()
                        .is_err()
                );
            }
            let mut invalid = json;
            invalid["extra"] = 0.into();
            assert!(serde_json::from_value::<Input>(invalid).is_err());
        }
        for attempt in [0, 1, 2, u8::MAX] {
            let message = Control::ClockSynced {
                epoch: 9,
                attempt,
                id: 1,
                guest_send_ns: 2,
                host_receive_ns: 3,
                host_send_ns: 4,
                guest_receive_ns: 5,
            };
            assert_eq!(message.validate().is_ok(), attempt <= 1);
        }
        let maximum = cocobeat_replay::MAX_FACTS as u64;
        for count in [maximum + 1, u64::MAX] {
            assert!(
                Input::ResumeOpen {
                    epoch: 9,
                    attempt: 1,
                    player: 1,
                    count
                }
                .validate()
                .is_err()
            );
            assert!(
                Input::ResumeEnd {
                    epoch: 9,
                    attempt: 1,
                    count,
                    blake3: [0; 32]
                }
                .validate()
                .is_err()
            );
            assert!(
                Input::ResumeFacts {
                    epoch: 9,
                    attempt: 1,
                    from_fact_index: count,
                    facts: vec![Fact::Watermark { through: -1 }],
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            Control::ResumeTapeReady {
                epoch: 9,
                attempt: 1,
                counts: [maximum, 1]
            }
            .validate()
            .is_err()
        );
        for invalid in 0..6 {
            let mut value = evidence.clone();
            match invalid {
                0 => value.observations.clear(),
                1 => value.observations.push(*value.observations.last().unwrap()),
                2 => value.observations[32].frame = 0,
                3 => value.observations[32].sequence = value.observations[31].sequence,
                4 => value.observations[32].publication_before_ns = 0,
                _ => value.observations[0].sequence = value.progress_sequence,
            }
            assert!(value.validate().is_err());
        }
        let mut extra = serde_json::to_value(&evidence).unwrap();
        extra["observations"][0]["extra"] = 0.into();
        assert!(serde_json::from_value::<GateEvidence>(extra).is_err());
    }

    #[test]
    fn live_io_errors_preserve_quinn_connection_loss_only() {
        for error in [
            LiveIoError::from(quinn::ReadExactError::ReadError(
                quinn::ReadError::ConnectionLost(quinn::ConnectionError::TimedOut),
            )),
            LiveIoError::from(quinn::WriteError::ConnectionLost(
                quinn::ConnectionError::TimedOut,
            )),
        ] {
            assert!(matches!(
                error,
                LiveIoError::Transport(quinn::ConnectionError::TimedOut)
            ));
        }
        for error in [
            LiveIoError::from(quinn::ReadExactError::FinishedEarly(2)),
            LiveIoError::from(quinn::ReadExactError::ReadError(quinn::ReadError::Reset(
                quinn::VarInt::from_u32(1),
            ))),
            LiveIoError::from(quinn::WriteError::Stopped(quinn::VarInt::from_u32(1))),
            LiveIoError::from("connection lost: forged text".to_owned()),
        ] {
            assert!(matches!(error, LiveIoError::Invalid(_)));
        }
    }

    #[test]
    fn frame_and_direction_limits_include_prefix_and_reject_overflow() {
        for length in [0, MAX_FRAME_BYTES + 1, usize::MAX] {
            let mut bytes = 7;
            assert!(reserve_frame(length, Some(&mut bytes)).is_err());
            assert_eq!(bytes, 7);
        }
        assert!(reserve_frame(1, None).is_ok());
        assert!(reserve_frame(MAX_FRAME_BYTES, None).is_ok());
        let mut bytes = MAX_INPUT_BYTES - 5;
        reserve_frame(1, Some(&mut bytes)).unwrap();
        assert_eq!(bytes, MAX_INPUT_BYTES);
        assert!(reserve_frame(1, Some(&mut bytes)).is_err());
        assert_eq!(bytes, MAX_INPUT_BYTES);
        let mut overflow = u64::MAX;
        assert!(reserve_frame(1, Some(&mut overflow)).is_err());
        assert_eq!(overflow, u64::MAX);
        let mut count = 0;
        for _ in 0..16 {
            count_control(&mut count).unwrap();
        }
        assert!(count_control(&mut count).is_err());
        assert_eq!(count, 16);
        let mut overflow_count = usize::MAX;
        assert!(count_control(&mut overflow_count).is_err());
    }

    #[test]
    fn facts_preserve_integer_values_and_bind_only_the_given_seat() {
        for fact in [
            Fact::Hit {
                seq: u64::MAX,
                frame: i64::MIN,
            },
            Fact::Watermark { through: i64::MAX },
        ] {
            for player in [PlayerId::P1, PlayerId::P2] {
                let epoch = SessionEpoch(u64::MAX);
                let input = fact.into_input(epoch, player);
                match input {
                    DuoInput::Hit(hit) => assert_eq!((hit.epoch, hit.player), (epoch, player)),
                    DuoInput::Watermark {
                        epoch: actual,
                        player: seat,
                        ..
                    } => {
                        assert_eq!((actual, seat), (epoch, player));
                    }
                }
                assert_eq!(Fact::from_input(input), fact);
            }
        }
    }
    #[test]
    fn reconnect_wire_scope_and_metadata_are_explicit_strict_and_keep_sixteen_message_budget() {
        let envelope = Control::Phase {
            epoch: 9,
            round: 1,
            attempt: 1,
            message: PhaseControl::Pause {},
        };
        envelope.validate().unwrap();
        let mut value = serde_json::to_value(&envelope).unwrap();
        value.as_object_mut().unwrap().remove("attempt");
        assert!(serde_json::from_value::<Control>(value).is_err());
        assert!(
            Control::Phase {
                epoch: 9,
                round: 1,
                attempt: 2,
                message: PhaseControl::Pause {}
            }
            .validate()
            .is_err()
        );
        let mut budget = PhaseBudget::default();
        for _ in 0..12 {
            budget.reserve(1, &PhaseControl::Pause {}, 100).unwrap();
        }
        for _ in 0..4 {
            budget.reserve(1, &PhaseControl::Pause {}, 100).unwrap();
        }
        assert!(budget.reserve(1, &PhaseControl::Pause {}, 100).is_err());
        assert_eq!(budget.round_messages, 16);
        assert_eq!(budget.messages, 16);
        let marker = Input::PhasePaused {
            epoch: 9,
            round: 1,
            attempt: 1,
            fact_count: 7,
        };
        marker.validate().unwrap();
        assert!(
            Input::PhasePaused {
                epoch: 9,
                round: 1,
                attempt: 2,
                fact_count: 7
            }
            .validate()
            .is_err()
        );
    }
}
