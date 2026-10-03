use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use cocobeat_core::DuoEngine;
use cocobeat_replay::{MAX_FACTS, MAX_FILE_BYTES, Replay, ReplayIdentity};
use cocobeat_schema::{Anchor, DuoInput, DuoRules, PlayerId, SessionEpoch};
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use serde::Serialize;
use tokio::{sync::mpsc, task::JoinSet, time::Instant};

use crate::{Invitation, PROTOCOL_VERSION, connect, listen, read_invite, wire, write_invite};
use wire::{Control, Fact, Identity, Input};

const RULESET: &str = "duo-watermark-v1";
const COMPLETE_CODE: u32 = 0x4342;
const COMPLETE_REASON: &[u8] = b"session complete";
const IDLE: Duration = Duration::from_secs(30);

#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub status: &'static str,
    pub protocol_version: u32,
    pub epoch: u64,
    pub player: u8,
    pub peer_authenticated: bool,
    pub content_id: String,
    pub endpoint: String,
    pub cert_blake3: String,
    pub template_blake3: String,
    pub template_epoch: u64,
    pub facts: [usize; 2],
    pub event_count: usize,
    pub live_replay_blake3: Option<String>,
    pub authority_replay_blake3: Option<String>,
    pub error: Option<String>,
}

struct Prepared {
    identity: Identity,
    anchors: Vec<Anchor>,
    local: Vec<Fact>,
    template_blake3: String,
    template_epoch: u64,
    end: i64,
    final_through: i64,
}

fn seat(input: DuoInput) -> PlayerId {
    match input {
        DuoInput::Hit(hit) => hit.player,
        DuoInput::Watermark { player, .. } => player,
    }
}

fn number(player: PlayerId) -> u8 {
    player.index() as u8 + 1
}

fn other(player: PlayerId) -> PlayerId {
    match player {
        PlayerId::P1 => PlayerId::P2,
        PlayerId::P2 => PlayerId::P1,
    }
}

fn validate_fact(fact: Fact, end: i64, final_through: i64) -> Result<(), String> {
    match fact {
        Fact::Hit { frame, .. } if !(0..end).contains(&frame) => {
            Err("Hit must be inside the song frame range".into())
        }
        Fact::Watermark { through } if through > final_through => {
            Err("watermark exceeds the song final watermark".into())
        }
        _ => Ok(()),
    }
}

fn prepare(package: &Path, template: &Path, player: PlayerId) -> Result<Prepared, String> {
    let package = cocobeat_media::validate_package(package)?;
    if package.chart.ruleset_id != RULESET {
        return Err("network sessions support only duo-watermark-v1".into());
    }
    let identity = Identity {
        content_id: format!(
            "package-blake3:{}",
            blake3::Hash::from_bytes(package.manifest.package_hash).to_hex()
        ),
        canonical_frames: package.manifest.canonical_frames,
        content_schema: package.manifest.schema_version,
        ruleset_id: package.chart.ruleset_id,
    };
    let end = i64::try_from(identity.canonical_frames).map_err(|_| "song frame overflow")?;
    let final_through = DuoRules::default()
        .confirmation_delay_frames()
        .and_then(|delay| end.checked_add(delay))
        .and_then(|through| through.checked_add(1))
        .ok_or("song final watermark overflow")?;
    let metadata = fs::metadata(template).map_err(|error| format!("read template: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err("template must be a regular Replay file of at most 20 MiB".into());
    }
    let file = File::open(template).map_err(|error| format!("open template: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|_| "read opened template metadata")?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err("opened template is not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "read template bytes failed")?;
    let replay = Replay::decode(bytes.as_slice()).map_err(|_| "invalid Replay template")?;
    replay
        .replay(
            &identity.content_id,
            RULESET,
            package.chart.anchors.clone(),
            DuoRules::default(),
        )
        .map_err(|_| "template content, rules, epoch or history is invalid")?;
    for input in replay.facts() {
        validate_fact(Fact::from_input(*input), end, final_through)?;
    }
    let local: Vec<_> = replay
        .facts()
        .iter()
        .filter(|input| seat(**input) == player)
        .map(|input| Fact::from_input(*input))
        .collect();
    if local.last()
        != Some(&Fact::Watermark {
            through: final_through,
        })
    {
        return Err("local template must end with the explicit song final watermark".into());
    }
    Ok(Prepared {
        identity,
        anchors: package.chart.anchors,
        local,
        template_blake3: blake3::hash(&bytes).to_hex().to_string(),
        template_epoch: replay.epoch().0,
        end,
        final_through,
    })
}

fn destination_outside_package(package: &Path, destination: &Path) -> Result<PathBuf, String> {
    let root = fs::canonicalize(package).map_err(|_| "resolve source package path failed")?;
    let name = destination
        .file_name()
        .ok_or("destination requires a file name")?;
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|_| "destination parent must already exist")?;
    let resolved = parent.join(name);
    if resolved.starts_with(root) {
        return Err("session output and invitation must be outside the source package".into());
    }
    Ok(resolved)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("create session evidence: {error}"))?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = result {
        fs::remove_file(path).map_err(|cleanup| {
            format!("save session evidence: {error}; remove partial evidence: {cleanup}")
        })?;
        return Err(format!("save session evidence: {error}"));
    }
    Ok(())
}

