//! One invited native round, with bounded queues and actual input histories

use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc as std_mpsc},
    thread,
    time::Duration,
};

use cocobeat_replay::MAX_FACTS;
use cocobeat_schema::{
    CONTENT_SCHEMA_VERSION, DuoInput, MAX_CANONICAL_FRAMES, PlayerId, SessionEpoch,
};
use quinn::{Connection, Endpoint};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinSet,
    time::Instant,
};

use crate::{
    Invitation, NetworkTiming, PROTOCOL_VERSION, connect_owned, listen, read_invite, resource,
    session::{self, ControlIo, InputIo, RULESET, Session, SessionSummary, other},
    sync,
    wire::{self, Control, Fact, Identity, Input},
    write_invite,
};

const COMMAND_CAPACITY: usize = 256;
const EVENT_CAPACITY: usize = 256;
const PREPARE_TIMEOUT: Duration = Duration::from_secs(120);
const MINIMUM_ARM_LEAD: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
pub enum LiveRole {
    Host {
        package: PathBuf,
        bind: SocketAddr,
        invite: PathBuf,
    },
    Join {
        package: PathBuf,
        invite: PathBuf,
    },
    Receive {
        package_destination: PathBuf,
        invite: PathBuf,
    },
}

#[derive(Clone, Debug)]
pub struct LiveConfig {
    pub role: LiveRole,
    pub output: PathBuf,
}

#[derive(Clone, Copy, Debug)]
pub enum LiveCommand {
    Ready,
    Armed,
    Fact(DuoInput),
    End,
}

#[derive(Debug)]
pub enum LiveEvent {
    Listening {
        epoch: SessionEpoch,
        endpoint: SocketAddr,
        invite: PathBuf,
    },
    Prepared {
        epoch: SessionEpoch,
        player: PlayerId,
        package_path: PathBuf,
        content_id: String,
        canonical_frames: u64,
        stage_compiler_version: u32,
        final_through: i64,
    },
    Scheduled {
        epoch: SessionEpoch,
        deadline: std::time::Instant,
        timing: NetworkTiming,
    },
    Started {
        epoch: SessionEpoch,
    },
    PeerFacts(Vec<DuoInput>),
    Complete(Box<SessionSummary>),
    Failed(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveSendError {
    Full,
    Closed,
}
impl std::fmt::Display for LiveSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Full => "live command queue is full",
            Self::Closed => "live worker stopped",
        })
    }
}
impl std::error::Error for LiveSendError {}

