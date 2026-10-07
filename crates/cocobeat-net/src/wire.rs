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
            Self::ClockSynced { attempt, .. } if *attempt > 1 => {
                return Err("clock attempt must be 0 or 1".into());
            }
            Self::ResumeHello {
                attempt,
                owner_count,
                ..
            }
            | Self::ResumeWelcome {
                attempt,
                owner_count,
                ..
            } => {
                validate_attempt(*attempt)?;
                validate_count(*owner_count)?;
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Input {
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
        serde_json::from_slice(&body)
            .map_err(|_| LiveIoError::Invalid("decode frame: invalid JSON message".into()))
    })
    .await
    .map_err(|_| LiveIoError::Deadline)?
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
