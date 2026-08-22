use serde::{Deserialize, Serialize};

use super::{CANONICAL_TIMELINE_STEP_MS, EvidenceProvenance, TechniqueScores};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScalarEvidence {
    pub value: f32,
    pub confidence: f32,
}

impl ScalarEvidence {
    pub fn validated(self) -> Option<Self> {
        (self.value.is_finite() && self.confidence.is_finite()).then_some(Self {
            value: self.value,
            confidence: self.confidence.clamp(0.0, 1.0),
        })
    }
}

/// Sparse semantic frame on the canonical 10 ms timeline. `None` means the
/// expert did not run; it is intentionally distinct from a measured zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceFrame {
    pub frame_index: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_f0_hz: Option<ScalarEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_f0_hz: Option<ScalarEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_pitch_hz: Option<ScalarEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_boundary: Option<ScalarEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stars_pitch_hz: Option<ScalarEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stars_boundary: Option<ScalarEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic_pitch_onset: Option<ScalarEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyric_boundary: Option<ScalarEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbolic_note_prior: Option<ScalarEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbolic_boundary_prior: Option<ScalarEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rms: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spectral_flux: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub periodicity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snr_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_id: Option<String>,
    #[serde(default)]
    pub techniques: TechniqueScores,
}

impl EvidenceFrame {
    pub fn time_seconds(&self) -> f64 {
        self.frame_index as f64 * CANONICAL_TIMELINE_STEP_MS as f64 / 1_000.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceSeries {
    #[serde(default = "timeline_step")]
    pub timeline_step_ms: u32,
    #[serde(default)]
    pub frames: Vec<EvidenceFrame>,
    #[serde(default)]
    pub provenance: Vec<EvidenceProvenance>,
}

const fn timeline_step() -> u32 {
    CANONICAL_TIMELINE_STEP_MS
}

impl EvidenceSeries {
    pub fn validate(&self) -> Result<(), String> {
        if self.timeline_step_ms != CANONICAL_TIMELINE_STEP_MS {
            return Err(format!(
                "evidence timeline step must be {CANONICAL_TIMELINE_STEP_MS} ms"
            ));
        }
        if self
            .frames
            .windows(2)
            .any(|pair| pair[0].frame_index >= pair[1].frame_index)
        {
            return Err("evidence frame indices must be strictly increasing".to_string());
        }
        Ok(())
    }
}