/// Tokio stays on the owned worker thread; callers never block to enqueue or poll
pub struct LiveSession {
    commands: mpsc::Sender<LiveCommand>,
    events: Mutex<std_mpsc::Receiver<LiveEvent>>,
    terminal: Arc<Mutex<Option<LiveEvent>>>,
    cancel: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl LiveSession {
    pub fn spawn(mut config: LiveConfig) -> Result<Self, String> {
        config.output = session::new_destination(&config.output)?;
        match &mut config.role {
            LiveRole::Host {
                package, invite, ..
            } => {
                *package =
                    fs::canonicalize(&*package).map_err(|_| "resolve source package failed")?;
                *invite = session::new_destination(&session::destination_outside_package(
                    package, invite,
                )?)?;
                session::destination_outside_package(package, &config.output)?;
                if invite == &config.output {
                    return Err("invitation and output must be separate paths".into());
                }
            }
            LiveRole::Join { package, .. } => {
                *package =
                    fs::canonicalize(&*package).map_err(|_| "resolve source package failed")?;
                session::destination_outside_package(package, &config.output)?;
            }
            LiveRole::Receive {
                package_destination,
                ..
            } => {
                *package_destination = session::new_destination(package_destination)?;
                if config.output.starts_with(&*package_destination)
                    || package_destination.starts_with(&config.output)
                {
                    return Err("received package and session output must be separate paths".into());
                }
            }
        }
        let (commands, rx) = mpsc::channel(COMMAND_CAPACITY);
        let (tx, events) = std_mpsc::sync_channel(EVENT_CAPACITY);
        let (cancel, cancelled) = oneshot::channel();
        let terminal = Arc::new(Mutex::new(None));
        let ending = terminal.clone();
        let worker = thread::Builder::new()
            .name("cocobeat-network".into())
            .spawn(move || {
                let result = worker(config, rx, tx, cancelled);
                let event = match result {
                    Ok(summary) => LiveEvent::Complete(Box::new(summary)),
                    Err(error) => LiveEvent::Failed(error),
                };
                *ending.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(event);
            })
            .map_err(|error| format!("spawn network worker: {error}"))?;
        Ok(Self {
            commands,
            events: Mutex::new(events),
            terminal,
            cancel: Some(cancel),
            thread: Some(worker),
        })
    }

    /// Full is terminal for the caller's round unless the exact unsent fact is retained
    pub fn try_send(&self, command: LiveCommand) -> Result<(), LiveSendError> {
        if self.cancel.is_none() {
            return Err(LiveSendError::Closed);
        }
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => LiveSendError::Full,
                mpsc::error::TrySendError::Closed(_) => LiveSendError::Closed,
            })
    }

    pub fn try_recv(&self) -> Result<LiveEvent, std_mpsc::TryRecvError> {
        match self
            .events
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .try_recv()
        {
            Ok(event) => Ok(event),
            Err(error) => {
                if let Some(event) = self
                    .terminal
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .take()
                {
                    Ok(event)
                } else if error == std_mpsc::TryRecvError::Disconnected && !self.is_finished() {
                    Err(std_mpsc::TryRecvError::Empty)
                } else {
                    Err(error)
                }
            }
        }
    }

    pub fn is_finished(&self) -> bool {
        self.thread
            .as_ref()
            .is_none_or(|worker| worker.is_finished())
    }

    /// Cancel is nonblocking; the worker saves its actual accepted prefix before exiting
    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        self.cancel();
        if self.is_finished()
            && let Some(worker) = self.thread.take()
        {
            let _ = worker.join();
        }
    }
}

fn emit(events: &std_mpsc::SyncSender<LiveEvent>, event: LiveEvent) -> Result<(), String> {
    events.try_send(event).map_err(|error| match error {
        std_mpsc::TrySendError::Full(_) => {
            "live event queue is full; stopping rather than dropping input".into()
        }
        std_mpsc::TrySendError::Disconnected(_) => "live event consumer stopped".into(),
    })
}

fn validate_identity(identity: &Identity) -> Result<(), String> {
    resource::package_hash(identity)?;
    if identity.ruleset_id != RULESET
        || identity.content_schema != CONTENT_SCHEMA_VERSION
        || identity.stage_compiler_version != Some(cocobeat_schema::STAGE_COMPILER_VERSION)
        || !(1..=MAX_CANONICAL_FRAMES).contains(&identity.canonical_frames)
    {
        return Err("host content identity or ruleset is invalid".into());
    }
    Ok(())
}

fn live_session(
    package: cocobeat_media::ValidatedPackage,
    player: PlayerId,
    invitation: &Invitation,
    output: PathBuf,
) -> Result<Session, String> {
    let mut prepared = session::prepare_package(package)?;
    prepared.identity.stage_compiler_version = Some(cocobeat_schema::STAGE_COMPILER_VERSION);
    let mut session = Session::new(prepared, player, invitation, output)?;
    session.declared = [None; 2];
    session.summary.mode = "live";
    Ok(session)
}

fn bind(session: &mut Session, identity: &Identity) -> Result<(), String> {
    if identity != &session.prepared.identity {
        return Err("peer validated package identity differs".into());
    }
    session.summary.peer_authenticated = true;
    Ok(())
}

