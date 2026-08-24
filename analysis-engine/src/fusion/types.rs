use serde::{Deserialize, Serialize};

use crate::contract::{CANONICAL_TIMEBASE, CanonicalTime};

pub const CANONICAL_TIMELINE_STEP_MS: u32 = 10;
pub const CANONICAL_TIMELINE_STEP: u64 = 10_000;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_group: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: CanonicalTime,
    pub end: CanonicalTime,
}

impl TimeRange {
    pub fn new(start: CanonicalTime, end: CanonicalTime) -> Result<Self, String> {
        if end <= start {
            return Err("time range must be non-empty".to_string());
        }
        Ok(Self { start, end })
    }

    pub fn from_seconds(start: f64, end: f64) -> Result<Self, String> {
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
            return Err("time range must be finite, non-negative, and non-empty".to_string());
        }
        let convert = |seconds: f64| {
            let units = seconds * CANONICAL_TIMEBASE as f64;
            if units > u64::MAX as f64 {
                Err("time range overflows the canonical timeline".to_string())
            } else {
                Ok(units.round() as u64)
            }
        };
        Self::new(convert(start)?, convert(end)?)
    }

    pub fn start_seconds(self) -> f64 {
        self.start as f64 / CANONICAL_TIMEBASE as f64
    }

    pub fn end_seconds(self) -> f64 {
        self.end as f64 / CANONICAL_TIMEBASE as f64
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

/// Technique probabilities are optional because no technique expert is part of
/// the current baseline. `None` is distinct from a measured probability of 0.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct TechniqueScores {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vibrato: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glissando: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub falsetto: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ornament: Option<f32>,
}

impl TechniqueScores {
    pub fn validated(self) -> Option<Self> {
        [self.vibrato, self.glissando, self.falsetto, self.ornament]
            .into_iter()
            .flatten()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
            .then_some(self)
    }
}
