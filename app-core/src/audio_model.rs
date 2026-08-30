//! Read-only Runtime Manager model presentation used by Settings.
//! Model lifecycle truth remains behind the `uta-runtime` protocol.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const AUDIO_CATALOG_VERSION: &str = "native-final-v1";
pub const AUDIO_CATALOG_SCHEMA_VERSION: u32 = 1;

pub const DEFAULT_VOCAL_MODEL_ID: &str = "bs_roformer_leap_xe90_vocals";
pub const DEFAULT_BGM_MODEL_ID: &str = "bs_polarformer_public_instrumental";

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioModelLicense {
    pub status: String,
    pub source_attribution: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_page: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioModelStatus {
    pub model_id: String,
    pub display_name: String,
    pub purpose: String,
    pub architecture: String,
    pub operation: String,
    pub runner: String,
    pub supported_backends: Vec<String>,
    pub license: AudioModelLicense,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_bytes: Option<u64>,
    pub state: String,
    pub catalog_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioModelCatalogSummary {
    pub schema_version: u32,
    pub catalog_version: String,
    pub models: Vec<AudioModelStatus>,
}
