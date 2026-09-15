use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::runtime_wire::{RuntimePolicyWire, RuntimeResourceStatusWire};

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
pub struct AnalysisWorkerReady {
    #[serde(rename = "type")]
    pub frame_type: String,
    pub protocol: u32,
    pub protocol_identity: String,
    pub component: String,
    pub engine_version: String,
    pub contract_versions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisLifecycleFrameWire {
    #[serde(rename = "type")]
    pub frame_type: String,
    pub schema_version: u32,
    pub request_id: String,
    pub node_id: String,
    #[serde(default)]
    pub presentation_node_id: Option<String>,
    pub capability_id: String,
    #[serde(default)]
    pub model_id: Option<String>,
    pub implementation: String,
    /// The resolved native backend actually dispatched for this node (e.g.
    /// `"ggml_vulkan"`, `"ggml_cpu"`, `"libtorch_xpu"`). `implementation`
    /// alone cannot distinguish these -- see `analysis-engine`'s
    /// `EngineLifecycleEvent::backend` doc comment for why this exists.
    #[serde(default)]
    pub backend: Option<String>,
    /// The GGML device class actually requested for this node (e.g. `"gpu"`,
    /// `"integrated_gpu"`, `"cpu"`) -- see `analysis-engine`'s
    /// `EngineLifecycleEvent::device_class` doc comment for why this exists.
    #[serde(default)]
    pub device_class: Option<String>,
    #[serde(default)]
    pub progress: Option<f32>,
    #[serde(default)]
    pub work_units_completed: Option<u64>,
    #[serde(default)]
    pub work_units_total: Option<u64>,
    #[serde(default)]
    pub worker_task_id: Option<String>,
    #[serde(default)]
    pub artifact: Option<String>,
    /// Present only alongside `artifact`, and only when the Engine
    /// considered it a real file the caller might reuse on a future run
    /// (see `analysis-engine`'s `LifecycleNodeGuard::artifact_with_path`).
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    pub event_at_ms: i64,
}

impl AnalysisLifecycleFrameWire {
    pub fn is_lifecycle_type(frame_type: &str) -> bool {
        matches!(
            frame_type,
            "node_started"
                | "node_progress"
                | "node_completed"
                | "node_failed"
                | "artifact"
                | "warning"
                | "degraded"
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRoleWire {
    #[default]
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
pub enum AudioSourceKindWire {
    LocalFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTimelineWire {
    pub timebase: u32,
    pub source_start: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSourceWire {
    pub id: String,
    pub kind: AudioSourceKindWire,
    pub path: PathBuf,
    pub sha256: String,
    pub role: AudioRoleWire,
    pub primary: bool,
    pub timeline: SourceTimelineWire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricsModeWire {
    None,
    Reference,
    Canonical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricTokenWire {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phonemes: Option<Vec<String>>,
    /// This token's known real-audio time range (`CANONICAL_TIMEBASE`
    /// units), when one exists -- e.g. a Timed LRC line's own stamped span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricsWire {
    pub mode: LyricsModeWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub tokens: Vec<LyricTokenWire>,
}

impl Default for LyricsWire {
    fn default() -> Self {
        Self {
            mode: LyricsModeWire::None,
            language: None,
            tokens: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryLevelWire {
    Phrase,
    Word,
    Syllable,
    Phoneme,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryAuthorityWire {
    #[default]
    Soft,
    Hard,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryConstraintWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_id: Option<String>,
    pub level: BoundaryLevelWire,
    pub start: u64,
    pub duration: u64,
    pub confidence: f32,
    #[serde(default)]
    pub authority: BoundaryAuthorityWire,
    pub source: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextAuthorityWire {
    #[default]
    Hint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignatureWire {
    pub beats: u16,
    pub unit: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantizationGridWire {
    Eighth,
    Sixteenth,
    ThirtySecond,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MusicalContextWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_signature: Option<TimeSignatureWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization_grid: Option<QuantizationGridWire>,
    #[serde(default)]
    pub authority: ContextAuthorityWire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisProfileWire {
    Fast,
    Balanced,
    Maximum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackTargetWire {
    Lead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisSpecWire {
    pub profile: AnalysisProfileWire,
    pub track_target: TrackTargetWire,
    pub preserve_continuous_pitch: bool,
    pub enable_quantization: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedArtifactsWire {
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
    pub stems: Vec<AudioRoleWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPolicyWire {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_settings: uta_model_settings::ModelSettings,
    /// Complete-model automatic GPU placement. Manual route fields are absent
    /// while enabled and return from persisted settings when disabled.
    #[serde(default)]
    pub turbo_acceleration: bool,
    #[serde(default)]
    pub runtime_policy: RuntimePolicyWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_backend: Option<super::NativeBackendWire>,
    /// Model-specific choices take precedence over the global selection in
    /// ordinary mode. Missing entries retain Runtime Manager's pinned route.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_backend_overrides: BTreeMap<String, super::NativeBackendWire>,
    /// Global device-class preference, orthogonal to `requested_backend`.
    /// Forwarded by the Engine to the GGML worker, which selects a matching
    /// physical device or fails without CPU fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_device: Option<super::DeviceClassWire>,
    /// Model-specific device-class choices, same precedence as
    /// `model_backend_overrides`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_device_overrides: BTreeMap<String, super::DeviceClassWire>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeRequestWire {
    pub contract: String,
    pub version: u32,
    pub request_id: String,
    pub audio_sources: Vec<AudioSourceWire>,
    #[serde(default)]
    pub lyrics: LyricsWire,
    #[serde(default)]
    pub boundary_constraints: Vec<BoundaryConstraintWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub musical_context: Option<MusicalContextWire>,
    pub analysis: AnalysisSpecWire,
    pub requested_artifacts: RequestedArtifactsWire,
    #[serde(default = "production_execution_policy")]
    pub execution_policy: ExecutionPolicyWire,
    #[serde(default)]
    pub satisfied_capabilities: Vec<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

fn production_execution_policy() -> ExecutionPolicyWire {
    ExecutionPolicyWire {
        model_settings: BTreeMap::new(),
        runtime_policy: RuntimePolicyWire::Production,
        turbo_acceleration: false,
        requested_backend: None,
        model_backend_overrides: BTreeMap::new(),
        requested_device: None,
        model_device_overrides: BTreeMap::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDescriptorWire {
    pub id: CapabilityIdWire,
    #[serde(default)]
    pub input_semantic_types: Vec<String>,
    #[serde(default)]
    pub output_semantic_types: Vec<String>,
    pub baseline_required: bool,
    pub implementation_exists: bool,
    pub runtime_policy_satisfied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRequirementResourceWire {
    pub resource: String,
    pub required: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRequirementsWire {
    pub schema: String,
    pub schema_version: u32,
    pub resources: Vec<AnalysisRequirementResourceWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityIdWire(pub String);

impl CapabilityIdWire {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for CapabilityIdWire {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRouteWire {
    pub primary_source_id: String,
    pub input_role: AudioRoleWire,
    pub preparation: Vec<CapabilityIdWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionNodeWire {
    pub id: String,
    pub capability: CapabilityIdWire,
    pub required: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPlanIdentityWire {
    pub contract: String,
    pub version: u32,
    pub workflow_schema_version: u32,
    pub workflow_id: String,
    pub workflow_revision: u64,
    pub definition_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeExecutionStateWire {
    Ready,
    Deferred,
    Disabled,
    ProfileSkipped,
    NotRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuousF0SourceWire {
    Rmvpe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteLengthSourceWire {
    F0Derived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnsetSupportSourceWire {
    Automatic,
    Acoustic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpertFusionPolicyWire {
    pub continuous_f0: ContinuousF0SourceWire,
    pub note_lengths: NoteLengthSourceWire,
    pub onset_support: OnsetSupportSourceWire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FusionModeWire {
    #[default]
    Algorithm,
    AiJudgment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionNodePlanWire {
    pub instance_id: String,
    pub analysis_node: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub execution_policy: String,
    pub execution_state: WorkflowNodeExecutionStateWire,
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_invocations: Vec<crate::workflow::WorkflowExecutionInvocationWire>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub input_bindings: Vec<crate::workflow::WorkflowBindingWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionPlanWire {
    pub identity: WorkflowPlanIdentityWire,
    #[serde(default)]
    pub nodes: Vec<WorkflowExecutionNodePlanWire>,
    #[serde(default)]
    pub terminal_outputs: Vec<crate::workflow::WorkflowTerminalOutputWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fusion_policy: Option<ExpertFusionPolicyWire>,
    /// Required in an exact Engine Plan. A missing backend field must fail
    /// decoding rather than silently project AI judgment as Algorithm.
    pub fusion_mode: FusionModeWire,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedResourceStatusWire {
    pub requirement: AnalysisRequirementResourceWire,
    #[serde(default)]
    pub status: Option<RuntimeResourceStatusWire>,
    #[serde(default)]
    pub resolution_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackRuleWire {
    pub capability: CapabilityIdWire,
    pub behavior: String,
    pub fingerprinted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDeclarationWire {
    pub semantic_type: String,
    pub required: bool,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisPlanWire {
    pub schema: String,
    pub schema_version: u32,
    pub request_id: String,
    pub source_route: SourceRouteWire,
    pub requested_outputs: Vec<String>,
    pub required_capabilities: Vec<CapabilityIdWire>,
    pub optional_capabilities: Vec<CapabilityIdWire>,
    pub requirements: AnalysisRequirementsWire,
    pub resolved_resources: Vec<PlannedResourceStatusWire>,
    pub execution_nodes: Vec<ExecutionNodeWire>,
    pub quality_gates: Vec<String>,
    pub fallback_policy: Vec<FallbackRuleWire>,
    pub artifact_declarations: Vec<ArtifactDeclarationWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_execution: Option<WorkflowExecutionPlanWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisErrorWire {
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
pub enum AnalysisStatusWire {
    Ok,
    OkDegraded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRefWire {
    pub path: PathBuf,
    pub media_type: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StemArtifactRefWire {
    pub role: AudioRoleWire,
    pub artifact: ArtifactRefWire,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisArtifactsWire {
    #[serde(default)]
    pub candidate_vocal_chart: Option<ArtifactRefWire>,
    #[serde(default)]
    pub pitch_evidence: Option<ArtifactRefWire>,
    #[serde(default)]
    pub technique_evidence: Option<ArtifactRefWire>,
    #[serde(default)]
    pub singing_analysis: Option<ArtifactRefWire>,
    #[serde(default)]
    pub transcript: Option<ArtifactRefWire>,
    #[serde(default)]
    pub alignment: Option<ArtifactRefWire>,
    #[serde(default)]
    pub stems: Vec<StemArtifactRefWire>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuantizationReportWire {
    pub algorithm: String,
    pub bpm: f64,
    pub grid: QuantizationGridWire,
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
pub enum QualityGateRequirementWire {
    Required,
    Degrading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityGateStatusWire {
    Passed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityMetricWire {
    pub name: String,
    pub value: f64,
    pub unit: String,
    #[serde(default)]
    pub lower_bound: Option<f64>,
    #[serde(default)]
    pub upper_bound: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityRegionWire {
    pub start: u64,
    pub end: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityGateOutcomeWire {
    pub gate: String,
    pub requirement: QualityGateRequirementWire,
    pub status: QualityGateStatusWire,
    pub summary: String,
    #[serde(default)]
    pub metrics: Vec<QualityMetricWire>,
    #[serde(default)]
    pub regions: Vec<QualityRegionWire>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VocalTopologyModeWire {
    SingleLead,
    AlternatingMultiLead,
    OverlappingMultiLead,
    LeadWithSupport,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VocalTopologyEstimateWire {
    pub contract: String,
    pub version: u32,
    pub timebase: u32,
    pub source_start: u64,
    pub duration: u64,
    pub mode: VocalTopologyModeWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub overlap_regions: Vec<QualityRegionWire>,
    #[serde(default)]
    pub support_regions: Vec<QualityRegionWire>,
    pub evidence_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioQualityReportWire {
    pub contract: String,
    pub version: u32,
    pub algorithm: String,
    pub profile: AnalysisProfileWire,
    pub evaluated_audio_role: String,
    pub duration: u64,
    pub planned_gates: Vec<String>,
    pub outcomes: Vec<QualityGateOutcomeWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocal_topology: Option<VocalTopologyEstimateWire>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnalysisDiagnosticsWire {
    #[serde(default)]
    pub decoded_audio: Vec<serde_json::Value>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub quantization: Option<QuantizationReportWire>,
    #[serde(default)]
    pub audio_quality: Option<AudioQualityReportWire>,
    #[serde(default)]
    pub separation_quality: Vec<super::SeparationQualityEvidenceWire>,
    #[serde(default)]
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisReusePolicyWire {
    Deterministic,
    PreservedRevisionOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision_mode", rename_all = "snake_case")]
pub enum FusionDecisionProvenanceWire {
    Algorithm {
        selector: String,
        selector_version: String,
        candidate_set_digest: String,
        selected_candidate_ids: Vec<String>,
        reuse_policy: AnalysisReusePolicyWire,
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
        reuse_policy: AnalysisReusePolicyWire,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisProvenanceWire {
    #[serde(default)]
    pub resources: Vec<serde_json::Value>,
    pub calibration_version: String,
    pub fusion_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fusion_decision: Option<FusionDecisionProvenanceWire>,
    pub quantization_version: String,
    #[serde(default)]
    pub audio_quality_version: String,
    pub postprocess_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisResultManifestWire {
    pub contract: String,
    pub version: u32,
    pub request_id: String,
    pub status: AnalysisStatusWire,
    pub artifacts: AnalysisArtifactsWire,
    #[serde(default)]
    pub diagnostics: AnalysisDiagnosticsWire,
    pub provenance: AnalysisProvenanceWire,
    pub fingerprint: String,
    #[serde(default)]
    pub degraded_reasons: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow_plan_json() -> serde_json::Value {
        serde_json::json!({
            "identity": {
                "contract": "uta.workflow-execution",
                "version": 1,
                "workflow_schema_version": crate::workflow::WORKFLOW_SCHEMA_VERSION,
                "workflow_id": "workflow:test",
                "workflow_revision": 1,
                "definition_digest": "digest"
            },
            "nodes": [],
            "terminal_outputs": []
        })
    }

    #[test]
    fn exact_workflow_plan_requires_a_fusion_mode_field() {
        let error =
            serde_json::from_value::<WorkflowExecutionPlanWire>(workflow_plan_json()).unwrap_err();
        assert!(error.to_string().contains("missing field `fusion_mode`"));
    }

    #[test]
    fn exact_workflow_plan_decodes_typed_provider_invocations() {
        let mut value = workflow_plan_json();
        value["nodes"] = serde_json::json!([{
            "instance_id": "split",
            "analysis_node": "split",
            "capabilities": ["audio.extract_vocals", "audio.extract_instrumental"],
            "execution_policy": "always",
            "execution_state": "ready",
            "priority": 10,
            "execution_invocations": [{
                "invocation_id": "split.dual",
                "provider_id": "dual-output-provider",
                "capabilities": ["audio.extract_vocals", "audio.extract_instrumental"],
                "output_ports": ["vocal", "instrumental"]
            }],
            "depends_on": [],
            "input_bindings": []
        }]);
        value["fusion_mode"] = serde_json::json!("algorithm");
        let plan = serde_json::from_value::<WorkflowExecutionPlanWire>(value).unwrap();
        assert_eq!(plan.nodes[0].execution_invocations.len(), 1);
        assert_eq!(
            plan.nodes[0].execution_invocations[0].invocation_id,
            "split.dual"
        );
    }

    #[test]
    fn exact_workflow_plan_decodes_explicit_ai_judgment() {
        let mut value = workflow_plan_json();
        value["fusion_mode"] = serde_json::json!("ai_judgment");
        let plan = serde_json::from_value::<WorkflowExecutionPlanWire>(value).unwrap();
        assert_eq!(plan.fusion_mode, FusionModeWire::AiJudgment);
    }
}
