use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::analysis_graph::{AnalysisGraphSpec, AnalysisNodeId};

use super::{
    CapabilityId, ExecutionPolicy, QualityMode, WorkflowDefinition, WorkflowNodeId,
    WorkflowPortType,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedRuntimeKind {
    OpenVino,
    Vulkan,
    NativeDsp,
    PinnedQwenAsrVulkan,
    PinnedQwenAlignVulkan,
    #[default]
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledNodeBinding {
    pub workflow_node: WorkflowNodeId,
    pub capability_id: CapabilityId,
    pub analysis_node: AnalysisNodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default)]
    pub execution_policy: ExecutionPolicy,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub runtime: ResolvedRuntimeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
}

const fn enabled_binding() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledArtifactBinding {
    pub from_node: AnalysisNodeId,
    pub from_port: String,
    pub to_node: AnalysisNodeId,
    pub to_port: String,
    pub port_type: WorkflowPortType,
    #[serde(default = "enabled_binding")]
    pub execution_active: bool,
    #[serde(default)]
    pub analyzer_attachment: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecutionSnapshot {
    pub schema_version: u32,
    pub workflow_id: String,
    pub workflow_revision: u64,
    #[serde(default)]
    pub quality_mode: QualityMode,
    pub definition_digest: String,
    pub graph: AnalysisGraphSpec,
    pub node_bindings: Vec<CompiledNodeBinding>,
    #[serde(default)]
    pub artifact_bindings: Vec<CompiledArtifactBinding>,
    #[serde(default)]
    pub resolved_parameters: BTreeMap<WorkflowNodeId, serde_json::Value>,
}

pub fn workflow_definition_digest(definition: &WorkflowDefinition) -> Result<String, String> {
    let mut canonical = definition.clone();
    canonical
        .nodes
        .sort_by(|left, right| left.instance_id.cmp(&right.instance_id));
    canonical.edges.sort_by(|left, right| {
        (
            &left.from.node,
            &left.from.port,
            &left.to.node,
            &left.to.port,
        )
            .cmp(&(
                &right.from.node,
                &right.from.port,
                &right.to.node,
                &right.to.port,
            ))
    });
    canonical.analyzer_bindings.sort_by(|left, right| {
        (
            &left.analyzer_node,
            &left.source.node,
            &left.source.port,
            &left.analyzer_input,
        )
            .cmp(&(
                &right.analyzer_node,
                &right.source.node,
                &right.source.port,
                &right.analyzer_input,
            ))
    });
    let bytes = serde_json::to_vec(&canonical).map_err(|error| error.to_string())?;
    Ok(blake3::hash(&bytes).to_hex()[..32].to_string())
}
