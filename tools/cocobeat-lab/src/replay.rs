use cocobeat_core::DuoEngine;
use cocobeat_media::ValidatedPackage;
use cocobeat_replay::{MAX_FACTS, MAX_FILE_BYTES, Replay, timing::TimingSidecar};
use cocobeat_schema::{
    AnchorGrade, AnchorJudgement, DuoEvent, DuoInput, DuoRules, MAX_CONTENT_ITEMS, PlayerId,
    SongTime,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::Path,
};

const RULES_ID: &str = "duo-watermark-v1";
fn report_version(replay: &Replay) -> u32 {
    if replay.identity().stage_compiler_version.is_some() {
        2
    } else {
        1
    }
}
const MAX_REPORT_BYTES: usize = 128 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 8 * 1024;
const MAX_REPORT_LINES: usize = 2 + MAX_FACTS + 3 * MAX_CONTENT_ITEMS + MAX_FACTS / 2;

pub fn inspect(source: &Path, replay_path: &Path, destination: &Path) -> Result<(), String> {
    inspect_with_timing(source, replay_path, destination, None)
}

pub fn inspect_with_timing(
    source: &Path,
    replay_path: &Path,
    destination: &Path,
    timing_path: Option<&Path>,
) -> Result<(), String> {
    let package = cocobeat_media::validate_package(source)?;
    let (replay, engine, timing) = load_with_timing(&package, replay_path, timing_path)?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if fs::canonicalize(parent)
        .map_err(|error| format!("Cannot resolve report destination parent: {error}"))?
        .starts_with(fs::canonicalize(source).map_err(|error| error.to_string())?)
    {
        return Err("Replay diagnostic must be written outside the source package".into());
    }
    write_new_report(
        destination,
        &package,
        &replay,
        &engine,
        DuoRules::default(),
        timing.as_ref(),
    )?;
    println!(
        "{}",
        json!({
            "report_version": if timing.is_some() { 3 } else { report_version(&replay) },
            "content_id": replay.identity().content_id,
            "fact_count": replay.facts().len(),
            "event_count": engine.events().len(),
        })
    );
    Ok(())
}

pub(crate) fn load(
    package: &ValidatedPackage,
    replay_path: &Path,
) -> Result<(Replay, DuoEngine), String> {
    load_with_timing(package, replay_path, None).map(|(replay, engine, _)| (replay, engine))
}

pub(crate) fn load_with_timing(
    package: &ValidatedPackage,
    replay_path: &Path,
    timing_path: Option<&Path>,
) -> Result<(Replay, DuoEngine, Option<TimingSidecar>), String> {
    if package.chart.ruleset_id != RULES_ID {
        return Err(format!(
            "Unsupported song ruleset: {}",
            package.chart.ruleset_id
        ));
    }
    if !fs::symlink_metadata(replay_path)
        .map_err(|error| format!("Cannot inspect Replay file: {error}"))?
        .is_file()
    {
        return Err("Replay input must be a regular file".into());
    }
    let file = File::open(replay_path).map_err(|error| format!("Cannot open Replay: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Opened Replay input must be a regular file".into());
    }
    let mut raw = Vec::new();
    let replay = if timing_path.is_some() {
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut raw)
            .map_err(|error| format!("Cannot read Replay for Timing: {error}"))?;
        if raw.len() as u64 > MAX_FILE_BYTES {
            return Err("Replay file byte limit exceeded".into());
        }
        Replay::decode(raw.as_slice())
    } else {
        Replay::decode(file)
    }
    .map_err(|error| format!("Cannot decode Replay: {error}"))?;
    for (index, fact) in replay.facts().iter().enumerate() {
        if matches!(fact, DuoInput::Hit(hit) if hit.song_time < SongTime::ZERO || hit.song_time.frames() as u64 >= package.manifest.canonical_frames)
        {
            return Err(format!(
                "Replay fact {}: Hit lies outside the song timeline",
                index + 1
            ));
        }
    }
    let hash: String = package
        .manifest
        .package_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let rules = DuoRules::default();
    let engine = replay
        .replay(
            &format!("package-blake3:{hash}"),
            RULES_ID,
            package.chart.anchors.clone(),
            rules,
        )
        .map_err(|error| error.to_string())?;
    let timing = timing_path
        .map(|path| TimingSidecar::load(path, &replay, &raw, package.manifest.canonical_frames))
        .transpose()
        .map_err(|error| format!("Cannot bind local Timing: {error}"))?;
    Ok((replay, engine, timing))
}

fn write_new_report(
    destination: &Path,
    package: &ValidatedPackage,
    replay: &Replay,
    engine: &DuoEngine,
    rules: DuoRules,
    timing: Option<&TimingSidecar>,
) -> Result<(), String> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("Cannot create Replay diagnostic: {error}"))?;
    let mut writer = BufWriter::new(file);
    let result = (|| {
        write_report(&mut writer, package, replay, engine, rules, timing)?;
        writer
            .flush()
            .map_err(|error| format!("Cannot flush Replay diagnostic: {error}"))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|error| format!("Cannot sync Replay diagnostic: {error}"))
    })();
    drop(writer);
    if let Err(error) = result {
        return Err(match fs::remove_file(destination) {
            Ok(()) => error,
            Err(cleanup) => format!("{error}; diagnostic cleanup failed: {cleanup}"),
        });
    }
    Ok(())
}

