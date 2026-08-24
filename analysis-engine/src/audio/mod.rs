mod acoustic;
mod decode;
mod quality;

pub use acoustic::analyze_acoustic_evidence;
pub use decode::{DecodedAudio, decode_audio};
pub(crate) use decode::{decode_audio_with_cancellation, extract_audio_window};
pub(crate) use quality::{
    CleanupComparison, QualityEvaluationInput, SignalAccumulator, SignalMetrics,
    enforce_required_quality, evaluate_audio_quality, quality_degraded_reasons,
};
