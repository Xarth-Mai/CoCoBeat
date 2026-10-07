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
                analysis: MusicAnalysis { schema_version: CONTENT_SCHEMA_VERSION, audio_hash: audio.asset.blake3, beats: vec![], onsets: vec![], sections: vec![],
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
                    LiveEvent::PeerFacts(facts) => peer[index].extend(facts),
                    LiveEvent::Complete(summary) => {
                        assert!(success, "unexpected completion: {scenario}");
                        assert_eq!(summary.status, "COMPLETE");
                        assert_eq!(summary.mode, "live");
                        assert!(summary.peer_authenticated);
                        assert!(summary.authority_replay_blake3.is_some());
                        terminal[index] = true;
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
    serde_json::json!({"status":"PASS","scenario":scenario,"epoch":prepared[0].map(|value| value.0.0),"started":started,"queued_facts":[accepted[0].len(), accepted[1].len()],"peer_facts":[peer[0].len(),peer[1].len()],"owned_workers_finished":true,"scope":"public production network worker, software loopback only; no PCM scheduling, physical input, audio device or two-machine proof"})
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
    let report = if scenario == "reenter-before-ready" || scenario == "reenter-after-hit" {
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
