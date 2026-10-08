//! Public production worker probe; Armed acknowledges software only, never an audio device

use cocobeat_net::{LiveCommand, LiveConfig, LiveEvent, LiveRole, LiveSession};
use cocobeat_replay::Replay;
use cocobeat_schema::{
    Anchor, CONTENT_SCHEMA_VERSION, CompiledChart, DuoInput, DuoRules, EnergySample, Hit,
    MusicAnalysis, PlayerId, SessionEpoch, SongTime,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

fn fixture(args: &[String]) -> bool {
    if args[1] == "fixture-pcm" {
        assert_eq!(args.len(), 5, "fixture-pcm SOURCE_OGG NEW_WAV REPEAT");
        let repeat: usize = args[4].parse().unwrap();
        assert!([12, 20].contains(&repeat));
        let mut pcm = Vec::new();
        cocobeat_media::decode_canonical(&args[2], 4800, |frames| {
            pcm.extend_from_slice(frames);
            Ok(())
        })
        .unwrap();
        let byte_len = (pcm.len() * repeat * 8) as u32;
        let mut writer = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[3])
            .unwrap();
        writer.write_all(b"RIFF").unwrap();
        writer.write_all(&(byte_len + 36).to_le_bytes()).unwrap();
        writer.write_all(b"WAVEfmt ").unwrap();
        writer.write_all(&16_u32.to_le_bytes()).unwrap();
        writer.write_all(&3_u16.to_le_bytes()).unwrap(); // IEEE float, preserving decoded PCM
        writer.write_all(&2_u16.to_le_bytes()).unwrap();
        writer.write_all(&48_000_u32.to_le_bytes()).unwrap();
        writer.write_all(&384_000_u32.to_le_bytes()).unwrap();
        writer.write_all(&8_u16.to_le_bytes()).unwrap();
        writer.write_all(&32_u16.to_le_bytes()).unwrap();
        writer.write_all(b"data").unwrap();
        writer.write_all(&byte_len.to_le_bytes()).unwrap();
        for _ in 0..repeat {
            for frame in &pcm {
                for sample in frame {
                    writer.write_all(&sample.to_le_bytes()).unwrap();
                }
            }
        }
        writer.sync_all().unwrap();
        println!(
            "{}",
            serde_json::json!({"frames":pcm.len()*repeat,"rate":48000,"channels":2,"method":"repeat exact production-decoded original synthetic PCM; no normalization or clipping"})
        );
        true
    } else if args[1] == "fixture-package" {
        assert_eq!(
            args.len(),
            5,
            "fixture-package QA_CANONICAL_OGG NEW_PACKAGE FRAMES"
        );
        let frames: u64 = args[4].parse().unwrap();
        assert!([57_600, 96_000].contains(&frames));
        let package = cocobeat_media::build_package(&args[2], frames, &args[3], |path, audio| {
            let mut squared = [0_f64; 2];
            let mut peak = [0_f32; 2];
            cocobeat_media::decode_canonical(path, frames, |block| {
                for frame in block { for index in 0..2 { squared[index] += f64::from(frame[index]).powi(2); peak[index] = peak[index].max(frame[index].abs()); } }
                Ok(())
            })?;
            Ok(cocobeat_media::PackageBuildInput {
                song_id: "live-original-synthetic-repeated".into(), importer_version: "qa-libvorbis-production-readback-v1".into(),
                analysis_version: "whole-file-measured-energy-v1".into(), chart_version: "manual-single-anchor-v1".into(),
                analysis: MusicAnalysis { capabilities: None,
            tempo_regions: Vec::new(),
            repetitions: Vec::new(),
            schema_version: CONTENT_SCHEMA_VERSION, audio_hash: audio.asset.blake3, beats: vec![], onsets: vec![], sections: vec![],
                    energy: vec![EnergySample { start: SongTime::from_frames(0), frames: frames as u32, rms: squared.map(|sum| (sum / frames as f64).sqrt() as f32), peak }],
                    diagnostics: "Original synthetic stereo PCM repeated, QA libvorbis encoding, measured whole-file energy; no MIR inference or production encoder admission".into() },
                chart: CompiledChart { schema_version: CONTENT_SCHEMA_VERSION, audio_hash: audio.asset.blake3, ruleset_id: "duo-watermark-v1".into(),
                    anchors: vec![Anchor { id: 1, song_time: SongTime::from_frames(frames as i64 / 2) }], sections: vec![] },
            })
        }).unwrap();
        println!(
            "{}",
            serde_json::json!({"canonical_frames":package.manifest.canonical_frames,"full_production_readback":true,"encoder_scope":"QA only; production encoder admission not claimed"})
        );
        true
    } else {
        false
    }
}

fn hit(epoch: SessionEpoch, player: PlayerId, seq: u64) -> DuoInput {
    DuoInput::Hit(Hit {
        epoch,
        player,
        seq,
        song_time: SongTime::from_frames(100 + seq as i64 * 40 + player.index() as i64 * 10),
    })
}

fn watermark(epoch: SessionEpoch, player: PlayerId, through: i64) -> DuoInput {
    DuoInput::Watermark {
        epoch,
        player,
        through: SongTime::from_frames(through),
    }
}

fn play(path: &Path, package: &Path) -> cocobeat_core::DuoEngine {
    let replay = Replay::load(path).unwrap();
    let package = cocobeat_media::validate_package(package).unwrap();
    let hash: String = package
        .manifest
        .package_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    replay
        .replay(
            &format!("package-blake3:{hash}"),
            &package.chart.ruleset_id,
            package.chart.anchors,
            DuoRules::default(),
        )
        .unwrap()
}

