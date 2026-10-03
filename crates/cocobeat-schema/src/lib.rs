//! CoCoBeat-owned data contracts. No engine, audio, transport, or MIR types.

pub mod asset;
pub mod gameplay;
pub mod time;

pub use asset::AssetRef;
pub use gameplay::{
    Anchor, AnchorGrade, AnchorJudgement, AnchorSyncEvent, DuoEvent, DuoInput, DuoRules,
    FreeSyncEvent, Hit, PlayerId, ResonanceState,
};
pub use time::{CANONICAL_SAMPLE_RATE, MusicalTick, SessionEpoch, SongTime};
