use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::contract::{
    CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult, FusionDecisionProvenanceV1,
};
use crate::fingerprint::{FUSION_VERSION, HSMM_VERSION};
use crate::fusion::{
    CanonicalSingingTrack, HardBoundarySetV1, SegmentCandidate, SingingReviewRegion,
    validate_candidate_path_with_boundaries, validate_canonical_singing_track,
};

pub const SINGING_ANALYSIS_CONTRACT: &str = "uta.analysis-engine.singing-analysis";
pub const SINGING_ANALYSIS_VERSION: u32 = 1;
pub const SINGING_ANALYSIS_FORMAT_VERSION: &str = "0.3.0";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SingingAnalysisProvenanceV1 {
    pub execution_fingerprint: String,
    pub fusion_algorithm: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fusion_decision: Option<FusionDecisionProvenanceV1>,
    /// Deserialize-only compatibility for artifacts emitted before mode-specific
    /// decision provenance. New artifacts never write this unconditional HSMM claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_graph_algorithm: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SingingAnalysisChartReferencesV1 {
    pub track_id: String,
    pub phrase_id: String,
    #[serde(default)]
    pub note_ids: Vec<String>,
    #[serde(default)]
    pub lyric_token_ids: Vec<String>,
}

impl SingingAnalysisChartReferencesV1 {
    fn from_track(track: &CanonicalSingingTrack) -> Self {
        let linked = track
            .notes
            .iter()
            .filter_map(|note| note.word_id.as_deref())
            .collect::<BTreeSet<_>>();
        let mut note_ids = track
            .notes
            .iter()
            .filter(|note| note.word_id.is_some())
            .map(|note| note.id.clone())
            .collect::<Vec<_>>();
        note_ids.extend(
            track
                .words
                .iter()
                .enumerate()
                .filter(|(_, word)| !linked.contains(word.word_id.as_str()))
                .map(|(index, _)| format!("spoken-{index}")),
        );
        note_ids.sort();
        let mut lyric_token_ids = track
            .words
            .iter()
            .map(|word| word.word_id.clone())
            .collect::<Vec<_>>();
        lyric_token_ids.sort();
        Self {
            track_id: "lead".to_string(),
            phrase_id: "phrase-1".to_string(),
            note_ids,
            lyric_token_ids,
        }
    }

    fn is_valid(&self) -> bool {
        !self.track_id.trim().is_empty()
            && !self.phrase_id.trim().is_empty()
            && !self.note_ids.is_empty()
            && self.note_ids.iter().all(|id| !id.trim().is_empty())
            && self.note_ids.iter().collect::<BTreeSet<_>>().len() == self.note_ids.len()
            && self.lyric_token_ids.iter().all(|id| !id.trim().is_empty())
            && self.lyric_token_ids.iter().collect::<BTreeSet<_>>().len()
                == self.lyric_token_ids.len()
    }
}

/// Stable immutable Engine analysis/review artifact. New artifacts reference
/// the strict Candidate VocalChart IDs and do not copy a second authoritative
/// note timeline. `track` is deserialize-only compatibility for legacy cache
/// entries emitted before strict VocalChart publication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SingingAnalysisV1 {
    pub contract: String,
    pub version: u32,
    pub format_version: String,
    pub timebase: u32,
    #[serde(default)]
    pub chart_references: SingingAnalysisChartReferencesV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<CanonicalSingingTrack>,
    /// Complete pre-decode candidate set. Candidate ranges are proposal evidence,
    /// never a second selected/authoritative chart geometry.
    #[serde(default)]
    pub candidate_evidence: Vec<SegmentCandidate>,
    /// Pool-level caller-authored hard boundary authority used by both selectors.
    #[serde(default)]
    pub candidate_hard_boundaries: HardBoundarySetV1,
    #[serde(default)]
    pub review_regions: Vec<SingingReviewRegion>,
    pub provenance: SingingAnalysisProvenanceV1,
}

