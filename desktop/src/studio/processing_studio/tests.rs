use super::node_card::{
    capability_summary, execution_policy_choices, node_execution_badge, provider_metadata,
    uses_binary_preprocessing_switch, workflow_policy_availability,
};
use super::stage_fusion::{
    FusionAdapterReadinessUi, ai_mode_label, classify_fusion_adapter_readiness,
};
use super::{
    port_label, processing_studio_scroll_max, reorderable_workflow_branch,
    reorderable_workflow_nodes, stage_renders_internal_node_cards,
};

#[test]
fn processing_studio_scroll_extent_handles_short_and_long_pages() {
    assert_eq!(processing_studio_scroll_max(600.0, 420.0), 0.0);
    assert_eq!(processing_studio_scroll_max(600.0, 600.0), 0.0);
    assert_eq!(processing_studio_scroll_max(600.0, 915.0), 315.0);
}

#[test]
fn processing_cards_report_typed_condition_not_resource_readiness() {
    let (always, _) = node_execution_badge(&app_core::ExecutionPolicy::Always);
    let (conditional, _) = node_execution_badge(&app_core::ExecutionPolicy::Conditional {
        condition: app_core::ConditionalExecution::DisagreementWindows,
    });
    let (disabled, _) = node_execution_badge(&app_core::ExecutionPolicy::Disabled);
    assert_eq!(always, "ENABLED");
    assert_eq!(conditional, "CONDITIONAL");
    assert_eq!(disabled, "DISABLED");
}

#[test]
fn capability_summaries_describe_product_roles_without_runtime_claims() {
    for (capability, expected) in [
        (
            "audio.separate_vocal_bgm",
            "Produces independent vocal and instrumental audio branches.",
        ),
        (
            "analysis.asr",
            "Transcribes the singing route into lyric evidence.",
        ),
        (
            "analysis.forced_alignment",
            "Aligns canonical lyrics to the song timeline.",
        ),
        (
            "analysis.pitch_f0",
            "Estimates the continuous singing-pitch contour.",
        ),
        (
            "analysis.note_boundary",
            "Proposes note onsets, offsets and boundaries.",
        ),
        (
            "fusion.candidate_graph",
            "Constructs the candidate singing-path graph.",
        ),
        (
            "finalize.canonical_singing_track",
            "Produces the canonical singing track.",
        ),
    ] {
        let summary = capability_summary(capability);
        assert_eq!(summary, expected);
        let lower = summary.to_ascii_lowercase();
        for forbidden in ["installed", "runtime ready", "backend available"] {
            assert!(
                !lower.contains(forbidden),
                "capability summary claims runtime readiness: {summary}"
            );
        }
    }
    assert_eq!(
        capability_summary("unknown.capability"),
        "Configures this capability in the product workflow."
    );
}

#[test]
fn preprocessing_lane_exposes_one_master_bypass_for_all_optional_processors() {
    let source = include_str!("mod.rs");
    assert!(source.contains("SetWorkflowPreprocessingEnabled"));
    assert!(source.contains("Optional cleanup"));
    assert!(source.contains("Lead isolation, denoise and dereverb share this quick switch"));
}

#[test]
fn contextual_sidebar_names_real_outputs_without_inventing_readiness_metrics() {
    let source = include_str!("workspace_sidebar.rs");
    for required in [
        "PLANNED OUTPUTS",
        "Terminal products from the current local compile snapshot.",
        "Canonical singing track",
        "Candidate singing chart",
        "Pitch evidence",
        "Note-boundary evidence",
        "Lyric alignment evidence",
        "Lead-vocal audio",
        "Instrumental audio",
        "Exact provider, backend and resource readiness is not inferred on this page.",
    ] {
        assert!(
            source.contains(required),
            "missing truthful workflow sidebar copy: {required}"
        );
    }
    for forbidden in ["readiness percentage", "success rate", "CPU usage"] {
        assert!(
            !source.contains(forbidden),
            "fabricated metric copy remains: {forbidden}"
        );
    }
}

