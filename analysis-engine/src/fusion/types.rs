use serde::{Deserialize, Serialize};

use crate::contract::{
    BoundaryAuthority, BoundaryConstraintV1, BoundaryLevel, CANONICAL_TIMEBASE, CanonicalTime,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

/// One caller-authored hard boundary interval. Hardness is intrinsic to this
/// type so generated candidate evidence cannot accidentally become structural
/// authority by carrying a copied boolean.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardBoundaryV1 {
    pub source: String,
    pub level: BoundaryLevel,
    pub range: TimeRange,
}

/// Normalized pool-level structural boundary authority shared by Algorithm and
/// AI selectors. Exact ranges are retained; timing tolerance is applied only
/// while querying their edges.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardBoundarySetV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boundaries: Vec<HardBoundaryV1>,
}

impl HardBoundarySetV1 {
    pub fn from_constraints(
        constraints: &[BoundaryConstraintV1],
        source_start: u64,
        source_end: u64,
    ) -> Result<Self, String> {
        let mut boundaries = Vec::new();
        for constraint in constraints
            .iter()
            .filter(|constraint| constraint.authority == BoundaryAuthority::Hard)
        {
            let end = constraint
                .start
                .checked_add(constraint.duration)
                .ok_or_else(|| "hard boundary end overflows canonical timeline".to_string())?;
            if constraint.start < source_start || end > source_end {
                return Err(format!(
                    "hard boundary from {} is outside the analyzed source timeline",
                    constraint.source
                ));
            }
            if constraint.source.trim().is_empty() {
                return Err("hard boundary source identity is empty".to_string());
            }
            boundaries.push(HardBoundaryV1 {
                source: constraint.source.clone(),
                level: constraint.level,
                range: TimeRange::new(constraint.start, end)?,
            });
        }
        boundaries.sort_by(|left, right| {
            (
                left.range.start,
                left.range.end,
                left.source.as_str(),
                left.level,
            )
                .cmp(&(
                    right.range.start,
                    right.range.end,
                    right.source.as_str(),
                    right.level,
                ))
        });
        boundaries.dedup();
        let result = Self { boundaries };
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.boundaries.iter().any(|boundary| {
            boundary.source.trim().is_empty() || boundary.range.start >= boundary.range.end
        }) || self.boundaries.windows(2).any(|pair| {
            (
                pair[0].range.start,
                pair[0].range.end,
                pair[0].source.as_str(),
                pair[0].level,
            ) >= (
                pair[1].range.start,
                pair[1].range.end,
                pair[1].source.as_str(),
                pair[1].level,
            )
        }) {
            return Err("hard boundary set is not normalized".to_string());
        }
        Ok(())
    }

    pub fn edge_times(&self) -> Vec<u64> {
        let mut times = self
            .boundaries
            .iter()
            .flat_map(|boundary| [boundary.range.start, boundary.range.end])
            .collect::<Vec<_>>();
        times.sort_unstable();
        times.dedup();
        times
    }

    pub fn crosses(&self, range: TimeRange, tolerance: u64) -> bool {
        let crosses = |time: u64| {
            time.saturating_add(tolerance) < range.end
                && time > range.start.saturating_add(tolerance)
        };
        self.boundaries
            .iter()
            .any(|boundary| crosses(boundary.range.start) || crosses(boundary.range.end))
    }

    pub fn resets_between(&self, previous_end: u64, next_start: u64, tolerance: u64) -> bool {
        let resets = |time: u64| {
            time >= previous_end.saturating_sub(tolerance)
                && time <= next_start.saturating_add(tolerance)
        };
        self.boundaries
            .iter()
            .any(|boundary| resets(boundary.range.start) || resets(boundary.range.end))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn constraint(source: &str, start: u64, authority: BoundaryAuthority) -> BoundaryConstraintV1 {
        BoundaryConstraintV1 {
            token_id: None,
            level: BoundaryLevel::Word,
            start,
            duration: 100_000,
            confidence: 1.0,
            authority,
            source: source.to_string(),
        }
    }

    #[test]
    fn hard_boundary_set_keeps_only_caller_hard_authority_and_normalizes_by_time() {
        let set = HardBoundarySetV1::from_constraints(
            &[
                constraint("hard-late", 500_000, BoundaryAuthority::Hard),
                constraint("soft", 250_000, BoundaryAuthority::Soft),
                constraint("hard-early", 100_000, BoundaryAuthority::Hard),
            ],
            0,
            1_000_000,
        )
        .unwrap();
        assert_eq!(set.boundaries.len(), 2);
        assert_eq!(set.boundaries[0].source, "hard-early");
        assert_eq!(set.boundaries[1].source, "hard-late");
        assert_eq!(set.edge_times(), [100_000, 200_000, 500_000, 600_000]);

        let invalid = HardBoundarySetV1 {
            boundaries: vec![HardBoundaryV1 {
                source: "invalid".to_string(),
                level: BoundaryLevel::Phrase,
                range: TimeRange {
                    start: 200_000,
                    end: 200_000,
                },
            }],
        };
        assert_eq!(
            invalid.validate().unwrap_err(),
            "hard boundary set is not normalized"
        );
    }
}
