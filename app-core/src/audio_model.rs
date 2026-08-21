//! Offline audio Model Catalog types shared by Settings, Analysis, and the
//! analyzer protocol. Model IDs are stable strings; checkpoint filenames
//! never leave this module's catalog representation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const AUDIO_CATALOG_VERSION: &str = "2026.08.1";
pub const AUDIO_CATALOG_SCHEMA_VERSION: u32 = 1;

pub const REQUIRED_AUDIO_MODEL_IDS: &[&str] = &[
    "bs_roformer_vocals_ep317",
    "melband_roformer_inst_v2",
    "htdemucs_6s",
    "melband_roformer_denoise_aufr33",
    "melband_roformer_dereverb_anvuew",
    "uvr_mdxnet_karaoke_2",
];

pub const DEFAULT_LEGACY_KARAOKE_MODEL_ID: &str = "melband_roformer_karaoke_aufr33_viperx";
pub const DEFAULT_BGM_MODEL_ID: &str = "melband_roformer_inst_v2";

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(untagged)]
pub enum AudioParameterValue {
    Bool(bool),
    Integer(i64),
    Number(f64),
    Text(String),
}

pub type AudioParameterMap = BTreeMap<String, AudioParameterValue>;

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioParameterSpec {
    pub key: String,
    pub value_type: String,
    pub default: AudioParameterValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    #[serde(default)]
    pub allowed_values: Vec<AudioParameterValue>,
    pub advanced: bool,
    pub affects_quality: bool,
    pub affects_memory: bool,
    pub affects_cache: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default)]
    pub applicable_backends: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioModelFileStatus {
    pub role: String,
    pub filename: String,
    pub present: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<bool>,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

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
    pub files: Vec<AudioModelFileStatus>,
    pub catalog_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioModelCatalogSummary {
    pub schema_version: u32,
    pub catalog_version: String,
    pub models: Vec<AudioModelStatus>,
}

pub fn audio_processing_root(models_dir: impl AsRef<Path>) -> PathBuf {
    models_dir.as_ref().join("audio-processing")
}

pub fn audio_model_dir(models_dir: impl AsRef<Path>, model_id: &str) -> PathBuf {
    audio_processing_root(models_dir).join(model_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_ids_are_stable_and_unique() {
        let mut ids = REQUIRED_AUDIO_MODEL_IDS.to_vec();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), REQUIRED_AUDIO_MODEL_IDS.len());
        assert_eq!(AUDIO_CATALOG_VERSION, "2026.08.1");
    }
}
