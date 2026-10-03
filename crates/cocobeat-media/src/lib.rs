//! Bounded source decoding, 48 kHz resampling and strict canonical audio readback

mod audio_asset;
mod decode;
mod resample;

pub use audio_asset::{PreparedCanonicalAudio, prepare_canonical_audio};
pub use decode::{DecodedSource, decode_canonical, decode_source};
pub use resample::{ResampledSource, resample_source};
