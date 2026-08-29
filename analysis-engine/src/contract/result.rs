use std::path::{Component, PathBuf};

use serde::{Deserialize, Serialize};

use super::{AudioQualityReportV1, EngineError, EngineErrorCode, EngineResult};
use crate::quantization::QuantizationReportV1;

pub const ANALYSIS_RESULT_CONTRACT: &str = "uta.analysis-engine.result";
pub const ANALYSIS_RESULT_VERSION: u32 = 1;
pub const EXPORT_REQUEST_CONTRACT: &str = "uta.analysis-engine.export";
pub const EXPORT_REQUEST_VERSION: u32 = 1;
pub const FUSION_AGENT_ADAPTER_RESOURCE: &str = "tool:fusion_agent_adapter";
pub const FUSION_AGENT_PROTOCOL: &str = "uta.fusion_agent_request/uta.fusion_agent_response";
pub const FUSION_AGENT_PROTOCOL_VERSION: u32 = 3;
pub const HSMM_VITERBI_SELECTOR: &str = "hsmm_viterbi";

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
        if self.bytes == 0 || self.media_type.trim().is_empty() {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "artifact media type or byte count is invalid",
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
    pub technique_evidence: Option<ArtifactRefV1>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<QuantizationReportV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_quality: Option<AudioQualityReportV1>,
    #[serde(default)]
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisReusePolicyV1 {
    Deterministic,
    PreservedRevisionOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision_mode", rename_all = "snake_case")]
pub enum FusionDecisionProvenanceV1 {
    Algorithm {
        selector: String,
        selector_version: String,
        candidate_set_digest: String,
        selected_candidate_ids: Vec<String>,
        reuse_policy: AnalysisReusePolicyV1,
    },
    AiJudgment {
        adapter_resource: String,
        adapter_protocol: String,
        adapter_protocol_version: u32,
        adapter_identity: String,
        adapter_version: String,
        candidate_set_digest: String,
        selected_candidate_ids: Vec<String>,
        response_digest: String,
        reuse_policy: AnalysisReusePolicyV1,
    },
}

impl FusionDecisionProvenanceV1 {
    pub fn validate(&self) -> EngineResult<()> {
        let (candidate_set_digest, selected_candidate_ids) = match self {
            Self::Algorithm {
                selector,
                selector_version,
                candidate_set_digest,
                selected_candidate_ids,
                reuse_policy,
            } => {
                if selector != HSMM_VITERBI_SELECTOR
                    || selector_version != crate::fingerprint::HSMM_VERSION
                    || *reuse_policy != AnalysisReusePolicyV1::Deterministic
                {
                    return Err(EngineError::new(
                        EngineErrorCode::OutputValidationFailed,
                        "algorithmic fusion decision provenance is invalid",
                    ));
                }
                (candidate_set_digest, selected_candidate_ids)
            }
            Self::AiJudgment {
                adapter_resource,
                adapter_protocol,
                adapter_protocol_version,
                adapter_identity,
                adapter_version,
                candidate_set_digest,
                selected_candidate_ids,
                response_digest,
                reuse_policy,
            } => {
                if adapter_resource != FUSION_AGENT_ADAPTER_RESOURCE
                    || adapter_protocol != FUSION_AGENT_PROTOCOL
                    || *adapter_protocol_version != FUSION_AGENT_PROTOCOL_VERSION
                    || adapter_identity.trim().is_empty()
                    || adapter_version.trim().is_empty()
                    || !valid_sha256(response_digest)
                    || *reuse_policy != AnalysisReusePolicyV1::PreservedRevisionOnly
                {
                    return Err(EngineError::new(
                        EngineErrorCode::OutputValidationFailed,
                        "AI judgment fusion decision provenance is invalid",
                    ));
                }
                (candidate_set_digest, selected_candidate_ids)
            }
        };
        let mut unique_ids = std::collections::BTreeSet::new();
        if !valid_sha256(candidate_set_digest)
            || selected_candidate_ids.is_empty()
            || selected_candidate_ids
                .iter()
                .any(|id| id.trim().is_empty() || !unique_ids.insert(id))
        {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "fusion decision candidate identity is invalid",
            ));
        }
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AnalysisProvenanceV1 {
    #[serde(default)]
    pub resources: Vec<ResolvedResourceProvenanceV1>,
    pub calibration_version: String,
    pub fusion_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fusion_decision: Option<FusionDecisionProvenanceV1>,
    pub quantization_version: String,
    pub audio_quality_version: String,
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
                self.artifacts.technique_evidence.as_ref(),
                "application/vnd.uta.technique-evidence+json;version=1",
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
        if let Some(decision) = &self.provenance.fusion_decision {
            decision.validate()?;
        }
        if (self.artifacts.candidate_vocal_chart.is_some()
            || self.artifacts.singing_analysis.is_some())
            && self.provenance.fusion_decision.is_none()
        {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "singing artifacts require final fusion decision provenance",
            ));
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
        if let Some(report) = &self.diagnostics.quantization {
            report.validate()?;
            if self.artifacts.candidate_vocal_chart.is_none()
                || self.provenance.quantization_version != report.algorithm
            {
                return Err(EngineError::new(
                    EngineErrorCode::OutputValidationFailed,
                    "quantization diagnostics require a matching Candidate VocalChart and provenance",
                ));
            }
        }
        if let Some(report) = &self.diagnostics.audio_quality {
            report.validate()?;
            if self.provenance.audio_quality_version != report.algorithm {
                return Err(EngineError::new(
                    EngineErrorCode::OutputValidationFailed,
                    "audio quality diagnostics require matching algorithm provenance",
                ));
            }
        } else if !self.provenance.audio_quality_version.is_empty() {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "audio quality provenance requires a typed diagnostics report",
            ));
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
        if self.request_id.is_empty() || self.fingerprint.trim().is_empty() {
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
    fn ai_result_manifest_serializes_truthful_decision_provenance_without_hsmm() {
        let manifest = AnalysisResultManifestV1 {
            contract: ANALYSIS_RESULT_CONTRACT.to_string(),
            version: ANALYSIS_RESULT_VERSION,
            request_id: "ai-result-1".to_string(),
            status: AnalysisStatus::Failed,
            artifacts: AnalysisArtifactsV1::default(),
            diagnostics: AnalysisDiagnosticsV1::default(),
            provenance: AnalysisProvenanceV1 {
                fusion_decision: Some(FusionDecisionProvenanceV1::AiJudgment {
                    adapter_resource: FUSION_AGENT_ADAPTER_RESOURCE.to_string(),
                    adapter_protocol: FUSION_AGENT_PROTOCOL.to_string(),
                    adapter_protocol_version: FUSION_AGENT_PROTOCOL_VERSION,
                    adapter_identity: "uta-test-adapter".to_string(),
                    adapter_version: "1.0.0".to_string(),
                    candidate_set_digest: "a".repeat(64),
                    selected_candidate_ids: vec!["candidate-1".to_string()],
                    response_digest: "b".repeat(64),
                    reuse_policy: AnalysisReusePolicyV1::PreservedRevisionOnly,
                }),
                ..AnalysisProvenanceV1::default()
            },
            fingerprint: "c".repeat(64),
            degraded_reasons: Vec::new(),
        };
        manifest.validate().unwrap();
        let json = serde_json::to_value(manifest).unwrap();
        assert_eq!(
            json["provenance"]["fusion_decision"]["decision_mode"],
            "ai_judgment"
        );
        assert!(json["provenance"].get("hsmm_version").is_none());
    }

    #[test]
    fn fusion_decision_provenance_requires_exact_selector_and_adapter_protocol_versions() {
        let algorithm = |selector_version: &str| FusionDecisionProvenanceV1::Algorithm {
            selector: HSMM_VITERBI_SELECTOR.to_string(),
            selector_version: selector_version.to_string(),
            candidate_set_digest: "a".repeat(64),
            selected_candidate_ids: vec!["candidate-1".to_string()],
            reuse_policy: AnalysisReusePolicyV1::Deterministic,
        };
        algorithm(crate::fingerprint::HSMM_VERSION)
            .validate()
            .unwrap();
        assert_eq!(
            algorithm("hsmm-v13").validate().unwrap_err().code,
            EngineErrorCode::OutputValidationFailed
        );

        let ai = |version| FusionDecisionProvenanceV1::AiJudgment {
            adapter_resource: FUSION_AGENT_ADAPTER_RESOURCE.to_string(),
            adapter_protocol: FUSION_AGENT_PROTOCOL.to_string(),
            adapter_protocol_version: version,
            adapter_identity: "uta-test-adapter".to_string(),
            adapter_version: "1.0.0".to_string(),
            candidate_set_digest: "a".repeat(64),
            selected_candidate_ids: vec!["candidate-1".to_string()],
            response_digest: "b".repeat(64),
            reuse_policy: AnalysisReusePolicyV1::PreservedRevisionOnly,
        };
        ai(FUSION_AGENT_PROTOCOL_VERSION).validate().unwrap();
        assert_eq!(
            ai(FUSION_AGENT_PROTOCOL_VERSION - 1)
                .validate()
                .unwrap_err()
                .code,
            EngineErrorCode::OutputValidationFailed
        );
    }

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
