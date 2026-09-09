mod acoustic;
mod decode;
mod quality;

pub use acoustic::analyze_acoustic_evidence;
pub(crate) use decode::decode_audio_with_cancellation;
pub use decode::{DecodedAudio, decode_audio};
pub(crate) use quality::{
    CleanupComparison, QualityEvaluationInput, SignalAccumulator, SignalMetrics, SignalProfile,
    build_signal_window, enforce_required_quality, estimate_instrumental_quality,
    estimate_vocal_topology, evaluate_audio_quality, quality_degraded_reasons,
    signal_profile_window_frames, topology_review_regions,
};
