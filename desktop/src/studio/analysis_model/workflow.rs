//! Projection of the compiled Processing Studio workflow into the native DAG.
//!
//! This is the only model used by Advanced Graph. It consumes the same
//! versioned workflow snapshot attached to Engine Preview/Execution and does
//! not reconstruct topology from presentation conventions.

use std::collections::BTreeMap;

use super::*;

pub(crate) fn exact_engine_capabilities_from_engine(
    engine: &app_core::EngineRunHistoryProjection,
) -> Option<std::collections::BTreeSet<String>> {
    let plan: app_core::AnalysisPlanWireV1 = serde_json::from_str(&engine.plan_json).ok()?;
    Some(
        plan.execution_nodes
            .into_iter()
            .map(|node| node.capability.as_str().to_string())
            .collect(),
    )
}

pub(crate) fn exact_workflow_plan_from_engine(
    engine: &app_core::EngineRunHistoryProjection,
) -> Option<(
    app_core::WorkflowExecutionWireV1,
    Option<app_core::WorkflowExecutionPlanWireV1>,
)> {
    let request: app_core::AnalyzeRequestWireV1 =
        serde_json::from_str(&engine.request_json).ok()?;
    let workflow = request
        .extensions
        .get(app_core::WORKFLOW_EXECUTION_EXTENSION_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())?;
    let plan = serde_json::from_str::<app_core::AnalysisPlanWireV1>(&engine.plan_json)
        .ok()
        .and_then(|plan| plan.workflow_execution);
    Some((workflow, plan))
}

pub(crate) fn workflow_graph_step(capability_id: Option<&str>) -> u8 {
    match capability_id {
        Some(id) if id.starts_with("audio.") => 1,
        Some(
            "lyrics.known"
            | "analysis.asr"
            | "analysis.forced_alignment"
            | "fusion.transcript"
            | "speech.transcribe"
            | "speech.transcribe.challenger"
            | "speech.align"
            | "fusion.alignment",
        ) => 2,
        Some(
            "analysis.pitch_f0"
            | "analysis.note_boundary"
            | "analysis.technique"
            | "analysis.acoustic_dsp"
            | "pitch.track"
            | "pitch.secondary.rmvpe"
            | "pitch.secondary.fcpe"
            | "notes.game"
            | "notes.basic_pitch"
            | "notes.rosvot"
            | "notes.stars"
            | "technique.analyze",
        ) => 3,
        _ => 4,
    }
}

pub(crate) fn workflow_graph_category(capability_id: Option<&str>) -> GraphNodeCategory {
    match capability_id {
        Some("audio.source" | "lyrics.known") => GraphNodeCategory::Source,
        Some(id) if id.starts_with("audio.") => GraphNodeCategory::Audio,
        Some("analysis.asr" | "analysis.forced_alignment") => GraphNodeCategory::Lyrics,
        Some("analysis.pitch_f0") => GraphNodeCategory::Pitch,
        Some(id) if id.starts_with("analysis.") => GraphNodeCategory::Evidence,
        Some(id) if id.starts_with("fusion.") => GraphNodeCategory::Fusion,
        Some(id) if id.starts_with("finalize.") => GraphNodeCategory::Output,
        _ => GraphNodeCategory::Evidence,
    }
}

fn local_state(node: &app_core::WorkflowNodeWireV1, quality: &str) -> GraphNodeState {
    match node.execution_policy.as_str() {
        "disabled" => GraphNodeState::Disabled,
        "maximum_only" if quality != "maximum" => GraphNodeState::ProfileSkipped,
        "on_disagreement" | "disagreement_windows" => GraphNodeState::Deferred,
        _ => GraphNodeState::Waiting,
    }
}

fn exact_state(state: app_core::WorkflowNodeExecutionStateWireV1) -> GraphNodeState {
    match state {
        app_core::WorkflowNodeExecutionStateWireV1::Ready => GraphNodeState::Waiting,
        app_core::WorkflowNodeExecutionStateWireV1::Deferred => GraphNodeState::Deferred,
        app_core::WorkflowNodeExecutionStateWireV1::Disabled => GraphNodeState::Disabled,
        app_core::WorkflowNodeExecutionStateWireV1::ProfileSkipped => {
            GraphNodeState::ProfileSkipped
        }
        app_core::WorkflowNodeExecutionStateWireV1::NotRequested => GraphNodeState::NotRequested,
    }
}

fn policy_label(policy: &str) -> &'static str {
    match policy {
        "always" => "Always",
        "disabled" => "Disabled",
        "maximum_only" => "Maximum only",
        "on_disagreement" => "On disagreement",
        "disagreement_windows" => "Disagreement windows",
        _ => "Unknown policy",
    }
}

