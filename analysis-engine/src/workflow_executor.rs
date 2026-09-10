// Copyright 2026 Uta! Studio contributors
// Licensed under the Apache License, Version 2.0.

//! Backend-owned execution projection for a compiled Processing Studio workflow.
//!
//! This module is deliberately independent from app-core. It turns the
//! validated wire snapshot into a deterministic backend schedule. Dependency
//! edges determine legality; priority only chooses between simultaneously ready
//! nodes. Conditional nodes remain explicitly deferred until the conditional
//! scheduler authorizes them.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::contract::{AnalysisProfile, EngineError, EngineErrorCode, EngineResult};
use crate::execution::CancellationToken;
use crate::workflow::{
    ExpertFusionPolicy, FusionMode, WorkflowBinding, WorkflowExecution,
    WorkflowExecutionInvocation, WorkflowExecutionPolicy, engine_capabilities,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPlanIdentity {
    pub contract: String,
    pub version: u32,
    pub workflow_schema_version: u32,
    pub workflow_id: String,
    pub workflow_revision: u64,
    pub definition_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeExecutionState {
    Ready,
    Deferred,
    Disabled,
    ProfileSkipped,
    NotRequested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionNodePlan {
    pub instance_id: String,
    pub analysis_node: String,
    pub capabilities: Vec<String>,
    pub execution_policy: WorkflowExecutionPolicy,
    pub execution_state: WorkflowNodeExecutionState,
    pub priority: i32,
    #[serde(default)]
    pub parameters: serde_json::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_invocations: Vec<WorkflowExecutionInvocation>,
    pub depends_on: Vec<String>,
    pub input_bindings: Vec<WorkflowBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledWorkflowExecutionPlan {
    pub identity: WorkflowPlanIdentity,
    pub nodes: Vec<WorkflowExecutionNodePlan>,
    pub terminal_outputs: Vec<crate::workflow::WorkflowTerminalOutput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fusion_policy: Option<ExpertFusionPolicy>,
    /// Exact Stage 4 decision intent from the validated workflow snapshot.
    /// This field is deliberately required when decoding an Engine Plan so a
    /// missing backend projection cannot masquerade as Algorithm mode.
    pub fusion_mode: FusionMode,
}

impl CompiledWorkflowExecutionPlan {
    pub fn compile(
        workflow: &WorkflowExecution,
        profile: AnalysisProfile,
        requested_capabilities: Option<&BTreeSet<String>>,
        force_lead_output: bool,
    ) -> EngineResult<Self> {
        let by_analysis_node = workflow
            .nodes
            .iter()
            .map(|node| (node.instance_id.as_str(), node))
            .collect::<BTreeMap<_, _>>();
        let mut indegree = workflow
            .nodes
            .iter()
            .map(|node| (node.instance_id.as_str(), 0usize))
            .collect::<BTreeMap<_, _>>();
        let mut outgoing: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for binding in &workflow.bindings {
            if outgoing
                .entry(binding.from_node.as_str())
                .or_default()
                .insert(binding.to_node.as_str())
            {
                *indegree.entry(binding.to_node.as_str()).or_default() += 1;
            }
        }

        let mut ordered: Vec<&str> = Vec::with_capacity(workflow.nodes.len());
        let mut scheduled = BTreeSet::new();
        while ordered.len() < workflow.nodes.len() {
            let next = indegree
                .iter()
                .filter(|(node, degree)| **degree == 0 && !scheduled.contains(**node))
                .map(|(node, _)| *node)
                .max_by(|left, right| {
                    let left = by_analysis_node[left];
                    let right = by_analysis_node[right];
                    left.priority
                        .cmp(&right.priority)
                        .then_with(|| right.instance_id.cmp(&left.instance_id))
                })
                .ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::InvalidContract,
                        "compiled workflow contains an unschedulable dependency cycle",
                    )
                })?;
            ordered.push(next);
            scheduled.insert(next);
            if let Some(children) = outgoing.get(next) {
                for child in children {
                    let degree = indegree
                        .get_mut(child)
                        .expect("validated workflow child exists");
                    *degree = degree.saturating_sub(1);
                }
            }
        }

        let nodes = ordered
            .into_iter()
            .map(|analysis_node| {
                let node = by_analysis_node[analysis_node];
                let capabilities = engine_capabilities(node)
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let requested = requested_capabilities.is_none_or(|requested| {
                    capabilities
                        .iter()
                        .any(|capability| requested.contains(capability))
                });
                let forced_lead_output = force_lead_output
                    && capabilities
                        .iter()
                        .any(|capability| capability == "audio.lead_isolate");
                let execution_state = match node.execution_policy {
                    _ if forced_lead_output => WorkflowNodeExecutionState::Ready,
                    WorkflowExecutionPolicy::Disabled => WorkflowNodeExecutionState::Disabled,
                    _ if !requested => WorkflowNodeExecutionState::NotRequested,
                    WorkflowExecutionPolicy::Always => WorkflowNodeExecutionState::Ready,
                    WorkflowExecutionPolicy::MaximumOnly if profile != AnalysisProfile::Maximum => {
                        WorkflowNodeExecutionState::ProfileSkipped
                    }
                    WorkflowExecutionPolicy::MaximumOnly => WorkflowNodeExecutionState::Ready,
                    WorkflowExecutionPolicy::OnDisagreement
                    | WorkflowExecutionPolicy::DisagreementWindows => {
                        WorkflowNodeExecutionState::Deferred
                    }
                };
                let mut input_bindings = workflow
                    .bindings
                    .iter()
                    .filter(|binding| binding.to_node == node.instance_id)
                    .cloned()
                    .collect::<Vec<_>>();
                input_bindings.sort_by(|left, right| {
                    (&left.to_port, &left.from_node, &left.from_port).cmp(&(
                        &right.to_port,
                        &right.from_node,
                        &right.from_port,
                    ))
                });
                let depends_on = input_bindings
                    .iter()
                    .map(|binding| binding.from_node.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let execution_invocations = node
                    .execution_invocations
                    .iter()
                    .filter(|invocation| {
                        requested_capabilities.is_none_or(|requested| {
                            invocation.capabilities.iter().any(|capability| {
                                requested.contains(capability)
                                    || (forced_lead_output && capability == "audio.lead_isolate")
                            })
                        })
                    })
                    .cloned()
                    .collect();
                WorkflowExecutionNodePlan {
                    instance_id: node.instance_id.clone(),
                    analysis_node: node.instance_id.clone(),
                    capabilities,
                    execution_policy: node.execution_policy,
                    execution_state,
                    priority: node.priority,
                    parameters: workflow.engine_resolved_parameters(node),
                    execution_invocations,
                    depends_on,
                    input_bindings,
                }
            })
            .collect();

        Ok(Self {
            identity: WorkflowPlanIdentity {
                contract: workflow.contract.clone(),
                version: workflow.version,
                workflow_schema_version: workflow.workflow_schema_version,
                workflow_id: workflow.workflow_id.clone(),
                workflow_revision: workflow.workflow_revision,
                definition_digest: workflow.definition_digest.clone(),
            },
            nodes,
            terminal_outputs: workflow.terminal_outputs.clone(),
            fusion_policy: workflow.resolved_expert_fusion_policy(profile),
            fusion_mode: workflow.fusion_mode(),
        })
    }

    pub fn node_for_capability(&self, capability: &str) -> Option<&WorkflowExecutionNodePlan> {
        self.nodes
            .iter()
            .filter(|node| node.capabilities.iter().any(|item| item == capability))
            .max_by_key(|node| {
                let state_rank = match node.execution_state {
                    WorkflowNodeExecutionState::Ready => 4,
                    WorkflowNodeExecutionState::Deferred => 3,
                    WorkflowNodeExecutionState::ProfileSkipped => 2,
                    WorkflowNodeExecutionState::NotRequested => 1,
                    WorkflowNodeExecutionState::Disabled => 0,
                };
                (state_rank, node.priority)
            })
    }

    pub fn ready_nodes_for_capability(
        &self,
        capability: &str,
    ) -> impl Iterator<Item = &WorkflowExecutionNodePlan> {
        self.nodes.iter().filter(move |node| {
            node.execution_state == WorkflowNodeExecutionState::Ready
                && node.capabilities.iter().any(|item| item == capability)
        })
    }

    /// CPU-only deterministic execution seam used by contract tests and by
    /// future capability adapters. Results remain staged until every ready
    /// node completes, so cancellation never publishes a partial terminal set.
    pub fn execute_control_plane<T, F>(
        &self,
        cancellation: &CancellationToken,
        mut execute: F,
    ) -> EngineResult<Vec<(String, T)>>
    where
        F: FnMut(&WorkflowExecutionNodePlan) -> EngineResult<T>,
    {
        let mut staged = Vec::new();
        for node in &self.nodes {
            if node.execution_state != WorkflowNodeExecutionState::Ready {
                continue;
            }
            if cancellation.is_cancelled() {
                return Err(EngineError::new(
                    EngineErrorCode::Cancelled,
                    "compiled workflow execution was cancelled between nodes",
                ));
            }
            staged.push((node.analysis_node.clone(), execute(node)?));
        }
        if cancellation.is_cancelled() {
            return Err(EngineError::new(
                EngineErrorCode::Cancelled,
                "compiled workflow execution was cancelled before publication",
            ));
        }
        Ok(staged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::AudioRole;
    use crate::contract::request::tests::valid_request;
    use crate::workflow::{WORKFLOW_EXECUTION_EXTENSION_KEY, WorkflowExecution};

    fn workflow() -> WorkflowExecution {
        let mut request = valid_request(AudioRole::OriginalMix);
        request.requested_artifacts.vocal_chart = false;
        request.requested_artifacts.singing_analysis = false;
        request.requested_artifacts.transcript = false;
        request.requested_artifacts.alignment = false;
        request.extensions.insert(
            WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
            serde_json::json!({
                "contract":"uta.workflow-execution",
                "version":1,
                "workflow_schema_version": crate::workflow::WORKFLOW_SCHEMA_VERSION,
                "workflow_id":"workflow:test",
                "workflow_revision":7,
                "quality_mode":"balanced",
                "definition_digest":"a".repeat(32),
                "nodes":[
                    {"instance_id":"source","capability_id":"audio.source","execution_policy":"always","priority":100},
                    {"instance_id":"split","capability_id":"audio.separate_vocal_bgm","provider_preferences":{"primary":"bs_roformer_leap_xe90_vocals","instrumental":"bs_roformer_leap_xe90_vocals"},"execution_invocations":[{"invocation_id":"split","provider_id":"bs_roformer_leap_xe90_vocals","capabilities":["audio.extract_vocals","audio.extract_instrumental"],"output_ports":["vocal","instrumental"]}],"execution_policy":"always","priority":90},
                    {"instance_id":"lead","capability_id":"audio.lead_isolate","provider_preferences":{"primary":"melband_roformer_harmony"},"execution_policy":"always","priority":80},
                    {"instance_id":"denoise-a","capability_id":"audio.denoise","provider_preferences":{"primary":"melband_roformer_denoise_aufr33"},"execution_policy":"always","priority":20},
                    {"instance_id":"denoise-b","capability_id":"audio.denoise","provider_preferences":{"primary":"melband_roformer_denoise_aufr33"},"execution_policy":"always","priority":10},
                    {"instance_id":"pitch","capability_id":"analysis.pitch_f0","provider_preferences":{"primary":"rmvpe"},"execution_policy":"always","priority":30}
                ],
                "bindings":[
                    {"from_node":"source","from_port":"mix","to_node":"split","to_port":"audio","semantic_type":"audio","audio_role":"source_mix","execution_active":true,"analyzer_attachment":false},
                    {"from_node":"split","from_port":"vocal","to_node":"lead","to_port":"audio","semantic_type":"audio","audio_role":"vocal","execution_active":true,"analyzer_attachment":false},
                    {"from_node":"lead","from_port":"lead","to_node":"denoise-a","to_port":"audio","semantic_type":"audio","audio_role":"lead_vocal","execution_active":true,"analyzer_attachment":false},
                    {"from_node":"denoise-a","from_port":"audio","to_node":"denoise-b","to_port":"audio","semantic_type":"audio","audio_role":"lead_vocal","execution_active":true,"analyzer_attachment":false},
                    {"from_node":"denoise-b","from_port":"audio","to_node":"pitch","to_port":"audio","semantic_type":"audio","audio_role":"lead_vocal","execution_active":true,"analyzer_attachment":true}
                ],
                "terminal_outputs":[{"node":"pitch","port":"pitch","semantic_type":"pitch_evidence"}]
            }),
        );
        WorkflowExecution::from_request(&request).unwrap().unwrap()
    }

    #[test]
    fn dependencies_order_nodes_and_duplicate_instances_remain_distinct() {
        let plan = CompiledWorkflowExecutionPlan::compile(
            &workflow(),
            AnalysisProfile::Balanced,
            None,
            false,
        )
        .unwrap();
        let order = plan
            .nodes
            .iter()
            .map(|node| node.instance_id.as_str())
            .collect::<Vec<_>>();
        assert!(
            order.iter().position(|id| *id == "denoise-a").unwrap()
                < order.iter().position(|id| *id == "denoise-b").unwrap()
        );
        assert_eq!(plan.ready_nodes_for_capability("audio.denoise").count(), 2);
        assert_eq!(plan.fusion_mode, FusionMode::Algorithm);
        assert_eq!(
            plan.node_for_capability("pitch.track")
                .unwrap()
                .execution_state,
            WorkflowNodeExecutionState::Ready
        );
    }

    #[test]
    fn compiled_plan_preserves_explicit_ai_judgment_mode() {
        let mut workflow = workflow();
        workflow.fusion_mode = FusionMode::AiJudgment;
        let plan = CompiledWorkflowExecutionPlan::compile(
            &workflow,
            AnalysisProfile::Balanced,
            None,
            false,
        )
        .unwrap();
        assert_eq!(plan.fusion_mode, FusionMode::AiJudgment);
    }

    #[test]
    fn compiled_plan_preserves_typed_provider_invocation_topology() {
        let mut workflow = workflow();
        let split = workflow
            .nodes
            .iter_mut()
            .find(|node| node.instance_id == "split")
            .unwrap();
        split.execution_invocations = vec![crate::workflow::WorkflowExecutionInvocation {
            invocation_id: "split.dual".to_string(),
            provider_id: "dual-output-provider".to_string(),
            capabilities: vec![
                "audio.extract_vocals".to_string(),
                "audio.extract_instrumental".to_string(),
            ],
            output_ports: vec!["vocal".to_string(), "instrumental".to_string()],
        }];
        let plan = CompiledWorkflowExecutionPlan::compile(
            &workflow,
            AnalysisProfile::Balanced,
            None,
            false,
        )
        .unwrap();
        let invocation = &plan
            .nodes
            .iter()
            .find(|node| node.instance_id == "split")
            .unwrap()
            .execution_invocations[0];
        assert_eq!(invocation.invocation_id, "split.dual");
        assert_eq!(invocation.provider_id, "dual-output-provider");
        assert_eq!(invocation.output_ports, ["vocal", "instrumental"]);

        let split = workflow
            .nodes
            .iter_mut()
            .find(|node| node.instance_id == "split")
            .unwrap();
        split
            .execution_invocations
            .push(crate::workflow::WorkflowExecutionInvocation {
                invocation_id: "split.instrumental-only".to_string(),
                provider_id: "instrumental-provider".to_string(),
                capabilities: vec!["audio.extract_instrumental".to_string()],
                output_ports: vec!["instrumental".to_string()],
            });
        let requested = BTreeSet::from(["audio.extract_vocals".to_string()]);
        let partial = CompiledWorkflowExecutionPlan::compile(
            &workflow,
            AnalysisProfile::Balanced,
            Some(&requested),
            false,
        )
        .unwrap();
        let invocations = &partial
            .nodes
            .iter()
            .find(|node| node.instance_id == "split")
            .unwrap()
            .execution_invocations;
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].invocation_id, "split.dual");
        assert_eq!(
            invocations[0].capabilities,
            ["audio.extract_vocals", "audio.extract_instrumental"]
        );
        assert_eq!(invocations[0].output_ports, ["vocal", "instrumental"]);
    }

    #[test]
    fn explicit_lead_output_overrides_conditional_state_without_changing_authored_policy() {
        let mut workflow = workflow();
        workflow
            .nodes
            .iter_mut()
            .find(|node| node.capability_id == "audio.lead_isolate")
            .unwrap()
            .execution_policy = WorkflowExecutionPolicy::OnDisagreement;
        let requested = BTreeSet::from(["audio.lead_isolate".to_string()]);
        let conditional = CompiledWorkflowExecutionPlan::compile(
            &workflow,
            AnalysisProfile::Balanced,
            Some(&requested),
            false,
        )
        .unwrap();
        assert_eq!(
            conditional
                .node_for_capability("audio.lead_isolate")
                .unwrap()
                .execution_state,
            WorkflowNodeExecutionState::Deferred
        );
        let forced = CompiledWorkflowExecutionPlan::compile(
            &workflow,
            AnalysisProfile::Balanced,
            Some(&requested),
            true,
        )
        .unwrap();
        let lead = forced.node_for_capability("audio.lead_isolate").unwrap();
        assert_eq!(lead.execution_state, WorkflowNodeExecutionState::Ready);
        assert_eq!(
            lead.execution_policy,
            WorkflowExecutionPolicy::OnDisagreement
        );
    }

    #[test]
    fn analyzer_attachment_and_priority_are_truthful() {
        let plan = CompiledWorkflowExecutionPlan::compile(
            &workflow(),
            AnalysisProfile::Balanced,
            None,
            false,
        )
        .unwrap();
        let pitch = plan.node_for_capability("pitch.track").unwrap();
        assert_eq!(pitch.input_bindings.len(), 1);
        assert_eq!(pitch.input_bindings[0].from_node, "denoise-b");
        assert!(pitch.input_bindings[0].analyzer_attachment);

        assert!(
            pitch
                .depends_on
                .iter()
                .any(|dependency| dependency.as_str().contains("denoise-b"))
        );
    }

    #[test]
    fn cancellation_discards_staged_control_plane_results() {
        let plan = CompiledWorkflowExecutionPlan::compile(
            &workflow(),
            AnalysisProfile::Balanced,
            None,
            false,
        )
        .unwrap();
        let cancellation = CancellationToken::default();
        let trigger = cancellation.clone();
        let mut calls = 0usize;
        let error = plan
            .execute_control_plane(&cancellation, |_| {
                calls += 1;
                if calls == 2 {
                    trigger.cancel();
                }
                Ok(calls)
            })
            .unwrap_err();
        assert_eq!(error.code, EngineErrorCode::Cancelled);
        assert_eq!(calls, 2);
    }
}
