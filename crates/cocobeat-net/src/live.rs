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
    pub publication: PhasePublication,
    /// Earlier unfinished Phase pause ACK to actual Playing progress ACK, disjoint from this pause
    pub prior_phase_pause: Option<[std::time::Instant; 2]>,
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

#[derive(Clone, Copy, Debug)]
pub struct PhasePublication {
    pub sequence: u64,
    pub position_seconds_bits: u64,
    pub published_between: [std::time::Instant; 2],
}

#[derive(Clone, Debug)]
pub struct PhaseObserved {
    pub generation: u64,
    pub source_id: u64,
    pub collected_at: std::time::Instant,
    pub publications: Vec<PhasePublication>,
}

#[derive(Clone, Debug)]
pub struct PhaseFrozen {
    pub replay: cocobeat_replay::Replay,
    pub paused_frame: SongTime,
    pub source_generation: u64,
    pub source_id: u64,
    pub paused_at: std::time::Instant,
    pub publication: PhasePublication,
}

#[derive(Clone, Debug)]
pub enum LiveCommand {
    Ready,
    Armed,
    PhaseSource {
        epoch: SessionEpoch,
        generation: u64,
        source_id: u64,
        publication: PhasePublication,
    },
    PhaseObserved {
        epoch: SessionEpoch,
        round: u16,
        attempt: u8,
        verification: bool,
        evidence: PhaseObserved,
    },
    PhaseFrozen {
        epoch: SessionEpoch,
        round: u16,
        attempt: u8,
        snapshot: Box<PhaseFrozen>,
    },
    PhaseArmed {
        epoch: SessionEpoch,
        round: u16,
        attempt: u8,
    },
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
    /// Actual active-connection four-timestamp samples, independent of audio phase
    ClockMaintained {
        epoch: SessionEpoch,
        round: u64,
        exchange: crate::clock::ClockExchange,
    },
    PhaseSampling {
        epoch: SessionEpoch,
        round: u16,
        attempt: u8,
        verification: bool,
        not_before: std::time::Instant,
        common_at: std::time::Instant,
        until: std::time::Instant,
    },
    PhasePausing {
        epoch: SessionEpoch,
        round: u16,
        attempt: u8,
    },
    PhaseScheduled {
        epoch: SessionEpoch,
        round: u16,
        attempt: u8,
        deadline: std::time::Instant,
        verify_at: std::time::Instant,
        common_frame: SongTime,
        timing: NetworkTiming,
    },
    PhaseReady {
        epoch: SessionEpoch,
        round: u16,
        attempt: u8,
    },
    PhaseRebound {
        epoch: SessionEpoch,
        round: u16,
        attempt: u8,
        deadline: std::time::Instant,
    },
    PeerFacts(Vec<DuoInput>),
    RecoveryPausing {
        epoch: SessionEpoch,
        attempt: u8,
        deadline: std::time::Instant,
        phase_round: Option<u16>,
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
        phase_round: Option<u16>,
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
        maintenance_round: 0,
        maintenance: None,
        phase: PhaseState::default(),
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
            maintenance_round: 0,
            maintenance: None,
            phase: PhaseState::default(),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PhaseStep {
    Sampling,
    GateAck,
    Pausing,
    ScheduleAck,
    Arming,
    VerifyClock,
    ReconnectClock,
    AwaitLive,
}

struct PhaseRound {
    round: u16,
    attempt: u8,
    reconnecting: bool,
    host_deadline_ns: u64,
    start_uncertainties_ns: [u64; 2],
    step: PhaseStep,
    anchor: wire::PhaseAnchor,
    host_point_ns: u64,
    verification: bool,
    deadline: Instant,
    own: Option<wire::PhaseEvidence>,
    peer: Option<wire::PhaseEvidence>,
    own_frozen: Option<Box<PhaseFrozen>>,
    frozen: [Option<(i64, wire::PhasePublication, u64)>; 2],
    markers: [Option<u64>; 2],
    common_frame: Option<i64>,
    host_common_ns: Option<u64>,
    host_verify_ns: Option<u64>,
    own_armed: bool,
    peer_armed: bool,
    peer_confirmed: bool,
    own_confirmed: bool,
    plan: Option<sync::ResumePlan>,
    bounds: Option<sync::PhaseBounds>,
    pending_schedule: Option<wire::PhaseControl>,
}

#[derive(Default)]
struct PhaseState {
    sources: [Option<(u64, u64)>; 2],
    previous: [Option<wire::PhasePublication>; 2],
    round: u16,
    corrections: u8,
    rounds: sync::PhaseRounds,
    active: Option<PhaseRound>,
    sealed: Option<wire::PhaseResume>,
    attempt: u8,
    next_check_frame: Option<i64>,
    progress_frame: i64,
    sent: wire::PhaseBudget,
    received: wire::PhaseBudget,
    peer_end_count: Option<u64>,
    deferred_peer: Vec<Fact>,
    deferred_end: Option<(u64, i64)>,
}

fn phase_reader(control: &mut ControlIo, phase: &mut PhaseState) -> Result<PhaseRead, String> {
    let mut stream = control
        .recv
        .take()
        .ok_or("duplicate phase control reader")?;
    let mut budget = phase.received.clone();
    // The shadow retains a maximum pending frame if the old connection is abandoned
    phase.received.pending_read()?;
    Ok(Box::pin(async move {
        let message = wire::recv_phase_control(&mut stream, &mut budget).await;
        (stream, budget, message)
    }))
}

fn phase_anchor(sample: sync::MaintainedClock) -> wire::PhaseAnchor {
    let exchange = sample.exchange;
    wire::PhaseAnchor {
        clock_round: sample.round,
        guest_send_ns: exchange.guest_send_ns,
        host_receive_ns: exchange.host_receive_ns,
        host_send_ns: exchange.host_send_ns,
        guest_receive_ns: exchange.guest_receive_ns,
    }
}

fn phase_sampling_event(
    session: &Session,
    round: &PhaseRound,
    sample: sync::MaintainedClock,
) -> Result<LiveEvent, String> {
    if phase_anchor(sample) != round.anchor {
        return Err("phase sampling anchor differs from the actual maintained exchange".into());
    }
    let point = if session.player == PlayerId::P1 {
        round.host_point_ns
    } else {
        u64::try_from(i128::from(round.host_point_ns) - i128::from(sample.estimate.offset_ns))
            .map_err(|_| "phase sampling center precedes local origin")?
    };
    let begin = point
        .checked_sub(40_000_000)
        .ok_or("phase sampling start underflow")?;
    let until = point
        .checked_add(80_000_000)
        .ok_or("phase sampling end overflow")?;
    let anchor = if session.player == PlayerId::P1 {
        sample.exchange.host_send_ns
    } else {
        sample.exchange.guest_receive_ns
    };
    if begin < anchor || begin <= sync::now_ns(session.origin)? {
        return Err(
            "phase sampling window is not entirely future and after its actual anchor".into(),
        );
    }
    if sync::local_instant(session.origin, until)? >= round.deadline {
        return Err("phase sampling tail crosses its fixed deadline".into());
    }
    Ok(LiveEvent::PhaseSampling {
        epoch: session.epoch,
        round: round.round,
        attempt: round.attempt,
        verification: round.verification,
        not_before: sync::local_instant(session.origin, begin)?.into_std(),
        common_at: sync::local_instant(session.origin, point)?.into_std(),
        until: sync::local_instant(session.origin, until)?.into_std(),
    })
}

async fn phase_send(
    session: &Session,
    control: &mut ControlIo,
    phase: &mut PhaseState,
    round: u16,
    deadline: Instant,
    message: wire::PhaseControl,
) -> Result<(), ExchangeError> {
    tokio::time::timeout_at(
        deadline,
        wire::send_phase_control(
            &mut control.send,
            &mut phase.sent,
            &Control::Phase {
                epoch: session.epoch.0,
                round,
                attempt: phase.attempt,
                message,
            },
        ),
    )
    .await
    .map_err(|_| "phase control write exceeded its fixed deadline")?
    .map_err(ExchangeError::from)
}

fn new_phase_round(
    round: u16,
    anchor: wire::PhaseAnchor,
    point: u64,
    deadline: Instant,
) -> PhaseRound {
    PhaseRound {
        round,
        attempt: 0,
        reconnecting: false,
        host_deadline_ns: 0,
        start_uncertainties_ns: [0; 2],
        step: PhaseStep::Sampling,
        anchor,
        host_point_ns: point,
        verification: false,
        deadline,
        own: None,
        peer: None,
        own_frozen: None,
        frozen: [None; 2],
        markers: [None; 2],
        common_frame: None,
        host_common_ns: None,
        host_verify_ns: None,
        own_armed: false,
        peer_armed: false,
        peer_confirmed: false,
        own_confirmed: false,
        plan: None,
        bounds: None,
        pending_schedule: None,
    }
}

async fn phase_clock_progress(
    session: &Session,
    control: &mut ControlIo,
    events: &std_mpsc::SyncSender<LiveEvent>,
    phase: &mut PhaseState,
    maintained: &mut sync::ClockMaintenance,
) -> Result<(), ExchangeError> {
    if session.ended.iter().any(|ended| *ended) || phase.peer_end_count.is_some() {
        return Ok(());
    }
    let sample = maintained
        .sample()
        .ok_or("phase clock progress lacks an actual sample")?;
    if session.player == PlayerId::P1
        && phase.active.is_none()
        && phase.sources.iter().all(Option::is_some)
        && phase
            .next_check_frame
            .is_some_and(|frame| phase.progress_frame >= frame)
        && phase.progress_frame + 48_000 < session.prepared.end
    {
        let round = phase
            .round
            .checked_add(1)
            .filter(|round| *round <= sync::MAX_PHASE_ROUNDS)
            .ok_or("phase round budget exhausted")?;
        let now = sync::now_ns(session.origin)?;
        let point = now
            .checked_add(250_000_000)
            .ok_or("phase common point overflow")?;
        let host_deadline_ns = now
            .checked_add(30_000_000_000)
            .ok_or("phase deadline overflow")?;
        let deadline = sync::local_instant(session.origin, host_deadline_ns)?;
        phase.rounds.begin(round, session.epoch, point, now)?;
        phase.round = round;
        phase.active = Some(new_phase_round(
            round,
            phase_anchor(sample),
            point,
            deadline,
        ));
        phase.active.as_mut().unwrap().host_deadline_ns = host_deadline_ns;
        phase.active.as_mut().unwrap().attempt = phase.attempt;
        phase_send(
            session,
            control,
            phase,
            round,
            deadline,
            wire::PhaseControl::Check {
                anchor: phase_anchor(sample),
                host_point_ns: point,
                host_deadline_ns,
            },
        )
        .await?;
        emit(
            events,
            phase_sampling_event(session, phase.active.as_ref().unwrap(), sample)?,
        )?;
    } else if session.player == PlayerId::P1
        && phase
            .active
            .as_ref()
            .is_some_and(|round| round.step == PhaseStep::ReconnectClock)
        && sample.round > phase.active.as_ref().unwrap().anchor.clock_round
    {
        let now = sync::now_ns(session.origin)?;
        let point = now
            .checked_add(250_000_000)
            .ok_or("reconnected common point overflow")?;
        let active = phase.active.as_mut().unwrap();
        phase.rounds.rebind(
            active.round,
            session.epoch,
            point,
            active.host_deadline_ns,
            now,
            phase.corrections,
        )?;
        active.anchor = phase_anchor(sample);
        active.host_point_ns = point;
        active.verification = true;
        active.step = PhaseStep::Sampling;
        let event = phase_sampling_event(session, active, sample)?;
        let (round, deadline, anchor) = (active.round, active.deadline, active.anchor);
        phase_send(
            session,
            control,
            phase,
            round,
            deadline,
            wire::PhaseControl::Verify {
                anchor,
                host_point_ns: point,
            },
        )
        .await?;
        emit(events, event)?;
    } else if session.player == PlayerId::P1
        && phase
            .active
            .as_ref()
            .is_some_and(|round| round.step == PhaseStep::VerifyClock)
    {
        let active = phase.active.as_mut().unwrap();
        let point = active
            .host_verify_ns
            .ok_or("phase verification point missing")?;
        let now = sync::now_ns(session.origin)?;
        if point
            .checked_sub(now)
            .is_some_and(|remaining| (100_000_000..=600_000_000).contains(&remaining))
            && sample.round > active.anchor.clock_round
        {
            active.anchor = phase_anchor(sample);
            active.host_point_ns = point;
            active.verification = true;
            active.own = None;
            active.peer = None;
            active.bounds = None;
            active.step = PhaseStep::Sampling;
            let event = phase_sampling_event(session, active, sample)?;
            let (round, deadline, anchor) = (active.round, active.deadline, active.anchor);
            phase_send(
                session,
                control,
                phase,
                round,
                deadline,
                wire::PhaseControl::Verify {
                    anchor,
                    host_point_ns: point,
                },
            )
            .await?;
            emit(events, event)?;
        }
    }
    Ok(())
}

async fn phase_try_gate(
    session: &Session,
    control: &mut ControlIo,
    phase: &mut PhaseState,
    maintained: &mut sync::ClockMaintenance,
) -> Result<(), ExchangeError> {
    if session.player != PlayerId::P1 {
        return Ok(());
    }
    let Some(active) = phase.active.as_mut() else {
        return Ok(());
    };
    if active.step != PhaseStep::Sampling || active.own.is_none() || active.peer.is_none() {
        return Ok(());
    }
    maintained
        .check_local_time(sync::now_ns(session.origin)?)
        .map_err(|error| error.to_string())?;
    let evidence = [
        active.own.as_ref().unwrap().clone(),
        active.peer.as_ref().unwrap().clone(),
    ];
    let host_received_ns = sync::now_ns(session.origin)?;
    let bounds = sync::source_phase_bounds(
        &mut maintained.clock,
        active.anchor.exchange(session.epoch.0),
        &evidence,
        sync::PhaseWindow {
            epoch: session.epoch,
            round: active.round,
            verification: active.verification,
            attempt: active.attempt,
            reconnecting: active.reconnecting,
            sources: phase
                .sources
                .map(|source| source.expect("bound phase sources")),
            previous: phase.previous,
            end: session.prepared.end,
            host_point_ns: active.host_point_ns,
            host_now_ns: host_received_ns,
        },
    )?;
    if active.reconnecting {
        bounds.verify_reconnect_resume(
            active
                .frozen
                .map(|frozen| frozen.expect("reconnect pause acknowledgments").1),
            active
                .common_frame
                .ok_or("missing reconnect common frame")?,
            active
                .host_common_ns
                .ok_or("missing reconnect common timestamp")?,
            active.start_uncertainties_ns,
        )?;
    } else if active.verification {
        bounds.verify_original_resume(
            active
                .frozen
                .map(|frozen| frozen.expect("acknowledged pause").1),
            active.common_frame.ok_or("missing phase common frame")?,
        )?;
    }
    session.record_phase(active.round, active.verification, active.reconnecting, serde_json::json!({
        "epoch": session.epoch.0, "round": active.round, "verification": active.verification,
        "connection_attempt": active.attempt, "reconnect_verification": active.reconnecting,
        "player": session.player.index() + 1, "actual_anchor": active.anchor,
        "host_point_ns": active.host_point_ns, "host_received_ns": host_received_ns,
        "own_receipt_ns": sync::now_ns(session.origin)?, "source_evidence": evidence,
        "source_intervals_frames": bounds.source_frames(), "guest_minus_host_frames": bounds.difference(),
        "within_guard": bounds.within_guard(), "common_frame": active.common_frame, "host_common_ns": active.host_common_ns,
        "start_uncertainties_ns": active.start_uncertainties_ns,
        "original_source_ids": phase.sources, "previous_publications": phase.previous,
        "frozen_publications": active.frozen.map(|frozen| frozen.map(|value| value.1)),
        "marker_owner_counts": active.markers,
    }))?;
    if !bounds.supports_guard() {
        return Err(
            "source phase publication uncertainty is too wide for the unchanged guard".into(),
        );
    }
    let correcting = !bounds.within_guard();
    active.bounds = Some(bounds);
    active.step = PhaseStep::GateAck;
    let (round, deadline, anchor, point, verification) = (
        active.round,
        active.deadline,
        active.anchor,
        active.host_point_ns,
        active.verification,
    );
    phase_send(
        session,
        control,
        phase,
        round,
        deadline,
        wire::PhaseControl::Gate {
            anchor,
            host_point_ns: point,
            verification,
            host_received_ns,
            correcting,
        },
    )
    .await
}

async fn phase_try_schedule(
    session: &Session,
    control: &mut ControlIo,
    phase: &mut PhaseState,
    maintained: &mut sync::ClockMaintenance,
) -> Result<(), ExchangeError> {
    if session.player != PlayerId::P1 {
        return Ok(());
    }
    let Some(active) = phase.active.as_mut() else {
        return Ok(());
    };
    if active.step != PhaseStep::Pausing
        || active.frozen.iter().any(Option::is_none)
        || active.markers.iter().any(Option::is_none)
    {
        return Ok(());
    }
    for index in 0..2 {
        if active.frozen[index].unwrap().2 != active.markers[index].unwrap() {
            return Err(
                "phase frozen owner count differs from its ordered pre-pause prefix".into(),
            );
        }
    }
    maintained
        .check_local_time(sync::now_ns(session.origin)?)
        .map_err(|error| error.to_string())?;
    phase.previous = active.frozen.map(|frozen| Some(frozen.unwrap().1));
    let paused = active.frozen.map(|frozen| frozen.unwrap().0);
    let common_frame = *paused.iter().max().unwrap();
    if paused
        .iter()
        .any(|frame| !(0..session.prepared.end).contains(frame))
        || common_frame
            .checked_add(4_800)
            .is_none_or(|frame| frame >= session.prepared.end)
    {
        return Err("phase correction needs unfinished original source extent".into());
    }
    let delays = paused
        .map(|frame| sync::catchup_ns(common_frame, frame))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let sample = maintained.sample().ok_or("missing phase schedule clock")?;
    let now = sync::now_ns(session.origin)?;
    let earliest_verify = now
        .checked_add(2_100_000_000)
        .and_then(|time| time.checked_add(*delays.iter().max()?))
        .ok_or("phase resume common time overflow")?;
    // Leave the 100 ms source horizon after a future CBMC cadence, rather than across it
    let since = earliest_verify
        .checked_sub(sample.exchange.host_receive_ns)
        .ok_or("phase schedule predates its sample")?;
    let beats = since.div_ceil(1_000_000_000);
    let verify = sample
        .exchange
        .host_receive_ns
        .checked_add(
            beats
                .checked_mul(1_000_000_000)
                .ok_or("phase cadence overflow")?,
        )
        .and_then(|time| time.checked_add(250_000_000))
        .ok_or("phase verification time overflow")?;
    let common = verify
        .checked_sub(100_000_000)
        .ok_or("phase resume time underflow")?;
    phase.rounds.correct(active.round, verify, now)?;
    phase.corrections = phase
        .corrections
        .checked_add(1)
        .filter(|count| *count <= sync::MAX_PHASE_CORRECTIONS)
        .ok_or("phase correction budget exhausted")?;
    active.anchor = phase_anchor(sample);
    active.common_frame = Some(common_frame);
    active.host_common_ns = Some(common);
    active.host_verify_ns = Some(verify);
    active.step = PhaseStep::ScheduleAck;
    let local_resume = common
        .checked_sub(delays[0])
        .ok_or("phase host resume underflow")?;
    active.plan = Some(sync::ResumePlan {
        deadline: sync::local_instant(session.origin, local_resume)?,
        verify_at: sync::local_instant(session.origin, verify)?,
        host_verify_ns: verify,
        common_frame,
        start_uncertainties_ns: [0; 2],
        phase_deadline: None,
        timing: phase_timing(sample, common, local_resume, 0),
    });
    let (round, deadline, anchor) = (active.round, active.deadline, active.anchor);
    phase_send(
        session,
        control,
        phase,
        round,
        deadline,
        wire::PhaseControl::Schedule {
            anchor,
            host_common_ns: common,
            host_verify_ns: verify,
            common_frame,
            paused,
        },
    )
    .await?;
    // Runtime scheduling is emitted only after the peer validates the whole clock interval
    Ok(())
}

fn phase_timing(
    sample: sync::MaintainedClock,
    host_common: u64,
    local_resume: u64,
    uncertainty: u64,
) -> NetworkTiming {
    NetworkTiming {
        clock: Some(sync::ClockSample {
            probe_id: sample.round,
            guest_sample_ns: sample.exchange.guest_receive_ns,
            offset_ns: sample.estimate.offset_ns,
            uncertainty_ns: sample.estimate.uncertainty_ns,
            round_trip_min_ns: sample.estimate.round_trip_min_ns,
            round_trip_max_ns: sample.estimate.round_trip_max_ns,
        }),
        host_start_ns: Some(host_common),
        local_start_ns: Some(local_resume),
        start_uncertainty_ns: Some(uncertainty),
        ..NetworkTiming::default()
    }
}

fn phase_scheduled(session: &Session, active: &PhaseRound) -> Result<LiveEvent, String> {
    let plan = active.plan.as_ref().ok_or("phase schedule is absent")?;
    if Instant::now()
        .checked_add(
            MINIMUM_ARM_LEAD + Duration::from_nanos(plan.timing.start_uncertainty_ns.unwrap_or(0)),
        )
        .is_none_or(|ready| ready >= plan.deadline)
    {
        return Err("phase original source scheduling lost its full minimum lead".into());
    }
    Ok(LiveEvent::PhaseScheduled {
        epoch: session.epoch,
        round: active.round,
        attempt: active.attempt,
        deadline: plan.deadline.into_std(),
        verify_at: plan.verify_at.into_std(),
        common_frame: SongTime::from_frames(plan.common_frame),
        timing: plan.timing.clone(),
    })
}

async fn phase_try_confirm(
    session: &Session,
    control: &mut ControlIo,
    phase: &mut PhaseState,
) -> Result<(), ExchangeError> {
    let Some(active) = phase.active.as_mut() else {
        return Ok(());
    };
    if active.step == PhaseStep::Arming
        && active.own_armed
        && active.peer_armed
        && !active.own_confirmed
    {
        active.own_confirmed = true;
        let (round, deadline) = (active.round, active.deadline);
        phase_send(
            session,
            control,
            phase,
            round,
            deadline,
            wire::PhaseControl::Confirmed {},
        )
        .await?;
    }
    let active = phase.active.as_mut().unwrap();
    if active.step == PhaseStep::Arming && active.own_confirmed && active.peer_confirmed {
        let event = phase_scheduled(session, active)?;
        // This recheck validates remaining lead, but the source was already scheduled once
        let _ = event;
        active.step = PhaseStep::VerifyClock;
    }
    Ok(())
}

async fn phase_finish(
    session: &mut Session,
    events: &std_mpsc::SyncSender<LiveEvent>,
    phase: &mut PhaseState,
    mode: &mut ExchangeControl<'_>,
) -> Result<(), String> {
    let active = phase
        .active
        .take()
        .ok_or("phase completion has no active round")?;
    let bounds = active
        .bounds
        .ok_or("phase completion lacks an original-source proof")?;
    if !bounds.within_guard()
        || session.ended.iter().any(|ended| *ended)
        || phase.peer_end_count.is_some()
    {
        return Err("phase cannot reopen ended sources or unverified positions".into());
    }
    if session.player == PlayerId::P1 {
        phase
            .rounds
            .complete(active.round, sync::now_ns(session.origin)?, bounds)?;
    }
    phase.sealed = if active.reconnecting {
        None
    } else {
        Some(phase_description(phase, &active, true)?)
    };
    phase.previous = bounds.last_publications().map(Some);
    phase.next_check_frame = bounds.source_frames()[0][1].checked_add(240_000);
    if active.verification && !active.reconnecting {
        let frozen = active
            .own_frozen
            .ok_or("phase completion lost the actual pause acknowledgment")?;
        let held = std::time::Instant::now()
            .checked_duration_since(frozen.paused_at)
            .filter(|held| *held < RECOVERY_TIMEOUT)
            .ok_or("phase held interval exceeds its fixed deadline")?;
        mode.deadline = mode
            .deadline
            .checked_add(held)
            .ok_or("phase gate deadline extension overflow")?;
        mode.running_deadline = mode.deadline;
    }
    for batch in std::mem::take(&mut phase.deferred_peer).chunks(64) {
        accept_peer_batch(session, other(session.player), batch, events)?;
    }
    if let Some((count, through)) = phase.deferred_end.take() {
        session.end_live(other(session.player), count, through)?;
        return Err("source phase peer ended during the reliable Live handoff".into());
    }
    emit(
        events,
        LiveEvent::PhaseReady {
            epoch: session.epoch,
            round: active.round,
            attempt: active.attempt,
        },
    )
}

async fn phase_command(
    session: &Session,
    io: (&mut ControlIo, &mut quinn::SendStream, &mut u64),
    phase: &mut PhaseState,
    maintained: &mut sync::ClockMaintenance,
    command: LiveCommand,
) -> Result<(), ExchangeError> {
    let (control, input_send, input_written) = io;
    let player = session.player.index();
    match command {
        LiveCommand::PhaseSource {
            epoch,
            generation,
            source_id,
            publication,
        } if epoch == session.epoch => {
            if session.ended.iter().any(|ended| *ended) || phase.peer_end_count.is_some() {
                return Ok(());
            }
            if phase.sources[player].is_some() || generation == 0 || source_id == 0 {
                return Err("original phase source identity is repeated or invalid".into());
            }
            let publication = phase_publication_wire(publication, session.origin)?;
            let frame =
                SongTime::try_from_seconds_f64(f64::from_bits(publication.position_seconds_bits))
                    .filter(|frame| (0..session.prepared.end).contains(&frame.frames()))
                    .ok_or("original phase source is not unfinished")?;
            if publication.sequence == 0
                || publication.publication_before_ns > publication.publication_after_ns
                || publication.publication_after_ns > sync::now_ns(session.origin)?
            {
                return Err(
                    "original phase source publication has no real coherent delivery".into(),
                );
            }
            phase.sources[player] = Some((generation, source_id));
            phase.previous[player] = Some(publication);
            if session.player == PlayerId::P1 {
                phase.next_check_frame = frame.frames().checked_add(240_000);
            }
            phase_send(
                session,
                control,
                phase,
                0,
                Instant::now() + RECOVERY_TIMEOUT,
                wire::PhaseControl::Source {
                    generation,
                    source_id,
                    publication,
                },
            )
            .await?;
        }
        LiveCommand::PhaseObserved {
            epoch,
            round,
            attempt,
            verification,
            evidence,
        } if epoch == session.epoch => {
            if session.ended.iter().any(|ended| *ended) || phase.peer_end_count.is_some() {
                return Ok(());
            }
            let active = phase
                .active
                .as_mut()
                .ok_or("phase evidence has no active window")?;
            if round != active.round
                || attempt != active.attempt
                || verification != active.verification
                || active.step != PhaseStep::Sampling
                || active.own.is_some()
            {
                return Err("phase source batch belongs to another round or window".into());
            }
            let evidence = phase_evidence_wire(evidence, session.origin)?;
            if Some((evidence.generation, evidence.source_id)) != phase.sources[player] {
                return Err("phase source batch changed the original local source".into());
            }
            let (anchor, point, deadline) = (active.anchor, active.host_point_ns, active.deadline);
            active.own = Some(evidence.clone());
            phase_send(
                session,
                control,
                phase,
                round,
                deadline,
                wire::PhaseControl::Evidence {
                    anchor,
                    host_point_ns: point,
                    verification,
                    evidence,
                },
            )
            .await?;
            phase_try_gate(session, control, phase, maintained).await?;
        }
        LiveCommand::PhaseFrozen {
            epoch,
            round,
            attempt,
            snapshot,
        } if epoch == session.epoch => {
            let active = phase
                .active
                .as_mut()
                .ok_or("phase pause acknowledgment has no active round")?;
            if round != active.round
                || attempt != active.attempt
                || active.step != PhaseStep::Pausing
                || active.own_frozen.is_some()
            {
                return Err("phase pause acknowledgment belongs to another domain or round".into());
            }
            let original =
                phase.sources[player].ok_or("phase pause lost original source identity")?;
            let publication =
                validate_phase_frozen(session, &snapshot, original, phase.previous[player])?;
            let count = session.counts[player] as u64;
            let frame = snapshot.paused_frame.frames();
            let deadline = active.deadline;
            active.own_frozen = Some(snapshot);
            active.frozen[player] = Some((frame, publication, count));
            // Both writer calls follow all prior accepted Fact commands in the same mpsc FIFO
            tokio::time::timeout_at(
                deadline,
                wire::send_live_input(
                    input_send,
                    input_written,
                    &Input::PhasePaused {
                        epoch: session.epoch.0,
                        round,
                        attempt,
                        fact_count: count,
                    },
                ),
            )
            .await
            .map_err(|_| "phase FIFO marker write exceeded the original deadline")??;
            phase.active.as_mut().unwrap().markers[player] = Some(count);
            phase_send(
                session,
                control,
                phase,
                round,
                deadline,
                wire::PhaseControl::Frozen {
                    frame,
                    generation: original.0,
                    source_id: original.1,
                    publication,
                    owner_count: count,
                },
            )
            .await?;
            phase_try_schedule(session, control, phase, maintained).await?;
        }
        LiveCommand::PhaseArmed {
            epoch,
            round,
            attempt,
        } if epoch == session.epoch => {
            let active = phase
                .active
                .as_mut()
                .ok_or("phase Armed has no active round")?;
            if round != active.round
                || attempt != active.attempt
                || active.step != PhaseStep::Arming
                || active.own_armed
            {
                return Err("phase Armed belongs to another round or is repeated".into());
            }
            phase_scheduled(session, active)?;
            active.own_armed = true;
            let deadline = active.deadline;
            phase_send(
                session,
                control,
                phase,
                round,
                deadline,
                wire::PhaseControl::Armed {},
            )
            .await?;
            phase_try_confirm(session, control, phase).await?;
        }
        _ => return Err("phase command domain, epoch or round differs".into()),
    }
    Ok(())
}

async fn phase_control(
    session: &mut Session,
    control: &mut ControlIo,
    events: &std_mpsc::SyncSender<LiveEvent>,
    phase: &mut PhaseState,
    maintained: &mut sync::ClockMaintenance,
    mode: &mut ExchangeControl<'_>,
    envelope: Control,
) -> Result<(), ExchangeError> {
    let Control::Phase {
        epoch,
        round,
        attempt,
        message,
    } = envelope
    else {
        return Err("non-phase control while phase reader owns the stream".into());
    };
    if epoch != session.epoch.0 || attempt != phase.attempt {
        return Err(
            "phase control epoch or connection attempt differs from the authenticated session"
                .into(),
        );
    }
    let peer = other(session.player).index();
    match message {
        wire::PhaseControl::Source {
            generation,
            source_id,
            publication,
        } if round == 0 => {
            if session.ended.iter().any(|ended| *ended)
                || phase.peer_end_count.is_some()
                || phase.sources[peer].is_some()
            {
                return Err(
                    "peer original phase identity is repeated or no longer eligible".into(),
                );
            }
            SongTime::try_from_seconds_f64(f64::from_bits(publication.position_seconds_bits))
                .filter(|frame| (0..session.prepared.end).contains(&frame.frames()))
                .ok_or("peer original source is outside unfinished song")?;
            phase.sources[peer] = Some((generation, source_id));
            phase.previous[peer] = Some(publication);
            return Ok(());
        }
        wire::PhaseControl::Ended { owner_count } if round == 0 => {
            if phase.peer_end_count.replace(owner_count).is_some() {
                return Err("duplicate phase control Ended".into());
            }
            if session.ended[peer] && owner_count != session.counts[peer] as u64 {
                return Err(
                    "drained control Ended count differs from the actual Input End prefix".into(),
                );
            }
            return Ok(());
        }
        wire::PhaseControl::Check {
            anchor,
            host_point_ns,
            host_deadline_ns,
        } if session.player == PlayerId::P2 => {
            if session.ended.iter().any(|ended| *ended)
                || phase.peer_end_count.is_some()
                || phase.active.is_some()
                || phase.sources.iter().any(Option::is_none)
                || phase.round.checked_add(1) != Some(round)
            {
                return Err(
                    "peer check skips a phase round, missing source or ended lifecycle".into(),
                );
            }
            let sample = maintained
                .sample()
                .ok_or("peer check lacks actual clock maintenance")?;
            if anchor != phase_anchor(sample) {
                return Err(
                    "peer check anchor differs from the actual accepted four timestamps".into(),
                );
            }
            let earliest = maintained
                .clock
                .conservative_deadline(
                    session.epoch,
                    host_deadline_ns,
                    sync::now_ns(session.origin)?,
                    100_000_000,
                )
                .map_err(|error| error.to_string())?;
            let deadline = sync::local_instant(session.origin, earliest)?
                .min(Instant::now() + RECOVERY_TIMEOUT);
            phase.round = round;
            phase.active = Some(new_phase_round(round, anchor, host_point_ns, deadline));
            phase.active.as_mut().unwrap().host_deadline_ns = host_deadline_ns;
            phase.active.as_mut().unwrap().attempt = phase.attempt;
            emit(
                events,
                phase_sampling_event(session, phase.active.as_ref().unwrap(), sample)?,
            )?;
            return Ok(());
        }
        _ => {}
    }
    if session.ended.iter().any(|ended| *ended) || phase.peer_end_count.is_some() {
        return Err("phase controls cannot revive an ended source".into());
    }
    let mut active = phase
        .active
        .take()
        .ok_or("phase control has no active round")?;
    if round != active.round || Instant::now() >= active.deadline {
        return Err("phase control round or fixed deadline differs".into());
    }
    let deadline = active.deadline;
    let mut sends = Vec::new();
    let mut ready = false;
    let mut schedule = false;
    let mut gate = false;
    let mut confirm = false;
    match message {
        wire::PhaseControl::Evidence {
            anchor,
            host_point_ns,
            verification,
            evidence,
        } if active.step == PhaseStep::Sampling
            && anchor == active.anchor
            && host_point_ns == active.host_point_ns
            && verification == active.verification
            && active.peer.is_none() =>
        {
            if Some((evidence.generation, evidence.source_id)) != phase.sources[peer] {
                return Err("peer phase evidence replaces the original source".into());
            }
            active.peer = Some(evidence);
            gate = true;
        }
        wire::PhaseControl::Gate {
            anchor,
            host_point_ns,
            verification,
            host_received_ns,
            correcting,
        } if session.player == PlayerId::P2
            && active.step == PhaseStep::Sampling
            && anchor == active.anchor
            && host_point_ns == active.host_point_ns
            && verification == active.verification =>
        {
            maintained
                .check_local_time(sync::now_ns(session.origin)?)
                .map_err(|error| error.to_string())?;
            let evidence = [
                active
                    .peer
                    .as_ref()
                    .ok_or("host gate lacks its original publications")?
                    .clone(),
                active
                    .own
                    .as_ref()
                    .ok_or("host gate lacks local original publications")?
                    .clone(),
            ];
            let proof = sync::source_phase_bounds(
                &mut maintained.clock,
                anchor.exchange(epoch),
                &evidence,
                sync::PhaseWindow {
                    epoch: session.epoch,
                    round,
                    verification,
                    attempt: active.attempt,
                    reconnecting: active.reconnecting,
                    sources: phase.sources.map(|source| source.unwrap()),
                    previous: phase.previous,
                    end: session.prepared.end,
                    host_point_ns,
                    host_now_ns: host_received_ns,
                },
            )?;
            if !proof.supports_guard() {
                return Err("peer phase uncertainty is too wide for the unchanged guard".into());
            }
            if correcting == proof.within_guard() {
                return Err(
                    "host phase decision differs from the complete original-source bounds".into(),
                );
            }
            if active.reconnecting {
                proof.verify_reconnect_resume(
                    active
                        .frozen
                        .map(|frozen| frozen.expect("reconnect pause acknowledgments").1),
                    active
                        .common_frame
                        .ok_or("missing reconnect common frame")?,
                    active
                        .host_common_ns
                        .ok_or("missing reconnect common timestamp")?,
                    active.start_uncertainties_ns,
                )?;
            } else if verification {
                proof.verify_original_resume(
                    active
                        .frozen
                        .map(|frozen| frozen.expect("both actual pause acknowledgments").1),
                    active
                        .common_frame
                        .ok_or("missing scheduled common frame")?,
                )?;
            }
            session.record_phase(round, verification, active.reconnecting, serde_json::json!({
                "epoch": epoch, "round": round, "verification": verification,
                "connection_attempt": active.attempt, "reconnect_verification": active.reconnecting,
                "player": session.player.index() + 1, "actual_anchor": anchor,
                "host_point_ns": host_point_ns, "host_received_ns": host_received_ns,
                "own_receipt_ns": sync::now_ns(session.origin)?, "source_evidence": evidence,
                "source_intervals_frames": proof.source_frames(), "guest_minus_host_frames": proof.difference(),
                "within_guard": proof.within_guard(), "common_frame": active.common_frame, "host_common_ns": active.host_common_ns,
        "start_uncertainties_ns": active.start_uncertainties_ns,
                "original_source_ids": phase.sources, "previous_publications": phase.previous,
                "frozen_publications": active.frozen.map(|frozen| frozen.map(|value| value.1)),
                "marker_owner_counts": active.markers,
            }))?;
            active.bounds = Some(proof);
            active.step = if correcting {
                PhaseStep::GateAck
            } else {
                PhaseStep::AwaitLive
            };
            sends.push(wire::PhaseControl::GateAck {
                anchor,
                host_point_ns,
                verification,
            });
        }
        wire::PhaseControl::GateAck {
            anchor,
            host_point_ns,
            verification,
        } if session.player == PlayerId::P1
            && active.step == PhaseStep::GateAck
            && anchor == active.anchor
            && host_point_ns == active.host_point_ns
            && verification == active.verification =>
        {
            let proof = active
                .bounds
                .ok_or("phase GateAck lacks the host's original proof")?;
            if proof.within_guard() {
                sends.push(wire::PhaseControl::Live {
                    anchor,
                    host_point_ns,
                    verification,
                });
                ready = true;
            } else {
                if verification {
                    return Err(
                        "post-correction gate cannot spend another correction in the same round"
                            .into(),
                    );
                }
                phase.previous = proof.last_publications().map(Some);
                active.step = PhaseStep::Pausing;
                sends.push(wire::PhaseControl::Pause {});
                emit(
                    events,
                    LiveEvent::PhasePausing {
                        epoch: session.epoch,
                        round,
                        attempt: active.attempt,
                    },
                )?;
            }
        }
        wire::PhaseControl::Pause {}
            if session.player == PlayerId::P2
                && active.step == PhaseStep::GateAck
                && !active.verification =>
        {
            let proof = active
                .bounds
                .ok_or("phase Pause lacks the verified check proof")?;
            if proof.within_guard() || phase.corrections >= sync::MAX_PHASE_CORRECTIONS {
                return Err("phase Pause contradicts source guard or correction budget".into());
            }
            phase.corrections += 1;
            phase.previous = proof.last_publications().map(Some);
            active.step = PhaseStep::Pausing;
            emit(
                events,
                LiveEvent::PhasePausing {
                    epoch: session.epoch,
                    round,
                    attempt: active.attempt,
                },
            )?;
        }
        wire::PhaseControl::Frozen {
            frame,
            generation,
            source_id,
            publication,
            owner_count,
        } if active.step == PhaseStep::Pausing && active.frozen[peer].is_none() => {
            if Some((generation, source_id)) != phase.sources[peer]
                || SongTime::try_from_seconds_f64(f64::from_bits(publication.position_seconds_bits))
                    != Some(SongTime::from_frames(frame))
                || !(0..session.prepared.end).contains(&frame)
                || phase.previous[peer].is_some_and(|last| {
                    publication.sequence < last.sequence
                        || f64::from_bits(publication.position_seconds_bits)
                            < f64::from_bits(last.position_seconds_bits)
                        || (publication.sequence == last.sequence && publication != last)
                        || (publication.sequence > last.sequence
                            && publication.publication_before_ns < last.publication_after_ns)
                })
            {
                return Err(
                    "peer frozen source replaced or rewound actual original publication".into(),
                );
            }
            active.frozen[peer] = Some((frame, publication, owner_count));
            schedule = true;
        }
        wire::PhaseControl::Schedule {
            anchor,
            host_common_ns,
            host_verify_ns,
            common_frame,
            paused,
        } if session.player == PlayerId::P2 && active.step == PhaseStep::Pausing => {
            if active.pending_schedule.is_some() {
                return Err("repeated pending phase schedule".into());
            }
            if active.markers.iter().any(Option::is_none) {
                active.pending_schedule = Some(wire::PhaseControl::Schedule {
                    anchor,
                    host_common_ns,
                    host_verify_ns,
                    common_frame,
                    paused,
                });
                phase.active = Some(active);
                return Ok(());
            }
            if active.frozen.iter().any(Option::is_none)
                || active.frozen.map(|frozen| frozen.unwrap().0) != paused
                || active
                    .frozen
                    .iter()
                    .zip(active.markers)
                    .any(|(frozen, count)| frozen.unwrap().2 != count.unwrap())
                || Some(&common_frame) != paused.iter().max()
                || host_common_ns.checked_add(100_000_000) != Some(host_verify_ns)
                || maintained
                    .sample()
                    .is_none_or(|sample| phase_anchor(sample) != anchor)
            {
                return Err(
                    "phase schedule differs from actual paired pause fences or fresh clock".into(),
                );
            }
            phase.previous = active.frozen.map(|frozen| Some(frozen.unwrap().1));
            let sample = maintained.sample().unwrap();
            let now = sync::now_ns(session.origin)?;
            let catchup = sync::catchup_ns(common_frame, paused[1])?;
            let resume = maintained
                .clock
                .schedule_start(
                    session.epoch,
                    host_common_ns
                        .checked_sub(catchup)
                        .ok_or("phase guest catchup underflow")?,
                    now,
                    100_000_000,
                )
                .map_err(|error| error.to_string())?;
            let common = maintained
                .clock
                .schedule_start(session.epoch, host_common_ns, now, 100_000_000)
                .map_err(|error| error.to_string())?;
            let verify = maintained
                .clock
                .schedule_start(session.epoch, host_verify_ns, now, 100_000_000)
                .map_err(|error| error.to_string())?;
            active.anchor = anchor;
            active.common_frame = Some(common_frame);
            active.host_common_ns = Some(host_common_ns);
            active.host_verify_ns = Some(host_verify_ns);
            active.plan = Some(sync::ResumePlan {
                deadline: sync::local_instant(session.origin, resume.guest_start_ns)?,
                verify_at: sync::local_instant(session.origin, verify.guest_start_ns)?,
                host_verify_ns,
                common_frame,
                start_uncertainties_ns: [0; 2],
                phase_deadline: None,
                timing: phase_timing(
                    sample,
                    host_common_ns,
                    resume.guest_start_ns,
                    resume.uncertainty_ns,
                ),
            });
            active.step = PhaseStep::Arming;
            sends.push(wire::PhaseControl::ScheduleAck {
                anchor,
                host_common_ns,
                common_frame,
                guest_now_ns: now,
                guest_resume_ns: resume.guest_start_ns,
                guest_common_ns: common.guest_start_ns,
                guest_verify_ns: verify.guest_start_ns,
                uncertainty_ns: resume.uncertainty_ns,
            });
            emit(events, phase_scheduled(session, &active)?)?;
        }
        wire::PhaseControl::ScheduleAck {
            anchor,
            host_common_ns,
            common_frame,
            guest_now_ns,
            guest_resume_ns,
            guest_common_ns,
            guest_verify_ns,
            uncertainty_ns,
        } if session.player == PlayerId::P1
            && active.step == PhaseStep::ScheduleAck
            && anchor == active.anchor
            && Some(host_common_ns) == active.host_common_ns
            && Some(common_frame) == active.common_frame =>
        {
            if maintained
                .sample()
                .is_none_or(|sample| phase_anchor(sample) != anchor)
            {
                return Err("phase schedule Ack lost its original clock anchor".into());
            }
            let delay = sync::catchup_ns(common_frame, active.frozen[1].unwrap().0)?;
            let resume = maintained
                .clock
                .schedule_start(
                    session.epoch,
                    host_common_ns
                        .checked_sub(delay)
                        .ok_or("phase peer catchup underflow")?,
                    guest_now_ns,
                    100_000_000,
                )
                .map_err(|error| error.to_string())?;
            let common = maintained
                .clock
                .schedule_start(session.epoch, host_common_ns, guest_now_ns, 100_000_000)
                .map_err(|error| error.to_string())?;
            let verify = maintained
                .clock
                .schedule_start(
                    session.epoch,
                    active.host_verify_ns.unwrap(),
                    guest_now_ns,
                    100_000_000,
                )
                .map_err(|error| error.to_string())?;
            if [
                guest_resume_ns,
                guest_common_ns,
                guest_verify_ns,
                uncertainty_ns,
            ] != [
                resume.guest_start_ns,
                common.guest_start_ns,
                verify.guest_start_ns,
                resume.uncertainty_ns,
            ] {
                return Err(
                    "phase schedule Ack differs from the complete future uncertainty interval"
                        .into(),
                );
            }
            active.step = PhaseStep::Arming;
            emit(events, phase_scheduled(session, &active)?)?;
        }
        wire::PhaseControl::Armed {} if active.step == PhaseStep::Arming && !active.peer_armed => {
            active.peer_armed = true;
            confirm = true;
        }
        wire::PhaseControl::Confirmed {}
            if active.step == PhaseStep::Arming
                && active.own_armed
                && active.peer_armed
                && !active.peer_confirmed =>
        {
            active.peer_confirmed = true;
            confirm = true;
        }
        wire::PhaseControl::Verify {
            anchor,
            host_point_ns,
        } if session.player == PlayerId::P2
            && (active.step == PhaseStep::VerifyClock
                && Some(host_point_ns) == active.host_verify_ns
                || active.step == PhaseStep::ReconnectClock && active.attempt == 1)
            && anchor.clock_round > active.anchor.clock_round =>
        {
            let sample = maintained
                .sample()
                .ok_or("verification lacks actual refreshed clock")?;
            if phase_anchor(sample) != anchor {
                return Err("verification refresh is not the actual accepted CBMC exchange".into());
            }
            if active.reconnecting {
                let earliest = maintained
                    .clock
                    .conservative_deadline(
                        session.epoch,
                        active.host_deadline_ns,
                        sync::now_ns(session.origin)?,
                        100_000_000,
                    )
                    .map_err(|error| error.to_string())?;
                active.deadline = active
                    .deadline
                    .min(sync::local_instant(session.origin, earliest)?);
            }
            active.anchor = anchor;
            active.host_point_ns = host_point_ns;
            active.verification = true;
            active.own = None;
            active.peer = None;
            active.bounds = None;
            active.step = PhaseStep::Sampling;
            emit(events, phase_sampling_event(session, &active, sample)?)?;
        }
        wire::PhaseControl::Live {
            anchor,
            host_point_ns,
            verification,
        } if session.player == PlayerId::P2
            && active.step == PhaseStep::AwaitLive
            && anchor == active.anchor
            && host_point_ns == active.host_point_ns
            && verification == active.verification =>
        {
            ready = true;
        }
        _ => return Err("phase reliable control does not match its bounded transition".into()),
    }
    phase.active = Some(active);
    for message in sends {
        phase_send(session, control, phase, round, deadline, message).await?;
    }
    if gate {
        phase_try_gate(session, control, phase, maintained).await?;
    }
    if schedule {
        phase_try_schedule(session, control, phase, maintained).await?;
    }
    if confirm {
        phase_try_confirm(session, control, phase).await?;
    }
    if ready {
        phase_finish(session, events, phase, mode).await?;
    }
    Ok(())
}

fn prior_phase_pause_extension(
    origin: std::time::Instant,
    active: Option<&PhaseRound>,
    prior: Option<[std::time::Instant; 2]>,
    paused_at: std::time::Instant,
) -> Result<Duration, String> {
    let Some([start, end]) = prior else {
        return Ok(Duration::ZERO);
    };
    let held = end
        .checked_duration_since(start)
        .filter(|held| !held.is_zero() && *held < RECOVERY_TIMEOUT)
        .ok_or("prior phase pause acknowledgment span is invalid")?;
    if start < origin || end > paused_at {
        return Err("prior phase pause span crosses origin or the new actual pause".into());
    }
    let Some(active) = active else {
        return Ok(Duration::ZERO);
    };
    if !active.verification
        && !matches!(
            active.step,
            PhaseStep::Pausing
                | PhaseStep::ScheduleAck
                | PhaseStep::Arming
                | PhaseStep::VerifyClock
        )
        || active
            .own_frozen
            .as_ref()
            .is_some_and(|frozen| frozen.paused_at != start)
    {
        return Err(
            "prior pause does not belong to the unfinished original phase correction".into(),
        );
    }
    Ok(held)
}

fn merge_phase_floor(
    left: Option<wire::PhasePublication>,
    right: Option<wire::PhasePublication>,
) -> Result<Option<wire::PhasePublication>, String> {
    for publication in [left, right].into_iter().flatten() {
        publication.validate()?;
    }
    let (Some(left), Some(right)) = (left, right) else {
        return Ok(left.or(right));
    };
    if left.sequence == right.sequence {
        if left != right {
            return Err("phase continuation mutates one accepted publication".into());
        }
        return Ok(Some(left));
    }
    let (old, next) = if left.sequence < right.sequence {
        (left, right)
    } else {
        (right, left)
    };
    if f64::from_bits(next.position_seconds_bits) < f64::from_bits(old.position_seconds_bits)
        || next.publication_before_ns < old.publication_after_ns
    {
        return Err("phase continuation rewinds its full publication floor".into());
    }
    Ok(Some(next))
}

fn phase_description(
    phase: &PhaseState,
    active: &PhaseRound,
    sealed: bool,
) -> Result<wire::PhaseResume, String> {
    let stage = if active.verification
        || matches!(
            active.step,
            PhaseStep::Pausing
                | PhaseStep::ScheduleAck
                | PhaseStep::Arming
                | PhaseStep::VerifyClock
        ) {
        wire::PhaseProofStage::Correction
    } else {
        wire::PhaseProofStage::Check
    };
    let descriptor = wire::PhaseResume {
        round: active.round,
        host_deadline_ns: active.host_deadline_ns,
        host_point_ns: active.host_point_ns,
        stage,
        corrections: phase.corrections,
        sealed,
        anchor: active.anchor,
        sources: [
            phase.sources[0].ok_or("phase continuation lost Host source")?,
            phase.sources[1].ok_or("phase continuation lost Guest source")?,
        ],
        previous: if sealed {
            active
                .bounds
                .ok_or("sealed phase lost accepted proof")?
                .last_publications()
                .map(Some)
        } else {
            phase.previous
        },
        markers: active.markers,
    };
    descriptor.validate()?;
    Ok(descriptor)
}

fn reconcile_phase_resume(
    state: &PhaseState,
    player: PlayerId,
    own: RemoteFreeze,
    peer: RemoteFreeze,
) -> Result<Option<wire::PhaseResume>, String> {
    own.publication.validate()?;
    peer.publication.validate()?;
    for freeze in [own, peer] {
        if SongTime::try_from_seconds_f64(f64::from_bits(freeze.publication.position_seconds_bits))
            != Some(SongTime::from_frames(freeze.frame))
        {
            return Err("continuation frame differs from actual original cursor bits".into());
        }
        if let Some(phase) = freeze.phase {
            phase.validate()?;
        }
    }
    let selected = if player == PlayerId::P1 {
        if let Some(active) = own.phase {
            Some(active)
        } else if let Some(remote) = peer.phase {
            let sealed = state
                .sealed
                .ok_or("peer unresolved phase has no last-sealed Host round")?;
            if remote.round != sealed.round {
                return Err("peer unresolved phase differs from last-sealed Host round".into());
            }
            Some(sealed)
        } else {
            None
        }
    } else {
        peer.phase
    };
    let Some(mut selected) = selected else {
        if own.phase.is_some() {
            return Err("Host continuation omits an unresolved Guest phase".into());
        }
        return Ok(None);
    };
    if selected.round != state.round
        && !(player == PlayerId::P2
            && own.phase.is_none()
            && state.round.checked_add(1) == Some(selected.round))
    {
        return Err("continuation skips or overlaps the original phase round".into());
    }
    if let Some(local) = own.phase
        && (local.round != selected.round
            || local.host_deadline_ns != selected.host_deadline_ns
            || local.sources != selected.sources)
    {
        return Err("continuation changes the original phase identity or deadline".into());
    }
    for descriptor in [own.phase, peer.phase].into_iter().flatten() {
        if descriptor.round != selected.round
            || descriptor.host_deadline_ns != selected.host_deadline_ns
            || descriptor.sources != selected.sources
            || descriptor.corrections.abs_diff(selected.corrections) > 1
            || (descriptor.corrections != selected.corrections
                && descriptor.stage == wire::PhaseProofStage::Check
                && selected.stage == wire::PhaseProofStage::Check)
        {
            return Err(
                "continuation contradicts the original phase sources or correction stage".into(),
            );
        }
        if selected.stage == wire::PhaseProofStage::Check
            && descriptor.stage == wire::PhaseProofStage::Correction
        {
            return Err("peer invents a correction not issued by Host".into());
        }
        selected.corrections = selected.corrections.max(descriptor.corrections);
        if descriptor.anchor.clock_round == selected.anchor.clock_round
            && descriptor.anchor != selected.anchor
        {
            return Err("continuation changes the actual old clock exchange".into());
        }
        if descriptor.anchor.clock_round > selected.anchor.clock_round {
            selected.anchor = descriptor.anchor;
        }
        for index in 0..2 {
            selected.previous[index] =
                merge_phase_floor(selected.previous[index], descriptor.previous[index])?;
            if selected.markers[index]
                .zip(descriptor.markers[index])
                .is_some_and(|(left, right)| left != right)
            {
                return Err("continuation disagrees with an accepted FIFO marker".into());
            }
            selected.markers[index] = selected.markers[index].or(descriptor.markers[index]);
        }
    }
    let freezes = if player == PlayerId::P1 {
        [own, peer]
    } else {
        [peer, own]
    };
    for (index, freeze) in freezes.into_iter().enumerate() {
        if selected.sources[index] != (freeze.generation, freeze.source_id)
            || selected.markers[index].is_some_and(|marker| marker > freeze.count)
        {
            return Err(
                "continuation replaces the original source or truncates its accepted prefix".into(),
            );
        }
        selected.previous[index] =
            merge_phase_floor(selected.previous[index], Some(freeze.publication))?;
        // Metadata floors cannot exceed this actual newly paused source
        if selected.previous[index] != Some(freeze.publication) {
            return Err("actual continuation pause predates an accepted publication floor".into());
        }
    }
    selected.validate()?;
    Ok(Some(selected))
}

fn prepare_phase_rebound(
    session: &Session,
    state: &mut PhaseState,
    context: &ResumeContext,
) -> Result<(), String> {
    if state.attempt != 0
        || session.ended.iter().any(|ended| *ended)
        || state.peer_end_count.is_some()
    {
        return Err("phase continuation is repeated or tries to revive End".into());
    }
    state.attempt = 1;
    let own_pub = phase_publication_wire(context.frozen.publication, session.origin)?;
    let freezes = if session.player == PlayerId::P1 {
        [
            (
                context.frozen.paused_frame.frames(),
                own_pub,
                session.counts[0] as u64,
            ),
            (
                context.peer.frame,
                context.peer.publication,
                session.counts[1] as u64,
            ),
        ]
    } else {
        [
            (
                context.peer.frame,
                context.peer.publication,
                session.counts[0] as u64,
            ),
            (
                context.frozen.paused_frame.frames(),
                own_pub,
                session.counts[1] as u64,
            ),
        ]
    };
    let sources = if session.player == PlayerId::P1 {
        [
            (context.frozen.source_generation, context.frozen.source_id),
            (context.peer.generation, context.peer.source_id),
        ]
    } else {
        [
            (context.peer.generation, context.peer.source_id),
            (context.frozen.source_generation, context.frozen.source_id),
        ]
    };
    for index in 0..2 {
        if state.sources[index].is_some_and(|ids| ids != sources[index]) {
            return Err("continuation changes original phase source".into());
        }
        state.sources[index] = Some(sources[index]);
        state.previous[index] = merge_phase_floor(state.previous[index], Some(freezes[index].1))?;
    }
    let Some(descriptor) = context.phase else {
        return Ok(());
    };
    let mut rebound = new_phase_round(
        descriptor.round,
        descriptor.anchor,
        descriptor.host_point_ns,
        context.deadline,
    );
    rebound.attempt = 1;
    rebound.reconnecting = true;
    rebound.host_deadline_ns = descriptor.host_deadline_ns;
    rebound.verification = true;
    rebound.step = PhaseStep::ReconnectClock;
    rebound.frozen = freezes.map(Some);
    rebound.markers = freezes.map(|freeze| Some(freeze.2));
    state.round = descriptor.round;
    state.corrections = state.corrections.max(descriptor.corrections);
    // No begin/reset, second correction or old proof/control is retained
    state.active = Some(rebound);
    state.sealed = None;
    Ok(())
}

struct Continuation {
    invitation: Invitation,
    capability: [u8; 32],
    used: bool,
    candidates: u8,
    maintenance_round: u64,
    maintenance: Option<sync::ClockMaintenance>,
    phase: PhaseState,
    pending: JoinSet<Result<AuthenticatedContinuation, String>>,
}

#[derive(Clone, Copy)]
struct RemoteFreeze {
    frame: i64,
    publication: wire::PhasePublication,
    phase: Option<wire::PhaseResume>,
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
impl From<crate::clock::ClockError> for ExchangeError {
    fn from(error: crate::clock::ClockError) -> Self {
        match error {
            crate::clock::ClockError::Stale => {
                Self::Recoverable("clock maintenance sample is stale")
            }
            other => Self::Terminal(other.to_string()),
        }
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
                publication,
                phase,
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
                    publication,
                    phase,
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
    phase: Option<wire::PhaseResume>,
    prior_phase_pause_extension: Duration,
    deadline: Instant,
}
struct ResumeSignals {
    armed: Option<oneshot::Sender<()>>,
    observed: Option<oneshot::Sender<RecoveryObserved>>,
}
struct ResumeCompletion {
    pause: Duration,
    control: ControlIo,
    host_common_ns: u64,
    common_frame: i64,
    start_uncertainties_ns: [u64; 2],
    phase_deadline: Option<Instant>,
}
type ResumeGate<'a> = Pin<Box<dyn Future<Output = Result<ResumeCompletion, String>> + 'a>>;
type PhaseRead = Pin<
    Box<
        dyn Future<
            Output = (
                quinn::RecvStream,
                wire::PhaseBudget,
                Result<Control, wire::LiveIoError>,
            ),
        >,
    >,
>;

struct ExchangeControl<'a> {
    control: Option<ControlIo>,
    phase_read: Option<PhaseRead>,
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
    let deadline = continuation
        .phase
        .active
        .as_ref()
        .map_or(Instant::now() + RECOVERY_TIMEOUT, |phase| {
            phase.deadline.min(Instant::now() + RECOVERY_TIMEOUT)
        });
    tokio::time::timeout_at(deadline, async {
        old_connection.close(RECOVERY_REQUESTED.into(), b"connection maintenance");
        emit(
            events,
            LiveEvent::RecoveryPausing {
                epoch: session.epoch,
                attempt: sync::RESUME_ATTEMPT,
                deadline: deadline.into_std(),
                phase_round: continuation.phase.active.as_ref().map(|phase| phase.round),
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
                    LiveCommand::PhaseObserved {
                        epoch, attempt: 0, ..
                    }
                    | LiveCommand::PhaseFrozen {
                        epoch, attempt: 0, ..
                    }
                    | LiveCommand::PhaseArmed {
                        epoch, attempt: 0, ..
                    } if epoch == session.epoch => {}
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
        let publication = phase_publication_wire(frozen.publication, session.origin)?;
        publication.validate()?;
        if SongTime::try_from_seconds_f64(f64::from_bits(publication.position_seconds_bits))
            != Some(frozen.paused_frame)
            || frozen.publication.published_between[1] > std::time::Instant::now()
            || std::time::Instant::now()
                .checked_duration_since(frozen.publication.published_between[0])
                .is_none_or(|age| age > Duration::from_millis(50))
        {
            return Err("recovery freeze lacks an actual coherent cursor acknowledgment".into());
        }
        let local = session.player.index();
        if continuation.phase.sources[local]
            .is_some_and(|ids| ids != (frozen.source_generation, frozen.source_id))
        {
            return Err("recovery freeze replaces the phase original source".into());
        }
        merge_phase_floor(continuation.phase.previous[local], Some(publication))?;
        let prior_phase_pause_extension = prior_phase_pause_extension(
            session.origin.into_std(), continuation.phase.active.as_ref(),
            frozen.prior_phase_pause, frozen.paused_at,
        )?;
        session.begin_recovery(&frozen.replay, serde_json::json!({
        "attempt": sync::RESUME_ATTEMPT, "epoch": session.epoch.0, "cause": cause,
        "paused_frame": frozen.paused_frame.frames(), "source_generation": frozen.source_generation,
        "source_id": frozen.source_id, "worker_counts": session.counts,
        "prior_phase_pause": frozen.prior_phase_pause.map(|span| span.map(|at| at.duration_since(session.origin.into_std()).as_nanos())),
        "prior_phase_pause_extension_ns": prior_phase_pause_extension.as_nanos(),
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
            publication,
            phase: continuation
                .phase
                .active
                .as_ref()
                .map(|active| phase_description(&continuation.phase, active, false))
                .transpose()?,
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
                        publication: own.publication,
                        phase: own.phase,
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
                        publication,
                        phase,
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
                            publication,
                            phase,
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
        let phase = reconcile_phase_resume(&continuation.phase, session.player, own, peer)?;
        let deadline = if session.player == PlayerId::P1 {
            phase
                .map(|phase| sync::local_instant(session.origin, phase.host_deadline_ns))
                .transpose()?
                .map_or(deadline, |phase| deadline.min(phase))
        } else {
            deadline
        };
        if session.player == PlayerId::P1 {
            control
                .send(Control::ResumeWelcome {
                    protocol_version: PROTOCOL_VERSION,
                    epoch: session.epoch.0,
                    player: 1,
                    identity: session.prepared.identity.clone(),
                    attempt: sync::RESUME_ATTEMPT,
                    pause_frame: own.frame,
                    publication: own.publication,
                    phase,
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
                phase,
                prior_phase_pause_extension,
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

fn phase_publication_wire(
    publication: PhasePublication,
    origin: Instant,
) -> Result<wire::PhasePublication, String> {
    let convert = |at: std::time::Instant| -> Result<u64, String> {
        u64::try_from(
            at.checked_duration_since(origin.into_std())
                .ok_or("phase publication predates session origin")?
                .as_nanos(),
        )
        .map_err(|_| "phase publication time overflow".into())
    };
    Ok(wire::PhasePublication {
        sequence: publication.sequence,
        position_seconds_bits: publication.position_seconds_bits,
        publication_before_ns: convert(publication.published_between[0])?,
        publication_after_ns: convert(publication.published_between[1])?,
    })
}

fn phase_evidence_wire(
    local: PhaseObserved,
    origin: Instant,
) -> Result<wire::PhaseEvidence, String> {
    let collected_at_ns = u64::try_from(
        local
            .collected_at
            .checked_duration_since(origin.into_std())
            .ok_or("phase collection predates session origin")?
            .as_nanos(),
    )
    .map_err(|_| "phase collection time overflow")?;
    let evidence = wire::PhaseEvidence {
        generation: local.generation,
        source_id: local.source_id,
        collected_at_ns,
        publications: local
            .publications
            .into_iter()
            .map(|row| phase_publication_wire(row, origin))
            .collect::<Result<_, _>>()?,
    };
    evidence.validate()?;
    if local.collected_at > std::time::Instant::now() {
        return Err("phase collection lies after its real delivery".into());
    }
    Ok(evidence)
}

fn validate_phase_frozen(
    session: &Session,
    frozen: &PhaseFrozen,
    original: (u64, u64),
    previous: Option<wire::PhasePublication>,
) -> Result<wire::PhasePublication, String> {
    let publication = phase_publication_wire(frozen.publication, session.origin)?;
    if frozen.replay.epoch() != session.epoch
        || frozen.replay.identity().content_id != session.prepared.identity.content_id
        || frozen.replay.identity().stage_compiler_version
            != session.prepared.identity.stage_compiler_version
        || original != (frozen.source_generation, frozen.source_id)
        || frozen.paused_at > std::time::Instant::now()
        || frozen.publication.published_between[1] > frozen.paused_at
        || frozen
            .paused_at
            .checked_duration_since(frozen.publication.published_between[0])
            .is_none_or(|age| age > Duration::from_millis(50))
        || SongTime::try_from_seconds_f64(f64::from_bits(publication.position_seconds_bits))
            != Some(frozen.paused_frame)
        || !(0..session.prepared.end).contains(&frozen.paused_frame.frames())
        || publication.sequence == 0
        || publication.publication_before_ns > publication.publication_after_ns
    {
        return Err("actual phase pause identity, cursor or acknowledgment differs".into());
    }
    if previous.is_some_and(|last| {
        publication.sequence < last.sequence
            || f64::from_bits(publication.position_seconds_bits)
                < f64::from_bits(last.position_seconds_bits)
            || (publication.sequence == last.sequence && publication != last)
            || (publication.sequence > last.sequence
                && publication.publication_before_ns < last.publication_after_ns)
    }) {
        return Err("phase frozen publication rewinds the accepted original source".into());
    }
    frozen
        .replay
        .replay(
            &session.prepared.identity.content_id,
            RULESET,
            session.prepared.anchors.clone(),
            cocobeat_schema::DuoRules::default(),
        )
        .map_err(|_| "phase frozen Replay identity or core facts are invalid")?;
    for player in [session.player, other(session.player)] {
        let gui: Vec<_> = frozen
            .replay
            .facts()
            .iter()
            .copied()
            .filter(|fact| session::seat(*fact) == player)
            .collect();
        let worker: Vec<_> = session
            .replay
            .facts()
            .iter()
            .copied()
            .filter(|fact| session::seat(*fact) == player)
            .collect();
        if (player == session.player && gui != worker)
            || (player != session.player && !worker.starts_with(&gui))
        {
            return Err(
                "phase frozen Replay differs from the accepted owner fence or peer prefix".into(),
            );
        }
    }
    Ok(publication)
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
    mut control: ControlIo,
    events: &std_mpsc::SyncSender<LiveEvent>,
    context: ResumeContext,
    acknowledgments: (oneshot::Receiver<()>, oneshot::Receiver<RecoveryObserved>),
) -> Result<ResumeCompletion, String> {
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
        let plan = sync::arm_resume(
            connection,
            &mut control,
            epoch,
            player,
            origin,
            (paused, context.phase.map(|phase| phase.host_deadline_ns)),
            end,
        )
        .await?;
        let original_deadline = plan
            .phase_deadline
            .map_or(context.deadline, |deadline| deadline.min(context.deadline));
        tokio::time::timeout_at(original_deadline, async {
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
                &mut control,
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
                || local.progress.sequence <= frozen.publication.sequence
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
                _ => {
                    return Err(
                        "peer resume observations belong to another epoch or attempt".into(),
                    );
                }
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
            Ok(ResumeCompletion {
                pause: pause_extension
                    .checked_add(context.prior_phase_pause_extension)
                    .ok_or("combined actual disjoint pause extension overflow")?,
                control,
                host_common_ns: plan
                    .host_verify_ns
                    .checked_sub(100_000_000)
                    .ok_or("reconnect common time underflow")?,
                common_frame: plan.common_frame,
                start_uncertainties_ns: plan.start_uncertainties_ns,
                phase_deadline: plan.phase_deadline,
            })
        })
        .await
        .map_err(|_| "resume source verification exceeded the original phase deadline")?
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
    let (endpoint, connection, control, mut inputs) = transport;
    let mut mode = ExchangeControl {
        control: Some(control),
        phase_read: None,
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
            let mut control = mode
                .control
                .take()
                .ok_or("live control disappeared before Finish")?;
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
    let running_deadline = mode.running_deadline;
    drop(mode);
    drop(inputs);
    for batch in std::mem::take(&mut continuation.phase.deferred_peer).chunks(64) {
        accept_peer_batch(session, other(session.player), batch, events)?;
    }
    if continuation.phase.deferred_end.is_some() || continuation.phase.peer_end_count.is_some() {
        return Err("ended phase cannot enter authenticated continuation".into());
    }
    let (connection, control, mut inputs, context) = freeze_and_reconnect(
        session,
        (&connection, endpoint, candidate),
        commands,
        events,
        continuation,
        owned_endpoint,
        cause,
    )
    .await?;
    prepare_phase_rebound(session, &mut continuation.phase, &context)?;
    let recovery_deadline = context.deadline;
    let (armed_tx, armed_rx) = oneshot::channel();
    let (observed_tx, observed_rx) = oneshot::channel();
    let gate = Box::pin(resume_control(
        &connection,
        control,
        events,
        context,
        (armed_rx, observed_rx),
    ));
    let mut mode = ExchangeControl {
        control: None,
        phase_read: None,
        gate: Some(gate),
        signals: Some(ResumeSignals {
            armed: Some(armed_tx),
            observed: Some(observed_tx),
        }),
        deadline: recovery_deadline,
        running_deadline,
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
    let mut control = mode
        .control
        .take()
        .ok_or("continued control disappeared before Finish")?;
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
    let mut maintenance = if mode.recovering {
        None
    } else if let Some(clock) = continuation.maintenance.take() {
        Some(clock)
    } else {
        Some(sync::ClockMaintenance::new(
            session.epoch,
            session.player,
            continuation.maintenance_round,
            sync::now_ns(session.origin)?,
        )?)
    };
    if let Some(control) = mode.control.as_mut() {
        mode.phase_read = Some(phase_reader(control, &mut continuation.phase)?);
    }
    let result = async {
    while !session.ended.iter().all(|ended| *ended)
        || (mode.control.is_some() && continuation.phase.peer_end_count.is_none())
    {
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
                let maintenance_deadline = maintenance
                    .as_ref()
                    .map(|clock| sync::local_instant(session.origin, clock.next_deadline_ns()?))
                    .transpose()?
                    .unwrap_or(mode.deadline);
                let phase_deadline = continuation
                    .phase
                    .active
                    .as_ref()
                    .map_or(mode.deadline, |phase| phase.deadline);
                let phase_verify_cutoff = continuation
                    .phase
                    .active
                    .as_ref()
                    .filter(|phase| phase.step == PhaseStep::VerifyClock)
                    .and_then(|phase| phase.plan.as_ref())
                    .and_then(|plan| plan.verify_at.checked_sub(Duration::from_millis(40)))
                    .unwrap_or(phase_deadline);
                tokio::select! {
                    message = &mut receiving => break Some(message.map_err(ExchangeError::from)?),
                    result = async { mode.phase_read.as_mut().expect("guarded phase reader").await }, if mode.phase_read.is_some() => {
                        let (stream, budget, message) = result;
                        mode.phase_read = None;
                        let message = message?;
                        continuation.phase.received = budget;
                        let mut control = mode.control.take().ok_or("phase control sender disappeared")?;
                        control.recv = Some(stream);
                        let clock = maintenance.as_mut().ok_or("phase control arrived while authenticated recovery owns its clock")?;
                        phase_control(session, &mut control, events, &mut continuation.phase, clock, mode, message).await?;
                        if continuation.phase.peer_end_count.is_none() {
                            mode.phase_read = Some(phase_reader(&mut control, &mut continuation.phase)?);
                        }
                        mode.control = Some(control);
                        if session.ended.iter().all(|ended| *ended) && continuation.phase.peer_end_count.is_some() { break None; }
                    }
                    _ = tokio::time::sleep_until(phase_deadline), if continuation.phase.active.is_some() => return Err("source phase exceeded its original fixed deadline".into()),
                    _ = tokio::time::sleep_until(phase_verify_cutoff), if continuation.phase.active.as_ref().is_some_and(|phase| phase.step == PhaseStep::VerifyClock) => return Err("source phase lacks a fresh CBMC before its fixed verification window".into()),
                    reason = connection.closed() => return Err(wire::LiveIoError::Transport(reason).into()),
                    error = session::extra_bidi(connection) => return Err(error.into()),
                    _ = tokio::time::sleep_until(mode.deadline) => return Err("live round or recovery exceeded its explicit deadline".into()),
                    bytes = connection.read_datagram(), if maintenance.is_some() => {
                        let bytes = bytes.map_err(wire::LiveIoError::Transport)?;
                        let received_ns = sync::now_ns(session.origin)?;
                        let clock = maintenance.as_mut().ok_or("clock maintenance state disappeared")?;
                        clock.check_local_time(received_ns)?;
                        let result = clock.receive(&bytes, received_ns, sync::now_ns(session.origin)?)?;
                        continuation.maintenance_round = clock.round();
                        if let Some(reply) = result.reply {
                            connection.send_datagram(reply.to_vec().into()).map_err(|_| "send clock maintenance datagram failed")?;
                        }
                        if let Some(sample) = result.sample {
                            emit(events, LiveEvent::ClockMaintained { epoch: sample.estimate.epoch, round: sample.round, exchange: sample.exchange })?;
                            if let Some(control) = mode.control.as_mut() {
                                phase_clock_progress(session, control, events, &mut continuation.phase, clock).await?;
                            }
                        }
                    }
                    _ = tokio::time::sleep_until(maintenance_deadline), if maintenance.is_some() => {
                        let clock = maintenance.as_mut().ok_or("clock maintenance state disappeared")?;
                        let now_ns = sync::now_ns(session.origin)?;
                        clock.check_local_time(now_ns)?;
                        if let Some(probe) = clock.tick(now_ns)? {
                            connection.send_datagram(probe.to_vec().into()).map_err(|_| "send clock maintenance probe failed")?;
                        }
                        continuation.maintenance_round = clock.round();
                    }
                    result = async { mode.gate.as_mut().expect("guarded resume gate").await }, if mode.gate.is_some() => {
                        let completed = result?;
                        let pause = completed.pause;
                        mode.control = Some(completed.control);
                        if let Some(active) = continuation.phase.active.as_mut().filter(|active| active.reconnecting) {
                            active.common_frame = Some(completed.common_frame);
                            active.host_common_ns = Some(completed.host_common_ns);
                            active.start_uncertainties_ns = completed.start_uncertainties_ns;
                            if let Some(deadline) = completed.phase_deadline { active.deadline = active.deadline.min(deadline); }
                        }
                        if Instant::now() >= mode.deadline { return Err("resume Ready missed its fixed deadline".into()); }
                        mode.gate = None;
                        mode.signals = None;
                        // Different QUIC streams can deliver a legitimate post-Live Hit before local control completion
                        if continuation.phase.active.as_ref().is_some_and(|active| active.reconnecting)
                            && deferred_peer.iter().any(|fact| matches!(fact, Fact::Hit { .. })) {
                            return Err("performing peer Hit crosses the unresolved reconnect phase".into());
                        }
                        for batch in std::mem::take(&mut deferred_peer).chunks(64) {
                            accept_peer_batch(session, peer, batch, events)?;
                        }
                        if let Some((count, through)) = deferred_end.take() {
                            session.end_live(peer, count, through)?;
                        }
                        if Instant::now() >= mode.deadline { return Err("deferred peer facts exceeded the fixed recovery deadline".into()); }
                        if continuation.phase.active.is_some() && session.ended.iter().any(|ended| *ended) {
                            return Err("ended source cannot reopen the unresolved reconnect phase".into());
                        }
                        mode.recovering = false;
                        if let Some(control) = mode.control.as_mut() {
                            mode.phase_read = Some(phase_reader(control, &mut continuation.phase)?);
                        }
                        mode.deadline = mode.running_deadline.checked_add(pause).ok_or("acknowledged pause deadline overflow")?;
                        let now = sync::now_ns(session.origin)?;
                        let mut retained = continuation.maintenance.take().ok_or("authenticated continuation lost its original maintained clock")?;
                        retained.reconnect(now)?;
                        maintenance = Some(retained);
                        let phase_round = continuation.phase.active.as_ref().map(|active| active.round);
                        emit(events, LiveEvent::RecoveryReady { epoch: session.epoch, attempt: sync::RESUME_ATTEMPT, phase_round })?;
                        if let Some(active) = &continuation.phase.active {
                            emit(events, LiveEvent::PhaseRebound { epoch: session.epoch, round: active.round, attempt: 1, deadline: active.deadline.into_std() })?;
                        }
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
                                if (mode.recovering || continuation.phase.active.as_ref().is_some_and(|phase| phase.markers[session.player.index()].is_some()))
                                    && matches!(input, DuoInput::Hit(_)) {
                                    return Err("performing Hit is disabled while original audio catches up".into());
                                }
                                ingest_local(session, input)?;
                                if let DuoInput::Watermark { through, .. } = input {
                                    continuation.phase.progress_frame = continuation.phase.progress_frame.max(through.frames());
                                }
                                tokio::time::timeout_at(mode.deadline.min(phase_deadline), wire::send_live_input(send, written,
                                    &Input::Facts { epoch: session.epoch.0, facts: vec![Fact::from_input(input)] }))
                                    .await.map_err(|_| "fact write exceeded the round or recovery deadline")?
                                    .map_err(ExchangeError::from)?;
                            }
                            LiveCommand::End if !mode.recovering => {
                                let count = session.counts[session.player.index()] as u64;
                                session.end_live(session.player, count, session.prepared.final_through)?;
                                tokio::time::timeout_at(mode.deadline.min(phase_deadline), wire::send_live_input(send, written,
                                    &Input::End { epoch: session.epoch.0, fact_count: count, final_through: session.prepared.final_through }))
                                    .await.map_err(|_| "End write exceeded the round deadline")?.map_err(ExchangeError::from)?;
                                send.finish().map_err(|_| "finish live input stream failed")?;
                                if let Some(control) = mode.control.as_mut() {
                                    continuation.phase.active = None;
                                    phase_send(session, control, &mut continuation.phase, 0, mode.deadline.min(phase_deadline),
                                        wire::PhaseControl::Ended { owner_count: count }).await?;
                                }
                            }
                            LiveCommand::PhaseObserved { epoch, attempt: 0, .. }
                            | LiveCommand::PhaseFrozen { epoch, attempt: 0, .. }
                            | LiveCommand::PhaseArmed { epoch, attempt: 0, .. } if epoch == session.epoch && continuation.phase.attempt == 1 => {},
                            command @ (LiveCommand::PhaseSource { .. } | LiveCommand::PhaseObserved { .. }
                                | LiveCommand::PhaseFrozen { .. } | LiveCommand::PhaseArmed { .. }) if !mode.recovering => {
                                let control = mode.control.as_mut().ok_or("phase command has no live control stream")?;
                                let clock = maintenance.as_mut().ok_or("phase command has no maintained clock")?;
                                phase_command(session, (control, send, written), &mut continuation.phase, clock, command).await?;
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
                        if session.ended.iter().all(|ended| *ended)
                            && (mode.control.is_none() || continuation.phase.peer_end_count.is_some()) { break None; }
                    }
                    _ = tokio::time::sleep_until(peer_progressed + session::IDLE), if !session.ended[peer.index()] && deferred_end.is_none() => return Err(ExchangeError::Recoverable("reliable peer progress deadline")),
                }
            }
        };
        if let Some(message) = message {
            match message {
                Input::PhasePaused {
                    epoch,
                    round,
                    attempt,
                    fact_count,
                } if epoch == session.epoch.0 && !mode.recovering => {
                    let active = continuation
                        .phase
                        .active
                        .as_mut()
                        .ok_or("phase FIFO marker has no active round")?;
                    if round != active.round
                        || attempt != active.attempt
                        || active.markers[peer.index()].is_some()
                        || !matches!(active.step, PhaseStep::Pausing | PhaseStep::GateAck)
                        || fact_count != session.counts[peer.index()] as u64
                    {
                        return Err(
                            "phase FIFO marker differs from the complete accepted peer prefix"
                                .into(),
                        );
                    }
                    active.markers[peer.index()] = Some(fact_count);
                    let pending = if active.markers.iter().all(Option::is_some) {
                        active.pending_schedule.take()
                    } else {
                        None
                    };
                    let mut control = mode
                        .control
                        .take()
                        .ok_or("phase marker has no control sender")?;
                    let clock = maintenance
                        .as_mut()
                        .ok_or("phase marker has no actual maintained clock")?;
                    if let Some(message) = pending {
                        phase_control(
                            session,
                            &mut control,
                            events,
                            &mut continuation.phase,
                            clock,
                            mode,
                            Control::Phase {
                                epoch,
                                round,
                                attempt,
                                message,
                            },
                        )
                        .await?;
                    } else {
                        phase_try_schedule(session, &mut control, &mut continuation.phase, clock)
                            .await?;
                    }
                    mode.control = Some(control);
                }
                Input::Facts { epoch, facts } if epoch == session.epoch.0 => {
                    if continuation
                        .phase
                        .active
                        .as_ref()
                        .is_some_and(|phase| phase.markers[peer.index()].is_some())
                        && (!continuation.phase.deferred_peer.is_empty()
                            || facts.iter().any(|fact| matches!(fact, Fact::Hit { .. })))
                    {
                        if continuation
                            .phase
                            .active
                            .as_ref()
                            .is_none_or(|phase| phase.step != PhaseStep::AwaitLive)
                            || continuation
                                .phase
                                .deferred_peer
                                .len()
                                .checked_add(facts.len())
                                .is_none_or(|count| {
                                    count > EVENT_CAPACITY * 64
                                        || count > MAX_FACTS - session.replay.facts().len()
                                })
                        {
                            return Err(
                                "performing peer Hit crosses the original-source phase gate".into(),
                            );
                        }
                        continuation.phase.deferred_peer.extend(facts);
                    } else if mode.recovering
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
                    let eof_deadline = continuation
                        .phase
                        .active
                        .as_ref()
                        .map_or(mode.deadline, |phase| mode.deadline.min(phase.deadline));
                    tokio::time::timeout_at(eof_deadline, wire::ensure_eof(recv))
                        .await
                        .map_err(|_| "peer End EOF exceeded the live or recovery deadline")??;
                    if continuation.phase.active.as_ref().is_some_and(|phase| {
                        phase.step == PhaseStep::AwaitLive && phase.markers[peer.index()].is_some()
                    }) {
                        if continuation
                            .phase
                            .deferred_end
                            .replace((fact_count, final_through))
                            .is_some()
                        {
                            return Err("duplicate peer End during source phase handoff".into());
                        }
                    } else if mode.recovering {
                        if deferred_end.replace((fact_count, final_through)).is_some() {
                            return Err("duplicate peer End during gate handoff".into());
                        }
                    } else {
                        session.end_live(peer, fact_count, final_through)?;
                        if continuation
                            .phase
                            .peer_end_count
                            .is_some_and(|count| count != fact_count)
                        {
                            return Err(
                                "peer phase control Ended differs from actual Input End count"
                                    .into(),
                            );
                        }
                        continuation.phase.active = None;
                    }
                }
                _ => return Err("unexpected live input state, attempt or epoch".into()),
            }
            peer_progressed = Instant::now();
        }
    }
    Ok(None)
    }.await;
    if maintenance.is_some() {
        continuation.maintenance = maintenance;
    }
    result
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
    fn phase_frozen_snapshot_preserves_original_raw_cursor_and_exact_owner_fence() {
        let mut session = fixture();
        session.origin = Instant::now() - Duration::from_secs(1);
        let own = hit(9, PlayerId::P1, 0, 10_000);
        ingest_local(&mut session, own).unwrap();
        let at = std::time::Instant::now() - Duration::from_millis(5);
        let publication = PhasePublication {
            sequence: 10,
            position_seconds_bits: (10_000.0_f64 / 48_000.0).to_bits(),
            published_between: [at - Duration::from_micros(50), at],
        };
        let frozen = PhaseFrozen {
            replay: session.replay.clone(),
            paused_frame: SongTime::from_frames(10_000),
            source_generation: 11,
            source_id: 21,
            paused_at: at + Duration::from_millis(1),
            publication,
        };
        let raw = validate_phase_frozen(&session, &frozen, (11, 21), None).unwrap();
        assert_eq!(raw.position_seconds_bits, publication.position_seconds_bits);
        assert_eq!(raw.sequence, 10);
        assert!(validate_phase_frozen(&session, &frozen, (12, 21), None).is_err());
        assert!(
            validate_phase_frozen(
                &session,
                &frozen,
                (11, 21),
                Some(wire::PhasePublication {
                    sequence: 11,
                    ..raw
                })
            )
            .is_err()
        );
        let mut wrong = frozen.clone();
        wrong.publication.position_seconds_bits = f64::NAN.to_bits();
        assert!(validate_phase_frozen(&session, &wrong, (11, 21), None).is_err());
        let mut stale = frozen.clone();
        stale.publication.published_between[0] = at - Duration::from_millis(51);
        assert!(validate_phase_frozen(&session, &stale, (11, 21), None).is_err());
        let before_new_owner = frozen.clone();
        let next = DuoInput::Watermark {
            epoch: session.epoch,
            player: session.player,
            through: SongTime::from_frames(10_001),
        };
        ingest_local(&mut session, next).unwrap();
        assert!(
            validate_phase_frozen(&session, &before_new_owner, (11, 21), None).is_err(),
            "a pause marker cannot discard the already accepted next Watermark"
        );
    }

    #[test]
    #[ignore = "explicit real host loopback required"]
    fn active_clock_maintenance_refreshes_beside_the_original_fact_fifo() {
        session::runtime().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let (host_endpoint, invitation) = listen("127.0.0.1:0".parse().unwrap()).unwrap();
                let mut owned_endpoint = None;
                let guest = connect_owned(&invitation, &mut owned_endpoint);
                let host = async { host_endpoint.accept().await.unwrap().await.unwrap() };
                let (guest, host_connection) = tokio::join!(guest, host);
                let (guest_endpoint, guest_connection) = guest.unwrap();
                let (host_inputs, guest_inputs) = tokio::join!(
                    session::open_inputs(&host_connection, PlayerId::P1, 9),
                    session::open_inputs(&guest_connection, PlayerId::P2, 9)
                );
                let mut host_inputs = host_inputs.unwrap();
                let mut guest_inputs = guest_inputs.unwrap();
                let mut host_state = fixture();
                let mut guest_state = fixture();
                guest_state.player = PlayerId::P2;
                let (host_sender, mut host_commands) = mpsc::channel(COMMAND_CAPACITY);
                let (guest_sender, mut guest_commands) = mpsc::channel(COMMAND_CAPACITY);
                let (host_events, host_receiver) = std_mpsc::sync_channel(EVENT_CAPACITY);
                let (guest_events, guest_receiver) = std_mpsc::sync_channel(EVENT_CAPACITY);
                let deadline = Instant::now() + Duration::from_secs(6);
                let mut host_mode = ExchangeControl {
                    control: None,
                    phase_read: None,
                    gate: None,
                    signals: None,
                    deadline,
                    running_deadline: deadline,
                    recovering: false,
                };
                let mut guest_mode = ExchangeControl {
                    control: None,
                    phase_read: None,
                    gate: None,
                    signals: None,
                    deadline,
                    running_deadline: deadline,
                    recovering: false,
                };
                let mut host_continuation = Continuation {
                    invitation: invitation.clone(),
                    capability: [0; 32],
                    used: true,
                    candidates: 0,
                    maintenance_round: 0,
                    maintenance: None,
                    phase: PhaseState::default(),
                    pending: JoinSet::new(),
                };
                let mut guest_continuation = Continuation {
                    invitation,
                    capability: [0; 32],
                    used: true,
                    candidates: 0,
                    maintenance_round: 0,
                    maintenance: None,
                    phase: PhaseState::default(),
                    pending: JoinSet::new(),
                };
                let host = exchange(
                    &mut host_state,
                    (&host_connection, &host_endpoint),
                    &mut host_inputs,
                    &mut host_commands,
                    &host_events,
                    &mut host_continuation,
                    &mut host_mode,
                );
                let guest = exchange(
                    &mut guest_state,
                    (&guest_connection, &guest_endpoint),
                    &mut guest_inputs,
                    &mut guest_commands,
                    &guest_events,
                    &mut guest_continuation,
                    &mut guest_mode,
                );
                let producer = async {
                    for (sender, player) in
                        [(&host_sender, PlayerId::P1), (&guest_sender, PlayerId::P2)]
                    {
                        sender
                            .send(LiveCommand::Fact(hit(9, player, 0, 10_000)))
                            .await
                            .unwrap();
                        sender
                            .send(LiveCommand::Fact(DuoInput::Watermark {
                                epoch: SessionEpoch(9),
                                player,
                                through: SongTime::from_frames(-2_400),
                            }))
                            .await
                            .unwrap();
                    }
                    // More than the unchanged two-second ClockSync sample age
                    tokio::time::sleep(Duration::from_millis(3_200)).await;
                    for (sender, player) in
                        [(&host_sender, PlayerId::P1), (&guest_sender, PlayerId::P2)]
                    {
                        sender
                            .send(LiveCommand::Fact(DuoInput::Watermark {
                                epoch: SessionEpoch(9),
                                player,
                                through: SongTime::from_frames(68_881),
                            }))
                            .await
                            .unwrap();
                        sender.send(LiveCommand::End).await.unwrap();
                    }
                };
                let (host, guest, ()) = tokio::join!(host, guest, producer);
                assert!(matches!(host, Ok(None)), "host: {:?}", host.as_ref().err());
                assert!(
                    matches!(guest, Ok(None)),
                    "guest: {:?}",
                    guest.as_ref().err()
                );
                for (state, receiver, continuation) in [
                    (&host_state, host_receiver, &host_continuation),
                    (&guest_state, guest_receiver, &guest_continuation),
                ] {
                    assert_eq!(state.epoch, SessionEpoch(9));
                    assert_eq!(state.ended, [true; 2]);
                    assert_eq!(state.counts, [3, 3]);
                    state.verify_replay(&state.replay).unwrap();
                    for player in [PlayerId::P1, PlayerId::P2] {
                        let owned: Vec<_> = state
                            .replay
                            .facts()
                            .iter()
                            .copied()
                            .filter(|fact| session::seat(*fact) == player)
                            .collect();
                        assert_eq!(
                            owned,
                            [
                                hit(9, player, 0, 10_000),
                                DuoInput::Watermark {
                                    epoch: SessionEpoch(9),
                                    player,
                                    through: SongTime::from_frames(-2_400)
                                },
                                DuoInput::Watermark {
                                    epoch: SessionEpoch(9),
                                    player,
                                    through: SongTime::from_frames(68_881)
                                }
                            ]
                        );
                    }
                    let samples: Vec<_> = receiver
                        .try_iter()
                        .filter_map(|event| match event {
                            LiveEvent::ClockMaintained {
                                epoch: SessionEpoch(9),
                                round,
                                exchange,
                            } => Some((round, exchange)),
                            _ => None,
                        })
                        .collect();
                    assert!(samples.len() >= 3, "actual exchanges: {}", samples.len());
                    for (index, (round, exchange)) in samples.iter().enumerate() {
                        assert_eq!(*round, index as u64 + 1);
                        assert!(exchange.guest_send_ns <= exchange.guest_receive_ns);
                        assert!(exchange.host_receive_ns <= exchange.host_send_ns);
                    }
                    assert_eq!(continuation.maintenance_round, samples.last().unwrap().0);
                    assert!(
                        continuation.used,
                        "refresh does not reset authenticated reconnect budget"
                    );
                }
                host_endpoint.close(0u32.into(), b"QA complete");
                guest_endpoint.close(0u32.into(), b"QA complete");
            })
            .await
            .expect("bounded active maintenance and fact FIFO");
        });
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
            ExchangeError::from(crate::clock::ClockError::Stale),
            ExchangeError::Recoverable("clock maintenance sample is stale")
        ));
        for error in [
            crate::clock::ClockError::NonMonotonic,
            crate::clock::ClockError::WrongEpoch,
            crate::clock::ClockError::Overflow,
        ] {
            assert!(matches!(
                ExchangeError::from(error),
                ExchangeError::Terminal(_)
            ));
        }
        assert!(matches!(
            ExchangeError::from("clock maintenance sample is stale"),
            ExchangeError::Terminal(_)
        ));
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
    #[ignore = "explicit real phase-reader loopback required"]
    fn phase_reader_preserves_requested_close_and_invalid_controls_remain_terminal() {
        session::runtime().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(15), async {
                for case in [
                    "requested-close",
                    "ordinary-close",
                    "invalid-json",
                    "budget",
                    "incomplete-eof",
                ] {
                    let (host_endpoint, invitation) =
                        listen("127.0.0.1:0".parse().unwrap()).unwrap();
                    let mut owned_endpoint = None;
                    let guest = connect_owned(&invitation, &mut owned_endpoint);
                    let host = async { host_endpoint.accept().await.unwrap().await.unwrap() };
                    let (guest, host_connection) = tokio::join!(guest, host);
                    let (guest_endpoint, guest_connection) = guest.unwrap();
                    let (mut send, _recv) = guest_connection.open_bi().await.unwrap();
                    if case == "budget" {
                        wire::send_control(
                            &mut send,
                            &mut 0,
                            &Control::Phase {
                                epoch: 9,
                                round: 1,
                                attempt: 0,
                                message: wire::PhaseControl::Armed {},
                            },
                        )
                        .await
                        .unwrap();
                    } else {
                        let prefix: &[u8] = if case == "invalid-json" {
                            &[0, 0, 0, 1, b'{']
                        } else {
                            &[0, 0]
                        };
                        send.write_all(prefix).await.unwrap();
                    }
                    let host_stream = host_connection.accept_bi().await.unwrap();
                    let mut control = ControlIo::new(host_stream);
                    let mut phase = PhaseState::default();
                    if case == "budget" {
                        for _ in 0..16 {
                            phase.received.reserve(1, &wire::PhaseControl::Armed {}, 5).unwrap();
                        }
                    }
                    let reading = phase_reader(&mut control, &mut phase).unwrap();
                    match case {
                        "requested-close" => guest_connection.close(
                            RECOVERY_REQUESTED.into(), b"phase-reader QA requested close",
                        ),
                        "ordinary-close" => guest_connection.close(
                            7_u32.into(), b"phase-reader QA ordinary close",
                        ),
                        "incomplete-eof" => send.finish().unwrap(),
                        _ => {}
                    }
                    let (_, _, result) = reading.await;
                    if case == "requested-close" {
                        assert!(matches!(&result,
                            Err(wire::LiveIoError::Transport(
                                quinn::ConnectionError::ApplicationClosed(close)
                            )) if close.error_code.into_inner() == u64::from(RECOVERY_REQUESTED)
                        ));
                    } else if case != "ordinary-close" {
                        assert!(matches!(&result, Err(wire::LiveIoError::Invalid(_))));
                    }
                    if case == "invalid-json" {
                        assert!(matches!(&result,
                            Err(wire::LiveIoError::Invalid(error)) if error == "invalid phase control JSON"
                        ));
                    } else if case == "budget" {
                        assert!(matches!(&result,
                            Err(wire::LiveIoError::Invalid(error)) if error == "phase per-round message budget exceeded"
                        ));
                    }
                    let error = match result {
                        Err(error) => ExchangeError::from(error),
                        Ok(_) => panic!("incomplete or malformed control unexpectedly accepted"),
                    };
                    if case == "requested-close" {
                        assert!(matches!(error, ExchangeError::Recoverable(
                            "authenticated peer requested connection maintenance"
                        )));
                    } else {
                        assert!(matches!(error, ExchangeError::Terminal(_)));
                    }
                    host_endpoint.close(0_u32.into(), b"phase-reader QA complete");
                    guest_endpoint.close(0_u32.into(), b"phase-reader QA complete");
                }
            })
            .await
            .expect("real phase-reader classification check stays within fifteen seconds");
        });
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
                        Ok(LiveEvent::RecoveryPausing { epoch, attempt, .. }) => {
                            host.try_send(LiveCommand::RecoveryFrozen {
                                epoch,
                                attempt,
                                snapshot: Box::new(RecoveryFrozen {
                                    replay: replay.as_ref().unwrap().clone(),
                                    paused_frame: SongTime::from_frames(10_000),
                                    source_generation: 1,
                                    source_id: 1,
                                    paused_at: std::time::Instant::now(),
                                    prior_phase_pause: None,
                                    publication: PhasePublication {
                                        sequence: 1,
                                        position_seconds_bits: (10_000.0_f64 / 48_000.0).to_bits(),
                                        published_between: [std::time::Instant::now(); 2],
                                    },
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
                    control.send(Control::ResumeHello { protocol_version: PROTOCOL_VERSION, epoch: epoch.0, player: 2, identity, attempt: 1, pause_frame: 10_000, publication: wire::PhasePublication { sequence: 1, position_seconds_bits: (10000.0_f64/48000.0).to_bits(), publication_before_ns: 1, publication_after_ns: 2 }, phase: None, source_generation: 2, source_id: 2, owner_count: tape.len() as u64, started: true, ended: false, capability }).await.unwrap();
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
    fn phase_gate_keeps_later_watermark_behind_hit_and_bounds_slow_end_eof() {
        session::runtime().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                let (host_endpoint, invitation) = listen("127.0.0.1:0".parse().unwrap()).unwrap();
                let mut owned_endpoint = None;
                let guest = connect_owned(&invitation, &mut owned_endpoint);
                let host = async { host_endpoint.accept().await.unwrap().await.unwrap() };
                let (guest, host_connection) = tokio::join!(guest, host);
                let (_, guest_connection) = guest.unwrap();
                let (host_inputs, guest_inputs) = tokio::join!(
                    session::open_inputs(&host_connection, PlayerId::P1, 9),
                    session::open_inputs(&guest_connection, PlayerId::P2, 9)
                );
                let mut host_inputs = host_inputs.unwrap();
                let mut guest_inputs = guest_inputs.unwrap();
                let mut state = fixture();
                state
                    .ingest(PlayerId::P2, Fact::Watermark { through: -2400 })
                    .unwrap();
                let (_sender, mut commands) = mpsc::channel(COMMAND_CAPACITY);
                let (events, receiver) = std_mpsc::sync_channel(EVENT_CAPACITY);
                let started = Instant::now();
                let deadline = started + Duration::from_millis(500);
                let mut active = new_phase_round(
                    1,
                    wire::PhaseAnchor {
                        clock_round: 1,
                        guest_send_ns: 1,
                        host_receive_ns: 2,
                        host_send_ns: 3,
                        guest_receive_ns: 4,
                    },
                    5,
                    deadline,
                );
                active.step = PhaseStep::AwaitLive;
                active.markers[PlayerId::P2.index()] = Some(1);
                let phase = PhaseState {
                    active: Some(active),
                    ..PhaseState::default()
                };
                let mut continuation = Continuation {
                    invitation,
                    capability: [0; 32],
                    used: true,
                    candidates: 0,
                    maintenance_round: 0,
                    maintenance: None,
                    phase,
                    pending: JoinSet::new(),
                };
                let mut mode = ExchangeControl {
                    control: None,
                    phase_read: None,
                    gate: None,
                    signals: None,
                    deadline: started + Duration::from_secs(4),
                    running_deadline: started + Duration::from_secs(4),
                    recovering: false,
                };
                let first = Fact::Hit {
                    seq: 0,
                    frame: 10_000,
                };
                let later = Fact::Watermark { through: 68_881 };
                let peer = async {
                    for fact in [first, later] {
                        wire::send_live_input(
                            &mut guest_inputs.send,
                            &mut guest_inputs.written,
                            &Input::Facts {
                                epoch: 9,
                                facts: vec![fact],
                            },
                        )
                        .await
                        .unwrap();
                    }
                    wire::send_live_input(
                        &mut guest_inputs.send,
                        &mut guest_inputs.written,
                        &Input::End {
                            epoch: 9,
                            fact_count: 3,
                            final_through: 68_881,
                        },
                    )
                    .await
                    .unwrap();
                    // Leave FIN absent so the actual EOF read must honor the shorter active deadline
                    tokio::time::sleep_until(started + Duration::from_millis(700)).await;
                    assert!(
                        matches!(receiver.try_recv(), Err(std_mpsc::TryRecvError::Empty)),
                        "a later watermark cannot publish ahead of its parked Hit"
                    );
                };
                let (result, ()) = tokio::join!(
                    exchange(
                        &mut state,
                        (&host_connection, &host_endpoint),
                        &mut host_inputs,
                        &mut commands,
                        &events,
                        &mut continuation,
                        &mut mode
                    ),
                    peer
                );
                let error = match result {
                    Err(error) => error.into_message(),
                    Ok(_) => panic!("missing FIN unexpectedly completed the active phase"),
                };
                assert_eq!(error, "peer End EOF exceeded the live or recovery deadline");
                assert!(
                    Instant::now() < started + Duration::from_secs(1),
                    "blocking EOF cannot spend the four-second whole-round deadline"
                );
                assert_eq!(state.counts, [0, 1]);
                assert_eq!(state.next_seq, [0, 0]);
                assert_eq!(continuation.phase.deferred_peer, [first, later]);
                accept_peer_batch(
                    &mut state,
                    PlayerId::P2,
                    &std::mem::take(&mut continuation.phase.deferred_peer),
                    &events,
                )
                .unwrap();
                assert_eq!(state.counts, [0, 3]);
                assert_eq!(state.next_seq, [0, 1]);
                state.verify_replay(&state.replay).unwrap();
                host_endpoint.close(0u32.into(), b"QA complete");
                if let Some(endpoint) = owned_endpoint {
                    endpoint.close(0u32.into(), b"QA complete");
                }
            })
            .await
            .expect("bounded actual Phase FIFO and slow EOF regression");
        });
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
                let control_guest = async {
                    let mut control = ControlIo::new(guest_connection.open_bi().await.unwrap());
                    wire::send_phase_control(&mut control.send, &mut wire::PhaseBudget::default(),
                        &Control::Phase { epoch: 9, round: 0, attempt: 1, message: wire::PhaseControl::Ended { owner_count: 3 } }).await.unwrap();
                    control
                };
                let control_host = async { ControlIo::new(host_connection.accept_bi().await.unwrap()) };
                let (_guest_control, host_control) = tokio::join!(control_guest, control_host);
                let (gate_sender, gate_receiver) = oneshot::channel();
                let mut mode = ExchangeControl { control: None, phase_read: None, gate: Some(Box::pin(async { gate_receiver.await.map_err(|_| "test gate cancelled".to_owned()) })), signals: None, deadline: Instant::now() + Duration::from_secs(4), running_deadline: Instant::now() + Duration::from_secs(4), recovering: true };
                let preserved_maintenance = sync::ClockMaintenance::new(
                    state.epoch,
                    state.player,
                    0,
                    sync::now_ns(state.origin).unwrap(),
                ).unwrap();
                let mut continuation = Continuation { invitation, capability: [0;32], used: true, candidates: 0, maintenance_round: 0, maintenance: Some(preserved_maintenance), phase: PhaseState { attempt: 1, ..PhaseState::default() }, pending: JoinSet::new() };
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
                    assert!(gate_sender.send(ResumeCompletion {
                        pause: Duration::from_millis(10),
                        control: host_control,
                        host_common_ns: 1,
                        common_frame: 0,
                        start_uncertainties_ns: [0; 2],
                        phase_deadline: None,
                    }).is_ok());
                    let mut actual = Vec::new();
                    loop {
                        match receiver.try_recv() {
                            Ok(LiveEvent::PeerFacts(facts)) => actual.extend(facts),
                            Ok(LiveEvent::RecoveryReady { epoch: SessionEpoch(9), attempt: 1, phase_round: None }) => break,
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
    #[test]
    fn reconnect_metadata_merges_complete_owner_floors_without_reopening_other_rounds() {
        let publication = wire::PhasePublication {
            sequence: 2,
            position_seconds_bits: (10_000.0_f64 / 48_000.0).to_bits(),
            publication_before_ns: 10,
            publication_after_ns: 11,
        };
        let descriptor = wire::PhaseResume {
            round: 1,
            host_deadline_ns: 30_000_000_000,
            host_point_ns: 250_000_000,
            stage: wire::PhaseProofStage::Correction,
            corrections: 1,
            sealed: false,
            anchor: wire::PhaseAnchor {
                clock_round: 1,
                guest_send_ns: 1,
                host_receive_ns: 2,
                host_send_ns: 3,
                guest_receive_ns: 4,
            },
            sources: [(11, 21), (12, 22)],
            previous: [Some(publication); 2],
            markers: [Some(1); 2],
        };
        let mut state = PhaseState {
            round: 1,
            sealed: Some(wire::PhaseResume {
                sealed: true,
                ..descriptor
            }),
            ..Default::default()
        };
        let own = RemoteFreeze {
            frame: 10_000,
            publication,
            phase: None,
            generation: 11,
            source_id: 21,
            count: 1,
        };
        let peer = RemoteFreeze {
            frame: 10_000,
            publication,
            phase: Some(descriptor),
            generation: 12,
            source_id: 22,
            count: 1,
        };
        let merged = reconcile_phase_resume(&state, PlayerId::P1, own, peer)
            .unwrap()
            .unwrap();
        assert_eq!(merged.host_deadline_ns, descriptor.host_deadline_ns);
        assert_eq!(merged.corrections, 1);
        assert_eq!(merged.previous, [Some(publication); 2]);
        assert!(
            reconcile_phase_resume(
                &state,
                PlayerId::P1,
                own,
                RemoteFreeze {
                    source_id: 23,
                    ..peer
                }
            )
            .is_err()
        );
        assert!(
            reconcile_phase_resume(&state, PlayerId::P1, own, RemoteFreeze { count: 0, ..peer })
                .is_err()
        );
        assert!(
            reconcile_phase_resume(
                &state,
                PlayerId::P1,
                own,
                RemoteFreeze {
                    phase: Some(wire::PhaseResume {
                        round: 2,
                        ..descriptor
                    }),
                    ..peer
                }
            )
            .is_err()
        );
        state.sealed = None;
        assert!(reconcile_phase_resume(&state, PlayerId::P1, own, peer).is_err());
        let changed = wire::PhasePublication {
            position_seconds_bits: f64::from_bits(publication.position_seconds_bits)
                .next_up()
                .to_bits(),
            ..publication
        };
        assert!(merge_phase_floor(Some(publication), Some(changed)).is_err());
        let rewind = wire::PhasePublication {
            sequence: 3,
            position_seconds_bits: 0.0_f64.to_bits(),
            publication_before_ns: 12,
            publication_after_ns: 13,
        };
        assert!(merge_phase_floor(Some(publication), Some(rewind)).is_err());
    }
    #[test]
    fn prior_waiting_resume_pause_is_disjoint_once_and_sealed_host_does_not_double_count() {
        let start = std::time::Instant::now();
        let resumed = start + Duration::from_secs(2);
        let new_pause = resumed + Duration::from_millis(100);
        let origin = start - Duration::from_secs(1);
        let mut active = new_phase_round(
            1,
            wire::PhaseAnchor {
                clock_round: 1,
                guest_send_ns: 1,
                host_receive_ns: 2,
                host_send_ns: 3,
                guest_receive_ns: 4,
            },
            250_000_000,
            Instant::now() + RECOVERY_TIMEOUT,
        );
        active.step = PhaseStep::Arming;
        let held =
            prior_phase_pause_extension(origin, Some(&active), Some([start, resumed]), new_pause)
                .unwrap();
        assert_eq!(held, Duration::from_secs(2));
        assert_eq!(
            prior_phase_pause_extension(origin, None, Some([start, resumed]), new_pause).unwrap(),
            Duration::ZERO
        );
        assert_eq!(
            prior_phase_pause_extension(origin, Some(&active), None, new_pause).unwrap(),
            Duration::ZERO
        );
        assert!(
            prior_phase_pause_extension(
                origin,
                Some(&active),
                Some([start, new_pause + Duration::from_millis(1)]),
                new_pause
            )
            .is_err()
        );
        assert!(
            prior_phase_pause_extension(origin, Some(&active), Some([resumed, start]), new_pause)
                .is_err()
        );
        active.step = PhaseStep::Sampling;
        assert!(
            prior_phase_pause_extension(origin, Some(&active), Some([start, resumed]), new_pause)
                .is_err()
        );
    }
    #[test]
    fn sealed_host_and_awaitlive_guest_reconcile_distinct_raw_floors_and_exact_fifo_markers() {
        // Metadata fixtures only: no claim that an actual QUIC peer is parked at AwaitLive
        let publication = |sequence, frame: i64, at| wire::PhasePublication {
            sequence,
            position_seconds_bits: (frame as f64 / 48_000.0).to_bits(),
            publication_before_ns: at,
            publication_after_ns: at + 1,
        };
        let original = [publication(10, 10_000, 100), publication(12, 10_100, 110)];
        let accepted = [publication(20, 11_000, 200), publication(22, 11_100, 210)];
        let frozen = [publication(30, 12_000, 300), publication(32, 12_100, 310)];
        let sources = [(11, 21), (12, 22)];
        let anchor = wire::PhaseAnchor {
            clock_round: 2,
            guest_send_ns: 1,
            host_receive_ns: 2,
            host_send_ns: 3,
            guest_receive_ns: 4,
        };
        let mut active = new_phase_round(3, anchor, 250_000_000, Instant::now() + RECOVERY_TIMEOUT);
        active.step = PhaseStep::AwaitLive;
        active.verification = true;
        active.host_deadline_ns = 30_000_000_000;
        active.markers = [Some(3), Some(4)];
        let guest = PhaseState {
            round: 3,
            corrections: 1,
            sources: sources.map(Some),
            previous: original.map(Some),
            active: Some(active),
            ..Default::default()
        };
        let unresolved = phase_description(&guest, guest.active.as_ref().unwrap(), false).unwrap();
        let sealed = wire::PhaseResume {
            sealed: true,
            previous: accepted.map(Some),
            ..unresolved
        };
        let host = PhaseState {
            round: 3,
            corrections: 1,
            sources: sources.map(Some),
            previous: accepted.map(Some),
            sealed: Some(sealed),
            ..Default::default()
        };
        let host_freeze = RemoteFreeze {
            frame: 12_000,
            publication: frozen[0],
            phase: None,
            generation: sources[0].0,
            source_id: sources[0].1,
            count: 5,
        };
        let guest_freeze = RemoteFreeze {
            frame: 12_100,
            publication: frozen[1],
            phase: Some(unresolved),
            generation: sources[1].0,
            source_id: sources[1].1,
            count: 6,
        };
        let selected = reconcile_phase_resume(&host, PlayerId::P1, host_freeze, guest_freeze)
            .unwrap()
            .unwrap();
        assert!(selected.sealed);
        assert_eq!(selected.round, 3);
        assert_eq!(selected.host_deadline_ns, unresolved.host_deadline_ns);
        assert_eq!(selected.corrections, 1);
        assert_eq!(selected.sources, sources);
        assert_eq!(selected.markers, [Some(3), Some(4)]);
        assert_eq!(selected.previous, frozen.map(Some));
        let authoritative = RemoteFreeze {
            phase: Some(selected),
            ..host_freeze
        };
        assert_eq!(
            reconcile_phase_resume(&guest, PlayerId::P2, guest_freeze, authoritative).unwrap(),
            Some(selected)
        );
        assert_eq!(host.previous, accepted.map(Some));
        assert_eq!(guest.previous, original.map(Some));
        assert!(matches!(
            guest.active.as_ref().unwrap().step,
            PhaseStep::AwaitLive
        ));
        // Both direction consumers reject a contradictory accepted marker even with ample owner count
        let conflicting = RemoteFreeze {
            phase: Some(wire::PhaseResume {
                markers: [Some(3), Some(5)],
                ..unresolved
            }),
            ..guest_freeze
        };
        assert!(reconcile_phase_resume(&host, PlayerId::P1, host_freeze, conflicting).is_err());
        assert!(reconcile_phase_resume(&guest, PlayerId::P2, conflicting, authoritative).is_err());
        // A later raw cursor cannot authorize a freeze whose sequence predates the Host's accepted proof
        let below_sealed = RemoteFreeze {
            publication: wire::PhasePublication {
                sequence: 15,
                ..frozen[0]
            },
            ..host_freeze
        };
        assert!(reconcile_phase_resume(&host, PlayerId::P1, below_sealed, guest_freeze).is_err());
        assert!(
            reconcile_phase_resume(
                &guest,
                PlayerId::P2,
                guest_freeze,
                RemoteFreeze {
                    phase: Some(selected),
                    ..below_sealed
                }
            )
            .is_err()
        );
    }
    #[test]
    #[ignore = "explicit real phase/FIFO write loopback required"]
    fn partial_phase_writes_preserve_live_gate_and_frozen_fifo_before_recovery() {
        session::runtime().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(15), async {
                for case in ["ordinary-close", "invalid-domain", "live", "frozen-marker"] {
                    let (host_endpoint, invitation) = listen("127.0.0.1:0".parse().unwrap()).unwrap();
                    let mut owned_endpoint = None;
                    let guest = connect_owned(&invitation, &mut owned_endpoint);
                    let host = async { host_endpoint.accept().await.unwrap().await.unwrap() };
                    let (guest, host_connection) = tokio::join!(guest, host);
                    let (guest_endpoint, guest_connection) = guest.unwrap();
                    let (mut opening, mut control_receiving) = guest_connection.open_bi().await.unwrap();
                    opening.write_all(&[0]).await.unwrap();
                    let mut control = ControlIo::new(host_connection.accept_bi().await.unwrap());
                    let (mut input_opening, mut input_receiving) = guest_connection.open_bi().await.unwrap();
                    input_opening.write_all(&[0]).await.unwrap();
                    let (mut input_send, _input_recv) = host_connection.accept_bi().await.unwrap();
                    let mut state = fixture();
                    state.origin = Instant::now() - Duration::from_secs(2);
                    let own_hit = hit(9, PlayerId::P1, 0, 10_000);
                    let own_watermark = DuoInput::Watermark { epoch: state.epoch, player: state.player, through: SongTime::from_frames(10_000) };
                    ingest_local(&mut state, own_hit).unwrap();
                    ingest_local(&mut state, own_watermark).unwrap();
                    let prefix = state.replay.encode().unwrap();
                    let counts = state.counts;
                    let sequences = state.next_seq;
                    let mut input_written = 0;
                    let prior_facts = Input::Facts { epoch: 9, facts: state.replay.facts().iter().copied().map(Fact::from_input).collect() };
                    let sending_prior = wire::send_live_input(&mut input_send, &mut input_written, &prior_facts);
                    let reading_prior = async { wire::recv_live_input(&mut input_receiving, &mut 0).await.unwrap() };
                    let (prior_result, actual_prior) = tokio::join!(sending_prior, reading_prior);
                    prior_result.unwrap();
                    assert_eq!(actual_prior, prior_facts); // Real original Fact FIFO is received before a partial marker
                    let prior_input_bytes = input_written;
                    let anchor = wire::PhaseAnchor { clock_round: 1, guest_send_ns: 500_000_000, host_receive_ns: 501_000_000, host_send_ns: 502_000_000, guest_receive_ns: 503_000_000 };
                    let point = 1_000_000_000;
                    let deadline = sync::local_instant(state.origin, 30_500_000_000).unwrap();
                    let mut active = new_phase_round(1, anchor, point, deadline);
                    active.host_deadline_ns = 30_500_000_000;
                    let previous = [wire::PhasePublication { sequence: 1, position_seconds_bits: (9000.0_f64 / 48_000.0).to_bits(), publication_before_ns: 600_000_000, publication_after_ns: 600_001_000 }; 2];
                    let mut phase = PhaseState { round: 1, sources: [Some((11, 21)), Some((12, 22))], previous: previous.map(Some), ..Default::default() };
                    phase.rounds.begin(1, state.epoch, point, 500_000_000).unwrap();
                    if case == "live" {
                        // Declared pre-existing metadata proof, not Kira or an actual Playing/Source acknowledgment
                        let evidence = std::array::from_fn(|index| wire::PhaseEvidence {
                            generation: phase.sources[index].unwrap().0,
                            source_id: phase.sources[index].unwrap().1,
                            collected_at_ns: 1_120_000_000,
                            publications: [(10, 10_000, 900_000_000), (11, 11_000, 1_100_000_000)].map(|(sequence, frame, at)| wire::PhasePublication {
                                sequence, position_seconds_bits: (frame as f64 / 48_000.0).to_bits(), publication_before_ns: at, publication_after_ns: at + 1_000,
                            }).to_vec(),
                        });
                        let mut clock = crate::clock::ClockSync::new(state.epoch, Default::default()).unwrap();
                        clock.observe(anchor.exchange(9)).unwrap();
                        let proof = sync::source_phase_bounds(&mut clock, anchor.exchange(9), &evidence, sync::PhaseWindow {
                            epoch: state.epoch, round: 1, verification: false, attempt: 0, reconnecting: false,
                            sources: [(11, 21), (12, 22)], previous: previous.map(Some), end: state.prepared.end,
                            host_point_ns: point, host_now_ns: 1_130_000_000,
                        }).unwrap();
                        assert!(proof.within_guard());
                        active.bounds = Some(proof);
                        active.step = PhaseStep::GateAck;
                    } else { active.step = PhaseStep::Pausing; }
                    phase.active = Some(active);
                    let (events, received) = std_mpsc::sync_channel(EVENT_CAPACITY);
                    let mut maintained = sync::ClockMaintenance::new(state.epoch, state.player, 0, sync::now_ns(state.origin).unwrap()).unwrap();
                    let mut mode = ExchangeControl { control: None, phase_read: None, gate: None, signals: None, deadline, running_deadline: deadline, recovering: false };
                    host_connection.set_send_window(1); // Only this declared QA connection
                    if case == "ordinary-close" || case == "invalid-domain" {
                        guest_connection.close(7_u32.into(), b"ordinary QA close");
                        assert!(matches!(host_connection.closed().await, quinn::ConnectionError::ApplicationClosed(close) if close.error_code.into_inner() == 7));
                        let result = phase_send(&state, &mut control, &mut phase, if case == "invalid-domain" { 0 } else { 1 }, deadline, wire::PhaseControl::Pause {}).await;
                        assert!(matches!(result, Err(ExchangeError::Terminal(_))));
                    } else {
                        let result = if case == "live" {
                            let writing = phase_control(&mut state, &mut control, &events, &mut phase, &mut maintained, &mut mode, Control::Phase { epoch: 9, round: 1, attempt: 0, message: wire::PhaseControl::GateAck { anchor, host_point_ns: point, verification: false } });
                            let closing = async {
                                let mut first = [0];
                                control_receiving.read_exact(&mut first).await.unwrap();
                                guest_connection.close(RECOVERY_REQUESTED.into(), b"QA partial Live write");
                                first
                            };
                            let (result, first) = tokio::join!(writing, closing);
                            assert_eq!(first, [0]);
                            result
                        } else {
                            let now = std::time::Instant::now();
                            let snapshot = PhaseFrozen { replay: state.replay.clone(), paused_frame: SongTime::from_frames(10_000), source_generation: 11, source_id: 21, paused_at: now, publication: PhasePublication { sequence: 20, position_seconds_bits: (10000.0_f64 / 48_000.0).to_bits(), published_between: [now; 2] } };
                            let expected_bytes = 4 + serde_json::to_vec(&Input::PhasePaused { epoch: 9, round: 1, attempt: 0, fact_count: counts[0] as u64 }).unwrap().len() as u64;
                            let writing = phase_command(&state, (&mut control, &mut input_send, &mut input_written), &mut phase, &mut maintained, LiveCommand::PhaseFrozen { epoch: state.epoch, round: 1, attempt: 0, snapshot: Box::new(snapshot) });
                            let closing = async {
                                let mut first = [0];
                                input_receiving.read_exact(&mut first).await.unwrap();
                                guest_connection.close(RECOVERY_REQUESTED.into(), b"QA partial FIFO pause marker");
                                first
                            };
                            let (result, first) = tokio::join!(writing, closing);
                            assert_eq!(first, [0]);
                            assert_eq!(input_written, prior_input_bytes + expected_bytes);
                            result
                        };
                        assert!(matches!(result, Err(ExchangeError::Recoverable("authenticated peer requested connection maintenance"))));
                    }
                    assert_eq!(state.replay.encode().unwrap(), prefix);
                    assert_eq!(state.counts, counts);
                    assert_eq!(state.next_seq, sequences);
                    assert_eq!(phase.previous, previous.map(Some));
                    assert_eq!(phase.corrections, 0);
                    assert_eq!(phase.attempt, 0);
                    assert_eq!(phase.round, 1);
                    assert!(phase.sealed.is_none());
                    assert_eq!(phase.active.as_ref().unwrap().deadline, deadline);
                    assert!(matches!(received.try_recv(), Err(std_mpsc::TryRecvError::Empty)), "write failure must not emit Ready/phase_finish");
                    if case == "live" { assert!(matches!(phase.active.as_ref().unwrap().step, PhaseStep::GateAck)); }
                    if case == "frozen-marker" {
                        let active = phase.active.as_ref().unwrap();
                        assert!(active.own_frozen.is_some() && active.frozen[0].is_some());
                        assert_eq!(active.markers, [None; 2]); // No successful FIFO marker and no retransmit/rollback
                        let count = counts[0] as u64;
                        assert_eq!(active.frozen[0].unwrap().2, count);
                    }
                    host_endpoint.close(0_u32.into(), b"phase/FIFO write QA complete");
                    guest_endpoint.close(0_u32.into(), b"phase/FIFO write QA complete");
                }
            }).await.expect("causal phase/FIFO write controls have a fixed fifteen-second QA budget");
        });
    }
}
