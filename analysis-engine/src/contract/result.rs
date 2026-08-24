use std::path::{Component, PathBuf};

use serde::{Deserialize, Serialize};

use super::{EngineError, EngineErrorCode, EngineResult};

pub const ANALYSIS_RESULT_CONTRACT: &str = "uta.analysis-engine.result";
pub const ANALYSIS_RESULT_VERSION: u32 = 1;
pub const EXPORT_REQUEST_CONTRACT: &str = "uta.analysis-engine.export";
pub const EXPORT_REQUEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatus {
    Ok,
    OkDegraded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRefV1 {
    pub path: PathBuf,
    pub media_type: String,
    pub sha256: String,
    pub bytes: u64,
}

impl ArtifactRefV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.path.is_absolute()
            || self.path.as_os_str().is_empty()
            || self.path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "artifact path must be a confined relative path",
            ));
        }
        if self.bytes == 0
            || self.media_type.trim().is_empty()
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "artifact media type or SHA-256 is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AnalysisArtifactsV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_vocal_chart: Option<ArtifactRefV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch_evidence: Option<ArtifactRefV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub singing_analysis: Option<ArtifactRefV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<ArtifactRefV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<ArtifactRefV1>,
    #[serde(default)]
    pub stems: Vec<StemArtifactRefV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StemArtifactRefV1 {
    pub role: super::AudioRole,
    pub artifact: ArtifactRefV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecodedAudioFactsV1 {
    pub source_id: String,
    pub container: String,
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub frame_count: u64,
    pub duration: super::CanonicalDuration,
    pub peak: f32,
    pub decode_backend: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AnalysisDiagnosticsV1 {
    #[serde(default)]
    pub decoded_audio: Vec<DecodedAudioFactsV1>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AnalysisProvenanceV1 {
    #[serde(default)]
    pub resources: Vec<ResolvedResourceProvenanceV1>,
    pub calibration_version: String,
    pub fusion_version: String,
    pub hsmm_version: String,
    pub quantization_version: String,
    pub postprocess_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedResourceProvenanceV1 {
    pub resource: String,
    pub generation: String,
    pub content_digest: String,
    pub runtime: String,
    pub runtime_generation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
    pub backend: String,
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisResultManifestV1 {
    pub contract: String,
    pub version: u32,
    pub request_id: String,
    pub status: AnalysisStatus,
    pub artifacts: AnalysisArtifactsV1,
    pub diagnostics: AnalysisDiagnosticsV1,
    pub provenance: AnalysisProvenanceV1,
    pub fingerprint: String,
    #[serde(default)]
    pub degraded_reasons: Vec<String>,
}

impl AnalysisResultManifestV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != ANALYSIS_RESULT_CONTRACT || self.version != ANALYSIS_RESULT_VERSION {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "analysis result contract identity is invalid",
            ));
        }
        for (artifact, expected_media_type) in [
            (
                self.artifacts.candidate_vocal_chart.as_ref(),
                "application/vnd.uta.vocal-chart+json;version=0.3",
            ),
            (
                self.artifacts.pitch_evidence.as_ref(),
                "application/vnd.uta.pitch-evidence+json;version=0.3",
            ),
            (
                self.artifacts.singing_analysis.as_ref(),
                "application/vnd.uta.singing-analysis+json;version=0.3",
            ),
            (
                self.artifacts.transcript.as_ref(),
                "application/vnd.uta.transcript+json;version=1",
            ),
            (
                self.artifacts.alignment.as_ref(),
                "application/vnd.uta.alignment+json;version=1",
            ),
        ] {
            if let Some(artifact) = artifact {
                artifact.validate()?;
                if artifact.media_type != expected_media_type {
                    return Err(EngineError::new(
                        EngineErrorCode::OutputValidationFailed,
                        "typed artifact media type does not match its result field",
                    ));
                }
            }
        }
        let mut stem_roles = std::collections::BTreeSet::new();
        for stem in &self.artifacts.stems {
            if !matches!(
                stem.role,
                super::AudioRole::Instrumental
                    | super::AudioRole::GuideVocals
                    | super::AudioRole::LeadVocal
                    | super::AudioRole::BackingVocal
                    | super::AudioRole::HarmonyVocal
            ) || !stem_roles.insert(stem.role)
            {
                return Err(EngineError::new(
                    EngineErrorCode::OutputValidationFailed,
                    "result stems contain an invalid or duplicate semantic role",
                ));
            }
            stem.artifact.validate()?;
        }
        for resource in &self.provenance.resources {
            if resource.resource.trim().is_empty()
                || resource.generation.trim().is_empty()
                || resource.content_digest.trim().is_empty()
                || resource.runtime.trim().is_empty()
                || resource.runtime_generation.trim().is_empty()
                || resource.backend.trim().is_empty()
                || resource.device.trim().is_empty()
            {
                return Err(EngineError::new(
                    EngineErrorCode::OutputValidationFailed,
                    "resolved resource provenance is incomplete",
                ));
            }
        }
        for facts in &self.diagnostics.decoded_audio {
            if facts.source_id.is_empty()
                || facts.container.trim().is_empty()
                || facts.codec.trim().is_empty()
                || facts.sample_rate == 0
                || facts.channels == 0
                || facts.frame_count == 0
                || facts.duration == 0
                || !facts.peak.is_finite()
                || facts.peak < 0.0
                || facts.decode_backend.trim().is_empty()
            {
                return Err(EngineError::new(
                    EngineErrorCode::OutputValidationFailed,
                    "decoded audio facts are invalid",
                ));
            }
        }
        if self.request_id.is_empty()
            || self.fingerprint.len() != 64
            || !self
                .fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "result request identity or fingerprint is invalid",
            ));
        }
        match self.status {
            AnalysisStatus::Ok if !self.degraded_reasons.is_empty() => {
                return Err(EngineError::new(
                    EngineErrorCode::OutputValidationFailed,
                    "ok results cannot contain degraded reasons",
                ));
            }
            AnalysisStatus::OkDegraded if self.degraded_reasons.is_empty() => {
                return Err(EngineError::new(
                    EngineErrorCode::OutputValidationFailed,
                    "ok_degraded results must state at least one degraded reason",
                ));
            }
            AnalysisStatus::Ok | AnalysisStatus::OkDegraded => {
                if [
                    &self.provenance.calibration_version,
                    &self.provenance.fusion_version,
                    &self.provenance.hsmm_version,
                    &self.provenance.quantization_version,
                    &self.provenance.postprocess_version,
                ]
                .into_iter()
                .any(|version| version.trim().is_empty())
                {
                    return Err(EngineError::new(
                        EngineErrorCode::OutputValidationFailed,
                        "successful results require complete algorithm provenance",
                    ));
                }
            }
            AnalysisStatus::Failed | AnalysisStatus::Cancelled => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Utz,
    Midi,
    Ustx,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRequestV1 {
    pub contract: String,
    pub version: u32,
    pub request_id: String,
    pub format: ExportFormat,
    pub result_manifest: PathBuf,
    pub output: PathBuf,
    #[serde(default)]
    pub overwrite: bool,
}

impl ExportRequestV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != EXPORT_REQUEST_CONTRACT || self.version != EXPORT_REQUEST_VERSION {
            return Err(EngineError::new(
                EngineErrorCode::UnsupportedContractVersion,
                "unsupported export request contract",
            ));
        }
        let expected = match self.format {
            ExportFormat::Utz => "utz",
            ExportFormat::Midi => "mid",
            ExportFormat::Ustx => "ustx",
        };
        if self
            .output
            .extension()
            .and_then(|extension| extension.to_str())
            .is_none_or(|extension| !extension.eq_ignore_ascii_case(expected))
        {
            return Err(EngineError::new(
                EngineErrorCode::ExportFailed,
                format!("export target must use .{expected}"),
            ));
        }
        if self.output.exists() && !self.overwrite {
            return Err(EngineError::new(
                EngineErrorCode::ExportFailed,
                "export target exists and overwrite was not authorized",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_references_are_confined() {
        let artifact = ArtifactRefV1 {
            path: PathBuf::from("candidate/chart.json"),
            media_type: "application/json".to_string(),
            sha256: "a".repeat(64),
            bytes: 1,
        };
        artifact.validate().unwrap();
        let mut escaped = artifact;
        escaped.path = PathBuf::from("../chart.json");
        assert_eq!(
            escaped.validate().unwrap_err().code,
            EngineErrorCode::OutputValidationFailed
        );
    }
}
