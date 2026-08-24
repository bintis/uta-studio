use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::runtime_wire::{RuntimePolicyWireV1, RuntimeResourceStatusWireV1};

pub const ANALYSIS_WORKER_PROTOCOL_VERSION: u32 = 1;
pub const ANALYSIS_WORKER_IDENTITY: &str = "uta.analysis-engine.worker";
pub const ANALYSIS_COMPONENT: &str = "uta-analysis-engine";
pub const ANALYZE_REQUEST_CONTRACT: &str = "uta.analysis-engine.request";
pub const ANALYZE_REQUEST_VERSION: u32 = 1;
pub const ANALYSIS_RESULT_CONTRACT: &str = "uta.analysis-engine.result";
pub const ANALYSIS_RESULT_VERSION: u32 = 1;
pub const AUDIO_QUALITY_REPORT_CONTRACT: &str = "uta.analysis-engine.audio-quality-report";
pub const AUDIO_QUALITY_REPORT_VERSION: u32 = 1;
pub const CANONICAL_TIMEBASE: u32 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisWorkerReadyV1 {
    #[serde(rename = "type")]
    pub frame_type: String,
    pub protocol: u32,
    pub protocol_identity: String,
    pub component: String,
    pub engine_version: String,
    pub contract_versions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRoleWireV1 {
    OriginalMix,
    VocalStem,
    GuideVocals,
    LeadVocal,
    CleanLeadVocal,
    Instrumental,
    BackingVocal,
    HarmonyVocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSourceKindWireV1 {
    LocalFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTimelineWireV1 {
    pub timebase: u32,
    pub source_start: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSourceWireV1 {
    pub id: String,
    pub kind: AudioSourceKindWireV1,
    pub path: PathBuf,
    pub sha256: String,
    pub role: AudioRoleWireV1,
    pub primary: bool,
    pub timeline: SourceTimelineWireV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricsModeWireV1 {
    None,
    Reference,
    Canonical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricTokenWireV1 {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phonemes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricsWireV1 {
    pub mode: LyricsModeWireV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub tokens: Vec<LyricTokenWireV1>,
}

impl Default for LyricsWireV1 {
    fn default() -> Self {
        Self {
            mode: LyricsModeWireV1::None,
            language: None,
            tokens: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryLevelWireV1 {
    Phrase,
    Word,
    Syllable,
    Phoneme,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryAuthorityWireV1 {
    #[default]
    Soft,
    Hard,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryConstraintWireV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_id: Option<String>,
    pub level: BoundaryLevelWireV1,
    pub start: u64,
    pub duration: u64,
    pub confidence: f32,
    #[serde(default)]
    pub authority: BoundaryAuthorityWireV1,
    pub source: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextAuthorityWireV1 {
    #[default]
    Hint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignatureWireV1 {
    pub beats: u16,
    pub unit: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantizationGridWireV1 {
    Eighth,
    Sixteenth,
    ThirtySecond,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MusicalContextWireV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_signature: Option<TimeSignatureWireV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization_grid: Option<QuantizationGridWireV1>,
    #[serde(default)]
    pub authority: ContextAuthorityWireV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisProfileWireV1 {
    Fast,
    Balanced,
    Maximum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackTargetWireV1 {
    Lead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisSpecWireV1 {
    pub profile: AnalysisProfileWireV1,
    pub track_target: TrackTargetWireV1,
    pub preserve_continuous_pitch: bool,
    pub enable_quantization: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedArtifactsWireV1 {
    #[serde(default)]
    pub vocal_chart: bool,
    #[serde(default)]
    pub pitch_evidence: bool,
    #[serde(default)]
    pub singing_analysis: bool,
    #[serde(default)]
    pub transcript: bool,
    #[serde(default)]
    pub alignment: bool,
    #[serde(default)]
    pub stems: Vec<AudioRoleWireV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPolicyWireV1 {
    #[serde(default)]
    pub runtime_policy: RuntimePolicyWireV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_backend: Option<super::NativeBackendWireV1>,
    /// Model-specific choices take precedence over the global selection.
    /// Missing entries retain each model's Runtime Manager-pinned route.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_backend_overrides: BTreeMap<String, super::NativeBackendWireV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeRequestWireV1 {
    pub contract: String,
    pub version: u32,
    pub request_id: String,
    pub audio_sources: Vec<AudioSourceWireV1>,
    #[serde(default)]
    pub lyrics: LyricsWireV1,
    #[serde(default)]
    pub boundary_constraints: Vec<BoundaryConstraintWireV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub musical_context: Option<MusicalContextWireV1>,
    pub analysis: AnalysisSpecWireV1,
    pub requested_artifacts: RequestedArtifactsWireV1,
    #[serde(default = "testing_execution_policy")]
    pub execution_policy: ExecutionPolicyWireV1,
    #[serde(default)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

fn testing_execution_policy() -> ExecutionPolicyWireV1 {
    ExecutionPolicyWireV1 {
        runtime_policy: RuntimePolicyWireV1::Experimental,
        requested_backend: None,
        model_backend_overrides: BTreeMap::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDescriptorWireV1 {
    pub id: CapabilityIdWireV1,
    #[serde(default)]
    pub input_semantic_types: Vec<String>,
    #[serde(default)]
    pub output_semantic_types: Vec<String>,
    pub baseline_required: bool,
    pub implementation_exists: bool,
    pub runtime_policy_satisfied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRequirementResourceWireV1 {
    pub resource: String,
    pub required: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRequirementsWireV1 {
    pub schema: String,
    pub schema_version: u32,
    pub resources: Vec<AnalysisRequirementResourceWireV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityIdWireV1(pub String);

impl CapabilityIdWireV1 {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for CapabilityIdWireV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRouteWireV1 {
    pub primary_source_id: String,
    pub input_role: AudioRoleWireV1,
    pub preparation: Vec<CapabilityIdWireV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionNodeWireV1 {
    pub id: String,
    pub capability: CapabilityIdWireV1,
    pub required: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPlanIdentityWireV1 {
    pub contract: String,
    pub version: u32,
    pub workflow_schema_version: u32,
    pub workflow_id: String,
    pub workflow_revision: u64,
    pub definition_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeExecutionStateWireV1 {
    Ready,
    Deferred,
    Disabled,
    ProfileSkipped,
    NotRequested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionNodePlanWireV1 {
    pub instance_id: String,
    pub analysis_node: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub execution_policy: String,
    pub execution_state: WorkflowNodeExecutionStateWireV1,
    pub priority: i32,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub input_bindings: Vec<crate::workflow::WorkflowBindingWireV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionPlanWireV1 {
    pub identity: WorkflowPlanIdentityWireV1,
    #[serde(default)]
    pub nodes: Vec<WorkflowExecutionNodePlanWireV1>,
    #[serde(default)]
    pub terminal_outputs: Vec<crate::workflow::WorkflowTerminalOutputWireV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedResourceStatusWireV1 {
    pub requirement: AnalysisRequirementResourceWireV1,
    #[serde(default)]
    pub status: Option<RuntimeResourceStatusWireV1>,
    #[serde(default)]
    pub resolution_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackRuleWireV1 {
    pub capability: CapabilityIdWireV1,
    pub behavior: String,
    pub fingerprinted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDeclarationWireV1 {
    pub semantic_type: String,
    pub required: bool,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisPlanWireV1 {
    pub schema: String,
    pub schema_version: u32,
    pub request_id: String,
    pub source_route: SourceRouteWireV1,
    pub requested_outputs: Vec<String>,
    pub required_capabilities: Vec<CapabilityIdWireV1>,
    pub optional_capabilities: Vec<CapabilityIdWireV1>,
    pub requirements: AnalysisRequirementsWireV1,
    pub resolved_resources: Vec<PlannedResourceStatusWireV1>,
    pub execution_nodes: Vec<ExecutionNodeWireV1>,
    pub quality_gates: Vec<String>,
    pub fallback_policy: Vec<FallbackRuleWireV1>,
    pub artifact_declarations: Vec<ArtifactDeclarationWireV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_execution: Option<WorkflowExecutionPlanWireV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisErrorWireV1 {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub capability: Option<String>,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatusWireV1 {
    Ok,
    OkDegraded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRefWireV1 {
    pub path: PathBuf,
    pub media_type: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StemArtifactRefWireV1 {
    pub role: AudioRoleWireV1,
    pub artifact: ArtifactRefWireV1,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisArtifactsWireV1 {
    #[serde(default)]
    pub candidate_vocal_chart: Option<ArtifactRefWireV1>,
    #[serde(default)]
    pub pitch_evidence: Option<ArtifactRefWireV1>,
    #[serde(default)]
    pub technique_evidence: Option<ArtifactRefWireV1>,
    #[serde(default)]
    pub singing_analysis: Option<ArtifactRefWireV1>,
    #[serde(default)]
    pub transcript: Option<ArtifactRefWireV1>,
    #[serde(default)]
    pub alignment: Option<ArtifactRefWireV1>,
    #[serde(default)]
    pub stems: Vec<StemArtifactRefWireV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuantizationReportWireV1 {
    pub algorithm: String,
    pub bpm: f64,
    pub grid: QuantizationGridWireV1,
    pub grid_step: u64,
    pub minimum_note_duration: u64,
    pub source_start: u64,
    pub source_end: u64,
    pub hard_boundary_count: usize,
    pub note_count: usize,
    pub adjusted_notes: usize,
    pub maximum_shift: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityGateRequirementWireV1 {
    Required,
    Degrading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityGateStatusWireV1 {
    Passed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityMetricWireV1 {
    pub name: String,
    pub value: f64,
    pub unit: String,
    #[serde(default)]
    pub lower_bound: Option<f64>,
    #[serde(default)]
    pub upper_bound: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityRegionWireV1 {
    pub start: u64,
    pub end: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityGateOutcomeWireV1 {
    pub gate: String,
    pub requirement: QualityGateRequirementWireV1,
    pub status: QualityGateStatusWireV1,
    pub summary: String,
    #[serde(default)]
    pub metrics: Vec<QualityMetricWireV1>,
    #[serde(default)]
    pub regions: Vec<QualityRegionWireV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioQualityReportWireV1 {
    pub contract: String,
    pub version: u32,
    pub algorithm: String,
    pub profile: AnalysisProfileWireV1,
    pub evaluated_audio_role: String,
    pub duration: u64,
    pub planned_gates: Vec<String>,
    pub outcomes: Vec<QualityGateOutcomeWireV1>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnalysisDiagnosticsWireV1 {
    #[serde(default)]
    pub decoded_audio: Vec<serde_json::Value>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub quantization: Option<QuantizationReportWireV1>,
    #[serde(default)]
    pub audio_quality: Option<AudioQualityReportWireV1>,
    #[serde(default)]
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisProvenanceWireV1 {
    #[serde(default)]
    pub resources: Vec<serde_json::Value>,
    pub calibration_version: String,
    pub fusion_version: String,
    pub hsmm_version: String,
    pub quantization_version: String,
    #[serde(default)]
    pub audio_quality_version: String,
    pub postprocess_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisResultManifestWireV1 {
    pub contract: String,
    pub version: u32,
    pub request_id: String,
    pub status: AnalysisStatusWireV1,
    pub artifacts: AnalysisArtifactsWireV1,
    #[serde(default)]
    pub diagnostics: AnalysisDiagnosticsWireV1,
    pub provenance: AnalysisProvenanceWireV1,
    pub fingerprint: String,
    #[serde(default)]
    pub degraded_reasons: Vec<String>,
}
