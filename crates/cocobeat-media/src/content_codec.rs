//! Bounded, versioned Postcard records for the three song content objects

use cocobeat_schema::{
    Anchor, AssetRef, SongTime,
    content::{
        ANALYSIS_SCHEMA_VERSION, AnalysisCapabilities, AnalysisCapability, AnalysisSource,
        AnalysisState, BeatFeature, CONTENT_SCHEMA_VERSION, CompiledChart, EnergySample,
        MAX_CONTENT_DIAGNOSTICS_BYTES, MAX_CONTENT_ITEMS, MAX_CONTENT_TEXT_BYTES, MusicAnalysis,
        OnsetFeature, RepetitionFeature, SectionCue, SectionFeature, SongPackage, TempoBeatUnit,
        TempoRegion,
    },
};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, DeserializeOwned, SeqAccess, Visitor},
    ser::SerializeSeq,
};
use std::{fmt, marker::PhantomData};

// Limits include the complete 20-byte header
pub(crate) const MAX_PACKAGE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_ANALYSIS_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_CHART_BYTES: usize = 4 * 1024 * 1024;
const HEADER_BYTES: usize = 20;
const ANALYSIS_MAGIC: &[u8; 8] = b"CCANLYS\0";
const CHART_MAGIC: &[u8; 8] = b"CCCHART\0";
const PACKAGE_MAGIC: &[u8; 8] = b"CCPACKG\0";

fn encode<T: Serialize>(magic: &[u8; 8], value: &T, limit: usize) -> Result<Vec<u8>, String> {
    encode_version(magic, value, limit, CONTENT_SCHEMA_VERSION)
}

fn encode_version<T: Serialize>(
    magic: &[u8; 8],
    value: &T,
    limit: usize,
    version: u32,
) -> Result<Vec<u8>, String> {
    let payload_len =
        postcard::serialize_with_flavor(value, postcard::ser_flavors::Size::default())
            .map_err(|error| format!("Cannot measure content record: {error}"))?;
    let total = HEADER_BYTES
        .checked_add(payload_len)
        .filter(|&total| total <= limit)
        .ok_or("Content record exceeds its byte limit")?;
    let mut bytes = vec![0; total];
    bytes[..8].copy_from_slice(magic);
    bytes[8..12].copy_from_slice(&version.to_le_bytes());
    bytes[12..20].copy_from_slice(&(payload_len as u64).to_le_bytes());
    postcard::to_slice(value, &mut bytes[HEADER_BYTES..])
        .map_err(|error| format!("Cannot encode content record: {error}"))?;
    Ok(bytes)
}

fn decode<T: DeserializeOwned>(magic: &[u8; 8], bytes: &[u8], limit: usize) -> Result<T, String> {
    decode_version(magic, bytes, limit, CONTENT_SCHEMA_VERSION)
}

fn decode_version<T: DeserializeOwned>(
    magic: &[u8; 8],
    bytes: &[u8],
    limit: usize,
    version: u32,
) -> Result<T, String> {
    if !(HEADER_BYTES..=limit).contains(&bytes.len()) {
        return Err("Content record length is outside its byte limit".into());
    }
    if &bytes[..8] != magic {
        return Err("Content record magic does not match its object type".into());
    }
    if u32::from_le_bytes(bytes[8..12].try_into().unwrap()) != version {
        return Err("Unsupported content record schema version".into());
    }
    let declared = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
    if declared != (bytes.len() - HEADER_BYTES) as u64 {
        return Err("Content payload length does not match its header".into());
    }
    let (value, remaining) = postcard::take_from_bytes(&bytes[HEADER_BYTES..])
        .map_err(|error| format!("Cannot decode content record: {error}"))?;
    if !remaining.is_empty() {
        return Err("Content record has trailing payload bytes".into());
    }
    Ok(value)
}