struct Session {
    prepared: Prepared,
    player: PlayerId,
    epoch: SessionEpoch,
    engine: DuoEngine,
    replay: Replay,
    declared: [usize; 2],
    counts: [usize; 2],
    last: [Option<Fact>; 2],
    ended: [bool; 2],
    output: PathBuf,
    summary: SessionSummary,
}

impl Session {
    fn new(
        prepared: Prepared,
        player: PlayerId,
        invitation: &Invitation,
        output: PathBuf,
    ) -> Result<Self, String> {
        let epoch = SessionEpoch(invitation.epoch);
        let engine = DuoEngine::new(epoch, prepared.anchors.clone(), DuoRules::default())
            .map_err(|_| "initialize session core failed")?;
        let replay = Replay::new(
            ReplayIdentity {
                content_id: prepared.identity.content_id.clone(),
                rules_id: RULESET.into(),
                build_id: format!("cocobeat-net/{}", env!("CARGO_PKG_VERSION")),
            },
            epoch,
        )
        .map_err(|_| "initialize session recorder failed")?;
        let summary = SessionSummary {
            status: "FAILED",
            protocol_version: PROTOCOL_VERSION,
            epoch: epoch.0,
            player: number(player),
            peer_authenticated: false,
            content_id: prepared.identity.content_id.clone(),
            endpoint: invitation.endpoint.clone(),
            cert_blake3: blake3::Hash::from_bytes(invitation.cert_blake3)
                .to_hex()
                .to_string(),
            template_blake3: prepared.template_blake3.clone(),
            template_epoch: prepared.template_epoch,
            facts: [0; 2],
            event_count: 0,
            live_replay_blake3: None,
            authority_replay_blake3: None,
            error: None,
        };
        let mut declared = [0; 2];
        declared[player.index()] = prepared.local.len();
        Ok(Self {
            prepared,
            player,
            epoch,
            engine,
            replay,
            declared,
            counts: [0; 2],
            last: [None; 2],
            ended: [false; 2],
            output,
            summary,
        })
    }

    fn bind_peer(&mut self, identity: &Identity, count: u64) -> Result<(), String> {
        if identity != &self.prepared.identity {
            return Err("peer validated package identity differs".into());
        }
        let count = usize::try_from(count).map_err(|_| "peer fact count exceeds capacity")?;
        if count == 0
            || self
                .prepared
                .local
                .len()
                .checked_add(count)
                .is_none_or(|sum| sum > MAX_FACTS)
        {
            return Err("combined declared history exceeds Replay capacity or is empty".into());
        }
        self.declared[other(self.player).index()] = count;
        self.summary.peer_authenticated = true;
        Ok(())
    }

    fn ingest(&mut self, player: PlayerId, fact: Fact) -> Result<(), String> {
        let index = player.index();
        if self.ended[index]
            || self.counts[index] >= self.declared[index]
            || self.replay.facts().len() >= MAX_FACTS
        {
            return Err("history exceeds its declared or recorder capacity".into());
        }
        validate_fact(fact, self.prepared.end, self.prepared.final_through)?;
        let input = fact.into_input(self.epoch, player);
        self.engine
            .ingest(input)
            .map_err(|error| format!("invalid player history: {error}"))?;
        self.replay
            .record(input)
            .map_err(|_| "record accepted input failed")?;
        self.counts[index] += 1;
        self.last[index] = Some(fact);
        Ok(())
    }

