use serde::{Deserialize, Serialize};

pub const CANONICAL_TIMELINE_STEP_MS: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpertTask {
    Transcript,
    WordBoundary,
    ContinuousPitch,
    NoteBoundary,
    Onset,
    Technique,
    SymbolicPrior,
    Acoustic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceProvenance {
    pub expert_id: String,
    pub task: ExpertTask,
    pub model_hash: String,
    pub runtime_recipe_digest: String,
    pub calibration_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_group: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
}

impl TimeRange {
    pub fn new(start: f64, end: f64) -> Result<Self, String> {
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
            return Err("time range must be finite, non-negative, and non-empty".to_string());
        }
        Ok(Self { start, end })
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct TechniqueScores {
    #[serde(default)]
    pub vibrato: f32,
    #[serde(default)]
    pub glissando: f32,
    #[serde(default)]
    pub falsetto: f32,
    #[serde(default)]
    pub ornament: f32,
}

impl TechniqueScores {
    pub fn clamped(self) -> Self {
        Self {
            vibrato: self.vibrato.clamp(0.0, 1.0),
            glissando: self.glissando.clamp(0.0, 1.0),
            falsetto: self.falsetto.clamp(0.0, 1.0),
            ornament: self.ornament.clamp(0.0, 1.0),
        }
    }
}
