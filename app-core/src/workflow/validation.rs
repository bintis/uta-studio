use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::analysis_graph::{
    AnalysisEdge, AnalysisGraphSpec, AnalysisNodeId, AnalysisNodeSpec, CachePolicy, DisablePolicy,
};

use super::{
    AnalyzerBinding, CapabilityClass, CapabilityId, NodeCapability, WorkflowDefinition,
    WorkflowEdge, WorkflowNodeId, WorkflowPortRef, builtin_capabilities,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowValidationCode {
    DuplicateNode,
    UnknownCapability,
    DuplicateSingletonCapability,
    UnknownNode,
    UnknownPort,
    InvalidPortDirection,
    TypeMismatch,
    DuplicateInput,
    MissingRequiredInput,
    ConditionalRequiredInput,
    InvalidAnalyzerBinding,
    MissingHardDependency,
    MissingFinalOutput,
    Cycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowValidationIssue {
    pub code: WorkflowValidationCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<WorkflowNodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkflowValidationReport {
    pub issues: Vec<WorkflowValidationIssue>,
    pub warnings: Vec<WorkflowValidationIssue>,
}

impl WorkflowValidationReport {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

fn issue(
    code: WorkflowValidationCode,
    message: impl Into<String>,
    node: Option<WorkflowNodeId>,
    edge_index: Option<usize>,
) -> WorkflowValidationIssue {
    WorkflowValidationIssue {
        code,
        message: message.into(),
        node,
        edge_index,
    }
}

fn structural_graph(definition: &WorkflowDefinition) -> AnalysisGraphSpec {
    let nodes = definition
        .nodes
        .iter()
        .map(|node| AnalysisNodeSpec {
            id: AnalysisNodeId::new(node.instance_id.as_str()),
            label: node.instance_id.to_string(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            disable_policy: DisablePolicy::Optional,
            cache_policy: CachePolicy::Generalized,
            algorithm_version: "workflow-validation-v1".to_string(),
            compound_children: Vec::new(),
        })
        .collect();
    let mut edges = definition
        .edges
        .iter()
        .map(|edge| AnalysisEdge {
            from: AnalysisNodeId::new(edge.from.node.as_str()),
            to: AnalysisNodeId::new(edge.to.node.as_str()),
        })
        .collect::<Vec<_>>();
    edges.extend(
        definition
            .analyzer_bindings
            .iter()
            .map(|binding| AnalysisEdge {
                from: AnalysisNodeId::new(binding.source.node.as_str()),
                to: AnalysisNodeId::new(binding.analyzer_node.as_str()),
            }),
    );
    edges.sort_by(|left, right| (&left.from, &left.to).cmp(&(&right.from, &right.to)));
    edges.dedup();
    AnalysisGraphSpec {
        schema_version: definition.schema_version,
        nodes,
        edges,
    }
}

fn resolved_output_types_for_nodes(
    definition: &WorkflowDefinition,
    nodes: &BTreeMap<&WorkflowNodeId, &NodeCapability>,
) -> BTreeMap<(WorkflowNodeId, String), super::WorkflowPortType> {
    let mut resolved = nodes
        .iter()
        .flat_map(|(node_id, capability)| {
            capability.outputs.iter().map(|output| {
                (
                    ((*node_id).clone(), output.id.clone()),
                    output.port_type.clone(),
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    for _ in 0..definition.nodes.len() {
        let mut changed = false;
        for (node_id, capability) in nodes {
            if !capability.preserves_audio_role {
                continue;
            }
            let Some(source_type) = definition
                .edges
                .iter()
                .find(|edge| &edge.to.node == *node_id && edge.to.port == "audio")
                .and_then(|edge| {
                    resolved
                        .get(&(edge.from.node.clone(), edge.from.port.clone()))
                        .cloned()
                })
            else {
                continue;
            };
            for output in capability
                .outputs
                .iter()
                .filter(|port| port.port_type.is_audio())
            {
                let key = ((*node_id).clone(), output.id.clone());
                if resolved.get(&key) != Some(&source_type) {
                    resolved.insert(key, source_type.clone());
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    resolved
}

pub(crate) fn resolved_workflow_output_types(
    definition: &WorkflowDefinition,
) -> BTreeMap<(WorkflowNodeId, String), super::WorkflowPortType> {
    let registry = builtin_capabilities()
        .into_iter()
        .map(|capability| (capability.id.clone(), capability))
        .collect::<BTreeMap<_, _>>();
    let nodes = definition
        .nodes
        .iter()
        .filter_map(|node| {
            registry
                .get(&node.capability_id)
                .map(|capability| (&node.instance_id, capability))
        })
        .collect::<BTreeMap<_, _>>();
    resolved_output_types_for_nodes(definition, &nodes)
}

fn validate_edge(
    index: usize,
    edge: &WorkflowEdge,
    nodes: &BTreeMap<&WorkflowNodeId, &NodeCapability>,
    resolved_outputs: &BTreeMap<(WorkflowNodeId, String), super::WorkflowPortType>,
    report: &mut WorkflowValidationReport,
) {
    let Some(from) = nodes.get(&edge.from.node) else {
        report.issues.push(issue(
            WorkflowValidationCode::UnknownNode,
            format!("Connection source {} does not exist.", edge.from.node),
            Some(edge.from.node.clone()),
            Some(index),
        ));
        return;
    };
    let Some(to) = nodes.get(&edge.to.node) else {
        report.issues.push(issue(
            WorkflowValidationCode::UnknownNode,
            format!("Connection target {} does not exist.", edge.to.node),
            Some(edge.to.node.clone()),
            Some(index),
        ));
        return;
    };
    let Some(output) = from.output(&edge.from.port) else {
        let code = if from.input(&edge.from.port).is_some() {
            WorkflowValidationCode::InvalidPortDirection
        } else {
            WorkflowValidationCode::UnknownPort
        };
        report.issues.push(issue(
            code,
            format!(
                "{} has no output named '{}'.",
                edge.from.node, edge.from.port
            ),
            Some(edge.from.node.clone()),
            Some(index),
        ));
        return;
    };
    let Some(input) = to.input(&edge.to.port) else {
        let code = if to.output(&edge.to.port).is_some() {
            WorkflowValidationCode::InvalidPortDirection
        } else {
            WorkflowValidationCode::UnknownPort
        };
        report.issues.push(issue(
            code,
            format!("{} has no input named '{}'.", edge.to.node, edge.to.port),
            Some(edge.to.node.clone()),
            Some(index),
        ));
        return;
    };
    let produced_type = resolved_outputs
        .get(&(edge.from.node.clone(), edge.from.port.clone()))
        .unwrap_or(&output.port_type);
    let compatible = if to.preserves_audio_role {
        input.port_type.is_audio() && produced_type.is_audio()
    } else {
        input.port_type.accepts(produced_type)
    };
    if !compatible {
        report.issues.push(issue(
            WorkflowValidationCode::TypeMismatch,
            format!(
                "{} produces {:?}, but {} requires {:?}.",
                edge.from.node, produced_type, edge.to.node, input.port_type
            ),
            Some(edge.to.node.clone()),
            Some(index),
        ));
    }
}

fn validate_binding(
    binding: &AnalyzerBinding,
    nodes: &BTreeMap<&WorkflowNodeId, &NodeCapability>,
    resolved_outputs: &BTreeMap<(WorkflowNodeId, String), super::WorkflowPortType>,
    report: &mut WorkflowValidationReport,
) {
    let Some(analyzer) = nodes.get(&binding.analyzer_node) else {
        report.issues.push(issue(
            WorkflowValidationCode::UnknownNode,
            format!("Analyzer {} does not exist.", binding.analyzer_node),
            Some(binding.analyzer_node.clone()),
            None,
        ));
        return;
    };
    let analyzer_input = analyzer.input(&binding.analyzer_input);
    let source_type =
        resolved_outputs.get(&(binding.source.node.clone(), binding.source.port.clone()));
    let valid_analyzer = matches!(analyzer.class, CapabilityClass::Analyzer)
        && analyzer_input.is_some_and(|port| port.port_type.is_audio());
    let valid_source = nodes
        .get(&binding.source.node)
        .and_then(|source| source.output(&binding.source.port))
        .is_some_and(|port| port.port_type.is_audio())
        && analyzer_input
            .zip(source_type)
            .is_some_and(|(input, produced)| input.port_type.accepts(produced));
    if !valid_analyzer || !valid_source {
        report.issues.push(issue(
            WorkflowValidationCode::InvalidAnalyzerBinding,
            format!(
                "Analyzer {} cannot consume artifact {} output '{}'. Choose a compatible audio role.",
                binding.analyzer_node, binding.source.node, binding.source.port
            ),
            Some(binding.analyzer_node.clone()),
            None,
        ));
    }
}

pub fn validate_workflow(definition: &WorkflowDefinition) -> WorkflowValidationReport {
    let registry = builtin_capabilities()
        .into_iter()
        .map(|capability| (capability.id.clone(), capability))
        .collect::<BTreeMap<_, _>>();
    let mut report = WorkflowValidationReport::default();
    let mut seen_nodes = BTreeSet::new();
    let mut seen_singletons = BTreeSet::new();
    let mut nodes = BTreeMap::new();

    for node in &definition.nodes {
        if !seen_nodes.insert(node.instance_id.clone()) {
            report.issues.push(issue(
                WorkflowValidationCode::DuplicateNode,
                format!("Node instance {} appears more than once.", node.instance_id),
                Some(node.instance_id.clone()),
                None,
            ));
            continue;
        }
        let Some(capability) = registry.get(&node.capability_id) else {
            report.issues.push(issue(
                WorkflowValidationCode::UnknownCapability,
                format!("Capability {} is not available.", node.capability_id),
                Some(node.instance_id.clone()),
                None,
            ));
            continue;
        };
        if !capability.allows_multiple_instances && !seen_singletons.insert(capability.id.clone()) {
            report.issues.push(issue(
                WorkflowValidationCode::DuplicateSingletonCapability,
                format!(
                    "{} may only appear once in this workflow.",
                    capability.label
                ),
                Some(node.instance_id.clone()),
                None,
            ));
        }
        nodes.insert(&node.instance_id, capability);
    }

    let resolved_outputs = resolved_output_types_for_nodes(definition, &nodes);
    for (index, edge) in definition.edges.iter().enumerate() {
        validate_edge(index, edge, &nodes, &resolved_outputs, &mut report);
    }
    for binding in &definition.analyzer_bindings {
        validate_binding(binding, &nodes, &resolved_outputs, &mut report);
    }

    let policies = definition
        .nodes
        .iter()
        .map(|node| (&node.instance_id, &node.execution_policy))
        .collect::<BTreeMap<_, _>>();
    let producer_is_guaranteed = |node_id: &WorkflowNodeId| {
        policies
            .get(node_id)
            .is_some_and(|policy| matches!(policy, super::ExecutionPolicy::Always))
    };
    let mut connected_inputs: BTreeMap<(&WorkflowNodeId, &str), usize> = BTreeMap::new();
    let mut guaranteed_inputs: BTreeMap<(&WorkflowNodeId, &str), usize> = BTreeMap::new();
    for edge in &definition.edges {
        *connected_inputs
            .entry((&edge.to.node, edge.to.port.as_str()))
            .or_default() += 1;
        if producer_is_guaranteed(&edge.from.node) {
            *guaranteed_inputs
                .entry((&edge.to.node, edge.to.port.as_str()))
                .or_default() += 1;
        }
    }
    for binding in &definition.analyzer_bindings {
        *connected_inputs
            .entry((&binding.analyzer_node, binding.analyzer_input.as_str()))
            .or_default() += 1;
        if producer_is_guaranteed(&binding.source.node) {
            *guaranteed_inputs
                .entry((&binding.analyzer_node, binding.analyzer_input.as_str()))
                .or_default() += 1;
        }
    }
    for (node_id, capability) in &nodes {
        for input in &capability.inputs {
            let count = connected_inputs
                .get(&(*node_id, input.id.as_str()))
                .copied()
                .unwrap_or(0);
            if input.required && count == 0 {
                report.issues.push(issue(
                    WorkflowValidationCode::MissingRequiredInput,
                    format!("{} requires an input for '{}'.", node_id, input.id),
                    Some((*node_id).clone()),
                    None,
                ));
            } else if input.required
                && !matches!(
                    policies.get(*node_id),
                    Some(super::ExecutionPolicy::Disabled)
                )
                && guaranteed_inputs
                    .get(&(*node_id, input.id.as_str()))
                    .copied()
                    .unwrap_or(0)
                    == 0
            {
                report.issues.push(issue(
                    WorkflowValidationCode::ConditionalRequiredInput,
                    format!(
                        "{} input '{}' depends only on conditional or disabled nodes. Keep one producer set to Always.",
                        node_id, input.id
                    ),
                    Some((*node_id).clone()),
                    None,
                ));
            }
            if !input.multiple && count > 1 {
                report.issues.push(issue(
                    WorkflowValidationCode::DuplicateInput,
                    format!(
                        "{} input '{}' accepts only one connection.",
                        node_id, input.id
                    ),
                    Some((*node_id).clone()),
                    None,
                ));
            }
        }
    }

    let graph = structural_graph(definition);
    if graph.validate().is_err() {
        report.issues.push(issue(
            WorkflowValidationCode::Cycle,
            "The workflow contains a cycle. Audio and evidence must flow forward.",
            None,
            None,
        ));
    } else {
        for (node_id, capability) in &nodes {
            let dependencies = graph.dependencies_of(&AnalysisNodeId::new(node_id.as_str()));
            let upstream_capabilities = dependencies
                .iter()
                .filter_map(|id| {
                    definition
                        .nodes
                        .iter()
                        .find(|node| node.instance_id.as_str() == id.as_str())
                        .map(|node| &node.capability_id)
                })
                .collect::<BTreeSet<&CapabilityId>>();
            for required in &capability.hard_dependencies {
                if !upstream_capabilities.contains(required) {
                    report.issues.push(issue(
                        WorkflowValidationCode::MissingHardDependency,
                        format!("{} requires {} upstream.", capability.label, required),
                        Some((*node_id).clone()),
                        None,
                    ));
                }
            }
        }
    }

    if !nodes
        .values()
        .any(|capability| capability.id.as_str() == "finalize.canonical_singing_track")
    {
        report.issues.push(issue(
            WorkflowValidationCode::MissingFinalOutput,
            "Add a Canonical Singing Track finalization node before running the workflow.",
            None,
            None,
        ));
    }

    let repeated = definition
        .edges
        .iter()
        .filter_map(|edge| {
            let from = nodes.get(&edge.from.node)?;
            let to = nodes.get(&edge.to.node)?;
            (from.id == to.id && from.preserves_audio_role).then(|| edge.to.node.clone())
        })
        .collect::<Vec<_>>();
    for node in repeated {
        report.warnings.push(issue(
            WorkflowValidationCode::DuplicateInput,
            "Repeated cleanup can remove consonants, breath, or transient detail.",
            Some(node),
            None,
        ));
    }

    report
}

pub(crate) fn edge(from_node: &str, from_port: &str, to_node: &str, to_port: &str) -> WorkflowEdge {
    WorkflowEdge {
        from: WorkflowPortRef {
            node: WorkflowNodeId::new(from_node),
            port: from_port.to_string(),
        },
        to: WorkflowPortRef {
            node: WorkflowNodeId::new(to_node),
            port: to_port.to_string(),
        },
    }
}
