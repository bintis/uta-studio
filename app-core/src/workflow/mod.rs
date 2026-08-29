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
            separation_strategy: None,
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
        let mut workflow = default_workflow("song-a");
        let duplicate =
            duplicate_audio_transformation(&mut workflow, &WorkflowNodeId::new("vocal_dereverb_1"))
                .unwrap();
        assert_ne!(duplicate, WorkflowNodeId::new("vocal_dereverb_1"));
        let original = WorkflowNodeId::new("vocal_dereverb_1");
        assert!(
            workflow
                .edges
                .iter()
                .any(|edge| { edge.from.node == original && edge.to.node == duplicate })
        );
        assert!(
            workflow
                .analyzer_bindings
                .iter()
                .filter(|binding| binding.source.port == "audio")
                .all(|binding| binding.source.node == duplicate)
        );

        reorder_audio_transformation(&mut workflow, &duplicate, true).unwrap();
        assert!(
            workflow
                .edges
                .iter()
                .any(|edge| { edge.from.node == duplicate && edge.to.node == original })
        );
        assert!(
            workflow
                .analyzer_bindings
                .iter()
                .filter(|binding| binding.source.port == "audio")
                .all(|binding| binding.source.node == original)
        );

        reorder_audio_transformation(&mut workflow, &duplicate, false).unwrap();
        assert!(
            workflow
                .edges
                .iter()
                .any(|edge| edge.from.node == original && edge.to.node == duplicate)
        );
        assert!(
            workflow
                .analyzer_bindings
                .iter()
                .filter(|binding| binding.source.port == "audio")
                .all(|binding| binding.source.node == duplicate)
        );
        assert!(compile_workflow(&workflow).is_ok());
    }

    #[test]
    fn insert_processor_on_bgm_branch_rewrites_real_topology() {
        let mut workflow = default_workflow("song-a");
        let inserted = insert_audio_transformation_after_output(
            &mut workflow,
            &WorkflowNodeId::new("vocal_bgm_split"),
            "instrumental",
            &CapabilityId::new("audio.denoise"),
            Some("melband_roformer_denoise_aufr33".to_string()),
        )
        .unwrap();
        assert!(workflow.edges.iter().any(|edge| {
            edge.from.node == WorkflowNodeId::new("vocal_bgm_split")
                && edge.from.port == "instrumental"
                && edge.to.node == inserted
                && edge.to.port == "audio"
        }));
        let snapshot = compile_workflow(&workflow).unwrap();
        assert!(
            snapshot
                .node_bindings
                .iter()
                .any(|node| node.workflow_node == inserted)
        );
    }

    #[test]
    fn deleting_audio_processor_bypasses_it_and_rebinds_analyzers() {
        let mut workflow = default_workflow("song-a");
        remove_workflow_node(&mut workflow, &WorkflowNodeId::new("vocal_cleanup_1")).unwrap();
        assert!(
            !workflow
                .nodes
                .iter()
                .any(|node| node.instance_id.as_str() == "vocal_cleanup_1")
        );
        assert!(
            workflow
                .analyzer_bindings
                .iter()
                .all(|binding| binding.source.node.as_str() == "vocal_dereverb_1")
        );
        assert!(compile_workflow(&workflow).is_ok());
    }

    #[test]
    fn deleting_game_uses_the_engine_owned_f0_fallback() {
        let mut workflow = default_workflow("song-a");
        remove_workflow_node(&mut workflow, &WorkflowNodeId::new("boundary_game")).unwrap();
        assert_eq!(
            expert_fusion_policy(&workflow).unwrap().note_lengths,
            BoundaryFusionPolicyV1::F0Derived
        );
        let fusion = workflow
            .nodes
            .iter()
            .find(|node| node.instance_id.as_str() == "evidence_fusion")
            .unwrap();
        assert!(!fusion.parameters.contains_key("boundary_owner"));
        assert!(compile_workflow(&workflow).is_ok());
    }

    #[test]
    fn required_asr_card_cannot_be_deleted_and_the_workflow_is_unchanged() {
        let mut workflow = default_workflow("song-a");
        let before = workflow.clone();
        let error =
            remove_workflow_node(&mut workflow, &WorkflowNodeId::new("asr_qwen")).unwrap_err();
        assert!(error.contains("required"));
        assert_eq!(workflow, before);
    }

    #[test]
    fn optional_stage_two_and_three_cards_can_be_deleted_and_added_back() {
        let mut workflow = default_workflow("song-a");
        set_workflow_execution_policy(
            &mut workflow,
            &WorkflowNodeId::new("asr_firered"),
            ExecutionPolicy::Disabled,
        )
        .unwrap();
        remove_workflow_node(&mut workflow, &WorkflowNodeId::new("asr_firered")).unwrap();
        remove_workflow_node(&mut workflow, &WorkflowNodeId::new("f0_fcpe")).unwrap();
        assert!(!workflow_has_optional_card(
            &workflow,
            OptionalWorkflowCardV1::FireRedTranscript
        ));
        let source = WorkflowPortRef {
            node: WorkflowNodeId::new("vocal_cleanup_1"),
            port: "audio".to_string(),
        };
        let firered = add_optional_workflow_card(
            &mut workflow,
            source.clone(),
            OptionalWorkflowCardV1::FireRedTranscript,
        )
        .unwrap();
        let fcpe =
            add_optional_workflow_card(&mut workflow, source, OptionalWorkflowCardV1::FcpePitch)
                .unwrap();
        assert_eq!(firered.as_str(), "asr_firered");
        assert_eq!(fcpe.as_str(), "f0_fcpe");
        assert!(compile_workflow(&workflow).is_ok());
    }

    #[test]
    fn optional_game_card_is_restored_disabled_until_the_user_selects_it() {
        let mut workflow = default_workflow("song-a");
        set_workflow_execution_policy(
            &mut workflow,
            &WorkflowNodeId::new("boundary_game"),
            ExecutionPolicy::Disabled,
        )
        .unwrap();
        remove_workflow_node(&mut workflow, &WorkflowNodeId::new("boundary_game")).unwrap();
        let restored = add_optional_workflow_card(
            &mut workflow,
            WorkflowPortRef {
                node: WorkflowNodeId::new("vocal_cleanup_1"),
                port: "audio".to_string(),
            },
            OptionalWorkflowCardV1::GameBoundary,
        )
        .unwrap();
        assert_eq!(restored.as_str(), "boundary_game");
        assert_eq!(
            workflow
                .nodes
                .iter()
                .find(|node| node.instance_id == restored)
                .unwrap()
                .execution_policy,
            ExecutionPolicy::Disabled
        );
        assert!(compile_workflow(&workflow).is_ok());
    }

    #[test]
    fn selectable_pitch_models_swap_stage_three_participation_without_owner_state() {
        let mut workflow = default_workflow("song-a");
        set_workflow_node_model(&mut workflow, &WorkflowNodeId::new("f0_rmvpe"), "fcpe").unwrap();
        assert_eq!(
            workflow
                .nodes
                .iter()
                .find(|node| node.instance_id.as_str() == "f0_rmvpe")
                .and_then(|node| node.model_id.as_deref()),
            Some("fcpe")
        );
        assert_eq!(
            workflow
                .nodes
                .iter()
                .find(|node| node.instance_id.as_str() == "f0_fcpe")
                .and_then(|node| node.model_id.as_deref()),
            Some("rmvpe")
        );
        let fusion = workflow
            .nodes
            .iter()
            .find(|node| node.instance_id.as_str() == "evidence_fusion")
            .unwrap();
        assert!(!fusion.parameters.contains_key("pitch_owner"));
        let snapshot = compile_workflow(&workflow).unwrap();
        assert_eq!(
            snapshot
                .node_bindings
                .iter()
                .find(|binding| binding.workflow_node.as_str() == "f0_rmvpe")
                .and_then(|binding| binding.model_id.as_deref()),
            Some("fcpe")
        );
    }

    #[test]
    fn separation_strategies_round_trip_with_truthful_invocation_counts() {
        let mut workflow = default_workflow("song-a");
        let node_id = WorkflowNodeId::new("vocal_bgm_split");
        let independent =
            WorkflowExecutionWireV1::from_snapshot(&compile_workflow(&workflow).unwrap()).unwrap();
        assert_eq!(
            independent
                .nodes
                .iter()
                .find(|node| node.instance_id == "vocal_bgm_split")
                .unwrap()
                .execution_invocations
                .len(),
            2
        );

        set_workflow_separation_strategy(
            &mut workflow,
            &node_id,
            SeparationStrategyV1::Ep317VocalResidual,
        )
        .unwrap();
        let json = serde_json::to_string(&workflow).unwrap();
        let round_tripped: WorkflowDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(
            round_tripped
                .nodes
                .iter()
                .find(|node| node.instance_id == node_id)
                .unwrap()
                .separation_strategy,
            Some(SeparationStrategyV1::Ep317VocalResidual)
        );
        let dual =
            WorkflowExecutionWireV1::from_snapshot(&compile_workflow(&round_tripped).unwrap())
                .unwrap();
        let invocation = &dual
            .nodes
            .iter()
            .find(|node| node.instance_id == "vocal_bgm_split")
            .unwrap()
            .execution_invocations;
        assert_eq!(invocation.len(), 1);
        assert_eq!(invocation[0].provider_id, "bs_roformer_vocals_ep317");
        assert_eq!(
            invocation[0].capabilities,
            ["audio.extract_vocals", "audio.extract_instrumental"]
        );
    }

    #[test]
    fn legacy_separation_fields_migrate_to_the_independent_strategy() {
        let mut stored = StoredWorkflow {
            definition: default_workflow("legacy-song"),
            layout: WorkflowLayout::default(),
            updated_at_ms: 0,
        };
        let separation = stored
            .definition
            .nodes
            .iter_mut()
            .find(|node| node.instance_id.as_str() == "vocal_bgm_split")
            .unwrap();
        separation.separation_strategy = None;
        separation.parameters.insert(
            "instrumental_model_id".to_string(),
            serde_json::json!("melband_roformer_inst_v2"),
        );
        migrate_stored_workflow(&mut stored).unwrap();
        let separation = stored
            .definition
            .nodes
            .iter()
            .find(|node| node.instance_id.as_str() == "vocal_bgm_split")
            .unwrap();
        assert_eq!(
            separation.separation_strategy,
            Some(SeparationStrategyV1::IndependentSpecialists)
        );
        assert!(!separation.parameters.contains_key("instrumental_model_id"));
    }

    #[test]
    fn invalid_model_selection_is_rejected_without_mutation() {
        let mut workflow = default_workflow("song-a");
        let before = workflow.clone();
        let error =
            set_workflow_node_model(&mut workflow, &WorkflowNodeId::new("f0_rmvpe"), "game")
                .unwrap_err();
        assert!(error.contains("not selectable"));
        assert_eq!(workflow, before);
    }

    #[test]
    fn game_can_move_between_stage_three_cards_without_step_four_rewrites() {
        let mut workflow = default_workflow("song-a");
        set_workflow_node_model(
            &mut workflow,
            &WorkflowNodeId::new("boundary_basic_pitch"),
            "game",
        )
        .unwrap();
        assert!(
            workflow
                .nodes
                .iter()
                .find(|node| node.instance_id.as_str() == "evidence_fusion")
                .is_some_and(|node| !node.parameters.contains_key("boundary_owner"))
        );
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
    fn compiled_snapshot_keeps_exact_artifact_roles_policy_and_priority() {
        let workflow = default_workflow("song-a");
        let snapshot = compile_workflow(&workflow).unwrap();
        let rmvpe = snapshot
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node.as_str() == "f0_rmvpe")
            .unwrap();
        assert_eq!(rmvpe.model_id.as_deref(), Some("rmvpe"));
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
            WorkflowPortType::Audio(AudioRole::Vocal)
        );
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
    fn legacy_back_vocal_role_migrates_without_aliasing_harmony() {
        let legacy: AudioRole = serde_json::from_str("\"back_vocal\"").unwrap();
        assert_eq!(legacy, AudioRole::BackingVocal);
        assert_eq!(serde_json::to_string(&legacy).unwrap(), "\"backing_vocal\"");
        assert_ne!(legacy, AudioRole::HarmonyVocal);
        assert!(
            !WorkflowPortType::Audio(AudioRole::BackingVocal)
                .accepts(&WorkflowPortType::Audio(AudioRole::HarmonyVocal))
        );

        let mut stored = StoredWorkflow {
            definition: default_workflow("legacy-song"),
            layout: WorkflowLayout::default(),
            updated_at_ms: 7,
        };
        stored.definition.schema_version = 1;
        migrate_stored_workflow(&mut stored).unwrap();
        assert_eq!(stored.definition.schema_version, WORKFLOW_SCHEMA_VERSION);
    }

    #[test]
    fn future_lead_partition_is_not_advertised_as_an_executable_workflow_node() {
        assert!(
            builtin_capabilities()
                .iter()
                .all(|capability| capability.id.as_str() != "audio.lead_partition")
        );
        let workflow = default_workflow("song-a");
        assert!(
            workflow
                .nodes
                .iter()
                .all(|node| node.capability_id.as_str() != "audio.lead_partition")
        );
    }

    #[test]
    fn disabled_optional_expert_is_retained_but_removed_from_execution_edges() {
        let mut workflow = default_workflow("song-a");
        set_workflow_execution_policy(
            &mut workflow,
            &WorkflowNodeId::new("boundary_stars"),
            ExecutionPolicy::Disabled,
        )
        .unwrap();
        let snapshot = compile_workflow(&workflow).unwrap();
        let stars = snapshot
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node.as_str() == "boundary_stars")
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

    #[test]
    fn disabling_selected_primary_f0_is_blocked_without_promotion() {
        let mut workflow = default_workflow("song-a");
        let before = workflow.clone();
        let error = set_workflow_execution_policy(
            &mut workflow,
            &WorkflowNodeId::new("f0_rmvpe"),
            ExecutionPolicy::Disabled,
        )
        .unwrap_err();
        assert!(error.contains("continuous F0") || error.contains("pitch"));
        assert_eq!(workflow, before);
    }

    #[test]
    fn disabling_game_resolves_the_engine_owned_f0_fallback() {
        let mut workflow = default_workflow("song-a");
        set_workflow_execution_policy(
            &mut workflow,
            &WorkflowNodeId::new("boundary_game"),
            ExecutionPolicy::Disabled,
        )
        .unwrap();
        assert_eq!(
            expert_fusion_policy(&workflow).unwrap().note_lengths,
            BoundaryFusionPolicyV1::F0Derived
        );
        assert!(compile_workflow(&workflow).is_ok());
    }

    #[test]
    fn selected_continuous_f0_expert_cannot_be_disabled() {
        let mut workflow = default_workflow("song-a");
        let before = workflow.clone();
        assert!(
            set_workflow_execution_policy(
                &mut workflow,
                &WorkflowNodeId::new("f0_rmvpe"),
                ExecutionPolicy::Disabled,
            )
            .is_err()
        );
        assert_eq!(workflow, before);
    }

    #[test]
    fn lyrics_execution_stages_reject_disable() {
        let mut workflow = default_workflow("song-a");
        for node in ["asr_qwen", "transcript_fusion", "forced_alignment"] {
            assert!(
                set_workflow_execution_policy(
                    &mut workflow,
                    &WorkflowNodeId::new(node),
                    ExecutionPolicy::Disabled,
                )
                .is_err(),
                "{node} must remain required"
            );
        }
    }

    #[test]
    fn disabled_vocal_cleanup_is_transparently_bypassed_and_can_be_reenabled() {
        let mut workflow = default_workflow("song-a");
        let cleanup_id = WorkflowNodeId::new("vocal_cleanup_1");
        set_workflow_execution_policy(&mut workflow, &cleanup_id, ExecutionPolicy::Disabled)
            .unwrap();
        let bypassed = compile_workflow(&workflow).unwrap();
        let cleanup = bypassed
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node == cleanup_id)
            .unwrap();
        let split = bypassed
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node.as_str() == "vocal_bgm_split")
            .unwrap();
        let rmvpe = bypassed
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node.as_str() == "f0_rmvpe")
            .unwrap();
        assert!(bypassed.artifact_bindings.iter().any(|binding| {
            binding.execution_active
                && binding.analyzer_attachment
                && binding.from_node == split.analysis_node
                && binding.from_port == "vocal"
                && binding.to_node == rmvpe.analysis_node
        }));
        assert!(
            bypassed
                .artifact_bindings
                .iter()
                .filter(|binding| {
                    binding.from_node == cleanup.analysis_node
                        || binding.to_node == cleanup.analysis_node
                })
                .all(|binding| !binding.execution_active)
        );

        set_workflow_execution_policy(&mut workflow, &cleanup_id, ExecutionPolicy::Always).unwrap();
        let restored = compile_workflow(&workflow).unwrap();
        let cleanup = restored
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node == cleanup_id)
            .unwrap();
        let rmvpe = restored
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node.as_str() == "f0_rmvpe")
            .unwrap();
        assert!(restored.artifact_bindings.iter().any(|binding| {
            binding.execution_active
                && binding.analyzer_attachment
                && binding.from_node == cleanup.analysis_node
                && binding.to_node == rmvpe.analysis_node
        }));
    }

    #[test]
    fn all_optional_singing_experts_can_be_disabled_together() {
        let mut workflow = default_workflow("song-a");
        for node in [
            "f0_fcpe",
            "boundary_game",
            "boundary_basic_pitch",
            "boundary_rosvot",
            "boundary_stars",
            "technique_stars",
            "acoustic_dsp",
        ] {
            set_workflow_execution_policy(
                &mut workflow,
                &WorkflowNodeId::new(node),
                ExecutionPolicy::Disabled,
            )
            .unwrap_or_else(|error| panic!("{node} should disable: {error}"));
        }
        assert_eq!(
            expert_fusion_policy(&workflow).unwrap().note_lengths,
            BoundaryFusionPolicyV1::F0Derived
        );
        assert!(compile_workflow(&workflow).is_ok());
    }

    #[test]
    fn lead_isolation_and_cleanup_can_both_be_disabled_with_vocal_bypass() {
        let mut workflow = default_workflow("song-a");
        for node in ["lead_isolate", "vocal_cleanup_1"] {
            set_workflow_execution_policy(
                &mut workflow,
                &WorkflowNodeId::new(node),
                ExecutionPolicy::Disabled,
            )
            .unwrap_or_else(|error| panic!("{node} should bypass: {error}"));
        }
        let snapshot = compile_workflow(&workflow).unwrap();
        let split = snapshot
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node.as_str() == "vocal_bgm_split")
            .unwrap();
        let rmvpe = snapshot
            .node_bindings
            .iter()
            .find(|binding| binding.workflow_node.as_str() == "f0_rmvpe")
            .unwrap();
        assert!(snapshot.artifact_bindings.iter().any(|binding| {
            binding.execution_active
                && binding.analyzer_attachment
                && binding.from_node == split.analysis_node
                && binding.from_port == "vocal"
                && binding.to_node == rmvpe.analysis_node
                && binding.port_type == WorkflowPortType::Audio(AudioRole::Vocal)
        }));
        for disabled in ["lead_isolate", "vocal_cleanup_1"] {
            let node = snapshot
                .node_bindings
                .iter()
                .find(|binding| binding.workflow_node.as_str() == disabled)
                .unwrap();
            assert!(
                snapshot
                    .artifact_bindings
                    .iter()
                    .filter(|binding| {
                        binding.from_node == node.analysis_node
                            || binding.to_node == node.analysis_node
                    })
                    .all(|binding| !binding.execution_active)
            );
        }
    }

    #[test]
    fn legacy_owner_parameters_are_rejected_without_stage_three_mutation() {
        for key in ["pitch_owner", "boundary_owner", "onset_owner"] {
            let mut workflow = default_workflow("song-a");
            let before = workflow.clone();
            let error = set_workflow_parameter(
                &mut workflow,
                &WorkflowNodeId::new("evidence_fusion"),
                key,
                serde_json::Value::String("legacy".to_string()),
            )
            .unwrap_err();
            assert!(error.contains("legacy Engine-internal"));
            assert_eq!(workflow, before);
        }
    }

    #[test]
    fn migration_drops_all_legacy_owners_without_changing_stage_three() {
        let mut stored = StoredWorkflow {
            definition: default_workflow("song-a"),
            layout: WorkflowLayout::default(),
            updated_at_ms: 0,
        };
        let policies = stored
            .definition
            .nodes
            .iter()
            .map(|node| (node.instance_id.clone(), node.execution_policy.clone()))
            .collect::<Vec<_>>();
        let fusion = stored
            .definition
            .nodes
            .iter_mut()
            .find(|node| node.instance_id.as_str() == "evidence_fusion")
            .unwrap();
        for key in ["pitch_owner", "boundary_owner", "onset_owner"] {
            fusion.parameters.insert(
                key.to_string(),
                serde_json::Value::String("legacy".to_string()),
            );
        }
        migrate_stored_workflow(&mut stored).unwrap();
        let fusion = stored
            .definition
            .nodes
            .iter()
            .find(|node| node.instance_id.as_str() == "evidence_fusion")
            .unwrap();
        assert!(
            ["pitch_owner", "boundary_owner", "onset_owner"]
                .iter()
                .all(|key| !fusion.parameters.contains_key(*key))
        );
        assert_eq!(
            stored
                .definition
                .nodes
                .iter()
                .map(|node| (node.instance_id.clone(), node.execution_policy.clone()))
                .collect::<Vec<_>>(),
            policies
        );
    }

    #[test]
    fn stage_three_evidence_can_change_without_step_four_owner_state() {
        let mut workflow = default_workflow("song-a");
        set_workflow_execution_policy(
            &mut workflow,
            &WorkflowNodeId::new("acoustic_dsp"),
            ExecutionPolicy::Disabled,
        )
        .unwrap();
        let fusion = workflow
            .nodes
            .iter()
            .find(|node| node.instance_id.as_str() == "evidence_fusion")
            .unwrap();
        assert_eq!(fusion_mode(&workflow), FusionModeV1::Algorithm);
        assert!(
            ["pitch_owner", "boundary_owner", "onset_owner"]
                .iter()
                .all(|key| !fusion.parameters.contains_key(*key))
        );
    }
}