fn record_maintained_clock(
    receipts: &mut Vec<serde_json::Value>,
    clock: &mut Option<cocobeat_net::clock::ClockSync>,
    epoch: SessionEpoch,
    round: u64,
    exchange: cocobeat_net::clock::ClockExchange,
) {
    assert_eq!(exchange.epoch, epoch);
    assert!(round > 0);
    if let Some(previous) = receipts.last() {
        assert!(round > previous["round"].as_u64().unwrap());
    }
    let clock = clock.get_or_insert_with(|| {
        cocobeat_net::clock::ClockSync::new(epoch, Default::default()).unwrap()
    });
    let estimate = clock
        .observe(exchange)
        .expect("actual maintenance exchange must remain monotonic and bounded");
    receipts.push(serde_json::json!({
        "epoch":epoch.0,"round":round,
        "guest_send_ns":exchange.guest_send_ns,
        "host_receive_ns":exchange.host_receive_ns,
        "host_send_ns":exchange.host_send_ns,
        "guest_receive_ns":exchange.guest_receive_ns,
        "offset_ns":estimate.offset_ns,"uncertainty_ns":estimate.uncertainty_ns,
        "round_trip_min_ns":estimate.round_trip_min_ns,"round_trip_max_ns":estimate.round_trip_max_ns
    }));
}

