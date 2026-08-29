use std::collections::BTreeMap;

use crate::analysis_graph::{
    AnalysisEdge, AnalysisGraphSpec, AnalysisNodeId, AnalysisNodeSpec, ArtifactKind, CachePolicy,
    DisablePolicy,
};

use super::{
    CompiledArtifactBinding, CompiledNodeBinding, ExecutionPolicy, WorkflowDefinition,
    WorkflowExecutionSnapshot, WorkflowPortType, builtin_capabilities, effective_workflow_source,
    resolved_workflow_output_types, validate_workflow, workflow_definition_digest,
};

#[derive(Debug, Clone)]
pub enum WorkflowCompileError {
    Invalid(super::WorkflowValidationReport),
    Internal(String),
}

impl std::fmt::Display for WorkflowCompileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(report) => {
                let message = report
                    .issues
                    .first()
                    .map(|issue| issue.message.as_str())
                    .unwrap_or("workflow is invalid");
                write!(formatter, "workflow is invalid: {message}")
            }
            Self::Internal(message) => formatter.write_str(message),
        }
    }
}

fn artifact_kind(port: &WorkflowPortType) -> ArtifactKind {
    match port {
        WorkflowPortType::Audio(_) => ArtifactKind::AudioStem,
        WorkflowPortType::Lyrics => ArtifactKind::CanonicalLyrics,
        WorkflowPortType::TranscriptEvidence => ArtifactKind::TranscriptEvidence,
        WorkflowPortType::AlignmentEvidence => ArtifactKind::AlignmentEvidence,
        WorkflowPortType::PitchEvidence => ArtifactKind::PitchEvidence,
        WorkflowPortType::BoundaryEvidence => ArtifactKind::BoundaryEvidence,
        WorkflowPortType::TechniqueEvidence => ArtifactKind::TechniqueEvidence,
        WorkflowPortType::AcousticEvidence => ArtifactKind::AcousticEvidence,
        WorkflowPortType::EvidenceBundle => ArtifactKind::EvidenceBundle,
        WorkflowPortType::CandidateGraph => ArtifactKind::CandidateGraph,
        WorkflowPortType::CanonicalSingingTrack => ArtifactKind::CanonicalSingingTrack,
        WorkflowPortType::CandidateChart => ArtifactKind::CandidateChart,
    }
}

fn compiled_node_id(workflow_id: &str) -> AnalysisNodeId {
    let safe = workflow_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    AnalysisNodeId::new(format!("workflow.{safe}"))
}

