//! Bounded source decoding, 48 kHz resampling and strict canonical audio readback

mod anchors;
mod audio_asset;
mod content_codec;
mod decode;
mod package;
mod resample;

pub use anchors::{
    ANCHOR_COMPILER_VERSION, AnchorDecision, AnchorEvidence, AnchorPolicy, AnchorProposal,
    compile_anchor_proposal,
};
pub use audio_asset::{PreparedCanonicalAudio, prepare_canonical_audio};
pub use decode::{DecodedSource, decode_canonical, decode_source};
pub use package::{
    PackageBuildInput, ValidatedPackage, build_package, export_anchors, read_package,
    validate_package,
};
pub use resample::{ResampledSource, resample_source};