    fn end(&mut self, player: PlayerId) -> Result<(), String> {
        let index = player.index();
        if self.ended[index]
            || self.counts[index] != self.declared[index]
            || self.last[index]
                != Some(Fact::Watermark {
                    through: self.prepared.final_through,
                })
        {
            return Err("player history ended without its exact declared final watermark".into());
        }
        self.ended[index] = true;
        Ok(())
    }

    fn verify_replay(&self, replay: &Replay) -> Result<(), String> {
        if replay.epoch() != self.epoch || !same_player_histories(replay, &self.replay) {
            return Err(
                "authority Replay does not contain the complete actual player histories".into(),
            );
        }
        let engine = replay
            .replay(
                &self.prepared.identity.content_id,
                RULESET,
                self.prepared.anchors.clone(),
                DuoRules::default(),
            )
            .map_err(|_| "authority Replay identity, epoch or core validation failed")?;
        if engine.events() != self.engine.events() || engine.resonance() != self.engine.resonance()
        {
            return Err("authority Replay and live core results differ".into());
        }
        Ok(())
    }

    fn save_live(&mut self) -> Result<(), String> {
        if self.summary.live_replay_blake3.is_none() {
            let bytes = self
                .replay
                .encode()
                .map_err(|_| "encode live Replay failed")?;
            write_new(&self.output.join("live.replay.json"), &bytes)?;
            self.summary.live_replay_blake3 = Some(blake3::hash(&bytes).to_hex().to_string());
        }
        Ok(())
    }

    fn finish(mut self, result: Result<(), String>) -> Result<SessionSummary, String> {
        let mut error = result.err();
        if let Err(save_error) = self.save_live() {
            error = Some(match error {
                Some(error) => format!("{error}; {save_error}"),
                None => save_error,
            });
        }
        self.summary.facts = self.counts;
        self.summary.event_count = self.engine.events().len();
        self.summary.status = if error.is_none() {
            "COMPLETE"
        } else if self.summary.authority_replay_blake3.is_some() {
            "AUTHORITY_VERIFIED_UNCONFIRMED"
        } else {
            "FAILED"
        };
        self.summary.error = error;
        let bytes = serde_json::to_vec_pretty(&self.summary)
            .map_err(|_| "encode session summary failed")?;
        write_new(&self.output.join("status.json"), &bytes)?;
        match &self.summary.error {
            Some(error) => Err(format!(
                "{error}; session evidence: {}",
                self.output.display()
            )),
            None => Ok(self.summary),
        }
    }
}

fn same_player_histories(left: &Replay, right: &Replay) -> bool {
    [PlayerId::P1, PlayerId::P2].into_iter().all(|player| {
        left.facts()
            .iter()
            .filter(|input| seat(**input) == player)
            .eq(right.facts().iter().filter(|input| seat(**input) == player))
    })
}

struct ControlIo {
    send: SendStream,
    recv: RecvStream,
    sent: usize,
    received: usize,
}
impl ControlIo {
    fn new((send, recv): (SendStream, RecvStream)) -> Self {
        Self {
            send,
            recv,
            sent: 0,
            received: 0,
        }
    }
    async fn send(&mut self, message: Control) -> Result<(), String> {
        wire::send_control(&mut self.send, &mut self.sent, &message).await
    }
    async fn recv(&mut self) -> Result<Control, String> {
        wire::recv_control(&mut self.recv, &mut self.received).await
    }
}

struct InputIo {
    send: SendStream,
    recv: RecvStream,
    written: u64,
    read: u64,
}

