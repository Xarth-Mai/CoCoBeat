//! CoCoBeat-owned data contracts. No engine, audio, transport, or MIR types.

pub mod time;

pub use time::{CANONICAL_SAMPLE_RATE, MusicalTick, SessionEpoch, SongTime};