fn worker(
    config: LiveConfig,
    mut commands: mpsc::Receiver<LiveCommand>,
    events: std_mpsc::SyncSender<LiveEvent>,
    mut cancel: oneshot::Receiver<()>,
) -> Result<SessionSummary, String> {
    let runtime = session::runtime()?;
    fs::create_dir(&config.output)
        .map_err(|error| format!("create new live session output: {error}"))?;
    let mut state: Option<Session> = None;
    let mut started = false;
    let mut owned_endpoint = None;
    let result = runtime.block_on(async {
        let result = tokio::select! {
            biased;
            _ = &mut cancel => Err("live session cancelled".into()),
            result = run(&config, &mut commands, &events, &mut state, &mut started, &mut owned_endpoint) => result,
        };
        // Keep QUIC's executor alive until cancellation is sent to the actual peer
        if let Some(endpoint) = owned_endpoint.take() {
            session::close_endpoint(&endpoint, result.is_err()).await;
        }
        result
    });
    if let Some(mut state) = state {
        // Already accepted GUI facts must survive cancellation of an in-flight wire write
        if started && result.is_err() {
            while let Ok(command) = commands.try_recv() {
                if let LiveCommand::Fact(input) = command {
                    if ingest_local(&mut state, input).is_err() {
                        break;
                    }
                } else if matches!(command, LiveCommand::End) {
                    let player = state.player;
                    let _ = state.end_live(
                        player,
                        state.counts[player.index()] as u64,
                        state.prepared.final_through,
                    );
                    break;
                } else {
                    break;
                }
            }
        }
        state.finish(result)
    } else {
        let error = result
            .err()
            .unwrap_or_else(|| "live session was not initialized".into());
        let status = serde_json::json!({"status":"FAILED","mode":"live","protocol_version":PROTOCOL_VERSION,"error":error,"facts":[0,0]});
        session::write_new(
            &config.output.join("status.json"),
            &serde_json::to_vec_pretty(&status).map_err(|_| "encode live failure status failed")?,
        )?;
        Err(format!(
            "{error}; session evidence: {}",
            config.output.display()
        ))
    }
}

async fn run(
    config: &LiveConfig,
    commands: &mut mpsc::Receiver<LiveCommand>,
    events: &std_mpsc::SyncSender<LiveEvent>,
    state: &mut Option<Session>,
    started: &mut bool,
    owned_endpoint: &mut Option<Endpoint>,
) -> Result<(), String> {
    let (_endpoint, connection, mut control, package_path, refusal) = match &config.role {
        LiveRole::Host {
            package,
            bind: address,
            invite,
        } => {
            host_prepare(
                package,
                *address,
                invite,
                &config.output,
                events,
                state,
                owned_endpoint,
            )
            .await?
        }
        LiveRole::Join { package, invite } => {
            guest_prepare(
                package,
                false,
                invite,
                &config.output,
                state,
                owned_endpoint,
            )
            .await?
        }
        LiveRole::Receive {
            package_destination,
            invite,
        } => {
            guest_prepare(
                package_destination,
                true,
                invite,
                &config.output,
                state,
                owned_endpoint,
            )
            .await?
        }
    };
    let session = state.as_mut().ok_or("live preparation disappeared")?;
    let result = async {
        emit(events, LiveEvent::Prepared { epoch: session.epoch, player: session.player, package_path, content_id: session.prepared.identity.content_id.clone(), canonical_frames: session.prepared.identity.canonical_frames, stage_compiler_version: cocobeat_schema::STAGE_COMPILER_VERSION, final_through: session.prepared.final_through })?;
        let ready = tokio::time::timeout(PREPARE_TIMEOUT, async {
            tokio::select! {
                biased;
                _ = connection.closed() => Err("peer disconnected before local Ready".into()),
                command = commands.recv() => command.ok_or_else(|| "live command sender stopped before Ready".to_owned()),
            }
        }).await.map_err(|_| "local Ready was not supplied within 120 seconds")??;
        if !matches!(ready, LiveCommand::Ready) { return Err("expected explicit local Ready after package and PCM preparation".into()); }
        let inputs = tokio::time::timeout(PREPARE_TIMEOUT, session::open_inputs(&connection, session.player, session.epoch.0)).await.map_err(|_| "peer did not open the live input stream")??;
        let timing = session.summary.network_timing.insert(NetworkTiming::default());
        let start = tokio::time::timeout(session::IDLE, sync::arm_start(&connection, &mut control, session.epoch, session.player, session.origin, timing)).await.map_err(|_| "live clock/Ready/ScheduleStart barrier timed out")??;
        emit(events, LiveEvent::Scheduled { epoch: session.epoch, deadline: start.into_std(), timing: timing.clone() })?;
        arm_barrier(&connection, &mut control, commands, session.epoch, session.player, start, timing.start_uncertainty_ns.unwrap_or(0)).await?;
        sync::wait_start(&connection, start, session.origin, timing).await?;
        *started = true;
        emit(events, LiveEvent::Started { epoch: session.epoch })?;
        let round_limit = Duration::from_secs(session.prepared.identity.canonical_frames / 48_000 + 60);
        tokio::time::timeout_at(start + round_limit, async {
            tokio::select! {
                biased;
                error = session::extra_bidi(&connection) => Err(error),
                result = async {
                    exchange(session, inputs, commands, events).await?;
                    if session.player == PlayerId::P1 { session::host_finish(session, &connection, &mut control).await }
                    else { session::guest_finish(session, &connection, &mut control).await }
                } => result,
            }
        }).await.map_err(|_| "live session exceeded the song duration plus 60 seconds")?
    }.await;
    drop(refusal);
    result
}