async fn open_inputs(
    connection: &Connection,
    player: PlayerId,
    epoch: u64,
) -> Result<InputIo, String> {
    let (send, recv) = if player == PlayerId::P1 {
        connection
            .accept_bi()
            .await
            .map_err(|_| "accept input stream failed")?
    } else {
        connection
            .open_bi()
            .await
            .map_err(|_| "open input stream failed")?
    };
    let mut inputs = InputIo {
        send,
        recv,
        written: 0,
        read: 0,
    };
    // Guest writes first so the host can accept the stream
    if player == PlayerId::P2 {
        wire::send_input(
            &mut inputs.send,
            &mut inputs.written,
            &Input::Open { epoch, player: 2 },
        )
        .await?;
    }
    if wire::recv_input(&mut inputs.recv, &mut inputs.read).await?
        != (Input::Open {
            epoch,
            player: number(other(player)),
        })
    {
        return Err("input stream epoch or fixed player direction differs".into());
    }
    if player == PlayerId::P1 {
        wire::send_input(
            &mut inputs.send,
            &mut inputs.written,
            &Input::Open { epoch, player: 1 },
        )
        .await?;
    }
    Ok(inputs)
}

async fn ready(
    connection: &Connection,
    control: &mut ControlIo,
    session: &Session,
) -> Result<(InputIo, Instant), String> {
    tokio::time::timeout(IDLE, async {
        let epoch = session.epoch.0;
        let inputs = open_inputs(connection, session.player, epoch).await?;
        if session.player == PlayerId::P2 {
            control.send(Control::Ready { epoch }).await?;
        }
        if control.recv().await? != (Control::Ready { epoch }) {
            return Err("expected the peer Ready for this epoch".into());
        }
        let start;
        if session.player == PlayerId::P1 {
            control.send(Control::Ready { epoch }).await?;
            start = Instant::now();
            control.send(Control::Start { epoch }).await?;
            if control.recv().await? != (Control::StartAck { epoch }) {
                return Err("expected StartAck for this epoch".into());
            }
        } else {
            if control.recv().await? != (Control::Start { epoch }) {
                return Err("expected Start for this epoch".into());
            }
            start = Instant::now();
            control.send(Control::StartAck { epoch }).await?;
        }
        Ok((inputs, start))
    })
    .await
    .map_err(|_| "Ready/Start barrier timed out")?
}

enum History {
    Batch(PlayerId, Vec<Fact>),
    End(PlayerId),
}

async fn exchange(session: &mut Session, inputs: InputIo) -> Result<(), String> {
    let InputIo {
        mut send,
        mut recv,
        mut written,
        mut read,
    } = inputs;
    let player = session.player;
    let peer = other(player);
    let epoch = session.epoch.0;
    let final_through = session.prepared.final_through;
    let expected = session.declared[peer.index()];
    let local = std::mem::take(&mut session.prepared.local);
    let (tx, mut rx) = mpsc::channel(4);
    let sender_tx = tx.clone();
    let mut tasks = JoinSet::new();
    tasks.spawn(async move {
        for facts in local.chunks(64) {
            sender_tx
                .send(History::Batch(player, facts.to_vec()))
                .await
                .map_err(|_| "local history consumer stopped")?;
            wire::send_input(
                &mut send,
                &mut written,
                &Input::Facts {
                    epoch,
                    facts: facts.to_vec(),
                },
            )
            .await?;
        }
        wire::send_input(
            &mut send,
            &mut written,
            &Input::End {
                epoch,
                fact_count: local.len() as u64,
                final_through,
            },
        )
        .await?;
        send.finish()
            .map_err(|_| "finish local input stream failed")?;
        sender_tx
            .send(History::End(player))
            .await
            .map_err(|_| "local history consumer stopped")?;
        Ok::<(), String>(())
    });
    tasks.spawn(async move {
        let mut count: usize = 0;
        loop {
            match wire::recv_input(&mut recv, &mut read).await? {
                Input::Facts {
                    epoch: actual,
                    facts,
                } if actual == epoch => {
                    count = count
                        .checked_add(facts.len())
                        .filter(|count| *count <= expected)
                        .ok_or("peer exceeded declared fact count")?;
                    tx.send(History::Batch(peer, facts))
                        .await
                        .map_err(|_| "remote history consumer stopped")?;
                }
                Input::End {
                    epoch: actual,
                    fact_count,
                    final_through: through,
                } if actual == epoch
                    && fact_count == expected as u64
                    && count == expected
                    && through == final_through =>
                {
                    wire::ensure_eof(&mut recv).await?;
                    tx.send(History::End(peer))
                        .await
                        .map_err(|_| "remote history consumer stopped")?;
                    return Ok::<(), String>(());
                }
                _ => return Err("unexpected input state, epoch, count or final watermark".into()),
            }
        }
    });
    let mut progressed = Instant::now();
    while !session.ended.iter().all(|ended| *ended) {
        tokio::select! {
            joined = tasks.join_next(), if !tasks.is_empty() => {
                joined.ok_or("history task disappeared")?.map_err(|_| "history worker failed")??;
            }
            message = tokio::time::timeout_at(progressed + IDLE, rx.recv()) => {
                match message.map_err(|_| "input history progress timed out")?.ok_or("input history stopped before both explicit ends")? {
                    History::Batch(player, facts) => {
                        for fact in facts { session.ingest(player, fact)?; }
                        progressed = Instant::now();
                    }
                    History::End(player) => session.end(player)?,
                }
            }
        }
    }
    while let Some(joined) = tasks.join_next().await {
        joined.map_err(|_| "history worker failed")??;
    }
    Ok(())
}

