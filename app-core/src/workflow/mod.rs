mod capability;
mod compiler;
mod definition;
mod migration;
mod snapshot;
mod types;
mod validation;

pub use capability::*;
pub use compiler::*;
pub use definition::*;
pub use migration::*;
pub use snapshot::*;
pub use types::*;
pub use validation::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrated_default_workflow_validates_and_compiles() {
        let workflow = default_workflow("song-a");
        let report = validate_workflow(&workflow);
        assert!(report.is_valid(), "{:?}", report.issues);
        let snapshot = compile_workflow(&workflow).expect("compile default workflow");
        assert!(snapshot.graph.validate().is_ok());
        assert!(
            snapshot
                .node_bindings
                .iter()
                .any(|binding| binding.capability_id.as_str() == "finalize.canonical_singing_track")
        );
    }

    #[test]
    fn layout_never_changes_execution_digest() {
        let workflow = default_workflow("song-a");
        let before = workflow_definition_digest(&workflow).unwrap();
        let mut layout = WorkflowLayout::default();
        layout.positions.insert(
            WorkflowNodeId::new("source"),
            NodePosition { x: 42.0, y: 99.0 },
        );
        assert!(!layout.positions.is_empty());
        assert_eq!(before, workflow_definition_digest(&workflow).unwrap());
    }

    #[test]
    fn duplicate_capability_instances_are_allowed_only_when_declared() {
        let mut workflow = default_workflow("song-a");
        workflow.nodes.push(WorkflowNodeInstance {
            instance_id: WorkflowNodeId::new("second_source"),
            capability_id: CapabilityId::new("audio.source"),
            model_id: None,
            parameters: Default::default(),
            execution_policy: ExecutionPolicy::Always,
            priority: 0,
        });
        let report = validate_workflow(&workflow);
        assert!(
            report.issues.iter().any(|issue| {
                issue.code == WorkflowValidationCode::DuplicateSingletonCapability
            })
        );
    }

    #[test]
    fn cycle_is_rejected_before_analysis_planning() {
        let mut workflow = default_workflow("song-a");
        workflow.edges.push(edge(
            "canonical_track",
            "chart",
            "transcript_fusion",
            "evidence",
        ));
        let report = validate_workflow(&workflow);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == WorkflowValidationCode::Cycle)
        );
    }

    #[test]
    fn duplicate_audio_transformation_is_a_real_second_dataflow_step() {
        let settings = crate::AudioProcessingSettings {
            vocal_cleanup_chain: vec!["native_denoise".to_string()],
            ..Default::default()
        };
        let mut workflow = workflow_from_audio_settings("song-a", &settings);
        let duplicate =
            duplicate_audio_transformation(&mut workflow, &WorkflowNodeId::new("vocal_cleanup_1"))
                .unwrap();
        assert_ne!(duplicate, WorkflowNodeId::new("vocal_cleanup_1"));
        assert!(workflow.edges.iter().any(|edge| {
            edge.from.node == WorkflowNodeId::new("vocal_cleanup_1") && edge.to.node == duplicate
        }));
        assert!(compile_workflow(&workflow).is_ok());
    }

    #[test]
    fn invalid_analyzer_rebind_is_blocked_without_mutating_the_workflow() {
        let mut workflow = default_workflow("song-a");
        let before = workflow.clone();
        let error = bind_workflow_analyzer(
            &mut workflow,
            &WorkflowNodeId::new("f0_rmvpe"),
            WorkflowPortRef {
                node: WorkflowNodeId::new("vocal_bgm_split"),
                port: "instrumental".to_string(),
            },
        )
        .unwrap_err();
        assert!(error.contains("cannot consume"));
        assert_eq!(workflow, before);
    }

    #[test]
    fn compiled_snapshot_keeps_exact_artifact_roles_policy_priority_and_runtime() {
        let workflow = default_workflow("song-a");
        let snapshot = compile_workflow(&workflow).unwrap();
        let rmvpe = snapshot
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node.as_str() == "f0_rmvpe")
            .unwrap();
        assert_eq!(rmvpe.model_id.as_deref(), Some("rmvpe"));
        assert_eq!(rmvpe.runtime, ResolvedRuntimeKind::OpenVino);
        assert_eq!(
            rmvpe.runtime_recipe_digest.as_deref(),
            Some(crate::native_runtime::OPENVINO_WORKER_RECIPE_SHA256)
        );
        assert_eq!(rmvpe.priority, 680);
        assert_eq!(rmvpe.execution_policy, ExecutionPolicy::Always);
        let attachment = snapshot
            .artifact_bindings
            .iter()
            .find(|binding| binding.to_node == rmvpe.analysis_node)
            .unwrap();
        assert!(attachment.analyzer_attachment);
        assert_eq!(
            attachment.port_type,
            WorkflowPortType::Audio(AudioRole::LeadVocal)
        );

        let qwen = snapshot
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node.as_str() == "asr_qwen")
            .unwrap();
        assert_eq!(qwen.runtime, ResolvedRuntimeKind::Unresolved);
        assert!(qwen.runtime_recipe_digest.is_some());
    }

    #[test]
    fn priority_changes_dispatch_intent_but_never_graph_dependencies() {
        let mut workflow = default_workflow("song-a");
        let before = compile_workflow(&workflow).unwrap();
        set_workflow_priority(&mut workflow, &WorkflowNodeId::new("f0_rmvpe"), -42).unwrap();
        let after = compile_workflow(&workflow).unwrap();
        assert_eq!(
            serde_json::to_value(&before.graph).unwrap(),
            serde_json::to_value(&after.graph).unwrap()
        );
        assert_ne!(before.definition_digest, after.definition_digest);
    }

    #[test]
    fn required_input_cannot_depend_only_on_conditional_experts() {
        let mut workflow = default_workflow("song-a");
        for id in ["f0_rmvpe", "f0_fcpe"] {
            workflow
                .nodes
                .iter_mut()
                .find(|node| node.instance_id.as_str() == id)
                .unwrap()
                .execution_policy = ExecutionPolicy::Conditional {
                condition: ConditionalExecution::OnDisagreement,
            };
        }
        let report = validate_workflow(&workflow);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| { issue.code == WorkflowValidationCode::ConditionalRequiredInput })
        );
    }

    #[test]
    fn disabled_optional_expert_is_retained_but_removed_from_execution_edges() {
        let mut workflow = default_workflow("song-a");
        set_workflow_execution_policy(
            &mut workflow,
            &WorkflowNodeId::new("technique_stars"),
            ExecutionPolicy::Disabled,
        )
        .unwrap();
        let snapshot = compile_workflow(&workflow).unwrap();
        let stars = snapshot
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node.as_str() == "technique_stars")
            .unwrap();
        assert!(
            snapshot
                .artifact_bindings
                .iter()
                .filter(|binding| binding.from_node == stars.analysis_node
                    || binding.to_node == stars.analysis_node)
                .all(|binding| !binding.execution_active)
        );
        assert!(
            snapshot
                .graph
                .edges
                .iter()
                .all(|edge| edge.from != stars.analysis_node && edge.to != stars.analysis_node)
        );
    }
}
