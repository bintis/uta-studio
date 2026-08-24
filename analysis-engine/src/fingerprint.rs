use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contract::{EngineError, EngineErrorCode, EngineResult};

pub const ACOUSTIC_DSP_VERSION: &str = "acoustic-dsp-v1";
pub const AUDIO_QUALITY_VERSION: &str = crate::contract::AUDIO_QUALITY_ALGORITHM_VERSION;
pub const CALIBRATION_VERSION: &str = "calibration-v1";
pub const FINALIZE_VOCAL_CHART_VERSION: &str = "finalize-vocal-chart-v2";
pub const FUSION_VERSION: &str = "fusion-v4";
pub const HSMM_VERSION: &str = "hsmm-v3";
pub const QUANTIZATION_VERSION: &str = "rhythm-grid-dp-v1";
pub const POSTPROCESS_VERSION: &str = "postprocess-v1";

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
    pub(crate) hsmm_version: &'static str,
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
