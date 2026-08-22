mod song_authoring_state_tests {
    use super::{QueuedStatus, SongAuthoringState, authoring_state_from_signals};

    #[test]
    fn failed_queue_entry_wins_over_everything_else() {
        let failed = QueuedStatus::Failed("boom".to_string());
        assert_eq!(
            authoring_state_from_signals(Some(&failed), true, true, true),
            SongAuthoringState::RetryFailedNode
        );
    }

    #[test]
    fn queued_or_analyzing_reports_in_progress() {
        assert_eq!(
            authoring_state_from_signals(Some(&QueuedStatus::Queued), true, true, true),
            SongAuthoringState::InProgress
        );
        assert_eq!(
            authoring_state_from_signals(Some(&QueuedStatus::Analyzing(42)), false, false, false),
            SongAuthoringState::InProgress
        );
    }

    #[test]
    fn never_analyzed_prompts_analyze_song() {
        assert_eq!(
            authoring_state_from_signals(None, false, false, false),
            SongAuthoringState::AnalyzeSong
        );
    }

    #[test]
    fn analyzed_without_a_chart_yet_prompts_open_editor() {
        assert_eq!(
            authoring_state_from_signals(None, true, false, false),
            SongAuthoringState::OpenEditor
        );
    }

    #[test]
    fn chart_present_but_editor_blocked_prompts_fix_chart_issues() {
        assert_eq!(
            authoring_state_from_signals(None, true, true, false),
            SongAuthoringState::FixChartIssues
        );
    }

    #[test]
    fn chart_present_and_editor_ready_prompts_edit_chart() {
        assert_eq!(
            authoring_state_from_signals(None, true, true, true),
            SongAuthoringState::EditChart
        );
    }
}

#[cfg(test)]
mod live_progress_tests {
    use super::{
        AnalysisProgressSnapshot, AnalysisStageRoute, LIVE_ANALYSIS, update_live_analysis,
    };

    fn route(node_id: &str, event: &str, progress: usize) -> AnalysisStageRoute {
        AnalysisStageRoute {
            stage: "separation".to_string(),
            node_id: Some(node_id.to_string()),
            node_event: Some(event.to_string()),
            binding_kind: None,
            committed_outputs: Vec::new(),
            input_revision_ids: Vec::new(),
            operation: node_id.to_string(),
            implementation: "fixture".to_string(),
            model: "fixture".to_string(),
            stage_progress: progress,
            requested_device: "cpu".to_string(),
            actual_device: "cpu".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms: Some(1),
            finished_at_ms: None,
            event_at_ms: Some(1),
            work_units_completed: None,
            work_units_total: None,
        }
    }

    fn snapshot(progress: usize, route: AnalysisStageRoute) -> AnalysisProgressSnapshot {
        AnalysisProgressSnapshot {
            stage: "separation".to_string(),
            overall_progress: progress,
            stage_progress: route.stage_progress,
            operation: route.operation.clone(),
            detail: String::new(),
            implementation: route.implementation.clone(),
            model: route.model.clone(),
            device: "cpu".to_string(),
            requested_device: "cpu".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            stage_routes: vec![route],
            node_id: None,
            node_event: None,
            artifact_reused_reason: None,
            analysis_log_path: None,
        }
    }

    #[test]
    fn progress_is_monotonic_merges_real_nodes_and_caps_live_messages_at_99() {
        let hash = "live-progress-contract-fixture";
        LIVE_ANALYSIS.lock().unwrap().remove(hash);

        update_live_analysis(
            hash,
            snapshot(80, route("stems.vocals", "completed", 100)),
        );
        update_live_analysis(
            hash,
            snapshot(20, route("instrumental.denoise", "started", 0)),
        );
        let current = LIVE_ANALYSIS.lock().unwrap().get(hash).cloned().unwrap();
        assert_eq!(current.overall_progress, 80);
        assert_eq!(current.stage_routes.len(), 2);

        update_live_analysis(
            hash,
            snapshot(100, route("instrumental.denoise", "completed", 100)),
        );
        let current = LIVE_ANALYSIS.lock().unwrap().remove(hash).unwrap();
        assert_eq!(current.overall_progress, 99);
    }
}

#[cfg(test)]
mod chart_protection_tests {
    use super::{apply_pitch_reanalysis_reset, apply_realign_reset, apply_reanalyze_reset};
    use crate::cache::CacheDir;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_cache(label: &str) -> CacheDir {
        let nonce = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "uta-studio-chart-protection-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp cache dir");
        CacheDir { path }
    }

    fn touch(path: &std::path::Path) {
        std::fs::write(path, b"{}").expect("write fixture file");
    }

    #[test]
    fn pitch_reanalysis_reset_preserves_the_authored_chart() {
        let cache = temp_cache("pitch");
        let hash = "songPitch";
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.pitch_track_path(hash));
        touch(&cache.pitch_notes_path(hash));

        apply_pitch_reanalysis_reset(&cache, hash);

        assert!(
            cache.vocal_chart_path(hash).is_file(),
            "authored chart must survive a pitch-only rerun"
        );
        assert!(!cache.pitch_track_path(hash).is_file());
        assert!(!cache.pitch_notes_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn realign_reset_preserves_the_authored_chart() {
        let cache = temp_cache("realign");
        let hash = "songRealign";
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.transcript_path(hash));
        touch(&cache.variant_transcript_path(hash, 1.2));

        apply_realign_reset(&cache, hash);

        assert!(
            cache.vocal_chart_path(hash).is_file(),
            "authored chart must survive realign"
        );
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.variant_transcript_path(hash, 1.2).is_file());
        cache.clear_all();
    }

    #[test]
    fn transcript_only_reanalyze_reset_preserves_the_authored_chart() {
        let cache = temp_cache("reanalyze-transcript");
        let hash = "songReanalyzeTranscript";
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.transcript_path(hash));
        touch(&cache.lyrics_path(hash));

        apply_reanalyze_reset(&cache, hash, false);

        assert!(cache.vocal_chart_path(hash).is_file());
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.lyrics_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn full_reanalyze_reset_preserves_the_authored_chart_but_clears_everything_else() {
        // The highest-stakes case: "Reanalyze all" regenerates every
        // analysis artifact, yet must still default to keeping the chart
        // (phase plan Phase 9 test: "Full Reanalysis 默认保留 Authored Chart").
        let cache = temp_cache("reanalyze-full");
        let hash = "songReanalyzeFull";
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.transcript_path(hash));
        touch(&cache.pitch_track_path(hash));
        touch(&cache.pitch_notes_path(hash));
        touch(&cache.music_analysis_path(hash));
        touch(&cache.vocals_path(hash));
        touch(&cache.instrumental_path(hash));

        apply_reanalyze_reset(&cache, hash, true);

        assert!(
            cache.vocal_chart_path(hash).is_file(),
            "authored chart must survive a full reanalysis reset"
        );
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.pitch_track_path(hash).is_file());
        assert!(!cache.pitch_notes_path(hash).is_file());
        assert!(!cache.music_analysis_path(hash).is_file());
        assert!(!cache.vocals_path(hash).is_file());
        assert!(!cache.instrumental_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn explicit_delete_song_cache_still_removes_the_chart() {
        // The one place total deletion remains correct: the explicit,
        // user-confirmed "Delete cache" action (delete_cache ->
        // cache.delete_song_cache), unaffected by this phase's change.
        let cache = temp_cache("delete-cache");
        let hash = "songDeleteCache";
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.transcript_path(hash));

        cache.delete_song_cache(hash);

        assert!(!cache.vocal_chart_path(hash).is_file());
        assert!(!cache.transcript_path(hash).is_file());
        cache.clear_all();
    }
}

#[cfg(test)]
mod pipeline_flags_tests {
    use super::pipeline_flags_for_request;
    use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
    use std::collections::BTreeSet;

    fn targets(ids: &[&str]) -> BTreeSet<AnalysisNodeId> {
        ids.iter().map(|s| AnalysisNodeId::new(*s)).collect()
    }

    fn no_freeze() -> BTreeSet<ArtifactKind> {
        BTreeSet::new()
    }

