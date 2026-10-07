//! CoCoBeat-owned data contracts. No engine, audio, transport, or MIR types.

/// Current native geometry contract shared with transport identity
pub const STAGE_COMPILER_VERSION: u32 = 2;

pub mod asset;
pub mod content;
pub mod gameplay;
pub mod time;

pub use asset::AssetRef;
pub use content::{
    ANALYSIS_SCHEMA_VERSION, AnalysisCapabilities, AnalysisCapability, AnalysisSource,
    AnalysisState, BeatFeature, CONTENT_SCHEMA_VERSION, CompiledChart, EnergySample,
    MAX_CANONICAL_FRAMES, MAX_CONTENT_DIAGNOSTICS_BYTES, MAX_CONTENT_ITEMS, MAX_CONTENT_TEXT_BYTES,
    MusicAnalysis, OnsetFeature, RepetitionFeature, SectionCue, SectionFeature, SongPackage,
    TempoBeatUnit, TempoRegion,
};
pub use gameplay::{
    Anchor, AnchorGrade, AnchorJudgement, AnchorSyncEvent, DuoEvent, DuoInput, DuoRules,
    FreeSyncEvent, Hit, PlayerId, ResonanceState,
};
pub use time::{CANONICAL_SAMPLE_RATE, MusicalTick, SessionEpoch, SongTime};