fn write_report(
    writer: &mut impl Write,
    package: &ValidatedPackage,
    replay: &Replay,
    engine: &DuoEngine,
    rules: DuoRules,
    timing: Option<&TimingSidecar>,
) -> Result<(), String> {
    let mut bytes = 0;
    let mut lines = 0;
    let mut line = |value| write_line(writer, &mut bytes, &mut lines, value);
    let mut header = header(package, replay, engine, rules);
    if let Some(timing) = timing {
        header["report_version"] = 3.into();
        header["local_timing"] = timing_header(timing);
    }
    line(header)?;
    let index = ReportIndex::new(&package.chart.anchors, replay);
    for (position, fact) in replay.facts().iter().enumerate() {
        let mut row = fact_row(position, *fact);
        if let Some(timing) = timing {
            annotate_timing(&mut row, timing);
        }
        line(row)?;
    }
    for (position, event) in engine.events().iter().enumerate() {
        let mut row = index.event_row(position, *event)?;
        if let Some(timing) = timing {
            annotate_timing(&mut row, timing);
        }
        line(row)?;
    }
    line(summary(package.chart.anchors.len(), replay, engine)?)
}

pub(crate) fn timing_header(timing: &TimingSidecar) -> Value {
    json!({
        "format": timing.format, "version": timing.version,
        "replay_blake3": timing.replay_blake3,
        "stage_compiler_version": timing.stage_compiler_version,
        "timestamp_basis": timing.timestamp_basis,
        "input_timestamp_kind": timing.input_timestamp_kind,
        "clock_observation_kind": timing.clock_observation_kind,
        "clock": timing.clock, "capture_players": timing.capture_players,
        "capture_count": timing.captures.len(),
        "audio_sampling_interval_ns": timing.audio_sampling_interval_ns,
        "audio_history_status": timing.audio_history_status,
        "audio_read_count": timing.audio_history.len(),
    })
}

fn timing_for_fact(timing: &TimingSidecar, fact_index: Option<u64>) -> Value {
    let Some(fact_index) = fact_index else {
        return json!({"status": "no_hit"});
    };
    // Shared validation guarantees ordered, exact local Hit fact indices
    let Ok(index) = timing
        .captures
        .binary_search_by_key(&fact_index, |row| row.fact_index)
    else {
        return json!({"status": "not_recorded", "input_fact_index": fact_index});
    };
    let capture = &timing.captures[index];
    let read_index = timing
        .audio_history
        .partition_point(|read| read.read_after_ns <= capture.observed_ns);
    json!({
        "status": "recorded", "capture": capture,
        "software_message_consumption_wait_ns": capture.consumed_ns.checked_sub(capture.observed_ns)
            .expect("Validated captures preserve observation before consumption"),
        "independent_audio_read_history": {
            "before_observation": read_index.checked_sub(1).and_then(|index| timing.audio_history.get(index)),
            "at_or_after_observation": timing.audio_history.get(read_index),
        },
    })
}

pub(crate) fn annotate_timing(record: &mut Value, timing: &TimingSidecar) {
    match record["type"].as_str() {
        Some("hit") => {
            record["local_timing"] = timing_for_fact(timing, record["fact_index"].as_u64())
        }
        Some("anchor_judged") => {
            record["local_timing"] = timing_for_fact(timing, record["input_fact_index"].as_u64())
        }
        Some("free_sync" | "anchor_sync") => {
            for player in ["p1", "p2"] {
                record[player]["local_timing"] =
                    timing_for_fact(timing, record[player]["input_fact_index"].as_u64());
            }
        }
        _ => {}
    }
}

pub(crate) fn header(
    package: &ValidatedPackage,
    replay: &Replay,
    engine: &DuoEngine,
    rules: DuoRules,
) -> Value {
    let mut value = json!({
        "type": "header", "format": "CoCoBeat Replay Diagnostic", "report_version": report_version(replay),
        "content_id": replay.identity().content_id, "rules_id": replay.identity().rules_id,
        "build_id": replay.identity().build_id, "epoch": replay.epoch().0,
        "canonical_frames": package.manifest.canonical_frames,
        "anchor_count": package.chart.anchors.len(), "fact_count": replay.facts().len(),
        "confirmation_delay_frames": engine.confirmation_delay_frames(),
        "rules": {
            "precise_window_frames": rules.precise_window_frames,
            "good_window_frames": rules.good_window_frames,
            "anchor_window_frames": rules.anchor_window_frames,
            "free_sync_window_frames": rules.free_sync_window_frames,
            "anchor_sync_window_frames": rules.anchor_sync_window_frames,
            "resonance_window_frames": rules.resonance_window_frames,
            "resonance_full_pairs": rules.resonance_full_pairs,
        },
    });
    if let Some(version) = replay.identity().stage_compiler_version {
        value["stage_compiler_version"] = version.into();
    }
    value
}