    fn no_bypass() -> BTreeSet<AnalysisNodeId> {
        BTreeSet::new()
    }

    #[test]
    fn no_targets_means_run_everything() {
        let flags = pipeline_flags_for_request(
            &BTreeSet::new(),
            &BTreeSet::new(),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(!flags.skip_transcription);
        assert!(!flags.skip_separation);
        assert!(!flags.skip_pitch);
        assert!(!flags.freeze_separation);
        assert!(!flags.freeze_pitch);
        assert!(!flags.bypass_separation);
    }

    #[test]
    fn pitch_only_target_skips_transcription_but_not_separation() {
        // Replaces the old PITCH_ONLY special case: pitch.extract requires
        // stems.separate transitively, so separation must still run, but no
        // lyrics node is targeted so transcription/alignment must not.
        let flags = pipeline_flags_for_request(
            &targets(&["pitch.extract"]),
            &BTreeSet::new(),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(flags.skip_transcription);
        assert!(!flags.skip_separation);
        assert!(!flags.skip_pitch);
    }

    #[test]
    fn lyrics_target_never_skips_transcription() {
        let flags = pipeline_flags_for_request(
            &targets(&["lyrics.align"]),
            &BTreeSet::new(),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(!flags.skip_transcription);
    }

    #[test]
    fn full_candidate_chart_target_skips_neither() {
        let flags = pipeline_flags_for_request(
            &targets(&["chart.build_candidate"]),
            &BTreeSet::new(),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(!flags.skip_transcription);
        assert!(!flags.skip_separation);
        assert!(!flags.skip_pitch);
    }

    #[test]
    fn disabling_pitch_extract_under_the_default_full_target_blocks_the_chart_but_is_not_rejected()
    {
        // pitch.extract feeds chart.build_candidate's PitchNoteCandidates
        // input directly, so disabling it under the default full-run target
        // makes the plan mark chart.build_candidate Blocked -- that's the
        // disable working as designed (docs/analysis-dag-redesign.md §6),
        // not a request the caller's own disable was refused for, so this
        // must still succeed and skip pitch.
        let flags = pipeline_flags_for_request(
            &BTreeSet::new(),
            &targets(&["pitch.extract"]),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(!flags.skip_transcription);
        assert!(!flags.skip_separation);
        assert!(flags.skip_pitch);
    }

    #[test]
    fn disabling_pitch_extract_while_targeting_only_stems_has_no_downstream_to_block() {
        let flags = pipeline_flags_for_request(
            &targets(&["stems.separate"]),
            &targets(&["pitch.extract"]),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(!flags.skip_separation);
        assert!(flags.skip_pitch);
    }

    #[test]
    fn disabling_an_always_required_node_is_rejected_with_a_warning() {
        let result = pipeline_flags_for_request(
            &BTreeSet::new(),
            &targets(&["chart.build_candidate"]),
            &no_freeze(),
            &no_bypass(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn freezing_stems_does_not_skip_separation_but_sets_the_freeze_flag() {
        // A Frozen stems.separate must still be "run" (so pipeline.py calls
        // run_stem_separation and gets a vocals path to hand downstream) --
        // it must NOT collapse to skip_separation the way a Blocked/Disabled
        // stems.separate would, or pitch.extract/transcription would get a
        // `None` vocals path and crash instead of reusing the frozen file.
        let mut frozen = BTreeSet::new();
        frozen.insert(ArtifactKind::VocalStem);
        let flags = pipeline_flags_for_request(
            &targets(&["pitch.extract"]),
            &BTreeSet::new(),
            &frozen,
            &no_bypass(),
        )
        .unwrap();
        assert!(!flags.skip_separation, "a frozen node must not also be skipped");
        assert!(flags.freeze_separation);
        assert!(!flags.freeze_pitch);
        assert!(!flags.bypass_separation);
    }

    #[test]
    fn freezing_pitch_sets_only_the_pitch_freeze_flag() {
        let mut frozen = BTreeSet::new();
        frozen.insert(ArtifactKind::PitchTrack);
        frozen.insert(ArtifactKind::PitchNoteCandidates);
        let flags = pipeline_flags_for_request(
            &targets(&["chart.build_candidate"]),
            &BTreeSet::new(),
            &frozen,
            &no_bypass(),
        )
        .unwrap();
        assert!(!flags.skip_separation);
        assert!(!flags.freeze_separation);
        assert!(!flags.skip_pitch, "a frozen node must not also be skipped");
        assert!(flags.freeze_pitch);
    }

    #[test]
    fn bypassing_stems_skips_separation_and_sets_the_bypass_flag() {
        // Unlike Freeze, a Bypassed stems.separate genuinely does not run --
        // pipeline.py substitutes the Original Mix as the vocals path
        // itself, so skip_separation stays true (no real separation call),
        // with bypass_separation telling it to use the substitute rather
        // than leaving the vocals path unset.
        let mut bypassed = BTreeSet::new();
        bypassed.insert(AnalysisNodeId::new("stems.separate"));
        let flags = pipeline_flags_for_request(
            &targets(&["pitch.extract"]),
            &BTreeSet::new(),
            &no_freeze(),
            &bypassed,
        )
        .unwrap();
        assert!(flags.skip_separation);
        assert!(!flags.freeze_separation);
        assert!(flags.bypass_separation);
    }
}

#[cfg(test)]
mod preview_full_analysis_plan_tests {
    use super::preview_full_analysis_plan;
    use crate::analysis_graph::AnalysisNodeId;
    use crate::analysis_profile::{AnalysisProfileSnapshot, set_song_analysis_profile};
    use std::collections::BTreeSet;

    #[test]
    fn targets_the_full_chart_build_and_lists_every_node() {
        let plan = preview_full_analysis_plan("preview-plan-test-song-a")
            .expect("baseline graph always plans");
        assert!(
            plan.target_nodes
                .contains(&AnalysisNodeId::new("chart.build_candidate"))
        );
        assert!(!plan.nodes.is_empty());
        assert!(plan.node(&AnalysisNodeId::new("music.analysis")).is_some());
    }

    #[test]
    fn falls_back_to_the_real_global_config_when_no_song_profile_is_saved() {
        // Phase 8: this used to fall back to `AnalysisProfileSnapshot::default()`'s
        // hardcoded stand-ins, which could silently disagree with the user's
        // actual global settings. Compares against the same
        // `from_app_config` resolution `process_song` now uses for real
        // execution, rather than a hardcoded literal, so this test doesn't
        // depend on what's in the real config file on the machine running
        // it (a real value each time, just not a fixed one).
        let hash = "preview-plan-test-song-b";
        let plan = preview_full_analysis_plan(hash).expect("baseline graph always plans");
        let expected =
            AnalysisProfileSnapshot::from_app_config(&crate::config::AppConfig::load(), hash);
        assert_eq!(plan.profile_snapshot, expected);
    }

    /// See `library_db::reconnect_for_test` -- shared crate-wide so
    /// isolation holds across every module's DB-touching tests, not just
    /// within this one.
    fn isolated_test_db(label: &str) -> std::sync::MutexGuard<'static, ()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "uta-studio-analyzer-plan-preview-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        crate::library_db::reconnect_for_test(&dir)
    }

    #[test]
    fn a_saved_song_profile_flows_into_the_previewed_plan() {
        let _guard = isolated_test_db("flows-in");
        let hash = "preview-plan-test-song-c";
        let saved = AnalysisProfileSnapshot {
            separator: "native_workflow".to_string(),
            alignment_backend: "mms_karaoke".to_string(),
            asr_engine: "transcript_fusion".to_string(),
            requested_device: "cuda".to_string(),
            language_override: Some("ja".to_string()),
        };
        set_song_analysis_profile(hash, &saved).expect("save profile");

        let plan = preview_full_analysis_plan(hash).expect("baseline graph always plans");

        assert_eq!(plan.profile_snapshot, saved);
    }

    #[test]
    fn selection_preview_with_nothing_disabled_matches_the_full_preview() {
        // Same shared `preview_analysis_request_for` resolution path -- an
        // empty disabled set should produce an identical plan to
        // `preview_full_analysis_plan`, not a second, potentially-drifted
        // copy of the same logic.
        let hash = "preview-plan-test-song-selection-empty";
        let full = preview_full_analysis_plan(hash).expect("baseline graph always plans");
        let selection = super::preview_analysis_plan_for_selection(hash, BTreeSet::new())
            .expect("baseline graph always plans");
        assert_eq!(full.nodes, selection.nodes);
        assert_eq!(full.profile_snapshot, selection.profile_snapshot);
    }

    // `disabling_pitch_extract_blocks_chart_build_candidate` deliberately
    // calls `analysis_plan::build_plan` directly with an explicit
    // `AnalysisRequest` (empty `model_availability`, which the planner
    // defaults to "available" for every node -- see that field's own doc
    // comment) rather than going through
    // `preview_analysis_plan_for_selection`. That function does a *real*
    // vendor/disk model-availability lookup (Phase 8 §8.6, by design) --
    // in the `nix build` sandbox, no real models exist on disk, so
    // `pitch.extract`'s own parent (`stems.separate`) already comes back
    // `Blocked` for a missing model before the disable check even runs,
    // and `build_plan`'s "blocking parent" propagation
    // (`analysis_plan.rs`, checked *before* the explicit-disable branch)
    // marks `pitch.extract` `Blocked` too -- not because disabling it
    // didn't work, but because its environment-dependent parent state hid
    // it. This test is about disable/blocked precedence, not about which
    // models happen to be installed on the machine running it, so it
    // constructs a deterministic request instead of depending on real
    // disk state.
    #[test]
    fn disabling_pitch_extract_blocks_chart_build_candidate() {
        use crate::analysis_graph::baseline_graph_spec;
        use crate::analysis_plan::{AnalysisRequest, LyricsRoute, NodeState, build_plan};

        let request = AnalysisRequest {
            file_hash: "preview-plan-test-song-selection-disable-pitch".to_string(),
            targets: BTreeSet::from([AnalysisNodeId::new("chart.build_candidate")]),
            disabled_nodes: BTreeSet::from([AnalysisNodeId::new("pitch.extract")]),
            frozen_artifacts: BTreeSet::new(),
            bypassed_nodes: BTreeSet::new(),
            lyrics_route: LyricsRoute::GeneratedLyrics,
            model_availability: std::collections::BTreeMap::new(),
            profile_snapshot: AnalysisProfileSnapshot::default(),
            active_stem_nodes: BTreeSet::new(),
            audio_processing: None,
            workflow_execution: None,
        };
        let plan =
            build_plan(&baseline_graph_spec(), &request).expect("baseline graph always plans");

        assert_eq!(
            plan.node(&AnalysisNodeId::new("pitch.extract"))
                .unwrap()
                .state,
            NodeState::Disabled
        );
        assert_eq!(
            plan.node(&AnalysisNodeId::new("chart.build_candidate"))
                .unwrap()
                .state,
            NodeState::Blocked
        );
    }
}

#[cfg(test)]
mod frozen_config_tests {
    //! Phase 4 §4.1 "Enqueue 时冻结配置": a queued job must run with the
    //! config snapshot captured when it joined the queue, not whatever the
    //! user has changed global settings to by the time a worker thread
    //! actually picks it up.
    use super::{AppConfig, FROZEN_CONFIGS, resolve_frozen_config};
    use std::sync::Mutex;

    /// `FROZEN_CONFIGS` is a process-wide singleton; serialize tests that
    /// touch it, same reasoning as `pending_intent_tests`'s guard.
    static GUARD: Mutex<()> = Mutex::new(());

    fn config_with_model(model: &str) -> AppConfig {
        AppConfig {
            whisper_model: Some(model.to_string()),
            ..AppConfig::default()
        }
    }

    #[test]
    fn resolve_frozen_config_returns_and_drains_the_frozen_snapshot() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "frozen-config-test-song";
        FROZEN_CONFIGS
            .lock()
            .unwrap()
            .insert(hash.to_string(), config_with_model("frozen-model"));

        let resolved = resolve_frozen_config(hash, hash, || config_with_model("fallback-model"));
        assert_eq!(resolved.whisper_model.as_deref(), Some("frozen-model"));

        // Drained -- a second resolve for the same hash must not see the
        // same snapshot reused; it should fall back.
        let resolved_again =
            resolve_frozen_config(hash, hash, || config_with_model("fallback-model"));
        assert_eq!(
            resolved_again.whisper_model.as_deref(),
            Some("fallback-model")
        );
    }

    #[test]
    fn resolve_frozen_config_falls_back_when_nothing_was_frozen() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "frozen-config-test-song-missing";
        FROZEN_CONFIGS.lock().unwrap().remove(hash);

        let resolved = resolve_frozen_config(hash, hash, || config_with_model("fallback-model"));
        assert_eq!(resolved.whisper_model.as_deref(), Some("fallback-model"));
    }

    #[test]
    fn resolve_frozen_config_finds_a_snapshot_stored_under_the_pre_rekey_hash() {
        // A remote song's hash can change between enqueue (frozen under
        // the pre-rekey hash) and process_song reaching this point (now
        // using the real, rekeyed hash).
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let initial_hash = "frozen-config-test-song-initial";
        let real_hash = "frozen-config-test-song-real";
        FROZEN_CONFIGS.lock().unwrap().remove(real_hash);
        FROZEN_CONFIGS
            .lock()
            .unwrap()
            .insert(initial_hash.to_string(), config_with_model("frozen-model"));

        let resolved = resolve_frozen_config(real_hash, initial_hash, || {
            config_with_model("fallback-model")
        });
        assert_eq!(resolved.whisper_model.as_deref(), Some("frozen-model"));
    }

    #[test]
    fn resolve_frozen_config_prefers_the_current_hash_over_the_initial_one() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let initial_hash = "frozen-config-test-song-initial-2";
        let real_hash = "frozen-config-test-song-real-2";
        FROZEN_CONFIGS
            .lock()
            .unwrap()
            .insert(initial_hash.to_string(), config_with_model("initial-model"));
        FROZEN_CONFIGS
            .lock()
            .unwrap()
            .insert(real_hash.to_string(), config_with_model("real-model"));

        let resolved = resolve_frozen_config(real_hash, initial_hash, || {
            config_with_model("fallback-model")
        });
        assert_eq!(resolved.whisper_model.as_deref(), Some("real-model"));

        // The initial-hash entry was never touched by this resolve call
        // (current-hash entry took priority), so drain it manually to
        // avoid leaking state into other tests.
        FROZEN_CONFIGS.lock().unwrap().remove(initial_hash);
    }
}

#[cfg(test)]
mod pending_intent_tests {
    use super::{PENDING_NODE_INTENTS, mark_stems_only, pipeline_flags_for_request};
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    /// `PENDING_NODE_INTENTS` is a process-wide singleton; serialize tests
    /// that touch it so they can't interleave and observe each other's
    /// stashed intents.
    static GUARD: Mutex<()> = Mutex::new(());

    #[test]
    fn mark_stems_only_stashes_a_pitch_extract_target() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "pending-intent-test-song";
        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);

        mark_stems_only(hash);

        let intents = PENDING_NODE_INTENTS.lock().unwrap();
        let intent = intents.get(hash).expect("intent must be stashed");
        assert!(
            intent
                .targets
                .contains(&crate::analysis_graph::AnalysisNodeId::new("pitch.extract"))
        );
        assert!(!intent.force_transcribe);
        drop(intents);
        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);
    }

    #[test]
    fn stashed_pitch_extract_target_resolves_to_skip_transcription_only() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "pending-intent-resolve-test-song";
        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);
        mark_stems_only(hash);

        let targets = PENDING_NODE_INTENTS
            .lock()
            .unwrap()
            .remove(hash)
            .unwrap()
            .targets;
        let flags = pipeline_flags_for_request(
            &targets,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .unwrap();
        assert!(flags.skip_transcription);
        assert!(!flags.skip_separation);
    }
}

#[cfg(test)]
mod compare_analysis_runs_tests {
    //! Phase 6 `compare_analysis_runs` / Phase 7 §7.5 "Compare with
    //! previous attempt". `compare_analysis_runs_from` is a pure function
    //! over already-loaded rows, so these build fixtures directly instead
    //! of needing a real DB.
    use super::{
        AnalysisRunHistory, NodeAttempt, compare_analysis_runs_from,
        compare_node_attempt_with_previous_run,
    };

