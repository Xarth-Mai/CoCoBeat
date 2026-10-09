//! Read-only review of the exact native candidate evidence bound to package analysis

use crate::ValidatedPackage;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NativeBeatEvidence {
    pub metadata: NativeBeatMetadata,
    pub records: Vec<NativeBeatRecord>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NativeBeatMetadata {
    pub profile: String,
    pub channel: usize,
    pub canonical_frames: u64,
    pub resampled_frames: u64,
    pub spectrogram_frames: usize,
    pub audio_blake3: String,
    pub model_blake3: String,
    pub summary_blake3: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeBeatKind {
    Beat,
    RawDownbeat,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NativePeakMember {
    pub q: usize,
    pub raw_logit: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NativeDownbeatAlignment {
    pub beat_group_index: usize,
    pub beat_q: f64,
    pub beat_frame: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NativeBeatRecord {
    pub kind: NativeBeatKind,
    pub group_index: usize,
    pub original_q: f64,
    pub frame: i64,
    pub uncalibrated_score: f32,
    pub members: Vec<NativePeakMember>,
    pub alignment: Option<NativeDownbeatAlignment>,
    pub package_downbeat_score: Option<f32>,
}

/// Reads evidence without inference or decoding audio; chart/CID edits do not invalidate preserved analysis
pub fn read_native_beat_evidence(
    package: &ValidatedPackage,
    evidence: &Path,
) -> Result<NativeBeatEvidence, String> {
    #[cfg(any(
        all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"),
        all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"),
        all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"),
        all(target_os = "windows", target_arch = "aarch64", target_env = "msvc")
    ))]
    {
        backend::read_native_beat_evidence(package, evidence)
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"),
        all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"),
        all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"),
        all(target_os = "windows", target_arch = "aarch64", target_env = "msvc")
    )))]
    {
        let _ = (package, evidence);
        Err("Unsupported experimental native beat evidence platform".into())
    }
}

#[cfg(any(
    all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"),
    all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"),
    all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"),
    all(target_os = "windows", target_arch = "aarch64", target_env = "msvc")
))]
mod backend {
    use super::*;
    use crate::native_beat;
    use cocobeat_schema::{
        AnalysisCapability, AnalysisSource, AnalysisState, MAX_CANONICAL_FRAMES,
    };
    use serde::Deserialize;
    use std::{
        fs::{self, File},
        io::Read,
    };

    const FILES: [&str; 6] = [
        "native-shape.json",
        "native-spect.f32",
        "aggregate-beat.f32",
        "aggregate-downbeat.f32",
        "minimal-raw-groups.json",
        "minimal-alignment.json",
    ];
    const SUMMARY_LIMIT: u64 = 16_384;
    const SHAPE_LIMIT: u64 = 4_096;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Summary {
        profile: String,
        model_blake3: String,
        audio_blake3: String,
        files: Vec<Resource>,
        confidence: serde_json::Value,
        production_admission: bool,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Resource {
        name: String,
        bytes: u64,
        blake3: String,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Shape {
        #[serde(rename = "N")]
        n: u64,
        #[serde(rename = "M")]
        m: u64,
        #[serde(rename = "F")]
        f: usize,
        shape: [usize; 3],
        channel: usize,
        profile: String,
        old_frontend_numeric: String,
        old_music_quality: String,
        confidence: serde_json::Value,
        production_admission: bool,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Groups {
        beat: Vec<native_beat::PeakGroup>,
        downbeat: Vec<native_beat::PeakGroup>,
        score_policy: String,
        confidence: serde_json::Value,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Alignment {
        original_downbeat_q: f64,
        nearest_beat_index: usize,
        aligned_beat_q: f64,
        canonical_frame: i64,
    }

    // Author source_note can contain diagnostic-looking text; only the producer's final annotation owns these keys
    fn annotation<'a>(diagnostics: &'a str, profile: &str) -> Result<&'a str, String> {
        let marker = format!("; experimental={profile};");
        let start = diagnostics
            .rfind(marker.as_str())
            .ok_or("Missing final native producer annotation")?;
        Ok(&diagnostics[start + 2..])
    }

    fn diagnostic<'a>(diagnostics: &'a str, key: &str) -> Result<&'a str, String> {
        let prefix = format!("{key}=");
        let mut matches = diagnostics
            .split(';')
            .filter_map(|v| v.trim().strip_prefix(prefix.as_str()));
        let value = matches
            .next()
            .ok_or_else(|| format!("Missing native diagnostic {key}"))?;
        if matches.next().is_some() {
            return Err(format!("Duplicate native diagnostic {key}"));
        }
        Ok(value)
    }

    fn hash(value: &str) -> Result<blake3::Hash, String> {
        let hash = blake3::Hash::from_hex(value).map_err(|e| format!("Native BLAKE3: {e}"))?;
        if hash.to_hex().as_str() != value {
            return Err("Native BLAKE3 must use canonical lowercase hex".into());
        }
        Ok(hash)
    }

    fn open(path: &Path, limit: u64) -> Result<(File, u64), String> {
        let metadata =
            fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !metadata.is_file() || metadata.len() > limit {
            return Err(format!(
                "Native evidence is not a bounded regular file: {}",
                path.display()
            ));
        }
        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() > limit {
            return Err("Native evidence changed type or exceeded length limit".into());
        }
        Ok((file, metadata.len()))
    }

