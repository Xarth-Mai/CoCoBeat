//! Bounded source decoding, 48 kHz resampling and strict canonical audio readback

mod anchors;
mod audio_asset;
mod content_codec;
mod decode;
mod encode;
mod package;
mod resample;

pub use anchors::{
    ANCHOR_COMPILER_VERSION, AnchorDecision, AnchorEvidence, AnchorPolicy, AnchorProposal,
    compile_anchor_proposal,
};
pub use audio_asset::{PreparedCanonicalAudio, prepare_canonical_audio};
pub use decode::{DecodedSource, decode_canonical, decode_source};
pub use encode::{CANONICAL_ENCODER_PROFILE, MAX_ENCODER_PCM_PEAK, encode_canonical_audio};
pub use package::{
    MAX_RECEIVED_PACKAGE_BYTES, PACKAGE_OBJECT_LIMITS, PACKAGE_OBJECT_NAMES, PackageBuildInput,
    ReceivedPackage, ValidatedPackage, build_package, export_anchors, read_package,
    validate_package, validate_package_objects,
};
pub use resample::{ResampledSource, resample_source};
