use serde::{Deserialize, Serialize};

use crate::contract::{CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult};
use crate::fingerprint::{FUSION_VERSION, HSMM_VERSION};
use crate::fusion::{
    CanonicalSingingTrack, SegmentCandidate, SingingReviewRegion, validate_canonical_singing_track,
};

pub const SINGING_ANALYSIS_CONTRACT: &str = "uta.analysis-engine.singing-analysis";
pub const SINGING_ANALYSIS_VERSION: u32 = 1;
pub const SINGING_ANALYSIS_FORMAT_VERSION: &str = "0.3.0";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SingingAnalysisProvenanceV1 {
    pub execution_fingerprint: String,
    pub fusion_algorithm: String,
    pub candidate_graph_algorithm: String,
}

/// Stable immutable Engine analysis/review artifact. It intentionally contains
/// no authored state and no operation that can promote Candidate authority.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SingingAnalysisV1 {
    pub contract: String,
    pub version: u32,
    pub format_version: String,
    pub timebase: u32,
    pub track: CanonicalSingingTrack,
    /// Complete pre-decode candidate set, including candidates not selected by
    /// the coherent path. This preserves overlap/disagreement evidence.
    #[serde(default)]
    pub candidate_evidence: Vec<SegmentCandidate>,
    #[serde(default)]
    pub review_regions: Vec<SingingReviewRegion>,
    pub provenance: SingingAnalysisProvenanceV1,
}

impl SingingAnalysisV1 {
    pub fn new(
        track: CanonicalSingingTrack,
        candidate_evidence: Vec<SegmentCandidate>,
        review_regions: Vec<SingingReviewRegion>,
        execution_fingerprint: &str,
    ) -> EngineResult<Self> {
        let artifact = Self {
            contract: SINGING_ANALYSIS_CONTRACT.to_string(),
            version: SINGING_ANALYSIS_VERSION,
            format_version: SINGING_ANALYSIS_FORMAT_VERSION.to_string(),
            timebase: CANONICAL_TIMEBASE,
            track,
            candidate_evidence,
            review_regions,
            provenance: SingingAnalysisProvenanceV1 {
                execution_fingerprint: execution_fingerprint.to_string(),
                fusion_algorithm: FUSION_VERSION.to_string(),
                candidate_graph_algorithm: HSMM_VERSION.to_string(),
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
            || !is_sha256(&self.provenance.execution_fingerprint)
            || self.provenance.fusion_algorithm != FUSION_VERSION
            || self.provenance.candidate_graph_algorithm != HSMM_VERSION
        {
            return Err(invalid("SingingAnalysis contract or provenance is invalid"));
        }
        validate_canonical_singing_track(&self.track).map_err(invalid)?;
        if self.candidate_evidence.is_empty()
            || self
                .candidate_evidence
                .iter()
                .any(|candidate| candidate.emission_utility().is_err())
        {
            return Err(invalid("SingingAnalysis candidate evidence is invalid"));
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

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}
