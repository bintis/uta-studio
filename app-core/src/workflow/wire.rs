//! Process-boundary DTO for a compiled Processing Studio workflow.
//!
//! This representation is intentionally local to app-core. The Analysis Engine
//! owns an independently declared mirror and validates every field after CLI
//! deserialization; no backend crate type crosses the product boundary.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{
    ConditionalExecution, ExecutionPolicy, QualityMode, WorkflowExecutionSnapshot,
    WorkflowPortType, builtin_capabilities,
};

pub const WORKFLOW_EXECUTION_EXTENSION_KEY: &str = "uta.workflow_execution";
pub const WORKFLOW_EXECUTION_CONTRACT: &str = "uta.workflow-execution";
pub const WORKFLOW_EXECUTION_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowContinuousF0SourceWire {
    Rmvpe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNoteLengthSourceWire {
    F0Derived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowOnsetSupportSourceWire {
    Automatic,
    Acoustic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowFusionModeWire {
    #[default]
    Algorithm,
    AiJudgment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExpertFusionPolicyWire {
    pub continuous_f0: WorkflowContinuousF0SourceWire,
    pub note_lengths: WorkflowNoteLengthSourceWire,
    pub onset_support: WorkflowOnsetSupportSourceWire,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecutionWire {
    pub contract: String,
    pub version: u32,
    pub workflow_schema_version: u32,
    pub workflow_id: String,
    pub workflow_revision: u64,
    pub quality_mode: String,
    pub definition_digest: String,
    pub nodes: Vec<WorkflowNodeWire>,
    pub bindings: Vec<WorkflowBindingWire>,
    pub terminal_outputs: Vec<WorkflowTerminalOutputWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fusion_policy: Option<WorkflowExpertFusionPolicyWire>,
    #[serde(default)]
    pub fusion_mode: WorkflowFusionModeWire,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowProviderPreferencesWire {
    /// Stable Engine resource ID for the node's primary capability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    /// Stable Engine resource ID for the independent Instrumental output slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrumental: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecutionInvocationWire {
    /// Stable UI/runtime correlation identity for exactly one provider call.
    pub invocation_id: String,
    pub provider_id: String,
    pub capabilities: Vec<String>,
    pub output_ports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNodeWire {
    pub instance_id: String,
    pub capability_id: String,
    pub execution_policy: String,
    pub priority: i32,
    #[serde(default, skip_serializing_if = "provider_preferences_are_empty")]
    pub provider_preferences: WorkflowProviderPreferencesWire,
    /// Typed provider-call topology. One descriptor is one real execution card;
    /// two descriptors are two independently progressing/logged provider calls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_invocations: Vec<WorkflowExecutionInvocationWire>,
}

fn provider_preferences_are_empty(preferences: &WorkflowProviderPreferencesWire) -> bool {
    preferences.primary.is_none() && preferences.instrumental.is_none()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowBindingWire {
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
pub struct WorkflowTerminalOutputWire {
    pub node: String,
    pub port: String,
    pub semantic_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_role: Option<String>,
}

fn fusion_mode(snapshot: &WorkflowExecutionSnapshot) -> WorkflowFusionModeWire {
    let Some(binding) = snapshot
        .node_bindings
        .iter()
        .find(|binding| binding.capability_id.as_str() == "fusion.singing_evidence")
    else {
        return WorkflowFusionModeWire::default();
    };
    let mode = snapshot
        .resolved_parameters
        .get(&binding.workflow_node)
        .and_then(serde_json::Value::as_object)
        .and_then(|parameters| parameters.get("fusion_mode"))
        .and_then(serde_json::Value::as_str);
    match mode {
        Some("ai") => WorkflowFusionModeWire::AiJudgment,
        _ => WorkflowFusionModeWire::Algorithm,
    }
}
impl WorkflowExecutionWire {
    pub fn from_snapshot(snapshot: &WorkflowExecutionSnapshot) -> Result<Self, String> {
        let mut nodes = snapshot
            .node_bindings
            .iter()
            .map(|binding| {
                let strategy = binding
                    .separation_strategy
                    .map(super::separation_strategy_descriptor);
                let primary = strategy
                    .and_then(|descriptor| descriptor.executions.first())
                    .map(|execution| execution.provider_id.to_string())
                    .or_else(|| binding.model_id.clone());
                let instrumental = strategy.and_then(|descriptor| {
                    descriptor.executions.iter().find_map(|execution| {
                        execution
                            .output_roles
                            .contains(&super::SeparationOutputRole::Instrumental)
                            .then(|| execution.provider_id.to_string())
                    })
                });
                let execution_invocations = strategy
                    .map(|descriptor| {
                        descriptor
                            .executions
                            .iter()
                            .map(|execution| {
                                let suffix = if descriptor.executions.len() == 1 {
                                    None
                                } else {
                                    Some(match execution.output_roles[0] {
                                        super::SeparationOutputRole::Vocal => "vocal",
                                        super::SeparationOutputRole::Instrumental => "instrumental",
                                    })
                                };
                                WorkflowExecutionInvocationWire {
                                    invocation_id: suffix.map_or_else(
                                        || binding.workflow_node.to_string(),
                                        |suffix| format!("{}.{suffix}", binding.workflow_node),
                                    ),
                                    provider_id: execution.provider_id.to_string(),
                                    capabilities: execution
                                        .output_roles
                                        .iter()
                                        .map(|role| role.engine_capability().to_string())
                                        .collect(),
                                    output_ports: execution
                                        .output_roles
                                        .iter()
                                        .map(|role| role.output_port().to_string())
                                        .collect(),
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                WorkflowNodeWire {
                    instance_id: binding.workflow_node.to_string(),
                    capability_id: binding.capability_id.to_string(),
                    execution_policy: policy_name(&binding.execution_policy).to_string(),
                    priority: binding.priority,
                    provider_preferences: WorkflowProviderPreferencesWire {
                        primary,
                        instrumental,
                    },
                    execution_invocations,
                }
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|left, right| left.instance_id.cmp(&right.instance_id));

        let analysis_to_instance = snapshot
            .node_bindings
            .iter()
            .map(|binding| {
                (
                    binding.analysis_node.as_str(),
                    binding.workflow_node.as_str(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut bindings = snapshot
            .artifact_bindings
            .iter()
            .map(|binding| {
                let (semantic_type, audio_role) = port_type_names(&binding.port_type);
                WorkflowBindingWire {
                    from_node: analysis_to_instance
                        .get(binding.from_node.as_str())
                        .copied()
                        .unwrap_or(binding.from_node.as_str())
                        .to_string(),
                    from_port: binding.from_port.clone(),
                    to_node: analysis_to_instance
                        .get(binding.to_node.as_str())
                        .copied()
                        .unwrap_or(binding.to_node.as_str())
                        .to_string(),
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
                if consumed.contains(&(node.instance_id.as_str(), output.id.as_str())) {
                    continue;
                }
                let (semantic_type, audio_role) = port_type_names(&output.port_type);
                terminal_outputs.push(WorkflowTerminalOutputWire {
                    node: node.instance_id.clone(),
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
            // Kept as a deserialize-only compatibility field. Step 4 no longer
            // authors expert ownership; the Engine derives it from Stage 3.
            fusion_policy: None,
            fusion_mode: fusion_mode(snapshot),
        })
    }
}

pub fn workflow_execution_extension(
    snapshot: &WorkflowExecutionSnapshot,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(WorkflowExecutionWire::from_snapshot(snapshot)?)
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
    fn default_wire_preserves_truthful_implemented_routes() {
        let snapshot = compile_workflow(&default_workflow("song-a")).unwrap();
        let wire = WorkflowExecutionWire::from_snapshot(&snapshot).unwrap();
        let node = |id: &str| {
            wire.nodes
                .iter()
                .find(|node| node.instance_id == id)
                .unwrap()
        };
        assert_eq!(node("lead_isolate").execution_policy, "disabled");
        assert_eq!(node("vocal_cleanup_1").execution_policy, "disabled");
        assert_eq!(node("vocal_dereverb_1").execution_policy, "disabled");
        assert_eq!(node("f0_rmvpe").execution_policy, "always");
        assert_eq!(node("acoustic_dsp").execution_policy, "always");
        assert_eq!(wire.fusion_policy, None);
        assert!(wire.bindings.iter().any(|binding| {
            binding.execution_active
                && binding.analyzer_attachment
                && binding.semantic_type == "audio"
                && binding.audio_role.as_deref() == Some("vocal")
        }));
        assert!(!wire.terminal_outputs.iter().any(|output| {
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
        let json = serde_json::to_value(&wire).unwrap();
        let node = &json["nodes"][0];
        for forbidden in [
            "analysis_node",
            "model_id",
            "runtime",
            "runtime_recipe_digest",
            "parameters",
        ] {
            assert!(node.get(forbidden).is_none(), "wire leaked {forbidden}");
        }
        let separation = wire
            .nodes
            .iter()
            .find(|node| node.instance_id == "vocal_bgm_split")
            .unwrap();
        assert_eq!(
            separation.provider_preferences.instrumental.as_deref(),
            Some("bs_roformer_leap_xe90_vocals")
        );
        assert_eq!(separation.execution_invocations.len(), 1);
        assert_eq!(
            separation.execution_invocations[0].invocation_id,
            "vocal_bgm_split"
        );
        assert_eq!(
            separation.execution_invocations[0].provider_id,
            "bs_roformer_leap_xe90_vocals"
        );
        assert_eq!(
            separation.execution_invocations[0].capabilities,
            ["audio.extract_vocals", "audio.extract_instrumental"]
        );
        assert_eq!(
            separation.execution_invocations[0].output_ports,
            ["vocal", "instrumental"]
        );
    }

    #[test]
    fn wire_never_authors_a_typed_fusion_policy() {
        let definition = default_workflow("song-a");
        let snapshot = compile_workflow(&definition).unwrap();
        let wire = WorkflowExecutionWire::from_snapshot(&snapshot).unwrap();
        assert_eq!(wire.fusion_policy, None);
        let json = serde_json::to_value(wire).unwrap();
        assert!(json.get("fusion_policy").is_none());
    }

    #[test]
    fn wire_defaults_to_algorithm_fusion_mode_and_carries_an_explicit_ai_selection() {
        let mut definition = default_workflow("song-a");
        let snapshot = compile_workflow(&definition).unwrap();
        let wire = WorkflowExecutionWire::from_snapshot(&snapshot).unwrap();
        assert_eq!(wire.fusion_mode, WorkflowFusionModeWire::Algorithm);
        assert_eq!(
            crate::workflow::fusion_mode(&definition),
            crate::workflow::FusionMode::Algorithm
        );

        crate::workflow::set_workflow_parameter(
            &mut definition,
            &crate::workflow::WorkflowNodeId::new("evidence_fusion"),
            "fusion_mode",
            serde_json::Value::String("ai".to_string()),
        )
        .unwrap();
        assert_eq!(
            crate::workflow::fusion_mode(&definition),
            crate::workflow::FusionMode::AiJudgment
        );
        let snapshot = compile_workflow(&definition).unwrap();
        let wire = WorkflowExecutionWire::from_snapshot(&snapshot).unwrap();
        assert_eq!(wire.fusion_mode, WorkflowFusionModeWire::AiJudgment);

        let json = serde_json::to_value(&wire).unwrap();
        let round_tripped: WorkflowExecutionWire = serde_json::from_value(json).unwrap();
        assert_eq!(
            round_tripped.fusion_mode,
            WorkflowFusionModeWire::AiJudgment
        );
    }

    #[test]
    fn set_workflow_parameter_rejects_an_unknown_fusion_mode() {
        let mut definition = default_workflow("song-a");
        let error = crate::workflow::set_workflow_parameter(
            &mut definition,
            &crate::workflow::WorkflowNodeId::new("evidence_fusion"),
            "fusion_mode",
            serde_json::Value::String("agentic".to_string()),
        )
        .unwrap_err();
        assert!(error.contains("fusion_mode"));
    }
}
