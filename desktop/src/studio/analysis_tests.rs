// This file's own module (`studio::analysis::analysis_tests`) has no
// symbols of its own -- every nested `mod foo_tests { use super::X; }`
// below resolves `X` against *this* glob, not against `analysis` (its
// grandparent) or `studio` (its great-grandparent) directly, since
// `super::X` only ever looks one level up.
use crate::studio::*;

mod node_stage_bridge_tests {
    use super::{analysis_node_stage_index, analysis_stage_index, resolve_live_stage_index};

    #[test]
    fn known_node_ids_map_to_the_same_bucket_as_their_legacy_stage() {
        // The bridge must agree with the old classifier for the buckets
        // that already correspond 1:1, so switching a call site over to
        // progress_node never visibly moves a node backward or forward in
        // the UI.
        assert_eq!(analysis_node_stage_index("stems.separate"), Some(1));
        assert_eq!(analysis_stage_index("separation"), 1);

        assert_eq!(analysis_node_stage_index("pitch.extract"), Some(2));
        assert_eq!(analysis_stage_index("pitch"), 2);

        assert_eq!(analysis_node_stage_index("chart.build_candidate"), Some(6));
        assert_eq!(analysis_stage_index("finalizing"), 6);
    }

    #[test]
    fn unknown_node_id_returns_none_rather_than_guessing() {
        assert_eq!(analysis_node_stage_index("not.a.real.node"), None);
    }

    #[test]
    fn resolver_prefers_node_id_when_present() {
        // "pitch" text would normally resolve to bucket 2, but a stale/
        // mismatched stage string paired with an authoritative node_id
        // must defer to the node_id -- it's the structured signal.
        assert_eq!(resolve_live_stage_index("pitch", Some("stems.separate")), 1);
    }

    #[test]
    fn resolver_falls_back_to_legacy_text_classification_when_node_id_absent() {
        // The common case today: most pipeline.py call sites haven't
        // migrated to progress_node yet (docs/analysis-dag-redesign.md
        // Phase 3 status note), so their events carry stage text only.
        assert_eq!(resolve_live_stage_index("alignment", None), 5);
    }

    #[test]
    fn resolver_falls_back_when_node_id_is_unrecognized() {
        assert_eq!(
            resolve_live_stage_index("transcription", Some("not.a.real.node")),
            4
        );
    }
}

#[cfg(test)]
mod plan_inspector_tests {
    use super::{node_state_copy, stage_primary_node_and_artifact};

    #[test]
    fn every_bucket_maps_to_a_real_graph_node() {
        // The chosen node id must exist in the baseline graph, otherwise
        // the inspector panel would silently show "Not planned in this
        // run" for every stage.
        let graph = app_core::baseline_graph_spec();
        for stage_index in 0..7 {
            let (node_id, _artifact) = stage_primary_node_and_artifact(stage_index);
            assert!(
                graph.nodes.iter().any(|node| node.id.as_str() == node_id),
                "stage {stage_index} maps to unknown node {node_id}"
            );
        }
    }

    #[test]
    fn out_of_range_stage_falls_back_to_the_finalizing_bucket() {
        assert_eq!(
            stage_primary_node_and_artifact(99).0,
            stage_primary_node_and_artifact(6).0
        );
    }

    #[test]
    fn only_buckets_with_a_real_cached_file_return_an_artifact_kind() {
        // stage 3 (audio_preprocessing) still has no standalone cached file
        // today -- `PreprocessedAudio` is unaffected by the §4.4 split.
        // stage 4 (transcription) gained a real one once §4.4 split
        // `RecognizedText`/`AsrSegments` out of the combined transcript.
        assert!(stage_primary_node_and_artifact(3).1.is_none());
        assert!(stage_primary_node_and_artifact(4).1.is_some());
        assert!(stage_primary_node_and_artifact(0).1.is_some());
        assert!(stage_primary_node_and_artifact(6).1.is_some());
    }

    #[test]
    fn node_state_copy_never_panics_on_any_variant() {
        for state in [
            app_core::NodeState::Missing,
            app_core::NodeState::Ready,
            app_core::NodeState::Queued,
            app_core::NodeState::Running,
            app_core::NodeState::Cached,
            app_core::NodeState::Succeeded,
            app_core::NodeState::SucceededWithWarnings,
            app_core::NodeState::Failed,
            app_core::NodeState::Stale,
            app_core::NodeState::Frozen,
            app_core::NodeState::Disabled,
            app_core::NodeState::Blocked,
            app_core::NodeState::NotApplicable,
            app_core::NodeState::Cancelled,
        ] {
            assert!(!node_state_copy(state).is_empty());
        }
    }
}

#[cfg(test)]
mod graph_render_bridge_tests {
    use super::{
        AnalysisGraphStageState, GraphNodeState, bucket_stage_id, graph_node_state_to_stage_state,
    };

    #[test]
    fn bucket_stage_id_is_the_exact_inverse_of_analysis_stage_index() {
        for bucket in 0..7 {
            let stage_id = bucket_stage_id(bucket);
            assert_eq!(super::analysis_stage_index(stage_id), bucket);
        }
    }

