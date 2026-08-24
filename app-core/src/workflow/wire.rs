//! Process-boundary DTO for a compiled Processing Studio workflow.
//!
//! This representation is intentionally local to app-core. The Analysis Engine
//! owns an independently declared mirror and validates every field after CLI
//! deserialization; no backend crate type crosses the product boundary.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{
    ConditionalExecution, ExecutionPolicy, QualityMode, ResolvedRuntimeKind,
    WorkflowExecutionSnapshot, WorkflowPortType, builtin_capabilities,
};

pub const WORKFLOW_EXECUTION_EXTENSION_KEY: &str = "uta.workflow_execution.v1";
pub const WORKFLOW_EXECUTION_CONTRACT: &str = "uta.workflow-execution";
pub const WORKFLOW_EXECUTION_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecutionWireV1 {
    pub contract: String,
    pub version: u32,
    pub workflow_schema_version: u32,
    pub workflow_id: String,
    pub workflow_revision: u64,
    pub quality_mode: String,
    pub definition_digest: String,
    pub nodes: Vec<WorkflowNodeWireV1>,
    pub bindings: Vec<WorkflowBindingWireV1>,
    pub terminal_outputs: Vec<WorkflowTerminalOutputWireV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNodeWireV1 {
    pub instance_id: String,
    pub capability_id: String,
    pub analysis_node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub execution_policy: String,
    pub priority: i32,
    pub runtime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowBindingWireV1 {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
    pub semantic_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_role: Option<String>,
    pub execution_active: bool,
    pub analyzer_attachment: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTerminalOutputWireV1 {
    pub node: String,
    pub port: String,
    pub semantic_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_role: Option<String>,
}

impl WorkflowExecutionWireV1 {
    pub fn from_snapshot(snapshot: &WorkflowExecutionSnapshot) -> Result<Self, String> {
        let mut nodes = snapshot
            .node_bindings
            .iter()
            .map(|binding| WorkflowNodeWireV1 {
                instance_id: binding.workflow_node.to_string(),
                capability_id: binding.capability_id.to_string(),
                analysis_node: binding.analysis_node.to_string(),
                model_id: binding.model_id.clone(),
                execution_policy: policy_name(&binding.execution_policy).to_string(),
                priority: binding.priority,
                runtime: runtime_name(&binding.runtime).to_string(),
                runtime_recipe_digest: binding.runtime_recipe_digest.clone(),
                parameters: snapshot
                    .resolved_parameters
                    .get(&binding.workflow_node)
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|left, right| left.instance_id.cmp(&right.instance_id));

        let mut bindings = snapshot
            .artifact_bindings
            .iter()
            .map(|binding| {
                let (semantic_type, audio_role) = port_type_names(&binding.port_type);
                WorkflowBindingWireV1 {
                    from_node: binding.from_node.to_string(),
                    from_port: binding.from_port.clone(),
                    to_node: binding.to_node.to_string(),
                    to_port: binding.to_port.clone(),
                    semantic_type: semantic_type.to_string(),
                    audio_role: audio_role.map(str::to_string),
                    execution_active: binding.execution_active,
                    analyzer_attachment: binding.analyzer_attachment,
                }
            })
            .collect::<Vec<_>>();
        bindings.sort_by(|left, right| {
            (
                &left.from_node,
                &left.from_port,
                &left.to_node,
                &left.to_port,
            )
                .cmp(&(
                    &right.from_node,
                    &right.from_port,
                    &right.to_node,
                    &right.to_port,
                ))
        });

        let consumed = bindings
            .iter()
            .filter(|binding| binding.execution_active)
            .map(|binding| (binding.from_node.as_str(), binding.from_port.as_str()))
            .collect::<BTreeSet<_>>();
        let registry = builtin_capabilities()
            .into_iter()
            .map(|capability| (capability.id.to_string(), capability))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut terminal_outputs = Vec::new();
        for node in &nodes {
            if node.execution_policy == "disabled" {
                continue;
            }
            let capability = registry.get(&node.capability_id).ok_or_else(|| {
                format!(
                    "compiled workflow references unknown capability {}",
                    node.capability_id
                )
            })?;
            for output in &capability.outputs {
                if consumed.contains(&(node.analysis_node.as_str(), output.id.as_str())) {
                    continue;
                }
                let (semantic_type, audio_role) = port_type_names(&output.port_type);
                terminal_outputs.push(WorkflowTerminalOutputWireV1 {
                    node: node.analysis_node.clone(),
                    port: output.id.clone(),
                    semantic_type: semantic_type.to_string(),
                    audio_role: audio_role.map(str::to_string),
                });
            }
        }
        terminal_outputs
            .sort_by(|left, right| (&left.node, &left.port).cmp(&(&right.node, &right.port)));

        Ok(Self {
            contract: WORKFLOW_EXECUTION_CONTRACT.to_string(),
            version: WORKFLOW_EXECUTION_VERSION,
            workflow_schema_version: snapshot.schema_version,
            workflow_id: snapshot.workflow_id.clone(),
            workflow_revision: snapshot.workflow_revision,
            quality_mode: quality_name(snapshot.quality_mode).to_string(),
            definition_digest: snapshot.definition_digest.clone(),
            nodes,
            bindings,
            terminal_outputs,
        })
    }
}

pub fn workflow_execution_extension(
    snapshot: &WorkflowExecutionSnapshot,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(WorkflowExecutionWireV1::from_snapshot(snapshot)?)
        .map_err(|error| format!("could not serialize compiled workflow: {error}"))
}

fn policy_name(policy: &ExecutionPolicy) -> &'static str {
    match policy {
        ExecutionPolicy::Always => "always",
        ExecutionPolicy::Disabled => "disabled",
        ExecutionPolicy::Conditional {
            condition: ConditionalExecution::MaximumOnly,
        } => "maximum_only",
        ExecutionPolicy::Conditional {
            condition: ConditionalExecution::OnDisagreement,
        } => "on_disagreement",
        ExecutionPolicy::Conditional {
            condition: ConditionalExecution::DisagreementWindows,
        } => "disagreement_windows",
    }
}

fn runtime_name(runtime: &ResolvedRuntimeKind) -> &'static str {
    match runtime {
        ResolvedRuntimeKind::OpenVino => "openvino",
        ResolvedRuntimeKind::Vulkan => "vulkan",
        ResolvedRuntimeKind::NativeDsp => "native_dsp",
        ResolvedRuntimeKind::CpuReference => "cpu_reference",
        ResolvedRuntimeKind::PinnedQwenAsrVulkan => "pinned_qwen_asr_vulkan",
        ResolvedRuntimeKind::PinnedQwenAlignVulkan => "pinned_qwen_align_vulkan",
        ResolvedRuntimeKind::Unresolved => "unresolved",
    }
}

fn quality_name(mode: QualityMode) -> &'static str {
    match mode {
        QualityMode::Fast => "fast",
        QualityMode::Balanced => "balanced",
        QualityMode::Maximum => "maximum",
        QualityMode::Custom => "custom",
    }
}

fn port_type_names(port_type: &WorkflowPortType) -> (&'static str, Option<&'static str>) {
    use super::AudioRole;
    match port_type {
        WorkflowPortType::Audio(role) => (
            "audio",
            Some(match role {
                AudioRole::SourceMix => "source_mix",
                AudioRole::Vocal => "vocal",
                AudioRole::LeadVocal => "lead_vocal",
                AudioRole::BackingVocal => "backing_vocal",
                AudioRole::HarmonyVocal => "harmony_vocal",
                AudioRole::VocalResidual => "vocal_residual",
                AudioRole::Instrumental => "instrumental",
                AudioRole::Drums => "drums",
                AudioRole::Bass => "bass",
                AudioRole::Guitar => "guitar",
                AudioRole::Piano => "piano",
                AudioRole::Other => "other",
            }),
        ),
        WorkflowPortType::Lyrics => ("lyrics", None),
        WorkflowPortType::TranscriptEvidence => ("transcript_evidence", None),
        WorkflowPortType::PitchEvidence => ("pitch_evidence", None),
        WorkflowPortType::BoundaryEvidence => ("boundary_evidence", None),
        WorkflowPortType::AlignmentEvidence => ("alignment_evidence", None),
        WorkflowPortType::TechniqueEvidence => ("technique_evidence", None),
        WorkflowPortType::AcousticEvidence => ("acoustic_evidence", None),
        WorkflowPortType::EvidenceBundle => ("evidence_bundle", None),
        WorkflowPortType::CandidateGraph => ("candidate_graph", None),
        WorkflowPortType::CanonicalSingingTrack => ("canonical_singing_track", None),
        WorkflowPortType::CandidateChart => ("candidate_chart", None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{compile_workflow, default_workflow};

    #[test]
    fn default_wire_preserves_truthful_baselines_and_conditional_experts() {
        let snapshot = compile_workflow(&default_workflow("song-a")).unwrap();
        let wire = WorkflowExecutionWireV1::from_snapshot(&snapshot).unwrap();
        let node = |id: &str| {
            wire.nodes
                .iter()
                .find(|node| node.instance_id == id)
                .unwrap()
        };
        assert_eq!(node("asr_qwen").execution_policy, "always");
        assert_eq!(node("asr_firered").execution_policy, "on_disagreement");
        assert_eq!(node("f0_fcpe").execution_policy, "disagreement_windows");
        assert_eq!(
            node("boundary_stars").capability_id,
            "analysis.note_boundary"
        );
        assert_eq!(node("boundary_stars").execution_policy, "maximum_only");
        assert_eq!(node("technique_stars").capability_id, "analysis.technique");
        assert_eq!(node("technique_stars").execution_policy, "maximum_only");
        assert!(wire.bindings.iter().any(|binding| {
            binding.semantic_type == "audio" && binding.audio_role.as_deref() == Some("lead_vocal")
        }));
        assert!(wire.terminal_outputs.iter().any(|output| {
            output.semantic_type == "audio"
                && output.audio_role.as_deref() == Some("vocal_residual")
        }));
        assert!(!wire.terminal_outputs.iter().any(|output| {
            matches!(
                output.audio_role.as_deref(),
                Some("backing_vocal" | "harmony_vocal")
            )
        }));
        assert!(
            wire.terminal_outputs.iter().any(|output| {
                output.semantic_type == "candidate_chart" && output.port == "chart"
            })
        );
    }
}
