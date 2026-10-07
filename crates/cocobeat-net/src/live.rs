//! One invited native round, with bounded queues and actual input histories

use std::{
    fs,
    future::Future,
    net::SocketAddr,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex, mpsc as std_mpsc},
    thread,
    time::Duration,
};

use cocobeat_replay::MAX_FACTS;
use cocobeat_schema::{
    CONTENT_SCHEMA_VERSION, DuoInput, MAX_CANONICAL_FRAMES, PlayerId, SessionEpoch, SongTime,
};
use quinn::{Connection, Endpoint};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinSet,
    time::Instant,
};

use crate::{
    Invitation, NetworkTiming, PROTOCOL_VERSION, connect_owned, continuation_capability, listen,
    read_invite, resource,
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

#[derive(Clone, Debug)]
pub struct RecoveryFrozen {
    pub replay: cocobeat_replay::Replay,
    pub paused_frame: SongTime,
    pub source_generation: u64,
    pub source_id: u64,
    pub paused_at: std::time::Instant,
}

#[derive(Clone, Copy, Debug)]
pub struct RecoveryPublication {
    pub sequence: u64,
    pub frame: SongTime,
    pub published_between: [std::time::Instant; 2],
}

#[derive(Clone, Debug)]
pub struct RecoveryObserved {
    pub generation: u64,
    pub source_id: u64,
    pub progress: RecoveryPublication,
    pub publications: Vec<RecoveryPublication>,
}

#[derive(Clone, Debug)]
pub enum LiveCommand {
    Ready,
    Armed,
    Fact(DuoInput),
    End,
    /// Explicit connection maintenance; this is not evidence of packet loss
    RequestRecovery {
        epoch: SessionEpoch,
    },
    RecoveryFrozen {
        epoch: SessionEpoch,
        attempt: u8,
        snapshot: Box<RecoveryFrozen>,
    },
    RecoveryArmed {
        epoch: SessionEpoch,
        attempt: u8,
    },
    RecoveryObserved {
        epoch: SessionEpoch,
        attempt: u8,
        evidence: RecoveryObserved,
    },
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
    RecoveryPausing {
        epoch: SessionEpoch,
        attempt: u8,
    },
    RecoveryScheduled {
        epoch: SessionEpoch,
        attempt: u8,
        deadline: std::time::Instant,
        verify_at: std::time::Instant,
        common_frame: SongTime,
        timing: NetworkTiming,
    },
    RecoverySampling {
        epoch: SessionEpoch,
        attempt: u8,
        not_before: std::time::Instant,
    },
    RecoveryReady {
        epoch: SessionEpoch,
        attempt: u8,
    },
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
        commands.close();
        // Keep QUIC's executor alive until cancellation is sent to the actual peer
        if let Some(endpoint) = owned_endpoint.take() {
            session::close_endpoint(&endpoint, result.is_err()).await;
        }
        result
    });
    if let Some(mut state) = state {
        // Already accepted GUI facts must survive cancellation of an in-flight wire write
        if started && result.is_err() {
            drain_accepted_commands(&mut state, &mut commands);
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

fn drain_accepted_commands(session: &mut Session, commands: &mut mpsc::Receiver<LiveCommand>) {
    for _ in 0..COMMAND_CAPACITY {
        let Ok(command) = commands.try_recv() else {
            break;
        };
        match command {
            LiveCommand::Fact(input) => {
                if ingest_local(session, input).is_err() {
                    break;
                }
            }
            LiveCommand::End => {
                let player = session.player;
                let _ = session.end_live(
                    player,
                    session.counts[player.index()] as u64,
                    session.prepared.final_through,
                );
                break;
            }
            LiveCommand::RecoveryFrozen { .. }
            | LiveCommand::RecoveryArmed { .. }
            | LiveCommand::RecoveryObserved { .. }
            | LiveCommand::RequestRecovery { .. } => {}
            _ => break,
        }
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
    let mut continuation = None;
    let (endpoint, connection, mut control, package_path, refusal) = match &config.role {
        LiveRole::Host {
            package,
            bind: address,
            invite,
        } => {
            host_prepare(
                package,
                (*address, invite),
                &config.output,
                events,
                state,
                owned_endpoint,
                &mut continuation,
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
                &mut continuation,
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
                &mut continuation,
            )
            .await?
        }
    };
    let session = state.as_mut().ok_or("live preparation disappeared")?;
    async {
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
        drop(refusal);
        let round_limit = Duration::from_secs(session.prepared.identity.canonical_frames / 48_000 + 60);
        let continuation = continuation.as_mut().ok_or("missing continuation identity")?;
        run_started(session, (&endpoint, connection, control, inputs), commands, events, continuation, owned_endpoint, start + round_limit).await

    }.await
}

async fn host_prepare(
    package_path: &Path,
    listener: (SocketAddr, &Path),
    output: &Path,
    events: &std_mpsc::SyncSender<LiveEvent>,
    state: &mut Option<Session>,
    owned_endpoint: &mut Option<Endpoint>,
    continuation: &mut Option<Continuation>,
) -> Result<(Endpoint, Connection, ControlIo, PathBuf, JoinSet<()>), String> {
    let (address, invite) = listener;
    let package = cocobeat_media::validate_package(package_path)?;
    let (endpoint, invitation) = listen(address)?;
    let capability = continuation_capability()?;
    *continuation = Some(Continuation {
        invitation: invitation.clone(),
        capability,
        used: false,
        candidates: 0,
        pending: JoinSet::new(),
    });
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
                capability,
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
    continuation: &mut Option<Continuation>,
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
        let (identity, capability) = match control.recv().await? {
            Control::LiveWelcome {
                protocol_version: PROTOCOL_VERSION,
                epoch,
                player: 1,
                identity,
                capability,
            } if epoch == invitation.epoch => (identity, capability),
            _ => return Err("host live role, epoch or protocol rejected".into()),
        };
        validate_identity(&identity)?;
        *continuation = Some(Continuation {
            invitation: invitation.clone(),
            capability,
            used: false,
            candidates: 0,
            pending: JoinSet::new(),
        });
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

const RECOVERY_REQUESTED: u32 = 0x4343;
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const CANDIDATE_LIMIT: u8 = 4;

struct Continuation {
    invitation: Invitation,
    capability: [u8; 32],
    used: bool,
    candidates: u8,
    pending: JoinSet<Result<AuthenticatedContinuation, String>>,
}

#[derive(Clone, Copy)]
struct RemoteFreeze {
    frame: i64,
    generation: u64,
    source_id: u64,
    count: u64,
}

struct AuthenticatedContinuation {
    connection: Connection,
    control: ControlIo,
    peer: RemoteFreeze,
}

#[derive(Debug)]
enum ExchangeError {
    Recoverable(&'static str),
    Terminal(String),
}
impl From<String> for ExchangeError {
    fn from(error: String) -> Self {
        Self::Terminal(error)
    }
}
impl From<&str> for ExchangeError {
    fn from(error: &str) -> Self {
        Self::Terminal(error.into())
    }
}
impl From<wire::LiveIoError> for ExchangeError {
    fn from(error: wire::LiveIoError) -> Self {
        match error {
            wire::LiveIoError::Deadline => Self::Recoverable("reliable frame deadline"),
            wire::LiveIoError::Transport(quinn::ConnectionError::TimedOut) => {
                Self::Recoverable("QUIC idle timeout")
            }
            wire::LiveIoError::Transport(quinn::ConnectionError::Reset) => {
                Self::Recoverable("QUIC stateless reset")
            }
            wire::LiveIoError::Transport(quinn::ConnectionError::ApplicationClosed(close))
                if close.error_code.into_inner() == u64::from(RECOVERY_REQUESTED) =>
            {
                Self::Recoverable("authenticated peer requested connection maintenance")
            }
            other => Self::Terminal(other.to_string()),
        }
    }
}
impl ExchangeError {
    fn into_message(self) -> String {
        match self {
            Self::Recoverable(reason) => reason.into(),
            Self::Terminal(error) => error,
        }
    }
}

async fn authenticate_continuation(
    incoming: quinn::Incoming,
    invitation: Invitation,
    identity: Identity,
    capability: [u8; 32],
) -> Result<AuthenticatedContinuation, String> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let connection = incoming
            .await
            .map_err(|_| "continuation TLS handshake failed")?;
        let mut control = ControlIo::new(
            connection
                .accept_bi()
                .await
                .map_err(|_| "accept continuation control failed")?,
        );
        let peer = match control.recv().await? {
            Control::ResumeHello {
                protocol_version: PROTOCOL_VERSION,
                epoch,
                player: 2,
                identity: actual,
                attempt: sync::RESUME_ATTEMPT,
                pause_frame,
                source_generation,
                source_id,
                owner_count,
                started: true,
                ended: false,
                capability: supplied,
            } if epoch == invitation.epoch
                && actual == identity
                && supplied == capability
                && (0..identity.canonical_frames as i64).contains(&pause_frame)
                && source_generation != 0
                && source_id != 0 =>
            {
                RemoteFreeze {
                    frame: pause_frame,
                    generation: source_generation,
                    source_id,
                    count: owner_count,
                }
            }
            _ => {
                connection.close(1_u8.into(), b"continuation rejected");
                return Err("continuation capability, identity, role or state rejected".into());
            }
        };
        Ok(AuthenticatedContinuation {
            connection,
            control,
            peer,
        })
    })
    .await
    .map_err(|_| "continuation authentication timed out")?
}

struct ResumeContext {
    epoch: SessionEpoch,
    player: PlayerId,
    origin: Instant,
    end: i64,
    frozen: Box<RecoveryFrozen>,
    peer: RemoteFreeze,
    deadline: Instant,
}
struct ResumeSignals {
    armed: Option<oneshot::Sender<()>>,
    observed: Option<oneshot::Sender<RecoveryObserved>>,
}
type ResumeGate<'a> = Pin<Box<dyn Future<Output = Result<Duration, String>> + 'a>>;

struct ExchangeControl<'a> {
    gate: Option<ResumeGate<'a>>,
    signals: Option<ResumeSignals>,
    deadline: Instant,
    running_deadline: Instant,
    recovering: bool,
}

async fn freeze_and_reconnect(
    session: &mut Session,
    transport: (&Connection, &Endpoint, Option<AuthenticatedContinuation>),
    commands: &mut mpsc::Receiver<LiveCommand>,
    events: &std_mpsc::SyncSender<LiveEvent>,
    continuation: &mut Continuation,
    owned_endpoint: &mut Option<Endpoint>,
    cause: &'static str,
) -> Result<(Connection, ControlIo, InputIo, ResumeContext), String> {
    let (old_connection, endpoint, authenticated) = transport;
    if (continuation.used && authenticated.is_none()) || session.ended.iter().any(|ended| *ended) {
        return Err("continuation is spent or a player already ended".into());
    }
    let deadline = Instant::now() + RECOVERY_TIMEOUT;
    tokio::time::timeout_at(deadline, async {
        old_connection.close(RECOVERY_REQUESTED.into(), b"connection maintenance");
        emit(
            events,
            LiveEvent::RecoveryPausing {
                epoch: session.epoch,
                attempt: sync::RESUME_ATTEMPT,
            },
        )?;
        let frozen = tokio::time::timeout_at(deadline, async {
            loop {
                match commands
                    .recv()
                    .await
                    .ok_or("recovery command sender stopped")?
                {
                    LiveCommand::Fact(input) => ingest_local(session, input)?,
                    LiveCommand::RecoveryFrozen {
                        epoch,
                        attempt: sync::RESUME_ATTEMPT,
                        snapshot,
                    } if epoch == session.epoch => break Ok::<_, String>(snapshot),
                    _ => {
                        return Err(
                            "expected actual audio freeze after the accepted fact fence".into()
                        );
                    }
                }
            }
        })
        .await
        .map_err(|_| "actual audio freeze timed out")??;
        if frozen.replay.epoch() != session.epoch
            || !(0..session.prepared.end).contains(&frozen.paused_frame.frames())
            || frozen.source_generation == 0
            || frozen.source_id == 0
            || frozen.paused_at > std::time::Instant::now()
            || frozen.paused_at < session.origin.into_std()
        {
            return Err("frozen audio source identity, frame or acknowledgment differs".into());
        }
        session.begin_recovery(&frozen.replay, serde_json::json!({
        "attempt": sync::RESUME_ATTEMPT, "epoch": session.epoch.0, "cause": cause,
        "paused_frame": frozen.paused_frame.frames(), "source_generation": frozen.source_generation,
        "source_id": frozen.source_id, "worker_counts": session.counts,
    }))?;
        let mut gui_tapes: [Vec<DuoInput>; 2] = [Vec::new(), Vec::new()];
        for &input in frozen.replay.facts() {
            gui_tapes[session::seat(input).index()].push(input);
        }
        let local_tape: Vec<_> = gui_tapes[session.player.index()]
            .iter()
            .copied()
            .map(Fact::from_input)
            .collect();
        session.append_owner_tape(session.player, &local_tape)?;
        if Instant::now() >= deadline {
            return Err(
                "freeze snapshot and owner preflight exceeded the recovery deadline".into(),
            );
        }
        let missing_gui = session.check_presented_peer(
            other(session.player),
            &gui_tapes[other(session.player).index()],
        )?;
        if !missing_gui.is_empty() {
            emit(events, LiveEvent::PeerFacts(missing_gui))?;
        }
        let own = RemoteFreeze {
            frame: frozen.paused_frame.frames(),
            generation: frozen.source_generation,
            source_id: frozen.source_id,
            count: local_tape.len() as u64,
        };
        let AuthenticatedContinuation {
            connection,
            mut control,
            peer,
        } = tokio::time::timeout_at(deadline, async {
            Ok::<_, String>(if session.player == PlayerId::P1 {
                let candidate = if let Some(candidate) = authenticated {
                    candidate
                } else {
                    loop {
                        if !continuation.pending.is_empty() {
                            if let Some(Ok(Ok(candidate))) = continuation.pending.join_next().await
                            {
                                break candidate;
                            }
                            continue;
                        }
                        if continuation.candidates >= CANDIDATE_LIMIT {
                            tokio::time::sleep_until(deadline).await;
                            return Err("continuation candidate budget exhausted".into());
                        }
                        let incoming = endpoint
                            .accept()
                            .await
                            .ok_or("host endpoint stopped during recovery")?;
                        continuation.candidates += 1;
                        if let Ok(candidate) = authenticate_continuation(
                            incoming,
                            continuation.invitation.clone(),
                            session.prepared.identity.clone(),
                            continuation.capability,
                        )
                        .await
                        {
                            break candidate;
                        }
                    }
                };
                continuation.used = true;
                candidate
            } else {
                let (_, connection) =
                    connect_owned(&continuation.invitation, owned_endpoint).await?;
                let mut control = ControlIo::new(
                    connection
                        .open_bi()
                        .await
                        .map_err(|_| "open continuation control failed")?,
                );
                control
                    .send(Control::ResumeHello {
                        protocol_version: PROTOCOL_VERSION,
                        epoch: session.epoch.0,
                        player: 2,
                        identity: session.prepared.identity.clone(),
                        attempt: sync::RESUME_ATTEMPT,
                        pause_frame: own.frame,
                        source_generation: own.generation,
                        source_id: own.source_id,
                        owner_count: own.count,
                        started: true,
                        ended: false,
                        capability: continuation.capability,
                    })
                    .await?;
                let peer = match control.recv().await? {
                    Control::ResumeWelcome {
                        protocol_version: PROTOCOL_VERSION,
                        epoch,
                        player: 1,
                        identity,
                        attempt: sync::RESUME_ATTEMPT,
                        pause_frame,
                        source_generation,
                        source_id,
                        owner_count,
                        started: true,
                        ended: false,
                    } if epoch == session.epoch.0
                        && identity == session.prepared.identity
                        && (0..session.prepared.end).contains(&pause_frame)
                        && source_generation != 0
                        && source_id != 0 =>
                    {
                        RemoteFreeze {
                            frame: pause_frame,
                            generation: source_generation,
                            source_id,
                            count: owner_count,
                        }
                    }
                    _ => return Err("host continuation state or identity differs".into()),
                };
                continuation.used = true;
                AuthenticatedContinuation {
                    connection,
                    control,
                    peer,
                }
            })
        })
        .await
        .map_err(|_| "continuation authentication exceeded the recovery deadline")??;
        if session.player == PlayerId::P1 {
            control
                .send(Control::ResumeWelcome {
                    protocol_version: PROTOCOL_VERSION,
                    epoch: session.epoch.0,
                    player: 1,
                    identity: session.prepared.identity.clone(),
                    attempt: sync::RESUME_ATTEMPT,
                    pause_frame: own.frame,
                    source_generation: own.generation,
                    source_id: own.source_id,
                    owner_count: own.count,
                    started: true,
                    ended: false,
                })
                .await?;
        }
        if own
            .count
            .checked_add(peer.count)
            .is_none_or(|count| count > MAX_FACTS as u64)
        {
            return Err("combined recovery owner tapes exceed Replay capacity".into());
        }
        let mut inputs = tokio::time::timeout_at(
            deadline,
            open_resume_inputs(&connection, session, own.count, peer.count),
        )
        .await
        .map_err(|_| "continuation input stream timed out")??;
        let peer_tape = tokio::time::timeout_at(deadline, async {
            let sending = send_owner_tape(
                &mut inputs.send,
                &mut inputs.written,
                session.epoch,
                &local_tape,
            );
            let receiving = receive_owner_tape(
                &mut inputs.recv,
                &mut inputs.read,
                session.epoch,
                peer.count,
            );
            tokio::try_join!(sending, receiving).map(|(_, tape)| tape)
        })
        .await
        .map_err(|_| "complete recovery tapes exceeded the deadline")??;
        let appended_peer = session.append_owner_tape(other(session.player), &peer_tape)?;
        if !appended_peer.is_empty() {
            emit(events, LiveEvent::PeerFacts(appended_peer))?;
        }
        if Instant::now() >= deadline {
            return Err("owner tape preflight exceeded the recovery deadline".into());
        }
        let counts = [session.counts[0] as u64, session.counts[1] as u64];
        let ready = Control::ResumeTapeReady {
            epoch: session.epoch.0,
            attempt: sync::RESUME_ATTEMPT,
            counts,
        };
        if session.player == PlayerId::P2 {
            control.send(ready.clone()).await?;
        }
        if control.recv().await? != ready {
            return Err("complete owner tape counts differ before resume".into());
        }
        if session.player == PlayerId::P1 {
            control.send(ready).await?;
        }
        Ok((
            connection,
            control,
            inputs,
            ResumeContext {
                epoch: session.epoch,
                player: session.player,
                origin: session.origin,
                end: session.prepared.end,
                frozen,
                peer,
                deadline,
            },
        ))
    })
    .await
    .map_err(|_| "freeze and continuation exceeded the total recovery deadline")?
}

async fn open_resume_inputs(
    connection: &Connection,
    session: &Session,
    owner_count: u64,
    peer_count: u64,
) -> Result<InputIo, String> {
    let (send, recv) = if session.player == PlayerId::P1 {
        connection.accept_bi().await
    } else {
        connection.open_bi().await
    }
    .map_err(|_| "open continuation input stream failed")?;
    let mut inputs = InputIo {
        send,
        recv,
        written: 0,
        read: 0,
    };
    let opening = Input::ResumeOpen {
        epoch: session.epoch.0,
        attempt: sync::RESUME_ATTEMPT,
        player: session.player.index() as u8 + 1,
        count: owner_count,
    };
    if session.player == PlayerId::P2 {
        wire::send_input(&mut inputs.send, &mut inputs.written, &opening).await?;
    }
    if wire::recv_input(&mut inputs.recv, &mut inputs.read).await?
        != (Input::ResumeOpen {
            epoch: session.epoch.0,
            attempt: sync::RESUME_ATTEMPT,
            player: other(session.player).index() as u8 + 1,
            count: peer_count,
        })
    {
        return Err("continuation input direction, attempt or frozen count differs".into());
    }
    if session.player == PlayerId::P1 {
        wire::send_input(&mut inputs.send, &mut inputs.written, &opening).await?;
    }
    Ok(inputs)
}

fn tape_digest(tape: &[Fact]) -> Result<[u8; 32], String> {
    let bytes = serde_json::to_vec(tape).map_err(|_| "encode owner tape digest failed")?;
    Ok(*blake3::hash(&bytes).as_bytes())
}
async fn send_owner_tape(
    send: &mut quinn::SendStream,
    bytes: &mut u64,
    epoch: SessionEpoch,
    tape: &[Fact],
) -> Result<(), String> {
    for (index, chunk) in tape.chunks(64).enumerate() {
        wire::send_input(
            send,
            bytes,
            &Input::ResumeFacts {
                epoch: epoch.0,
                attempt: sync::RESUME_ATTEMPT,
                from_fact_index: (index * 64) as u64,
                facts: chunk.to_vec(),
            },
        )
        .await?;
    }
    wire::send_input(
        send,
        bytes,
        &Input::ResumeEnd {
            epoch: epoch.0,
            attempt: sync::RESUME_ATTEMPT,
            count: tape.len() as u64,
            blake3: tape_digest(tape)?,
        },
    )
    .await
}
async fn receive_owner_tape(
    recv: &mut quinn::RecvStream,
    bytes: &mut u64,
    epoch: SessionEpoch,
    count: u64,
) -> Result<Vec<Fact>, String> {
    let mut tape = Vec::with_capacity(usize::try_from(count).map_err(|_| "owner count overflow")?);
    loop {
        match wire::recv_input(recv, bytes).await? {
            Input::ResumeFacts {
                epoch: actual,
                attempt: sync::RESUME_ATTEMPT,
                from_fact_index,
                facts,
            } if actual == epoch.0
                && from_fact_index == tape.len() as u64
                && (tape.len() as u64)
                    .checked_add(facts.len() as u64)
                    .is_some_and(|total| total <= count) =>
            {
                tape.extend(facts)
            }
            Input::ResumeEnd {
                epoch: actual,
                attempt: sync::RESUME_ATTEMPT,
                count: declared,
                blake3,
            } if actual == epoch.0
                && declared == count
                && tape.len() as u64 == count
                && blake3 == tape_digest(&tape)? =>
            {
                return Ok(tape);
            }
            _ => return Err("recovery owner tape index, count, digest or attempt differs".into()),
        }
    }
}

fn publication_wire(
    publication: RecoveryPublication,
    origin: Instant,
) -> Result<wire::GateObservation, String> {
    let convert = |time: std::time::Instant| -> Result<u64, String> {
        u64::try_from(
            time.checked_duration_since(origin.into_std())
                .ok_or("publication precedes session origin")?
                .as_nanos(),
        )
        .map_err(|_| "publication monotonic overflow".into())
    };
    Ok(wire::GateObservation {
        sequence: publication.sequence,
        frame: publication.frame.frames(),
        publication_before_ns: convert(publication.published_between[0])?,
        publication_after_ns: convert(publication.published_between[1])?,
    })
}

fn check_publication_continuation(local: &RecoveryObserved) -> Result<(), String> {
    let first = local
        .publications
        .first()
        .ok_or("missing resumed source publications")?;
    if first.frame < local.progress.frame
        || first.published_between[0] < local.progress.published_between[1]
    {
        return Err("source publication regressed behind acknowledged resume progress".into());
    }
    Ok(())
}

async fn resume_control(
    connection: &Connection,
    control: &mut ControlIo,
    events: &std_mpsc::SyncSender<LiveEvent>,
    context: ResumeContext,
    acknowledgments: (oneshot::Receiver<()>, oneshot::Receiver<RecoveryObserved>),
) -> Result<Duration, String> {
    tokio::time::timeout_at(context.deadline, async {
        let ResumeContext {
            epoch,
            player,
            origin,
            end,
            frozen,
            peer,
            ..
        } = context;
        let mut paused = [peer.frame; 2];
        paused[player.index()] = frozen.paused_frame.frames();
        let mut sources = [(peer.generation, peer.source_id); 2];
        sources[player.index()] = (frozen.source_generation, frozen.source_id);
        let plan =
            sync::arm_resume(connection, control, epoch, player, origin, paused, end).await?;
        emit(
            events,
            LiveEvent::RecoveryScheduled {
                epoch,
                attempt: sync::RESUME_ATTEMPT,
                deadline: plan.deadline.into_std(),
                verify_at: plan.verify_at.into_std(),
                common_frame: SongTime::from_frames(plan.common_frame),
                timing: plan.timing.clone(),
            },
        )?;
        let cutoff = plan
            .deadline
            .checked_sub(
                MINIMUM_ARM_LEAD
                    + Duration::from_nanos(plan.timing.start_uncertainty_ns.unwrap_or(0)),
            )
            .ok_or("resume arm interval overflow")?;
        let (armed, observed) = acknowledgments;
        tokio::time::timeout_at(cutoff, async {
            armed
                .await
                .map_err(|_| "local resume scheduler stopped before Armed")?;
            let armed = Control::ResumeArmed {
                epoch: epoch.0,
                attempt: sync::RESUME_ATTEMPT,
            };
            let confirmed = Control::ResumeConfirmed {
                epoch: epoch.0,
                attempt: sync::RESUME_ATTEMPT,
            };
            if player == PlayerId::P2 {
                control.send(armed.clone()).await?;
            }
            if control.recv().await? != armed {
                return Err("peer resume Armed identity differs".into());
            }
            if player == PlayerId::P1 {
                control.send(armed).await?;
            }
            if player == PlayerId::P2 {
                control.send(confirmed.clone()).await?;
            }
            if control.recv().await? != confirmed {
                return Err("peer resume confirmation differs".into());
            }
            if player == PlayerId::P1 {
                control.send(confirmed).await?;
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(|_| "resume Armed barrier missed the safety deadline")??;
        if Instant::now() >= cutoff {
            return Err("confirmed resume lost the minimum arm lead".into());
        }
        tokio::time::sleep_until(
            plan.verify_at
                .checked_sub(Duration::from_millis(300))
                .ok_or("verification clock refresh underflow")?,
        )
        .await;
        let mut refreshed = NetworkTiming::default();
        let mut clock = sync::capture_clock(
            connection,
            control,
            epoch,
            sync::RESUME_ATTEMPT,
            player,
            origin,
            &mut refreshed,
        )
        .await?;
        let sample = refreshed
            .clock
            .as_ref()
            .ok_or("missing refreshed resume clock")?
            .guest_sample_ns;
        emit(
            events,
            LiveEvent::RecoverySampling {
                epoch,
                attempt: sync::RESUME_ATTEMPT,
                not_before: std::time::Instant::now(),
            },
        )?;
        let local = observed
            .await
            .map_err(|_| "source observation producer stopped")?;
        if (local.generation, local.source_id) != (frozen.source_generation, frozen.source_id)
            || local.progress.sequence == 0
            || local.progress.frame <= frozen.paused_frame
            || local.progress.frame.frames() >= end
            || local.progress.published_between[0] < frozen.paused_at
            || local.progress.published_between[0] > local.progress.published_between[1]
            || local.progress.published_between[1] > std::time::Instant::now()
        {
            return Err("original source did not acknowledge real forward resume".into());
        }
        let pause_extension = local.progress.published_between[1]
            .checked_duration_since(frozen.paused_at)
            .filter(|duration| *duration <= RECOVERY_TIMEOUT)
            .ok_or("acknowledged pause exceeded the recovery window")?;
        let read_finished = std::time::Instant::now();
        if local
            .publications
            .iter()
            .any(|row| row.published_between[1] > read_finished)
        {
            return Err("source publication lies after the completed snapshot read".into());
        }
        check_publication_continuation(&local)?;
        let own = wire::GateEvidence {
            generation: local.generation,
            source_id: local.source_id,
            progress_sequence: local.progress.sequence,
            observations: local
                .publications
                .into_iter()
                .map(|publication| publication_wire(publication, origin))
                .collect::<Result<_, _>>()?,
        };
        own.validate()?;
        if player == PlayerId::P2 {
            control
                .send(Control::ResumeObserved {
                    epoch: epoch.0,
                    attempt: sync::RESUME_ATTEMPT,
                    evidence: own.clone(),
                })
                .await?;
        }
        let remote = match control.recv().await? {
            Control::ResumeObserved {
                epoch: actual,
                attempt: sync::RESUME_ATTEMPT,
                evidence,
            } if actual == epoch.0 => evidence,
            _ => return Err("peer resume observations belong to another epoch or attempt".into()),
        };
        if player == PlayerId::P1 {
            control
                .send(Control::ResumeObserved {
                    epoch: epoch.0,
                    attempt: sync::RESUME_ATTEMPT,
                    evidence: own.clone(),
                })
                .await?;
        }
        let evidence = if player == PlayerId::P1 {
            [own, remote]
        } else {
            [remote, own]
        };
        sync::resume_gate(
            &mut clock,
            sample,
            &evidence,
            sync::GateWindow {
                paused,
                sources,
                end,
                common_frame: plan.common_frame,
                host_verify_ns: plan.host_verify_ns,
            },
        )?;
        if Instant::now() >= context.deadline {
            return Err("source gate exceeded the fixed recovery deadline".into());
        }
        let gate = Control::ResumeGate {
            epoch: epoch.0,
            attempt: sync::RESUME_ATTEMPT,
        };
        let ack = Control::ResumeGateAck {
            epoch: epoch.0,
            attempt: sync::RESUME_ATTEMPT,
        };
        let live = Control::ResumeLive {
            epoch: epoch.0,
            attempt: sync::RESUME_ATTEMPT,
        };
        if player == PlayerId::P1 {
            control.send(gate).await?;
            if control.recv().await? != ack {
                return Err("peer did not confirm actual source gate".into());
            }
            if Instant::now() >= context.deadline {
                return Err("resume Live missed the fixed recovery deadline".into());
            }
            control.send(live).await?;
        } else {
            if control.recv().await? != gate {
                return Err("host actual source gate differs".into());
            }
            control.send(ack).await?;
            if control.recv().await? != live {
                return Err("host continuation Live belongs to another attempt".into());
            }
        }
        if Instant::now() >= context.deadline {
            return Err("confirmed resume exceeded the fixed recovery deadline".into());
        }
        Ok(pause_extension)
    })
    .await
    .map_err(|_| "resume exceeded the total recovery deadline")?
}

async fn run_started(
    session: &mut Session,
    transport: (&Endpoint, Connection, ControlIo, InputIo),
    commands: &mut mpsc::Receiver<LiveCommand>,
    events: &std_mpsc::SyncSender<LiveEvent>,
    continuation: &mut Continuation,
    owned_endpoint: &mut Option<Endpoint>,
    deadline: Instant,
) -> Result<(), String> {
    let (endpoint, connection, mut control, mut inputs) = transport;
    let mut mode = ExchangeControl {
        gate: None,
        signals: None,
        deadline,
        running_deadline: deadline,
        recovering: false,
    };
    let outcome = exchange(
        session,
        (&connection, endpoint),
        &mut inputs,
        commands,
        events,
        continuation,
        &mut mode,
    )
    .await;
    let (candidate, cause) = match outcome {
        Ok(None) => {
            return tokio::time::timeout_at(mode.deadline, async {
                if session.player == PlayerId::P1 {
                    session::host_finish(session, &connection, &mut control).await
                } else {
                    session::guest_finish(session, &connection, &mut control).await
                }
            })
            .await
            .map_err(|_| "live FinishAck exceeded the round deadline")?;
        }
        Ok(Some(candidate)) => (Some(candidate), "authenticated peer continuation"),
        Err(ExchangeError::Recoverable(cause))
            if !continuation.used && !session.ended.iter().any(|ended| *ended) =>
        {
            (None, cause)
        }
        Err(error) => return Err(error.into_message()),
    };
    drop(inputs);
    let (connection, mut control, mut inputs, context) = freeze_and_reconnect(
        session,
        (&connection, endpoint, candidate),
        commands,
        events,
        continuation,
        owned_endpoint,
        cause,
    )
    .await?;
    let recovery_deadline = context.deadline;
    let (armed_tx, armed_rx) = oneshot::channel();
    let (observed_tx, observed_rx) = oneshot::channel();
    let gate = Box::pin(resume_control(
        &connection,
        &mut control,
        events,
        context,
        (armed_rx, observed_rx),
    ));
    let mut mode = ExchangeControl {
        gate: Some(gate),
        signals: Some(ResumeSignals {
            armed: Some(armed_tx),
            observed: Some(observed_tx),
        }),
        deadline: recovery_deadline,
        running_deadline: deadline,
        recovering: true,
    };
    match exchange(
        session,
        (&connection, endpoint),
        &mut inputs,
        commands,
        events,
        continuation,
        &mut mode,
    )
    .await
    {
        Ok(None) => {}
        Ok(Some(_)) => return Err("a second continuation is forbidden".into()),
        Err(error) => return Err(error.into_message()),
    }
    let finish_deadline = mode.deadline;
    drop(mode);
    tokio::time::timeout_at(finish_deadline, async {
        if session.player == PlayerId::P1 {
            session::host_finish(session, &connection, &mut control).await
        } else {
            session::guest_finish(session, &connection, &mut control).await
        }
    })
    .await
    .map_err(|_| "continued FinishAck exceeded the acknowledged pause deadline")?
}

fn accept_peer_batch(
    session: &mut Session,
    peer: PlayerId,
    facts: &[Fact],
    events: &std_mpsc::SyncSender<LiveEvent>,
) -> Result<(), String> {
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
    for &fact in facts {
        session.ingest(peer, fact)?;
        accepted.push(fact.into_input(session.epoch, peer));
    }
    emit(events, LiveEvent::PeerFacts(accepted))
}

async fn exchange(
    session: &mut Session,
    transport: (&Connection, &Endpoint),
    inputs: &mut InputIo,
    commands: &mut mpsc::Receiver<LiveCommand>,
    events: &std_mpsc::SyncSender<LiveEvent>,
    continuation: &mut Continuation,
    mode: &mut ExchangeControl<'_>,
) -> Result<Option<AuthenticatedContinuation>, ExchangeError> {
    let (connection, endpoint) = transport;
    let InputIo {
        send,
        recv,
        written,
        read,
    } = inputs;
    let peer = other(session.player);
    let mut peer_progressed = Instant::now();
    let mut deferred_peer = Vec::new();
    let mut deferred_end = None;
    while !session.ended.iter().all(|ended| *ended) {
        let peer_ended = session.ended[peer.index()];
        let receiving_closed = peer_ended || deferred_end.is_some();
        let message = {
            // This same frame read survives writes, clock/gate phases and the return to Running
            let receiving = async {
                if receiving_closed {
                    std::future::pending().await
                } else {
                    wire::recv_live_input(recv, read).await
                }
            };
            tokio::pin!(receiving);
            loop {
                tokio::select! {
                    message = &mut receiving => break Some(message.map_err(ExchangeError::from)?),
                    reason = connection.closed() => return Err(wire::LiveIoError::Transport(reason).into()),
                    error = session::extra_bidi(connection) => return Err(error.into()),
                    _ = tokio::time::sleep_until(mode.deadline) => return Err("live round or recovery exceeded its explicit deadline".into()),
                    result = async { mode.gate.as_mut().expect("guarded resume gate").await }, if mode.gate.is_some() => {
                        let pause = result?;
                        if Instant::now() >= mode.deadline { return Err("resume Ready missed its fixed deadline".into()); }
                        mode.gate = None;
                        mode.signals = None;
                        // Different QUIC streams can deliver a legitimate post-Live Hit before local control completion
                        for batch in std::mem::take(&mut deferred_peer).chunks(64) {
                            accept_peer_batch(session, peer, batch, events)?;
                        }
                        if let Some((count, through)) = deferred_end.take() {
                            session.end_live(peer, count, through)?;
                        }
                        if Instant::now() >= mode.deadline { return Err("deferred peer facts exceeded the fixed recovery deadline".into()); }
                        mode.recovering = false;
                        mode.deadline = mode.running_deadline.checked_add(pause).ok_or("acknowledged pause deadline overflow")?;
                        emit(events, LiveEvent::RecoveryReady { epoch: session.epoch, attempt: sync::RESUME_ATTEMPT })?;
                    }
                    incoming = endpoint.accept(), if session.player == PlayerId::P1 && !continuation.used
                        && continuation.candidates < CANDIDATE_LIMIT && continuation.pending.is_empty()
                        && !session.ended.iter().any(|ended| *ended) => {
                        let incoming = incoming.ok_or("host endpoint stopped")?;
                        continuation.candidates += 1;
                        continuation.pending.spawn(authenticate_continuation(incoming, continuation.invitation.clone(),
                            session.prepared.identity.clone(), continuation.capability));
                    }
                    candidate = continuation.pending.join_next(), if !continuation.pending.is_empty() => {
                        if let Some(Ok(Ok(candidate))) = candidate {
                            if session.ended.iter().any(|ended| *ended) || continuation.used {
                                candidate.connection.close(1_u8.into(), b"continuation no longer eligible");
                            } else {
                                continuation.used = true;
                                return Ok(Some(candidate));
                            }
                        }
                        // Invalid candidates exhaust only the bounded accept budget, never the original round
                    }
                    command = commands.recv(), if !session.ended[session.player.index()] => {
                        match command.ok_or("live command sender stopped before End")? {
                            LiveCommand::Fact(input) => {
                                if mode.recovering && matches!(input, DuoInput::Hit(_)) {
                                    return Err("performing Hit is disabled while original audio catches up".into());
                                }
                                ingest_local(session, input)?;
                                tokio::time::timeout_at(mode.deadline, wire::send_live_input(send, written,
                                    &Input::Facts { epoch: session.epoch.0, facts: vec![Fact::from_input(input)] }))
                                    .await.map_err(|_| "fact write exceeded the round or recovery deadline")?
                                    .map_err(ExchangeError::from)?;
                            }
                            LiveCommand::End if !mode.recovering => {
                                let count = session.counts[session.player.index()] as u64;
                                session.end_live(session.player, count, session.prepared.final_through)?;
                                tokio::time::timeout_at(mode.deadline, wire::send_live_input(send, written,
                                    &Input::End { epoch: session.epoch.0, fact_count: count, final_through: session.prepared.final_through }))
                                    .await.map_err(|_| "End write exceeded the round deadline")?.map_err(ExchangeError::from)?;
                                send.finish().map_err(|_| "finish live input stream failed")?;
                            }
                            LiveCommand::RequestRecovery { epoch } if epoch == session.epoch && !mode.recovering && !continuation.used => {
                                return Err(ExchangeError::Recoverable("explicit local connection maintenance"));
                            }
                            LiveCommand::RecoveryArmed { epoch, attempt: sync::RESUME_ATTEMPT }
                                if epoch == session.epoch && mode.recovering => {
                                mode.signals.as_mut().and_then(|signals| signals.armed.take())
                                    .ok_or("duplicate or stale local resume Armed")?.send(())
                                    .map_err(|_| "resume arm consumer stopped")?;
                            }
                            LiveCommand::RecoveryObserved { epoch, attempt: sync::RESUME_ATTEMPT, evidence }
                                if epoch == session.epoch && mode.recovering => {
                                mode.signals.as_mut().and_then(|signals| signals.observed.take())
                                    .ok_or("duplicate or stale source evidence")?.send(evidence)
                                    .map_err(|_| "resume observation consumer stopped")?;
                            }
                            _ => return Err("unexpected live command or recovery generation".into()),
                        }
                        if session.ended.iter().all(|ended| *ended) { break None; }
                    }
                    _ = tokio::time::sleep_until(peer_progressed + session::IDLE), if !session.ended[peer.index()] && deferred_end.is_none() => return Err(ExchangeError::Recoverable("reliable peer progress deadline")),
                }
            }
        };
        if let Some(message) = message {
            match message {
                Input::Facts { epoch, facts } if epoch == session.epoch.0 => {
                    if mode.recovering
                        && (!deferred_peer.is_empty()
                            || facts.iter().any(|fact| matches!(fact, Fact::Hit { .. })))
                    {
                        if deferred_peer
                            .len()
                            .checked_add(facts.len())
                            .is_none_or(|count| {
                                count > EVENT_CAPACITY * 64
                                    || count > MAX_FACTS - session.replay.facts().len()
                            })
                        {
                            return Err(
                                "post-Live peer facts exceeded the bounded gate handoff queue"
                                    .into(),
                            );
                        }
                        deferred_peer.extend(facts);
                    } else {
                        accept_peer_batch(session, peer, &facts, events)?;
                    }
                }
                Input::End {
                    epoch,
                    fact_count,
                    final_through,
                } if epoch == session.epoch.0 => {
                    // Verify the real stream EOF before parking its End behind the fixed source gate
                    tokio::time::timeout_at(mode.deadline, wire::ensure_eof(recv))
                        .await
                        .map_err(|_| "peer End EOF exceeded the live or recovery deadline")??;
                    if mode.recovering {
                        if deferred_end.replace((fact_count, final_through)).is_some() {
                            return Err("duplicate peer End during gate handoff".into());
                        }
                    } else {
                        session.end_live(peer, fact_count, final_through)?;
                    }
                }
                _ => return Err("unexpected live input state, attempt or epoch".into()),
            }
            peer_progressed = Instant::now();
        }
    }
    Ok(None)
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
    fn recovery_eligibility_uses_typed_transport_and_deadline_not_diagnostic_text() {
        assert!(matches!(
            ExchangeError::from(wire::LiveIoError::Deadline),
            ExchangeError::Recoverable(_)
        ));
        for reason in [
            quinn::ConnectionError::TimedOut,
            quinn::ConnectionError::Reset,
        ] {
            assert!(matches!(
                ExchangeError::from(wire::LiveIoError::Transport(reason)),
                ExchangeError::Recoverable(_)
            ));
        }
        assert!(matches!(
            ExchangeError::from(wire::LiveIoError::Invalid("QUIC idle timeout".into())),
            ExchangeError::Terminal(_)
        ));
        assert!(matches!(
            ExchangeError::from(wire::LiveIoError::Transport(
                quinn::ConnectionError::LocallyClosed
            )),
            ExchangeError::Terminal(_)
        ));
    }

    #[test]
    fn recovery_publications_continue_the_acknowledged_progress() {
        let time = std::time::Instant::now();
        let publication = |sequence, frame, offset| RecoveryPublication {
            sequence,
            frame: SongTime::from_frames(frame),
            published_between: [time + Duration::from_millis(offset); 2],
        };
        let mut observed = RecoveryObserved {
            generation: 1,
            source_id: 1,
            progress: publication(100, 10_000, 10),
            publications: vec![publication(101, 10_000, 11), publication(102, 10_100, 12)],
        };
        check_publication_continuation(&observed).unwrap();
        observed.publications[0].frame = SongTime::from_frames(9_000);
        assert!(check_publication_continuation(&observed).is_err());
        observed.publications[0].frame = SongTime::from_frames(10_000);
        observed.publications[0].published_between = [time + Duration::from_millis(9); 2];
        assert!(check_publication_continuation(&observed).is_err());
    }

    #[test]
    fn recovery_terminal_drain_keeps_accepted_facts_after_unconsumed_control_signals() {
        let mut session = fixture();
        let (sender, mut commands) = mpsc::channel(COMMAND_CAPACITY);
        sender
            .try_send(LiveCommand::RecoveryArmed {
                epoch: session.epoch,
                attempt: 1,
            })
            .unwrap();
        let watermark = DuoInput::Watermark {
            epoch: session.epoch,
            player: PlayerId::P1,
            through: SongTime::from_frames(-2_400),
        };
        sender.try_send(LiveCommand::Fact(watermark)).unwrap();
        sender
            .try_send(LiveCommand::RequestRecovery {
                epoch: session.epoch,
            })
            .unwrap();
        let actual_hit = hit(session.epoch.0, PlayerId::P1, 0, 10_000);
        sender.try_send(LiveCommand::Fact(actual_hit)).unwrap();
        commands.close();
        assert!(sender.try_send(LiveCommand::Fact(actual_hit)).is_err());
        drain_accepted_commands(&mut session, &mut commands);
        assert_eq!(session.replay.facts(), &[watermark, actual_hit]);
        assert_eq!(session.next_seq, [1, 0]);
    }

    #[test]
    #[ignore = "explicit long QA package and real host loopback required"]
    fn authenticated_recovery_rejects_changed_prefix_sequence_watermark_and_order() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let package = PathBuf::from(
            std::env::var("COCOBEAT_RECOVERY_QA_PACKAGE").expect("declare validated QA package"),
        );
        let output = PathBuf::from(
            std::env::var("COCOBEAT_RECOVERY_QA_OUTPUT").expect("declare new evidence output"),
        );
        fs::create_dir(&output).unwrap();
        let original = vec![
            Fact::Hit { seq: 0, frame: 100 },
            Fact::Watermark { through: 100 },
        ];
        for case in [
            "changed-prefix",
            "duplicate-seq",
            "backward-watermark",
            "reordered",
        ] {
            let directory = output.join(case);
            fs::create_dir(&directory).unwrap();
            let invite_path = directory.join("invite.json");
            let host_output = directory.join("host");
            let mut host = LiveSession::spawn(LiveConfig {
                role: LiveRole::Host {
                    package: package.clone(),
                    bind: "127.0.0.1:0".parse().unwrap(),
                    invite: invite_path.clone(),
                },
                output: host_output.clone(),
            })
            .unwrap();
            let peer_count = Arc::new(AtomicUsize::new(0));
            let count_for_ui = peer_count.clone();
            let ui = thread::spawn(move || {
                let mut replay = None;
                let limit = std::time::Instant::now() + Duration::from_secs(20);
                let failure = loop {
                    if std::time::Instant::now() >= limit {
                        host.cancel();
                        panic!("bounded malicious-peer UI timeout");
                    }
                    match host.try_recv() {
                        Ok(LiveEvent::Listening { .. }) => {}
                        Ok(LiveEvent::Prepared {
                            epoch,
                            content_id,
                            stage_compiler_version,
                            ..
                        }) => {
                            replay = Some(
                                cocobeat_replay::Replay::new(
                                    cocobeat_replay::ReplayIdentity {
                                        content_id,
                                        rules_id: RULESET.into(),
                                        build_id: "authenticated-negative-QA".into(),
                                        stage_compiler_version: Some(stage_compiler_version),
                                    },
                                    epoch,
                                )
                                .unwrap(),
                            );
                            host.try_send(LiveCommand::Ready).unwrap();
                        }
                        Ok(LiveEvent::Scheduled { .. }) => {
                            host.try_send(LiveCommand::Armed).unwrap()
                        }
                        Ok(LiveEvent::Started { .. }) => {}
                        Ok(LiveEvent::PeerFacts(facts)) => {
                            for fact in facts {
                                replay.as_mut().unwrap().record(fact).unwrap();
                                count_for_ui.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                        Ok(LiveEvent::RecoveryPausing { epoch, attempt }) => {
                            host.try_send(LiveCommand::RecoveryFrozen {
                                epoch,
                                attempt,
                                snapshot: Box::new(RecoveryFrozen {
                                    replay: replay.as_ref().unwrap().clone(),
                                    paused_frame: SongTime::from_frames(10_000),
                                    source_generation: 1,
                                    source_id: 1,
                                    paused_at: std::time::Instant::now(),
                                }),
                            })
                            .unwrap();
                        }
                        Ok(LiveEvent::Failed(error)) => break error,
                        Ok(other) => panic!("invalid owner tape reached later phase: {other:?}"),
                        Err(std_mpsc::TryRecvError::Empty) => {
                            thread::sleep(Duration::from_millis(1))
                        }
                        Err(error) => panic!("worker stopped without terminal event: {error}"),
                    }
                };
                let limit = std::time::Instant::now() + Duration::from_secs(3);
                while !host.is_finished() {
                    assert!(std::time::Instant::now() < limit);
                    thread::sleep(Duration::from_millis(1));
                }
                failure
            });
            let wait = std::time::Instant::now() + Duration::from_secs(3);
            while !invite_path.exists() {
                assert!(std::time::Instant::now() < wait);
                thread::sleep(Duration::from_millis(1));
            }
            let invitation = read_invite(&invite_path).unwrap();
            let mut tape = original.clone();
            match case {
                "changed-prefix" => tape[0] = Fact::Hit { seq: 0, frame: 101 },
                "duplicate-seq" => tape.push(Fact::Hit { seq: 0, frame: 200 }),
                "backward-watermark" => tape.push(Fact::Watermark { through: 99 }),
                "reordered" => tape.swap(0, 1),
                _ => unreachable!(),
            }
            let runtime = session::runtime().unwrap();
            let (epoch, snapshot_bytes) = runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(15), async {
                    let mut owned_endpoint = None;
                    let (_, connection) = connect_owned(&invitation, &mut owned_endpoint).await.unwrap();
                    let mut identity = session::prepare_package(cocobeat_media::validate_package(&package).unwrap()).unwrap().identity;
                    identity.stage_compiler_version = Some(cocobeat_schema::STAGE_COMPILER_VERSION);
                    let epoch = SessionEpoch(invitation.epoch);
                    let origin = Instant::now();
                    let mut control = ControlIo::new(connection.open_bi().await.unwrap());
                    control.send(Control::LiveHello { protocol_version: PROTOCOL_VERSION, epoch: epoch.0, player: 2, token: invitation.token, identity: identity.clone() }).await.unwrap();
                    let capability = match control.recv().await.unwrap() {
                        Control::LiveWelcome { capability, identity: actual, .. } if actual == identity => capability,
                        _ => panic!("initial authenticated welcome differs"),
                    };
                    let mut inputs = session::open_inputs(&connection, PlayerId::P2, epoch.0).await.unwrap();
                    let mut timing = NetworkTiming::default();
                    let start = sync::arm_start(&connection, &mut control, epoch, PlayerId::P2, origin, &mut timing).await.unwrap();
                    let (sender, mut commands) = mpsc::channel(1);
                    sender.try_send(LiveCommand::Armed).unwrap();
                    arm_barrier(&connection, &mut control, &mut commands, epoch, PlayerId::P2, start, timing.start_uncertainty_ns.unwrap()).await.unwrap();
                    sync::wait_start(&connection, start, origin, &mut timing).await.unwrap();
                    wire::send_input(&mut inputs.send, &mut inputs.written, &Input::Facts { epoch: epoch.0, facts: original.clone() }).await.unwrap();
                    while peer_count.load(Ordering::SeqCst) != original.len() { tokio::time::sleep(Duration::from_millis(1)).await; }
                    connection.close(RECOVERY_REQUESTED.into(), b"QA controlled rebuild");
                    let (_, resumed) = connect_owned(&invitation, &mut owned_endpoint).await.unwrap();
                    let mut control = ControlIo::new(resumed.open_bi().await.unwrap());
                    control.send(Control::ResumeHello { protocol_version: PROTOCOL_VERSION, epoch: epoch.0, player: 2, identity, attempt: 1, pause_frame: 10_000, source_generation: 2, source_id: 2, owner_count: tape.len() as u64, started: true, ended: false, capability }).await.unwrap();
                    assert!(matches!(control.recv().await.unwrap(), Control::ResumeWelcome { epoch: actual, attempt: 1, owner_count: 0, .. } if actual == epoch.0), "negative tape must reach authenticated continuation");
                    let names = ["worker-prefix.replay.json", "gui-prefix.replay.json", "metadata.json"];
                    let snapshot_bytes = names.map(|name| fs::read(host_output.join("recovery-1").join(name)).unwrap());
                    let (mut send, mut recv) = resumed.open_bi().await.unwrap();
                    let mut written = 0;
                    let mut read = 0;
                    wire::send_input(&mut send, &mut written, &Input::ResumeOpen { epoch: epoch.0, attempt: 1, player: 2, count: tape.len() as u64 }).await.unwrap();
                    assert_eq!(wire::recv_input(&mut recv, &mut read).await.unwrap(), Input::ResumeOpen { epoch: epoch.0, attempt: 1, player: 1, count: 0 });
                    tokio::try_join!(send_owner_tape(&mut send, &mut written, epoch, &tape), receive_owner_tape(&mut recv, &mut read, epoch, 0)).unwrap();
                    let _ = resumed.closed().await;
                    (epoch, snapshot_bytes)
                }).await.expect("malicious continuation bounded by real protocol deadline")
            });
            let failure = ui.join().unwrap();
            let replay =
                cocobeat_replay::Replay::load(host_output.join("live.replay.json")).unwrap();
            assert_eq!(replay.epoch(), epoch);
            assert_eq!(
                replay.facts(),
                original
                    .iter()
                    .map(|fact| fact.into_input(epoch, PlayerId::P2))
                    .collect::<Vec<_>>()
            );
            for (name, bytes) in [
                "worker-prefix.replay.json",
                "gui-prefix.replay.json",
                "metadata.json",
            ]
            .iter()
            .zip(snapshot_bytes)
            {
                assert_eq!(
                    fs::read(host_output.join("recovery-1").join(name)).unwrap(),
                    bytes
                );
            }
            assert!(!host_output.join("authority.replay.json").exists());
            println!(
                "{}",
                serde_json::json!({"scenario":case,"authenticated_continuation":true,"status":"PASS","failure":failure,"owner_prefix_unchanged":true,"snapshots_unchanged":true,"owned_worker_finished":true,"scope":"real production Host LiveSession with independent authenticated malformed owner-tape peer; software loopback only"})
            );
        }
    }

    #[test]
    #[ignore = "explicit real host loopback required"]
    fn recovery_gate_parks_real_peer_end_and_eof_after_fifo_facts() {
        session::runtime().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let (host_endpoint, invitation) = listen("127.0.0.1:0".parse().unwrap()).unwrap();
                let mut owned_endpoint = None;
                let guest = connect_owned(&invitation, &mut owned_endpoint);
                let host = async { host_endpoint.accept().await.unwrap().await.unwrap() };
                let (guest, host_connection) = tokio::join!(guest, host);
                let (_, guest_connection) = guest.unwrap();
                let (host_inputs, guest_inputs) = tokio::join!(session::open_inputs(&host_connection, PlayerId::P1, 9), session::open_inputs(&guest_connection, PlayerId::P2, 9));
                let mut host_inputs = host_inputs.unwrap();
                let mut guest_inputs = guest_inputs.unwrap();
                let mut state = fixture();
                let (sender, mut commands) = mpsc::channel(COMMAND_CAPACITY);
                let (events, receiver) = std_mpsc::sync_channel(EVENT_CAPACITY);
                let (gate_sender, gate_receiver) = oneshot::channel();
                let mut mode = ExchangeControl { gate: Some(Box::pin(async { gate_receiver.await.map_err(|_| "test gate cancelled".to_owned()) })), signals: None, deadline: Instant::now() + Duration::from_secs(4), running_deadline: Instant::now() + Duration::from_secs(4), recovering: true };
                let mut continuation = Continuation { invitation, capability: [0;32], used: true, candidates: 0, pending: JoinSet::new() };
                let exchange = exchange(&mut state, (&host_connection, &host_endpoint), &mut host_inputs, &mut commands, &events, &mut continuation, &mut mode);
                let peer = async {
                    wire::send_live_input(&mut guest_inputs.send, &mut guest_inputs.written, &Input::Facts { epoch: 9, facts: vec![Fact::Watermark { through: -2400 }] }).await.unwrap();
                    let first = loop { if let Ok(event) = receiver.try_recv() { break event; } tokio::time::sleep(Duration::from_millis(1)).await; };
                    assert!(matches!(first, LiveEvent::PeerFacts(facts) if facts == [DuoInput::Watermark { epoch: SessionEpoch(9), player: PlayerId::P2, through: SongTime::from_frames(-2400) }]));
                    let facts = vec![Fact::Hit { seq: 0, frame: 10_000 }, Fact::Watermark { through: 68_881 }];
                    wire::send_live_input(&mut guest_inputs.send, &mut guest_inputs.written, &Input::Facts { epoch: 9, facts }).await.unwrap();
                    wire::send_live_input(&mut guest_inputs.send, &mut guest_inputs.written, &Input::End { epoch: 9, fact_count: 3, final_through: 68_881 }).await.unwrap();
                    guest_inputs.send.finish().unwrap();
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    assert!(matches!(receiver.try_recv(), Err(std_mpsc::TryRecvError::Empty)), "post-Live peer facts/End remain behind the pending local gate");
                    gate_sender.send(Duration::from_millis(10)).unwrap();
                    let mut actual = Vec::new();
                    loop {
                        match receiver.try_recv() {
                            Ok(LiveEvent::PeerFacts(facts)) => actual.extend(facts),
                            Ok(LiveEvent::RecoveryReady { epoch: SessionEpoch(9), attempt: 1 }) => break,
                            Ok(other) => panic!("unexpected event during gate drain: {other:?}"),
                            Err(std_mpsc::TryRecvError::Empty) => tokio::time::sleep(Duration::from_millis(1)).await,
                            Err(error) => panic!("gate event sender disappeared: {error}"),
                        }
                    }
                    assert_eq!(actual, [hit(9, PlayerId::P2, 0, 10_000), DuoInput::Watermark { epoch: SessionEpoch(9), player: PlayerId::P2, through: SongTime::from_frames(68_881) }]);
                    sender.try_send(LiveCommand::Fact(DuoInput::Watermark { epoch: SessionEpoch(9), player: PlayerId::P1, through: SongTime::from_frames(68_881) })).unwrap();
                    sender.try_send(LiveCommand::End).unwrap();
                };
                let (result, ()) = tokio::join!(exchange, peer);
                assert!(matches!(result, Ok(None)));
                assert_eq!(state.ended, [true; 2]);
                assert_eq!(state.counts, [1, 3]);
                assert_eq!(state.next_seq, [0, 1]);
                state.verify_replay(&state.replay).unwrap();
                host_endpoint.close(0u32.into(), b"QA complete");
                if let Some(endpoint) = owned_endpoint { endpoint.close(0u32.into(), b"QA complete"); }
            }).await.expect("bounded real End/FIN gate handoff");
        });
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
