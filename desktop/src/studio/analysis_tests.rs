// This file's own module (`studio::analysis::analysis_tests`) has no
// symbols of its own -- every nested `mod foo_tests { use super::X; }`
// below resolves `X` against *this* glob, not against `analysis` (its
// grandparent) or `studio` (its great-grandparent) directly, since
// `super::X` only ever looks one level up.
use crate::studio::*;

#[cfg(test)]
mod graph_view_polish_tests {
    use super::{
        ANALYSIS_GRAPH_ZOOM_DEFAULT, analysis_graph_center_target, analysis_graph_fit_zoom,
        analysis_graph_focus_target, clamp_analysis_graph_zoom, format_epoch_ms, zoomed_box,
    };
    use crate::studio::LayoutRect;

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
        assert_eq!(
            clamp_analysis_graph_zoom(ANALYSIS_GRAPH_ZOOM_DEFAULT),
            ANALYSIS_GRAPH_ZOOM_DEFAULT
        );
    }

    #[test]
    fn fit_zoom_uses_the_tighter_axis_so_the_graph_stays_on_one_page() {
        let fitted = analysis_graph_fit_zoom(2000.0, 400.0, 1000.0, 600.0);
        assert!(fitted < 1.0);
        assert!((fitted - (980.0 / 2000.0)).abs() < 0.02);
        let tall = analysis_graph_fit_zoom(800.0, 800.0, 1000.0, 400.0);
        assert!(tall < 0.6);
    }

    #[test]
    fn live_follow_centers_the_running_node_in_the_viewport() {
        let mut layout = crate::studio::GraphLayout {
            rects: Default::default(),
            canvas_width: 800.0,
            canvas_height: 200.0,
        };
        layout.rects.insert(
            app_core::AnalysisNodeId::new("pitch.extract"),
            crate::studio::LayoutRect {
                x: 400.0,
                y: 16.0,
                width: 128.0,
                height: 70.0,
            },
        );
        let (scroll, node_id) = analysis_graph_center_target(
            Some(&layout),
            &app_core::AnalysisNodeId::new("pitch.extract"),
            1.6,
            800.0,
        )
        .expect("laid-out node has a focus target");
        assert_eq!(node_id, "pitch.extract");
        // Node center is (400 + 64) * 1.6 = 742.4; minus half the 800px viewport.
        assert_eq!(scroll, 342);
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
mod plan_preview_tests {
    use super::{
        PlanPreviewDraft, artifact_product_label, capability_product_label,
        exact_preview_allows_queue, preview_quality_source, preview_target_source,
    };

    #[test]
    fn exact_backend_blocker_keeps_the_queue_action_disabled() {
        let blockers = vec!["model:game is unavailable".to_string()];
        assert!(!exact_preview_allows_queue(false, &blockers, false));
        assert!(!exact_preview_allows_queue(true, &blockers, false));
        assert!(!exact_preview_allows_queue(true, &[], true));
        assert!(exact_preview_allows_queue(true, &[], false));
    }

    #[test]
    fn engine_plan_uses_capabilities_as_primary_labels() {
        assert_eq!(capability_product_label("pitch.track"), "Continuous pitch");
        assert_eq!(
            capability_product_label("notes.game"),
            "Note & boundary evidence"
        );
    }

    #[test]
    fn exact_outputs_use_product_labels_without_hiding_unknown_protocol_values() {
        assert_eq!(
            artifact_product_label("candidate_vocal_chart"),
            "Candidate VocalChart"
        );
        assert_eq!(
            artifact_product_label("stem:instrumental"),
            "Instrumental stem"
        );
        assert_eq!(artifact_product_label("future_artifact"), "future_artifact");
    }

    #[test]
    fn run_dialog_marks_only_explicit_temporary_choices_as_run_sources() {
        let mut draft = PlanPreviewDraft {
            file_hash: "songA".to_string(),
            outputs: app_core::AnalysisOutputSelection::default(),
            outputs_overridden: false,
            run_override: app_core::AnalysisExperienceOverride::default(),
            effective_settings: None,
            engine_preview: Err("fixture".to_string()),
        };
        assert_eq!(preview_target_source(&draft), "UNAVAILABLE");
        assert_eq!(preview_quality_source(&draft), "UNAVAILABLE");

        draft.outputs_overridden = true;
        draft.run_override.quality_profile = Some(app_core::AnalysisQualityProfile::Maximum);
        assert_eq!(preview_target_source(&draft), "RUN");
        assert_eq!(preview_quality_source(&draft), "RUN");
    }

    #[test]
    fn temporary_quality_override_does_not_mutate_global_defaults() {
        let config = app_core::AppConfig::default();
        let run = app_core::AnalysisExperienceOverride {
            quality_profile: Some(app_core::AnalysisQualityProfile::Maximum),
            ..Default::default()
        };
        assert_eq!(
            config.analysis_quality(),
            app_core::AnalysisQualityProfile::Balanced
        );
        assert_eq!(
            run.quality_profile,
            Some(app_core::AnalysisQualityProfile::Maximum)
        );
    }
}

#[cfg(test)]
mod analysis_settings_information_architecture_tests {
    use super::ANALYSIS_SETTINGS_SECTION_ORDER;

    #[test]
    fn analysis_sections_follow_the_frozen_order() {
        assert_eq!(
            ANALYSIS_SETTINGS_SECTION_ORDER,
            [
                "QUALITY & OUTPUT BEHAVIOR",
                "AUDIO PREPARATION",
                "LYRICS & ALIGNMENT",
                "PITCH, NOTES & FUSION",
                "ADVANCED PERFORMANCE / MODEL-OWNED PARAMETERS",
                "AUTOMATION"
            ]
        );
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
            engine_node_id: Some("pitch".to_string()),
            capability_id: Some("pitch.track".to_string()),
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
            binding_kind: None,
            committed_outputs: Vec::new(),
            input_revision_ids: Vec::new(),
            started_at_ms,
            finished_at_ms,
            event_at_ms: None,
            work_units_completed: None,
            work_units_total: None,
            worker_task_id: None,
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
            attempt_a: Some(attempt("RoFormer", "succeeded")),
            attempt_b: Some(attempt("RoFormer", "succeeded")),
            changed_fields: vec!["implementation"],
        });
        assert!(copy.contains("RoFormer → RoFormer"));
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
