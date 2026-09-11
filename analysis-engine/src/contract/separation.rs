//! Descriptive separation measurements, not reference-based quality scores or gates.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeparationQualityEvidence {
    pub node_id: String,
    pub model_id: String,
    pub measurement: String,
    pub reference_status: String,
    pub stems: Vec<SeparatedStemMeasurement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeparatedStemMeasurement {
    pub role: String,
    /// Relative to this run's output root; never resolved against current artifacts.
    pub artifact_path: PathBuf,
    pub sample_rate: u32,
    pub channels: u16,
    pub frame_count: u64,
    pub duration_seconds: f64,
    pub sample_count: u64,
    pub finite_samples: bool,
    pub peak_amplitude: f32,
    pub rms_amplitude: f64,
    /// Existing decoder definition: abs(sample) >= 0.999.
    pub near_full_scale_ratio: f64,
    /// Existing decoder definition: abs(sample) <= 0.0001.
    pub silent_sample_ratio: f64,
}