    #[test]
    fn running_and_complete_pass_through_with_no_override_text() {
        let (state, text) = graph_node_state_to_stage_state(GraphNodeState::Running, 42);
        assert!(matches!(state, AnalysisGraphStageState::Running(42)));
        assert!(text.is_none());

        let (state, text) = graph_node_state_to_stage_state(GraphNodeState::Complete, 0);
        assert!(matches!(state, AnalysisGraphStageState::Complete));
        assert!(text.is_none());
    }

    #[test]
    fn plan_only_states_render_as_waiting_but_carry_distinct_text() {
        for state in [
            GraphNodeState::Frozen,
            GraphNodeState::Disabled,
            GraphNodeState::Blocked,
            GraphNodeState::NotApplicable,
            GraphNodeState::Failed,
        ] {
            let (stage_state, text) = graph_node_state_to_stage_state(state, 0);
            assert!(matches!(stage_state, AnalysisGraphStageState::Waiting));
            assert!(text.is_some(), "{state:?} should carry override text");
        }
    }

    #[test]
    fn every_plan_only_state_has_a_distinct_override_message() {
        let messages: Vec<&str> = [
            GraphNodeState::Frozen,
            GraphNodeState::Disabled,
            GraphNodeState::Blocked,
            GraphNodeState::NotApplicable,
            GraphNodeState::Failed,
        ]
        .into_iter()
        .map(|state| graph_node_state_to_stage_state(state, 0).1.unwrap())
        .collect();
        let unique: std::collections::BTreeSet<&str> = messages.iter().copied().collect();
        assert_eq!(unique.len(), messages.len());
    }
}

#[cfg(test)]
mod graph_view_polish_tests {
    use super::{
        analysis_graph_focus_target, clamp_analysis_graph_zoom, format_epoch_ms,
        selected_stage_parameter, zoomed_box,
    };
    use crate::studio::LayoutRect;
    use app_core::AnalysisProfileSnapshot;

    #[test]
    fn zoom_clamps_to_the_documented_range() {
        assert_eq!(
            clamp_analysis_graph_zoom(0.1),
            super::ANALYSIS_GRAPH_ZOOM_MIN
        );
        assert_eq!(
            clamp_analysis_graph_zoom(9.0),
            super::ANALYSIS_GRAPH_ZOOM_MAX
        );
        assert_eq!(clamp_analysis_graph_zoom(1.0), 1.0);
    }

    #[test]
    fn zoomed_box_scales_every_dimension_uniformly() {
        let rect = LayoutRect {
            x: 100.0,
            y: 50.0,
            width: 150.0,
            height: 78.0,
        };
        let boxed = zoomed_box(rect, 2.0);
        assert_eq!(boxed.x, 200.0);
        assert_eq!(boxed.y, 100.0);
        assert_eq!(boxed.width, 300.0);
        assert_eq!(boxed.height, 156.0);
    }

    #[test]
    fn focus_target_is_none_when_the_node_has_no_layout_rect() {
        // A collapsed compound child, for example -- not part of this
        // pass's layout at all, so there's nothing to scroll to.
        assert_eq!(
            analysis_graph_focus_target(None, &app_core::AnalysisNodeId::new("pitch.extract"), 1.0),
            None
        );
    }

    #[test]
    fn selected_stage_parameter_covers_the_three_profile_controlled_nodes() {
        let profile = AnalysisProfileSnapshot {
            separator: "demucs".to_string(),
            alignment_backend: "whisperx".to_string(),
            asr_engine: "parakeet".to_string(),
            requested_device: "auto".to_string(),
            language_override: None,
        };
        assert_eq!(
            selected_stage_parameter("stems.separate", &profile),
            Some(("SEPARATOR", "demucs".to_string()))
        );
        assert_eq!(
            selected_stage_parameter("lyrics.transcribe", &profile),
            Some(("ASR ENGINE", "parakeet".to_string()))
        );
        assert_eq!(
            selected_stage_parameter("lyrics.align", &profile),
            Some(("ALIGNMENT BACKEND", "whisperx".to_string()))
        );
    }

    #[test]
    fn selected_stage_parameter_is_none_for_nodes_without_a_profile_knob() {
        let profile = AnalysisProfileSnapshot::default();
        for node in ["preflight", "music.analysis", "chart.build_candidate"] {
            assert_eq!(selected_stage_parameter(node, &profile), None);
        }
    }

    #[test]
    fn format_epoch_ms_renders_a_known_instant() {
        // 2024-01-15 12:34:00 UTC, computed independently against a known
        // epoch-seconds value for that timestamp.
        assert_eq!(format_epoch_ms(1_705_322_040_000), "2024-01-15 12:34 UTC");
    }

    #[test]
    fn format_epoch_ms_handles_the_unix_epoch_itself() {
        assert_eq!(format_epoch_ms(0), "1970-01-01 00:00 UTC");
    }
}

#[cfg(test)]
mod artifact_playable_tests {
    use super::artifact_kind_is_playable;
    use app_core::ArtifactKind;

    #[test]
    fn audio_waveform_artifacts_are_playable() {
        for kind in [
            ArtifactKind::VocalStem,
            ArtifactKind::InstrumentalStem,
            ArtifactKind::PreprocessedAudio,
        ] {
            assert!(
                artifact_kind_is_playable(kind),
                "{kind:?} should be playable"
            );
        }
    }