impl SingingAnalysisV1 {
    pub fn new(
        track: &CanonicalSingingTrack,
        candidate_evidence: Vec<SegmentCandidate>,
        candidate_hard_boundaries: HardBoundarySetV1,
        review_regions: Vec<SingingReviewRegion>,
        execution_fingerprint: &str,
        fusion_decision: &FusionDecisionProvenanceV1,
    ) -> EngineResult<Self> {
        validate_canonical_singing_track(track).map_err(invalid)?;
        let artifact = Self {
            contract: SINGING_ANALYSIS_CONTRACT.to_string(),
            version: SINGING_ANALYSIS_VERSION,
            format_version: SINGING_ANALYSIS_FORMAT_VERSION.to_string(),
            timebase: CANONICAL_TIMEBASE,
            chart_references: SingingAnalysisChartReferencesV1::from_track(track),
            track: None,
            candidate_evidence,
            candidate_hard_boundaries,
            review_regions,
            provenance: SingingAnalysisProvenanceV1 {
                execution_fingerprint: execution_fingerprint.to_string(),
                fusion_algorithm: FUSION_VERSION.to_string(),
                fusion_decision: Some(fusion_decision.clone()),
                candidate_graph_algorithm: None,
            },
        };
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != SINGING_ANALYSIS_CONTRACT
            || self.version != SINGING_ANALYSIS_VERSION
            || self.format_version != SINGING_ANALYSIS_FORMAT_VERSION
            || self.timebase != CANONICAL_TIMEBASE
            || self.provenance.execution_fingerprint.trim().is_empty()
            || self.provenance.fusion_algorithm != FUSION_VERSION
        {
            return Err(invalid("SingingAnalysis contract or provenance is invalid"));
        }
        match (
            self.provenance.fusion_decision.as_ref(),
            self.provenance.candidate_graph_algorithm.as_deref(),
        ) {
            (Some(decision), None) => decision.validate()?,
            (None, Some(legacy)) if legacy == HSMM_VERSION => {}
            _ => return Err(invalid("SingingAnalysis decision provenance is invalid")),
        }
        if let Some(track) = self.track.as_ref() {
            validate_canonical_singing_track(track).map_err(invalid)?;
        } else if !self.chart_references.is_valid() {
            return Err(invalid("SingingAnalysis chart references are invalid"));
        }
        self.candidate_hard_boundaries.validate().map_err(invalid)?;
        let mut candidate_ids = BTreeSet::new();
        if self.candidate_evidence.is_empty()
            || self.candidate_evidence.iter().any(|candidate| {
                candidate.id.trim().is_empty()
                    || !candidate_ids.insert(candidate.id.as_str())
                    || candidate.emission_utility().is_err()
            })
        {
            return Err(invalid("SingingAnalysis candidate evidence is invalid"));
        }
        if let Some(decision) = self.provenance.fusion_decision.as_ref() {
            let selected_ids = match decision {
                FusionDecisionProvenanceV1::Algorithm {
                    selected_candidate_ids,
                    ..
                }
                | FusionDecisionProvenanceV1::AiJudgment {
                    selected_candidate_ids,
                    ..
                } => selected_candidate_ids,
            };
            if selected_ids
                .iter()
                .any(|id| !candidate_ids.contains(id.as_str()))
            {
                return Err(invalid(
                    "SingingAnalysis decision does not match its candidate evidence",
                ));
            }
            let selected = selected_ids
                .iter()
                .map(|id| {
                    self.candidate_evidence
                        .iter()
                        .find(|candidate| &candidate.id == id)
                        .cloned()
                        .ok_or_else(|| invalid("selected candidate evidence is missing"))
                })
                .collect::<EngineResult<Vec<_>>>()?;
            validate_candidate_path_with_boundaries(
                &self.candidate_evidence,
                &selected,
                &self.candidate_hard_boundaries,
            )
            .map_err(invalid)?;
        }
        if self
            .review_regions
            .windows(2)
            .any(|pair| pair[0].range.end > pair[1].range.start)
            || self.review_regions.iter().any(|region| {
                region.id.trim().is_empty()
                    || region.range.end <= region.range.start
                    || region.reasons.is_empty()
                    || region
                        .confidence
                        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            })
        {
            return Err(invalid("SingingAnalysis review regions are invalid"));
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::AnalysisReusePolicyV1;

    #[test]
    fn ai_provenance_serialization_never_claims_hsmm_selection() {
        let provenance = SingingAnalysisProvenanceV1 {
            execution_fingerprint: "execution-fingerprint".to_string(),
            fusion_algorithm: FUSION_VERSION.to_string(),
            fusion_decision: Some(FusionDecisionProvenanceV1::AiJudgment {
                adapter_resource: "tool:fusion_agent_adapter".to_string(),
                adapter_protocol: "uta.fusion_agent_request/uta.fusion_agent_response".to_string(),
                adapter_protocol_version: crate::contract::FUSION_AGENT_PROTOCOL_VERSION,
                adapter_identity: "uta-test-adapter".to_string(),
                adapter_version: "1.0.0".to_string(),
                candidate_set_digest: "a".repeat(64),
                selected_candidate_ids: vec!["candidate-1".to_string()],
                response_digest: "b".repeat(64),
                reuse_policy: AnalysisReusePolicyV1::PreservedRevisionOnly,
            }),
            candidate_graph_algorithm: None,
        };
        provenance
            .fusion_decision
            .as_ref()
            .unwrap()
            .validate()
            .unwrap();
        let json = serde_json::to_value(provenance).unwrap();
        assert_eq!(json["fusion_decision"]["decision_mode"], "ai_judgment");
        assert!(json.get("candidate_graph_algorithm").is_none());
        assert!(!json.to_string().contains(HSMM_VERSION));
    }
}
