//! Bounded source decoding, 48 kHz resampling and strict canonical audio readback

mod decode;
mod resample;

pub use decode::{DecodedSource, decode_canonical, decode_source};
pub use resample::{ResampledSource, resample_source};