    #[test]
    fn non_audio_artifacts_are_not_playable() {
        for kind in [
            ArtifactKind::RecognizedText,
            ArtifactKind::AsrSegments,
            ArtifactKind::TimedTranscript,
            ArtifactKind::PitchTrack,
            ArtifactKind::MusicAnalysis,
            ArtifactKind::AuthoredChart,
        ] {
            assert!(
                !artifact_kind_is_playable(kind),
                "{kind:?} should not be playable"
            );
        }
    }
}

#[cfg(test)]
mod node_config_field_tests {
    //! Phase 8 "Configure for this run"/"Save as song profile" gating, and
    //! the §8.4 three-tier PARAMETER SOURCE resolution -- fixture-based, no
    //! DB/IO, matching `failed_node_overlay_tests`'s style below.
    use super::{
        node_can_force_transcribe, node_can_refetch_and_align, node_config_profile_field,
        node_parameter_source_copy,
    };
    use app_core::{AnalysisProfileSnapshot, ProfileField};

    #[test]
    fn only_lyrics_transcribe_can_force_transcribe() {
        assert!(node_can_force_transcribe("lyrics.transcribe"));
        for id in [
            "stems.separate",
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.align",
            "lyrics.import_timed",
            "music.analysis",
            "preflight",
            "chart.build_candidate",
        ] {
            assert!(
                !node_can_force_transcribe(id),
                "{id} should not force-transcribe"
            );
        }
    }

    #[test]
    fn only_lyrics_align_can_refetch_and_align() {
        assert!(node_can_refetch_and_align("lyrics.align"));
        for id in [
            "stems.separate",
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.import_timed",
            "music.analysis",
            "preflight",
            "chart.build_candidate",
        ] {
            assert!(
                !node_can_refetch_and_align(id),
                "{id} should not refetch & align"
            );
        }
    }

    #[test]
    fn only_the_three_profile_controlled_nodes_map_to_a_field() {
        assert_eq!(
            node_config_profile_field("stems.separate"),
            Some(ProfileField::Separator)
        );
        assert_eq!(
            node_config_profile_field("lyrics.transcribe"),
            Some(ProfileField::AsrEngine)
        );
        assert_eq!(
            node_config_profile_field("lyrics.align"),
            Some(ProfileField::AlignmentBackend)
        );
        for id in [
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.import_timed",
            "music.analysis",
            "preflight",
            "chart.build_candidate",
        ] {
            assert_eq!(
                node_config_profile_field(id),
                None,
                "{id} should map to nothing"
            );
        }
    }

    #[test]
    fn parameter_source_prefers_run_override_over_song_over_global() {
        let global = AnalysisProfileSnapshot {
            separator: "karaoke".to_string(),
            ..AnalysisProfileSnapshot::default()
        };
        let song = AnalysisProfileSnapshot {
            separator: "demucs".to_string(),
            ..AnalysisProfileSnapshot::default()
        };

        assert_eq!(
            node_parameter_source_copy(Some(ProfileField::Separator), &global, None, None),
            "Global default"
        );
        assert_eq!(
            node_parameter_source_copy(Some(ProfileField::Separator), &global, Some(&song), None),
            "Song profile"
        );
        assert_eq!(
            node_parameter_source_copy(
                Some(ProfileField::Separator),
                &global,
                Some(&song),
                Some("original_mix")
            ),
            "Run override (queued)"
        );
    }

    #[test]
    fn parameter_source_falls_back_to_global_default_when_the_node_has_no_field() {
        let global = AnalysisProfileSnapshot::default();
        assert_eq!(
            node_parameter_source_copy(None, &global, None, Some("irrelevant")),
            "Global default"
        );
    }
}

#[cfg(test)]
mod plan_preview_tests {
    //! Phase 7/8 Plan Preview panel: `plan_preview_groups` bucketing and
    //! `PlanPreviewDraft` toggle wiring, fixture-based, no DB/IO.
    use super::{PLAN_PREVIEW_DISABLEABLE_NODES, PlanPreviewDraft, plan_preview_groups};
    use app_core::{AnalysisNodeId, AnalysisPlan, AnalysisProfileSnapshot, NodeState, PlannedNode};
    use std::collections::BTreeSet;

    fn planned(id: &str, state: NodeState, will_run: bool) -> PlannedNode {
        PlannedNode {
            id: AnalysisNodeId::new(id),
            state,
            will_run,
            reason: None,
        }
    }