fn run(scenario: &str, package: &Path, output: &Path) -> serde_json::Value {
    assert!(
        [
            "installed",
            "receive",
            "cancel-before-ready",
            "cancel-after-hit",
            "missing-armed",
            "wrong-player",
            "wrong-epoch"
        ]
        .contains(&scenario)
    );
    let success = scenario == "installed" || scenario == "receive";
    let package = package.to_path_buf();
    let output = output.to_path_buf();
    fs::create_dir(&output).unwrap();
    let invite = output.join("invite.json");
    let received = output.join("received");
    let host = LiveSession::spawn(LiveConfig {
        role: LiveRole::Host {
            package: package.clone(),
            bind: "127.0.0.1:0".parse().unwrap(),
            invite: invite.clone(),
        },
        output: output.join("host"),
    })
    .unwrap();
    let mut workers = vec![host];
    let mut prepared = [None, None];
    let mut scheduled = [false; 2];
    let mut started = [false; 2];
    let mut terminal = [false; 2];
    let mut accepted: [Vec<DuoInput>; 2] = [Vec::new(), Vec::new()];
    let mut peer: [Vec<DuoInput>; 2] = [Vec::new(), Vec::new()];
    let mut maintained_clocks: [Vec<serde_json::Value>; 2] = [Vec::new(), Vec::new()];
    let mut clock_sync: [Option<cocobeat_net::clock::ClockSync>; 2] = [None, None];
    let mut seq = [0_u64; 2];
    let mut ended = [false; 2];
    let mut action = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    while !terminal.iter().all(|done| *done) {
        assert!(
            Instant::now() < deadline,
            "bounded probe timeout: {scenario}"
        );
        for index in 0..workers.len() {
            while let Ok(event) = workers[index].try_recv() {
                println!("{} {event:?}", if index == 0 { "host" } else { "guest" });
                match event {
                    LiveEvent::Listening { .. } => assert_eq!(index, 0),
                    LiveEvent::Prepared {
                        epoch,
                        player,
                        canonical_frames,
                        final_through,
                        ..
                    } => {
                        assert_eq!(player.index(), index);
                        assert!(
                            canonical_frames >= 4800,
                            "probe facts require at least 4800 frames"
                        );
                        prepared[index] = Some((epoch, player, final_through));
                        if scenario != "cancel-before-ready" {
                            workers[index].try_send(LiveCommand::Ready).unwrap();
                        }
                    }
                    LiveEvent::Scheduled {
                        epoch, deadline, ..
                    } => {
                        assert_eq!(Some(epoch), prepared[index].map(|value| value.0));
                        assert!(
                            deadline.saturating_duration_since(Instant::now())
                                >= Duration::from_millis(100)
                        );
                        scheduled[index] = true;
                        if scenario != "missing-armed" || index == 0 {
                            workers[index].try_send(LiveCommand::Armed).unwrap();
                        }
                    }
                    LiveEvent::Started { epoch } => {
                        assert_eq!(Some(epoch), prepared[index].map(|value| value.0));
                        assert!(scheduled[index]);
                        started[index] = true;
                    }
                    LiveEvent::ClockMaintained {
                        epoch,
                        round,
                        exchange,
                    } => {
                        assert_eq!(Some(epoch), prepared[index].map(|value| value.0));
                        assert!(started[index] && !terminal[index]);
                        record_maintained_clock(
                            &mut maintained_clocks[index],
                            &mut clock_sync[index],
                            epoch,
                            round,
                            exchange,
                        );
                    }
                    LiveEvent::PeerFacts(facts) => peer[index].extend(facts),
                    LiveEvent::Complete(summary) => {
                        assert!(success, "unexpected completion: {scenario}");
                        assert_eq!(summary.status, "COMPLETE");
                        assert_eq!(summary.mode, "live");
                        assert!(summary.peer_authenticated);
                        assert!(summary.authority_replay_blake3.is_some());
                        terminal[index] = true;
                    }
                    LiveEvent::RecoveryPausing { .. }
                    | LiveEvent::RecoveryScheduled { .. }
                    | LiveEvent::RecoverySampling { .. }
                    | LiveEvent::RecoveryReady { .. } => {
                        panic!("unexpected recovery in baseline scenario")
                    }
                    LiveEvent::Failed(_) => {
                        assert!(!success, "unexpected production worker failure: {scenario}");
                        terminal[index] = true;
                    }
                }
            }
        }
        if workers.len() == 1 && invite.exists() {
            let role = if scenario == "receive" {
                LiveRole::Receive {
                    package_destination: received.clone(),
                    invite: invite.clone(),
                }
            } else {
                LiveRole::Join {
                    package: package.clone(),
                    invite: invite.clone(),
                }
            };
            workers.push(
                LiveSession::spawn(LiveConfig {
                    role,
                    output: output.join("guest"),
                })
                .unwrap(),
            );
        }
        if scenario == "cancel-before-ready" && prepared.iter().all(Option::is_some) && !action {
            workers[0].cancel();
            action = true;
        }
        if started.iter().all(|value| *value) {
            if success {
                for index in 0..2 {
                    if ended[index] || terminal[index] {
                        continue;
                    }
                    let (epoch, player, final_through) = prepared[index].unwrap();
                    let count = if index == 0 { 73 } else { 19 };
                    let batch = if index == 0 { 4 } else { 3 };
                    for _ in 0..batch {
                        if seq[index] == count {
                            break;
                        }
                        let input = hit(epoch, player, seq[index]);
                        workers[index].try_send(LiveCommand::Fact(input)).unwrap();
                        accepted[index].push(input);
                        seq[index] += 1;
                    }
                    let input = watermark(
                        epoch,
                        player,
                        100 + (seq[index] as i64 - 1) * 40 + index as i64 * 10,
                    );
                    workers[index].try_send(LiveCommand::Fact(input)).unwrap();
                    accepted[index].push(input);
                    if seq[index] == count {
                        let input = watermark(epoch, player, final_through);
                        workers[index].try_send(LiveCommand::Fact(input)).unwrap();
                        accepted[index].push(input);
                        workers[index].try_send(LiveCommand::End).unwrap();
                        ended[index] = true;
                    }
                }
            } else if !action {
                let (mut epoch, mut player, _) = prepared[0].unwrap();
                if scenario == "wrong-player" {
                    player = PlayerId::P2;
                }
                if scenario == "wrong-epoch" {
                    epoch = SessionEpoch(epoch.0.wrapping_add(1));
                }
                let input = hit(epoch, player, 0);
                workers[0].try_send(LiveCommand::Fact(input)).unwrap();
                accepted[0].push(input);
                action = true;
            }
            if scenario == "cancel-after-hit" && peer[1].len() == 1 {
                workers[0].cancel();
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    let stopped = Instant::now() + Duration::from_secs(3);
    while !workers.iter().all(LiveSession::is_finished) {
        assert!(
            Instant::now() < stopped,
            "terminal event must correspond to exiting owned workers"
        );
        thread::sleep(Duration::from_millis(1));
    }
    if success {
        assert_eq!(peer[0], accepted[1]);
        assert_eq!(peer[1], accepted[0]);
        let authority = output.join("host/authority.replay.json");
        assert_eq!(
            fs::read(&authority).unwrap(),
            fs::read(output.join("guest/authority.replay.json")).unwrap()
        );
        let host = play(&output.join("host/live.replay.json"), &package);
        let guest = play(
            &output.join("guest/live.replay.json"),
            if scenario == "receive" {
                &received
            } else {
                &package
            },
        );
        let canonical = play(&authority, &package);
        assert_eq!(host.events(), guest.events());
        assert_eq!(host.events(), canonical.events());
        assert_eq!(host.resonance(), guest.resonance());
    } else {
        if scenario == "missing-armed" || scenario == "cancel-before-ready" {
            assert_eq!(started, [false, false]);
        }
        for index in 0..2 {
            let path = output.join(if index == 0 {
                "host/live.replay.json"
            } else {
                "guest/live.replay.json"
            });
            let replay = Replay::load(path).unwrap();
            if scenario == "cancel-after-hit" {
                assert_eq!(replay.facts(), accepted[0]);
            } else {
                assert!(replay.facts().is_empty());
            }
        }
    }
    serde_json::json!({"status":"PASS","scenario":scenario,"epoch":prepared[0].map(|value| value.0.0),"started":started,"queued_facts":[accepted[0].len(), accepted[1].len()],"peer_facts":[peer[0].len(),peer[1].len()],"owned_workers_finished":true,"clock_maintenance_samples":[maintained_clocks[0].len(),maintained_clocks[1].len()],"clock_maintenance_receipts":maintained_clocks,"scope":"public production network worker, software loopback only; no PCM scheduling, physical input, audio device or two-machine proof"})
}

#[derive(Clone, Default)]
struct RelayCounts {
    received: [u64; 2],
    sent: [u64; 2],
    dropped: [u64; 2],
    unknown: u64,
    clients: u64,
}
impl RelayCounts {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({"received":self.received,"sent":self.sent,"dropped":self.dropped,"unknown":self.unknown,"clients":self.clients})
    }
}
#[derive(Default)]
struct RelayState {
    dropping: bool,
    counts: RelayCounts,
    transitions: Vec<serde_json::Value>,
}
struct UdpRelay {
    endpoint: std::net::SocketAddr,
    origin: Instant,
    state: std::sync::Arc<std::sync::Mutex<RelayState>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    raw_path: PathBuf,
}
impl UdpRelay {
    fn new(server: std::net::SocketAddr, raw_path: PathBuf) -> Self {
        let socket = std::sync::Arc::new(std::net::UdpSocket::bind("127.0.0.1:0").unwrap());
        socket
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let endpoint = socket.local_addr().unwrap();
        let origin = Instant::now();
        let state = std::sync::Arc::new(std::sync::Mutex::new(RelayState::default()));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (thread_state, thread_stop) = (state.clone(), stop.clone());
        let thread = thread::spawn(move || {
            let mut backends = Vec::new();
            let mut responses = Vec::new();
            let mut buffer = [0_u8; 65_535];
            while !thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                let (length, client) = match socket.recv_from(&mut buffer) {
                    Ok(packet) => packet,
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        continue;
                    }
                    Err(error) => panic!("UDP relay receive failed: {error}"),
                };
                if !client.ip().is_loopback() {
                    thread_state.lock().unwrap().counts.unknown += 1;
                    continue;
                }
                if !backends.iter().any(|(address, _)| *address == client) {
                    if backends.len() == 2 {
                        thread_state.lock().unwrap().counts.unknown += 1;
                        continue;
                    }
                    let backend =
                        std::sync::Arc::new(std::net::UdpSocket::bind("127.0.0.1:0").unwrap());
                    backend
                        .set_read_timeout(Some(Duration::from_millis(20)))
                        .unwrap();
                    backends.push((client, backend.clone()));
                    thread_state.lock().unwrap().counts.clients += 1;
                    let (front, state, stop) =
                        (socket.clone(), thread_state.clone(), thread_stop.clone());
                    responses.push(thread::spawn(move || {
                        let mut buffer = [0_u8; 65_535];
                        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                            let (length, source) = match backend.recv_from(&mut buffer) {
                                Ok(packet) => packet,
                                Err(error)
                                    if matches!(
                                        error.kind(),
                                        std::io::ErrorKind::WouldBlock
                                            | std::io::ErrorKind::TimedOut
                                    ) =>
                                {
                                    continue;
                                }
                                Err(error) => panic!("UDP backend receive failed: {error}"),
                            };
                            let mut state = state.lock().unwrap();
                            if source != server {
                                state.counts.unknown += 1;
                                continue;
                            }
                            state.counts.received[1] += 1;
                            if state.dropping {
                                state.counts.dropped[1] += 1;
                            } else {
                                assert_eq!(
                                    front.send_to(&buffer[..length], client).unwrap(),
                                    length
                                );
                                state.counts.sent[1] += 1;
                            }
                        }
                    }));
                }
                let mut state = thread_state.lock().unwrap();
                state.counts.received[0] += 1;
                // The same lock brackets send completion and blackhole transitions
                if state.dropping {
                    state.counts.dropped[0] += 1;
                } else {
                    let backend = &backends
                        .iter()
                        .find(|(address, _)| *address == client)
                        .unwrap()
                        .1;
                    assert_eq!(backend.send_to(&buffer[..length], server).unwrap(), length);
                    state.counts.sent[0] += 1;
                }
            }
            for response in responses {
                response.join().unwrap();
            }
        });
        Self {
            endpoint,
            origin,
            state,
            stop,
            thread: Some(thread),
            raw_path,
        }
    }
    fn blackhole(&self, dropping: bool) {
        let mut state = self.state.lock().unwrap();
        assert_ne!(state.dropping, dropping);
        state.dropping = dropping;
        let event = serde_json::json!({"dropping":dropping,"at_ns":self.origin.elapsed().as_nanos() as u64,"counts":state.counts.json()});
        state.transitions.push(event);
        self.save(&state);
    }
    fn save(&self, state: &RelayState) {
        fs::write(
            &self.raw_path,
            serde_json::to_vec_pretty(
                &serde_json::json!({"transitions":state.transitions,"counts":state.counts.json()}),
            )
            .unwrap(),
        )
        .unwrap();
    }
    fn event_ns(&self, time: Instant) -> u64 {
        let elapsed = time
            .checked_duration_since(self.origin)
            .expect("event predates relay coordinate origin");
        u64::try_from(elapsed.as_nanos()).expect("relay coordinate overflow")
    }
    fn finish(mut self, maintenance_stale: bool) -> serde_json::Value {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
        let state = self.state.lock().unwrap();
        self.save(&state);
        assert_eq!(state.counts.clients, 2);
        assert_eq!(state.counts.unknown, 0);
        assert_eq!(state.transitions.len(), 2);
        let begin = &state.transitions[0];
        let end = &state.transitions[1];
        assert_eq!(begin["dropping"], true);
        assert_eq!(end["dropping"], false);
        if !maintenance_stale {
            assert!(
                end["at_ns"].as_u64().unwrap() - begin["at_ns"].as_u64().unwrap() >= 29_000_000_000
            );
        }
        assert_eq!(begin["counts"]["sent"], end["counts"]["sent"]);
        for direction in 0..2 {
            assert!(
                end["counts"]["dropped"][direction].as_u64().unwrap()
                    > begin["counts"]["dropped"][direction].as_u64().unwrap()
            );
        }
        serde_json::json!({"direction_order":["client_to_server","server_to_client"],"transitions":state.transitions,"final":state.counts.json()})
    }
}
impl Drop for UdpRelay {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
            if let Ok(state) = self.state.lock() {
                self.save(&state);
            }
        }
    }
}