pub(crate) fn fact_row(index: usize, fact: DuoInput) -> Value {
    match fact {
        DuoInput::Hit(hit) => json!({
            "type": "hit", "fact_index": index + 1, "epoch": hit.epoch.0,
            "player": hit.player.index() + 1, "seq": hit.seq,
            "song_time_frames": hit.song_time.frames(),
        }),
        DuoInput::Watermark {
            epoch,
            player,
            through,
        } => json!({
            "type": "watermark", "fact_index": index + 1, "epoch": epoch.0,
            "player": player.index() + 1, "through_frames": through.frames(),
        }),
    }
}

pub(crate) struct ReportIndex {
    hits: BTreeMap<(PlayerId, u64), (usize, SongTime)>,
    anchors: BTreeMap<u64, SongTime>,
}

impl ReportIndex {
    pub(crate) fn new(anchors: &[cocobeat_schema::Anchor], replay: &Replay) -> Self {
        Self {
            hits: replay
                .facts()
                .iter()
                .enumerate()
                .filter_map(|(index, fact)| match fact {
                    DuoInput::Hit(hit) => Some(((hit.player, hit.seq), (index + 1, hit.song_time))),
                    DuoInput::Watermark { .. } => None,
                })
                .collect(),
            anchors: anchors
                .iter()
                .map(|anchor| (anchor.id, anchor.song_time))
                .collect(),
        }
    }

    pub(crate) fn event_row(&self, index: usize, event: DuoEvent) -> Result<Value, String> {
        let anchor_frame = |id| {
            self.anchors
                .get(&id)
                .map(|time| time.frames())
                .ok_or_else(|| format!("Rule event refers to missing Anchor {id}"))
        };
        let mut row = match event {
            DuoEvent::AnchorJudged(value) => {
                let mut row = judgement(value, &self.hits)?;
                row["type"] = "anchor_judged".into();
                row["anchor_frame"] = anchor_frame(value.anchor_id)?.into();
                row
            }
            DuoEvent::FreeSync(value) => {
                let (p1_index, p1_time) = recorded_hit(&self.hits, PlayerId::P1, value.p1_input)?;
                let (p2_index, p2_time) = recorded_hit(&self.hits, PlayerId::P2, value.p2_input)?;
                json!({
                    "type": "free_sync",
                    "p1": {"input_seq": value.p1_input, "input_fact_index": p1_index, "input_song_time_frames": p1_time.frames()},
                    "p2": {"input_seq": value.p2_input, "input_fact_index": p2_index, "input_song_time_frames": p2_time.frames()},
                    "delta_frames": value.delta_frames, "midpoint_frames": value.midpoint.frames(),
                })
            }
            DuoEvent::AnchorSync(value) => json!({
                "type": "anchor_sync", "anchor_id": value.anchor_id,
                "anchor_frame": anchor_frame(value.anchor_id)?,
                "p1": judgement(value.p1, &self.hits)?, "p2": judgement(value.p2, &self.hits)?,
                "relative_delta_frames": value.relative_delta_frames,
            }),
        };
        row["event_index"] = (index + 1).into();
        Ok(row)
    }
}

pub(crate) fn summary(
    anchor_count: usize,
    replay: &Replay,
    engine: &DuoEngine,
) -> Result<Value, String> {
    let mut hit_counts = [0_u64; 2];
    let mut watermarks = [None; 2];
    for fact in replay.facts() {
        match fact {
            DuoInput::Hit(hit) => hit_counts[hit.player.index()] += 1,
            DuoInput::Watermark {
                player, through, ..
            } => {
                watermarks[player.index()] = Some(through.frames());
            }
        }
    }
    let mut judged = 0;
    let mut free_sync = 0;
    let mut anchor_sync = 0;
    for event in engine.events() {
        match event {
            DuoEvent::AnchorJudged(_) => judged += 1,
            DuoEvent::FreeSync(_) => free_sync += 1,
            DuoEvent::AnchorSync(_) => anchor_sync += 1,
        }
    }
    if judged % 2 != 0 {
        return Err("Rule events contain an incomplete pair of Anchor judgements".into());
    }
    let confirmed = judged / 2;
    let pending = anchor_count
        .checked_sub(confirmed)
        .ok_or("Rule events exceed the source Anchor count")?;
    let resonance = engine.resonance();
    Ok(json!({
        "type": "summary", "fact_count": replay.facts().len(), "event_count": engine.events().len(),
        "hit_counts": hit_counts, "anchor_count": anchor_count,
        "confirmed_anchor_count": confirmed, "pending_anchor_count": pending,
        "free_sync_count": free_sync, "anchor_sync_count": anchor_sync,
        "last_watermarks_frames": watermarks,
        "resonance": {
            "through_frames": resonance.through.map(SongTime::frames), "inputs": resonance.inputs,
            "matched_pairs": resonance.matched_pairs, "mutual_match_per_mille": resonance.mutual_match_per_mille,
            "activity_per_mille": resonance.activity_per_mille, "level_per_mille": resonance.level_per_mille,
        },
    }))
}

fn recorded_hit(
    hits: &BTreeMap<(PlayerId, u64), (usize, SongTime)>,
    player: PlayerId,
    seq: u64,
) -> Result<(usize, SongTime), String> {
    hits.get(&(player, seq)).copied().ok_or_else(|| {
        format!(
            "Rule event refers to missing player {} Hit {seq}",
            player.index() + 1
        )
    })
}