fn bounded_items<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct ItemsVisitor<T>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for ItemsVisitor<T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a bounded content sequence")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Vec<T>, A::Error> {
            if sequence
                .size_hint()
                .is_some_and(|count| count > MAX_CONTENT_ITEMS)
            {
                return Err(de::Error::custom("Content sequence exceeds its item limit"));
            }
            // A malformed length may have no size_hint; never reserve from it
            let mut values = Vec::new();
            while let Some(value) = sequence.next_element()? {
                if values.len() == MAX_CONTENT_ITEMS {
                    return Err(de::Error::custom("Content sequence exceeds its item limit"));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(ItemsVisitor(PhantomData))
}

struct Text<const LIMIT: usize>(String);

impl<'de, const LIMIT: usize> Deserialize<'de> for Text<LIMIT> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <&str>::deserialize(deserializer)?;
        if text.len() > LIMIT {
            return Err(de::Error::custom("Content text exceeds its byte limit"));
        }
        Ok(Self(text.to_owned()))
    }
}

type Label = Text<MAX_CONTENT_TEXT_BYTES>;
type BeatDto = (i64, f32, Option<f32>, Option<f32>);
type OnsetDto = (i64, f32, Option<f32>);
type SectionDto = (i64, i64, Option<f32>, Label);
type EnergyDto = (i64, u32, [f32; 2], [f32; 2]);
type AssetDto = (Label, u64, [u8; 32]);

#[derive(Deserialize)]
struct AnalysisDto {
    schema_version: u32,
    audio_hash: [u8; 32],
    #[serde(deserialize_with = "bounded_items")]
    beats: Vec<BeatDto>,
    #[serde(deserialize_with = "bounded_items")]
    onsets: Vec<OnsetDto>,
    #[serde(deserialize_with = "bounded_items")]
    sections: Vec<SectionDto>,
    #[serde(deserialize_with = "bounded_items")]
    energy: Vec<EnergyDto>,
    diagnostics: Text<MAX_CONTENT_DIAGNOSTICS_BYTES>,
}

type CapabilityDto = (u8, u8, Option<f32>);
type CapabilitiesDto = (
    CapabilityDto,
    CapabilityDto,
    CapabilityDto,
    CapabilityDto,
    CapabilityDto,
    CapabilityDto,
    CapabilityDto,
);
type TempoDto = (i64, i64, f32, Option<u8>, Option<f32>);
type RepetitionDto = (i64, i64, i64, i64, Option<f32>);

#[derive(Deserialize)]
struct AnalysisV2Dto {
    // The legacy field layout precedes the v2 fields
    legacy: AnalysisDto,
    capabilities: CapabilitiesDto,
    #[serde(deserialize_with = "bounded_items")]
    tempo_regions: Vec<TempoDto>,
    #[serde(deserialize_with = "bounded_items")]
    repetitions: Vec<RepetitionDto>,
}

#[derive(Deserialize)]
struct ChartDto {
    schema_version: u32,
    audio_hash: [u8; 32],
    ruleset_id: Label,
    #[serde(deserialize_with = "bounded_items")]
    anchors: Vec<(u64, i64)>,
    #[serde(deserialize_with = "bounded_items")]
    sections: Vec<(u64, i64, Label)>,
}

#[derive(Deserialize)]
struct PackageDto {
    schema_version: u32,
    song_id: Label,
    audio: AssetDto,
    analysis: AssetDto,
    chart: AssetDto,
    canonical_sample_rate: u32,
    channels: u8,
    canonical_frames: u64,
    importer_version: Label,
    analysis_version: Label,
    chart_version: Label,
    package_hash: [u8; 32],
}

// Serialize schema slices without allocating a second collection or copying text
struct Mapped<'a, T, F>(&'a [T], F);

impl<'a, T, F, U> Serialize for Mapped<'a, T, F>
where
    F: Fn(&'a T) -> U,
    U: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for item in self.0 {
            sequence.serialize_element(&(self.1)(item))?;
        }
        sequence.end()
    }
}

