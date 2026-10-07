use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use cocobeat_core::DuoEngine;
use cocobeat_replay::{MAX_FACTS, MAX_FILE_BYTES, Replay, ReplayIdentity};
use cocobeat_schema::{
    Anchor, CONTENT_SCHEMA_VERSION, DuoInput, DuoRules, MAX_CANONICAL_FRAMES, PlayerId,
    SessionEpoch,
};
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use serde::Serialize;
use tokio::{sync::mpsc, task::JoinSet, time::Instant};

use crate::{
    Invitation, PROTOCOL_VERSION, connect, listen, read_invite, resource, sync, wire, write_invite,
};
use wire::{Control, Fact, Identity, Input};

pub(crate) const RULESET: &str = "duo-watermark-v1";
const COMPLETE_CODE: u32 = 0x4342;
const COMPLETE_REASON: &[u8] = b"session complete";
pub(crate) const IDLE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Serialize)]
pub struct SessionSummary {
    pub status: &'static str,
    pub mode: &'static str,
    pub protocol_version: u32,
    pub epoch: u64,
    pub player: u8,
    pub peer_authenticated: bool,
    pub package_received: bool,
    pub resource_bytes: u64,
    pub network_timing: Option<sync::NetworkTiming>,
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

#[derive(Clone)]
pub(crate) struct Prepared {
    pub(crate) identity: Identity,
    pub(crate) anchors: Vec<Anchor>,
    pub(crate) local: Vec<Fact>,
    pub(crate) template_blake3: String,
    pub(crate) template_epoch: u64,
    pub(crate) end: i64,
    pub(crate) final_through: i64,
}

pub(crate) fn seat(input: DuoInput) -> PlayerId {
    match input {
        DuoInput::Hit(hit) => hit.player,
        DuoInput::Watermark { player, .. } => player,
    }
}

fn number(player: PlayerId) -> u8 {
    player.index() as u8 + 1
}

pub(crate) fn other(player: PlayerId) -> PlayerId {
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
    prepare_validated(package, template, player)
}

pub(crate) fn prepare_package(
    package: cocobeat_media::ValidatedPackage,
) -> Result<Prepared, String> {
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
        stage_compiler_version: None,
    };
    let end = i64::try_from(identity.canonical_frames).map_err(|_| "song frame overflow")?;
    let final_through = DuoRules::default()
        .confirmation_delay_frames()
        .and_then(|delay| end.checked_add(delay))
        .and_then(|through| through.checked_add(1))
        .ok_or("song final watermark overflow")?;
    Ok(Prepared {
        identity,
        anchors: package.chart.anchors,
        local: Vec::new(),
        template_blake3: String::new(),
        template_epoch: 0,
        end,
        final_through,
    })
}

fn prepare_validated(
    package: cocobeat_media::ValidatedPackage,
    template: &Path,
    player: PlayerId,
) -> Result<Prepared, String> {
    let mut prepared = prepare_package(package)?;
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
            &prepared.identity.content_id,
            RULESET,
            prepared.anchors.clone(),
            DuoRules::default(),
        )
        .map_err(|_| "template content, rules, epoch or history is invalid")?;
    for input in replay.facts() {
        validate_fact(
            Fact::from_input(*input),
            prepared.end,
            prepared.final_through,
        )?;
    }
    let local: Vec<_> = replay
        .facts()
        .iter()
        .filter(|input| seat(**input) == player)
        .map(|input| Fact::from_input(*input))
        .collect();
    if local.last()
        != Some(&Fact::Watermark {
            through: prepared.final_through,
        })
    {
        return Err("local template must end with the explicit song final watermark".into());
    }
    prepared.local = local;
    prepared.template_blake3 = blake3::hash(&bytes).to_hex().to_string();
    prepared.template_epoch = replay.epoch().0;
    Ok(prepared)
}