fn relay_invitation(original: &Path, relay: &UdpRelay, copied: &Path) {
    let bytes = fs::read(original).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let old = format!(
        "\"endpoint\":{}",
        serde_json::to_string(&value["endpoint"]).unwrap()
    );
    let new = format!(
        "\"endpoint\":{}",
        serde_json::to_string(&relay.endpoint.to_string()).unwrap()
    );
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert_eq!(text.matches(&old).count(), 1);
    let modified = text.replacen(&old, &new, 1);
    assert_eq!(modified.replacen(&new, &old, 1).as_bytes(), bytes);
    let mut parsed: serde_json::Value = serde_json::from_str(&modified).unwrap();
    parsed["endpoint"] = value["endpoint"].clone();
    assert_eq!(parsed, value);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(copied).unwrap();
    file.write_all(modified.as_bytes()).unwrap();
    file.sync_all().unwrap();
}

// The oscillator below is declared QA input, not a PCM/device acknowledgment
fn recovery_model(scenario: &str, package: &Path, output: &Path) -> serde_json::Value {
    use cocobeat_net::{RecoveryFrozen, RecoveryObserved, RecoveryPublication};
    use cocobeat_replay::ReplayIdentity;
    let udp = scenario == "recovery-udp-blackhole";
    let mut relay: Option<UdpRelay> = None;
    let mut pausing_times = [None, None];
    let mut local_start_ns = [None, None];
    let mut last_maintained_at_pause: [Option<serde_json::Value>; 2] = [None, None];
    let mut frozen = [false; 2];
    let mut last_original_watermark = [None, None];
    let valid = matches!(
        scenario,
        "recovery-model"
            | "recovery-unauth-candidate"
            | "recovery-immediate-end"
            | "recovery-udp-blackhole"
    );
    let mut candidate: Option<LiveSession> = None;
    let mut candidate_failed = scenario != "recovery-unauth-candidate";
    let mut second_loss = false;
    let mut cancelled = false;
    fs::create_dir(output).unwrap();
    let invite = output.join("invite.json");
    let mut workers = vec![
        LiveSession::spawn(LiveConfig {
            role: LiveRole::Host {
                package: package.into(),
                bind: "127.0.0.1:0".parse().unwrap(),
                invite: invite.clone(),
            },
            output: output.join("host"),
        })
        .unwrap(),
    ];
    let mut prepared = [None, None];
    let mut identities = [None, None];
    let mut initial_deadline = [None, None];
    let mut started = [false; 2];
    let mut terminal = [false; 2];
    let mut accepted: [Vec<DuoInput>; 2] = [Vec::new(), Vec::new()];
    let mut peer: [Vec<DuoInput>; 2] = [Vec::new(), Vec::new()];
    let mut maintained_clocks: [Vec<serde_json::Value>; 2] = [Vec::new(), Vec::new()];
    let mut clock_sync: [Option<cocobeat_net::clock::ClockSync>; 2] = [None, None];
    let mut paused = [0_i64; 2];
    let mut resume = [None, None];
    let mut verify = [None, None];
    let mut sampling = [false; 2];
    let mut ready = [false; 2];
    let mut progress = [None, None];
    let mut observations: [Vec<RecoveryPublication>; 2] = [Vec::new(), Vec::new()];
    let mut snapshot_bytes: [Option<Vec<Vec<u8>>>; 2] = [None, None];
    let mut publication_seq = [0_u64; 2];
    let mut observed_sent = [false; 2];
    let mut final_sent = [false; 2];
    let mut request = false;
    let limit = Instant::now() + Duration::from_secs(if udp { 110 } else { 40 });
    while !terminal.iter().all(|done| *done) {
        assert!(Instant::now() < limit, "bounded recovery model timeout");
        for index in 0..workers.len() {
            while let Ok(event) = workers[index].try_recv() {
                println!("{} {event:?}", if index == 0 { "host" } else { "guest" });
                match event {
                    LiveEvent::Listening { .. } => {}
                    LiveEvent::Prepared {
                        epoch,
                        player,
                        content_id,
                        canonical_frames,
                        stage_compiler_version,
                        final_through,
                        ..
                    } => {
                        assert!(
                            canonical_frames >= 57_600,
                            "model requires a declared long QA fixture"
                        );
                        if udp {
                            assert!(
                                canonical_frames >= 3_072_000,
                                "UDP blackout needs an unchanged source lasting at least 64 seconds"
                            );
                        }
                        prepared[index] = Some((epoch, player, final_through));
                        identities[index] = Some(ReplayIdentity {
                            content_id,
                            rules_id: "duo-watermark-v1".into(),
                            build_id: "software-oscillator-QA".into(),
                            stage_compiler_version: Some(stage_compiler_version),
                        });
                        workers[index].try_send(LiveCommand::Ready).unwrap();
                    }
                    LiveEvent::Scheduled {
                        epoch,
                        deadline,
                        timing,
                    } => {
                        assert_eq!(epoch, prepared[index].unwrap().0);
                        initial_deadline[index] = Some(deadline);
                        local_start_ns[index] = Some(
                            timing
                                .local_start_ns
                                .expect("actual scheduled process coordinate is required"),
                        );
                        workers[index].try_send(LiveCommand::Armed).unwrap();
                    }
                    LiveEvent::Started { epoch } => {
                        assert_eq!(epoch, prepared[index].unwrap().0);
                        started[index] = true;
                        let input = hit(epoch, prepared[index].unwrap().1, 0);
                        workers[index].try_send(LiveCommand::Fact(input)).unwrap();
                        accepted[index].push(input);
                    }
                    LiveEvent::ClockMaintained {
                        epoch,
                        round,
                        exchange,
                    } => {
                        assert_eq!(Some(epoch), prepared[index].map(|value| value.0));
                        assert!(started[index] && !terminal[index]);
                        record_maintained_clock(
                            &mut maintained_clocks[index],
                            &mut clock_sync[index],
                            epoch,
                            round,
                            exchange,
                        );
                    }
                    LiveEvent::PeerFacts(facts) => peer[index].extend(facts),
                    LiveEvent::RecoveryPausing { epoch, attempt } => {
                        assert_eq!(epoch, prepared[index].unwrap().0);
                        assert_eq!(attempt, 1);
                        let now = Instant::now();
                        frozen[index] = true;
                        if let Some(relay) = &relay {
                            let sample = maintained_clocks[index]
                                .last()
                                .expect("UDP clock timeout requires an actual maintained sample");
                            let stamp_key = if index == 0 {
                                "host_receive_ns"
                            } else {
                                "guest_receive_ns"
                            };
                            let local_sample_ns = sample[stamp_key].as_u64().unwrap();
                            let start_ns = local_start_ns[index].unwrap();
                            let scheduled = initial_deadline[index].unwrap();
                            let origin = scheduled
                                .checked_sub(Duration::from_nanos(start_ns))
                                .expect("process origin mapping overflow");
                            let sample_at = origin
                                .checked_add(Duration::from_nanos(local_sample_ns))
                                .expect("maintained sample mapping overflow");
                            let age = now
                                .checked_duration_since(sample_at)
                                .expect("maintained sample cannot follow pausing");
                            let age_ns = u64::try_from(age.as_nanos()).unwrap();
                            let sample_relay_ns = relay.event_ns(sample_at);
                            last_maintained_at_pause[index] = Some(serde_json::json!({
                                "sample":sample,"local_stamp_key":stamp_key,"local_sample_ns":local_sample_ns,
                                "scheduled_deadline_relay_ns":relay.event_ns(scheduled),"local_start_ns":start_ns,
                                "local_sample_relay_ns":sample_relay_ns,"sample_age_at_pause_ns":age_ns,
                                "coordinate_policy":"process origin = Scheduled.deadline - timing.local_start_ns; actual role-local maintained t2/t4 mapped to relay origin; software capture only, not kernel or DAC"
                            }));
                            pausing_times[index] = Some(relay.event_ns(now));
                            if pausing_times.iter().filter(|time| time.is_some()).count() == 1 {
                                relay.blackhole(false);
                            }
                        }
                        paused[index] = (now
                            .duration_since(initial_deadline[index].unwrap())
                            .as_nanos()
                            * 48_000
                            / 1_000_000_000) as i64
                            + index as i64 * 500;
                        let mut replay =
                            Replay::new(identities[index].clone().unwrap(), epoch).unwrap();
                        for input in accepted[index].iter().chain(&peer[index]) {
                            replay.record(*input).unwrap();
                        }
                        workers[index]
                            .try_send(LiveCommand::RecoveryFrozen {
                                epoch,
                                attempt,
                                snapshot: Box::new(RecoveryFrozen {
                                    replay,
                                    paused_frame: SongTime::from_frames(paused[index]),
                                    source_generation: 1,
                                    source_id: index as u64 + 1,
                                    paused_at: now,
                                }),
                            })
                            .unwrap();
                    }
                    LiveEvent::RecoveryScheduled {
                        epoch,
                        attempt,
                        deadline,
                        verify_at,
                        ..
                    } => {
                        assert_eq!(epoch, prepared[index].unwrap().0);
                        assert_eq!(attempt, 1);
                        assert!(
                            deadline.saturating_duration_since(Instant::now())
                                >= Duration::from_millis(100)
                        );
                        snapshot_bytes[index] = Some(
                            [
                                "worker-prefix.replay.json",
                                "gui-prefix.replay.json",
                                "metadata.json",
                            ]
                            .map(|name| {
                                fs::read(
                                    output
                                        .join(if index == 0 { "host" } else { "guest" })
                                        .join("recovery-1")
                                        .join(name),
                                )
                                .unwrap()
                            })
                            .to_vec(),
                        );
                        resume[index] = Some(deadline);
                        verify[index] = Some(verify_at);
                        workers[index]
                            .try_send(LiveCommand::RecoveryArmed { epoch, attempt })
                            .unwrap();
                        if scenario == "recovery-cancel" && !cancelled {
                            workers[0].cancel();
                            cancelled = true;
                        }
                    }
                    LiveEvent::RecoverySampling {
                        epoch,
                        attempt,
                        not_before,
                    } => {
                        assert_eq!(epoch, prepared[index].unwrap().0);
                        assert_eq!(attempt, 1);
                        assert!(not_before <= Instant::now());
                        sampling[index] = true;
                    }
                    LiveEvent::RecoveryReady { epoch, attempt } => {
                        assert!(
                            valid || scenario == "recovery-second-loss",
                            "negative model must not unlock input"
                        );
                        assert_eq!(epoch, prepared[index].unwrap().0);
                        assert_eq!(attempt, 1);
                        ready[index] = true;
                    }
                    LiveEvent::Complete(summary) => {
                        assert!(valid);
                        assert_eq!(summary.status, "COMPLETE");
                        assert_eq!(summary.epoch, prepared[index].unwrap().0.0);
                        terminal[index] = true;
                    }
                    LiveEvent::Failed(error) => {
                        assert!(!valid, "unexpected continuation failure: {error}");
                        terminal[index] = true;
                    }
                }
            }
        }
        if workers.len() == 1 && invite.exists() {
            let guest_invite = if udp {
                let original: serde_json::Value =
                    serde_json::from_slice(&fs::read(&invite).unwrap()).unwrap();
                let proxy = UdpRelay::new(
                    original["endpoint"].as_str().unwrap().parse().unwrap(),
                    output.join("udp-relay-raw.json"),
                );
                let copied = output.join("relay-invite.json");
                relay_invitation(&invite, &proxy, &copied);
                relay = Some(proxy);
                copied
            } else {
                invite.clone()
            };
            workers.push(
                LiveSession::spawn(LiveConfig {
                    role: LiveRole::Join {
                        package: package.into(),
                        invite: guest_invite,
                    },
                    output: output.join("guest"),
                })
                .unwrap(),
            );
        }
        if let Some(worker) = candidate.as_ref() {
            while let Ok(event) = worker.try_recv() {
                assert!(
                    matches!(event, LiveEvent::Failed(_)),
                    "unauthenticated candidate must not become Prepared"
                );
                candidate_failed = true;
            }
        }
        if scenario == "recovery-unauth-candidate" && candidate.is_none() && started == [true; 2] {
            candidate = Some(
                LiveSession::spawn(LiveConfig {
                    role: LiveRole::Join {
                        package: package.into(),
                        invite: invite.clone(),
                    },
                    output: output.join("unauth-candidate"),
                })
                .unwrap(),
            );
        }
        if !request
            && candidate_failed
            && started == [true; 2]
            && peer.iter().all(|facts| !facts.is_empty())
            && initial_deadline.iter().all(|deadline| {
                Instant::now().duration_since(deadline.unwrap()) >= Duration::from_millis(120)
            })
        {
            if udp {
                relay.as_ref().unwrap().blackhole(true);
            } else {
                workers[0]
                    .try_send(LiveCommand::RequestRecovery {
                        epoch: prepared[0].unwrap().0,
                    })
                    .unwrap();
            }
            request = true;
        }
        for index in 0..workers.len() {
            if terminal[index] || final_sent[index] {
                continue;
            }
            if udp
                && started[index]
                && !frozen[index]
                && last_original_watermark[index]
                    .is_none_or(|last: Instant| last.elapsed() >= Duration::from_millis(50))
            {
                let now = Instant::now();
                let frame = (now
                    .duration_since(initial_deadline[index].unwrap())
                    .as_nanos()
                    * 48_000
                    / 1_000_000_000) as i64
                    + index as i64 * 500;
                let (epoch, player, _) = prepared[index].unwrap();
                let input = watermark(epoch, player, frame - 2_400);
                workers[index].try_send(LiveCommand::Fact(input)).unwrap();
                accepted[index].push(input);
                last_original_watermark[index] = Some(now);
            }
            if let Some(deadline) = resume[index] {
                let now = Instant::now();
                if now > deadline {
                    let frame = paused[index]
                        + (now.duration_since(deadline).as_nanos() * 48_000 / 1_000_000_000) as i64;
                    publication_seq[index] += 1;
                    let row = RecoveryPublication {
                        sequence: publication_seq[index],
                        frame: SongTime::from_frames(frame),
                        published_between: [now; 2],
                    };
                    if progress[index].is_none() {
                        progress[index] = Some(row);
                    } else if sampling[index] && !observed_sent[index] {
                        observations[index].push(row);
                        assert!(observations[index].len() <= 64);
                    }
                    let (epoch, player, final_through) = prepared[index].unwrap();
                    let input = watermark(epoch, player, frame - 2_400);
                    match workers[index].try_send(LiveCommand::Fact(input)) {
                        Ok(()) => accepted[index].push(input),
                        Err(cocobeat_net::LiveSendError::Closed) if !valid => continue,
                        Err(error) => panic!("model owner fact was not accepted: {error:?}"),
                    }
                    if sampling[index]
                        && !observed_sent[index]
                        && now > verify[index].unwrap() + Duration::from_millis(50)
                    {
                        assert!(observations[index].len() >= 2);
                        workers[index]
                            .try_send(LiveCommand::RecoveryObserved {
                                epoch,
                                attempt: 1,
                                evidence: RecoveryObserved {
                                    generation: 1,
                                    source_id: if scenario == "recovery-bad-source" && index == 1 {
                                        999
                                    } else {
                                        index as u64 + 1
                                    },
                                    progress: progress[index].unwrap(),
                                    publications: observations[index].clone(),
                                },
                            })
                            .unwrap();
                        observed_sent[index] = true;
                    }
                    if ready == [true; 2] && scenario == "recovery-second-loss" && !second_loss {
                        workers[0]
                            .try_send(LiveCommand::RequestRecovery { epoch })
                            .unwrap();
                        second_loss = true;
                    }
                    if valid
                        && (ready == [true; 2]
                            || scenario == "recovery-immediate-end" && ready[index])
                    {
                        let input = DuoInput::Hit(Hit {
                            epoch,
                            player,
                            seq: 1,
                            song_time: SongTime::from_frames(frame),
                        });
                        workers[index].try_send(LiveCommand::Fact(input)).unwrap();
                        accepted[index].push(input);
                        let input = watermark(epoch, player, final_through);
                        workers[index].try_send(LiveCommand::Fact(input)).unwrap();
                        accepted[index].push(input);
                        workers[index].try_send(LiveCommand::End).unwrap();
                        final_sent[index] = true;
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    let stop = Instant::now() + Duration::from_secs(3);
    while !workers.iter().all(LiveSession::is_finished) {
        assert!(Instant::now() < stop);
        thread::sleep(Duration::from_millis(1));
    }
    if let Some(candidate) = candidate {
        assert!(candidate_failed);
        assert!(candidate.is_finished());
    }
    assert_eq!(prepared[0].unwrap().0, prepared[1].unwrap().0);
    for side in ["host", "guest"] {
        let directory = output.join(side);
        assert!(
            directory
                .join("recovery-1/worker-prefix.replay.json")
                .exists()
        );
        assert!(directory.join("recovery-1/gui-prefix.replay.json").exists());
        let replay = Replay::load(directory.join("live.replay.json")).unwrap();
        if let Some(bytes) = &snapshot_bytes[if side == "host" { 0 } else { 1 }] {
            for (name, expected) in [
                "worker-prefix.replay.json",
                "gui-prefix.replay.json",
                "metadata.json",
            ]
            .iter()
            .zip(bytes)
            {
                assert_eq!(
                    fs::read(directory.join("recovery-1").join(name)).unwrap(),
                    *expected
                );
            }
        }
        for name in ["worker-prefix.replay.json", "gui-prefix.replay.json"] {
            let prefix = Replay::load(directory.join("recovery-1").join(name)).unwrap();
            for player in [PlayerId::P1, PlayerId::P2] {
                let tape = |replay: &Replay| {
                    replay
                        .facts()
                        .iter()
                        .filter(|input| match input {
                            DuoInput::Hit(hit) => hit.player == player,
                            DuoInput::Watermark { player: actual, .. } => *actual == player,
                        })
                        .copied()
                        .collect::<Vec<_>>()
                };
                assert!(tape(&replay).starts_with(&tape(&prefix)));
            }
        }
        assert_eq!(replay.epoch(), prepared[0].unwrap().0);
        assert_eq!(directory.join("authority.replay.json").exists(), valid);
        for player in [PlayerId::P1, PlayerId::P2] {
            let sequences: Vec<_> = replay
                .facts()
                .iter()
                .filter_map(|input| match input {
                    DuoInput::Hit(hit) if hit.player == player => Some(hit.seq),
                    _ => None,
                })
                .collect();
            assert_eq!(sequences, (0..sequences.len() as u64).collect::<Vec<_>>());
        }
    }
    if valid {
        assert_eq!(ready, [true; 2]);
        assert_eq!(peer[0], accepted[1]);
        assert_eq!(peer[1], accepted[0]);
        let authority = output.join("host/authority.replay.json");
        assert_eq!(
            fs::read(&authority).unwrap(),
            fs::read(output.join("guest/authority.replay.json")).unwrap()
        );
        assert_eq!(
            play(&authority, package).events(),
            play(&output.join("host/live.replay.json"), package).events()
        );
    } else {
        assert_eq!(
            ready,
            if scenario == "recovery-second-loss" {
                [true; 2]
            } else {
                [false; 2]
            }
        );
    }
    let causes: Vec<_> = if udp {
        ["host", "guest"]
            .iter()
            .map(|side| {
                serde_json::from_slice::<serde_json::Value>(
                    &fs::read(output.join(side).join("recovery-1/metadata.json")).unwrap(),
                )
                .unwrap()["cause"]
                    .clone()
            })
            .collect()
    } else {
        Vec::new()
    };
    let maintenance_stale = causes
        .iter()
        .any(|cause| cause == "clock maintenance sample is stale");
    let udp_receipt = relay.map(|relay| relay.finish(maintenance_stale));
    if udp {
        if maintenance_stale {
            for (index, cause) in causes.iter().enumerate() {
                if cause == "clock maintenance sample is stale" {
                    assert!(
                        last_maintained_at_pause[index].as_ref().unwrap()["sample_age_at_pause_ns"]
                            .as_u64()
                            .unwrap()
                            > cocobeat_net::clock::ClockConfig::default().max_sample_age_ns
                    );
                }
            }
        }
        assert!(causes.iter().any(|cause| matches!(
            cause.as_str(),
            Some(
                "QUIC idle timeout"
                    | "reliable peer progress deadline"
                    | "reliable frame deadline"
                    | "clock maintenance sample is stale"
            )
        )));
        assert!(
            !causes
                .iter()
                .any(|cause| cause == "explicit local connection maintenance")
        );
        let raw = serde_json::json!({"relay":udp_receipt,"pausing_event_ns":pausing_times,"frozen_frames":paused,"metadata_causes":causes,"maintenance_stale_triggered":maintenance_stale,"last_maintained_at_pause":last_maintained_at_pause,"clock_sample_max_age_ns":cocobeat_net::clock::ClockConfig::default().max_sample_age_ns,"request_recovery_commands":0,"invitation_only_endpoint_changed":true,"original_watermark_interval_ms":50,"scope":"real UDP blackhole and production recovery; unchanged monotonic software oscillator, no CPAL/DAC"});
        fs::write(
            output.join("udp-loss.json"),
            serde_json::to_vec_pretty(&raw).unwrap(),
        )
        .unwrap();
    }
    serde_json::json!({"status":"PASS","scenario":scenario,"same_epoch":true,"snapshots_unchanged":true,"player_tape_prefix_preserved":true,"recovery_ready":ready,"owned_workers_finished":true,"clock_maintenance_samples":[maintained_clocks[0].len(),maintained_clocks[1].len()],"clock_maintenance_receipts":maintained_clocks,"queued_facts":accepted.map(|facts|facts.len()),"unauth_candidate_rejected":scenario == "recovery-unauth-candidate" && candidate_failed,"second_recovery_terminal":second_loss,"cancel_requested":cancelled,"actual_udp_blackhole":udp,"scope":if udp { "actual UDP blackhole and typed clock-freshness or reliable-input deadline triggered same-epoch production worker recovery; unchanged declared integer monotonic software source model; no Kira, original SoundHandle, PCM/device or two-machine acceptance" } else { "actual TLS/QUIC production worker with declared integer monotonic software source model; controlled active connection rebuilding; no Kira, original SoundHandle, PCM/device, UDP loss, or two-machine acceptance" }})
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() > 1 && fixture(&args) {
        return;
    }
    assert_eq!(args.len(), 4, "driver SCENARIO PACKAGE NEW_OUTPUT");
    let scenario = &args[1];
    let package = PathBuf::from(&args[2]);
    let output = PathBuf::from(&args[3]);
    let report = if scenario.starts_with("recovery-") {
        recovery_model(scenario, &package, &output)
    } else if scenario == "reenter-before-ready" || scenario == "reenter-after-hit" {
        fs::create_dir(&output).unwrap();
        let first = run(
            if scenario == "reenter-before-ready" {
                "cancel-before-ready"
            } else {
                "cancel-after-hit"
            },
            &package,
            &output.join("round-1"),
        );
        let paths = [
            "invite.json",
            "host/live.replay.json",
            "host/status.json",
            "guest/live.replay.json",
            "guest/status.json",
        ];
        let old: Vec<_> = paths
            .iter()
            .map(|path| fs::read(output.join("round-1").join(path)).unwrap())
            .collect();
        let second = run("installed", &package, &output.join("round-2"));
        assert_ne!(first["epoch"], second["epoch"]);
        assert_ne!(
            fs::read(output.join("round-1/invite.json")).unwrap(),
            fs::read(output.join("round-2/invite.json")).unwrap()
        );
        for (path, bytes) in paths.iter().zip(old) {
            assert_eq!(fs::read(output.join("round-1").join(path)).unwrap(), bytes);
        }
        let replay = Replay::load(output.join("round-2/host/authority.replay.json")).unwrap();
        for player in [PlayerId::P1, PlayerId::P2] {
            let seq: Vec<_> = replay
                .facts()
                .iter()
                .filter_map(|fact| match fact {
                    DuoInput::Hit(hit) if hit.player == player => Some(hit.seq),
                    _ => None,
                })
                .collect();
            assert_eq!(seq, (0..seq.len() as u64).collect::<Vec<_>>());
        }
        serde_json::json!({"status":"PASS", "scenario":scenario, "same_process":true, "rounds":[first,second], "old_files_unchanged":true, "fresh_invitation":true, "fresh_epoch":true, "seq_reset":true})
    } else {
        run(scenario, &package, &output)
    };
    println!("{report}");
}
