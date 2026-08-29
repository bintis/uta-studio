use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{AnalysisProfile, EngineError, EngineErrorCode, EngineResult};

pub const AUDIO_QUALITY_REPORT_CONTRACT: &str = "uta.analysis-engine.audio-quality-report";
pub const AUDIO_QUALITY_REPORT_VERSION: u32 = 1;
pub const AUDIO_QUALITY_ALGORITHM_VERSION: &str = "audio-quality-gates-v2";
const LEGACY_AUDIO_QUALITY_ALGORITHM_VERSION: &str = "audio-quality-gates-v1";
pub const VOCAL_TOPOLOGY_ESTIMATE_CONTRACT: &str = "uta.analysis-engine.vocal-topology-estimate";
pub const VOCAL_TOPOLOGY_ESTIMATE_VERSION: u32 = 1;

pub const TIMELINE_VALID_GATE: &str = "timeline_valid";
pub const FINITE_SAMPLES_GATE: &str = "finite_samples";
pub const CLIPPING_GATE: &str = "clipping";
pub const SILENCE_RATIO_GATE: &str = "silence_ratio";
pub const ENERGY_RATIO_GATE: &str = "energy_ratio";
pub const LEAD_PURITY_GATE: &str = "lead_purity";
pub const VOCAL_LEAKAGE_GATE: &str = "vocal_leakage";
pub const MUSICAL_DAMAGE_GATE: &str = "musical_damage";
pub const CLEANUP_CONSISTENCY_GATE: &str = "cleanup_consistency";
pub const VOCAL_TOPOLOGY_GATE: &str = "vocal_topology";

pub const AUDIO_QUALITY_GATE_ORDER: &[&str] = &[
    TIMELINE_VALID_GATE,
    FINITE_SAMPLES_GATE,
    CLIPPING_GATE,
    SILENCE_RATIO_GATE,
    ENERGY_RATIO_GATE,
    LEAD_PURITY_GATE,
    VOCAL_LEAKAGE_GATE,
    MUSICAL_DAMAGE_GATE,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VocalTopologyModeV1 {
    SingleLead,
    AlternatingMultiLead,
    OverlappingMultiLead,
    LeadWithSupport,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VocalTopologyEstimateV1 {
    pub contract: String,
    pub version: u32,
    pub timebase: u32,
    pub source_start: u64,
    pub duration: u64,
    pub mode: VocalTopologyModeV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub overlap_regions: Vec<QualityRegionV1>,
    #[serde(default)]
    pub support_regions: Vec<QualityRegionV1>,
    pub evidence_sources: Vec<String>,
}

impl VocalTopologyEstimateV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != VOCAL_TOPOLOGY_ESTIMATE_CONTRACT
            || self.version != VOCAL_TOPOLOGY_ESTIMATE_VERSION
            || self.timebase != super::CANONICAL_TIMEBASE
            || self.duration == 0
            || self.evidence_sources.is_empty()
            || self
                .evidence_sources
                .iter()
                .any(|source| source.trim().is_empty())
            || self
                .confidence
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || !valid_topology_regions(self.source_start, self.duration, &self.overlap_regions)
            || !valid_topology_regions(self.source_start, self.duration, &self.support_regions)
        {
            return Err(invalid(
                "vocal topology estimate identity or shape is invalid",
            ));
        }
        let mode_shape_valid = match self.mode {
            VocalTopologyModeV1::SingleLead | VocalTopologyModeV1::Unknown => {
                self.overlap_regions.is_empty() && self.support_regions.is_empty()
            }
            VocalTopologyModeV1::AlternatingMultiLead => self.overlap_regions.is_empty(),
            VocalTopologyModeV1::OverlappingMultiLead => !self.overlap_regions.is_empty(),
            VocalTopologyModeV1::LeadWithSupport => !self.support_regions.is_empty(),
        };
        if !mode_shape_valid {
            return Err(invalid(
                "vocal topology mode conflicts with its typed regions",
            ));
        }
        Ok(())
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocal_topology: Option<VocalTopologyEstimateV1>,
}

impl AudioQualityReportV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != AUDIO_QUALITY_REPORT_CONTRACT
            || self.version != AUDIO_QUALITY_REPORT_VERSION
            || !matches!(
                self.algorithm.as_str(),
                AUDIO_QUALITY_ALGORITHM_VERSION | LEGACY_AUDIO_QUALITY_ALGORITHM_VERSION
            )
            || self.evaluated_audio_role.trim().is_empty()
            || self.duration == 0
            || self.planned_gates.is_empty()
            || self.planned_gates.len() != self.outcomes.len()
            || self.vocal_topology.as_ref().is_some_and(|topology| {
                topology.duration != self.duration || topology.validate().is_err()
            })
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
            if !valid_quality_region_shapes(&outcome.regions) {
                return Err(invalid("audio quality region is invalid"));
            }
        }
        Ok(())
    }

    pub fn validate_for_source(&self, source_start: u64) -> EngineResult<()> {
        self.validate()?;
        if self
            .vocal_topology
            .as_ref()
            .is_some_and(|topology| topology.source_start != source_start)
            || self.outcomes.iter().any(|outcome| {
                !valid_topology_regions(source_start, self.duration, &outcome.regions)
            })
        {
            return Err(invalid(
                "audio quality regions do not match the source timeline",
            ));
        }
        Ok(())
    }
}

fn valid_quality_region_shapes(regions: &[QualityRegionV1]) -> bool {
    regions
        .iter()
        .all(|region| region.start < region.end && !region.reason.trim().is_empty())
}

fn valid_topology_regions(source_start: u64, duration: u64, regions: &[QualityRegionV1]) -> bool {
    let Some(source_end) = source_start.checked_add(duration) else {
        return false;
    };
    regions.iter().all(|region| {
        region.start >= source_start
            && region.end <= source_end
            && region.start < region.end
            && !region.reason.trim().is_empty()
    }) && regions.windows(2).all(|pair| pair[0].end <= pair[1].start)
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