fn judgement(
    value: AnchorJudgement,
    hits: &BTreeMap<(PlayerId, u64), (usize, SongTime)>,
) -> Result<Value, String> {
    let input = value
        .input_seq
        .map(|seq| recorded_hit(hits, value.player, seq))
        .transpose()?;
    Ok(json!({
        "player": value.player.index() + 1, "anchor_id": value.anchor_id, "input_seq": value.input_seq,
        "input_fact_index": input.map(|(index, _)| index),
        "input_song_time_frames": input.map(|(_, time)| time.frames()),
        "offset_frames": value.offset_frames,
        "grade": match value.grade {
            AnchorGrade::Precise => "precise", AnchorGrade::Good => "good",
            AnchorGrade::LateOrEarly => "late_or_early", AnchorGrade::Miss => "miss",
        },
    }))
}

fn write_line(
    writer: &mut impl Write,
    bytes: &mut usize,
    lines: &mut usize,
    value: Value,
) -> Result<(), String> {
    let mut encoded = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    encoded.push(b'\n');
    if encoded.len() > MAX_LINE_BYTES {
        return Err("Replay diagnostic line exceeds 8 KiB including newline".into());
    }
    if *lines >= MAX_REPORT_LINES || encoded.len() > MAX_REPORT_BYTES - *bytes {
        return Err("Replay diagnostic exceeds its line count or 128 MiB byte limit".into());
    }
    writer
        .write_all(&encoded)
        .map_err(|error| format!("Cannot write Replay diagnostic: {error}"))?;
    *bytes += encoded.len();
    *lines += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_media::PackageBuildInput;
    use cocobeat_replay::ReplayIdentity;
    use cocobeat_schema::{
        Anchor, CompiledChart, EnergySample, Hit, MusicAnalysis, SectionCue, SessionEpoch,
    };
    use std::{
        io,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        package: ValidatedPackage,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "cocobeat-replay-diagnostic-{}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let source = root.join("source");
            let audio = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/synthetic/media-import/stereo-canonical.ogg");
            let package = cocobeat_media::build_package(audio, 4_800, &source, |_, prepared| Ok(PackageBuildInput {
                song_id: "replay-diagnostic-test".into(), importer_version: "test-v1".into(),
                analysis_version: "test-v1".into(), chart_version: "test-v1".into(),
                analysis: MusicAnalysis {
                    capabilities: None,
            tempo_regions: Vec::new(),
            repetitions: Vec::new(),
            schema_version: 1, audio_hash: prepared.asset.blake3,
                    beats: vec![], onsets: vec![], sections: vec![],
                    energy: vec![EnergySample { start: SongTime::ZERO, frames: 4_800, rms: [0.0; 2], peak: [0.0; 2] }],
                    diagnostics: "Constructed rule fixture with placeholder energy, not measured music evidence".into(),
                },
                chart: CompiledChart {
                    schema_version: 1, audio_hash: prepared.asset.blake3, ruleset_id: RULES_ID.into(),
                    anchors: vec![Anchor { id: 7, song_time: SongTime::from_frames(1_200) }],
                    sections: vec![SectionCue { id: 9, time: SongTime::from_frames(2_400), label: "authored cue".into() }],
                },
            })).unwrap();
            Self {
                root,
                source,
                package,
            }
        }

        fn replay(&self, facts: &[DuoInput]) -> Replay {
            let hash: String = self
                .package
                .manifest
                .package_hash
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            let mut replay = Replay::new(
                ReplayIdentity {
                    content_id: format!("package-blake3:{hash}"),
                    rules_id: RULES_ID.into(),
                    build_id: "test\0é".into(),
                    stage_compiler_version: None,
                },
                SessionEpoch(17),
            )
            .unwrap();
            for fact in facts {
                replay.record(*fact).unwrap();
            }
            replay
        }

        fn run(&self, name: &str, replay: &Replay) -> (Vec<u8>, Vec<Value>) {
            let input = self.root.join(format!("{name}.replay"));
            let output = self.root.join(format!("{name}.jsonl"));
            replay.save(&input).unwrap();
            inspect(&self.source, &input, &output).unwrap();
            let bytes = fs::read(output).unwrap();
            assert_eq!(bytes.last(), Some(&b'\n'));
            let rows = bytes
                .split(|&byte| byte == b'\n')
                .filter(|line| !line.is_empty())
                .map(|line| serde_json::from_slice(line).unwrap())
                .collect();
            (bytes, rows)
        }

        fn source_bytes(&self) -> [Vec<u8>; 4] {
            [
                "song.audio.ogg",
                "analysis.bin",
                "chart.bin",
                "song.package",
            ]
            .map(|name| fs::read(self.source.join(name)).unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn hit(player: PlayerId, seq: u64, frame: i64) -> DuoInput {
        DuoInput::Hit(Hit {
            epoch: SessionEpoch(17),
            player,
            seq,
            song_time: SongTime::from_frames(frame),
        })
    }

    fn watermark(player: PlayerId, frame: i64) -> DuoInput {
        DuoInput::Watermark {
            epoch: SessionEpoch(17),
            player,
            through: SongTime::from_frames(frame),
        }
    }

    fn complete_facts() -> [DuoInput; 6] {
        [
            hit(PlayerId::P2, u64::MAX, 1_201),
            hit(PlayerId::P1, 7, 1_200),
            hit(PlayerId::P1, 8, 3_000),
            hit(PlayerId::P2, 0, 3_002),
            watermark(PlayerId::P2, 25_681),
            watermark(PlayerId::P1, 25_681),
        ]
    }

    #[test]
    fn explicit_stage_reports_preserve_core_rows_and_reconstruct_geometry() {
        let fixture = Fixture::new();
        let legacy = fixture.replay(&complete_facts());
        let (_, legacy_rows) = fixture.run("stage-legacy", &legacy);
        assert_eq!(legacy_rows[0]["report_version"], 1);
        assert!(legacy_rows[0].get("stage_compiler_version").is_none());
        assert!(
            crate::stage::inspect_replay(
                &fixture.source,
                &fixture.root.join("stage-legacy.replay"),
                "0"
            )
            .unwrap_err()
            .contains("no Stage compiler identity")
        );
        for version in [1, 2] {
            let mut identity = legacy.identity().clone();
            identity.stage_compiler_version = Some(version);
            let mut recording = Replay::new(identity, legacy.epoch()).unwrap();
            for fact in legacy.facts() {
                recording.record(*fact).unwrap();
            }
            let name = format!("stage-{version}");
            let (_, rows) = fixture.run(&name, &recording);
            assert_eq!(rows[0]["report_version"], 2);
            assert_eq!(rows[0]["stage_compiler_version"], version);
            assert_eq!(rows[1..], legacy_rows[1..]);
            let path = fixture.root.join(format!("{name}.replay"));
            crate::stage::inspect_replay(&fixture.source, &path, "0").unwrap();
            crate::stage::inspect_replay(&fixture.source, &path, "4800").unwrap();
            assert!(crate::stage::inspect_replay(&fixture.source, &path, "4801").is_err());
        }
    }

    #[test]
    fn real_package_diagnostic_preserves_input_order_and_exact_core_results() {
        let fixture = Fixture::new();
        let source = fixture.source_bytes();
        let replay = fixture.replay(&complete_facts());
        let original_replay = replay.encode().unwrap();
        let (bytes, rows) = fixture.run("complete", &replay);
        let (loaded, engine) =
            load(&fixture.package, &fixture.root.join("complete.replay")).unwrap();
        let mut shared = Vec::new();
        write_report(
            &mut shared,
            &fixture.package,
            &loaded,
            &engine,
            DuoRules::default(),
            None,
        )
        .unwrap();
        assert_eq!(loaded, replay);
        assert_eq!(shared, bytes);
        assert_eq!(rows.len(), 12);
        assert_eq!(rows[0]["report_version"], 1);
        assert_eq!(rows[0]["build_id"], "test\0é");
        assert_eq!(rows[0]["confirmation_delay_frames"], 20_880);
        assert_eq!(rows[1]["player"], 2);
        assert_eq!(rows[1]["seq"], json!(u64::MAX));
        assert_eq!(rows[1]["song_time_frames"], 1_201);
        assert_eq!(rows[2]["player"], 1);
        for (index, row) in rows[1..7].iter().enumerate() {
            assert_eq!(row["fact_index"], index + 1);
        }
        assert_eq!(
            rows[7],
            json!({
                "type": "anchor_judged", "event_index": 1, "anchor_id": 7, "anchor_frame": 1_200,
                "player": 1, "input_seq": 7, "input_fact_index": 2, "input_song_time_frames": 1_200,
                "offset_frames": 0, "grade": "precise",
            })
        );
        assert_eq!(rows[8]["input_fact_index"], 1);
        assert_eq!(rows[8]["offset_frames"], 1);
        assert_eq!(rows[9]["type"], "anchor_sync");
        assert_eq!(rows[9]["relative_delta_frames"], 1);
        assert_eq!(rows[9]["p2"]["input_seq"], json!(u64::MAX));
        assert_eq!(
            rows[10],
            json!({
                "type": "free_sync", "event_index": 4,
                "p1": {"input_seq": 8, "input_fact_index": 3, "input_song_time_frames": 3_000},
                "p2": {"input_seq": 0, "input_fact_index": 4, "input_song_time_frames": 3_002},
                "delta_frames": 2, "midpoint_frames": 3_001,
            })
        );
        assert_eq!(
            rows[11],
            json!({
                "type": "summary", "fact_count": 6, "event_count": 4, "hit_counts": [2,2],
                "anchor_count": 1, "confirmed_anchor_count": 1, "pending_anchor_count": 0,
                "free_sync_count": 1, "anchor_sync_count": 1, "last_watermarks_frames": [25_681,25_681],
                "resonance": {"through_frames": 4_801, "inputs": [2,2], "matched_pairs": 2,
                    "mutual_match_per_mille": 1_000, "activity_per_mille": 250, "level_per_mille": 250},
            })
        );
        let again = fixture.root.join("again.jsonl");
        inspect(
            &fixture.source,
            &fixture.root.join("complete.replay"),
            &again,
        )
        .unwrap();
        assert_eq!(fs::read(again).unwrap(), bytes);
        assert_eq!(fixture.source_bytes(), source);
        assert_eq!(
            fs::read(fixture.root.join("complete.replay")).unwrap(),
            original_replay
        );
    }

    #[test]
    fn partial_and_extreme_watermarks_preserve_pending_and_null_facts() {
        let fixture = Fixture::new();
        for (name, facts, watermarks, through, pending) in [
            ("empty", vec![], json!([null, null]), Value::Null, 1),
            (
                "one-side",
                vec![watermark(PlayerId::P1, -2_400)],
                json!([-2_400, null]),
                Value::Null,
                1,
            ),
            (
                "minimum",
                vec![
                    watermark(PlayerId::P1, i64::MIN),
                    watermark(PlayerId::P2, i64::MIN),
                ],
                json!([i64::MIN, i64::MIN]),
                Value::Null,
                1,
            ),
            (
                "negative",
                vec![watermark(PlayerId::P1, -1), watermark(PlayerId::P2, -1)],
                json!([-1, -1]),
                json!(-20_881),
                1,
            ),
            (
                "maximum",
                vec![
                    watermark(PlayerId::P1, i64::MAX),
                    watermark(PlayerId::P2, i64::MAX),
                ],
                json!([i64::MAX, i64::MAX]),
                json!(i64::MAX - 20_880),
                0,
            ),
        ] {
            let (_, rows) = fixture.run(name, &fixture.replay(&facts));
            let summary = rows.last().unwrap();
            assert_eq!(summary["last_watermarks_frames"], watermarks, "{name}");
            assert_eq!(summary["resonance"]["through_frames"], through, "{name}");
            assert_eq!(summary["pending_anchor_count"], pending, "{name}");
            let confirmed = 1 - pending;
            assert_eq!(summary["confirmed_anchor_count"], confirmed, "{name}");
            assert_eq!(summary["event_count"], 2 * confirmed, "{name}");
            assert_eq!(rows.len(), facts.len() + 2 + 2 * confirmed, "{name}");
            assert!(summary.get("finished").is_none());
            for player in 1..=2 * confirmed {
                assert_eq!(
                    rows[facts.len() + player],
                    json!({
                        "type": "anchor_judged", "event_index": player,
                        "anchor_id": 7, "anchor_frame": 1_200, "player": player,
                        "input_seq": null, "input_fact_index": null,
                        "input_song_time_frames": null, "offset_frames": null, "grade": "miss",
                    })
                );
            }
        }
        for (through, events, pending) in [(9_839, 0, 1), (9_840, 3, 0)] {
            let facts = [
                watermark(PlayerId::P1, -1),
                hit(PlayerId::P2, u64::MAX, 1_201),
                hit(PlayerId::P1, 7, 1_200),
                watermark(PlayerId::P1, 25_681),
                watermark(PlayerId::P2, through),
            ];
            let (_, rows) = fixture.run(&format!("partial-{through}"), &fixture.replay(&facts));
            let summary = rows.last().unwrap();
            assert_eq!(summary["fact_count"], 5);
            assert_eq!(summary["hit_counts"], json!([1, 1]));
            assert_eq!(summary["event_count"], events);
            assert_eq!(summary["confirmed_anchor_count"], 1 - pending);
            assert_eq!(summary["pending_anchor_count"], pending);
            assert_eq!(summary["free_sync_count"], 0);
            assert_eq!(rows.len(), 7 + events);
            assert_eq!(
                summary["resonance"],
                json!({
                    "through_frames": through - 20_880, "inputs": [0, 0], "matched_pairs": 0,
                    "mutual_match_per_mille": 0, "activity_per_mille": 0, "level_per_mille": 0,
                })
            );
            if events == 3 {
                assert_eq!(rows[6]["input_fact_index"], 3);
                assert_eq!(rows[6]["offset_frames"], 0);
                assert_eq!(rows[7]["input_fact_index"], 2);
                assert_eq!(rows[7]["input_seq"], json!(u64::MAX));
                assert_eq!(rows[7]["offset_frames"], 1);
                assert_eq!(rows[8]["type"], "anchor_sync");
                assert_eq!(rows[8]["relative_delta_frames"], 1);
            }
        }
    }

    #[test]
    fn invalid_histories_and_output_paths_leave_sources_and_existing_files_intact() {
        let fixture = Fixture::new();
        let source = fixture.source_bytes();
        let input = fixture.root.join("source.replay");
        let output = fixture.root.join("report.jsonl");
        for (facts, cause) in [
            (vec![hit(PlayerId::P1, 1, -1)], "Replay fact 1: Hit"),
            (vec![hit(PlayerId::P1, 1, 4_800)], "Replay fact 1: Hit"),
            (
                vec![hit(PlayerId::P1, 1, 1), hit(PlayerId::P1, 1, 1)],
                "Replay fact 2: DuplicateHit",
            ),
            (
                vec![watermark(PlayerId::P1, 1), hit(PlayerId::P1, 1, 1)],
                "Replay fact 2: ClosedHistory",
            ),
        ] {
            fixture.replay(&facts).save(&input).unwrap();
            let before = fs::read(&input).unwrap();
            let error = inspect(&fixture.source, &input, &output).unwrap_err();
            assert!(error.contains(cause), "{error}");
            assert!(!output.exists());
            assert_eq!(fs::read(&input).unwrap(), before);
        }
        for wrong_rules in [false, true] {
            let mut identity = fixture.replay(&[]).identity().clone();
            if wrong_rules {
                identity.rules_id = "other-rules".into();
            } else {
                identity.content_id = "other-content".into();
            }
            Replay::new(identity, SessionEpoch(17))
                .unwrap()
                .save(&input)
                .unwrap();
            assert!(
                inspect(&fixture.source, &input, &output)
                    .unwrap_err()
                    .contains("identity mismatch")
            );
        }
        fixture.replay(&complete_facts()).save(&input).unwrap();
        let original_replay = fs::read(&input).unwrap();
        fs::write(&output, b"keep").unwrap();
        assert!(inspect(&fixture.source, &input, &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"keep");
        assert!(inspect(&fixture.source, &input, &input).is_err());
        assert!(inspect(&fixture.source, &input, &fixture.source.join("new.jsonl")).is_err());
        assert!(
            inspect(
                &fixture.source,
                &input,
                &fixture.root.join("missing/report.jsonl")
            )
            .is_err()
        );
        assert!(
            inspect(
                &fixture.source,
                &fixture.root,
                &fixture.root.join("directory-input.jsonl")
            )
            .is_err()
        );
        let empty = fixture.root.join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(inspect(&fixture.source, &input, &empty).is_err());
        fs::write(empty.join("keep"), b"previous").unwrap();
        assert!(inspect(&fixture.source, &input, &empty).is_err());
        assert_eq!(fs::read(empty.join("keep")).unwrap(), b"previous");
        #[cfg(unix)]
        {
            let alias = fixture.root.join("source-alias");
            std::os::unix::fs::symlink(&fixture.source, &alias).unwrap();
            assert!(inspect(&fixture.source, &input, &alias.join("new.jsonl")).is_err());
            let occupied = fixture.root.join("occupied-link");
            std::os::unix::fs::symlink(&output, &occupied).unwrap();
            assert!(inspect(&fixture.source, &input, &occupied).is_err());
            assert!(
                fs::symlink_metadata(occupied)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        assert_eq!(fixture.source_bytes(), source);
        assert_eq!(fs::read(&input).unwrap(), original_replay);
        assert_eq!(fs::read_dir(&fixture.source).unwrap().count(), 4);
    }

    #[test]
    fn explicit_timing_keeps_core_rows_and_rejects_changed_raw_replay() {
        use cocobeat_replay::timing::{
            AudioRead, CallbackPublication, CaptureTiming, InputKind, MappingAnchor,
            SourcePublication, TimingClock, TimingPhase,
        };

        let fixture = Fixture::new();
        for stage in [None, Some(1), Some(2)] {
            let original = fixture.replay(&complete_facts());
            let mut identity = original.identity().clone();
            identity.stage_compiler_version = stage;
            let mut recording = Replay::new(identity, original.epoch()).unwrap();
            for fact in original.facts() {
                recording.record(*fact).unwrap();
            }
            let name = format!("timing-{stage:?}");
            let (default_bytes, default_rows) = fixture.run(&name, &recording);
            let path = fixture.root.join(format!("{name}.replay"));
            let raw = fs::read(&path).unwrap();
            let timing_path = fixture.root.join(format!("{name}.timing.json"));
            let mut timing = TimingSidecar::new(
                &recording,
                &raw,
                4_800,
                vec![1],
                TimingClock {
                    max_extrapolation_ns: 250_000_000,
                    max_drift_ppm: 1_000,
                    history_capacity: 256,
                },
            )
            .unwrap();
            for (index, fact) in recording.facts().iter().enumerate() {
                if let DuoInput::Hit(hit) = fact
                    && hit.player == PlayerId::P1
                {
                    let observed_ns = hit.song_time.frames() as u64 * 1_000_000_000 / 48_000;
                    timing.captures.push(CaptureTiming {
                        fact_index: index as u64 + 1,
                        player: 1,
                        seq: hit.seq,
                        song_time_frames: hit.song_time.frames(),
                        observed_ns,
                        consumed_ns: observed_ns + 1_000_000,
                        uncertainty_frames: 2_402,
                        input_kind: InputKind::KeyboardMessage,
                        mapping_anchor: MappingAnchor {
                            monotonic_ns: 0,
                            song_time_frames: 0,
                            uncertainty_frames: 2_400,
                        },
                    });
                }
            }
            for before in [10_000_000, 70_000_000] {
                timing.audio_history.push(AudioRead {
                    read_before_ns: before,
                    read_after_ns: before + 1_000_000,
                    phase: TimingPhase::Running,
                    source: Some(SourcePublication {
                        generation: 1,
                        source_id: 1,
                        sequence: 2,
                        position_seconds: 0.001,
                        published_before_ns: 1_000_000,
                        published_after_ns: 2_000_000,
                    }),
                    callback: Some(CallbackPublication {
                        generation: 1,
                        sequence: 7,
                        observed_ns: 3_000_000,
                        previous_observed_ns: Some(2_000_000),
                        previous_frames: Some(48),
                        output_sample_rate: 48_000,
                    }),
                });
            }
            timing
                .save_new(&timing_path, &recording, &raw, 4_800)
                .unwrap();
            let output = fixture.root.join(format!("{name}.timed.jsonl"));
            inspect_with_timing(&fixture.source, &path, &output, Some(&timing_path)).unwrap();
            let mut rows: Vec<Value> = fs::read(&output)
                .unwrap()
                .split(|&byte| byte == b'\n')
                .filter(|line| !line.is_empty())
                .map(|line| serde_json::from_slice(line).unwrap())
                .collect();
            assert_eq!(rows[0]["report_version"], 3);
            assert_eq!(
                rows[0]["local_timing"]["stage_compiler_version"],
                json!(stage)
            );
            assert_eq!(rows[1]["local_timing"]["status"], "not_recorded");
            assert_eq!(rows[2]["local_timing"]["capture"]["fact_index"], 2);
            assert_eq!(
                rows[2]["local_timing"]["software_message_consumption_wait_ns"],
                1_000_000
            );
            assert_eq!(rows[9]["p1"]["local_timing"]["capture"]["seq"], 7);
            assert_eq!(rows[9]["p2"]["local_timing"]["status"], "not_recorded");
            assert_eq!(rows[10]["p1"]["local_timing"]["capture"]["seq"], 8);
            let history = &rows[2]["local_timing"]["independent_audio_read_history"];
            assert_eq!(history["before_observation"]["source"]["sequence"], 2);
            assert_eq!(history["before_observation"]["callback"]["sequence"], 7);
            assert_eq!(
                history["at_or_after_observation"]["read_before_ns"],
                70_000_000
            );
            for (index, row) in rows.iter_mut().enumerate() {
                if index == 0 {
                    row["report_version"] = report_version(&recording).into();
                }
                row.as_object_mut().unwrap().remove("local_timing");
                for player in ["p1", "p2"] {
                    if let Some(pair) = row.get_mut(player).and_then(Value::as_object_mut) {
                        pair.remove("local_timing");
                    }
                }
            }
            assert_eq!(rows, default_rows);
            let explicit_none = fixture.root.join(format!("{name}.none.jsonl"));
            inspect_with_timing(&fixture.source, &path, &explicit_none, None).unwrap();
            assert_eq!(fs::read(explicit_none).unwrap(), default_bytes);
            let mut changed_raw = raw.clone();
            changed_raw.push(b'\n');
            fs::write(&path, changed_raw).unwrap();
            let rejected = fixture.root.join(format!("{name}.rejected.jsonl"));
            assert!(
                inspect_with_timing(&fixture.source, &path, &rejected, Some(&timing_path))
                    .unwrap_err()
                    .contains("byte identity differs")
            );
            assert!(!rejected.exists());
        }
    }

    #[test]
    fn line_limits_include_newlines_and_failed_output_is_removed() {
        let mut output = Vec::new();
        let (mut bytes, mut lines) = (0, 0);
        write_line(
            &mut output,
            &mut bytes,
            &mut lines,
            json!("x".repeat(8_189)),
        )
        .unwrap();
        assert_eq!(output.len(), 8_192);
        let before = output.clone();
        assert!(
            write_line(
                &mut output,
                &mut bytes,
                &mut lines,
                json!("x".repeat(8_190))
            )
            .is_err()
        );
        assert_eq!(output, before);
        (bytes, lines) = (MAX_REPORT_BYTES - 5, 0);
        write_line(&mut output, &mut bytes, &mut lines, Value::Null).unwrap();
        assert_eq!(bytes, 134_217_728);
        let before = output.clone();
        assert!(write_line(&mut output, &mut bytes, &mut lines, Value::Null).is_err());
        assert_eq!(output, before);
        (bytes, lines) = (0, MAX_REPORT_LINES - 1);
        write_line(&mut output, &mut bytes, &mut lines, Value::Null).unwrap();
        assert_eq!(lines, 540_002);
        assert!(write_line(&mut output, &mut bytes, &mut lines, Value::Null).is_err());

        struct FailingWriter(usize);
        impl Write for FailingWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.0 == 0 {
                    return Err(io::Error::other("controlled write failure"));
                }
                let count = self.0.min(bytes.len());
                self.0 -= count;
                Ok(count)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let fixture = Fixture::new();
        let replay = fixture.replay(&complete_facts());
        let engine = replay
            .replay(
                &replay.identity().content_id,
                RULES_ID,
                fixture.package.chart.anchors.clone(),
                DuoRules::default(),
            )
            .unwrap();
        assert!(
            write_report(
                &mut FailingWriter(1_024),
                &fixture.package,
                &replay,
                &engine,
                DuoRules::default(),
                None
            )
            .unwrap_err()
            .contains("controlled write failure")
        );
        let mut inconsistent = fixture.package.clone();
        inconsistent.chart.anchors.clear();
        let destination = fixture.root.join("partial.jsonl");
        assert!(
            write_new_report(
                &destination,
                &inconsistent,
                &replay,
                &engine,
                DuoRules::default(),
                None
            )
            .unwrap_err()
            .contains("missing Anchor")
        );
        assert!(!destination.exists());
        fs::write(&destination, b"keep").unwrap();
        assert!(
            write_new_report(
                &destination,
                &inconsistent,
                &replay,
                &engine,
                DuoRules::default(),
                None
            )
            .is_err()
        );
        assert_eq!(fs::read(destination).unwrap(), b"keep");
    }
}
