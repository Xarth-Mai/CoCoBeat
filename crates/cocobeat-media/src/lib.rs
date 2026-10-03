//! Bounded source decoding and 48 kHz resampling for the canonical audio import path

mod decode;
mod resample;

pub use decode::{DecodedSource, decode_source};
pub use resample::{ResampledSource, resample_source};