fn groups_parallel_experts(capability_id: Option<&str>) -> bool {
    matches!(
        capability_id,
        Some(
            "analysis.asr" | "analysis.pitch_f0" | "analysis.note_boundary" | "analysis.technique"
        )
    )
}

fn aggregate_member_state(members: &[RenderNodeMember]) -> GraphNodeState {
    let active = members
        .iter()
        .filter(|member| {
            !matches!(
                member.state,
                GraphNodeState::Disabled
                    | GraphNodeState::ProfileSkipped
                    | GraphNodeState::NotRequested
            )
        })
        .collect::<Vec<_>>();
    if active
        .iter()
        .any(|member| member.state == GraphNodeState::Failed)
    {
        GraphNodeState::Failed
    } else if active
        .iter()
        .any(|member| member.state == GraphNodeState::Running)
    {
        GraphNodeState::Running
    } else if !active.is_empty()
        && active
            .iter()
            .all(|member| member.state == GraphNodeState::Complete)
    {
        GraphNodeState::Complete
    } else if active
        .iter()
        .any(|member| member.state == GraphNodeState::Waiting)
    {
        GraphNodeState::Waiting
    } else if active
        .iter()
        .any(|member| member.state == GraphNodeState::Deferred)
    {
        GraphNodeState::Deferred
    } else if active
        .iter()
        .any(|member| member.state == GraphNodeState::Cancelled)
    {
        GraphNodeState::Cancelled
    } else if members
        .iter()
        .any(|member| member.state == GraphNodeState::ProfileSkipped)
    {
        GraphNodeState::ProfileSkipped
    } else {
        GraphNodeState::NotRequested
    }
}

fn group_parallel_expert_nodes(nodes: Vec<RenderNode>, edges: Vec<RenderEdge>) -> RenderGraph {
    let mut grouped = Vec::<RenderNode>::new();
    let mut purpose_indices = BTreeMap::<String, usize>::new();
    let mut remapped_ids = BTreeMap::<String, AnalysisNodeId>::new();

    for node in nodes {
        let group_key = node
            .capability_id
            .as_deref()
            .filter(|capability| groups_parallel_experts(Some(capability)))
            .map(str::to_string);
        if let Some(index) = group_key
            .as_ref()
            .and_then(|key| purpose_indices.get(key))
            .copied()
        {
            let target = &mut grouped[index];
            for member in node.members {
                remapped_ids.insert(member.id.as_str().to_string(), target.id.clone());
                for model in &member.model_ids {
                    if !target.model_ids.contains(model) {
                        target.model_ids.push(model.clone());
                    }
                }
                target.members.push(member);
            }
            for output in node.terminal_outputs {
                if !target.terminal_outputs.contains(&output) {
                    target.terminal_outputs.push(output);
                }
            }
            target.state = aggregate_member_state(&target.members);
            continue;
        }

        let index = grouped.len();
        if let Some(key) = group_key {
            purpose_indices.insert(key, index);
        }
        for member in &node.members {
            remapped_ids.insert(member.id.as_str().to_string(), node.id.clone());
        }
        grouped.push(node);
    }

    for node in &mut grouped {
        if node.members.len() > 1 {
            node.detail = format!("{} models · {}", node.model_ids.len(), node.detail);
            node.state = aggregate_member_state(&node.members);
        }
    }
    let edges = edges
        .into_iter()
        .filter_map(|mut edge| {
            edge.from = remapped_ids
                .get(edge.from.as_str())
                .cloned()
                .unwrap_or(edge.from);
            edge.to = remapped_ids
                .get(edge.to.as_str())
                .cloned()
                .unwrap_or(edge.to);
            (edge.from != edge.to).then_some(edge)
        })
        .collect();
    RenderGraph {
        nodes: grouped,
        edges,
    }
}