#[test]
fn step_four_is_selector_only_and_keeps_saved_unavailable_ai_visible() {
    let source = include_str!("stage_fusion.rs");
    for required in [
        "STEP 4 · FINAL FUSION",
        "Configured evidence",
        "Potential evidence",
        "Evidence normalization -> candidate construction -> final path selection -> canonical singing track",
    ] {
        assert!(source.contains(required), "missing Step 4 copy: {required}");
    }
    for forbidden in [
        "Resolved fusion intent",
        "Enabled evidence inputs",
        "Continuous F0",
        "Note lengths",
        "Onset support",
        "duration-aware decode",
    ] {
        assert!(
            !source.contains(forbidden),
            "legacy Step 4 copy remains: {forbidden}"
        );
    }
    assert_eq!(
        ai_mode_label(true, FusionAdapterReadinessUi::Missing),
        "✓ AI judgment · adapter not configured in Models & runtime"
    );
    assert_eq!(
        ai_mode_label(false, FusionAdapterReadinessUi::Usable),
        "AI judgment"
    );
}

#[test]
fn separation_picker_exposes_only_typed_executable_strategies() {
    let options = app_core::separation_strategy_options();
    assert_eq!(options.len(), 1);
    let option = &options[0];
    assert_eq!(
        option.strategy,
        app_core::SeparationStrategyV1::IndependentSpecialists
    );
    assert_eq!(option.executions.len(), 2);
    assert_eq!(
        option.executions[0].provider_id,
        "bs_roformer_leap_xe90_vocals"
    );
    assert_eq!(
        option.executions[1].provider_id,
        "bs_polarformer_public_instrumental"
    );
    let mod_source = include_str!("mod.rs");
    let node_card_source = include_str!("node_card.rs");
    let fusion_source = include_str!("stage_fusion.rs");
    assert!(node_card_source.contains("SetWorkflowSeparationStrategy"));
    assert!(mod_source.contains("Fusion & output"));
    assert!(fusion_source.contains("STEP 4 · FINAL FUSION"));
    assert!(!mod_source.contains("04 · ENGINE FUSION POLICY"));
    assert!(!node_card_source.contains("04 · ENGINE FUSION POLICY"));
    assert!(!mod_source.contains("Choose typed ownership among evidence enabled in stage 03"));
    assert!(
        !node_card_source.contains("Choose typed ownership among evidence enabled in stage 03")
    );
}

#[test]
fn provider_is_secondary_metadata_and_plan_preview_remains_authoritative() {
    let metadata = provider_metadata(Some("rmvpe"));
    assert!(metadata.starts_with("Configured provider:"));
    assert!(metadata.contains("resolved in Plan Preview"));
    assert!(!metadata.starts_with("RMVPE"));

    let native = provider_metadata(None);
    assert!(native.contains("Studio capability logic"));
    assert!(native.contains("Plan Preview"));
}

#[test]
fn ai_judgment_requires_fresh_usable_adapter_readiness() {
    assert_eq!(
        classify_fusion_adapter_readiness(true, false, Some(true)),
        FusionAdapterReadinessUi::Checking
    );
    assert_eq!(
        classify_fusion_adapter_readiness(false, true, Some(true)),
        FusionAdapterReadinessUi::StatusError
    );
    assert_eq!(
        classify_fusion_adapter_readiness(false, false, None),
        FusionAdapterReadinessUi::Missing
    );
    assert_eq!(
        classify_fusion_adapter_readiness(false, false, Some(false)),
        FusionAdapterReadinessUi::Unusable
    );
    assert_eq!(
        classify_fusion_adapter_readiness(false, false, Some(true)),
        FusionAdapterReadinessUi::Usable
    );
}