pub(crate) fn destination_outside_package(
    package: &Path,
    destination: &Path,
) -> Result<PathBuf, String> {
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

pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
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

#[derive(Clone)]
pub(crate) struct Session {
    pub(crate) prepared: Prepared,
    pub(crate) player: PlayerId,
    pub(crate) epoch: SessionEpoch,
    pub(crate) engine: DuoEngine,
    pub(crate) replay: Replay,
    pub(crate) declared: [Option<usize>; 2],
    pub(crate) counts: [usize; 2],
    pub(crate) next_seq: [u64; 2],
    pub(crate) last: [Option<Fact>; 2],
    pub(crate) ended: [bool; 2],
    output: PathBuf,
    pub(crate) summary: SessionSummary,
    pub(crate) origin: Instant,
}

impl Session {
    pub(crate) fn new(
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
                stage_compiler_version: prepared.identity.stage_compiler_version,
            },
            epoch,
        )
        .map_err(|_| "initialize session recorder failed")?;
        let summary = SessionSummary {
            status: "FAILED",
            mode: "headless",
            protocol_version: PROTOCOL_VERSION,
            epoch: epoch.0,
            player: number(player),
            peer_authenticated: false,
            package_received: false,
            resource_bytes: 0,
            network_timing: None,
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
        let mut declared = [None; 2];
        declared[player.index()] = Some(prepared.local.len());
        Ok(Self {
            prepared,
            player,
            epoch,
            engine,
            replay,
            declared,
            counts: [0; 2],
            next_seq: [0; 2],
            last: [None; 2],
            ended: [false; 2],
            output,
            summary,
            origin: Instant::now(),
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
        self.declared[other(self.player).index()] = Some(count);
        self.summary.peer_authenticated = true;
        Ok(())
    }

    pub(crate) fn ingest(&mut self, player: PlayerId, fact: Fact) -> Result<(), String> {
        let index = player.index();
        if self.ended[index]
            || self.declared[index].is_some_and(|declared| self.counts[index] >= declared)
            || self.replay.facts().len() >= MAX_FACTS
        {
            return Err("history exceeds its declared or recorder capacity".into());
        }
        let next_seq = if self.summary.mode == "live"
            && let Fact::Hit { seq, .. } = fact
        {
            if seq != self.next_seq[index] {
                return Err("live Hit sequence must be continuous from zero".into());
            }
            Some(seq.checked_add(1).ok_or("live Hit sequence overflow")?)
        } else {
            None
        };
        validate_fact(fact, self.prepared.end, self.prepared.final_through)?;
        let input = fact.into_input(self.epoch, player);
        self.engine
            .ingest(input)
            .map_err(|error| format!("invalid player history: {error}"))?;
        self.replay
            .record(input)
            .map_err(|_| "record accepted input failed")?;
        self.counts[index] += 1;
        if let Some(next_seq) = next_seq {
            self.next_seq[index] = next_seq;
        }
        self.last[index] = Some(fact);
        Ok(())
    }

    pub(crate) fn player_tape(&self, player: PlayerId) -> Vec<Fact> {
        self.replay
            .facts()
            .iter()
            .filter(|input| seat(**input) == player)
            .map(|input| Fact::from_input(*input))
            .collect()
    }

    pub(crate) fn append_owner_tape(
        &mut self,
        player: PlayerId,
        tape: &[Fact],
    ) -> Result<Vec<DuoInput>, String> {
        if tape.len() > MAX_FACTS {
            return Err("recovery owner tape exceeds Replay capacity".into());
        }
        let accepted = self.player_tape(player);
        if !tape.starts_with(&accepted) {
            return Err("accepted player history is not an exact prefix of its owner tape".into());
        }
        let suffix = &tape[accepted.len()..];
        if suffix.is_empty() {
            return Ok(Vec::new());
        }
        let mut staged = self.clone();
        for fact in suffix {
            staged.ingest(player, *fact)?;
        }
        drop(staged);
        for fact in suffix {
            self.ingest(player, *fact)?;
        }
        Ok(suffix
            .iter()
            .map(|fact| fact.into_input(self.epoch, player))
            .collect())
    }

    pub(crate) fn check_presented_peer(
        &self,
        peer: PlayerId,
        presented: &[DuoInput],
    ) -> Result<Vec<DuoInput>, String> {
        if peer != other(self.player) || presented.len() > MAX_FACTS {
            return Err("recovery presentation must be the bounded peer tape".into());
        }
        let accepted: Vec<_> = self
            .replay
            .facts()
            .iter()
            .filter(|input| seat(**input) == peer)
            .copied()
            .collect();
        if !accepted.starts_with(presented) {
            return Err("presented peer history is not an exact prefix of worker history".into());
        }
        Ok(accepted[presented.len()..].to_vec())
    }

    pub(crate) fn begin_recovery(
        &self,
        gui_replay: &Replay,
        metadata: serde_json::Value,
    ) -> Result<(), String> {
        if self.summary.mode != "live"
            || self.ended.iter().any(|ended| *ended)
            || gui_replay.epoch() != self.epoch
            || gui_replay.identity().stage_compiler_version
                != self.prepared.identity.stage_compiler_version
        {
            return Err("recovery snapshot session, epoch or stage identity differs".into());
        }
        gui_replay
            .replay(
                &self.prepared.identity.content_id,
                RULESET,
                self.prepared.anchors.clone(),
                DuoRules::default(),
            )
            .map_err(|_| "recovery GUI Replay identity or history is invalid")?;
        for input in gui_replay.facts() {
            validate_fact(
                Fact::from_input(*input),
                self.prepared.end,
                self.prepared.final_through,
            )?;
        }
        let worker_bytes = self
            .replay
            .encode()
            .map_err(|_| "encode worker recovery Replay failed")?;
        let gui_bytes = gui_replay
            .encode()
            .map_err(|_| "encode GUI recovery Replay failed")?;
        let metadata_bytes =
            serde_json::to_vec_pretty(&metadata).map_err(|_| "encode recovery metadata failed")?;
        let output = self.output.join("recovery-1");
        fs::create_dir(&output).map_err(|error| format!("create recovery evidence: {error}"))?;
        write_new(&output.join("worker-prefix.replay.json"), &worker_bytes)?;
        write_new(&output.join("gui-prefix.replay.json"), &gui_bytes)?;
        write_new(&output.join("metadata.json"), &metadata_bytes)
    }

    pub(crate) fn end_live(
        &mut self,
        player: PlayerId,
        count: u64,
        through: i64,
    ) -> Result<(), String> {
        if self.declared[player.index()].is_some()
            || count != self.counts[player.index()] as u64
            || through != self.prepared.final_through
            || count == 0
        {
            return Err("live End differs from the actual accepted history".into());
        }
        self.end(player)
    }

    fn end(&mut self, player: PlayerId) -> Result<(), String> {
        let index = player.index();
        if self.ended[index]
            || self.declared[index].is_some_and(|declared| self.counts[index] != declared)
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

    pub(crate) fn verify_replay(&self, replay: &Replay) -> Result<(), String> {
        if replay.identity().stage_compiler_version != self.prepared.identity.stage_compiler_version
        {
            return Err("authority Replay Stage compiler identity differs".into());
        }
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

    pub(crate) fn finish(mut self, result: Result<(), String>) -> Result<SessionSummary, String> {
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

pub(crate) struct ControlIo {
    send: SendStream,
    recv: RecvStream,
    sent: usize,
    received: usize,
}
impl ControlIo {
    pub(crate) fn new((send, recv): (SendStream, RecvStream)) -> Self {
        Self {
            send,
            recv,
            sent: 0,
            received: 0,
        }
    }
    pub(crate) async fn send(&mut self, message: Control) -> Result<(), String> {
        wire::send_control(&mut self.send, &mut self.sent, &message).await
    }
    pub(crate) async fn recv(&mut self) -> Result<Control, String> {
        wire::recv_control(&mut self.recv, &mut self.received).await
    }
}

pub(crate) struct InputIo {
    pub(crate) send: SendStream,
    pub(crate) recv: RecvStream,
    pub(crate) written: u64,
    pub(crate) read: u64,
}

pub(crate) async fn open_inputs(
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
    session: &mut Session,
) -> Result<(InputIo, Instant), String> {
    tokio::time::timeout(IDLE, async {
        let inputs = open_inputs(connection, session.player, session.epoch.0).await?;
        let timing = session
            .summary
            .network_timing
            .insert(sync::NetworkTiming::default());
        let start = sync::ready_start(
            connection,
            control,
            session.epoch,
            session.player,
            session.origin,
            timing,
        )
        .await?;
        Ok((inputs, start))
    })
    .await
    .map_err(|_| "clock/Ready/ScheduleStart barrier timed out")?
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
    let expected = session.declared[peer.index()].ok_or("headless peer count was not declared")?;
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

pub(crate) async fn host_finish(
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

pub(crate) async fn guest_finish(
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
pub(crate) async fn extra_uni(connection: &Connection) -> String {
    if connection.accept_uni().await.is_ok() {
        return "unexpected additional unidirectional stream".into();
    }
    std::future::pending().await
}

pub(crate) async fn extra_bidi(connection: &Connection) -> String {
    if connection.accept_bi().await.is_ok() {
        return "unexpected additional player stream".into();
    }
    std::future::pending().await
}

pub(crate) async fn close_endpoint(endpoint: &Endpoint, failed: bool) {
    if failed {
        endpoint.close(1_u8.into(), b"session failed");
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), endpoint.wait_idle()).await;
}

pub(crate) fn runtime() -> Result<tokio::runtime::Runtime, String> {
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
            let (connection, mut control, fetch) = tokio::time::timeout(Duration::from_secs(10), async {
                let connection = incoming.await.map_err(|_| "guest TLS handshake failed")?;
                let mut control = ControlIo::new(connection.accept_bi().await.map_err(|_| "accept control stream failed")?);
                let fetch = match control.recv().await? {
                    Control::Hello { protocol_version: PROTOCOL_VERSION, epoch, player: 2, token, identity, fact_count }
                        if epoch == invitation.epoch && token == invitation.token => { session.bind_peer(&identity, fact_count)?; false },
                    Control::Fetch { protocol_version: PROTOCOL_VERSION, epoch, player: 2, token }
                        if epoch == invitation.epoch && token == invitation.token => { session.summary.peer_authenticated = true; true },
                    _ => return Err("guest capability, role, epoch or protocol rejected".into()),
                };
                control.send(Control::Welcome { protocol_version: PROTOCOL_VERSION, epoch: invitation.epoch, player: 1, identity: session.prepared.identity.clone(), fact_count: session.prepared.local.len() as u64 }).await?;
                Ok::<_, String>((connection, control, fetch))
            }).await.map_err(|_| "guest TLS/capability phase timed out")??;
            if fetch {
                tokio::time::timeout(resource::TRANSFER_TIMEOUT, async {
                    let transfer = async {
                        let source = resource::Source::open(package, &session.prepared.identity)?;
                        control.send(Control::Resources { epoch: invitation.epoch, objects: source.objects }).await?;
                        source.send(&connection, invitation.epoch).await?;
                        match control.recv().await? {
                            Control::Installed { epoch, identity, fact_count } if epoch == invitation.epoch => session.bind_peer(&identity, fact_count),
                            _ => Err("expected validated Installed package for this epoch".into()),
                        }
                    };
                    tokio::select! {
                        biased;
                        error = extra_bidi(&connection) => Err(error),
                        result = transfer => result,
                    }
                }).await.map_err(|_| "package transfer and validation exceeded 5 minutes")??;
                control.send(Control::InstalledAck { epoch: invitation.epoch }).await?;
            }
            let (inputs, start) = ready(&connection, &mut control, &mut session).await?;
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
            guest_run(&mut session, &connection, &mut control).await
        }
        .await;
        close_endpoint(&endpoint, result.is_err()).await;
        result
    });
    session.finish(result)
}

async fn guest_run(
    session: &mut Session,
    connection: &Connection,
    control: &mut ControlIo,
) -> Result<(), String> {
    let (inputs, start) = ready(connection, control, session).await?;
    tokio::time::timeout_at(start + Duration::from_secs(15 * 60), async {
        exchange(session, inputs).await?;
        guest_finish(session, connection, control).await
    })
    .await
    .map_err(|_| "accelerated session exceeded 15 minutes")?
}

pub(crate) fn new_destination(destination: &Path) -> Result<PathBuf, String> {
    let name = destination
        .file_name()
        .ok_or("destination requires a file name")?;
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let resolved = fs::canonicalize(parent)
        .map_err(|_| "destination parent must already exist")?
        .join(name);
    match fs::symlink_metadata(&resolved) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(resolved),
        _ => Err("destination must be a new path".into()),
    }
}

/// Authenticates an invited host before receiving and validating a new four-object package
/// The local Replay is used only after the received package has passed complete validation
pub fn join_receive(
    package_destination: &Path,
    template: &Path,
    invite: &Path,
    output: &Path,
) -> Result<SessionSummary, String> {
    let package_destination = new_destination(package_destination)?;
    let output = new_destination(output)?;
    if output.starts_with(&package_destination) || package_destination.starts_with(&output) {
        return Err("received package and session output must be separate paths".into());
    }
    let invitation = read_invite(invite)?;
    let runtime = runtime()?;
    fs::create_dir(&output)
        .map_err(|error| format!("create new session output directory: {error}"))?;
    let mut session = None;
    let mut authenticated = false;
    let mut received = false;
    let mut content_id = None;
    let mut resource_bytes = 0_u64;
    let result = runtime.block_on(async {
        let handshake_deadline = Instant::now() + Duration::from_secs(10);
        let connected = tokio::time::timeout_at(handshake_deadline, connect(&invitation)).await
            .map_err(|_| "host TLS/capability phase timed out")?;
        let (endpoint, connection) = connected?;
        let result = async {
            let (mut control, identity, fact_count) = tokio::time::timeout_at(handshake_deadline, async {
                let mut control = ControlIo::new(connection.open_bi().await.map_err(|_| "open control stream failed")?);
                control.send(Control::Fetch { protocol_version: PROTOCOL_VERSION, epoch: invitation.epoch, player: 2, token: invitation.token }).await?;
                match control.recv().await? {
                    Control::Welcome { protocol_version: PROTOCOL_VERSION, epoch, player: 1, identity, fact_count } if epoch == invitation.epoch => Ok::<_, String>((control, identity, fact_count)),
                    _ => Err("host role, epoch or protocol rejected".into()),
                }
            }).await.map_err(|_| "host capability phase timed out")??;
            authenticated = true;
            content_id = Some(identity.content_id.clone());
            resource::package_hash(&identity)?;
            if identity.ruleset_id != RULESET
                || identity.content_schema != CONTENT_SCHEMA_VERSION
                || identity.stage_compiler_version.is_some()
                || !(1..=MAX_CANONICAL_FRAMES).contains(&identity.canonical_frames)
                || fact_count == 0 || fact_count > MAX_FACTS as u64 {
                return Err("host content identity, ruleset or fact count is invalid".into());
            }
            let package = tokio::time::timeout(resource::TRANSFER_TIMEOUT, async {
                let objects = match control.recv().await? {
                    Control::Resources { epoch, objects } if epoch == invitation.epoch => objects,
                    _ => return Err("expected four resource descriptors for this epoch".into()),
                };
                resource::validate_objects(&objects)?;
                resource_bytes = objects.iter().try_fold(0_u64, |sum, object| sum.checked_add(object.bytes)).ok_or("resource byte count overflow")?;
                let mut stream = connection.accept_uni().await.map_err(|_| "accept package stream failed")?;
                tokio::select! {
                    biased;
                    error = extra_uni(&connection) => Err(error),
                    package = resource::receive(&mut stream, &package_destination, &identity, invitation.epoch, objects) => package,
                }
            }).await.map_err(|_| "package transfer and validation exceeded 5 minutes")??;
            received = true;
            let prepared = prepare_validated(package, template, PlayerId::P2)?;
            if prepared.identity != identity {
                return Err("received package identity differs from authenticated Welcome".into());
            }
            let mut installed = Session::new(prepared, PlayerId::P2, &invitation, output.clone())?;
            installed.summary.package_received = true;
            installed.summary.resource_bytes = resource_bytes;
            installed.bind_peer(&identity, fact_count)?;
            session = Some(installed);
            let installed = session.as_mut().ok_or("received session disappeared")?;
            control.send(Control::Installed { epoch: invitation.epoch, identity: installed.prepared.identity.clone(), fact_count: installed.prepared.local.len() as u64 }).await?;
            if control.recv().await? != (Control::InstalledAck { epoch: invitation.epoch }) {
                return Err("expected InstalledAck for this validated package".into());
            }
            guest_run(installed, &connection, &mut control).await
        }.await;
        close_endpoint(&endpoint, result.is_err()).await;
        result
    });
    if let Some(session) = session {
        return session.finish(result);
    }
    let error = result
        .err()
        .unwrap_or_else(|| "received session was not initialized".into());
    let summary = serde_json::json!({
        "status": "FAILED", "mode": "headless", "protocol_version": PROTOCOL_VERSION,
        "epoch": invitation.epoch, "player": 2, "peer_authenticated": authenticated,
        "package_received": received, "resource_bytes": resource_bytes,
        "network_timing": null,
        "content_id": content_id, "endpoint": invitation.endpoint,
        "cert_blake3": blake3::Hash::from_bytes(invitation.cert_blake3).to_hex().to_string(),
        "facts": [0,0], "event_count": 0, "live_replay_blake3": null,
        "authority_replay_blake3": null, "error": error,
    });
    write_new(
        &output.join("status.json"),
        &serde_json::to_vec_pretty(&summary)
            .map_err(|_| "encode resource failure evidence failed")?,
    )?;
    Err(format!("{error}; session evidence: {}", output.display()))
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
            stage_compiler_version: None,
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
            protocol_version: PROTOCOL_VERSION,
            endpoint: "127.0.0.1:1".into(),
            server_name: "cocobeat.local".into(),
            certificate_der: vec![],
            cert_blake3: [0; 32],
            token: [0; 32],
            epoch: 7,
        };
        Session::new(prepared, PlayerId::P1, &invitation, PathBuf::new()).unwrap()
    }

    fn live_fixture() -> Session {
        let mut session = fixture();
        session.summary.mode = "live";
        session.declared = [None; 2];
        session
    }

    fn temporary_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "cocobeat-net-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn recovery_owner_requires_exact_prefix_and_preflights_the_whole_suffix() {
        let mut session = live_fixture();
        let prefix = [
            Fact::Hit { seq: 0, frame: 10 },
            Fact::Watermark { through: 10 },
        ];
        for fact in prefix {
            session.ingest(PlayerId::P1, fact).unwrap();
        }
        session
            .ingest(PlayerId::P2, Fact::Watermark { through: 10 })
            .unwrap();
        let before = session.clone();
        let bytes = before.replay.encode().unwrap();
        for tape in [
            vec![prefix[0]],
            vec![prefix[0], Fact::Hit { seq: 1, frame: 20 }],
            vec![prefix[1], prefix[0]],
            vec![Fact::Hit { seq: 0, frame: 11 }, prefix[1]],
            vec![
                prefix[0],
                prefix[1],
                Fact::Hit { seq: 1, frame: 20 },
                Fact::Hit {
                    seq: 2,
                    frame: 48_000,
                },
            ],
        ] {
            assert!(session.append_owner_tape(PlayerId::P1, &tape).is_err());
            assert_eq!(session.replay.encode().unwrap(), bytes);
            assert_eq!(session.counts, before.counts);
            assert_eq!(session.next_seq, before.next_seq);
            assert_eq!(session.last, before.last);
            assert_eq!(session.ended, before.ended);
            assert_eq!(session.engine.events(), before.engine.events());
            assert_eq!(session.engine.resonance(), before.engine.resonance());
        }
        let tape = [
            prefix[0],
            prefix[1],
            Fact::Hit { seq: 1, frame: 20 },
            Fact::Watermark { through: 20 },
        ];
        assert_eq!(
            session.append_owner_tape(PlayerId::P1, &tape).unwrap(),
            tape[2..]
                .iter()
                .map(|fact| fact.into_input(session.epoch, PlayerId::P1))
                .collect::<Vec<_>>()
        );
        assert_eq!(session.player_tape(PlayerId::P1), tape);
        assert_eq!(session.counts, [4, 1]);
        assert_eq!(session.next_seq, [2, 0]);
        let mut old_prefix = Replay::new(before.replay.identity().clone(), before.epoch).unwrap();
        for input in &session.replay.facts()[..before.replay.facts().len()] {
            old_prefix.record(*input).unwrap();
        }
        assert_eq!(old_prefix.encode().unwrap(), bytes);
        let complete = session.replay.clone();
        assert!(
            session
                .append_owner_tape(PlayerId::P1, &tape)
                .unwrap()
                .is_empty()
        );
        assert_eq!(session.replay, complete);
    }

    #[test]
    fn recovery_presentation_returns_only_the_exact_unpresented_peer_suffix() {
        let mut session = live_fixture();
        let tape = [
            Fact::Hit { seq: 0, frame: 10 },
            Fact::Watermark { through: 10 },
            Fact::Hit { seq: 1, frame: 20 },
            Fact::Watermark { through: 20 },
        ];
        for fact in tape {
            session.ingest(PlayerId::P2, fact).unwrap();
        }
        let inputs: Vec<_> = tape
            .iter()
            .map(|fact| fact.into_input(session.epoch, PlayerId::P2))
            .collect();
        let before = session.replay.clone();
        assert_eq!(
            session
                .check_presented_peer(PlayerId::P2, &inputs[..2])
                .unwrap(),
            inputs[2..]
        );
        assert!(
            session
                .check_presented_peer(PlayerId::P2, &inputs)
                .unwrap()
                .is_empty()
        );
        for presented in [
            vec![inputs[0], inputs[2]],
            vec![inputs[1], inputs[0]],
            vec![Fact::Hit { seq: 0, frame: 11 }.into_input(session.epoch, PlayerId::P2)],
            vec![tape[0].into_input(SessionEpoch(session.epoch.0 + 1), PlayerId::P2)],
            vec![tape[0].into_input(session.epoch, PlayerId::P1)],
            inputs.iter().copied().chain([inputs[3]]).collect(),
        ] {
            assert!(
                session
                    .check_presented_peer(PlayerId::P2, &presented)
                    .is_err()
            );
        }
        assert!(session.check_presented_peer(PlayerId::P1, &[]).is_err());
        assert_eq!(session.replay, before);
    }

    #[test]
    fn recovery_snapshots_are_verbatim_new_evidence_without_finishing_the_session() {
        let root = temporary_root();
        fs::create_dir(&root).unwrap();
        let mut session = live_fixture();
        session.output = root.clone();
        session
            .ingest(PlayerId::P1, Fact::Watermark { through: -123 })
            .unwrap();
        let mut gui_identity = session.replay.identity().clone();
        gui_identity.build_id = "runtime-build".into();
        let mut gui = Replay::new(gui_identity.clone(), session.epoch).unwrap();
        gui.record(session.replay.facts()[0]).unwrap();
        gui.record(Fact::Hit { seq: 0, frame: 10 }.into_input(session.epoch, PlayerId::P1))
            .unwrap();
        let before = session.replay.clone();
        let before_summary = serde_json::to_vec(&session.summary).unwrap();
        gui_identity.stage_compiler_version = Some(2);
        let wrong = Replay::new(gui_identity, session.epoch).unwrap();
        assert!(
            session
                .begin_recovery(&wrong, serde_json::json!({}))
                .is_err()
        );
        assert!(!root.join("recovery-1").exists());
        let metadata = serde_json::json!({"attempt": 1, "frame": 10});
        session.begin_recovery(&gui, metadata.clone()).unwrap();
        let recovery = root.join("recovery-1");
        assert_eq!(
            fs::read(recovery.join("worker-prefix.replay.json")).unwrap(),
            before.encode().unwrap()
        );
        assert_eq!(
            fs::read(recovery.join("gui-prefix.replay.json")).unwrap(),
            gui.encode().unwrap()
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &fs::read(recovery.join("metadata.json")).unwrap()
            )
            .unwrap(),
            metadata
        );
        assert!(
            session
                .begin_recovery(&gui, serde_json::json!({"attempt": 2}))
                .is_err()
        );
        assert_eq!(
            fs::read(recovery.join("worker-prefix.replay.json")).unwrap(),
            before.encode().unwrap()
        );
        assert_eq!(session.replay, before);
        assert_eq!(
            serde_json::to_vec(&session.summary).unwrap(),
            before_summary
        );
        assert!(!root.join("live.replay.json").exists());
        assert!(!root.join("status.json").exists());
        let absent = root.join("absent/output");
        session.output = absent;
        assert!(session.begin_recovery(&gui, serde_json::json!({})).is_err());
        assert_eq!(session.replay, before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn peer_binding_and_intake_fail_before_core_or_recorder_change() {
        let mut session = fixture();
        let identity = session.prepared.identity.clone();
        let mut wrong = identity.clone();
        wrong.canonical_frames += 1;
        assert!(session.bind_peer(&wrong, 2).is_err());
        assert!(!session.summary.peer_authenticated);
        wrong = identity.clone();
        wrong.stage_compiler_version = Some(2);
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
        session.declared = [Some(3), Some(2)];
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
        let mut wrong_identity = reordered.identity().clone();
        wrong_identity.stage_compiler_version = Some(2);
        let mut wrong_stage = Replay::new(wrong_identity, reordered.epoch()).unwrap();
        for fact in reordered.facts() {
            wrong_stage.record(*fact).unwrap();
        }
        assert!(session.verify_replay(&wrong_stage).is_err());
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
        let root = temporary_root();
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