fn section_wire(section: &SectionFeature) -> (i64, i64, Option<f32>, &str) {
    (
        section.start.frames(),
        section.end.frames(),
        section.confidence,
        &section.label,
    )
}

fn cue_wire(section: &SectionCue) -> (u64, i64, &str) {
    (section.id, section.time.frames(), &section.label)
}

fn analysis_wire(value: &MusicAnalysis) -> impl Serialize + '_ {
    (
        value.schema_version,
        value.audio_hash,
        Mapped(&value.beats, |beat: &BeatFeature| {
            (
                beat.time.frames(),
                beat.strength,
                beat.downbeat_probability,
                beat.confidence,
            )
        }),
        Mapped(&value.onsets, |onset: &OnsetFeature| {
            (onset.time.frames(), onset.strength, onset.confidence)
        }),
        Mapped(&value.sections, section_wire),
        Mapped(&value.energy, |energy: &EnergySample| {
            (
                energy.start.frames(),
                energy.frames,
                energy.rms,
                energy.peak,
            )
        }),
        value.diagnostics.as_str(),
    )
}

fn capability_wire(value: AnalysisCapability) -> CapabilityDto {
    let state = match value.state {
        AnalysisState::NotRun => 0,
        AnalysisState::Unsupported => 1,
        AnalysisState::Candidate => 2,
        AnalysisState::Validated => 3,
    };
    let source = match value.source {
        AnalysisSource::Algorithm => 0,
        AnalysisSource::Authored => 1,
        AnalysisSource::Measured => 2,
    };
    (state, source, value.confidence)
}

fn capability_from_wire(
    (state, source, confidence): CapabilityDto,
) -> Result<AnalysisCapability, String> {
    Ok(AnalysisCapability {
        state: match state {
            0 => AnalysisState::NotRun,
            1 => AnalysisState::Unsupported,
            2 => AnalysisState::Candidate,
            3 => AnalysisState::Validated,
            _ => return Err("Unknown analysis capability state".into()),
        },
        source: match source {
            0 => AnalysisSource::Algorithm,
            1 => AnalysisSource::Authored,
            2 => AnalysisSource::Measured,
            _ => return Err("Unknown analysis capability source".into()),
        },
        confidence,
    })
}

fn tempo_unit_wire(unit: Option<TempoBeatUnit>) -> Option<u8> {
    unit.map(|unit| match unit {
        TempoBeatUnit::Quarter => 0,
        TempoBeatUnit::Eighth => 1,
        TempoBeatUnit::DottedQuarter => 2,
    })
}

fn tempo_unit_from_wire(unit: Option<u8>) -> Result<Option<TempoBeatUnit>, String> {
    match unit {
        None => Ok(None),
        Some(0) => Ok(Some(TempoBeatUnit::Quarter)),
        Some(1) => Ok(Some(TempoBeatUnit::Eighth)),
        Some(2) => Ok(Some(TempoBeatUnit::DottedQuarter)),
        _ => Err("Unknown tempo beat unit".into()),
    }
}

pub(crate) fn encode_analysis(value: &MusicAnalysis, frames: u64) -> Result<Vec<u8>, String> {
    value.validate(frames)?;
    if let Some(caps) = value.capabilities {
        encode_version(
            ANALYSIS_MAGIC,
            &(
                analysis_wire(value),
                (
                    capability_wire(caps.tempo),
                    capability_wire(caps.onset),
                    capability_wire(caps.beat),
                    capability_wire(caps.downbeat),
                    capability_wire(caps.sections),
                    capability_wire(caps.repetition),
                    capability_wire(caps.energy),
                ),
                Mapped(&value.tempo_regions, |region: &TempoRegion| {
                    (
                        region.start.frames(),
                        region.end.frames(),
                        region.bpm,
                        tempo_unit_wire(region.beat_unit),
                        region.confidence,
                    )
                }),
                Mapped(&value.repetitions, |repeat: &RepetitionFeature| {
                    (
                        repeat.source_start.frames(),
                        repeat.source_end.frames(),
                        repeat.target_start.frames(),
                        repeat.target_end.frames(),
                        repeat.confidence,
                    )
                }),
            ),
            MAX_ANALYSIS_BYTES,
            ANALYSIS_SCHEMA_VERSION,
        )
    } else {
        encode(ANALYSIS_MAGIC, &analysis_wire(value), MAX_ANALYSIS_BYTES)
    }
}