#[test]
fn preprocessing_defaults_off_and_uses_binary_switches() {
    let workflow = app_core::default_workflow("song-a");
    let capabilities = app_core::list_workflow_capabilities()
        .into_iter()
        .map(|capability| (capability.id.clone(), capability))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert!(!uses_binary_preprocessing_switch(
        capabilities
            .get(&app_core::CapabilityId::new("audio.separate_vocal_bgm"))
            .unwrap()
    ));
    for node_id in ["lead_isolate", "vocal_cleanup_1", "vocal_dereverb_1"] {
        let node = workflow
            .nodes
            .iter()
            .find(|node| node.instance_id.as_str() == node_id)
            .unwrap();
        assert_eq!(node.execution_policy, app_core::ExecutionPolicy::Disabled);
        assert!(uses_binary_preprocessing_switch(
            capabilities.get(&node.capability_id).unwrap()
        ));
    }
}

#[test]
fn every_backend_execution_condition_is_explicitly_selectable() {
    let choices = execution_policy_choices();
    assert_eq!(choices.len(), 5);
    assert!(choices.iter().any(|(_, policy)| {
        matches!(
            policy,
            app_core::ExecutionPolicy::Conditional {
                condition: app_core::ConditionalExecution::MaximumOnly
            }
        )
    }));
    assert_eq!(choices[0].0, "Always");
    assert_eq!(choices[4].0, "Disabled");
}

#[test]
fn semantic_reorder_changes_the_processing_card_order() {
    let mut workflow = app_core::default_workflow("song-a");
    let original = app_core::WorkflowNodeId::new("vocal_cleanup_1");
    let duplicate = app_core::duplicate_audio_transformation(&mut workflow, &original).unwrap();
    let dereverb = app_core::WorkflowNodeId::new("vocal_dereverb_1");
    assert_eq!(
        reorderable_workflow_nodes(&workflow),
        [original.clone(), duplicate.clone(), dereverb.clone()]
    );

    app_core::reorder_audio_transformation(&mut workflow, &duplicate, true).unwrap();
    assert_eq!(
        reorderable_workflow_nodes(&workflow),
        [duplicate, original, dereverb]
    );
}

#[test]
fn drag_reorder_targets_are_confined_to_one_semantic_audio_branch() {
    let mut workflow = app_core::default_workflow("song-a");
    let vocal = app_core::WorkflowNodeId::new("vocal_cleanup_1");
    let vocal_copy = app_core::duplicate_audio_transformation(&mut workflow, &vocal).unwrap();
    let bgm = app_core::insert_audio_transformation_after_output(
        &mut workflow,
        &app_core::WorkflowNodeId::new("vocal_bgm_split"),
        "instrumental",
        &app_core::CapabilityId::new("audio.denoise"),
        Some("melband_roformer_denoise_aufr33".to_string()),
    )
    .unwrap();
    assert_eq!(
        reorderable_workflow_branch(&workflow, &vocal),
        [
            vocal,
            vocal_copy,
            app_core::WorkflowNodeId::new("vocal_dereverb_1")
        ]
    );
    assert_eq!(reorderable_workflow_branch(&workflow, &bgm), [bgm]);
}

#[test]
fn disabled_is_available_only_when_the_compiled_topology_allows_it() {
    let workflow = app_core::default_workflow("song-a");
    assert!(
        workflow_policy_availability(
            &workflow,
            &app_core::WorkflowNodeId::new("boundary_stars"),
            app_core::ExecutionPolicy::Disabled,
        )
        .is_ok()
    );
    assert!(
        workflow_policy_availability(
            &workflow,
            &app_core::WorkflowNodeId::new("vocal_bgm_split"),
            app_core::ExecutionPolicy::Disabled,
        )
        .is_err()
    );
}

