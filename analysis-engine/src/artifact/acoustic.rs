use serde::{Deserialize, Serialize};

use crate::contract::{CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult};
use crate::fingerprint::ACOUSTIC_DSP_VERSION;

pub const ACOUSTIC_EVIDENCE_CONTRACT: &str = "uta.analysis-engine.acoustic-evidence";
pub const ACOUSTIC_EVIDENCE_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcousticEvidenceFrameV1 {
    pub start: u64,
    pub rms: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spectral_flux: Option<f32>,
    pub periodicity: f32,
    pub snr_db: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fundamental_hz: Option<f32>,
    /// Source-local deterministic observations. These are not calibrated
    /// technique probabilities.
    #[serde(default)]
    pub vibrato_activation: f32,
    #[serde(default)]
    pub glide_activation: f32,
    #[serde(default)]
    pub ornament_activation: f32,
    #[serde(default)]
    pub breath_activation: f32,
    #[serde(default)]
    pub voicing_transition_activation: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcousticEvidenceV1 {
    pub contract: String,
    pub version: u32,
    pub algorithm: String,
    pub timebase: u32,
    pub start: u64,
    pub hop: u64,
    pub sample_rate: u32,
    pub window_samples: u32,
    pub semantic_audio_role: String,
    pub decoded_audio_sha256: String,
    pub frames: Vec<AcousticEvidenceFrameV1>,
}

impl AcousticEvidenceV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != ACOUSTIC_EVIDENCE_CONTRACT
            || self.version != ACOUSTIC_EVIDENCE_VERSION
            || self.algorithm != ACOUSTIC_DSP_VERSION
            || self.timebase != CANONICAL_TIMEBASE
            || self.hop == 0
            || self.sample_rate == 0
            || self.window_samples == 0
            || self.semantic_audio_role.trim().is_empty()
            || self.frames.is_empty()
        {
            return Err(invalid("acoustic evidence identity or shape is invalid"));
        }
        for (index, frame) in self.frames.iter().enumerate() {
            let expected = (index as u64)
                .checked_mul(self.hop)
                .and_then(|offset| self.start.checked_add(offset))
                .ok_or_else(|| invalid("acoustic evidence timeline overflows"))?;
            if frame.start != expected
                || !frame.rms.is_finite()
                || frame.rms < 0.0
                || frame
                    .spectral_flux
                    .is_some_and(|value| !value.is_finite() || value < 0.0)
                || !frame.periodicity.is_finite()
                || !(0.0..=1.0).contains(&frame.periodicity)
                || !frame.snr_db.is_finite()
                || frame
                    .fundamental_hz
                    .is_some_and(|value| !value.is_finite() || value <= 0.0)
                || [
                    frame.vibrato_activation,
                    frame.glide_activation,
                    frame.ornament_activation,
                    frame.breath_activation,
                    frame.voicing_transition_activation,
                ]
                .into_iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            {
                return Err(invalid("acoustic evidence frame is invalid or off-grid"));
            }
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}
