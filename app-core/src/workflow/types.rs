use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const WORKFLOW_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowNodeId(pub String);

impl WorkflowNodeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorkflowNodeId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityId(pub String);

impl CapabilityId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CapabilityId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRole {
    SourceMix,
    Vocal,
    LeadVocal,
    BackVocal,
    Instrumental,
    Drums,
    Bass,
    Guitar,
    Piano,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioArtifactDescriptor {
    pub role: AudioRole,
    pub sample_rate: u32,
    pub channels: u16,
    #[serde(default)]
    pub processing_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_node: Option<WorkflowNodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "role", rename_all = "snake_case")]
pub enum WorkflowPortType {
    Audio(AudioRole),
    Lyrics,
    TranscriptEvidence,
    AlignmentEvidence,
    PitchEvidence,
    BoundaryEvidence,
    TechniqueEvidence,
    AcousticEvidence,
    EvidenceBundle,
    CandidateGraph,
    CanonicalSingingTrack,
    CandidateChart,
}

impl WorkflowPortType {
    pub fn accepts(&self, produced: &Self) -> bool {
        self == produced
            || matches!(
                (self, produced),
                (
                    Self::Audio(AudioRole::Vocal),
                    Self::Audio(AudioRole::LeadVocal | AudioRole::BackVocal)
                )
            )
    }

    pub fn is_audio(&self) -> bool {
        matches!(self, Self::Audio(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPortSpec {
    pub id: String,
    pub port_type: WorkflowPortType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub multiple: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPortRef {
    pub node: WorkflowNodeId,
    pub port: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub from: WorkflowPortRef,
    pub to: WorkflowPortRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerBinding {
    pub analyzer_node: WorkflowNodeId,
    pub source: WorkflowPortRef,
    #[serde(default = "default_analyzer_input")]
    pub analyzer_input: String,
}

fn default_analyzer_input() -> String {
    "audio".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ExecutionPolicy {
    #[default]
    Always,
    Conditional {
        condition: ConditionalExecution,
    },
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionalExecution {
    OnDisagreement,
    DisagreementWindows,
    MaximumOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowNodeInstance {
    pub instance_id: WorkflowNodeId,
    pub capability_id: CapabilityId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub execution_policy: ExecutionPolicy,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct NodePosition {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkflowLayout {
    #[serde(default)]
    pub positions: BTreeMap<WorkflowNodeId, NodePosition>,
    #[serde(default)]
    pub zoom: f32,
    #[serde(default)]
    pub pan_x: f32,
    #[serde(default)]
    pub pan_y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QualityMode {
    Fast,
    #[default]
    Balanced,
    Maximum,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    #[serde(default = "workflow_schema_version")]
    pub schema_version: u32,
    pub workflow_id: WorkflowId,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub quality_mode: QualityMode,
    #[serde(default)]
    pub preset_id: Option<String>,
    #[serde(default)]
    pub nodes: Vec<WorkflowNodeInstance>,
    #[serde(default)]
    pub edges: Vec<WorkflowEdge>,
    #[serde(default)]
    pub analyzer_bindings: Vec<AnalyzerBinding>,
}

const fn workflow_schema_version() -> u32 {
    WORKFLOW_SCHEMA_VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredWorkflow {
    pub definition: WorkflowDefinition,
    #[serde(default)]
    pub layout: WorkflowLayout,
    pub updated_at_ms: i64,
}