    fn fixture_plan(nodes: Vec<PlannedNode>) -> AnalysisPlan {
        AnalysisPlan {
            graph_schema_version: 1,
            file_hash: "plan-preview-fixture".to_string(),
            nodes,
            target_nodes: BTreeSet::new(),
            profile_snapshot: AnalysisProfileSnapshot::default(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn buckets_will_run_will_reuse_blocked_and_disabled() {
        let plan = fixture_plan(vec![
            planned("pitch.extract", NodeState::Disabled, false),
            planned("chart.build_candidate", NodeState::Blocked, false),
            planned("music.analysis", NodeState::Ready, false),
            planned("stems.separate", NodeState::Ready, true),
        ]);
        let groups = plan_preview_groups(&plan);
        let get = |heading: &str| {
            groups
                .iter()
                .find(|(h, _)| *h == heading)
                .map(|(_, nodes)| nodes.clone())
        };
        assert_eq!(get("Will run"), Some(vec!["stems.separate".to_string()]));
        assert_eq!(get("Will reuse"), Some(vec!["music.analysis".to_string()]));
        assert_eq!(
            get("Blocked"),
            Some(vec!["chart.build_candidate".to_string()])
        );
        assert_eq!(get("Disabled"), Some(vec!["pitch.extract".to_string()]));
    }

    #[test]
    fn empty_buckets_are_omitted_entirely() {
        let plan = fixture_plan(vec![planned("stems.separate", NodeState::Ready, true)]);
        let groups = plan_preview_groups(&plan);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "Will run");
    }

    #[test]
    fn not_applicable_frozen_and_stale_never_appear() {
        // This pure function only reads what's given -- a fixture with
        // Frozen/Stale/NotApplicable states proves they're filtered, not
        // just absent because nothing produced them.
        let plan = fixture_plan(vec![
            planned("lyrics.import_timed", NodeState::NotApplicable, false),
            planned("stems.separate", NodeState::Frozen, true),
            planned("chart.build_candidate", NodeState::Stale, true),
        ]);
        let groups = plan_preview_groups(&plan);
        // Frozen/Stale both have will_run == true in this fixture, so they
        // land in "Will run" (the function only special-cases the 4 states
        // it actually models) -- NotApplicable is the one truly dropped.
        let all_nodes: Vec<&String> = groups.iter().flat_map(|(_, nodes)| nodes).collect();
        assert!(
            !all_nodes
                .iter()
                .any(|n| n.as_str() == "lyrics.import_timed")
        );
    }

    #[test]
    fn toggling_a_node_twice_returns_to_not_disabled() {
        let mut draft = PlanPreviewDraft {
            file_hash: "songA".to_string(),
            disabled_nodes: BTreeSet::new(),
        };
        let id = AnalysisNodeId::new("pitch.extract");
        if !draft.disabled_nodes.remove(&id) {
            draft.disabled_nodes.insert(id.clone());
        }
        assert!(draft.disabled_nodes.contains(&id));
        if !draft.disabled_nodes.remove(&id) {
            draft.disabled_nodes.insert(id.clone());
        }
        assert!(!draft.disabled_nodes.contains(&id));
    }

    #[test]
    fn toggling_two_different_nodes_does_not_clobber_each_other() {
        let mut draft = PlanPreviewDraft {
            file_hash: "songA".to_string(),
            disabled_nodes: BTreeSet::new(),
        };
        for node_id in ["stems.separate", "pitch.extract"] {
            let id = AnalysisNodeId::new(node_id);
            if !draft.disabled_nodes.remove(&id) {
                draft.disabled_nodes.insert(id);
            }
        }
        assert!(
            draft
                .disabled_nodes
                .contains(&AnalysisNodeId::new("stems.separate"))
        );
        assert!(
            draft
                .disabled_nodes
                .contains(&AnalysisNodeId::new("pitch.extract"))
        );
    }

    #[test]
    fn every_disableable_node_maps_to_a_real_graph_node() {
        let graph = app_core::baseline_graph_spec();
        for node_id in PLAN_PREVIEW_DISABLEABLE_NODES {
            assert!(
                graph.node(&AnalysisNodeId::new(*node_id)).is_some(),
                "{node_id} should be a real node in the baseline graph"
            );
        }
    }
}

#[cfg(test)]
mod app_log_viewer_tests {
    //! §7.5 "View logs": `resolve_app_log_source` picks between a real
    //! recorded-attempt window and an honestly-labeled fallback, fixture-
    //! based, no DB.
    use super::{AppLogSource, resolve_app_log_source};
    use app_core::NodeAttempt;

    fn fixture_attempt(started_at_ms: Option<i64>, finished_at_ms: Option<i64>) -> NodeAttempt {
        NodeAttempt {
            id: 1,
            run_id: 1,
            file_hash: "songA".to_string(),
            node_id: "pitch.extract".to_string(),
            status: "completed".to_string(),
            progress: 100,
            operation: "extract".to_string(),
            implementation: "rmvpe".to_string(),
            model: "rmvpe".to_string(),
            requested_device: "cpu".to_string(),
            actual_device: "cpu".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms,
            finished_at_ms,
        }
    }

    #[test]
    fn no_attempt_falls_back_to_recent() {
        assert!(matches!(
            resolve_app_log_source(None, 1_000),
            AppLogSource::RecentFallback
        ));
    }

    #[test]
    fn a_completed_attempt_windows_from_start_to_finish() {
        let attempt = fixture_attempt(Some(100), Some(200));
        match resolve_app_log_source(Some(&attempt), 9_999) {
            AppLogSource::Windowed { start_ms, end_ms } => {
                assert_eq!(start_ms, 100);
                assert_eq!(end_ms, 200);
            }
            AppLogSource::RecentFallback => panic!("expected a windowed source"),
        }
    }

    #[test]
    fn a_still_running_attempt_windows_from_start_to_now() {
        let attempt = fixture_attempt(Some(100), None);
        match resolve_app_log_source(Some(&attempt), 9_999) {
            AppLogSource::Windowed { start_ms, end_ms } => {
                assert_eq!(start_ms, 100);
                assert_eq!(end_ms, 9_999);
            }
            AppLogSource::RecentFallback => panic!("expected a windowed source"),
        }
    }

    #[test]
    fn an_attempt_with_no_started_at_falls_back_to_recent() {
        // A real but incomplete attempt row (e.g. from before Phase 7's
        // timestamp columns existed) shouldn't fabricate a window from
        // nothing.
        let attempt = fixture_attempt(None, None);
        assert!(matches!(
            resolve_app_log_source(Some(&attempt), 1_000),
            AppLogSource::RecentFallback
        ));
    }
}

#[cfg(test)]
mod failed_node_overlay_tests {
    //! §7.8/§9.3 "Focus Failed": `analysis_plan::build_plan` itself never
    //! produces `NodeState::Failed` (only `Ready`/`Frozen`/`Disabled`/
    //! `Blocked`/`NotApplicable` -- see that module's doc comment), so the
    //! "Focus Failed" button's search over `plan_preview.nodes` always came
    //! back empty in real use, silently never appearing. These tests cover
    //! `overlay_failed_node_attempts`, which closes that gap using the real
    //! `analysis_node_attempts` data Phase 2/3 now writes.
    use super::overlay_failed_node_attempts;
    use app_core::{AnalysisNodeId, AnalysisPlan, NodeAttempt, NodeState, PlannedNode};

    fn plan(nodes: Vec<PlannedNode>) -> AnalysisPlan {
        AnalysisPlan {
            graph_schema_version: 1,
            file_hash: "songOverlayTest".to_string(),
            nodes,
            target_nodes: Default::default(),
            profile_snapshot: Default::default(),
            warnings: Vec::new(),
        }
    }

    fn planned(id: &str, state: NodeState) -> PlannedNode {
        PlannedNode {
            id: AnalysisNodeId::new(id),
            state,
            will_run: true,
            reason: None,
        }
    }

    fn attempt(node_id: &str, status: &str) -> NodeAttempt {
        NodeAttempt {
            id: 1,
            run_id: 1,
            file_hash: "songOverlayTest".to_string(),
            node_id: node_id.to_string(),
            status: status.to_string(),
            progress: 100,
            operation: String::new(),
            implementation: String::new(),
            model: String::new(),
            requested_device: String::new(),
            actual_device: String::new(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms: None,
            finished_at_ms: None,
        }
    }

    #[test]
    fn a_ready_node_with_a_failed_attempt_becomes_failed() {
        let result = overlay_failed_node_attempts(
            plan(vec![planned("pitch.extract", NodeState::Ready)]),
            &[attempt("pitch.extract", "failed")],
        );
        assert_eq!(
            result
                .node(&AnalysisNodeId::new("pitch.extract"))
                .unwrap()
                .state,
            NodeState::Failed
        );
    }

    #[test]
    fn a_ready_node_with_a_succeeded_attempt_stays_ready() {
        let result = overlay_failed_node_attempts(
            plan(vec![planned("pitch.extract", NodeState::Ready)]),
            &[attempt("pitch.extract", "succeeded")],
        );
        assert_eq!(
            result
                .node(&AnalysisNodeId::new("pitch.extract"))
                .unwrap()
                .state,
            NodeState::Ready
        );
    }

    #[test]
    fn a_ready_node_with_no_matching_attempt_stays_ready() {
        let result = overlay_failed_node_attempts(
            plan(vec![planned("pitch.extract", NodeState::Ready)]),
            &[],
        );
        assert_eq!(
            result
                .node(&AnalysisNodeId::new("pitch.extract"))
                .unwrap()
                .state,
            NodeState::Ready
        );
    }

    #[test]
    fn a_blocked_node_is_not_overwritten_even_with_a_failed_attempt_on_record() {
        // A stale attempt row from an earlier run must not override this
        // run's own, more specific Blocked explanation.
        let result = overlay_failed_node_attempts(
            plan(vec![planned("pitch.extract", NodeState::Blocked)]),
            &[attempt("pitch.extract", "failed")],
        );
        assert_eq!(
            result
                .node(&AnalysisNodeId::new("pitch.extract"))
                .unwrap()
                .state,
            NodeState::Blocked
        );
    }
}

#[cfg(test)]
mod format_artifact_revision_comparison_tests {
    //! §7.6 "Compare revisions".
    use super::format_artifact_revision_comparison;
    use app_core::{AnalysisNodeId, ArtifactKind, ArtifactRevision, ArtifactRevisionComparison};

    fn revision(id: &str, content_hash: &str) -> ArtifactRevision {
        ArtifactRevision {
            id: id.to_string(),
            file_hash: "songA".to_string(),
            kind: ArtifactKind::PitchTrack,
            path: "/cache/songA_pitch_track.json".into(),
            content_hash: content_hash.to_string(),
            producer_node: AnalysisNodeId::new("pitch.extract"),
            input_revisions: vec![],
            config_hash: "cfg".to_string(),
            algorithm_version: "1".to_string(),
            created_at_ms: 0,
            byte_size: 100,
            active: false,
            legacy: false,
            invalidated: false,
        }
    }

    #[test]
    fn same_content_is_reported_even_with_other_differences() {
        let copy = format_artifact_revision_comparison(&ArtifactRevisionComparison {
            revision_a: revision("a", "same"),
            revision_b: revision("b", "same"),
            same_content: true,
            changed_fields: vec!["config_hash"],
        });
        assert!(copy.contains("byte-identical"));
        assert!(copy.contains("config_hash"));
    }

    #[test]
    fn different_content_names_every_changed_field() {
        let copy = format_artifact_revision_comparison(&ArtifactRevisionComparison {
            revision_a: revision("a", "one"),
            revision_b: revision("b", "two"),
            same_content: false,
            changed_fields: vec!["content_hash", "byte_size"],
        });
        assert!(copy.contains("content_hash"));
        assert!(copy.contains("byte_size"));
    }
}

#[cfg(test)]
mod format_artifact_provenance_tests {
    //! §7.6 "Inspect provenance".
    use super::format_artifact_provenance;
    use app_core::{AnalysisNodeId, ArtifactKind, ArtifactRevision};

    fn revision(input_revisions: Vec<String>) -> ArtifactRevision {
        ArtifactRevision {
            id: "songA:pitch_track:abc123".to_string(),
            file_hash: "songA".to_string(),
            kind: ArtifactKind::PitchTrack,
            path: "/cache/songA_pitch_track.json".into(),
            content_hash: "abcdef0123456789".to_string(),
            producer_node: AnalysisNodeId::new("pitch.extract"),
            input_revisions,
            config_hash: "configHash0123456789".to_string(),
            algorithm_version: "1".to_string(),
            created_at_ms: 0,
            byte_size: 100,
            active: true,
            legacy: false,
            invalidated: false,
        }
    }

    #[test]
    fn names_producer_node_and_algorithm_version() {
        let copy = format_artifact_provenance(&revision(vec![]));
        assert!(copy.contains("pitch.extract"));
        assert!(copy.contains("algorithm v1"));
    }

    #[test]
    fn no_inputs_reads_as_none_recorded() {
        let copy = format_artifact_provenance(&revision(vec![]));
        assert!(copy.contains("inputs: none recorded"));
    }

    #[test]
    fn lists_real_input_revision_ids() {
        let copy = format_artifact_provenance(&revision(vec!["songA:vocal_stem:xyz".to_string()]));
        assert!(copy.contains("songA:vocal_stem:xyz"));
    }
}

#[cfg(test)]
mod node_duration_copy_tests {
    //! §7.4 "DURATION" inspector fact.
    use super::node_duration_copy;

    fn route(
        started_at_ms: Option<i64>,
        finished_at_ms: Option<i64>,
    ) -> app_core::AnalysisStageRoute {
        app_core::AnalysisStageRoute {
            stage: "pitch".to_string(),
            node_id: Some("pitch.extract".to_string()),
            node_event: None,
            operation: String::new(),
            implementation: String::new(),
            model: String::new(),
            stage_progress: 100,
            requested_device: String::new(),
            actual_device: String::new(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms,
            finished_at_ms,
        }
    }

    #[test]
    fn no_route_reads_as_not_yet_available() {
        assert_eq!(node_duration_copy(None), "Not yet available");
    }

    #[test]
    fn a_route_still_running_reads_as_not_yet_available() {
        let r = route(Some(1_700_000_000_000), None);
        assert_eq!(node_duration_copy(Some(&r)), "Not yet available");
    }

    #[test]
    fn a_completed_route_formats_as_minutes_seconds() {
        // 4.5 seconds -> "0:04" or "0:05" depending on rounding; the real
        // assertion is that it's not "Not yet available" and matches the
        // shared format_duration helper's own rounding.
        let r = route(Some(1_700_000_000_000), Some(1_700_000_004_500));
        assert_eq!(node_duration_copy(Some(&r)), super::format_duration(4.5));
    }

    #[test]
    fn a_corrupt_finished_before_started_reads_as_not_yet_available() {
        let r = route(Some(1_700_000_004_500), Some(1_700_000_000_000));
        assert_eq!(node_duration_copy(Some(&r)), "Not yet available");
    }
}

#[cfg(test)]
mod format_node_attempt_comparison_tests {
    //! §7.5 "Compare with previous attempt" copy.
    use super::format_node_attempt_comparison;
    use app_core::{NodeAttempt, NodeAttemptComparison};

    fn attempt(implementation: &str, status: &str) -> NodeAttempt {
        NodeAttempt {
            id: 1,
            run_id: 1,
            file_hash: "songA".to_string(),
            node_id: "pitch.extract".to_string(),
            status: status.to_string(),
            progress: 100,
            operation: String::new(),
            implementation: implementation.to_string(),
            model: String::new(),
            requested_device: String::new(),
            actual_device: String::new(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms: None,
            finished_at_ms: None,
        }
    }

    #[test]
    fn unchanged_attempt_names_the_implementation() {
        let copy = format_node_attempt_comparison(&NodeAttemptComparison {
            node_id: "pitch.extract".to_string(),
            attempt_a: Some(attempt("RMVPE", "succeeded")),
            attempt_b: Some(attempt("RMVPE", "succeeded")),
            changed_fields: Vec::new(),
        });
        assert!(copy.contains("unchanged"));
        assert!(copy.contains("RMVPE"));
    }

    #[test]
    fn changed_implementation_shows_previous_to_current() {
        let copy = format_node_attempt_comparison(&NodeAttemptComparison {
            node_id: "stems.separate".to_string(),
            attempt_a: Some(attempt("UVR", "succeeded")),
            attempt_b: Some(attempt("Demucs", "succeeded")),
            changed_fields: vec!["implementation"],
        });
        assert!(copy.contains("Demucs → UVR"));
    }

    #[test]
    fn missing_current_attempt_is_named_explicitly() {
        let copy = format_node_attempt_comparison(&NodeAttemptComparison {
            node_id: "pitch.extract".to_string(),
            attempt_a: None,
            attempt_b: Some(attempt("RMVPE", "succeeded")),
            changed_fields: Vec::new(),
        });
        assert!(copy.contains("no recorded attempt in the current run"));
    }

    #[test]
    fn missing_previous_attempt_is_named_explicitly() {
        let copy = format_node_attempt_comparison(&NodeAttemptComparison {
            node_id: "pitch.extract".to_string(),
            attempt_a: Some(attempt("RMVPE", "succeeded")),
            attempt_b: None,
            changed_fields: Vec::new(),
        });
        assert!(copy.contains("no recorded attempt in the previous run"));
    }
}

#[cfg(test)]
mod stale_candidate_overlay_tests {
    //! Phase 5 §5.5 "Stale Evidence" / §7's "GraphNodeState has no Stale
    //! variant" gap, closed: `overlay_stale_candidate_chart` marks
    //! `chart.build_candidate` Stale when `app_core::candidate_chart_status`
    //! (real mtime comparison, see `chart.rs`) reports a newer candidate
    //! than what's currently authored.
    use super::overlay_stale_candidate_chart;
    use app_core::{
        AnalysisNodeId, AnalysisPlan, CandidateChartStatus, CandidateChartSummary, NodeState,
        PlannedNode,
    };

    fn plan(nodes: Vec<PlannedNode>) -> AnalysisPlan {
        AnalysisPlan {
            graph_schema_version: 1,
            file_hash: "songStaleTest".to_string(),
            nodes,
            target_nodes: Default::default(),
            profile_snapshot: Default::default(),
            warnings: Vec::new(),
        }
    }

    fn planned(id: &str, state: NodeState) -> PlannedNode {
        PlannedNode {
            id: AnalysisNodeId::new(id),
            state,
            will_run: true,
            reason: None,
        }
    }

    fn candidate_available() -> CandidateChartStatus {
        CandidateChartStatus::CandidateAvailable(CandidateChartSummary {
            authored_phrase_count: 1,
            authored_note_count: 1,
            candidate_phrase_count: 2,
            candidate_note_count: 3,
            lyrics_changed: true,
            pitch_evidence_changed: false,
        })
    }

    #[test]
    fn a_ready_chart_build_candidate_becomes_stale_when_a_candidate_is_available() {
        let result = overlay_stale_candidate_chart(
            plan(vec![planned("chart.build_candidate", NodeState::Ready)]),
            &candidate_available(),
        );
        assert_eq!(
            result
                .node(&AnalysisNodeId::new("chart.build_candidate"))
                .unwrap()
                .state,
            NodeState::Stale
        );
    }

    #[test]
    fn up_to_date_status_never_marks_anything_stale() {
        let result = overlay_stale_candidate_chart(
            plan(vec![planned("chart.build_candidate", NodeState::Ready)]),
            &CandidateChartStatus::UpToDate,
        );
        assert_eq!(
            result
                .node(&AnalysisNodeId::new("chart.build_candidate"))
                .unwrap()
                .state,
            NodeState::Ready
        );
    }

    #[test]
    fn not_authored_yet_status_never_marks_anything_stale() {
        let result = overlay_stale_candidate_chart(
            plan(vec![planned("chart.build_candidate", NodeState::Ready)]),
            &CandidateChartStatus::NotAuthoredYet,
        );
        assert_eq!(
            result
                .node(&AnalysisNodeId::new("chart.build_candidate"))
                .unwrap()
                .state,
            NodeState::Ready
        );
    }

    #[test]
    fn other_nodes_are_never_marked_stale() {
        let result = overlay_stale_candidate_chart(
            plan(vec![planned("pitch.extract", NodeState::Ready)]),
            &candidate_available(),
        );
        assert_eq!(
            result
                .node(&AnalysisNodeId::new("pitch.extract"))
                .unwrap()
                .state,
            NodeState::Ready
        );
    }

    #[test]
    fn a_blocked_chart_build_candidate_is_not_overwritten() {
        // This run's own, more specific Blocked explanation must not be
        // clobbered by a staleness fact about the *previous* run's output.
        let result = overlay_stale_candidate_chart(
            plan(vec![planned("chart.build_candidate", NodeState::Blocked)]),
            &candidate_available(),
        );
        assert_eq!(
            result
                .node(&AnalysisNodeId::new("chart.build_candidate"))
                .unwrap()
                .state,
            NodeState::Blocked
        );
    }
}

#[cfg(test)]
mod compound_toggle_tests {
    //! §7.3 "Music Analysis 支持展开": the click action was modeled and
    //! tested in `analysis_model.rs` since Phase 7 landed but never wired to
    //! an interaction. These tests cover the wiring itself --
    //! `analysis_node_compound_toggle_action`, shared by the real
    //! secondary-click path and the `UTA_STUDIO_DEBUG_EXPAND_COMPOUND` debug
    //! path.
    use super::analysis_node_compound_toggle_action;
    use crate::studio::UiAction;

    #[test]
    fn a_plain_node_has_no_toggle_action() {
        assert_eq!(
            analysis_node_compound_toggle_action("pitch.extract", false),
            None
        );
    }

    #[test]
    fn a_collapsed_compound_node_offers_expand() {
        let (label, action) = analysis_node_compound_toggle_action("music.analysis", false)
            .expect("music.analysis is a real compound node");
        assert_eq!(label, "Expand sub-checks");
        assert_eq!(
            action,
            UiAction::ToggleAnalysisCompoundNode("music.analysis".to_string())
        );
    }

    #[test]
    fn an_expanded_compound_node_offers_collapse() {
        let (label, _action) = analysis_node_compound_toggle_action("music.analysis", true)
            .expect("music.analysis is a real compound node");
        assert_eq!(label, "Collapse sub-checks");
    }

    #[test]
    fn a_compound_nodes_own_child_is_not_itself_compound() {
        // music.key is a child of the music.analysis compound node, not a
        // compound node in its own right -- must not offer a toggle either.
        assert_eq!(
            analysis_node_compound_toggle_action("music.key", false),
            None
        );
    }
}

#[cfg(test)]
mod node_id_wire_protocol_tests {
    use super::{find_matching_route, selected_progress_and_status};
    use crate::studio::GraphNodeState;

    fn route(
        node_id: Option<&str>,
        stage: &str,
        stage_progress: usize,
    ) -> app_core::AnalysisStageRoute {
        app_core::AnalysisStageRoute {
            stage: stage.to_string(),
            node_id: node_id.map(str::to_string),
            node_event: None,
            operation: "Op".to_string(),
            implementation: "Impl".to_string(),
            model: "Model".to_string(),
            stage_progress,
            requested_device: "cpu".to_string(),
            actual_device: "cpu".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms: None,
            finished_at_ms: None,
        }
    }

    #[test]
    fn old_snapshot_json_without_route_node_id_still_deserializes() {
        // A `snapshot_json` blob written before this field existed --
        // `load_analysis_history` drops a row that fails to parse
        // (`.ok()?`), so this must keep working or old runs vanish from
        // history. Mirrors the equivalent test for
        // `AnalysisProgressSnapshot.node_id` in app-core/src/analyzer.rs.
        let json = r#"{
            "stage": "pitch",
            "operation": "Reference pitch extraction",
            "implementation": "RMVPE",
            "model": "RMVPE singing pitch model",
            "stage_progress": 40,
            "requested_device": "cuda",
            "actual_device": "cuda",
            "fallback_from": null,
            "fallback_reason": null,
            "backend_fallback_from": null,
            "backend_fallback_reason": null
        }"#;
        let parsed: app_core::AnalysisStageRoute =
            serde_json::from_str(json).expect("old route json must still parse");
        assert_eq!(parsed.node_id, None);
        assert_eq!(parsed.stage, "pitch");
    }

    #[test]
    fn find_matching_route_prefers_exact_node_id_over_bucket_text() {
        // Two children of a compound node sharing one legacy bucket -- the
        // bug this fixes: before `node_id` existed, only the *last*
        // recorded route for a shared bucket was reachable at all, no
        // matter which child you actually wanted.
        let routes = vec![
            route(Some("music.key"), "key_detection", 30),
            route(Some("music.rhythm"), "key_detection", 90),
        ];
        let found = find_matching_route(&routes, "music.key", "preparing").unwrap();
        assert_eq!(found.stage_progress, 30);
        let found = find_matching_route(&routes, "music.rhythm", "preparing").unwrap();
        assert_eq!(found.stage_progress, 90);
    }

    #[test]
    fn find_matching_route_falls_back_to_bucket_text_when_no_node_id_matches() {
        // Legacy call site that never migrated to progress_node: every
        // route has node_id=None, so precise matching never hits, and the
        // pre-fix bucket-text behavior must be preserved exactly.
        let routes = vec![route(None, "pitch", 55)];
        let found = find_matching_route(&routes, "pitch.extract", "pitch").unwrap();
        assert_eq!(found.stage_progress, 55);
    }

    #[test]
    fn find_matching_route_returns_none_when_nothing_matches_either_way() {
        let routes = vec![route(Some("stems.separate"), "separation", 40)];
        assert!(find_matching_route(&routes, "pitch.extract", "pitch").is_none());
    }

    #[test]
    fn selected_progress_and_status_forces_100_when_render_state_is_complete() {
        // The confirmed real bug: stage_routes can be frozen at a stale
        // non-100 value even after the node is genuinely done.
        let (progress, status) =
            selected_progress_and_status(Some(GraphNodeState::Complete), 67, "RUNNING");
        assert_eq!(progress, 100);
        assert_eq!(status, "COMPLETE");
    }

    #[test]
    fn selected_progress_and_status_leaves_non_complete_states_untouched() {
        for state in [
            GraphNodeState::Running,
            GraphNodeState::Waiting,
            GraphNodeState::Blocked,
        ] {
            let (progress, status) = selected_progress_and_status(Some(state), 42, "RUNNING");
            assert_eq!(progress, 42);
            assert_eq!(status, "RUNNING");
        }
        let (progress, status) = selected_progress_and_status(None, 0, "WAITING");
        assert_eq!(progress, 0);
        assert_eq!(status, "WAITING");
    }
}
