use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contract::{EngineError, EngineErrorCode, EngineResult, FusionDecisionProvenance};

pub const ACOUSTIC_DSP_VERSION: &str = "acoustic-dsp";
pub const AUDIO_QUALITY_VERSION: &str = crate::contract::AUDIO_QUALITY_ALGORITHM_VERSION;
pub const CALIBRATION_VERSION: &str = "calibration";
pub const FINALIZE_VOCAL_CHART_VERSION: &str = "finalize-vocal-chart";
pub const FUSION_VERSION: &str = "fusion";
pub const HSMM_VERSION: &str = "hsmm";
pub const QUANTIZATION_VERSION: &str = "rhythm-grid-dp";
pub const POSTPROCESS_VERSION: &str = "postprocess";

#[derive(Debug, Serialize)]
pub(crate) struct ExecutionIdentity<'a> {
    pub(crate) request: serde_json::Value,
    pub(crate) resources: Vec<FingerprintResource<'a>>,
    pub(crate) acoustic_dsp_version: &'static str,
    pub(crate) audio_quality_version: &'static str,
    pub(crate) quality_gates: &'a [String],
    pub(crate) calibration_version: &'static str,
    pub(crate) finalize_vocal_chart_version: &'static str,
    pub(crate) fusion_version: &'static str,
    pub(crate) fusion_decision: Option<&'a FusionDecisionProvenance>,
    pub(crate) quantization_version: &'static str,
    pub(crate) postprocess_version: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct FingerprintResource<'a> {
    pub(crate) model_id: &'a str,
    pub(crate) generation: &'a str,
    pub(crate) content_digest: &'a str,
    pub(crate) model_recipe_digest: &'a str,
    pub(crate) runtime_id: &'a str,
    pub(crate) runtime_generation: &'a str,
    pub(crate) runtime_recipe_digest: Option<&'a str>,
    pub(crate) backend: uta_runtime_manager::NativeBackend,
    pub(crate) device: &'static str,
}

pub fn deterministic_fingerprint<T: Serialize>(identity: &T) -> EngineResult<String> {
    let bytes = serde_json::to_vec(identity).map_err(|error| {
        EngineError::new(
            EngineErrorCode::InternalError,
            format!("could not serialize analysis identity: {error}"),
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{AnalysisReusePolicy, FusionDecisionProvenance};

    fn ai_decision(response_digest: &str) -> FusionDecisionProvenance {
        FusionDecisionProvenance::AiJudgment {
            adapter_resource: "tool:fusion_agent_adapter".to_string(),
            adapter_protocol: "uta.fusion_agent_request/uta.fusion_agent_response".to_string(),
            adapter_protocol_version: crate::contract::FUSION_AGENT_PROTOCOL_VERSION,
            adapter_identity: "test-adapter".to_string(),
            adapter_version: "1.0.0".to_string(),
            candidate_set_digest: "a".repeat(64),
            selected_candidate_ids: vec!["candidate-1".to_string()],
            response_digest: response_digest.to_string(),
            reuse_policy: AnalysisReusePolicy::PreservedRevisionOnly,
        }
    }

    #[test]
    fn fresh_ai_responses_have_distinct_execution_identity() {
        let first = ai_decision(&"b".repeat(64));
        let second = ai_decision(&"c".repeat(64));
        first.validate().unwrap();
        second.validate().unwrap();
        assert_ne!(
            deterministic_fingerprint(&first).unwrap(),
            deterministic_fingerprint(&second).unwrap()
        );
    }
}