async fn host_prepare(
    package_path: &Path,
    address: SocketAddr,
    invite: &Path,
    output: &Path,
    events: &std_mpsc::SyncSender<LiveEvent>,
    state: &mut Option<Session>,
    owned_endpoint: &mut Option<Endpoint>,
) -> Result<(Endpoint, Connection, ControlIo, PathBuf, JoinSet<()>), String> {
    let package = cocobeat_media::validate_package(package_path)?;
    let (endpoint, invitation) = listen(address)?;
    *owned_endpoint = Some(endpoint.clone());
    *state = Some(live_session(
        package,
        PlayerId::P1,
        &invitation,
        output.to_owned(),
    )?);
    let session = state.as_mut().ok_or("host session disappeared")?;
    write_invite(invite, &invitation)?;
    emit(
        events,
        LiveEvent::Listening {
            epoch: session.epoch,
            endpoint: endpoint
                .local_addr()
                .map_err(|_| "read host address failed")?,
            invite: invite.to_owned(),
        },
    )?;
    let incoming = tokio::time::timeout(PREPARE_TIMEOUT, endpoint.accept())
        .await
        .map_err(|_| "invited live guest did not connect within 120 seconds")?
        .ok_or("host endpoint stopped")?;
    let refusal_endpoint = endpoint.clone();
    let mut refusal = JoinSet::new();
    refusal.spawn(async move {
        while let Some(incoming) = refusal_endpoint.accept().await {
            incoming.refuse();
        }
    });
    let (connection, mut control, fetch) = tokio::time::timeout(Duration::from_secs(10), async {
        let connection = incoming.await.map_err(|_| "guest TLS handshake failed")?;
        let mut control = ControlIo::new(
            connection
                .accept_bi()
                .await
                .map_err(|_| "accept live control failed")?,
        );
        let fetch = match control.recv().await? {
            Control::LiveHello {
                protocol_version: PROTOCOL_VERSION,
                epoch,
                player: 2,
                token,
                identity,
            } if epoch == invitation.epoch && token == invitation.token => {
                bind(session, &identity)?;
                false
            }
            Control::LiveFetch {
                protocol_version: PROTOCOL_VERSION,
                epoch,
                player: 2,
                token,
            } if epoch == invitation.epoch && token == invitation.token => {
                session.summary.peer_authenticated = true;
                true
            }
            _ => return Err("live guest capability, role, epoch or protocol rejected".into()),
        };
        control
            .send(Control::LiveWelcome {
                protocol_version: PROTOCOL_VERSION,
                epoch: invitation.epoch,
                player: 1,
                identity: session.prepared.identity.clone(),
            })
            .await?;
        Ok::<_, String>((connection, control, fetch))
    })
    .await
    .map_err(|_| "live guest capability phase timed out")??;
    if fetch {
        tokio::time::timeout(resource::TRANSFER_TIMEOUT, async {
            let source = resource::Source::open(package_path, &session.prepared.identity)?;
            control
                .send(Control::Resources {
                    epoch: invitation.epoch,
                    objects: source.objects,
                })
                .await?;
            source.send(&connection, invitation.epoch).await?;
            match control.recv().await? {
                Control::LiveInstalled { epoch, identity } if epoch == invitation.epoch => {
                    bind(session, &identity)
                }
                _ => Err("expected validated live Installed package".into()),
            }
        })
        .await
        .map_err(|_| "live resource transfer exceeded 5 minutes")??;
        control
            .send(Control::InstalledAck {
                epoch: invitation.epoch,
            })
            .await?;
    }
    Ok((
        endpoint,
        connection,
        control,
        package_path.to_owned(),
        refusal,
    ))
}