async fn host_finish(
    session: &mut Session,
    connection: &Connection,
    control: &mut ControlIo,
) -> Result<(), String> {
    let epoch = session.epoch.0;
    let hash = tokio::time::timeout(Duration::from_secs(60), async {
        session.verify_replay(&session.replay)?;
        session.save_live()?;
        let bytes = session
            .replay
            .encode()
            .map_err(|_| "encode authority Replay failed")?;
        let hash = *blake3::hash(&bytes).as_bytes();
        write_new(&session.output.join("authority.replay.json"), &bytes)?;
        session.summary.authority_replay_blake3 =
            Some(blake3::Hash::from_bytes(hash).to_hex().to_string());
        control
            .send(Control::Finish {
                epoch,
                bytes: bytes.len() as u64,
                blake3: hash,
            })
            .await?;
        control
            .send
            .finish()
            .map_err(|_| "finish host control stream failed")?;
        let mut stream = connection
            .open_uni()
            .await
            .map_err(|_| "open authority Replay stream failed")?;
        stream
            .write_all(&bytes)
            .await
            .map_err(|_| "send authority Replay failed")?;
        stream
            .finish()
            .map_err(|_| "finish authority Replay stream failed")?;
        Ok::<_, String>(hash)
    })
    .await
    .map_err(|_| "authority Replay transfer timed out")??;
    tokio::time::timeout(IDLE, async {
        if control.recv().await?
            != (Control::FinishAck {
                epoch,
                blake3: hash,
            })
        {
            return Err("FinishAck does not match this epoch and authority Replay".into());
        }
        wire::ensure_eof(&mut control.recv).await?;
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "FinishAck timed out")??;
    connection.close(COMPLETE_CODE.into(), COMPLETE_REASON);
    Ok(())
}

async fn guest_finish(
    session: &mut Session,
    connection: &Connection,
    control: &mut ControlIo,
) -> Result<(), String> {
    let epoch = session.epoch.0;
    let hash = tokio::time::timeout(Duration::from_secs(60), async {
        let (length, hash) = match control.recv().await? {
            Control::Finish {
                epoch: actual,
                bytes,
                blake3,
            } if actual == epoch && (1..=MAX_FILE_BYTES).contains(&bytes) => {
                (bytes as usize, blake3)
            }
            _ => return Err("invalid Finish epoch or authority Replay size".into()),
        };
        wire::ensure_eof(&mut control.recv).await?;
        let mut stream = connection
            .accept_uni()
            .await
            .map_err(|_| "accept authority Replay stream failed")?;
        let receive = async {
            let mut bytes = vec![0; length];
            stream
                .read_exact(&mut bytes)
                .await
                .map_err(|_| "authority Replay is truncated")?;
            wire::ensure_eof(&mut stream).await?;
            if blake3::hash(&bytes).as_bytes() != &hash {
                return Err("authority Replay hash mismatch".into());
            }
            let replay = Replay::decode(bytes.as_slice())
                .map_err(|_| "authority Replay format is invalid")?;
            session.verify_replay(&replay)?;
            write_new(&session.output.join("authority.replay.json"), &bytes)?;
            session.summary.authority_replay_blake3 =
                Some(blake3::Hash::from_bytes(hash).to_hex().to_string());
            session.save_live()?;
            Ok::<_, String>(hash)
        };
        tokio::select! {
            biased;
            error = extra_uni(connection) => Err(error),
            result = receive => result,
        }
    })
    .await
    .map_err(|_| "authority Replay verification timed out")??;
    tokio::time::timeout(IDLE, async {
        control.send(Control::FinishAck { epoch, blake3: hash }).await?;
        control.send.finish().map_err(|_| "finish guest control stream failed")?;
        tokio::select! {
            biased;
            error = extra_uni(connection) => Err(error),
            closed = connection.closed() => {
                match closed {
                    quinn::ConnectionError::ApplicationClosed(close) if close.error_code == COMPLETE_CODE.into() && close.reason.as_ref() == COMPLETE_REASON => Ok(()),
                    _ => Err("host closed without confirming the completed session".into()),
                }
            }
        }
    }).await.map_err(|_| "FinishAck/host completion confirmation timed out")?
}

// Connection closure is handled by the active protocol operation, not as an extra stream
async fn extra_uni(connection: &Connection) -> String {
    if connection.accept_uni().await.is_ok() {
        return "unexpected additional authority stream".into();
    }
    std::future::pending().await
}

async fn extra_bidi(connection: &Connection) -> String {
    if connection.accept_bi().await.is_ok() {
        return "unexpected additional player stream".into();
    }
    std::future::pending().await
}

async fn close_endpoint(endpoint: &Endpoint, failed: bool) {
    if failed {
        endpoint.close(1_u8.into(), b"session failed");
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), endpoint.wait_idle()).await;
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("create network runtime: {error}"))
}