#[test]
fn expert_disable_buttons_follow_the_real_minimum_evidence_contract() {
    let mut workflow = app_core::default_workflow("song-a");
    for node in [
        "lead_isolate",
        "vocal_cleanup_1",
        "boundary_game",
        "boundary_basic_pitch",
        "acoustic_dsp",
    ] {
        assert!(
            workflow_policy_availability(
                &workflow,
                &app_core::WorkflowNodeId::new(node),
                app_core::ExecutionPolicy::Disabled,
            )
            .is_ok(),
            "{node} should be individually disableable"
        );
    }
    assert!(
        workflow_policy_availability(
            &workflow,
            &app_core::WorkflowNodeId::new("f0_rmvpe"),
            app_core::ExecutionPolicy::Disabled,
        )
        .is_err(),
        "a sticky selected F0 expert needs an explicit policy transition"
    );
    app_core::set_workflow_execution_policy(
        &mut workflow,
        &app_core::WorkflowNodeId::new("f0_fcpe"),
        app_core::ExecutionPolicy::Always,
    )
    .unwrap();
    app_core::set_workflow_execution_policy(
        &mut workflow,
        &app_core::WorkflowNodeId::new("f0_rmvpe"),
        app_core::ExecutionPolicy::Disabled,
    )
    .unwrap();
    assert!(
        workflow_policy_availability(
            &workflow,
            &app_core::WorkflowNodeId::new("f0_fcpe"),
            app_core::ExecutionPolicy::Disabled,
        )
        .is_err(),
        "the final continuous F0 expert must remain enabled"
    );
    assert!(
        workflow_policy_availability(
            &workflow,
            &app_core::WorkflowNodeId::new("forced_alignment"),
            app_core::ExecutionPolicy::Disabled,
        )
        .is_err(),
        "forced alignment is a required lyrics stage"
    );
}

#[test]
fn executable_lead_isolation_labels_lead_and_residual_without_fake_stems() {
    let capability = app_core::list_workflow_capabilities()
        .into_iter()
        .find(|capability| capability.id.as_str() == "audio.lead_isolate")
        .unwrap();
    let labels = capability
        .outputs
        .iter()
        .map(port_label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["lead · LeadVocal", "residual · VocalResidual"]);
    assert!(
        !labels
            .iter()
            .any(|label| { label.contains("BackingVocal") || label.contains("HarmonyVocal") })
    );
}

#[test]
fn expert_fusion_surface_authors_only_the_selector_mode() {
    let workflow = app_core::default_workflow("song-a");
    let fusion = workflow
        .nodes
        .iter()
        .find(|node| node.instance_id.as_str() == "evidence_fusion")
        .unwrap();
    assert_eq!(fusion.parameters.len(), 1);
    assert_eq!(
        fusion.parameters.get("fusion_mode"),
        Some(&serde_json::json!("algorithm"))
    );
}

#[test]
fn expert_fusion_stage_hides_internal_execution_cards() {
    assert!(stage_renders_internal_node_cards(1));
    assert!(stage_renders_internal_node_cards(2));
    assert!(stage_renders_internal_node_cards(3));
    assert!(!stage_renders_internal_node_cards(4));

    let mut workflow = app_core::default_workflow("song-a");
    assert!(
        app_core::set_workflow_parameter(
            &mut workflow,
            &app_core::WorkflowNodeId::new("evidence_fusion"),
            "onset_owner",
            serde_json::json!("basic_pitch"),
        )
        .is_err()
    );
    app_core::set_workflow_parameter(
        &mut workflow,
        &app_core::WorkflowNodeId::new("evidence_fusion"),
        "fusion_mode",
        serde_json::json!("ai"),
    )
    .unwrap();
    assert_eq!(
        app_core::fusion_mode(&workflow),
        app_core::FusionModeV1::AiJudgment
    );
}

#[test]
fn selected_module_edits_in_the_contextual_inspector_not_inside_a_stage_lane() {
    let workspace = include_str!("mod.rs");
    let inspector = include_str!("workspace_sidebar.rs");

    assert!(workspace.contains("workspace_sidebar::spawn_workflow_sidebar"));
    assert!(workspace.contains("expanded: false"));
    assert!(inspector.contains("selected_workflow_node.as_ref().and_then"));
    assert!(inspector.contains("if let Some((node, capability)) = selected"));
    assert!(inspector.contains("allow_drag_reorder: false"));
    assert!(inspector.contains("MODULE SETTINGS"));
}