pub(crate) fn decode_analysis(bytes: &[u8], frames: u64) -> Result<MusicAnalysis, String> {
    let version = bytes.get(8..12).ok_or("Analysis header is incomplete")?;
    let version = u32::from_le_bytes(version.try_into().unwrap());
    let (dto, capabilities, tempo_regions, repetitions) = match version {
        CONTENT_SCHEMA_VERSION => (
            decode(ANALYSIS_MAGIC, bytes, MAX_ANALYSIS_BYTES)?,
            None,
            Vec::new(),
            Vec::new(),
        ),
        ANALYSIS_SCHEMA_VERSION => {
            let v2: AnalysisV2Dto =
                decode_version(ANALYSIS_MAGIC, bytes, MAX_ANALYSIS_BYTES, version)?;
            let (tempo, onset, beat, downbeat, sections, repetition, energy) = v2.capabilities;
            let capabilities = AnalysisCapabilities {
                tempo: capability_from_wire(tempo)?,
                onset: capability_from_wire(onset)?,
                beat: capability_from_wire(beat)?,
                downbeat: capability_from_wire(downbeat)?,
                sections: capability_from_wire(sections)?,
                repetition: capability_from_wire(repetition)?,
                energy: capability_from_wire(energy)?,
            };
            let tempo_regions = v2
                .tempo_regions
                .into_iter()
                .map(|(start, end, bpm, unit, confidence)| {
                    Ok(TempoRegion {
                        start: SongTime::from_frames(start),
                        end: SongTime::from_frames(end),
                        bpm,
                        beat_unit: tempo_unit_from_wire(unit)?,
                        confidence,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let repetitions = v2
                .repetitions
                .into_iter()
                .map(
                    |(source_start, source_end, target_start, target_end, confidence)| {
                        RepetitionFeature {
                            source_start: SongTime::from_frames(source_start),
                            source_end: SongTime::from_frames(source_end),
                            target_start: SongTime::from_frames(target_start),
                            target_end: SongTime::from_frames(target_end),
                            confidence,
                        }
                    },
                )
                .collect();
            (v2.legacy, Some(capabilities), tempo_regions, repetitions)
        }
        _ => return Err("Unsupported analysis record schema version".into()),
    };
    let value = MusicAnalysis {
        capabilities,
        tempo_regions,
        repetitions,
        schema_version: dto.schema_version,
        audio_hash: dto.audio_hash,
        beats: dto
            .beats
            .into_iter()
            .map(
                |(time, strength, downbeat_probability, confidence)| BeatFeature {
                    time: SongTime::from_frames(time),
                    strength,
                    downbeat_probability,
                    confidence,
                },
            )
            .collect(),
        onsets: dto
            .onsets
            .into_iter()
            .map(|(time, strength, confidence)| OnsetFeature {
                time: SongTime::from_frames(time),
                strength,
                confidence,
            })
            .collect(),
        sections: dto
            .sections
            .into_iter()
            .map(|(start, end, confidence, label)| SectionFeature {
                start: SongTime::from_frames(start),
                end: SongTime::from_frames(end),
                confidence,
                label: label.0,
            })
            .collect(),
        energy: dto
            .energy
            .into_iter()
            .map(|(start, frames, rms, peak)| EnergySample {
                start: SongTime::from_frames(start),
                frames,
                rms,
                peak,
            })
            .collect(),
        diagnostics: dto.diagnostics.0,
    };
    if value.schema_version != version {
        return Err("Analysis payload version does not match its header".into());
    }
    value.validate(frames)?;
    Ok(value)
}

fn chart_wire(value: &CompiledChart) -> impl Serialize + '_ {
    (
        value.schema_version,
        value.audio_hash,
        value.ruleset_id.as_str(),
        Mapped(&value.anchors, |anchor: &Anchor| {
            (anchor.id, anchor.song_time.frames())
        }),
        Mapped(&value.sections, cue_wire),
    )
}

pub(crate) fn encode_chart(value: &CompiledChart, frames: u64) -> Result<Vec<u8>, String> {
    value.validate(frames)?;
    encode(CHART_MAGIC, &chart_wire(value), MAX_CHART_BYTES)
}

pub(crate) fn decode_chart(bytes: &[u8], frames: u64) -> Result<CompiledChart, String> {
    let dto: ChartDto = decode(CHART_MAGIC, bytes, MAX_CHART_BYTES)?;
    let value = CompiledChart {
        schema_version: dto.schema_version,
        audio_hash: dto.audio_hash,
        ruleset_id: dto.ruleset_id.0,
        anchors: dto
            .anchors
            .into_iter()
            .map(|(id, time)| Anchor {
                id,
                song_time: SongTime::from_frames(time),
            })
            .collect(),
        sections: dto
            .sections
            .into_iter()
            .map(|(id, time, label)| SectionCue {
                id,
                time: SongTime::from_frames(time),
                label: label.0,
            })
            .collect(),
    };
    value.validate(frames)?;
    Ok(value)
}

fn asset_wire(asset: &AssetRef) -> (&str, u64, [u8; 32]) {
    (&asset.file_name, asset.byte_len, asset.blake3)
}

fn asset_value((file_name, byte_len, blake3): AssetDto) -> AssetRef {
    AssetRef {
        file_name: file_name.0,
        byte_len,
        blake3,
    }
}

fn package_wire(value: &SongPackage, package_hash: [u8; 32]) -> impl Serialize + '_ {
    (
        value.schema_version,
        value.song_id.as_str(),
        asset_wire(&value.audio),
        asset_wire(&value.analysis),
        asset_wire(&value.chart),
        value.canonical_sample_rate,
        value.channels,
        value.canonical_frames,
        value.importer_version.as_str(),
        value.analysis_version.as_str(),
        value.chart_version.as_str(),
        package_hash,
    )
}

pub(crate) fn encode_package(value: &SongPackage) -> Result<Vec<u8>, String> {
    value.validate()?;
    encode(
        PACKAGE_MAGIC,
        &package_wire(value, value.package_hash),
        MAX_PACKAGE_BYTES,
    )
}

pub(crate) fn decode_package(bytes: &[u8]) -> Result<SongPackage, String> {
    let dto: PackageDto = decode(PACKAGE_MAGIC, bytes, MAX_PACKAGE_BYTES)?;
    let value = SongPackage {
        schema_version: dto.schema_version,
        song_id: dto.song_id.0,
        audio: asset_value(dto.audio),
        analysis: asset_value(dto.analysis),
        chart: asset_value(dto.chart),
        canonical_sample_rate: dto.canonical_sample_rate,
        channels: dto.channels,
        canonical_frames: dto.canonical_frames,
        importer_version: dto.importer_version.0,
        analysis_version: dto.analysis_version.0,
        chart_version: dto.chart_version.0,
        package_hash: dto.package_hash,
    };
    value.validate()?;
    Ok(value)
}

pub(crate) fn package_hash(value: &SongPackage) -> Result<[u8; 32], String> {
    value.validate()?;
    let bytes = encode(
        PACKAGE_MAGIC,
        &package_wire(value, [0; 32]),
        MAX_PACKAGE_BYTES,
    )?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_schema::CANONICAL_SAMPLE_RATE;

    fn fixtures() -> (MusicAnalysis, CompiledChart, SongPackage) {
        let analysis = MusicAnalysis {
            capabilities: None,
            tempo_regions: Vec::new(),
            repetitions: Vec::new(),
            schema_version: CONTENT_SCHEMA_VERSION,
            audio_hash: [1; 32],
            beats: vec![BeatFeature {
                time: SongTime::ZERO,
                strength: 0.5,
                downbeat_probability: Some(0.75),
                confidence: None,
            }],
            onsets: vec![OnsetFeature {
                time: SongTime::from_frames(99),
                strength: 2.0,
                confidence: Some(0.25),
            }],
            sections: vec![SectionFeature {
                start: SongTime::ZERO,
                end: SongTime::from_frames(100),
                confidence: None,
                label: "手写区间".into(),
            }],
            energy: vec![EnergySample {
                start: SongTime::ZERO,
                frames: 100,
                rms: [0.2, 0.4],
                peak: [0.5, 0.75],
            }],
            diagnostics: "Manual fixture, not inferred confidence".into(),
        };
        let chart = CompiledChart {
            schema_version: CONTENT_SCHEMA_VERSION,
            audio_hash: [1; 32],
            ruleset_id: "duo-v1".into(),
            anchors: vec![Anchor {
                id: 7,
                song_time: SongTime::from_frames(50),
            }],
            sections: vec![SectionCue {
                id: 8,
                time: SongTime::ZERO,
                label: "start".into(),
            }],
        };
        let asset = |file_name: &str| AssetRef {
            file_name: file_name.into(),
            byte_len: 1,
            blake3: [2; 32],
        };
        let package = SongPackage {
            schema_version: CONTENT_SCHEMA_VERSION,
            song_id: "manual-song".into(),
            audio: asset("song.audio.ogg"),
            analysis: asset("analysis.bin"),
            chart: asset("chart.bin"),
            canonical_sample_rate: CANONICAL_SAMPLE_RATE,
            channels: 2,
            canonical_frames: 100,
            importer_version: "fixture-v1".into(),
            analysis_version: "measured-v1".into(),
            chart_version: "authored-v1".into(),
            package_hash: [9; 32],
        };
        (analysis, chart, package)
    }

    #[test]
    fn records_roundtrip_and_package_hash_uses_the_complete_zeroed_manifest() {
        let (analysis, chart, mut package) = fixtures();
        let analysis_bytes = encode_analysis(&analysis, 100).unwrap();
        let chart_bytes = encode_chart(&chart, 100).unwrap();
        let package_bytes = encode_package(&package).unwrap();
        assert_eq!(decode_analysis(&analysis_bytes, 100).unwrap(), analysis);
        assert_eq!(decode_chart(&chart_bytes, 100).unwrap(), chart);
        assert_eq!(decode_package(&package_bytes).unwrap(), package);
        for (bytes, magic) in [
            (&analysis_bytes, ANALYSIS_MAGIC),
            (&chart_bytes, CHART_MAGIC),
            (&package_bytes, PACKAGE_MAGIC),
        ] {
            assert_eq!(&bytes[..8], magic);
            assert_eq!(&bytes[8..12], &1u32.to_le_bytes());
            assert_eq!(
                u64::from_le_bytes(bytes[12..20].try_into().unwrap()),
                (bytes.len() - 20) as u64
            );
        }
        let identity = package_hash(&package).unwrap();
        package.package_hash = [0; 32];
        let zeroed = encode_package(&package).unwrap();
        assert_eq!(identity, *blake3::hash(&zeroed).as_bytes());
        assert_ne!(identity, *blake3::hash(&zeroed[HEADER_BYTES..]).as_bytes());
        package.package_hash = identity;
        assert_eq!(package_hash(&package).unwrap(), identity);
        package.chart.blake3[0] ^= 1;
        assert_ne!(package_hash(&package).unwrap(), identity);
    }

    #[test]
    fn analysis_v2_roundtrips_candidates_and_keeps_v1_unknown_and_bytes() {
        let (mut analysis, chart, package) = fixtures();
        let legacy = encode_analysis(&analysis, 100).unwrap();
        let decoded = decode_analysis(&legacy, 100).unwrap();
        assert_eq!(decoded.capabilities, None);
        assert_eq!(encode_analysis(&decoded, 100).unwrap(), legacy);
        let old_chart = encode_chart(&chart, 100).unwrap();
        let old_package = encode_package(&package).unwrap();
        let candidate = AnalysisCapability {
            state: AnalysisState::Candidate,
            source: AnalysisSource::Algorithm,
            confidence: None,
        };
        let mut caps = AnalysisCapabilities::authored();
        caps.onset = candidate;
        caps.beat = candidate;
        caps.downbeat = candidate;
        caps.tempo = candidate;
        caps.repetition = candidate;
        analysis.schema_version = ANALYSIS_SCHEMA_VERSION;
        analysis.capabilities = Some(caps);
        analysis.tempo_regions.push(TempoRegion {
            start: SongTime::ZERO,
            end: SongTime::from_frames(100),
            bpm: 120.0,
            beat_unit: None,
            confidence: None,
        });
        analysis.repetitions.push(RepetitionFeature {
            source_start: SongTime::ZERO,
            source_end: SongTime::from_frames(20),
            target_start: SongTime::from_frames(50),
            target_end: SongTime::from_frames(70),
            confidence: None,
        });
        let bytes = encode_analysis(&analysis, 100).unwrap();
        assert_eq!(&bytes[8..12], &ANALYSIS_SCHEMA_VERSION.to_le_bytes());
        assert_eq!(decode_analysis(&bytes, 100).unwrap(), analysis);
        let capability_offset = HEADER_BYTES
            + postcard::serialize_with_flavor(
                &analysis_wire(&analysis),
                postcard::ser_flavors::Size::default(),
            )
            .unwrap();
        for (offset, unknown) in [(capability_offset, 4), (capability_offset + 1, 3)] {
            let mut changed = bytes.clone();
            changed[offset] = unknown;
            assert!(
                decode_analysis(&changed, 100)
                    .unwrap_err()
                    .contains("Unknown analysis capability")
            );
        }
        assert!(decode_analysis(&bytes[..bytes.len() - 1], 100).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_analysis(&trailing, 100).is_err());
        let mut wrong_header = bytes.clone();
        wrong_header[8..12].copy_from_slice(&CONTENT_SCHEMA_VERSION.to_le_bytes());
        assert!(decode_analysis(&wrong_header, 100).is_err());
        assert_eq!(encode_chart(&chart, 100).unwrap(), old_chart);
        assert_eq!(encode_package(&package).unwrap(), old_package);
        analysis.capabilities.as_mut().unwrap().onset.state = AnalysisState::Unsupported;
        assert!(encode_analysis(&analysis, 100).is_err());
        analysis.capabilities.as_mut().unwrap().onset.state = AnalysisState::Candidate;
        analysis.tempo_regions[0].bpm = f32::NAN;
        assert!(encode_analysis(&analysis, 100).is_err());
        assert!(capability_from_wire((255, 0, None)).is_err());
        assert!(capability_from_wire((0, 255, None)).is_err());
        assert!(tempo_unit_from_wire(Some(255)).is_err());
        let mut changed = bytes;
        changed[8..12].copy_from_slice(&99_u32.to_le_bytes());
        assert!(decode_analysis(&changed, 100).is_err());
    }

    #[test]
    fn malformed_headers_lengths_trailing_payload_and_byte_limits_are_rejected() {
        let (_, chart, _) = fixtures();
        let bytes = encode_chart(&chart, 100).unwrap();
        for length in [0, 7, 19, bytes.len() - 1] {
            assert!(decode_chart(&bytes[..length], 100).is_err());
        }
        for offset in [0, 8, 12] {
            let mut changed = bytes.clone();
            changed[offset] ^= 1;
            assert!(decode_chart(&changed, 100).is_err());
        }
        let mut changed = bytes.clone();
        changed[12..20].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode_chart(&changed, 100).is_err());
        changed = bytes;
        changed.push(0);
        assert!(decode_chart(&changed, 100).is_err());
        let payload_len = (changed.len() - HEADER_BYTES) as u64;
        changed[12..20].copy_from_slice(&payload_len.to_le_bytes());
        assert!(
            decode_chart(&changed, 100)
                .unwrap_err()
                .contains("trailing")
        );
        assert!(decode_package(&vec![0; MAX_PACKAGE_BYTES + 1]).is_err());
        assert!(decode_chart(&vec![0; MAX_CHART_BYTES + 1], 100).is_err());
        assert!(decode_analysis(&vec![0; MAX_ANALYSIS_BYTES + 1], 100).is_err());
        assert!(
            encode(
                PACKAGE_MAGIC,
                &vec![0u8; MAX_PACKAGE_BYTES],
                MAX_PACKAGE_BYTES
            )
            .is_err()
        );
    }

    #[test]
    fn bounded_decoding_rejects_huge_sequence_claims_and_utf8_text_before_owning_it() {
        struct UnreachableItem;
        impl<'de> Deserialize<'de> for UnreachableItem {
            fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
                panic!("an oversized sequence must be rejected before visiting an item")
            }
        }
        #[derive(Deserialize)]
        struct BoundedProbe {
            #[serde(deserialize_with = "bounded_items")]
            _items: Vec<UnreachableItem>,
        }
        let oversized = postcard::to_allocvec(&vec![0u8; MAX_CONTENT_ITEMS + 1]).unwrap();
        assert!(postcard::take_from_bytes::<BoundedProbe>(&oversized).is_err());
        let huge_claim = encode(
            CHART_MAGIC,
            &(1u32, [0u8; 32], "duo-v1", usize::MAX),
            MAX_CHART_BYTES,
        )
        .unwrap();
        assert!(decode_chart(&huge_claim, 100).is_err());
        let (mut analysis, mut chart, _) = fixtures();
        chart.ruleset_id = "中".repeat(86);
        let bytes = encode(CHART_MAGIC, &chart_wire(&chart), MAX_CHART_BYTES).unwrap();
        assert!(decode_chart(&bytes, 100).is_err());
        analysis.diagnostics = "x".repeat(MAX_CONTENT_DIAGNOSTICS_BYTES + 1);
        let bytes = encode(
            ANALYSIS_MAGIC,
            &analysis_wire(&analysis),
            MAX_ANALYSIS_BYTES,
        )
        .unwrap();
        assert!(decode_analysis(&bytes, 100).is_err());
    }

    #[test]
    fn encoded_and_decoded_objects_reuse_schema_semantics_and_real_frame_counts() {
        let (mut analysis, mut chart, mut package) = fixtures();
        let bytes = encode_analysis(&analysis, 100).unwrap();
        assert!(decode_analysis(&bytes, 99).is_err());
        analysis.beats[0].strength = f32::NAN;
        assert!(encode_analysis(&analysis, 100).is_err());
        let bytes = encode(
            ANALYSIS_MAGIC,
            &analysis_wire(&analysis),
            MAX_ANALYSIS_BYTES,
        )
        .unwrap();
        assert!(decode_analysis(&bytes, 100).is_err());
        chart.anchors[0].song_time = SongTime::from_frames(100);
        assert!(encode_chart(&chart, 100).is_err());
        let bytes = encode(CHART_MAGIC, &chart_wire(&chart), MAX_CHART_BYTES).unwrap();
        assert!(decode_chart(&bytes, 100).is_err());
        package.schema_version = 2;
        assert!(encode_package(&package).is_err());
        let bytes = encode(
            PACKAGE_MAGIC,
            &package_wire(&package, [0; 32]),
            MAX_PACKAGE_BYTES,
        )
        .unwrap();
        assert!(decode_package(&bytes).is_err());
    }
}