/// Runs one invited headless P1 session; every output is new and outside PACKAGE
pub fn host(
    package: &Path,
    template: &Path,
    bind: SocketAddr,
    invite: &Path,
    output: &Path,
) -> Result<SessionSummary, String> {
    let prepared = prepare(package, template, PlayerId::P1)?;
    let invite = destination_outside_package(package, invite)?;
    let output = destination_outside_package(package, output)?;
    runtime()?.block_on(async {
        let (endpoint, invitation) = listen(bind)?;
        fs::create_dir(&output).map_err(|error| format!("create new session output directory: {error}"))?;
        let mut session = Session::new(prepared, PlayerId::P1, &invitation, output)?;
        let result = async {
            write_invite(&invite, &invitation)?;
            println!("{}", serde_json::json!({"status":"INVITING", "invitation":invite, "endpoint":invitation.endpoint, "cert_blake3":session.summary.cert_blake3, "epoch":invitation.epoch}));
            let incoming = tokio::time::timeout(Duration::from_secs(120), endpoint.accept()).await
                .map_err(|_| "invited guest did not connect within 120 seconds")?.ok_or("host endpoint stopped before a guest connected")?;
            let refusal_endpoint = endpoint.clone();
            let mut refusal = JoinSet::new();
            refusal.spawn(async move { while let Some(incoming) = refusal_endpoint.accept().await { incoming.refuse(); } });
            let (connection, mut control) = tokio::time::timeout(Duration::from_secs(10), async {
                let connection = incoming.await.map_err(|_| "guest TLS handshake failed")?;
                let mut control = ControlIo::new(connection.accept_bi().await.map_err(|_| "accept control stream failed")?);
                match control.recv().await? {
                    Control::Hello { protocol_version: PROTOCOL_VERSION, epoch, player: 2, token, identity, fact_count }
                        if epoch == invitation.epoch && token == invitation.token => session.bind_peer(&identity, fact_count)?,
                    _ => return Err("guest capability, role, epoch or protocol rejected".into()),
                }
                control.send(Control::Welcome { protocol_version: PROTOCOL_VERSION, epoch: invitation.epoch, player: 1, identity: session.prepared.identity.clone(), fact_count: session.prepared.local.len() as u64 }).await?;
                Ok::<_, String>((connection, control))
            }).await.map_err(|_| "guest TLS/capability phase timed out")??;
            let (inputs, start) = ready(&connection, &mut control, &session).await?;
            let run = async {
                exchange(&mut session, inputs).await?;
                host_finish(&mut session, &connection, &mut control).await
            };
            tokio::time::timeout_at(start + Duration::from_secs(15 * 60), async {
                tokio::select! {
                    biased;
                    error = extra_bidi(&connection) => Err(error),
                    result = run => result,
                }
            }).await.map_err(|_| "accelerated session exceeded 15 minutes")?
        }.await;
        close_endpoint(&endpoint, result.is_err()).await;
        session.finish(result)
    })
}