/// Builds a graph from the compiled Workflow wire snapshot. `exact_plan` is
/// present for queued/live/history Engine runs and supplies request/profile
/// selection states. For an editable current workflow, the persisted policy
/// and quality mode provide the truthful local preview state.
pub(crate) fn build_workflow_render_graph(
    workflow: &app_core::WorkflowExecutionWireV1,
    exact_plan: Option<&app_core::WorkflowExecutionPlanWireV1>,
    exact_engine_capabilities: Option<&std::collections::BTreeSet<String>>,
    run_completed: bool,
) -> RenderGraph {
    let capabilities = app_core::list_workflow_capabilities()
        .into_iter()
        .map(|capability| (capability.id.to_string(), capability))
        .collect::<BTreeMap<_, _>>();
    let exact_nodes = exact_plan
        .map(|plan| {
            plan.nodes
                .iter()
                .map(|node| (node.instance_id.as_str(), node))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let split_invocations = workflow
        .nodes
        .iter()
        .filter(|node| node.capability_id == "audio.separate_vocal_bgm")
        .map(|node| {
            (
                node.instance_id.as_str(),
                node.execution_invocations.as_slice(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let legacy_split_nodes = workflow
        .nodes
        .iter()
        .filter(|node| {
            node.capability_id == "audio.separate_vocal_bgm"
                && node.execution_invocations.is_empty()
        })
        .map(|node| node.instance_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let nodes = workflow
        .nodes
        .iter()
        .flat_map(|node| {
            let capability = capabilities.get(&node.capability_id);
            let exact_node = exact_nodes.get(node.instance_id.as_str()).copied();
            let mut state = exact_node
                .map(|node| exact_state(node.execution_state))
                .unwrap_or_else(|| local_state(node, &workflow.quality_mode));
            if state == GraphNodeState::Waiting
                && exact_engine_capabilities.is_some_and(|planned| {
                    exact_node.is_some_and(|node| {
                        !node.capabilities.is_empty()
                            && node
                                .capabilities
                                .iter()
                                .all(|capability| !planned.contains(capability))
                    })
                })
            {
                state = GraphNodeState::NotRequested;
            }
            if run_completed && state == GraphNodeState::Waiting {
                state = GraphNodeState::Complete;
            }
            let terminal_outputs = |port: Option<&str>| {
                workflow
                    .terminal_outputs
                    .iter()
                    .filter(|output| {
                        output.node == node.instance_id
                            && port.is_none_or(|port| output.port == port)
                    })
                    .map(|output| RenderTerminalOutput {
                        port: output.port.clone(),
                        semantic_type: output.semantic_type.clone(),
                        audio_role: output.audio_role.clone(),
                    })
                    .collect()
            };
            if node.capability_id == "audio.separate_vocal_bgm" {
                if node.execution_invocations.is_empty() {
                    return [
                        (
                            "vocal",
                            "audio.extract_vocals",
                            node.provider_preferences.primary.as_deref(),
                            "Vocal extraction",
                        ),
                        (
                            "instrumental",
                            "audio.extract_instrumental",
                            node.provider_preferences.instrumental.as_deref(),
                            "BGM / Instrumental extraction",
                        ),
                    ]
                    .into_iter()
                    .map(|(port, concrete_capability, provider, label)| {
                        let id = AnalysisNodeId::new(format!("{}.{}", node.instance_id, port));
                        let model_ids = vec![provider.unwrap_or("engine-resolved").to_string()];
                        let concrete_state = if exact_engine_capabilities
                            .is_some_and(|planned| !planned.contains(concrete_capability))
                        {
                            GraphNodeState::NotRequested
                        } else {
                            state
                        };
                        RenderNode {
                            id: id.clone(),
                            kind: RenderNodeKind::Compute,
                            label: label.to_string(),
                            model_ids: model_ids.clone(),
                            detail: format!(
                                "{} · priority {}",
                                policy_label(&node.execution_policy),
                                node.priority
                            ),
                            state: concrete_state,
                            category: GraphNodeCategory::Audio,
                            capability_id: Some(concrete_capability.to_string()),
                            terminal_outputs: terminal_outputs(Some(port)),
                            members: vec![RenderNodeMember {
                                id,
                                model_ids,
                                state: concrete_state,
                            }],
                        }
                    })
                    .collect();
                }
                return node
                    .execution_invocations
                    .iter()
                    .map(|invocation| {
                        let planned = exact_engine_capabilities.is_none_or(|planned| {
                            invocation
                                .capabilities
                                .iter()
                                .any(|capability| planned.contains(capability))
                        });
                        let concrete_state = if planned {
                            state
                        } else {
                            GraphNodeState::NotRequested
                        };
                        let combined = invocation.output_ports.len() > 1;
                        let label = if combined {
                            "Vocal / Instrumental separation"
                        } else if invocation.output_ports.iter().any(|port| port == "vocal") {
                            "Vocal extraction"
                        } else {
                            "BGM / Instrumental extraction"
                        };
                        RenderNode {
                            id: AnalysisNodeId::new(&invocation.invocation_id),
                            kind: RenderNodeKind::Compute,
                            label: label.to_string(),
                            model_ids: vec![invocation.provider_id.clone()],
                            detail: format!(
                                "{} · priority {}",
                                policy_label(&node.execution_policy),
                                node.priority
                            ),
                            state: concrete_state,
                            category: GraphNodeCategory::Audio,
                            capability_id: invocation.capabilities.first().cloned(),
                            terminal_outputs: invocation
                                .output_ports
                                .iter()
                                .flat_map(|port| terminal_outputs(Some(port)))
                                .collect(),
                            members: vec![RenderNodeMember {
                                id: AnalysisNodeId::new(&invocation.invocation_id),
                                model_ids: vec![invocation.provider_id.clone()],
                                state: concrete_state,
                            }],
                        }
                    })
                    .collect();
            }
            let model = node
                .provider_preferences
                .primary
                .as_deref()
                .unwrap_or("native DSP");
            let id = AnalysisNodeId::new(&node.instance_id);
            let model_ids = vec![model.to_string()];
            vec![RenderNode {
                id: id.clone(),
                kind: RenderNodeKind::Compute,
                label: capability
                    .map(|capability| capability.label.clone())
                    .unwrap_or_else(|| node.instance_id.clone()),
                model_ids: model_ids.clone(),
                detail: format!(
                    "{} · priority {}",
                    policy_label(&node.execution_policy),
                    node.priority
                ),
                state,
                category: workflow_graph_category(Some(&node.capability_id)),
                capability_id: Some(node.capability_id.clone()),
                terminal_outputs: terminal_outputs(None),
                members: vec![RenderNodeMember {
                    id,
                    model_ids,
                    state,
                }],
            }]
        })
        .collect::<Vec<_>>();

    let inactive_nodes = nodes
        .iter()
        .filter(|node| {
            matches!(
                node.state,
                GraphNodeState::Disabled
                    | GraphNodeState::ProfileSkipped
                    | GraphNodeState::NotRequested
            )
        })
        .map(|node| node.id.as_str().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let edges: Vec<RenderEdge> = workflow
        .bindings
        .iter()
        .flat_map(|binding| {
            let role = if !binding.execution_active {
                RenderEdgeRole::InactiveBinding
            } else if binding.analyzer_attachment {
                RenderEdgeRole::AnalyzerAttachment
            } else {
                RenderEdgeRole::ComputeDependency
            };
            // A confirmed split node's real render nodes are named by
            // invocation id (or, for the legacy empty-invocations shape,
            // by `{instance_id}.{port}`) -- never by the bare workflow
            // instance id. Falling through to `binding.from_node` for a
            // split node whose compiled invocations don't cover this
            // binding's port would create an edge to a node id that does
            // not exist in `nodes`, which corrupts the topological sort
            // and blanks the entire DAG instead of just this one edge
            // (real repro: a binding on the instrumental port when this
            // run's exact plan only compiled a vocal-only separation
            // invocation). Only nodes that are not split nodes at all may
            // use the raw instance id.
            let from = match split_invocations.get(binding.from_node.as_str()) {
                Some(invocations) if !invocations.is_empty() => invocations
                    .iter()
                    .find(|invocation| {
                        invocation
                            .output_ports
                            .iter()
                            .any(|port| port == &binding.from_port)
                    })
                    .map(|invocation| invocation.invocation_id.clone()),
                Some(_) => legacy_split_nodes
                    .contains(binding.from_node.as_str())
                    .then(|| format!("{}.{}", binding.from_node, binding.from_port)),
                None => Some(binding.from_node.clone()),
            };
            let Some(from) = from else {
                return Vec::new();
            };
            let targets = split_invocations
                .get(binding.to_node.as_str())
                .map(|invocations| {
                    invocations
                        .iter()
                        .map(|invocation| invocation.invocation_id.clone())
                        .collect()
                })
                .filter(|targets: &Vec<String>| !targets.is_empty())
                .or_else(|| {
                    legacy_split_nodes
                        .contains(binding.to_node.as_str())
                        .then(|| {
                            vec![
                                format!("{}.vocal", binding.to_node),
                                format!("{}.instrumental", binding.to_node),
                            ]
                        })
                })
                .unwrap_or_else(|| vec![binding.to_node.clone()]);
            targets
                .into_iter()
                .map(|target| {
                    let concrete_role =
                        if inactive_nodes.contains(&from) || inactive_nodes.contains(&target) {
                            RenderEdgeRole::InactiveBinding
                        } else {
                            role
                        };
                    RenderEdge {
                        from: AnalysisNodeId::new(&from),
                        from_port: binding.from_port.clone(),
                        to: AnalysisNodeId::new(target),
                        to_port: binding.to_port.clone(),
                        semantic_type: binding.semantic_type.clone(),
                        audio_role: binding.audio_role.clone(),
                        role: concrete_role,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let disabled = nodes
        .iter()
        .filter(|node| node.state == GraphNodeState::Disabled)
        .map(|node| node.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let nodes = nodes
        .into_iter()
        .filter(|node| !disabled.contains(&node.id))
        .collect();
    let edges = edges
        .into_iter()
        .filter(|edge| !disabled.contains(&edge.from) && !disabled.contains(&edge.to))
        .collect();
    group_parallel_expert_nodes(nodes, edges)
}

pub(crate) fn overlay_workflow_runtime(graph: &mut RenderGraph, task: &app_core::AnalysisTask) {
    let Some(live) = task.live.as_ref() else {
        return;
    };

    for route in &live.stage_routes {
        let Some(node_id) = route.node_id.as_deref() else {
            continue;
        };
        let Some(node) = graph.nodes.iter_mut().find(|node| {
            node.members
                .iter()
                .any(|member| member.id.as_str() == node_id)
        }) else {
            continue;
        };
        let Some(member) = node
            .members
            .iter_mut()
            .find(|member| member.id.as_str() == node_id)
        else {
            continue;
        };
        match route.node_event.as_deref() {
            Some("node_failed") => member.state = GraphNodeState::Failed,
            Some("node_cancelled" | "cancelled") => member.state = GraphNodeState::Cancelled,
            Some("node_completed" | "artifact_reused") => {
                member.state = GraphNodeState::Complete;
            }
            _ if route.finished_at_ms.is_some() => member.state = GraphNodeState::Complete,
            _ => {}
        }
        node.state = aggregate_member_state(&node.members);
    }

    if matches!(
        live.node_event.as_deref(),
        Some("cancelled" | "node_cancelled")
    ) {
        for node in &mut graph.nodes {
            for member in &mut node.members {
                if matches!(
                    member.state,
                    GraphNodeState::Waiting | GraphNodeState::Running | GraphNodeState::Deferred
                ) {
                    member.state = GraphNodeState::Cancelled;
                }
            }
            node.state = aggregate_member_state(&node.members);
        }
        return;
    }

    if matches!(task.status, app_core::QueuedStatus::Analyzing(_))
        && let Some(node_id) = live.node_id.as_deref()
        && let Some(node) = graph.nodes.iter_mut().find(|node| {
            node.members
                .iter()
                .any(|member| member.id.as_str() == node_id)
        })
    {
        if let Some(member) = node
            .members
            .iter_mut()
            .find(|member| member.id.as_str() == node_id)
        {
            member.state = GraphNodeState::Running;
        }
        node.state = aggregate_member_state(&node.members);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wire(definition: &app_core::WorkflowDefinition) -> app_core::WorkflowExecutionWireV1 {
        let snapshot = app_core::compile_workflow(definition).unwrap();
        app_core::WorkflowExecutionWireV1::from_snapshot(&snapshot).unwrap()
    }

    #[test]
    fn duplicate_reorder_and_attachment_are_rendered_from_exact_bindings() {
        let mut definition = app_core::default_workflow("song");
        let original = app_core::WorkflowNodeId::new("vocal_cleanup_1");
        app_core::set_workflow_execution_policy(
            &mut definition,
            &original,
            app_core::ExecutionPolicy::Always,
        )
        .unwrap();
        let duplicate =
            app_core::duplicate_audio_transformation(&mut definition, &original).unwrap();
        app_core::reorder_audio_transformation(&mut definition, &duplicate, true).unwrap();
        let graph = build_workflow_render_graph(&wire(&definition), None, None, false);
        let original = AnalysisNodeId::new("vocal_cleanup_1");
        let duplicate = AnalysisNodeId::new(format!("{duplicate}"));
        assert!(graph.node(&original).is_some());
        assert!(graph.node(&duplicate).is_some());
        let edge = graph
            .edges
            .iter()
            .find(|edge| edge.from == duplicate && edge.to == original)
            .expect("duplicate binding is rendered");
        assert!(!edge.from_port.is_empty());
        assert!(!edge.to_port.is_empty());
        assert!(!edge.semantic_type.is_empty());
        let attachment = graph
            .edges
            .iter()
            .find(|edge| edge.role == RenderEdgeRole::AnalyzerAttachment)
            .expect("compiled analyzer attachment is preserved");
        assert!(!attachment.from_port.is_empty());
        assert!(!attachment.to_port.is_empty());
        assert!(!attachment.semantic_type.is_empty());
    }

    fn runtime_task(routes: serde_json::Value, live_node_id: &str) -> app_core::AnalysisTask {
        let live = serde_json::from_value(json!({
            "stage": "shared display text",
            "overall_progress": 50,
            "stage_progress": 50,
            "operation": "running",
            "detail": "",
            "implementation": "native",
            "model": "model",
            "device": "vulkan",
            "requested_device": "vulkan",
            "fallback_from": null,
            "fallback_reason": null,
            "backend_fallback_from": null,
            "backend_fallback_reason": null,
            "stage_routes": routes,
            "node_id": live_node_id
        }))
        .unwrap();
        app_core::AnalysisTask {
            file_hash: "song".to_string(),
            title: "Song".to_string(),
            artist: "Artist".to_string(),
            status: app_core::QueuedStatus::Analyzing(50),
            live: Some(live),
        }
    }

    fn runtime_route(node_id: Option<&str>, event: &str) -> serde_json::Value {
        json!({
            "stage": "shared display text",
            "node_id": node_id,
            "node_event": event,
            "operation": event,
            "implementation": "native",
            "model": "model",
            "stage_progress": 100,
            "requested_device": "vulkan",
            "actual_device": "vulkan",
            "fallback_from": null,
            "fallback_reason": null,
            "backend_fallback_from": null,
            "backend_fallback_reason": null,
            "finished_at_ms": 2
        })
    }

    #[test]
    fn multi_model_separation_card_expands_to_one_dag_node_per_model_execution() {
        let workflow = wire(&app_core::default_workflow("song"));
        let graph = build_workflow_render_graph(&workflow, None, None, false);
        let vocal = graph
            .node(&AnalysisNodeId::new("vocal_bgm_split.vocal"))
            .unwrap();
        let instrumental = graph
            .node(&AnalysisNodeId::new("vocal_bgm_split.instrumental"))
            .unwrap();
        assert_eq!(vocal.model_ids, ["bs_roformer_leap_xe90_vocals"]);
        assert_eq!(
            instrumental.model_ids,
            ["bs_polarformer_public_instrumental"]
        );
        assert!(
            graph
                .node(&AnalysisNodeId::new("vocal_bgm_split"))
                .is_none()
        );
    }

    #[test]
    fn parallel_experts_with_the_same_purpose_share_one_model_card() {
        let workflow = wire(&app_core::default_workflow("song"));
        let graph = build_workflow_render_graph(&workflow, None, None, false);

        let boundary = graph
            .nodes
            .iter()
            .find(|node| node.capability_id.as_deref() == Some("analysis.note_boundary"))
            .expect("the boundary experts own one grouped purpose card");
        assert_eq!(
            boundary.capability_id.as_deref(),
            Some("analysis.note_boundary")
        );
        assert_eq!(boundary.members.len(), 5);
        let models = boundary
            .model_ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            models,
            std::collections::BTreeSet::from([
                "game",
                "basic_pitch",
                "rosvot",
                "stars",
                "jbm555_cectc_80"
            ])
        );
        assert_eq!(
            graph
                .nodes
                .iter()
                .filter(|node| { node.capability_id.as_deref() == Some("analysis.note_boundary") })
                .count(),
            1
        );

        let pitch = graph
            .nodes
            .iter()
            .find(|node| node.capability_id.as_deref() == Some("analysis.pitch_f0"))
            .expect("continuous-pitch experts share one purpose card");
        assert_eq!(
            pitch
                .model_ids
                .iter()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from(["rmvpe", "fcpe"])
        );
        assert_eq!(pitch.members.len(), 2);
    }

    #[test]
    fn a_binding_on_a_port_no_compiled_invocation_produced_is_dropped_not_dangling() {
        // Real repro: an exact plan whose separation invocation only
        // produced the vocal port (e.g. instrumental wasn't requested this
        // run) still has the abstract workflow's binding to the
        // instrumental port. That binding used to fall through to the bare
        // `vocal_bgm_split` instance id, which isn't a real render node
        // once invocations are non-empty -- a dangling edge that corrupts
        // the DAG's topological sort and blanks the whole canvas.
        let mut workflow = wire(&app_core::default_workflow("song"));
        let separation = workflow
            .nodes
            .iter_mut()
            .find(|node| node.instance_id == "vocal_bgm_split")
            .unwrap();
        separation.execution_invocations = vec![app_core::WorkflowExecutionInvocationWireV1 {
            invocation_id: "vocal-only-invocation".to_string(),
            provider_id: "vocal-only-provider".to_string(),
            capabilities: vec!["audio.extract_vocals".to_string()],
            output_ports: vec!["vocal".to_string()],
        }];
        let graph = build_workflow_render_graph(&workflow, None, None, false);
        assert!(
            graph
                .node(&AnalysisNodeId::new("vocal-only-invocation"))
                .is_some()
        );
        assert!(
            graph
                .node(&AnalysisNodeId::new("vocal_bgm_split"))
                .is_none()
        );
        let node_ids = graph
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for edge in &graph.edges {
            assert!(
                node_ids.contains(&edge.from),
                "dangling edge from {:?}",
                edge.from
            );
            assert!(
                node_ids.contains(&edge.to),
                "dangling edge to {:?}",
                edge.to
            );
        }
    }

    #[test]
    fn one_typed_dual_output_invocation_renders_as_one_execution_card() {
        let mut workflow = wire(&app_core::default_workflow("song"));
        let separation = workflow
            .nodes
            .iter_mut()
            .find(|node| node.instance_id == "vocal_bgm_split")
            .unwrap();
        separation.execution_invocations = vec![app_core::WorkflowExecutionInvocationWireV1 {
            invocation_id: "vocal_bgm_split".to_string(),
            provider_id: "dual-output-provider".to_string(),
            capabilities: vec![
                "audio.extract_vocals".to_string(),
                "audio.extract_instrumental".to_string(),
            ],
            output_ports: vec!["vocal".to_string(), "instrumental".to_string()],
        }];
        let graph = build_workflow_render_graph(&workflow, None, None, false);
        let combined = graph
            .node(&AnalysisNodeId::new("vocal_bgm_split"))
            .expect("one invocation renders one card");
        assert!(combined.label.contains("Vocal / Instrumental"));
        let mut output_ports = graph
            .edges
            .iter()
            .filter(|edge| edge.from.as_str() == "vocal_bgm_split")
            .map(|edge| edge.from_port.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        output_ports.extend(
            combined
                .terminal_outputs
                .iter()
                .map(|output| output.port.as_str()),
        );
        assert!(output_ports.contains("vocal"));
        assert!(output_ports.contains("instrumental"));
        assert!(
            graph
                .node(&AnalysisNodeId::new("vocal_bgm_split.vocal"))
                .is_none()
        );
        assert!(
            graph
                .node(&AnalysisNodeId::new("vocal_bgm_split.instrumental"))
                .is_none()
        );
    }

    #[test]
    fn independent_provider_routes_keep_independent_progress_and_failure_state() {
        let workflow = wire(&app_core::default_workflow("song"));
        let mut graph = build_workflow_render_graph(&workflow, None, None, false);
        let mut task = runtime_task(
            json!([
                runtime_route(Some("vocal_bgm_split.vocal"), "node_completed"),
                runtime_route(Some("vocal_bgm_split.instrumental"), "node_failed")
            ]),
            "vocal_bgm_split.instrumental",
        );
        task.status = app_core::QueuedStatus::Queued;
        overlay_workflow_runtime(&mut graph, &task);
        assert_eq!(
            graph
                .node(&AnalysisNodeId::new("vocal_bgm_split.vocal"))
                .unwrap()
                .state,
            GraphNodeState::Complete
        );
        assert_eq!(
            graph
                .node(&AnalysisNodeId::new("vocal_bgm_split.instrumental"))
                .unwrap()
                .state,
            GraphNodeState::Failed
        );
    }

    #[test]
    fn unplanned_separation_model_is_not_marked_complete() {
        let workflow = wire(&app_core::default_workflow("song"));
        let planned = [
            "audio.decode".to_string(),
            "audio.extract_vocals".to_string(),
        ]
        .into_iter()
        .collect();
        let graph = build_workflow_render_graph(&workflow, None, Some(&planned), true);
        assert_eq!(
            graph
                .node(&AnalysisNodeId::new("vocal_bgm_split.vocal"))
                .unwrap()
                .state,
            GraphNodeState::Complete
        );
        let instrumental = AnalysisNodeId::new("vocal_bgm_split.instrumental");
        assert_eq!(
            graph.node(&instrumental).unwrap().state,
            GraphNodeState::NotRequested
        );
        assert!(graph.edges.iter().any(|edge| {
            edge.to == instrumental && edge.role == RenderEdgeRole::InactiveBinding
        }));
    }

    #[test]
    fn runtime_overlay_uses_only_exact_node_ids() {
        let workflow = wire(&app_core::default_workflow("song"));
        let mut graph = build_workflow_render_graph(&workflow, None, None, false);
        let ids = graph
            .nodes
            .iter()
            .take(4)
            .map(|node| node.id.as_str().to_string())
            .collect::<Vec<_>>();
        let untouched_before = graph.node(&AnalysisNodeId::new(&ids[3])).unwrap().state;
        let task = runtime_task(
            json!([
                runtime_route(Some(&ids[0]), "node_completed"),
                runtime_route(Some(&ids[1]), "node_failed"),
                runtime_route(None, "node_completed")
            ]),
            &ids[2],
        );

        overlay_workflow_runtime(&mut graph, &task);

        assert_eq!(
            graph.node(&AnalysisNodeId::new(&ids[0])).unwrap().state,
            GraphNodeState::Complete
        );
        assert_eq!(
            graph.node(&AnalysisNodeId::new(&ids[1])).unwrap().state,
            GraphNodeState::Failed
        );
        assert_eq!(
            graph.node(&AnalysisNodeId::new(&ids[2])).unwrap().state,
            GraphNodeState::Running
        );
        assert_eq!(
            graph.node(&AnalysisNodeId::new(&ids[3])).unwrap().state,
            untouched_before,
            "a route without node_id must not match shared display text"
        );
    }

    #[test]
    fn historical_routes_overlay_events_without_marking_the_last_node_running() {
        let workflow = wire(&app_core::default_workflow("song"));
        let mut graph = build_workflow_render_graph(&workflow, None, None, false);
        let ids = graph
            .nodes
            .iter()
            .take(2)
            .map(|node| node.id.as_str().to_string())
            .collect::<Vec<_>>();
        let mut task = runtime_task(
            json!([runtime_route(Some(&ids[0]), "node_completed")]),
            &ids[1],
        );
        task.status = app_core::QueuedStatus::Queued;

        overlay_workflow_runtime(&mut graph, &task);

        assert_eq!(
            graph.node(&AnalysisNodeId::new(&ids[0])).unwrap().state,
            GraphNodeState::Complete
        );
        assert_ne!(
            graph.node(&AnalysisNodeId::new(&ids[1])).unwrap().state,
            GraphNodeState::Running
        );
    }

    #[test]
    fn cancelled_run_marks_every_unfinished_dag_node_cancelled() {
        let workflow = wire(&app_core::default_workflow("song"));
        let mut graph = build_workflow_render_graph(&workflow, None, None, false);
        let ids = graph
            .nodes
            .iter()
            .take(2)
            .map(|node| node.id.as_str().to_string())
            .collect::<Vec<_>>();
        let mut task = runtime_task(
            json!([runtime_route(Some(&ids[0]), "node_completed")]),
            &ids[1],
        );
        task.status = app_core::QueuedStatus::Queued;
        task.live.as_mut().unwrap().node_event = Some("cancelled".to_string());

        overlay_workflow_runtime(&mut graph, &task);

        assert_eq!(
            graph.node(&AnalysisNodeId::new(&ids[0])).unwrap().state,
            GraphNodeState::Complete
        );
        assert!(
            graph.nodes.iter().any(|node| {
                node.id.as_str() != ids[0] && node.state == GraphNodeState::Cancelled
            })
        );
        assert!(graph.nodes.iter().all(|node| {
            !matches!(
                node.state,
                GraphNodeState::Waiting | GraphNodeState::Running | GraphNodeState::Deferred
            )
        }));
    }

    #[test]
    fn disabled_nodes_are_absent_without_ghost_edges_while_conditional_states_remain() {
        let mut definition = app_core::default_workflow("song");
        app_core::set_workflow_execution_policy(
            &mut definition,
            &app_core::WorkflowNodeId::new("boundary_stars"),
            app_core::ExecutionPolicy::Disabled,
        )
        .unwrap();
        let graph = build_workflow_render_graph(&wire(&definition), None, None, false);
        assert!(graph.node(&AnalysisNodeId::new("boundary_stars")).is_none());
        assert!(graph.edges.iter().all(|edge| {
            edge.from.as_str() != "boundary_stars" && edge.to.as_str() != "boundary_stars"
        }));
        let pitch = graph
            .nodes
            .iter()
            .find(|node| node.capability_id.as_deref() == Some("analysis.pitch_f0"))
            .unwrap();
        assert_eq!(
            pitch
                .members
                .iter()
                .find(|member| member.id.as_str() == "f0_fcpe")
                .unwrap()
                .state,
            GraphNodeState::Deferred
        );
        app_core::set_workflow_execution_policy(
            &mut definition,
            &app_core::WorkflowNodeId::new("boundary_stars"),
            app_core::ExecutionPolicy::Conditional {
                condition: app_core::ConditionalExecution::MaximumOnly,
            },
        )
        .unwrap();
        let restored = build_workflow_render_graph(&wire(&definition), None, None, false);
        let boundary = restored
            .nodes
            .iter()
            .find(|node| node.capability_id.as_deref() == Some("analysis.note_boundary"))
            .expect("re-enabling restores the model inside its purpose card");
        let stars = boundary
            .members
            .iter()
            .find(|member| member.id.as_str() == "boundary_stars")
            .unwrap();
        assert_eq!(stars.model_ids, ["stars"]);
        assert_eq!(stars.state, GraphNodeState::ProfileSkipped);
        let terminal = graph
            .nodes
            .iter()
            .flat_map(|node| &node.terminal_outputs)
            .next()
            .expect("compiled terminal outputs remain visible");
        assert!(!terminal.port.is_empty());
        assert!(!terminal.semantic_type.is_empty());
    }
}