    fn read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
        let (mut file, length) = open(path, limit)?;
        let mut bytes = vec![0; usize::try_from(length).map_err(|e| e.to_string())?];
        file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        if file.read(&mut [0]).map_err(|e| e.to_string())? != 0 {
            return Err("Native evidence changed length while read".into());
        }
        Ok(bytes)
    }

    fn read_resource(root: &Path, resource: &Resource, limit: u64) -> Result<Vec<u8>, String> {
        if resource.bytes > limit {
            return Err(format!("Native resource exceeds limit: {}", resource.name));
        }
        let bytes = read(&root.join(&resource.name), limit)?;
        if bytes.len() as u64 != resource.bytes || blake3::hash(&bytes) != hash(&resource.blake3)? {
            return Err(format!(
                "Native resource identity mismatch: {}",
                resource.name
            ));
        }
        Ok(bytes)
    }

    fn verify_spect(root: &Path, resource: &Resource, length: u64) -> Result<(), String> {
        if resource.bytes != length {
            return Err("Native spect length mismatch".into());
        }
        let (mut file, actual) = open(&root.join(&resource.name), length)?;
        if actual != length {
            return Err("Native spect actual length mismatch".into());
        }
        let mut count = 0u64;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0; 32_768];
        loop {
            let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            count += read as u64;
            if count > length {
                return Err("Native spect grew while read".into());
            }
            hasher.update(&buffer[..read]);
        }
        if count != length || hasher.finalize() != hash(&resource.blake3)? {
            return Err("Native spect identity mismatch".into());
        }
        Ok(())
    }

    fn decode_logits(bytes: &[u8], frames: usize) -> Result<Vec<f32>, String> {
        if bytes.len() != frames * 4 {
            return Err("Native aggregate length mismatch".into());
        }
        let values: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&v| f32::from_le_bytes(v))
            .collect();
        if values.iter().any(|v| !v.is_finite()) {
            return Err("Nonfinite native aggregate logits".into());
        }
        Ok(values)
    }

    fn validate_groups(
        groups: &[native_beat::PeakGroup],
        logits: &[f32],
        n: u64,
    ) -> Result<Vec<i64>, String> {
        // Use the producer's running-mean coordinates, retaining the serialized scores
        // A fresh exp may differ in its last bits across platforms and is not a confidence check
        let actual = native_beat::peaks(logits)?;
        if groups.len() != actual.len()
            || groups.iter().zip(&actual).any(|(group, expected)| {
                group.q != expected.q
                    || group.members != expected.members
                    || !group.score.is_finite()
                    || !(0.5..=1.0).contains(&group.score)
            })
        {
            return Err("Native raw peak members/mean/score mismatch".into());
        }
        let frames: Vec<i64> = groups
            .iter()
            .map(|v| native_beat::canonical_frame(v.q, n))
            .collect::<Result<_, _>>()?;
        if frames.windows(2).any(|v| v[0] >= v[1]) {
            return Err("Native peak frames duplicate or descend".into());
        }
        Ok(frames)
    }

    /// Reads evidence without inference or decoding audio; chart/CID edits do not invalidate preserved analysis
    pub(super) fn read_native_beat_evidence(
        package: &ValidatedPackage,
        evidence: &Path,
    ) -> Result<NativeBeatEvidence, String> {
        if !fs::symlink_metadata(evidence)
            .map_err(|e| e.to_string())?
            .is_dir()
        {
            return Err("Native evidence root must be a regular directory".into());
        }
        let n = package.manifest.canonical_frames;
        if !(1..=MAX_CANONICAL_FRAMES).contains(&n) {
            return Err("Invalid native canonical extent".into());
        }
        package.analysis.validate(n)?;
        let candidate = AnalysisCapability {
            state: AnalysisState::Candidate,
            source: AnalysisSource::Algorithm,
            confidence: None,
        };
        let unsupported = AnalysisCapability {
            state: AnalysisState::Unsupported,
            ..candidate
        };
        let capabilities = package
            .analysis
            .capabilities
            .ok_or("Native evidence requires v2 capabilities")?;
        let profile = package.manifest.analysis_version.as_str();
        let profile = if package.analysis.presentation.is_some() {
            [
                crate::PRESENTATION_ANALYSIS_PROFILE,
                "canonical-chroma-8192-v1-candidate",
            ]
            .iter()
            .find_map(|suffix| profile.strip_suffix(&format!("+{suffix}")))
            .unwrap_or(profile)
        } else {
            profile
        };
        let include_auto = match profile {
            native_beat::ANALYSIS_VERSION => false,
            native_beat::AUTO_ANALYSIS_VERSION => true,
            _ => return Err("Unsupported native analysis profile".into()),
        };
        let onset = if include_auto && n >= crate::native_onset::MIN_ONSET_FRAMES {
            candidate
        } else {
            unsupported
        };
        if capabilities.beat != candidate
            || capabilities.downbeat != candidate
            || capabilities.tempo != if include_auto { candidate } else { unsupported }
            || capabilities.onset != onset
            || capabilities.repetition != unsupported
        {
            return Err(
                "Native profile/capability must be Candidate/Algorithm with unknown confidence"
                    .into(),
            );
        }
        let diagnostics = annotation(&package.analysis.diagnostics, profile)?;
        let summary_bytes = read(&evidence.join("evidence-summary.json"), SUMMARY_LIMIT)?;
        let summary_hash = blake3::hash(&summary_bytes);
        if summary_hash != hash(diagnostic(diagnostics, "evidence_summary_blake3")?)? {
            return Err("Native summary does not match package analysis".into());
        }
        let summary: Summary = serde_json::from_slice(&summary_bytes)
            .map_err(|e| format!("Native summary JSON: {e}"))?;
        if summary.profile != profile
            || !summary.confidence.is_null()
            || summary.production_admission
            || hash(&summary.model_blake3)? != blake3::Hash::from_bytes(native_beat::MODEL_BLAKE3)
            || hash(&summary.audio_blake3)?.as_bytes() != &package.manifest.audio.blake3
            || package.analysis.audio_hash != package.manifest.audio.blake3
            || diagnostic(diagnostics, "experimental")? != summary.profile
            || diagnostic(diagnostics, "model_blake3")? != summary.model_blake3
        {
            return Err("Native summary profile/model/audio/nullable admission mismatch".into());
        }
        let mut names = FILES.to_vec();
        if include_auto {
            names.push(native_beat::AUTO_RESOURCE);
        }
        if summary.files.len() != names.len() {
            return Err("Native summary resource count differs from its fixed profile".into());
        }
        let mut resources = Vec::with_capacity(names.len());
        for name in names {
            let mut matching = summary.files.iter().filter(|v| v.name == name);
            let resource = matching
                .next()
                .ok_or_else(|| format!("Missing fixed native resource {name}"))?;
            if matching.next().is_some() {
                return Err(format!("Duplicate fixed native resource {name}"));
            }
            resources.push(resource);
        }
        let shape: Shape =
            serde_json::from_slice(&read_resource(evidence, resources[0], SHAPE_LIMIT)?)
                .map_err(|e| format!("Native shape JSON: {e}"))?;
        let m = (n * 22_050).div_ceil(48_000);
        let f = 1 + m as usize / 441;
        if m <= 512
            || shape.n != n
            || shape.m != m
            || shape.f != f
            || shape.shape != [1, f, 128]
            || shape.channel > 1
            || shape.profile != native_beat::ANALYSIS_VERSION
            || !shape.confidence.is_null()
            || shape.production_admission
            || shape.old_frontend_numeric != "FAIL_PRESERVED_19_OF_28"
            || shape.old_music_quality != "FAIL_PRESERVED"
        {
            return Err("Native N/M/F/channel/profile/nullable shape mismatch".into());
        }
        for (key, value) in [
            ("N", n.to_string()),
            ("M", m.to_string()),
            ("F", f.to_string()),
            ("channel", shape.channel.to_string()),
        ] {
            if diagnostic(diagnostics, key)? != value {
                return Err(format!("Native shape diagnostic mismatch: {key}"));
            }
        }
        verify_spect(evidence, resources[1], f as u64 * 128 * 4)?;
        let mut logits = Vec::with_capacity(2);
        for resource in &resources[2..4] {
            logits.push(decode_logits(
                &read_resource(evidence, resource, f as u64 * 4)?,
                f,
            )?);
        }
        let json_limit = f as u64 * 256 + 4_096;
        let groups: Groups =
            serde_json::from_slice(&read_resource(evidence, resources[4], json_limit)?)
                .map_err(|e| format!("Native groups JSON: {e}"))?;
        if groups.score_policy != "max raw sigmoid(member logit), uncalibrated"
            || !groups.confidence.is_null()
        {
            return Err("Native groups score policy/unknown confidence mismatch".into());
        }
        let beat_frames = validate_groups(&groups.beat, &logits[0], n)?;
        let down_frames = validate_groups(&groups.downbeat, &logits[1], n)?;
        let alignment: Vec<Alignment> =
            serde_json::from_slice(&read_resource(evidence, resources[5], json_limit)?)
                .map_err(|e| format!("Native alignment JSON: {e}"))?;
        if alignment.len() != groups.downbeat.len()
            || groups.beat.len() != package.analysis.beats.len()
            || (groups.beat.is_empty() && !groups.downbeat.is_empty())
        {
            return Err("Native group/alignment/package beat count mismatch".into());
        }
        let mut down_scores = vec![None::<f32>; groups.beat.len()];
        for (group, aligned) in groups.downbeat.iter().zip(&alignment) {
            let index = native_beat::nearest_beat_index(group.q, &groups.beat);
            if aligned.original_downbeat_q != group.q
                || aligned.nearest_beat_index != index
                || aligned.aligned_beat_q != groups.beat[index].q
                || aligned.canonical_frame != beat_frames[index]
            {
                return Err("Native nearest/tie-left alignment mismatch".into());
            }
            down_scores[index] =
                Some(down_scores[index].map_or(group.score, |old| old.max(group.score)));
        }
        for (index, (group, beat)) in groups.beat.iter().zip(&package.analysis.beats).enumerate() {
            if beat.time.frames() != beat_frames[index]
                || beat.strength.to_bits() != group.score.to_bits()
                || beat.downbeat_probability.map(f32::to_bits)
                    != down_scores[index].map(f32::to_bits)
                || beat.confidence.is_some()
            {
                return Err("Native original scores/coordinates do not match package beats".into());
            }
        }
        if include_auto {
            let bytes = read_resource(evidence, resources[6], native_beat::AUTO_RESOURCE_LIMIT)?;
            validate_auto_evidence(package, shape.channel, &bytes)?;
        }
        let mut records = Vec::with_capacity(groups.beat.len() + groups.downbeat.len());
        for (slot, (kind, groups, frames)) in [
            (NativeBeatKind::Beat, &groups.beat, &beat_frames),
            (NativeBeatKind::RawDownbeat, &groups.downbeat, &down_frames),
        ]
        .into_iter()
        .enumerate()
        {
            for (index, group) in groups.iter().enumerate() {
                let aligned = (slot == 1).then(|| &alignment[index]);
                let package_index = aligned.map_or(index, |v| v.nearest_beat_index);
                records.push(NativeBeatRecord {
                    kind,
                    group_index: index,
                    original_q: group.q,
                    frame: frames[index],
                    uncalibrated_score: group.score,
                    members: group
                        .members
                        .iter()
                        .map(|&q| NativePeakMember {
                            q,
                            raw_logit: logits[slot][q],
                        })
                        .collect(),
                    alignment: aligned.map(|v| NativeDownbeatAlignment {
                        beat_group_index: v.nearest_beat_index,
                        beat_q: v.aligned_beat_q,
                        beat_frame: v.canonical_frame,
                    }),
                    package_downbeat_score: package.analysis.beats[package_index]
                        .downbeat_probability,
                });
            }
        }
        records.sort_by_key(|v| v.frame);
        Ok(NativeBeatEvidence {
            metadata: NativeBeatMetadata {
                profile: summary.profile,
                channel: shape.channel,
                canonical_frames: n,
                resampled_frames: m,
                spectrogram_frames: f,
                audio_blake3: summary.audio_blake3,
                model_blake3: summary.model_blake3,
                summary_blake3: summary_hash.to_hex().to_string(),
            },
            records,
        })
    }

    fn validate_auto_evidence(
        package: &ValidatedPackage,
        channel: usize,
        bytes: &[u8],
    ) -> Result<(), String> {
        let auto: native_beat::AutoAnalysisEvidence = serde_json::from_slice(bytes)
            .map_err(|e| format!("Automatic analysis evidence JSON: {e}"))?;
        let n = package.manifest.canonical_frames;
        let reason =
            (n < crate::native_onset::MIN_ONSET_FRAMES).then_some("insufficient_analysis_frames");
        if auto.onset_profile != crate::native_onset::ONSET_PROFILE
            || auto.tempo_profile != crate::native_tempo::INTERBEAT_PROFILE
            || auto.channel != channel
            || auto.canonical_frames != n
            || hash(&auto.audio_blake3)?.as_bytes() != &package.analysis.audio_hash
            || auto.unsupported_reason.as_deref() != reason
            || !auto.confidence.is_null()
            || auto.production_admission
        {
            return Err(
                "Automatic analysis profile/source/support/nullable metadata mismatch".into(),
            );
        }
        let onsets = crate::native_onset::onsets_from_records(&auto.records, n)?;
        if onsets.len() != package.analysis.onsets.len()
            || onsets
                .iter()
                .zip(&package.analysis.onsets)
                .any(|(expected, actual)| {
                    expected.time != actual.time
                        || expected.strength.to_bits() != actual.strength.to_bits()
                        || actual.confidence.is_some()
                })
        {
            return Err("Automatic onset records differ from package analysis".into());
        }
        let tempo = crate::native_tempo::interbeat_tempo(&package.analysis.beats, n)?;
        if tempo.len() != package.analysis.tempo_regions.len()
            || tempo
                .iter()
                .zip(&package.analysis.tempo_regions)
                .any(|(expected, actual)| {
                    expected.start != actual.start
                        || expected.end != actual.end
                        || expected.bpm.to_bits() != actual.bpm.to_bits()
                        || actual.beat_unit.is_some()
                        || actual.confidence.is_some()
                })
        {
            return Err("Automatic tempo differs from the original adjacent beat intervals".into());
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{
            PACKAGE_OBJECT_NAMES, PackageBuildInput, build_package, decode_canonical,
            export_anchors,
        };
        use cocobeat_schema::{
            ANALYSIS_SCHEMA_VERSION, AnalysisCapabilities, Anchor, CONTENT_SCHEMA_VERSION,
            CompiledChart, EnergySample, MusicAnalysis, SongTime,
        };
        use serde_json::{Value, json};
        use std::{
            path::PathBuf,
            sync::atomic::{AtomicU64, Ordering},
        };

        const STEREO: &[u8] =
            include_bytes!("../../../testdata/synthetic/media-import/stereo-canonical.ogg");
        static NEXT: AtomicU64 = AtomicU64::new(0);

        // Controlled aggregate logits exercise file binding and mapping, not model/music quality
        #[test]
        fn presentation_profiles_are_exact_and_require_their_payload() {
            let mut fixture = Fixture::tie();
            for profile in [
                "canonical-chroma-8192-v1-candidate",
                crate::PRESENTATION_ANALYSIS_PROFILE,
            ] {
                fixture.package.manifest.analysis_version =
                    format!("{}+{profile}", native_beat::ANALYSIS_VERSION);
                fixture.package.analysis.presentation = None;
                assert!(
                    fixture
                        .read()
                        .unwrap_err()
                        .contains("Unsupported native analysis profile")
                );
                fixture.package.analysis.presentation =
                    Some(cocobeat_schema::content::MusicPresentation {
                        capability: cocobeat_schema::AnalysisCapability {
                            state: cocobeat_schema::AnalysisState::Candidate,
                            source: cocobeat_schema::AnalysisSource::Algorithm,
                            confidence: None,
                        },
                        windows: vec![cocobeat_schema::content::PresentationWindow {
                            start: SongTime::ZERO,
                            end: SongTime::from_frames(4800),
                            chroma: [0.0; 12],
                            chord: None,
                            key: None,
                            tonal_confidence: 0.0,
                            onset_density: 0.0,
                            brightness: 0.0,
                            energy: 0.0,
                        }],
                    });
                fixture.read().unwrap();
                fixture
                    .package
                    .manifest
                    .analysis_version
                    .push_str("-unknown");
                assert!(
                    fixture
                        .read()
                        .unwrap_err()
                        .contains("Unsupported native analysis profile")
                );
            }
        }

        struct Fixture {
            root: PathBuf,
            package: ValidatedPackage,
        }

        impl Fixture {
            fn new(logits: [Vec<f32>; 2]) -> Self {
                let root = std::env::temp_dir().join(format!(
                    "cocobeat-native-evidence-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                fs::create_dir(&root).unwrap();
                let evidence = root.join("evidence");
                fs::create_dir(&evidence).unwrap();
                let source = root.join("source.ogg");
                fs::write(&source, STEREO).unwrap();
                let beats = native_beat::minimal(&logits, 4_800, &evidence).unwrap();
                fs::write(evidence.join(FILES[0]), serde_json::to_vec_pretty(&json!({
                "N":4800,"M":2205,"F":6,"shape":[1,6,128],"channel":1,
                "profile":native_beat::ANALYSIS_VERSION,"old_frontend_numeric":"FAIL_PRESERVED_19_OF_28",
                "old_music_quality":"FAIL_PRESERVED","confidence":null,"production_admission":false
            })).unwrap()).unwrap();
                fs::write(evidence.join(FILES[1]), vec![0u8; 6 * 128 * 4]).unwrap();
                for (index, values) in logits.iter().enumerate() {
                    fs::write(
                        evidence.join(FILES[2 + index]),
                        values
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                }
                let audio_hash = *blake3::hash(STEREO).as_bytes();
                let summary = json!({
                    "profile":native_beat::ANALYSIS_VERSION,
                    "model_blake3":blake3::Hash::from_bytes(native_beat::MODEL_BLAKE3).to_hex().to_string(),
                    "audio_blake3":blake3::Hash::from_bytes(audio_hash).to_hex().to_string(),
                    "files":FILES.map(|name| {
                        let bytes=fs::read(evidence.join(name)).unwrap();
                        json!({"name":name,"bytes":bytes.len(),"blake3":blake3::hash(&bytes).to_hex().to_string()})
                    }),
                    "confidence":null,"production_admission":false
                });
                let summary = serde_json::to_vec_pretty(&summary).unwrap();
                fs::write(evidence.join("evidence-summary.json"), &summary).unwrap();
                let diagnostics = Self::diagnostics(&summary);
                let package =
                    build_package(&source, 4_800, root.join("package"), |staged, prepared| {
                        let candidate = AnalysisCapability {
                            state: AnalysisState::Candidate,
                            source: AnalysisSource::Algorithm,
                            confidence: None,
                        };
                        let unsupported = AnalysisCapability {
                            state: AnalysisState::Unsupported,
                            ..candidate
                        };
                        let capabilities = AnalysisCapabilities {
                            beat: candidate,
                            downbeat: candidate,
                            tempo: unsupported,
                            onset: unsupported,
                            repetition: unsupported,
                            ..AnalysisCapabilities::authored()
                        };
                        let mut sum = [0.0f64; 2];
                        let mut peak = [0.0f32; 2];
                        decode_canonical(staged, 4_800, |samples| {
                            for sample in samples {
                                for channel in 0..2 {
                                    sum[channel] += f64::from(sample[channel]).powi(2);
                                    peak[channel] = peak[channel].max(sample[channel].abs());
                                }
                            }
                            Ok(())
                        })?;
                        Ok(PackageBuildInput {
                            song_id: "controlled-native-evidence".into(),
                            importer_version: "fixture".into(),
                            analysis_version: native_beat::ANALYSIS_VERSION.into(),
                            chart_version: "hand-authored-v1".into(),
                            analysis: MusicAnalysis {
                                presentation: None,
                                schema_version: ANALYSIS_SCHEMA_VERSION,
                                audio_hash: prepared.asset.blake3,
                                capabilities: Some(capabilities),
                                tempo_regions: vec![],
                                repetitions: vec![],
                                beats,
                                onsets: vec![],
                                sections: vec![],
                                energy: vec![EnergySample {
                                    start: SongTime::ZERO,
                                    frames: 4_800,
                                    rms: sum.map(|v| (v / 4_800.0).sqrt() as f32),
                                    peak,
                                }],
                                diagnostics,
                            },
                            chart: CompiledChart {
                                schema_version: CONTENT_SCHEMA_VERSION,
                                audio_hash: prepared.asset.blake3,
                                ruleset_id: "manual-fixture-v1".into(),
                                anchors: vec![],
                                sections: vec![],
                            },
                        })
                    })
                    .unwrap();
                Self { root, package }
            }

            fn tie() -> Self {
                Self::new([
                    vec![1.0, 1.0, 1.0, 1.0, -1.0, -1.0],
                    vec![-1.0, 2.0, 2.0, -1.0, -1.0, -1.0],
                ])
            }

            fn evidence(&self) -> PathBuf {
                self.root.join("evidence")
            }

            fn diagnostics(summary: &[u8]) -> String {
                format!(
                    "controlled native evidence; experimental={}; model_blake3={}; evidence_summary_blake3={}; channel=1; N=4800; M=2205; F=6; confidence=None",
                    native_beat::ANALYSIS_VERSION,
                    blake3::Hash::from_bytes(native_beat::MODEL_BLAKE3),
                    blake3::hash(summary)
                )
            }

            fn read(&self) -> Result<NativeBeatEvidence, String> {
                read_native_beat_evidence(&self.package, &self.evidence())
            }

            // Rebind a controlled fixture to reach semantic guards after exact-byte hash validation
            fn rebind(&mut self, name: &str, bytes: &[u8]) {
                let evidence = self.evidence();
                fs::write(evidence.join(name), bytes).unwrap();
                let mut summary: Value = serde_json::from_slice(
                    &fs::read(evidence.join("evidence-summary.json")).unwrap(),
                )
                .unwrap();
                let entry = summary["files"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|v| v["name"] == name)
                    .unwrap();
                entry["bytes"] = json!(bytes.len());
                entry["blake3"] = json!(blake3::hash(bytes).to_hex().to_string());
                self.rebind_summary(summary);
            }

            fn rebind_summary(&mut self, value: Value) {
                let bytes = serde_json::to_vec_pretty(&value).unwrap();
                fs::write(self.evidence().join("evidence-summary.json"), &bytes).unwrap();
                self.package.analysis.diagnostics = Self::diagnostics(&bytes);
            }

            fn json(&self, name: &str) -> Value {
                serde_json::from_slice(&fs::read(self.evidence().join(name)).unwrap()).unwrap()
            }
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.root);
            }
        }

        // Beat logits remain a controlled fixture; onset, canonical snapshot and summary use production paths
        fn enrich_fixture(fixture: &Fixture) -> (ValidatedPackage, PathBuf) {
            let evidence = fixture.root.join("auto-evidence");
            fs::create_dir(&evidence).unwrap();
            for name in FILES {
                fs::copy(fixture.evidence().join(name), evidence.join(name)).unwrap();
            }
            let package = build_package(
                fixture.root.join("source.ogg"),
                4_800,
                fixture.root.join("auto-package"),
                |staged, prepared| {
                    let mut analysis = fixture.package.analysis.clone();
                    let cancel = crate::NativeBeatCancellation::default();
                    native_beat::enrich_auto_analysis(
                        staged,
                        prepared,
                        1,
                        &evidence,
                        &mut analysis,
                        &cancel,
                    )?;
                    native_beat::finish_analysis_evidence(
                        &evidence,
                        &mut analysis,
                        4_800,
                        1,
                        6,
                        true,
                        &cancel,
                    )?;
                    Ok(PackageBuildInput {
                        song_id: fixture.package.manifest.song_id.clone(),
                        importer_version: "controlled-logits-real-onset-producer".into(),
                        analysis_version: native_beat::AUTO_ANALYSIS_VERSION.into(),
                        chart_version: fixture.package.manifest.chart_version.clone(),
                        analysis,
                        chart: fixture.package.chart.clone(),
                    })
                },
            )
            .unwrap();
            (package, evidence)
        }

        #[test]
        fn real_auto_producer_roundtrips_with_legacy_beats_and_authored_chart() {
            let mut fixture = Fixture::tie();
            let legacy = fixture.read().unwrap();
            fixture.package = export_anchors(
                fixture.root.join("package"),
                fixture.package.manifest.package_hash,
                &[Anchor {
                    id: 7,
                    song_time: SongTime::from_frames(1024),
                }],
                fixture.root.join("authored-anchors"),
            )
            .unwrap();
            let (package, evidence) = enrich_fixture(&fixture);
            let current = read_native_beat_evidence(&package, &evidence).unwrap();
            assert_eq!(current.metadata.profile, native_beat::AUTO_ANALYSIS_VERSION);
            assert_eq!(current.records, legacy.records);
            assert_eq!(package.chart, fixture.package.chart);
            assert_eq!(package.analysis.energy, fixture.package.analysis.energy);
            assert_eq!(package.analysis.tempo_regions.len(), 1);
            let interval = package.analysis.tempo_regions[0];
            assert_eq!(
                (interval.start.frames(), interval.end.frames(), interval.bpm),
                (480, 2400, 1500.0)
            );
            assert_eq!(fixture.read().unwrap().records, legacy.records);
            let summary: Value =
                serde_json::from_slice(&fs::read(evidence.join("evidence-summary.json")).unwrap())
                    .unwrap();
            assert_eq!(summary["files"].as_array().unwrap().len(), 7);
            let proposal = crate::compile_anchor_proposal(
                &package.analysis,
                4_800,
                crate::AnchorPolicy {
                    min_confidence: 0.5,
                    min_gap_frames: 100,
                },
            )
            .unwrap();
            assert!(proposal.anchors.is_empty());
            assert!(
                proposal
                    .evidence
                    .iter()
                    .all(|row| row.decision == crate::AnchorDecision::UnknownConfidence)
            );
        }

        #[test]
        fn auto_evidence_mutations_are_checked_against_the_real_producer_summary() {
            let fixture = Fixture::tie();
            let (package, evidence) = enrich_fixture(&fixture);
            let original = fs::read(evidence.join(native_beat::AUTO_RESOURCE)).unwrap();
            let mut changed = original.clone();
            changed.push(b' ');
            fs::write(evidence.join(native_beat::AUTO_RESOURCE), &changed).unwrap();
            assert!(read_native_beat_evidence(&package, &evidence).is_err());
            fs::write(evidence.join(native_beat::AUTO_RESOURCE), &original).unwrap();
            let mut changed = package.clone();
            changed.analysis.tempo_regions[0].bpm += 1.0;
            assert!(
                read_native_beat_evidence(&changed, &evidence)
                    .unwrap_err()
                    .contains("adjacent beat")
            );
            let mut changed = package.clone();
            changed.analysis.onsets.clear();
            changed.analysis.onsets.push(cocobeat_schema::OnsetFeature {
                time: SongTime::from_frames(512),
                strength: 0.25,
                confidence: None,
            });
            if changed.analysis.onsets == package.analysis.onsets {
                changed.analysis.onsets[0].strength = 0.5;
            }
            assert!(
                read_native_beat_evidence(&changed, &evidence)
                    .unwrap_err()
                    .contains("onset records")
            );
            // Rebind the actual produced resource only to reach its semantic checks
            let mut value: Value = serde_json::from_slice(&original).unwrap();
            value["tempo_profile"] = json!("default-120-bpm");
            let changed_bytes = serde_json::to_vec(&value).unwrap();
            fs::write(evidence.join(native_beat::AUTO_RESOURCE), &changed_bytes).unwrap();
            let mut summary: Value =
                serde_json::from_slice(&fs::read(evidence.join("evidence-summary.json")).unwrap())
                    .unwrap();
            let resource = summary["files"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|r| r["name"] == native_beat::AUTO_RESOURCE)
                .unwrap();
            resource["bytes"] = json!(changed_bytes.len());
            resource["blake3"] = json!(blake3::hash(&changed_bytes).to_hex().to_string());
            let summary_bytes = serde_json::to_vec(&summary).unwrap();
            let old_summary =
                blake3::hash(&fs::read(evidence.join("evidence-summary.json")).unwrap());
            fs::write(evidence.join("evidence-summary.json"), &summary_bytes).unwrap();
            let mut changed = package.clone();
            changed.analysis.diagnostics = changed.analysis.diagnostics.replace(
                &format!("evidence_summary_blake3={old_summary}"),
                &format!("evidence_summary_blake3={}", blake3::hash(&summary_bytes)),
            );
            assert!(
                read_native_beat_evidence(&changed, &evidence)
                    .unwrap_err()
                    .contains("metadata mismatch")
            );
        }

        #[test]
        fn author_source_note_keys_are_outside_final_producer_annotation() {
            let mut fixture = Fixture::tie();
            let expected = fixture.read().unwrap();
            let producer = fixture.package.analysis.diagnostics.clone();
            fixture.package.analysis.diagnostics = format!(
                "Energy measured; source note: channel comparison; channel=1; N=4800; experimental={}; evidence_summary_blake3={}; preserved author note; {producer}",
                native_beat::ANALYSIS_VERSION,
                "00".repeat(32)
            );
            assert_eq!(fixture.read().unwrap(), expected);
            let original = fixture.package.analysis.diagnostics.clone();
            for duplicate in ["; channel=1", "; N=4800", "; evidence_summary_blake3=00"] {
                fixture.package.analysis.diagnostics = original.clone() + duplicate;
                assert!(
                    fixture
                        .read()
                        .unwrap_err()
                        .contains("Duplicate native diagnostic")
                );
            }
            fixture.package.analysis.diagnostics = "source note; channel=1; N=4800".into();
            assert!(
                fixture
                    .read()
                    .unwrap_err()
                    .contains("Missing final native producer annotation")
            );
        }

        #[test]
        fn original_members_running_mean_tie_left_and_max_scores() {
            let fixture = Fixture::tie();
            let evidence = fixture.read().unwrap();
            assert_eq!(evidence.metadata.canonical_frames, 4_800);
            assert_eq!(
                (
                    evidence.metadata.resampled_frames,
                    evidence.metadata.spectrogram_frames,
                    evidence.metadata.channel
                ),
                (2205, 6, 1)
            );
            assert_eq!(
                evidence.records.iter().map(|v| v.frame).collect::<Vec<_>>(),
                [480, 1440, 2400]
            );
            let raw = &evidence.records[1];
            assert_eq!(raw.kind, NativeBeatKind::RawDownbeat);
            assert_eq!(raw.original_q, 1.5);
            assert_eq!(
                raw.members,
                [
                    NativePeakMember {
                        q: 1,
                        raw_logit: 2.0
                    },
                    NativePeakMember {
                        q: 2,
                        raw_logit: 2.0
                    }
                ]
            );
            assert_eq!(
                raw.alignment,
                Some(NativeDownbeatAlignment {
                    beat_group_index: 0,
                    beat_q: 0.5,
                    beat_frame: 480
                })
            );
            assert_eq!(
                raw.package_downbeat_score,
                fixture.package.analysis.beats[0].downbeat_probability
            );
            assert_eq!(
                serde_json::to_value(raw.kind).unwrap(),
                json!("raw_downbeat")
            );

            let running = Fixture::new([vec![1.0, 1.0, 1.0, 1.0, 1.0, -1.0], vec![-1.0; 6]]);
            assert_eq!(
                running
                    .read()
                    .unwrap()
                    .records
                    .iter()
                    .map(|v| v.original_q)
                    .collect::<Vec<_>>(),
                [0.5, 2.5, 4.0]
            );
            let max = Fixture::new([
                vec![-1.0, -1.0, 1.0, -1.0, -1.0, -1.0],
                vec![1.0, -1.0, -1.0, -1.0, 2.0, -1.0],
            ]);
            let records = max.read().unwrap().records;
            assert_eq!(
                records.iter().map(|v| v.frame).collect::<Vec<_>>(),
                [0, 1920, 3840]
            );
            assert!(records[0].uncalibrated_score < records[2].uncalibrated_score);
            assert_eq!(
                records[0].package_downbeat_score,
                Some(records[2].uncalibrated_score)
            );
            assert_eq!(
                records[1].package_downbeat_score,
                Some(records[2].uncalibrated_score)
            );
        }

        #[test]
        fn same_frame_stays_two_records_and_empty_candidates_remain_valid() {
            let same = Fixture::new([
                vec![1.0, -1.0, -1.0, -1.0, -1.0, -1.0],
                vec![2.0, -1.0, -1.0, -1.0, -1.0, -1.0],
            ]);
            let records = same.read().unwrap().records;
            assert_eq!(records.len(), 2);
            assert_eq!(
                (records[0].kind, records[1].kind),
                (NativeBeatKind::Beat, NativeBeatKind::RawDownbeat)
            );
            assert_eq!((records[0].frame, records[1].frame), (0, 0));
            let empty = Fixture::new([vec![-1.0; 6], vec![-1.0; 6]]);
            assert!(empty.read().unwrap().records.is_empty());
            let tiny = Fixture::new([
                vec![f32::from_bits(1), -1.0, -1.0, -1.0, -1.0, -1.0],
                vec![-1.0; 6],
            ]);
            assert_eq!(tiny.read().unwrap().records[0].uncalibrated_score, 0.5);
        }

        #[test]
        fn real_anchor_export_preserves_evidence_binding_despite_new_cid() {
            let fixture = Fixture::tie();
            let source = fixture.root.join("package");
            let destination = fixture.root.join("edited");
            let before = fixture.read().unwrap();
            let edited = export_anchors(
                &source,
                fixture.package.manifest.package_hash,
                &[Anchor {
                    id: 9,
                    song_time: SongTime::from_frames(4799),
                }],
                &destination,
            )
            .unwrap();
            assert_ne!(
                edited.manifest.package_hash,
                fixture.package.manifest.package_hash
            );
            assert_ne!(edited.manifest.chart, fixture.package.manifest.chart);
            assert_eq!(edited.manifest.audio, fixture.package.manifest.audio);
            assert_eq!(edited.manifest.analysis, fixture.package.manifest.analysis);
            for name in &PACKAGE_OBJECT_NAMES[..2] {
                assert_eq!(
                    fs::read(source.join(name)).unwrap(),
                    fs::read(destination.join(name)).unwrap()
                );
            }
            assert_eq!(
                read_native_beat_evidence(&edited, &fixture.evidence()).unwrap(),
                before
            );
        }

        #[test]
        fn exact_summary_and_every_resource_are_bound_before_decode() {
            let mut fixture = Fixture::tie();
            let summary = fs::read(fixture.evidence().join("evidence-summary.json")).unwrap();
            let mut replaced = summary.clone();
            replaced.push(b' ');
            fs::write(fixture.evidence().join("evidence-summary.json"), &replaced).unwrap();
            assert!(
                fixture
                    .read()
                    .unwrap_err()
                    .contains("summary does not match")
            );
            fs::write(fixture.evidence().join("evidence-summary.json"), &summary).unwrap();
            for name in FILES {
                let path = fixture.evidence().join(name);
                let original = fs::read(&path).unwrap();
                let mut changed = original.clone();
                changed[0] ^= 1;
                fs::write(&path, &changed).unwrap();
                assert!(
                    fixture.read().unwrap_err().contains("identity mismatch"),
                    "{name}"
                );
                fs::write(&path, &original).unwrap();
            }
            fixture.package.analysis.diagnostics.push_str(&format!(
                "; evidence_summary_blake3={}",
                blake3::hash(&summary)
            ));
            assert!(
                fixture
                    .read()
                    .unwrap_err()
                    .contains("Duplicate native diagnostic")
            );
        }

        #[test]
        fn rebound_shape_profile_capability_and_nullable_fail_closed() {
            let mut fixture = Fixture::tie();
            let original = fixture.json(FILES[0]);
            for (key, value) in [
                ("N", json!(4801)),
                ("M", json!(2206)),
                ("F", json!(7)),
                ("shape", json!([1, 6, 127])),
                ("channel", json!(2)),
                ("profile", json!("other")),
                ("confidence", json!(0.9)),
                ("production_admission", json!(true)),
            ] {
                let mut shape = original.clone();
                shape[key] = value;
                fixture.rebind(FILES[0], &serde_json::to_vec(&shape).unwrap());
                assert!(fixture.read().is_err(), "{key}");
            }
            let mut missing = original.clone();
            missing.as_object_mut().unwrap().remove("confidence");
            fixture.rebind(FILES[0], &serde_json::to_vec(&missing).unwrap());
            assert!(fixture.read().is_err());
            fixture.rebind(FILES[0], &serde_json::to_vec(&original).unwrap());
            let summary = fixture.json("evidence-summary.json");
            for (key, value) in [
                ("profile", json!("other")),
                ("model_blake3", json!("00".repeat(32))),
                ("audio_blake3", json!("00".repeat(32))),
                ("confidence", json!(0.9)),
                ("production_admission", json!(true)),
            ] {
                let mut changed = summary.clone();
                changed[key] = value;
                fixture.rebind_summary(changed);
                assert!(fixture.read().is_err(), "{key}");
            }
            fixture.rebind_summary(summary);
            let analysis = fixture.package.analysis.clone();
            for changed in [
                AnalysisCapability {
                    state: AnalysisState::Validated,
                    source: AnalysisSource::Algorithm,
                    confidence: None,
                },
                AnalysisCapability {
                    state: AnalysisState::Candidate,
                    source: AnalysisSource::Authored,
                    confidence: None,
                },
                AnalysisCapability {
                    state: AnalysisState::Candidate,
                    source: AnalysisSource::Algorithm,
                    confidence: Some(0.9),
                },
            ] {
                fixture.package.analysis.capabilities.as_mut().unwrap().beat = changed;
                assert!(fixture.read().is_err());
            }
            fixture.package.analysis = analysis;
            fixture.package.analysis.beats[0].confidence = Some(0.9);
            assert!(fixture.read().is_err());
        }

        #[test]
        fn rebound_raw_members_coordinates_alignment_and_scores_fail_closed() {
            let mut fixture = Fixture::tie();
            let groups = fixture.json(FILES[4]);
            for changed in [json!([99]), json!([1, 2, 3])] {
                let mut value = groups.clone();
                value["downbeat"][0]["members"] = changed;
                fixture.rebind(FILES[4], &serde_json::to_vec(&value).unwrap());
                assert!(fixture.read().is_err());
            }
            for q in [-1.0, 5.0, 1.0] {
                let mut value = groups.clone();
                value["downbeat"][0]["q"] = json!(q);
                fixture.rebind(FILES[4], &serde_json::to_vec(&value).unwrap());
                assert!(fixture.read().is_err());
            }
            let mut value = groups.clone();
            value["downbeat"][0]["score"] = json!(0.49);
            fixture.rebind(FILES[4], &serde_json::to_vec(&value).unwrap());
            assert!(fixture.read().is_err());
            fixture.rebind(FILES[4], &serde_json::to_vec(&groups).unwrap());
            let alignment = fixture.json(FILES[5]);
            for (key, value) in [
                ("original_downbeat_q", json!(0.5)),
                ("nearest_beat_index", json!(1)),
                ("aligned_beat_q", json!(2.5)),
                ("canonical_frame", json!(2400)),
            ] {
                let mut changed = alignment.clone();
                changed[0][key] = value;
                fixture.rebind(FILES[5], &serde_json::to_vec(&changed).unwrap());
                assert!(fixture.read().is_err(), "{key}");
            }
            fixture.rebind(FILES[5], &serde_json::to_vec(&alignment).unwrap());
            let beats = fixture.package.analysis.beats.clone();
            fixture.package.analysis.beats[0].strength = 0.7;
            assert!(fixture.read().is_err());
            fixture.package.analysis.beats = beats.clone();
            fixture.package.analysis.beats[0].downbeat_probability = None;
            assert!(fixture.read().is_err());
            fixture.package.analysis.beats = beats;
            let original = fs::read(fixture.evidence().join(FILES[2])).unwrap();
            let mut nonfinite = original.clone();
            nonfinite[..4].copy_from_slice(&f32::NAN.to_le_bytes());
            fixture.rebind(FILES[2], &nonfinite);
            assert!(fixture.read().unwrap_err().contains("Nonfinite"));
            // Producer-accurate out-of-range tail peak is rejected before any schema clipping
            let mut tail = [-1.0f32; 6];
            tail[5] = 1.0;
            fixture.rebind(
                FILES[2],
                &tail
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            );
            let mut raw = groups;
            raw["beat"] = json!([{"q":5.0,"score":0.7310586,"members":[5]}]);
            fixture.rebind(FILES[4], &serde_json::to_vec(&raw).unwrap());
            assert!(fixture.read().unwrap_err().contains("outside"));
        }

        #[test]
        fn fixed_resources_and_regular_file_lengths_are_required() {
            let mut fixture = Fixture::tie();
            let summary = fixture.json("evidence-summary.json");
            for name in ["../outside", FILES[1]] {
                let mut changed = summary.clone();
                changed["files"][0]["name"] = json!(name);
                fixture.rebind_summary(changed);
                assert!(fixture.read().is_err());
            }
            let mut changed = summary.clone();
            changed["files"].as_array_mut().unwrap().pop();
            fixture.rebind_summary(changed);
            assert!(fixture.read().is_err());
            fixture.rebind_summary(summary.clone());
            let mut changed = summary;
            changed["files"][1]["bytes"] = json!(6 * 128 * 4 + 1);
            fixture.rebind_summary(changed);
            assert!(fixture.read().unwrap_err().contains("spect length"));
            let bytes = fixture.json("evidence-summary.json");
            let mut changed = bytes;
            changed["files"][1]["bytes"] = json!(6 * 128 * 4);
            fixture.rebind_summary(changed);
            let path = fixture.evidence().join(FILES[0]);
            let original = fs::read(&path).unwrap();
            fs::remove_file(&path).unwrap();
            fs::create_dir(&path).unwrap();
            assert!(fixture.read().unwrap_err().contains("regular file"));
            fs::remove_dir(&path).unwrap();
            fs::write(&path, vec![b' '; SHAPE_LIMIT as usize + 1]).unwrap();
            assert!(fixture.read().unwrap_err().contains("bounded regular file"));
            fs::write(&path, &original).unwrap();
            #[cfg(unix)]
            {
                let actual = fixture.root.join("shape-outside.json");
                fs::rename(&path, &actual).unwrap();
                std::os::unix::fs::symlink(&actual, &path).unwrap();
                assert!(fixture.read().unwrap_err().contains("regular file"));
                let alias = fixture.root.join("evidence-alias");
                std::os::unix::fs::symlink(fixture.evidence(), &alias).unwrap();
                assert!(
                    read_native_beat_evidence(&fixture.package, &alias)
                        .unwrap_err()
                        .contains("regular directory")
                );
            }
        }
    }
}
