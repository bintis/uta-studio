use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{AnalysisProfile, EngineError, EngineErrorCode, EngineResult};

pub const AUDIO_QUALITY_REPORT_CONTRACT: &str = "uta.analysis-engine.audio-quality-report";
pub const AUDIO_QUALITY_REPORT_VERSION: u32 = 1;
pub const AUDIO_QUALITY_ALGORITHM_VERSION: &str = "audio-quality-gates-v1";

pub const TIMELINE_VALID_GATE: &str = "timeline_valid";
pub const FINITE_SAMPLES_GATE: &str = "finite_samples";
pub const CLIPPING_GATE: &str = "clipping";
pub const SILENCE_RATIO_GATE: &str = "silence_ratio";
pub const ENERGY_RATIO_GATE: &str = "energy_ratio";
pub const LEAD_PURITY_GATE: &str = "lead_purity";
pub const CLEANUP_CONSISTENCY_GATE: &str = "cleanup_consistency";
pub const VOCAL_TOPOLOGY_GATE: &str = "vocal_topology";

pub const AUDIO_QUALITY_GATE_ORDER: &[&str] = &[
    TIMELINE_VALID_GATE,
    FINITE_SAMPLES_GATE,
    CLIPPING_GATE,
    SILENCE_RATIO_GATE,
    ENERGY_RATIO_GATE,
    LEAD_PURITY_GATE,
    CLEANUP_CONSISTENCY_GATE,
    VOCAL_TOPOLOGY_GATE,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityGateRequirementV1 {
    Required,
    Degrading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityGateStatusV1 {
    Passed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityMetricV1 {
    pub name: String,
    pub value: f64,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lower_bound: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper_bound: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityRegionV1 {
    pub start: u64,
    pub end: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityGateOutcomeV1 {
    pub gate: String,
    pub requirement: QualityGateRequirementV1,
    pub status: QualityGateStatusV1,
    pub summary: String,
    #[serde(default)]
    pub metrics: Vec<QualityMetricV1>,
    #[serde(default)]
    pub regions: Vec<QualityRegionV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioQualityReportV1 {
    pub contract: String,
    pub version: u32,
    pub algorithm: String,
    pub profile: AnalysisProfile,
    pub evaluated_audio_role: String,
    pub duration: u64,
    pub planned_gates: Vec<String>,
    pub outcomes: Vec<QualityGateOutcomeV1>,
}

impl AudioQualityReportV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != AUDIO_QUALITY_REPORT_CONTRACT
            || self.version != AUDIO_QUALITY_REPORT_VERSION
            || self.algorithm != AUDIO_QUALITY_ALGORITHM_VERSION
            || self.evaluated_audio_role.trim().is_empty()
            || self.duration == 0
            || self.planned_gates.is_empty()
            || self.planned_gates.len() != self.outcomes.len()
        {
            return Err(invalid("audio quality report identity or shape is invalid"));
        }
        let mut seen = BTreeSet::new();
        let mut previous_order = None;
        for (planned, outcome) in self.planned_gates.iter().zip(&self.outcomes) {
            let order = AUDIO_QUALITY_GATE_ORDER
                .iter()
                .position(|known| *known == planned)
                .ok_or_else(|| invalid(format!("unknown audio quality gate: {planned}")))?;
            if !seen.insert(planned.as_str())
                || previous_order.is_some_and(|previous| order <= previous)
                || outcome.gate != *planned
                || outcome.requirement != gate_requirement(planned)
                || outcome.summary.trim().is_empty()
                || (outcome.requirement == QualityGateRequirementV1::Required
                    && outcome.status == QualityGateStatusV1::Unknown)
            {
                return Err(invalid(
                    "audio quality gate identity or ordering is invalid",
                ));
            }
            previous_order = Some(order);
            for metric in &outcome.metrics {
                if metric.name.trim().is_empty()
                    || metric.unit.trim().is_empty()
                    || !metric.value.is_finite()
                    || metric.lower_bound.is_some_and(|value| !value.is_finite())
                    || metric.upper_bound.is_some_and(|value| !value.is_finite())
                    || matches!((metric.lower_bound, metric.upper_bound), (Some(low), Some(high)) if low > high)
                {
                    return Err(invalid("audio quality metric is invalid"));
                }
            }
            for region in &outcome.regions {
                if region.start >= region.end || region.reason.trim().is_empty() {
                    return Err(invalid("audio quality region is invalid"));
                }
            }
        }
        Ok(())
    }
}

pub fn gate_requirement(gate: &str) -> QualityGateRequirementV1 {
    match gate {
        TIMELINE_VALID_GATE | FINITE_SAMPLES_GATE | SILENCE_RATIO_GATE | ENERGY_RATIO_GATE => {
            QualityGateRequirementV1::Required
        }
        _ => QualityGateRequirementV1::Degrading,
    }
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}