    fn run(id: i64, file_hash: &str, finished_at_ms: i64) -> AnalysisRunHistory {
        AnalysisRunHistory {
            id,
            file_hash: file_hash.to_string(),
            title: "Test Song".to_string(),
            artist: "Test Artist".to_string(),
            status: "completed".to_string(),
            started_at_ms: finished_at_ms - 1000,
            finished_at_ms,
            error_message: None,
            log_path: None,
            snapshot: super::AnalysisProgressSnapshot {
                stage: "complete".to_string(),
                overall_progress: 100,
                stage_progress: 100,
                operation: String::new(),
                detail: String::new(),
                implementation: String::new(),
                model: String::new(),
                device: String::new(),
                requested_device: String::new(),
                fallback_from: None,
                fallback_reason: None,
                backend_fallback_from: None,
                backend_fallback_reason: None,
                stage_routes: Vec::new(),
                node_id: None,
                node_event: None,
                artifact_reused_reason: None,
                analysis_log_path: None,
            },
        }
    }

    fn attempt(run_id: i64, node_id: &str, status: &str, implementation: &str) -> NodeAttempt {
        NodeAttempt {
            id: 1,
            run_id,
            file_hash: "songA".to_string(),
            node_id: node_id.to_string(),
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
    fn a_node_run_in_both_with_the_same_fields_has_no_changed_fields() {
        let history = vec![run(1, "songA", 1_000), run(2, "songA", 2_000)];
        let result = compare_analysis_runs_from(
            &history,
            1,
            vec![attempt(1, "pitch.extract", "succeeded", "RMVPE")],
            2,
            vec![attempt(2, "pitch.extract", "succeeded", "RMVPE")],
        )
        .unwrap();
        let diff = result
            .node_differences
            .iter()
            .find(|d| d.node_id == "pitch.extract")
            .unwrap();
        assert!(diff.changed_fields.is_empty());
        assert!(diff.attempt_a.is_some());
        assert!(diff.attempt_b.is_some());
    }

    #[test]
    fn a_changed_implementation_is_reported() {
        let history = vec![run(1, "songA", 1_000), run(2, "songA", 2_000)];
        let result = compare_analysis_runs_from(
            &history,
            1,
            vec![attempt(1, "stems.separate", "succeeded", "RoFormer")],
            2,
            vec![attempt(2, "stems.separate", "succeeded", "RoFormer v2")],
        )
        .unwrap();
        let diff = result
            .node_differences
            .iter()
            .find(|d| d.node_id == "stems.separate")
            .unwrap();
        assert_eq!(diff.changed_fields, vec!["implementation"]);
    }

    #[test]
    fn a_node_only_attempted_in_one_run_has_no_changed_fields_but_a_missing_side() {
        let history = vec![run(1, "songA", 1_000), run(2, "songA", 2_000)];
        let result = compare_analysis_runs_from(
            &history,
            1,
            vec![attempt(1, "pitch.extract", "succeeded", "RMVPE")],
            2,
            vec![],
        )
        .unwrap();
        let diff = result
            .node_differences
            .iter()
            .find(|d| d.node_id == "pitch.extract")
            .unwrap();
        assert!(diff.attempt_a.is_some());
        assert!(diff.attempt_b.is_none());
        assert!(diff.changed_fields.is_empty());
    }

    #[test]
    fn comparing_runs_from_different_songs_is_rejected() {
        let history = vec![run(1, "songA", 1_000), run(2, "songB", 2_000)];
        let result = compare_analysis_runs_from(&history, 1, vec![], 2, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn an_unknown_run_id_is_rejected() {
        let history = vec![run(1, "songA", 1_000)];
        let result = compare_analysis_runs_from(&history, 1, vec![], 999, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn compare_with_previous_run_needs_a_real_history_lookup() {
        // compare_node_attempt_with_previous_run calls load_analysis_history
        // itself (real DB), so without a matching real run id this must
        // fail cleanly rather than panic -- the actual "found a previous
        // run" path is covered indirectly via compare_analysis_runs_from
        // above, same DB-avoidance reasoning as cancel_analysis_run_tests.
        let result = compare_node_attempt_with_previous_run(
            "compare-test-song-never-analyzed-xyz",
            "pitch.extract",
            999_999_999,
        );
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod cancel_analysis_run_tests {
    //! Deliberately does not cover the success path (actually removing a
    //! queued hash): `ANALYZER` is a real process-wide singleton with no
    //! test-injection seam, so mutating `state.queue` from a test risks
    //! interleaving with any other test that touches it -- same caution
    //! `run_analysis_plan_tests` below already documents for
    //! `enqueue_one`. The rejection path needs no such mutation.
    use super::{cancel_analysis_run, stop_analysis_run};

    #[test]
    fn cancelling_a_hash_that_was_never_queued_is_rejected() {
        let error = cancel_analysis_run("cancel-test-hash-never-queued-xyz")
            .expect_err("a hash that was never queued cannot be cancelled");
        assert!(error.contains("not currently queued"));
    }

    #[test]
    fn stopping_a_hash_that_was_never_queued_is_rejected() {
        let error = stop_analysis_run("stop-test-hash-never-queued-xyz")
            .expect_err("a hash that was never queued or running cannot be stopped");
        assert!(error.contains("not currently queued or running"));
    }
}

#[cfg(test)]
mod stopped_run_cleanup_tests {
    use super::cleanup_unfinished_output_temps;
    use crate::cache::CacheDir;

    #[test]
    fn cleanup_removes_only_matching_temporary_outputs() {
        let path = std::env::temp_dir().join(format!(
            "uta-studio-stop-cleanup-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let cache = CacheDir { path };
        let unfinished = cache.path.join(".song_pitch_track.json.random.tmp");
        let final_output = cache.path.join("song_pitch_track.json");
        let other_song = cache.path.join(".other_pitch_track.json.random.tmp");
        for fixture in [&unfinished, &final_output, &other_song] {
            std::fs::write(fixture, b"fixture").unwrap();
        }

        cleanup_unfinished_output_temps(&cache, "song");

        assert!(!unfinished.exists());
        assert!(final_output.exists());
        assert!(other_song.exists());
        cache.clear_all();
    }
}

#[cfg(test)]
mod run_analysis_plan_tests {
    // Deliberately does not cover `run_analysis_plan`'s success path: that
    // path calls `enqueue_one`, which spawns a real background worker
    // thread and touches the process-wide `ANALYZER`/library_db state --
    // out of scope for a unit test (`pipeline_flags_for_request`'s own
    // tests above already cover the flag-derivation logic this success
    // path relies on). Every case here is a rejection, which returns
    // before either of those side effects happen.
    use super::{node_can_be_disabled_for_run, run_analysis_plan};
    use std::collections::BTreeSet;

    #[test]
    fn rejects_disabling_a_node_the_pipeline_cannot_honor() {
        let result = run_analysis_plan(
            "run-analysis-plan-test-song",
            BTreeSet::new(),
            BTreeSet::from([crate::analysis_graph::AnalysisNodeId::new(
                "music.descriptors",
            )]),
        );
        let error = result.expect_err("music.descriptors cannot be gated by run_pipeline yet");
        assert!(error.contains("music.descriptors"));
        assert!(!node_can_be_disabled_for_run("music.descriptors"));
    }

    #[test]
    fn rejects_disabling_an_always_required_node() {
        let result = run_analysis_plan(
            "run-analysis-plan-test-song-2",
            BTreeSet::new(),
            BTreeSet::from([crate::analysis_graph::AnalysisNodeId::new(
                "chart.build_candidate",
            )]),
        );
        assert!(result.is_err());
    }

    #[test]
    fn every_pipeline_honorable_node_reports_itself_as_disableable() {
        for node_id in [
            "stems.separate",
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.align",
            "lyrics.import_timed",
        ] {
            assert!(
                node_can_be_disabled_for_run(node_id),
                "{node_id} should be disableable"
            );
        }
        for node_id in [
            "music.key",
            "music.rhythm",
            "preflight",
            "chart.build_candidate",
        ] {
            assert!(
                !node_can_be_disabled_for_run(node_id),
                "{node_id} should not be disableable"
            );
        }
    }
}

#[cfg(test)]
mod downstream_closure_tests {
    //! §7.5 "Run this node and downstream". `downstream_closure` is pure
    //! graph traversal over the real `baseline_graph_spec` edges, so these
    //! lock its shape directly against the graph rather than against
    //! `run_analysis_node_downstream`'s side-effecting success path (which
    //! -- like `run_analysis_plan`'s own success path -- calls
    //! `enqueue_one` and touches process-wide state, out of scope here).
    use super::downstream_closure;
    use crate::analysis_graph::{AnalysisNodeId, baseline_graph_spec};

    fn ids(values: &[&str]) -> std::collections::BTreeSet<AnalysisNodeId> {
        values.iter().map(|s| AnalysisNodeId::new(*s)).collect()
    }

    #[test]
    fn a_leaf_nodes_downstream_closure_is_only_itself() {
        let graph = baseline_graph_spec();
        assert_eq!(
            downstream_closure(&graph, &AnalysisNodeId::new("chart.build_candidate")),
            ids(&["chart.build_candidate"])
        );
    }

    #[test]
    fn stems_downstream_includes_pitch_and_the_lyrics_route_but_not_import_timed() {
        // lyrics.import_timed is fed directly by preflight (Timed LRC
        // doesn't need a vocal stem), so it must not appear here.
        let graph = baseline_graph_spec();
        assert_eq!(
            downstream_closure(&graph, &AnalysisNodeId::new("stems.separate")),
            ids(&[
                "stems.separate",
                "stems.bind_analysis_outputs",
                "pitch.extract",
                "lyrics.preprocess",
                "lyrics.transcribe",
                "lyrics.align",
                "chart.build_candidate",
            ])
        );
    }

    #[test]
    fn preflights_downstream_closure_is_the_entire_graph() {
        let graph = baseline_graph_spec();
        let closure = downstream_closure(&graph, &AnalysisNodeId::new("preflight"));
        let connected: std::collections::BTreeSet<_> = graph
            .edges
            .iter()
            .flat_map(|edge| [edge.from.clone(), edge.to.clone()])
            .collect();
        for node in &graph.nodes {
            if !connected.contains(&node.id) {
                continue;
            }
            assert!(
                closure.contains(&node.id),
                "{} missing from closure",
                node.id
            );
        }
    }

    #[test]
    fn pitch_downstream_never_pulls_in_its_own_ancestor_stems_separate() {
        let graph = baseline_graph_spec();
        let closure = downstream_closure(&graph, &AnalysisNodeId::new("pitch.extract"));
        assert!(!closure.contains(&AnalysisNodeId::new("stems.separate")));
        assert!(closure.contains(&AnalysisNodeId::new("chart.build_candidate")));
    }
}

#[cfg(test)]
mod freeze_analysis_node_tests {
    //! Phase 4 §4.5 Freeze consumer. `node_can_be_frozen_for_run` and
    //! `freeze_analysis_node_outputs_for_run` both check the same two
    //! preconditions (`pipeline_can_honor_freeze` + on-disk output
    //! existence); these tests exercise the pieces that don't need the real
    //! global data directory (`pipeline_can_honor_freeze`,
    //! `frozen_artifact_kinds_for_node`, `node_output_exists_for_freeze`
    //! against a temp `CacheDir`) directly, the same way
    //! `reanalysis_backup_tests` below tests cache-path logic without
    //! touching a real song.
    use super::{
        frozen_artifact_kinds_for_node, node_output_exists_for_freeze, pipeline_can_honor_freeze,
    };
    use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
    use crate::cache::CacheDir;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_cache_dir(label: &str) -> CacheDir {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-freeze-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp cache dir");
        CacheDir { path }
    }

    #[test]
    fn only_stems_and_pitch_are_freezable() {
        for id in ["stems.separate", "pitch.extract"] {
            assert!(
                pipeline_can_honor_freeze(&AnalysisNodeId::new(id)),
                "{id} should be freezable"
            );
        }
        // Lyrics nodes share one merged transcript.json -- no standalone
        // file to freeze independently until Phase 4 §4.4 artifact
        // splitting exists.
        for id in [
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.align",
            "lyrics.import_timed",
            "music.analysis",
            "preflight",
            "chart.build_candidate",
        ] {
            assert!(
                !pipeline_can_honor_freeze(&AnalysisNodeId::new(id)),
                "{id} should not be freezable yet"
            );
        }
    }

    #[test]
    fn frozen_artifact_kinds_map_to_the_nodes_real_outputs() {
        assert_eq!(
            frozen_artifact_kinds_for_node(&AnalysisNodeId::new("stems.separate")),
            std::collections::BTreeSet::from([
                ArtifactKind::VocalStem,
                ArtifactKind::InstrumentalStem
            ]),
        );
        assert_eq!(
            frozen_artifact_kinds_for_node(&AnalysisNodeId::new("pitch.extract")),
            std::collections::BTreeSet::from([
                ArtifactKind::PitchTrack,
                ArtifactKind::PitchNoteCandidates
            ]),
        );
        assert!(frozen_artifact_kinds_for_node(&AnalysisNodeId::new("music.analysis")).is_empty());
    }

    #[test]
    fn stems_output_missing_is_not_freezable() {
        let cache = temp_cache_dir("stems-missing");
        assert!(!node_output_exists_for_freeze(
            &cache,
            "songA",
            &AnalysisNodeId::new("stems.separate")
        ));
        cache.clear_all();
    }

    #[test]
    fn stems_output_requires_both_vocal_and_instrumental_files() {
        let cache = temp_cache_dir("stems-partial");
        std::fs::write(cache.vocals_path("songA"), b"fake-audio").unwrap();
        // Instrumental missing -- must not report freezable on vocals alone.
        assert!(!node_output_exists_for_freeze(
            &cache,
            "songA",
            &AnalysisNodeId::new("stems.separate")
        ));
        std::fs::write(cache.instrumental_path("songA"), b"fake-audio").unwrap();
        assert!(node_output_exists_for_freeze(
            &cache,
            "songA",
            &AnalysisNodeId::new("stems.separate")
        ));
        cache.clear_all();
    }

    #[test]
    fn pitch_output_requires_both_track_and_notes_files() {
        let cache = temp_cache_dir("pitch-partial");
        std::fs::write(cache.pitch_track_path("songA"), b"{}").unwrap();
        assert!(!node_output_exists_for_freeze(
            &cache,
            "songA",
            &AnalysisNodeId::new("pitch.extract")
        ));
        std::fs::write(cache.pitch_notes_path("songA"), b"{}").unwrap();
        assert!(node_output_exists_for_freeze(
            &cache,
            "songA",
            &AnalysisNodeId::new("pitch.extract")
        ));
        cache.clear_all();
    }

    #[test]
    fn a_different_song_hash_never_sees_another_songs_frozen_output() {
        let cache = temp_cache_dir("cross-song");
        std::fs::write(cache.vocals_path("songA"), b"fake-audio").unwrap();
        std::fs::write(cache.instrumental_path("songA"), b"fake-audio").unwrap();
        assert!(!node_output_exists_for_freeze(
            &cache,
            "songB",
            &AnalysisNodeId::new("stems.separate")
        ));
        cache.clear_all();
    }
}

#[cfg(test)]
mod bypass_analysis_node_tests {
    //! Phase 4 §4.5 Bypass consumer.
    use super::{node_can_be_bypassed_for_run, pipeline_can_honor_bypass};
    use crate::analysis_graph::AnalysisNodeId;

    #[test]
    fn only_stems_separate_can_be_bypassed() {
        assert!(pipeline_can_honor_bypass(&AnalysisNodeId::new(
            "stems.separate"
        )));
        for id in [
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.align",
            "lyrics.import_timed",
            "music.analysis",
            "preflight",
            "chart.build_candidate",
        ] {
            assert!(
                !pipeline_can_honor_bypass(&AnalysisNodeId::new(id)),
                "{id} should not be bypassable yet"
            );
        }
    }

    #[test]
    fn node_can_be_bypassed_for_run_has_no_per_song_existence_check() {
        // Unlike Freeze, Bypass's substitute input is the song's own source
        // media -- always present for a real song, so this is purely
        // structural and doesn't need a real file_hash to answer correctly.
        assert!(node_can_be_bypassed_for_run("stems.separate"));
        assert!(!node_can_be_bypassed_for_run("pitch.extract"));
    }
}

#[cfg(test)]
mod configure_node_tests {
    //! Phase 8's Run tier (§8.4's previously-missing third tier).
    //! Deliberately does not cover `configure_analysis_node_for_run`'s
    //! success path (calls `enqueue_one`, same real-side-effect concern
    //! `run_analysis_plan_tests` already documents) -- only its rejection
    //! path, plus the pure mapping and the `PENDING_NODE_INTENTS`
    //! read-through, which don't touch the real analyzer process.
    use super::{
        PENDING_NODE_INTENTS, configure_analysis_node_for_run, node_can_be_configured_for_run,
        pending_run_override_for, save_node_config_as_song_profile,
    };
    use crate::analysis_profile::{
        AnalysisProfileSnapshot, ProfileField, get_song_analysis_profile, set_song_analysis_profile,
    };
    use crate::config::AppConfig;
    use std::sync::Mutex;

    /// `PENDING_NODE_INTENTS` is a process-wide singleton; serialize tests
    /// that touch it, same reasoning as `pending_intent_tests`'s guard.
    static GUARD: Mutex<()> = Mutex::new(());

    fn isolated_test_db(label: &str) -> std::sync::MutexGuard<'static, ()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "uta-studio-configure-node-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        crate::library_db::reconnect_for_test(&dir)
    }

    #[test]
    fn only_lyrics_model_nodes_are_configurable() {
        for id in ["lyrics.transcribe", "lyrics.align"] {
            assert!(
                node_can_be_configured_for_run(id),
                "{id} should be configurable"
            );
        }
        for id in [
            "stems.separate",
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.import_timed",
            "music.analysis",
            "preflight",
            "chart.build_candidate",
        ] {
            assert!(
                !node_can_be_configured_for_run(id),
                "{id} should not be configurable"
            );
        }
    }

    #[test]
    fn configure_and_save_both_reject_a_node_with_no_controllable_field() {
        let error = configure_analysis_node_for_run("some-song", "music.analysis", "x".into())
            .expect_err("music.analysis has no profile-controlled parameter");
        assert!(error.contains("music.analysis"));

        let error = save_node_config_as_song_profile("some-song", "music.analysis")
            .expect_err("music.analysis has no profile-controlled parameter");
        assert!(error.contains("music.analysis"));
    }

    #[test]
    fn pending_run_override_only_surfaces_for_the_field_the_node_maps_to() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "configure-node-test-pending-override";
        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);
        {
            let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
            intents.entry(hash.to_string()).or_default().run_override =
                Some((ProfileField::AsrEngine, "whisper".to_string()));
        }

        assert_eq!(
            pending_run_override_for(hash, "lyrics.transcribe"),
            Some("whisper".to_string())
        );
        // Alignment maps to a different field; the ASR override must not leak.
        assert_eq!(pending_run_override_for(hash, "lyrics.align"), None);

        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);
    }

    #[test]
    fn pending_run_override_is_none_when_nothing_is_queued() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "configure-node-test-no-pending-override";
        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);
        assert_eq!(pending_run_override_for(hash, "stems.separate"), None);
    }

    #[test]
    fn save_as_song_profile_preserves_other_fields_when_saving_just_one() {
        let _db_guard = isolated_test_db("preserve-others");
        let hash = "configure-node-test-song-preserve";
        let seeded = AnalysisProfileSnapshot {
            alignment_backend: "mms_karaoke".to_string(),
            ..AnalysisProfileSnapshot::from_app_config(&AppConfig::load(), hash)
        };
        set_song_analysis_profile(hash, &seeded).unwrap();

        save_node_config_as_song_profile(hash, "lyrics.transcribe").unwrap();

        let saved = get_song_analysis_profile(hash).unwrap();
        assert_eq!(saved.alignment_backend, "mms_karaoke");
        assert_eq!(saved.separator, seeded.separator);
    }

    #[test]
    fn save_as_song_profile_seeds_a_fresh_profile_from_real_global_defaults() {
        let _db_guard = isolated_test_db("fresh-profile");
        let hash = "configure-node-test-song-fresh";
        assert!(get_song_analysis_profile(hash).is_none());

        save_node_config_as_song_profile(hash, "lyrics.transcribe").unwrap();

        let saved = get_song_analysis_profile(hash).expect("a profile now exists");
        let expected_global = AnalysisProfileSnapshot::from_app_config(&AppConfig::load(), hash);
        assert_eq!(saved.asr_engine, expected_global.asr_engine);
        // Untouched fields are also seeded from the real global config, not
        // left as `AnalysisProfileSnapshot::default()`'s hardcoded values.
        assert_eq!(saved.separator, expected_global.separator);
        assert_eq!(saved.alignment_backend, expected_global.alignment_backend);
    }
}

#[cfg(test)]
mod reanalysis_backup_tests {
    //! Phase 5 fix (docs/analysis-dag-redesign.md, phase plan §9.2 "失败时
    //! 保留旧 Pitch"): `reanalyze_pitch` used to delete old pitch data
    //! *before* the rerun was even queued, so a failed/crashed/OOM-killed
    //! rerun permanently destroyed the previous good output. These tests
    //! lock the rename-instead-of-delete + existence-based
    //! restore-or-commit behavior that replaced it.
    use super::{apply_pitch_reanalysis_reset, back_up_before_reset, restore_or_commit_backup};
    use crate::cache::CacheDir;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_cache_dir(label: &str) -> CacheDir {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-reanalysis-backup-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp cache dir");
        CacheDir { path }
    }

    #[test]
    fn back_up_before_reset_renames_an_existing_file_and_returns_the_pair() {
        let cache = temp_cache_dir("rename");
        let original = cache.path.join("pitch.json");
        std::fs::write(&original, b"old pitch data").unwrap();

        let (returned_original, backup) = back_up_before_reset(&original).unwrap();

        assert_eq!(returned_original, original);
        assert!(!original.is_file(), "original must be moved, not copied");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old pitch data");
        cache.clear_all();
    }

    #[test]
    fn back_up_before_reset_returns_none_when_nothing_exists_to_back_up() {
        let cache = temp_cache_dir("missing");
        let original = cache.path.join("does-not-exist.json");
        assert!(back_up_before_reset(&original).is_none());
        cache.clear_all();
    }

    #[test]
    fn back_up_before_reset_clears_a_stale_leftover_backup_first() {
        // A .bak from some earlier, never-resolved run must not silently
        // become "the" backup for this run's original content.
        let cache = temp_cache_dir("stale");
        let original = cache.path.join("pitch.json");
        let mut backup_name = original.as_os_str().to_os_string();
        backup_name.push(".bak");
        let stale_backup = std::path::PathBuf::from(&backup_name);
        std::fs::write(&stale_backup, b"stale leftover").unwrap();
        std::fs::write(&original, b"current data").unwrap();

        back_up_before_reset(&original).unwrap();

        assert_eq!(std::fs::read(&stale_backup).unwrap(), b"current data");
        cache.clear_all();
    }

    #[test]
    fn restore_or_commit_backup_deletes_the_backup_when_a_fresh_file_was_written() {
        let cache = temp_cache_dir("commit");
        let original = cache.path.join("pitch.json");
        let backup = cache.path.join("pitch.json.bak");
        std::fs::write(&backup, b"old data").unwrap();
        std::fs::write(&original, b"freshly regenerated data").unwrap();

        restore_or_commit_backup(&original, &backup);

        assert!(!backup.is_file());
        assert_eq!(
            std::fs::read(&original).unwrap(),
            b"freshly regenerated data"
        );
        cache.clear_all();
    }

    #[test]
    fn restore_or_commit_backup_restores_the_old_file_when_the_run_produced_nothing() {
        // The exact bug being fixed: a failed/crashed/OOM-killed rerun (or
        // pipeline.py's analyze_pitch catching its own exception and
        // continuing without writing anything) must not leave the song
        // pitch-less.
        let cache = temp_cache_dir("restore");
        let original = cache.path.join("pitch.json");
        let backup = cache.path.join("pitch.json.bak");
        std::fs::write(&backup, b"old good pitch data").unwrap();

        restore_or_commit_backup(&original, &backup);

        assert!(!backup.is_file());
        assert_eq!(std::fs::read(&original).unwrap(), b"old good pitch data");
        cache.clear_all();
    }

    #[test]
    fn apply_pitch_reanalysis_reset_backs_up_both_pitch_files_and_leaves_neither_at_its_original_path()
     {
        let cache = temp_cache_dir("apply-reset");
        let hash = "songPitchReset";
        std::fs::write(cache.pitch_track_path(hash), b"track data").unwrap();
        std::fs::write(cache.pitch_notes_path(hash), b"notes data").unwrap();

        let backups = apply_pitch_reanalysis_reset(&cache, hash);

        assert_eq!(backups.len(), 2);
        assert!(!cache.pitch_track_path(hash).is_file());
        assert!(!cache.pitch_notes_path(hash).is_file());
        for (original, backup) in &backups {
            assert!(!original.is_file());
            assert!(backup.is_file());
        }
        cache.clear_all();
    }

    #[test]
    fn apply_pitch_reanalysis_reset_is_a_noop_when_no_prior_pitch_data_exists() {
        // A song being analyzed for the first time (or one whose pitch
        // extraction already failed and left nothing behind) must not
        // error or fabricate a backup out of nothing.
        let cache = temp_cache_dir("apply-reset-empty");
        let hash = "songNeverAnalyzed";

        let backups = apply_pitch_reanalysis_reset(&cache, hash);

        assert!(backups.is_empty());
        cache.clear_all();
    }

    // The realign/reanalyze extension of the same fix (docs/plan.md §2 item
    // 5, "realign/reanalyze_full 的同款急切删除问题"): identical
    // trigger-time eager-delete bug over a larger, directory-scanned file
    // set, now made safe the same way instead of left as a known gap.
    use super::{apply_realign_reset, apply_reanalyze_reset};

    #[test]
    fn apply_realign_reset_backs_up_the_transcript_and_every_variant() {
        let cache = temp_cache_dir("realign-reset");
        let hash = "songRealign";
        std::fs::write(cache.transcript_path(hash), b"base transcript").unwrap();
        std::fs::write(
            cache.variant_transcript_path(hash, 1.25),
            b"variant transcript",
        )
        .unwrap();

        let backups = apply_realign_reset(&cache, hash);

        assert_eq!(backups.len(), 2);
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.variant_transcript_path(hash, 1.25).is_file());
        for (original, backup) in &backups {
            assert!(!original.is_file());
            assert!(backup.is_file());
        }
        cache.clear_all();
    }

    #[test]
    fn apply_realign_reset_also_backs_up_the_split_transcript_artifacts_when_present() {
        // §4.4: a realign must not leave stale recognized_text/asr_segments
        // from the previous run behind once transcript.json/timed_transcript.json
        // regenerate fresh.
        let cache = temp_cache_dir("realign-reset-split");
        let hash = "songRealignSplit";
        std::fs::write(cache.transcript_path(hash), b"base transcript").unwrap();
        std::fs::write(cache.recognized_text_path(hash), b"recognized").unwrap();
        std::fs::write(cache.asr_segments_path(hash), b"segments").unwrap();
        std::fs::write(cache.timed_transcript_path(hash), b"timed").unwrap();

        let backups = apply_realign_reset(&cache, hash);

        assert_eq!(backups.len(), 4);
        assert!(!cache.recognized_text_path(hash).is_file());
        assert!(!cache.asr_segments_path(hash).is_file());
        assert!(!cache.timed_transcript_path(hash).is_file());
        for (original, backup) in &backups {
            assert!(!original.is_file());
            assert!(backup.is_file());
        }
        cache.clear_all();
    }

    #[test]
    fn apply_realign_reset_leaves_the_authored_chart_alone() {
        let cache = temp_cache_dir("realign-reset-chart");
        let hash = "songRealignChart";
        std::fs::write(cache.transcript_path(hash), b"base transcript").unwrap();
        std::fs::write(cache.vocal_chart_path(hash), b"authored chart").unwrap();

        apply_realign_reset(&cache, hash);

        assert!(cache.vocal_chart_path(hash).is_file());
        assert_eq!(
            std::fs::read(cache.vocal_chart_path(hash)).unwrap(),
            b"authored chart"
        );
        cache.clear_all();
    }

    #[test]
    fn apply_reanalyze_reset_transcript_only_backs_up_transcript_lyrics_and_variants_but_not_pitch()
    {
        let cache = temp_cache_dir("reanalyze-transcript-reset");
        let hash = "songReanalyzeTranscript";
        std::fs::write(cache.transcript_path(hash), b"transcript").unwrap();
        std::fs::write(cache.lyrics_path(hash), b"lyrics").unwrap();
        std::fs::write(cache.variant_transcript_path(hash, 0.8), b"variant").unwrap();
        std::fs::write(cache.pitch_track_path(hash), b"pitch track").unwrap();

        let backups = apply_reanalyze_reset(&cache, hash, false);

        assert_eq!(backups.len(), 3);
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.lyrics_path(hash).is_file());
        assert!(!cache.variant_transcript_path(hash, 0.8).is_file());
        // Transcript-only reanalysis must not touch pitch data at all --
        // neither delete it nor back it up.
        assert!(cache.pitch_track_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn apply_reanalyze_reset_transcript_only_also_backs_up_the_split_transcript_artifacts() {
        let cache = temp_cache_dir("reanalyze-transcript-reset-split");
        let hash = "songReanalyzeTranscriptSplit";
        std::fs::write(cache.transcript_path(hash), b"transcript").unwrap();
        std::fs::write(cache.recognized_text_path(hash), b"recognized").unwrap();
        std::fs::write(cache.asr_segments_path(hash), b"segments").unwrap();
        std::fs::write(cache.timed_transcript_path(hash), b"timed").unwrap();
        std::fs::write(cache.pitch_track_path(hash), b"pitch track").unwrap();

        let backups = apply_reanalyze_reset(&cache, hash, false);

        assert_eq!(backups.len(), 4);
        assert!(!cache.recognized_text_path(hash).is_file());
        assert!(!cache.asr_segments_path(hash).is_file());
        assert!(!cache.timed_transcript_path(hash).is_file());
        // Transcript-only reanalysis must not touch pitch data.
        assert!(cache.pitch_track_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn apply_reanalyze_reset_full_backs_up_every_analysis_output_but_not_the_authored_chart() {
        let cache = temp_cache_dir("reanalyze-full-reset");
        let hash = "songReanalyzeFull";
        std::fs::write(cache.transcript_path(hash), b"transcript").unwrap();
        std::fs::write(cache.pitch_track_path(hash), b"pitch track").unwrap();
        std::fs::write(cache.pitch_notes_path(hash), b"pitch notes").unwrap();
        std::fs::write(cache.music_analysis_path(hash), b"music analysis").unwrap();
        std::fs::write(cache.vocal_chart_path(hash), b"authored chart").unwrap();

        let backups = apply_reanalyze_reset(&cache, hash, true);

        assert_eq!(backups.len(), 4);
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.pitch_track_path(hash).is_file());
        assert!(!cache.pitch_notes_path(hash).is_file());
        assert!(!cache.music_analysis_path(hash).is_file());
        for (original, backup) in &backups {
            assert!(!original.is_file());
            assert!(backup.is_file());
        }
        assert!(
            cache.vocal_chart_path(hash).is_file(),
            "full reanalysis must still preserve the Authored Chart by default"
        );
        cache.clear_all();
    }

    #[test]
    fn apply_reanalyze_reset_is_a_noop_when_nothing_exists_yet() {
        let cache = temp_cache_dir("reanalyze-reset-empty");
        let hash = "songReanalyzeNeverRun";

        assert!(apply_reanalyze_reset(&cache, hash, true).is_empty());
        assert!(apply_reanalyze_reset(&cache, hash, false).is_empty());
        cache.clear_all();
    }
}

#[cfg(test)]
mod node_attempt_tests {
    use super::{
        AnalysisProgressSnapshot, AnalysisStageRoute, node_attempt_status, record_node_attempts,
    };
    use crate::library_db;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-node-attempt-status-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp db root");
        path
    }

    #[test]
    fn node_attempt_status_maps_every_real_event_kind() {
        assert_eq!(node_attempt_status(Some("node_completed")), "succeeded");
        assert_eq!(node_attempt_status(Some("node_failed")), "failed");
        assert_eq!(node_attempt_status(Some("artifact_reused")), "reused");
        assert_eq!(node_attempt_status(Some("node_cancelled")), "cancelled");
    }

    #[test]
    fn node_attempt_status_treats_unterminated_or_unknown_events_as_incomplete() {
        // node_started/node_progress mean the node was reached but the run
        // ended (or moved on) before a terminal event -- not success, not
        // failure, and not silently dropped either.
        assert_eq!(node_attempt_status(Some("node_started")), "incomplete");
        assert_eq!(node_attempt_status(Some("node_progress")), "incomplete");
        assert_eq!(
            node_attempt_status(Some("something_unrecognized")),
            "incomplete"
        );
        assert_eq!(node_attempt_status(None), "incomplete");
    }

    fn route(node_id: Option<&str>, node_event: Option<&str>) -> AnalysisStageRoute {
        AnalysisStageRoute {
            stage: "pitch".to_string(),
            node_id: node_id.map(str::to_string),
            node_event: node_event.map(str::to_string),
            binding_kind: None,
            committed_outputs: Vec::new(),
            input_revision_ids: Vec::new(),
            operation: "Reference pitch extraction".to_string(),
            implementation: "RMVPE".to_string(),
            model: "RMVPE singing pitch model".to_string(),
            stage_progress: 100,
            requested_device: "cpu".to_string(),
            actual_device: "cpu".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms: None,
            finished_at_ms: None,
            event_at_ms: None,
            work_units_completed: None,
            work_units_total: None,
        }
    }

    #[test]
    fn record_node_attempts_skips_routes_without_a_real_node_id() {
        let root = temp_root("skip-legacy");
        let _guard = library_db::reconnect_for_test(&root);
        let run_id = library_db::analysis_history_insert(&library_db::NewAnalysisHistory {
            file_hash: "songE",
            title: "Title",
            artist: "Artist",
            status: "completed",
            started_at_ms: 1_000,
            finished_at_ms: 2_000,
            snapshot_json: "{}",
            error_message: None,
            log_path: None,
        })
        .expect("insert run");

        let snapshot = AnalysisProgressSnapshot {
            stage: "complete".into(),
            overall_progress: 100,
            stage_progress: 100,
            operation: "Analysis complete".into(),
            detail: String::new(),
            implementation: String::new(),
            model: String::new(),
            device: String::new(),
            requested_device: String::new(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            stage_routes: vec![
                route(Some("pitch.extract"), Some("node_completed")),
                route(None, None),
            ],
            node_id: None,
            node_event: None,
            artifact_reused_reason: None,
            analysis_log_path: None,
        };
        record_node_attempts(run_id, "songE", &snapshot);

        let attempts = library_db::analysis_node_attempts_load(run_id).expect("load attempts");
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].node_id, "pitch.extract");
        assert_eq!(attempts[0].status, "succeeded");

        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod analysis_log_cleanup_tests {
    use super::{analysis_log_line_matches_node, delete_analysis_logs_in};

    #[test]
    fn jsonl_node_filter_keeps_matching_node_and_run_level_records_only() {
        let matching = r#"{"record_type":"node_event","node_id":"stems.instrumental"}"#;
        let sibling = r#"{"record_type":"node_event","node_id":"stems.vocals"}"#;
        let matching_output =
            r#"{"record_type":"process_output","node_id":"stems.instrumental"}"#;
        let sibling_output =
            r#"{"record_type":"process_output","node_id":"stems.vocals"}"#;
        let terminal = r#"{"record_type":"history_terminal","status":"completed"}"#;

        assert!(analysis_log_line_matches_node(
            matching,
            Some("stems.instrumental")
        ));
        assert!(analysis_log_line_matches_node(
            matching_output,
            Some("stems.instrumental")
        ));
        assert!(!analysis_log_line_matches_node(
            sibling,
            Some("stems.instrumental")
        ));
        assert!(!analysis_log_line_matches_node(
            sibling_output,
            Some("stems.instrumental")
        ));
        assert!(analysis_log_line_matches_node(
            terminal,
            Some("stems.instrumental")
        ));
    }

    #[test]
    fn cleanup_removes_only_referenced_files_directly_inside_the_log_root() {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-analysis-log-cleanup-{}-{}",
            std::process::id(),
            super::unix_time_ms()
        ));
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("one.log"), b"one").unwrap();
        std::fs::write(root.join("unreferenced.log"), b"keep").unwrap();
        std::fs::write(nested.join("keep.log"), b"keep").unwrap();

        delete_analysis_logs_in(
            &root,
            &[root.join("one.log"), nested.join("keep.log")],
        )
        .expect_err("an out-of-root reference must be reported");

        assert!(!root.join("one.log").exists());
        assert!(root.join("unreferenced.log").exists());
        assert!(nested.join("keep.log").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod analysis_history_log_tests {
    use super::library_db;

    #[test]
    fn cancelled_history_round_trips_its_dedicated_log_path() {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-analysis-history-log-{}-{}",
            std::process::id(),
            super::unix_time_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let guard = library_db::reconnect_for_test(&root);
        let log_path = root.join("analysis-logs/run.jsonl");

        library_db::analysis_history_insert(&library_db::NewAnalysisHistory {
            file_hash: "cancelled-song",
            title: "Cancelled",
            artist: "Fixture",
            status: "cancelled",
            started_at_ms: 1,
            finished_at_ms: 2,
            snapshot_json: "{}",
            error_message: Some("cancelled by user"),
            log_path: Some(&log_path),
        })
        .unwrap();

        let rows = library_db::analysis_history_load(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "cancelled");
        assert_eq!(rows[0].log_path.as_deref(), Some(log_path.as_path()));
        std::fs::remove_dir_all(root).unwrap();
        drop(guard);
    }
}

#[cfg(test)]
mod enqueue_tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{QueuedStatus, queue_entry_blocks_enqueue, validate_analysis_source};

    #[test]
    fn analyze_all_retries_failed_entries_but_not_active_work() {
        assert!(!queue_entry_blocks_enqueue(None));
        assert!(!queue_entry_blocks_enqueue(Some(&QueuedStatus::Failed(
            "previous failure".into()
        ))));
        assert!(queue_entry_blocks_enqueue(Some(&QueuedStatus::Queued)));
        assert!(queue_entry_blocks_enqueue(Some(&QueuedStatus::Analyzing(
            42
        ))));
    }

    #[test]
    fn empty_analysis_source_is_rejected_before_server_start() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-empty-analysis-source-{}-{nonce}.flac",
            std::process::id()
        ));
        std::fs::File::create(&path).expect("create empty source fixture");
        let error = validate_analysis_source(&path).expect_err("empty source must be rejected");
        let _ = std::fs::remove_file(&path);
        assert!(error.to_string().contains("source media is empty"));
    }
}
