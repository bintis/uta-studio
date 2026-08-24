use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contract::{EngineError, EngineErrorCode, EngineResult};

pub const ACOUSTIC_DSP_VERSION: &str = "acoustic-dsp-v1";
pub const CALIBRATION_VERSION: &str = "calibration-v1";
pub const FINALIZE_VOCAL_CHART_VERSION: &str = "finalize-vocal-chart-v2";
pub const FUSION_VERSION: &str = "fusion-v4";
pub const HSMM_VERSION: &str = "hsmm-v3";
pub const QUANTIZATION_VERSION: &str = "quantization-v1";
pub const POSTPROCESS_VERSION: &str = "postprocess-v1";

pub fn deterministic_fingerprint<T: Serialize>(identity: &T) -> EngineResult<String> {
    let bytes = serde_json::to_vec(identity).map_err(|error| {
        EngineError::new(
            EngineErrorCode::InternalError,
            format!("could not serialize analysis identity: {error}"),
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
