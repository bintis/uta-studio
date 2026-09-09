mod capability;
mod compiler;
mod default_definition;
mod definition;
mod snapshot;
mod types;
mod validation;
mod wire;

pub use capability::*;
pub use compiler::*;
pub use default_definition::*;
pub use definition::*;
pub use snapshot::*;
pub use types::*;
pub use validation::*;
pub use wire::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_default_workflow_validates_and_compiles() {
        let workflow = default_workflow("song-a");
        let report = validate_workflow(&workflow);
        assert!(report.is_valid(), "{:?}", report.issues);
        let snapshot = compile_workflow(&workflow).expect("compile default workflow");
        assert!(snapshot.graph.validate().is_ok());
        assert!(snapshot.node_bindings.iter().any(|binding| {
            binding.capability_id.as_str() == "finalize.canonical_singing_track"
        }));
    }

    #[test]
    fn current_default_uses_only_implemented_model_providers() {
        let workflow = default_workflow("song-a");
        let models = workflow
            .nodes
            .iter()
            .filter_map(|node| node.model_id.as_deref())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            models,
            [
                "basic_pitch",
                "bs_roformer_leap_xe90_vocals",
                "fcpe",
                "firered_asr2_aed",
                "game_1_0_3_medium",
                "jbm555_cectc_80",
                "melband_roformer_denoise_aufr33",
                "melband_roformer_dereverb_anvuew",
                "melband_roformer_harmony",
                "qwen3_asr_1_7b",
                "qwen3_forced_aligner_0_6b",
                "rmvpe",
                "rosvot",
                "stars",
            ]
            .into_iter()
            .collect()
        );
        assert!(
            workflow_model_options(&CapabilityId::new("analysis.pitch_f0"), Some("rmvpe"))
                .is_empty()
        );
        assert_eq!(
            workflow_model_options(
                &CapabilityId::new("analysis.note_boundary"),
                Some("game_1_0_3_medium")
            )
            .iter()
            .map(|option| option.model_id)
            .collect::<Vec<_>>(),
            ["game_1_0_3_small", "game_1_0_3_medium", "game_1_0_3_large"]
        );
    }

    #[test]
    fn schema_four_migration_removes_an_unavailable_provider() {
        let mut stored = StoredWorkflow {
            definition: default_workflow("song-a"),
            layout: WorkflowLayout::default(),
            updated_at_ms: 0,
        };
        stored.definition.schema_version = 4;
        stored.definition.nodes.push(WorkflowNodeInstance {
            instance_id: WorkflowNodeId::new("retired"),
            capability_id: CapabilityId::new("analysis.asr"),
            model_id: Some("retired-provider".to_string()),
            separation_strategy: None,
            parameters: Default::default(),
            execution_policy: ExecutionPolicy::Always,
            priority: 1,
            skip_if_unchanged: false,
        });
        stored.definition.analyzer_bindings.push(AnalyzerBinding {
            analyzer_node: WorkflowNodeId::new("retired"),
            source: WorkflowPortRef {
                node: WorkflowNodeId::new("vocal_dereverb_1"),
                port: "audio".to_string(),
            },
            analyzer_input: "audio".to_string(),
        });
        migrate_stored_workflow(&mut stored).unwrap();
        assert_eq!(stored.definition.schema_version, WORKFLOW_SCHEMA_VERSION);
        assert!(
            stored
                .definition
                .nodes
                .iter()
                .all(|node| node.instance_id.as_str() != "retired")
        );
        assert!(compile_workflow(&stored.definition).is_ok());
    }

    #[test]
    fn schema_five_migration_adds_the_validated_fcpe_secondary() {
        let mut stored = StoredWorkflow {
            definition: default_workflow("song-a"),
            layout: WorkflowLayout::default(),
            updated_at_ms: 0,
        };
        stored.definition.schema_version = 5;
        stored
            .definition
            .nodes
            .retain(|node| node.model_id.as_deref() != Some("fcpe"));
        stored.definition.edges.retain(|edge| {
            edge.from.node.as_str() != "f0_fcpe" && edge.to.node.as_str() != "f0_fcpe"
        });
        stored
            .definition
            .analyzer_bindings
            .retain(|binding| binding.analyzer_node.as_str() != "f0_fcpe");

        migrate_stored_workflow(&mut stored).unwrap();

        let fcpe = stored
            .definition
            .nodes
            .iter()
            .find(|node| node.model_id.as_deref() == Some("fcpe"))
            .expect("migration adds FCPE");
        assert_eq!(
            fcpe.execution_policy,
            ExecutionPolicy::Conditional {
                condition: ConditionalExecution::MaximumOnly
            }
        );
        assert!(compile_workflow(&stored.definition).is_ok());
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
    fn duplicate_singleton_capability_is_rejected() {
        let mut workflow = default_workflow("song-a");
        workflow.nodes.push(WorkflowNodeInstance {
            instance_id: WorkflowNodeId::new("second_source"),
            capability_id: CapabilityId::new("audio.source"),
            model_id: None,
            separation_strategy: None,
            parameters: Default::default(),
            execution_policy: ExecutionPolicy::Always,
            priority: 0,
            skip_if_unchanged: false,
        });
        let report = validate_workflow(&workflow);
        assert!(
            report.issues.iter().any(|issue| {
                issue.code == WorkflowValidationCode::DuplicateSingletonCapability
            })
        );
    }

    #[test]
    fn separation_strategies_keep_one_dual_output_invocation() {
        let mut workflow = default_workflow("song-a");
        for (strategy, provider) in [
            (
                SeparationStrategyV1::LeapDualOutput,
                "bs_roformer_leap_xe90_vocals",
            ),
            (
                SeparationStrategyV1::LeapInstrumentalDirect,
                "bs_roformer_leap_xe90_instrumental",
            ),
            (
                SeparationStrategyV1::PolarformerBoth,
                "bs_polarformer_public_instrumental",
            ),
        ] {
            set_workflow_separation_strategy(
                &mut workflow,
                &WorkflowNodeId::new("vocal_bgm_split"),
                strategy,
            )
            .unwrap();
            let snapshot = compile_workflow(&workflow).unwrap();
            let wire = WorkflowExecutionWireV1::from_snapshot(&snapshot).unwrap();
            let separation = wire
                .nodes
                .iter()
                .find(|node| node.instance_id == "vocal_bgm_split")
                .unwrap();
            assert_eq!(separation.execution_invocations.len(), 1);
            assert_eq!(separation.execution_invocations[0].provider_id, provider);
            assert_eq!(
                separation.execution_invocations[0].output_ports,
                ["vocal", "instrumental"]
            );
        }
    }

    #[test]
    fn rmvpe_is_the_required_continuous_pitch_provider() {
        let mut workflow = default_workflow("song-a");
        let error = set_workflow_execution_policy(
            &mut workflow,
            &WorkflowNodeId::new("f0_rmvpe"),
            ExecutionPolicy::Disabled,
        )
        .unwrap_err();
        assert!(error.contains("RMVPE") || error.contains("pitch"));
    }
}
