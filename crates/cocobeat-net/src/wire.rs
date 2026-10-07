use std::time::Duration;

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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Control {
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
    Start {
        epoch: u64,
    },
    StartAck {
        epoch: u64,
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
}

impl Input {
    fn validate(&self) -> Result<(), String> {
        if let Self::Facts { facts, .. } = self
            && !(1..=64).contains(&facts.len())
        {
            return Err("input batch must contain 1 to 64 facts".into());
        }
        Ok(())
    }
}

pub(crate) async fn recv_control(
    stream: &mut RecvStream,
    count: &mut usize,
) -> Result<Control, String> {
    count_control(count)?;
    recv_frame(stream, None).await
}

pub(crate) async fn send_control(
    stream: &mut SendStream,
    count: &mut usize,
    message: &Control,
) -> Result<(), String> {
    count_control(count)?;
    send_frame(stream, None, message).await
}

pub(crate) async fn recv_input(stream: &mut RecvStream, bytes: &mut u64) -> Result<Input, String> {
    let input: Input = recv_frame(stream, Some(bytes)).await?;
    input.validate()?;
    Ok(input)
}

pub(crate) async fn send_input(
    stream: &mut SendStream,
    bytes: &mut u64,
    message: &Input,
) -> Result<(), String> {
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
) -> Result<T, String> {
    // A cancelled partial frame is terminal; the session must not reuse this stream
    tokio::time::timeout(FRAME_TIMEOUT, async {
        let mut prefix = [0; 4];
        stream
            .read_exact(&mut prefix)
            .await
            .map_err(|error| format!("read frame length: {error}"))?;
        let length = u32::from_be_bytes(prefix) as usize;
        reserve_frame(length, bytes)?;
        let mut body = vec![0; length];
        stream
            .read_exact(&mut body)
            .await
            .map_err(|error| format!("read frame body: {error}"))?;
        serde_json::from_slice(&body).map_err(|_| "decode frame: invalid JSON message".to_owned())
    })
    .await
    .map_err(|_| "frame read timed out".to_owned())?
}

async fn send_frame<T: Serialize>(
    stream: &mut SendStream,
    bytes: Option<&mut u64>,
    message: &T,
) -> Result<(), String> {
    let body = serde_json::to_vec(message).map_err(|error| format!("encode frame: {error}"))?;
    reserve_frame(body.len(), bytes)?;
    tokio::time::timeout(FRAME_TIMEOUT, async {
        stream
            .write_all(&(body.len() as u32).to_be_bytes())
            .await
            .map_err(|error| format!("write frame length: {error}"))?;
        stream
            .write_all(&body)
            .await
            .map_err(|error| format!("write frame body: {error}"))
    })
    .await
    .map_err(|_| "frame write timed out".to_owned())?
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
            },
            fact_count: 3,
        };
        let mut json = serde_json::to_value(&hello).unwrap();
        assert_eq!(
            serde_json::from_value::<Control>(json.clone()).unwrap(),
            hello
        );
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
