use std::collections::BTreeMap;

use crate::analysis_graph::{
    AnalysisEdge, AnalysisGraphSpec, AnalysisNodeId, AnalysisNodeSpec, ArtifactKind, CachePolicy,
    DisablePolicy,
};

use super::{
    CapabilityClass, CompiledArtifactBinding, CompiledNodeBinding, ExecutionPolicy,
    ResolvedRuntimeKind, WorkflowDefinition, WorkflowExecutionSnapshot, WorkflowPortType,
    builtin_capabilities, resolved_workflow_output_types, validate_workflow,
    workflow_definition_digest,
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

fn resolved_runtime(
    capability_class: &CapabilityClass,
    model_id: Option<&str>,
) -> (ResolvedRuntimeKind, Option<String>) {
    let Some(model_id) = model_id else {
        return match capability_class {
            CapabilityClass::Analyzer | CapabilityClass::AudioTransformation => {
                (ResolvedRuntimeKind::Unresolved, None)
            }
            CapabilityClass::Source | CapabilityClass::Fusion | CapabilityClass::Finalization => {
                (ResolvedRuntimeKind::NativeDsp, None)
            }
        };
    };
    let Some(model) = crate::native_runtime::native_runtime_registry()
        .into_iter()
        .find(|model| model.model_id == model_id)
    else {
        return (ResolvedRuntimeKind::Unresolved, None);
    };
    let recipe = model.runtime_recipe_digest.clone();
    let production = model.backends.iter().find(|capability| {
        capability.validation == crate::native_runtime::ValidationState::ProductionPinned
            && model
                .pinned_backend
                .is_none_or(|backend| backend == capability.backend)
    });
    let runtime = match production.map(|capability| capability.backend) {
        Some(crate::native_runtime::NativeBackend::OpenVino) => ResolvedRuntimeKind::OpenVino,
        Some(crate::native_runtime::NativeBackend::Vulkan) if model_id == "qwen3_asr_1_7b" => {
            ResolvedRuntimeKind::PinnedQwenAsrVulkan
        }
        Some(crate::native_runtime::NativeBackend::Vulkan)
            if model_id == "qwen3_forced_aligner_0_6b" =>
        {
            ResolvedRuntimeKind::PinnedQwenAlignVulkan
        }
        Some(crate::native_runtime::NativeBackend::Vulkan) => ResolvedRuntimeKind::Vulkan,
        Some(crate::native_runtime::NativeBackend::NativeDsp) => ResolvedRuntimeKind::NativeDsp,
        Some(crate::native_runtime::NativeBackend::CpuReference) | None => {
            ResolvedRuntimeKind::Unresolved
        }
    };
    (runtime, recipe)
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
        let (runtime, runtime_recipe_digest) =
            resolved_runtime(&capability.class, node.model_id.as_deref());
        bindings.push(CompiledNodeBinding {
            workflow_node: node.instance_id.clone(),
            capability_id: node.capability_id.clone(),
            analysis_node: id,
            model_id: node.model_id.clone(),
            execution_policy: node.execution_policy.clone(),
            priority: node.priority,
            runtime,
            runtime_recipe_digest,
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
            let capability = registry
                .get(
                    &definition
                        .nodes
                        .iter()
                        .find(|node| node.instance_id == edge.from.node)
                        .expect("validated edge source exists")
                        .capability_id,
                )
                .expect("validated edge capability exists");
            let port_type = resolved_outputs
                .get(&(edge.from.node.clone(), edge.from.port.clone()))
                .cloned()
                .unwrap_or_else(|| {
                    capability
                        .output(&edge.from.port)
                        .expect("validated edge output exists")
                        .port_type
                        .clone()
                });
            CompiledArtifactBinding {
                from_node: node_ids[&edge.from.node].clone(),
                from_port: edge.from.port.clone(),
                to_node: node_ids[&edge.to.node].clone(),
                to_port: edge.to.port.clone(),
                port_type,
                execution_active: execution_active[&edge.from.node]
                    && execution_active[&edge.to.node],
                analyzer_attachment: false,
            }
        })
        .collect::<Vec<_>>();
    artifact_bindings.extend(definition.analyzer_bindings.iter().map(|binding| {
        let capability = registry
            .get(
                &definition
                    .nodes
                    .iter()
                    .find(|node| node.instance_id == binding.source.node)
                    .expect("validated analyzer source exists")
                    .capability_id,
            )
            .expect("validated analyzer source capability exists");
        let port_type = resolved_outputs
            .get(&(binding.source.node.clone(), binding.source.port.clone()))
            .cloned()
            .unwrap_or_else(|| {
                capability
                    .output(&binding.source.port)
                    .expect("validated analyzer output exists")
                    .port_type
                    .clone()
            });
        CompiledArtifactBinding {
            from_node: node_ids[&binding.source.node].clone(),
            from_port: binding.source.port.clone(),
            to_node: node_ids[&binding.analyzer_node].clone(),
            to_port: binding.analyzer_input.clone(),
            port_type,
            execution_active: execution_active[&binding.source.node]
                && execution_active[&binding.analyzer_node],
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