/// Joins one pinned host as P2 after validating the complete local package
pub fn join(
    package: &Path,
    template: &Path,
    invite: &Path,
    output: &Path,
) -> Result<SessionSummary, String> {
    let prepared = prepare(package, template, PlayerId::P2)?;
    let invitation = read_invite(invite)?;
    let output = destination_outside_package(package, output)?;
    let runtime = runtime()?;
    fs::create_dir(&output)
        .map_err(|error| format!("create new session output directory: {error}"))?;
    let mut session = Session::new(prepared, PlayerId::P2, &invitation, output)?;
    let result = runtime.block_on(async {
        let handshake_deadline = Instant::now() + Duration::from_secs(10);
        let (endpoint, connection) =
            tokio::time::timeout_at(handshake_deadline, connect(&invitation))
                .await
                .map_err(|_| "host TLS/capability phase timed out")??;
        let result = async {
            let mut control = tokio::time::timeout_at(handshake_deadline, async {
                let mut control = ControlIo::new(
                    connection
                        .open_bi()
                        .await
                        .map_err(|_| "open control stream failed")?,
                );
                control
                    .send(Control::Hello {
                        protocol_version: PROTOCOL_VERSION,
                        epoch: invitation.epoch,
                        player: 2,
                        token: invitation.token,
                        identity: session.prepared.identity.clone(),
                        fact_count: session.prepared.local.len() as u64,
                    })
                    .await?;
                match control.recv().await? {
                    Control::Welcome {
                        protocol_version: PROTOCOL_VERSION,
                        epoch,
                        player: 1,
                        identity,
                        fact_count,
                    } if epoch == invitation.epoch => session.bind_peer(&identity, fact_count)?,
                    _ => return Err("host role, epoch or protocol rejected".into()),
                }
                Ok::<_, String>(control)
            })
            .await
            .map_err(|_| "host capability phase timed out")??;
            let (inputs, start) = ready(&connection, &mut control, &session).await?;
            tokio::time::timeout_at(start + Duration::from_secs(15 * 60), async {
                exchange(&mut session, inputs).await?;
                guest_finish(&mut session, &connection, &mut control).await
            })
            .await
            .map_err(|_| "accelerated session exceeded 15 minutes")?
        }
        .await;
        close_endpoint(&endpoint, result.is_err()).await;
        result
    });
    session.finish(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_schema::SongTime;

    fn fixture() -> Session {
        let identity = Identity {
            content_id: "fixture".into(),
            canonical_frames: 48_000,
            content_schema: 1,
            ruleset_id: RULESET.into(),
        };
        let prepared = Prepared {
            identity,
            anchors: vec![Anchor {
                id: 1,
                song_time: SongTime::from_frames(10_000),
            }],
            local: vec![Fact::Watermark { through: 68_881 }],
            template_blake3: "fixture".into(),
            template_epoch: 1,
            end: 48_000,
            final_through: 68_881,
        };
        let invitation = Invitation {
            invite_version: 1,
            protocol_version: 1,
            endpoint: "127.0.0.1:1".into(),
            server_name: "cocobeat.local".into(),
            certificate_der: vec![],
            cert_blake3: [0; 32],
            token: [0; 32],
            epoch: 7,
        };
        Session::new(prepared, PlayerId::P1, &invitation, PathBuf::new()).unwrap()
    }

    #[test]
    fn peer_binding_and_intake_fail_before_core_or_recorder_change() {
        let mut session = fixture();
        let identity = session.prepared.identity.clone();
        let mut wrong = identity.clone();
        wrong.canonical_frames += 1;
        assert!(session.bind_peer(&wrong, 2).is_err());
        assert!(!session.summary.peer_authenticated);
        assert!(session.bind_peer(&identity, MAX_FACTS as u64).is_err());
        assert!(session.bind_peer(&identity, 0).is_err());
        session.bind_peer(&identity, 2).unwrap();
        for fact in [
            Fact::Hit { seq: 1, frame: -1 },
            Fact::Hit {
                seq: 1,
                frame: 48_000,
            },
            Fact::Watermark { through: 68_882 },
        ] {
            assert!(session.ingest(PlayerId::P2, fact).is_err());
        }
        assert!(session.replay.facts().is_empty());
        assert!(session.engine.events().is_empty());
        session
            .ingest(
                PlayerId::P2,
                Fact::Hit {
                    seq: 99,
                    frame: 10_000,
                },
            )
            .unwrap();
        assert!(session.end(PlayerId::P2).is_err());
        assert!(
            session
                .ingest(
                    PlayerId::P2,
                    Fact::Hit {
                        seq: 99,
                        frame: 10_000
                    }
                )
                .is_err()
        );
        assert_eq!(session.replay.facts().len(), 1);
        session
            .ingest(PlayerId::P2, Fact::Watermark { through: 68_881 })
            .unwrap();
        session.end(PlayerId::P2).unwrap();
        assert!(
            session
                .ingest(PlayerId::P2, Fact::Watermark { through: 68_881 })
                .is_err()
        );
        assert_eq!(session.replay.facts().len(), 2);
        assert!(
            session.engine.events().is_empty(),
            "P1 has not closed its history"
        );
        session
            .ingest(PlayerId::P1, Fact::Watermark { through: 68_881 })
            .unwrap();
        session.end(PlayerId::P1).unwrap();
        assert_eq!(session.engine.events().len(), 2);
        session.verify_replay(&session.replay).unwrap();
    }

    #[test]
    fn authority_comparison_requires_equal_complete_player_subsequences() {
        let mut session = fixture();
        session.declared = [3, 2];
        let inputs = [
            (PlayerId::P1, Fact::Hit { seq: 5, frame: 10 }),
            (PlayerId::P2, Fact::Hit { seq: 1, frame: 12 }),
            (PlayerId::P1, Fact::Hit { seq: 2, frame: 20 }),
            (PlayerId::P1, Fact::Watermark { through: 68_881 }),
            (PlayerId::P2, Fact::Watermark { through: 68_881 }),
        ];
        for (player, fact) in inputs {
            session.ingest(player, fact).unwrap();
        }
        let mut reordered = Replay::new(session.replay.identity().clone(), session.epoch).unwrap();
        for index in [1, 4, 0, 2, 3] {
            reordered.record(session.replay.facts()[index]).unwrap();
        }
        assert!(same_player_histories(&reordered, &session.replay));
        session.verify_replay(&reordered).unwrap();
        let mut short = Replay::new(session.replay.identity().clone(), session.epoch).unwrap();
        for input in &session.replay.facts()[..4] {
            short.record(*input).unwrap();
        }
        assert!(!same_player_histories(&short, &session.replay));
        reordered.record(session.replay.facts()[4]).unwrap();
        assert!(!same_player_histories(&reordered, &session.replay));
        let mut wrong_order =
            Replay::new(session.replay.identity().clone(), session.epoch).unwrap();
        for index in [2, 1, 0, 3, 4] {
            wrong_order.record(session.replay.facts()[index]).unwrap();
        }
        assert!(!same_player_histories(&wrong_order, &session.replay));
    }

    #[test]
    fn output_aliases_cannot_add_a_fifth_package_file_or_replace_existing_evidence() {
        let root = std::env::temp_dir().join(format!(
            "cocobeat-net-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let package = root.join("package");
        fs::create_dir(&package).unwrap();
        assert!(destination_outside_package(&package, &package.join("invite.json")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&package, root.join("alias")).unwrap();
            assert!(
                destination_outside_package(&package, &root.join("alias/invite.json")).is_err()
            );
        }
        let output = destination_outside_package(&package, &root.join("evidence.json")).unwrap();
        write_new(&output, b"old").unwrap();
        assert!(write_new(&output, b"new").is_err());
        assert_eq!(fs::read(output).unwrap(), b"old");
        fs::remove_dir_all(root).unwrap();
    }
}