async fn guest_prepare(
    package_path: &Path,
    receive: bool,
    invite: &Path,
    output: &Path,
    state: &mut Option<Session>,
    owned_endpoint: &mut Option<Endpoint>,
) -> Result<(Endpoint, Connection, ControlIo, PathBuf, JoinSet<()>), String> {
    let invitation = read_invite(invite)?;
    let local_package = if receive {
        None
    } else {
        Some(cocobeat_media::validate_package(package_path)?)
    };
    if let Some(package) = local_package {
        *state = Some(live_session(
            package,
            PlayerId::P2,
            &invitation,
            output.to_owned(),
        )?);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let (endpoint, connection) =
        tokio::time::timeout_at(deadline, connect_owned(&invitation, owned_endpoint))
            .await
            .map_err(|_| "host live TLS handshake timed out")??;
    let (mut control, expected) = tokio::time::timeout_at(deadline, async {
        let mut control = ControlIo::new(
            connection
                .open_bi()
                .await
                .map_err(|_| "open live control failed")?,
        );
        let hello = if let Some(session) = state {
            Control::LiveHello {
                protocol_version: PROTOCOL_VERSION,
                epoch: invitation.epoch,
                player: 2,
                token: invitation.token,
                identity: session.prepared.identity.clone(),
            }
        } else {
            Control::LiveFetch {
                protocol_version: PROTOCOL_VERSION,
                epoch: invitation.epoch,
                player: 2,
                token: invitation.token,
            }
        };
        control.send(hello).await?;
        let identity = match control.recv().await? {
            Control::LiveWelcome {
                protocol_version: PROTOCOL_VERSION,
                epoch,
                player: 1,
                identity,
            } if epoch == invitation.epoch => identity,
            _ => return Err("host live role, epoch or protocol rejected".into()),
        };
        validate_identity(&identity)?;
        Ok::<_, String>((control, identity))
    })
    .await
    .map_err(|_| "host live capability phase timed out")??;
    if receive {
        let (package, bytes) = tokio::time::timeout(resource::TRANSFER_TIMEOUT, async {
            let objects = match control.recv().await? {
                Control::Resources { epoch, objects } if epoch == invitation.epoch => objects,
                _ => return Err("expected four live resource descriptors".into()),
            };
            resource::validate_objects(&objects)?;
            let bytes = objects.iter().try_fold(0_u64, |sum, object| sum.checked_add(object.bytes)).ok_or("resource bytes overflow")?;
            let mut stream = connection.accept_uni().await.map_err(|_| "accept live resource stream failed")?;
            let package = tokio::select! {
                biased;
                error = session::extra_uni(&connection) => Err(error),
                result = resource::receive(&mut stream, package_path, &expected, invitation.epoch, objects) => result,
            }?;
            Ok::<_, String>((package, bytes))
        }).await.map_err(|_| "live package transfer exceeded 5 minutes")??;
        let mut session = live_session(package, PlayerId::P2, &invitation, output.to_owned())?;
        session.summary.package_received = true;
        session.summary.resource_bytes = bytes;
        bind(&mut session, &expected)?;
        *state = Some(session);
        control
            .send(Control::LiveInstalled {
                epoch: invitation.epoch,
                identity: expected,
            })
            .await?;
        if control.recv().await?
            != (Control::InstalledAck {
                epoch: invitation.epoch,
            })
        {
            return Err("expected InstalledAck for validated live package".into());
        }
    } else {
        bind(
            state.as_mut().ok_or("guest session disappeared")?,
            &expected,
        )?;
    }
    Ok((
        endpoint,
        connection,
        control,
        package_path.to_owned(),
        JoinSet::new(),
    ))
}

async fn arm_barrier(
    connection: &Connection,
    control: &mut ControlIo,
    commands: &mut mpsc::Receiver<LiveCommand>,
    epoch: SessionEpoch,
    player: PlayerId,
    start: Instant,
    uncertainty_ns: u64,
) -> Result<(), String> {
    let cutoff = start
        .checked_sub(MINIMUM_ARM_LEAD + Duration::from_nanos(uncertainty_ns))
        .ok_or("arm interval overflow")?;
    tokio::time::timeout_at(cutoff, async {
        let command = commands
            .recv()
            .await
            .ok_or("local audio scheduler stopped before Armed")?;
        if !matches!(command, LiveCommand::Armed) {
            return Err("expected local Armed after actual audio scheduling".into());
        }
        if player == PlayerId::P2 {
            control.send(Control::Armed { epoch: epoch.0 }).await?;
        }
        if control.recv().await? != (Control::Armed { epoch: epoch.0 }) {
            return Err("expected peer Armed for this epoch".into());
        }
        if player == PlayerId::P1 {
            control.send(Control::Armed { epoch: epoch.0 }).await?;
        }
        if player == PlayerId::P2 {
            control
                .send(Control::StartConfirmed { epoch: epoch.0 })
                .await?;
        }
        if control.recv().await? != (Control::StartConfirmed { epoch: epoch.0 }) {
            return Err("expected peer StartConfirmed for this epoch".into());
        }
        if player == PlayerId::P1 {
            control
                .send(Control::StartConfirmed { epoch: epoch.0 })
                .await?;
        }
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "audio arm barrier missed the 100 millisecond safety deadline")??;
    if Instant::now() >= cutoff {
        return Err("confirmed audio barrier no longer leaves minimum start lead".into());
    }
    if connection.close_reason().is_some() {
        return Err("connection closed during audio arm barrier".into());
    }
    Ok(())
}

fn ingest_local(session: &mut Session, input: DuoInput) -> Result<(), String> {
    let epoch = match input {
        DuoInput::Hit(hit) => hit.epoch,
        DuoInput::Watermark { epoch, .. } => epoch,
    };
    if epoch != session.epoch || session::seat(input) != session.player {
        return Err("local live input epoch or player direction differs".into());
    }
    session.ingest(session.player, Fact::from_input(input))
}

async fn exchange(
    session: &mut Session,
    inputs: InputIo,
    commands: &mut mpsc::Receiver<LiveCommand>,
    events: &std_mpsc::SyncSender<LiveEvent>,
) -> Result<(), String> {
    let InputIo {
        mut send,
        mut recv,
        mut written,
        mut read,
    } = inputs;
    let peer = other(session.player);
    let mut peer_progressed = Instant::now();
    while !session.ended.iter().all(|ended| *ended) {
        let peer_ended = session.ended[peer.index()];
        let message = {
            // Keep the same read future across local writes so framing stays intact
            let receiving = async {
                if peer_ended {
                    std::future::pending().await
                } else {
                    wire::recv_input(&mut recv, &mut read).await
                }
            };
            tokio::pin!(receiving);
            loop {
                tokio::select! {
                    message = &mut receiving => break Some(message?),
                    command = commands.recv(), if !session.ended[session.player.index()] => {
                        match command.ok_or("live command sender stopped before End")? {
                            LiveCommand::Fact(input) => {
                                ingest_local(session, input)?;
                                wire::send_input(&mut send, &mut written, &Input::Facts { epoch: session.epoch.0, facts: vec![Fact::from_input(input)] }).await?;
                            }
                            LiveCommand::End => {
                                let count = session.counts[session.player.index()] as u64;
                                session.end_live(session.player, count, session.prepared.final_through)?;
                                wire::send_input(&mut send, &mut written, &Input::End { epoch: session.epoch.0, fact_count: count, final_through: session.prepared.final_through }).await?;
                                send.finish().map_err(|_| "finish live input stream failed")?;
                            }
                            _ => return Err("unexpected live command after start".into()),
                        }
                        if session.ended.iter().all(|ended| *ended) { break None; }
                    }
                    _ = tokio::time::sleep_until(peer_progressed + session::IDLE), if !peer_ended => return Err("peer live input progress timed out".into()),
                }
            }
        };
        if let Some(message) = message {
            match message {
                Input::Facts { epoch, facts } if epoch == session.epoch.0 => {
                    if session
                        .replay
                        .facts()
                        .len()
                        .checked_add(facts.len())
                        .is_none_or(|count| count > MAX_FACTS)
                    {
                        return Err("combined live history exceeds Replay capacity".into());
                    }
                    let mut accepted = Vec::with_capacity(facts.len());
                    for fact in facts {
                        session.ingest(peer, fact)?;
                        accepted.push(fact.into_input(session.epoch, peer));
                    }
                    emit(events, LiveEvent::PeerFacts(accepted))?;
                }
                Input::End {
                    epoch,
                    fact_count,
                    final_through,
                } if epoch == session.epoch.0 => {
                    session.end_live(peer, fact_count, final_through)?;
                    wire::ensure_eof(&mut recv).await?;
                }
                _ => return Err("unexpected live input state or epoch".into()),
            }
            peer_progressed = Instant::now();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Prepared;
    use cocobeat_schema::{Anchor, Hit, SongTime};

    fn fixture() -> Session {
        let invitation = Invitation {
            invite_version: 1,
            protocol_version: PROTOCOL_VERSION,
            endpoint: "127.0.0.1:1".into(),
            server_name: "cocobeat.local".into(),
            certificate_der: Vec::new(),
            cert_blake3: [0; 32],
            token: [0; 32],
            epoch: 9,
        };
        let prepared = Prepared {
            identity: Identity {
                content_id: "fixture".into(),
                canonical_frames: 48_000,
                content_schema: 1,
                ruleset_id: RULESET.into(),
                stage_compiler_version: Some(cocobeat_schema::STAGE_COMPILER_VERSION),
            },
            anchors: vec![Anchor {
                id: 1,
                song_time: SongTime::from_frames(10_000),
            }],
            local: Vec::new(),
            template_blake3: String::new(),
            template_epoch: 0,
            end: 48_000,
            final_through: 68_881,
        };
        let mut session =
            Session::new(prepared, PlayerId::P1, &invitation, PathBuf::new()).unwrap();
        session.declared = [None; 2];
        session.summary.mode = "live";
        session
    }

    fn hit(epoch: u64, player: PlayerId, seq: u64, frame: i64) -> DuoInput {
        DuoInput::Hit(Hit {
            epoch: SessionEpoch(epoch),
            player,
            seq,
            song_time: SongTime::from_frames(frame),
        })
    }

    #[test]
    fn live_intake_has_actual_counts_and_no_fabricated_peer_watermark() {
        let mut session = fixture();
        for input in [
            hit(10, PlayerId::P1, 0, 10_000),
            hit(9, PlayerId::P2, 0, 10_000),
            hit(9, PlayerId::P1, 1, 10_000),
            hit(9, PlayerId::P1, 0, -1),
        ] {
            assert!(ingest_local(&mut session, input).is_err());
        }
        assert!(session.replay.facts().is_empty());
        assert_eq!(session.next_seq, [0; 2]);
        ingest_local(&mut session, hit(9, PlayerId::P1, 0, 10_000)).unwrap();
        session
            .ingest(
                PlayerId::P2,
                Fact::Hit {
                    seq: 0,
                    frame: 10_001,
                },
            )
            .unwrap();
        assert!(
            session
                .ingest(
                    PlayerId::P2,
                    Fact::Hit {
                        seq: 0,
                        frame: 10_001
                    }
                )
                .is_err()
        );
        session
            .ingest(PlayerId::P1, Fact::Watermark { through: -2400 })
            .unwrap();
        assert!(
            session
                .ingest(PlayerId::P1, Fact::Watermark { through: -2401 })
                .is_err()
        );
        assert!(session.end_live(PlayerId::P1, 2, 68_881).is_err());
        session
            .ingest(PlayerId::P1, Fact::Watermark { through: 68_881 })
            .unwrap();
        assert!(session.end_live(PlayerId::P1, 2, 68_881).is_err());
        assert!(session.end_live(PlayerId::P1, 3, 68_882).is_err());
        assert!(!session.ended[0]);
        session.end_live(PlayerId::P1, 3, 68_881).unwrap();
        assert!(
            session.engine.events().is_empty(),
            "peer has not closed its actual history"
        );
        assert!(session.end_live(PlayerId::P1, 3, 68_881).is_err());
        assert!(ingest_local(&mut session, hit(9, PlayerId::P1, 1, 20_000)).is_err());
        session
            .ingest(PlayerId::P2, Fact::Watermark { through: 68_881 })
            .unwrap();
        session.end_live(PlayerId::P2, 2, 68_881).unwrap();
        assert_eq!(session.counts, [3, 2]);
        assert_eq!(session.declared, [None, None]);
        assert!(!session.engine.events().is_empty());
        session.verify_replay(&session.replay).unwrap();
    }

    #[test]
    fn queue_pressure_is_explicit_and_terminal_event_survives_full_queue() {
        let (commands, mut commands_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (events_tx, events) = std_mpsc::sync_channel(EVENT_CAPACITY);
        let terminal = Arc::new(Mutex::new(None));
        let (cancel, _cancelled) = oneshot::channel();
        let session = LiveSession {
            commands,
            events: Mutex::new(events),
            terminal: terminal.clone(),
            cancel: Some(cancel),
            thread: None,
        };
        for _ in 0..COMMAND_CAPACITY {
            session.try_send(LiveCommand::Ready).unwrap();
        }
        assert_eq!(session.try_send(LiveCommand::End), Err(LiveSendError::Full));
        assert!(matches!(
            commands_rx.try_recv().unwrap(),
            LiveCommand::Ready
        ));
        for _ in 0..EVENT_CAPACITY {
            emit(
                &events_tx,
                LiveEvent::Started {
                    epoch: SessionEpoch(9),
                },
            )
            .unwrap();
        }
        assert!(
            emit(&events_tx, LiveEvent::PeerFacts(Vec::new()))
                .unwrap_err()
                .contains("rather than dropping input")
        );
        *terminal.lock().unwrap() = Some(LiveEvent::Failed("overflow".into()));
        for _ in 0..EVENT_CAPACITY {
            assert!(matches!(
                session.try_recv().unwrap(),
                LiveEvent::Started { .. }
            ));
        }
        assert!(
            matches!(session.try_recv().unwrap(),LiveEvent::Failed(error) if error=="overflow")
        );
        assert!(matches!(
            session.try_recv(),
            Err(std_mpsc::TryRecvError::Empty)
        ));
        drop(commands_rx);
        assert_eq!(
            session.try_send(LiveCommand::End),
            Err(LiveSendError::Closed)
        );
        drop(events_tx);
        assert!(matches!(
            session.try_recv(),
            Err(std_mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn cancellation_exits_owned_thread_and_flushes_failure_status() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<LiveSession>();
        let root = std::env::temp_dir().join(format!(
            "cocobeat-live-cancel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let mut session = LiveSession::spawn(LiveConfig {
            role: LiveRole::Receive {
                package_destination: root.join("package"),
                invite: root.join("missing-invite"),
            },
            output: root.join("output"),
        })
        .unwrap();
        session.cancel();
        assert_eq!(
            session.try_send(LiveCommand::Ready),
            Err(LiveSendError::Closed)
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !session.is_finished() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(session.is_finished(), "cancelled worker did not exit");
        let status: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("output/status.json")).unwrap()).unwrap();
        assert_eq!(status["status"], "FAILED");
        assert_eq!(status["facts"], serde_json::json!([0, 0]));
        assert!(!root.join("package").exists());
        assert!(matches!(session.try_recv().unwrap(), LiveEvent::Failed(_)));
        drop(session);
        fs::remove_dir_all(root).unwrap();
    }
}