pub fn compile_workflow(
    definition: &WorkflowDefinition,
) -> Result<WorkflowExecutionSnapshot, WorkflowCompileError> {
    let report = validate_workflow(definition);
    if !report.is_valid() {
        return Err(WorkflowCompileError::Invalid(report));
    }
    let registry = builtin_capabilities()
        .into_iter()
        .map(|capability| (capability.id.clone(), capability))
        .collect::<BTreeMap<_, _>>();
    let node_ids = definition
        .nodes
        .iter()
        .map(|node| {
            (
                node.instance_id.clone(),
                compiled_node_id(node.instance_id.as_str()),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut nodes = Vec::with_capacity(definition.nodes.len());
    let mut bindings = Vec::with_capacity(definition.nodes.len());
    for node in &definition.nodes {
        let capability = registry
            .get(&node.capability_id)
            .ok_or_else(|| WorkflowCompileError::Internal("capability disappeared".to_string()))?;
        let id = node_ids
            .get(&node.instance_id)
            .expect("every workflow node has a compiled id")
            .clone();
        nodes.push(AnalysisNodeSpec {
            id: id.clone(),
            label: capability.label.clone(),
            inputs: capability
                .inputs
                .iter()
                .map(|port| artifact_kind(&port.port_type))
                .collect(),
            outputs: capability
                .outputs
                .iter()
                .map(|port| artifact_kind(&port.port_type))
                .collect(),
            disable_policy: if matches!(node.execution_policy, ExecutionPolicy::Always) {
                DisablePolicy::AlwaysRequired
            } else {
                DisablePolicy::Optional
            },
            cache_policy: CachePolicy::Generalized,
            algorithm_version: "workflow-v1".to_string(),
            compound_children: Vec::new(),
        });
        bindings.push(CompiledNodeBinding {
            workflow_node: node.instance_id.clone(),
            capability_id: node.capability_id.clone(),
            analysis_node: id,
            model_id: node.model_id.clone(),
            separation_strategy: node.separation_strategy,
            execution_policy: node.execution_policy.clone(),
            priority: node.priority,
        });
    }

    let resolved_outputs = resolved_workflow_output_types(definition);
    let execution_active = definition
        .nodes
        .iter()
        .map(|node| {
            (
                &node.instance_id,
                !matches!(node.execution_policy, ExecutionPolicy::Disabled),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut artifact_bindings = definition
        .edges
        .iter()
        .map(|edge| {
            let destination_active = execution_active[&edge.to.node];
            let source = if destination_active {
                effective_workflow_source(definition, &registry, &edge.from)
            } else {
                edge.from.clone()
            };
            let source_instance = definition
                .nodes
                .iter()
                .find(|node| node.instance_id == source.node)
                .expect("validated effective edge source exists");
            let capability = registry
                .get(&source_instance.capability_id)
                .expect("validated effective edge capability exists");
            let port_type = resolved_outputs
                .get(&(source.node.clone(), source.port.clone()))
                .cloned()
                .unwrap_or_else(|| {
                    capability
                        .output(&source.port)
                        .expect("validated effective edge output exists")
                        .port_type
                        .clone()
                });
            CompiledArtifactBinding {
                from_node: node_ids[&source.node].clone(),
                from_port: source.port,
                to_node: node_ids[&edge.to.node].clone(),
                to_port: edge.to.port.clone(),
                port_type,
                execution_active: execution_active[&source.node] && destination_active,
                analyzer_attachment: false,
            }
        })
        .collect::<Vec<_>>();
    artifact_bindings.extend(definition.analyzer_bindings.iter().map(|binding| {
        let analyzer_active = execution_active[&binding.analyzer_node];
        let source = if analyzer_active {
            effective_workflow_source(definition, &registry, &binding.source)
        } else {
            binding.source.clone()
        };
        let source_instance = definition
            .nodes
            .iter()
            .find(|node| node.instance_id == source.node)
            .expect("validated effective analyzer source exists");
        let capability = registry
            .get(&source_instance.capability_id)
            .expect("validated effective analyzer source capability exists");
        let port_type = resolved_outputs
            .get(&(source.node.clone(), source.port.clone()))
            .cloned()
            .unwrap_or_else(|| {
                capability
                    .output(&source.port)
                    .expect("validated effective analyzer output exists")
                    .port_type
                    .clone()
            });
        CompiledArtifactBinding {
            from_node: node_ids[&source.node].clone(),
            from_port: source.port,
            to_node: node_ids[&binding.analyzer_node].clone(),
            to_port: binding.analyzer_input.clone(),
            port_type,
            execution_active: execution_active[&source.node] && analyzer_active,
            analyzer_attachment: true,
        }
    }));
    artifact_bindings.sort_by(|left, right| {
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

    let mut edges = artifact_bindings
        .iter()
        .filter(|binding| binding.execution_active)
        .map(|binding| AnalysisEdge {
            from: binding.from_node.clone(),
            to: binding.to_node.clone(),
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| (&left.from, &left.to).cmp(&(&right.from, &right.to)));
    edges.dedup();

    let graph = AnalysisGraphSpec {
        schema_version: definition.schema_version,
        nodes,
        edges,
    };
    graph
        .validate()
        .map_err(|error| WorkflowCompileError::Internal(error.to_string()))?;

    Ok(WorkflowExecutionSnapshot {
        schema_version: definition.schema_version,
        workflow_id: definition.workflow_id.0.clone(),
        workflow_revision: definition.revision,
        quality_mode: definition.quality_mode,
        definition_digest: workflow_definition_digest(definition)
            .map_err(WorkflowCompileError::Internal)?,
        graph,
        node_bindings: bindings,
        artifact_bindings,
        resolved_parameters: definition
            .nodes
            .iter()
            .map(|node| {
                (
                    node.instance_id.clone(),
                    serde_json::to_value(&node.parameters).unwrap_or_default(),
                )
            })
            .collect(),
    })
}
