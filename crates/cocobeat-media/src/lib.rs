//! Bounded source decoding, 48 kHz resampling and strict canonical audio readback

mod anchors;
mod audio_asset;
mod authored;
mod content_codec;
mod decode;
mod encode;
#[cfg(any(
    all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"),
    all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"),
    all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"),
    all(target_os = "windows", target_arch = "aarch64", target_env = "msvc")
))]
mod native_beat;
mod native_beat_evidence;
mod native_cancellation;
mod package;
mod resample;

pub use anchors::{
    ANCHOR_COMPILER_VERSION, AnchorDecision, AnchorEvidence, AnchorPolicy, AnchorProposal,
    compile_anchor_proposal,
};
pub use audio_asset::{PreparedCanonicalAudio, prepare_canonical_audio};
pub use authored::{
    build_authored_package, import_authored_package, import_experimental_beat_package,
    import_experimental_beat_package_with_cancellation,
};
pub use decode::{DecodedSource, decode_canonical, decode_source};
pub use encode::{CANONICAL_ENCODER_PROFILE, MAX_ENCODER_PCM_PEAK, encode_canonical_audio};
pub use native_beat_evidence::{
    NativeBeatEvidence, NativeBeatKind, NativeBeatMetadata, NativeBeatRecord,
    NativeDownbeatAlignment, NativePeakMember, read_native_beat_evidence,
};
pub use native_cancellation::NativeBeatCancellation;
pub use package::{
    MAX_RECEIVED_PACKAGE_BYTES, PACKAGE_OBJECT_LIMITS, PACKAGE_OBJECT_NAMES, PackageBuildInput,
    ReceivedPackage, ValidatedPackage, build_package, export_anchors, read_package,
    validate_package, validate_package_objects,
};
pub use resample::{ResampledSource, resample_source};
