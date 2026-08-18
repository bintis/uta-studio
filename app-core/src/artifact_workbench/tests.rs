mod tests {
    use super::*;

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "uta-studio-workbench-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn source_revision(cache: &CacheDir, hash: &str, text: &str) -> ArtifactRevision {
        let canonical = cache.path.join(format!("{hash}-lyrics.txt"));
        std::fs::create_dir_all(&cache.path).unwrap();
        std::fs::write(&canonical, text).unwrap();
        let (path, content_hash, byte_size) = ArtifactStore::new(&cache.path)
            .unwrap()
            .capture(hash, ArtifactKind::LyricsInput, &canonical)
            .unwrap();
        ArtifactRevision {
            id: format!("{hash}:lyrics:{content_hash}"),
            file_hash: hash.to_string(),
            kind: ArtifactKind::LyricsInput,
            path,
            content_hash,
            producer_node: AnalysisNodeId::new("lyrics.import"),
            input_revisions: Vec::new(),
            config_hash: "test".to_string(),
            algorithm_version: "test".to_string(),
            created_at_ms: 1,
            byte_size,
            active: true,
            legacy: false,
            invalidated: false,
        }
    }

    fn chart_revision(
        cache: &CacheDir,
        hash: &str,
        kind: ArtifactKind,
        midi: u8,
        lyric: &str,
    ) -> ArtifactRevision {
        let value = serde_json::json!({
            "format": "uta.vocal-chart",
            "format_version": "1.0.0",
            "timebase": 1000,
            "language": "en",
            "tracks": [{
                "id": "lead", "role": "lead", "part": null,
                "singer": "Singer", "scoring_enabled": true,
                "phrases": [{"id": "phrase-1", "notes": [{
                    "id": "note-1", "start": 1000, "duration": 500,
                    "pitch": {"midi": midi, "cents": 0},
                    "vocal_mode": "pitched", "bonus": "normal",
                    "scoring": {"mode": "pitch", "weight": 1.0},
                    "lyrics": [{"id": "lyric-1", "text": lyric,
                        "join_before": "none"}]
                }]}]
            }]
        });
        let canonical = cache.path.join(format!("{hash}-{kind:?}.json"));
        std::fs::create_dir_all(&cache.path).unwrap();
        std::fs::write(&canonical, serde_json::to_vec(&value).unwrap()).unwrap();
        let (path, content_hash, byte_size) = ArtifactStore::new(&cache.path)
            .unwrap()
            .capture(hash, kind, &canonical)
            .unwrap();
        ArtifactRevision {
            id: format!("{hash}:{kind:?}:{content_hash}"),
            file_hash: hash.to_string(),
            kind,
            path,
            content_hash,
            producer_node: AnalysisNodeId::new("test.chart"),
            input_revisions: Vec::new(),
            config_hash: "test".into(),
            algorithm_version: "test".into(),
            created_at_ms: 1,
            byte_size,
            active: true,
            legacy: false,
            invalidated: false,
        }
    }

    #[test]
    fn downstream_impact_uses_real_graph_closure() {
        let impact = preview_node_downstream_impact("lyrics.align").unwrap();
        assert!(
            impact
                .affected_nodes
                .iter()
                .any(|id| id.as_str() == "chart.build_candidate")
        );
        assert!(impact.authored_chart_preserved);
        assert!(
            impact
                .queued_targets
                .iter()
                .any(|id| id.as_str() == "lyrics.align")
        );
    }

    #[test]
    fn frozen_impact_groups_match_the_plan_that_confirmation_would_queue() {
        let impact = preview_frozen_downstream_impact(
            "impact-song",
            ImpactTrigger::RunDownstream,
            Some("lyrics.align"),
        )
        .unwrap();
        let request = analysis_request_from_impact("impact-song", &impact);
        assert!(queued_request_matches_preview(&impact, &request));
        let plan =
            crate::analysis_plan::preview_analysis_plan("impact-song", request.clone()).unwrap();
        let planned_run = plan
            .nodes
            .iter()
            .filter(|node| node.will_run)
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(impact.will_run, planned_run);
        let planned_reuse = plan
            .nodes
            .iter()
            .filter(|node| node.state == NodeState::Frozen)
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(impact.will_reuse, planned_reuse);
        let planned_blocked = plan
            .nodes
            .iter()
            .filter(|node| matches!(node.state, NodeState::Blocked | NodeState::Disabled))
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(impact.will_be_blocked, planned_blocked);
    }

    #[test]
    fn freeze_trigger_puts_stem_kinds_on_the_queued_request() {
        let impact = preview_frozen_downstream_impact(
            "freeze-impact",
            ImpactTrigger::Freeze,
            Some("stems.separate"),
        )
        .unwrap();
        assert!(impact.queued_frozen.contains(&ArtifactKind::VocalStem));
        assert!(
            impact
                .queued_frozen
                .contains(&ArtifactKind::InstrumentalStem)
        );
        let request = analysis_request_from_impact("freeze-impact", &impact);
        assert!(queued_request_matches_preview(&impact, &request));
        assert!(
            impact
                .will_reuse
                .iter()
                .any(|id| id.as_str() == "stems.separate")
        );
    }

    #[test]
    fn candidate_merge_uses_exact_revisions_and_keeps_authored_track_metadata() {
        let root = test_root("chart-merge");
        let _guard = library_db::reconnect_for_test(&root.join("db"));
        let cache = CacheDir { path: root.clone() };
        let hash = "merge-song";
        let candidate = chart_revision(&cache, hash, ArtifactKind::CandidateChart, 64, "new");
        let authored = chart_revision(&cache, hash, ArtifactKind::AuthoredChart, 60, "old");
        record_artifact_revision(&candidate).unwrap();
        record_artifact_revision(&authored).unwrap();

        let merged = merge_chart_revisions(
            &revision_ref(&candidate),
            &revision_ref(&authored),
            ChartRevisionMergeMode::TakeCandidatePitch,
        )
        .unwrap();
        let note = &merged.tracks[0].phrases[0].notes[0];
        assert_eq!(note.pitch.unwrap().midi, 64);
        assert_eq!(merged.tracks[0].singer.as_deref(), Some("Singer"));
        let utz::LyricToken::Text(text) = &note.lyrics[0] else {
            panic!("expected text lyric")
        };
        assert_eq!(text.text, "old");

        drop(_guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn dummy_chart(hash: &str) -> crate::chart::ChartDocument {
        crate::chart::ChartDocument {
            file_hash: hash.to_string(),
            vocal_chart: crate::VocalChartV1 {
                format: "uta.vocal-chart".into(),
                format_version: "1.0.0".into(),
                timebase: 1000,
                language: Some("en".into()),
                tracks: Vec::new(),
            },
            pitch_track: serde_json::json!({"source": "canonical"}),
            audio: crate::chart::ChartAudio {
                instrumental: "/tmp/missing-instrumental.flac".into(),
                vocals: None,
                original: "/tmp/missing-original.flac".into(),
            },
            repaired_issues: Vec::new(),
        }
    }

    fn json_revision(
        cache: &CacheDir,
        hash: &str,
        kind: ArtifactKind,
        value: &serde_json::Value,
        active: bool,
    ) -> ArtifactRevision {
        let canonical = cache.path.join(format!("{hash}-{kind:?}.json"));
        std::fs::create_dir_all(&cache.path).unwrap();
        std::fs::write(&canonical, serde_json::to_vec(value).unwrap()).unwrap();
        let (path, content_hash, byte_size) = ArtifactStore::new(&cache.path)
            .unwrap()
            .capture(hash, kind, &canonical)
            .unwrap();
        ArtifactRevision {
            id: format!("{hash}:{kind:?}:{content_hash}"),
            file_hash: hash.to_string(),
            kind,
            path,
            content_hash,
            producer_node: AnalysisNodeId::new("test.json"),
            input_revisions: Vec::new(),
            config_hash: "test".into(),
            algorithm_version: "test".into(),
            created_at_ms: 1,
            byte_size,
            active,
            legacy: false,
            invalidated: false,
        }
    }

    #[test]
    fn selected_pitch_revision_replaces_canonical_editor_evidence() {
        let root = test_root("pitch-revision");
        let _guard = library_db::reconnect_for_test(&root.join("db"));
        let cache = CacheDir { path: root.clone() };
        let hash = "pitch-song";
        let selected = serde_json::json!({
            "frames": [{"t": 1.25, "midi": 64.0, "confidence": 0.92}],
            "revision": "selected"
        });
        let revision = json_revision(&cache, hash, ArtifactKind::PitchTrack, &selected, true);
        record_artifact_revision(&revision).unwrap();

        let mut chart = dummy_chart(hash);
        apply_artifact_revision_to_chart(&mut chart, &revision_ref(&revision)).unwrap();
        assert_eq!(chart.pitch_track["revision"], "selected");
        assert_eq!(chart.pitch_track["frames"][0]["midi"], 64.0);

        drop(_guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selected_pitch_note_candidates_import_through_active_transcript() {
        let root = test_root("pitch-notes-import");
        let _guard = library_db::reconnect_for_test(&root.join("db"));
        let cache = CacheDir { path: root.clone() };
        let hash = "candidate-notes";
        let transcript = json_revision(
            &cache,
            hash,
            ArtifactKind::TimedTranscript,
            &serde_json::json!({
                "language": "en",
                "segments": [{
                    "text": "hello",
                    "start": 1.0,
                    "end": 1.5,
                    "words": [{"word": "hello", "start": 1.0, "end": 1.5}]
                }]
            }),
            true,
        );
        let notes = json_revision(
            &cache,
            hash,
            ArtifactKind::PitchNoteCandidates,
            &serde_json::json!({
                "format_version": 1,
                "notes": [{"start": 1.0, "end": 1.5, "midi": 67, "confidence": 0.88}]
            }),
            true,
        );
        record_artifact_revision(&transcript).unwrap();
        record_artifact_revision(&notes).unwrap();
        crate::analysis_artifact::set_active_artifact_revision(
            &cache.path,
            hash,
            ArtifactKind::TimedTranscript,
            &transcript.id,
        )
        .unwrap();

        let mut chart = dummy_chart(hash);
        apply_artifact_revision_to_chart(&mut chart, &revision_ref(&notes)).unwrap();
        let note = &chart.vocal_chart.tracks[0].phrases[0].notes[0];
        assert_eq!(note.pitch.unwrap().midi, 67);

        drop(_guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn phrase_and_note_range_merge_use_exact_selection() {
        let root = test_root("chart-merge-selection");
        let _guard = library_db::reconnect_for_test(&root.join("db"));
        let cache = CacheDir { path: root.clone() };
        let hash = "merge-select";
        let candidate_value = serde_json::json!({
            "format": "uta.vocal-chart",
            "format_version": "1.0.0",
            "timebase": 1000,
            "language": "en",
            "tracks": [{
                "id": "lead", "role": "lead", "part": null,
                "singer": "Candidate", "scoring_enabled": true,
                "phrases": [
                    {"id": "phrase-1", "notes": [{
                        "id": "c1", "start": 1000, "duration": 400,
                        "pitch": {"midi": 70, "cents": 0},
                        "vocal_mode": "pitched", "bonus": "normal",
                        "scoring": {"mode": "pitch", "weight": 1.0},
                        "lyrics": [{"id": "c1l", "text": "one", "join_before": "none"}]
                    }]},
                    {"id": "phrase-2", "notes": [{
                        "id": "c2", "start": 2000, "duration": 400,
                        "pitch": {"midi": 72, "cents": 0},
                        "vocal_mode": "pitched", "bonus": "normal",
                        "scoring": {"mode": "pitch", "weight": 1.0},
                        "lyrics": [{"id": "c2l", "text": "two", "join_before": "none"}]
                    }]}
                ]
            }]
        });
        let authored_value = serde_json::json!({
            "format": "uta.vocal-chart",
            "format_version": "1.0.0",
            "timebase": 1000,
            "language": "en",
            "tracks": [{
                "id": "lead", "role": "lead", "part": null,
                "singer": "Authored", "scoring_enabled": true,
                "phrases": [
                    {"id": "phrase-1", "notes": [{
                        "id": "a1", "start": 1000, "duration": 400,
                        "pitch": {"midi": 50, "cents": 0},
                        "vocal_mode": "pitched", "bonus": "normal",
                        "scoring": {"mode": "pitch", "weight": 1.0},
                        "lyrics": [{"id": "a1l", "text": "old-one", "join_before": "none"}]
                    }]},
                    {"id": "phrase-2", "notes": [{
                        "id": "a2", "start": 2000, "duration": 400,
                        "pitch": {"midi": 52, "cents": 0},
                        "vocal_mode": "pitched", "bonus": "normal",
                        "scoring": {"mode": "pitch", "weight": 1.0},
                        "lyrics": [{"id": "a2l", "text": "old-two", "join_before": "none"}]
                    }]}
                ]
            }]
        });
        let candidate = json_revision(
            &cache,
            hash,
            ArtifactKind::CandidateChart,
            &candidate_value,
            true,
        );
        let authored = json_revision(
            &cache,
            hash,
            ArtifactKind::AuthoredChart,
            &authored_value,
            true,
        );
        record_artifact_revision(&candidate).unwrap();
        record_artifact_revision(&authored).unwrap();

        let phrase = merge_chart_revisions(
            &revision_ref(&candidate),
            &revision_ref(&authored),
            ChartRevisionMergeMode::ReplacePhrase {
                track: 0,
                phrase: 1,
            },
        )
        .unwrap();
        assert_eq!(phrase.tracks[0].singer.as_deref(), Some("Authored"));
        assert_eq!(phrase.tracks[0].phrases[0].notes[0].pitch.unwrap().midi, 50);
        assert_eq!(phrase.tracks[0].phrases[1].notes[0].pitch.unwrap().midi, 72);

        let range = merge_chart_revisions(
            &revision_ref(&candidate),
            &revision_ref(&authored),
            ChartRevisionMergeMode::ReplaceNoteRange {
                track: 0,
                start: 900,
                end: 1500,
            },
        )
        .unwrap();
        assert_eq!(range.tracks[0].phrases[0].notes[0].pitch.unwrap().midi, 70);
        assert_eq!(range.tracks[0].phrases[1].notes[0].pitch.unwrap().midi, 52);

        drop(_guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn authored_chart_pin_state_is_queryable() {
        let root = test_root("authored-pin");
        let _guard = library_db::reconnect_for_test(&root.join("db"));
        let cache = CacheDir { path: root.clone() };
        let hash = "pin-song";
        let authored = chart_revision(&cache, hash, ArtifactKind::AuthoredChart, 60, "old");
        record_artifact_revision(&authored).unwrap();
        crate::analysis_artifact::set_active_artifact_revision(
            &cache.path,
            hash,
            ArtifactKind::AuthoredChart,
            &authored.id,
        )
        .unwrap();
        assert!(!authored_chart_is_pinned(hash));
        set_artifact_pinned(&revision_ref(&authored), true).unwrap();
        assert!(authored_chart_is_pinned(hash));

        drop(_guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_node_is_rejected() {
        assert!(preview_node_downstream_impact("does.not.exist").is_err());
    }

    #[test]
    fn media_types_are_typed() {
        assert_eq!(
            media_type(ArtifactKind::VocalStem),
            ArtifactMediaType::Audio
        );
        assert_eq!(
            media_type(ArtifactKind::TimedTranscript),
            ArtifactMediaType::Json
        );
        assert_eq!(
            media_type(ArtifactKind::AuthoredChart),
            ArtifactMediaType::Chart
        );
    }

    #[test]
    fn timed_transcript_validation_preserves_extensions_and_word_timing() {
        let mut value = serde_json::json!({
            "language": "ja",
            "vendor_extension": {"keep": true},
            "segments": [{
                "start": 1.0,
                "end": 2.0,
                "text": "同じ行",
                "words": [
                    {"start": 1.0, "end": 1.4, "word": "同じ", "score": 0.9},
                    {"start": 1.4, "end": 2.0, "word": "行", "custom": "kept"}
                ]
            }, {
                "start": 2.0,
                "end": 3.0,
                "text": "同じ行",
                "words": [{"start": 2.0, "end": 3.0, "word": "同じ行"}]
            }]
        });
        assert_eq!(
            validate_timed_transcript(&value).status,
            ArtifactHealthStatus::Valid
        );
        value["segments"][0]["words"][0]["end"] = serde_json::json!(2.5);
        assert_eq!(
            validate_timed_transcript(&value).status,
            ArtifactHealthStatus::Invalid
        );
        assert_eq!(value["vendor_extension"]["keep"], true);
        assert_eq!(value["segments"][0]["words"][1]["custom"], "kept");
    }

    #[test]
    fn ordered_text_diff_preserves_duplicate_lines() {
        let (added, removed, changes) = ordered_line_diff("chorus\nchorus\nend", "chorus\nend");
        assert_eq!(added, 0);
        assert_eq!(removed, 1);
        assert!(!changes.is_empty());
    }

    #[test]
    fn recursive_json_diff_reports_word_timing_path() {
        let a = serde_json::json!({"segments": [{"words": [{"start": 1.0}]}]});
        let b = serde_json::json!({"segments": [{"words": [{"start": 1.25}]}]});
        let mut changes = Vec::new();
        recursive_json_changes(&a, &b, "", &mut changes);
        assert_eq!(changes, vec!["Changed segments[0].words[0].start"]);
    }

    #[test]
    fn pitch_validators_reject_midi_and_confidence_outside_their_ranges() {
        let notes = serde_json::json!({
            "notes": [{"start": 0.0, "end": 1.0, "midi": 128, "confidence": 1.1}]
        });
        let health = validate_pitch_notes(&notes);
        assert_eq!(health.status, ArtifactHealthStatus::Invalid);
        assert!(
            health
                .messages
                .iter()
                .any(|message| message.contains("MIDI"))
        );
        assert!(
            health
                .messages
                .iter()
                .any(|message| message.contains("confidence"))
        );
    }

    #[test]
    fn pitch_note_diff_names_moved_and_transposed_notes() {
        let a = serde_json::json!({
            "notes": [{"start": 0.0, "end": 1.0, "midi": 60, "confidence": 0.9}]
        });
        let b = serde_json::json!({
            "notes": [{"start": 0.1, "end": 1.1, "midi": 62, "confidence": 0.9}]
        });
        let (summary, details) = pitch_note_semantic_diff(&a, &b);
        assert!(summary.contains("1 moved"));
        assert!(summary.contains("1 transposed"));
        assert_eq!(details.len(), 2);
    }

    #[test]
    fn intermediate_capture_request_persists_mode_and_can_be_disabled() {
        let root = test_root("capture-request");
        std::fs::create_dir_all(&root).unwrap();
        let _guard = crate::library_db::reconnect_for_test(&root);
        let mut request = CaptureIntermediateRequest {
            file_hash: "capture-song".to_string(),
            node_id: AnalysisNodeId::new("lyrics.preprocess"),
            kind: ArtifactKind::PreprocessedAudio,
            enabled: true,
            persistent: false,
        };
        set_intermediate_capture_request(&request).unwrap();
        assert_eq!(
            intermediate_capture_request("capture-song").unwrap(),
            Some(request.clone())
        );

        request.persistent = true;
        set_intermediate_capture_request(&request).unwrap();
        assert!(
            intermediate_capture_request("capture-song")
                .unwrap()
                .unwrap()
                .persistent
        );

        request.enabled = false;
        set_intermediate_capture_request(&request).unwrap();
        assert!(
            intermediate_capture_request("capture-song")
                .unwrap()
                .is_none()
        );
        drop(_guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lyrics_draft_save_creates_an_immutable_user_revision_without_queueing() {
        let root = test_root("lyrics-draft");
        std::fs::create_dir_all(&root).unwrap();
        let _guard = crate::library_db::reconnect_for_test(&root);
        let cache = CacheDir {
            path: root.join("cache"),
        };
        let source = source_revision(&cache, "draft-song", "first line");
        record_artifact_revision(&source).unwrap();
        let source_ref = revision_ref(&source);
        let mut draft = begin_artifact_edit(&source_ref).unwrap();
        draft
            .replace_text("first line\n二行目".to_string())
            .unwrap();
        let committed = commit_artifact_edit(
            &cache,
            &draft,
            ArtifactSaveOptions {
                mode: ArtifactSaveMode::SaveOnly,
                set_active: true,
                fork_from_old_revision: false,
            },
        )
        .unwrap();

        assert_ne!(committed.revision.id, source.id);
        assert_eq!(committed.revision.input_revisions, vec![source.id.clone()]);
        assert_eq!(std::fs::read_to_string(&source.path).unwrap(), "first line");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&committed.revision.path).unwrap()).unwrap();
        assert_eq!(saved["lines"], serde_json::json!(["first line", "二行目"]));
        assert!(!committed.requires_downstream_confirmation);
        assert!(committed.downstream_impact.is_none());
        assert_eq!(
            load_active_artifact("draft-song", ArtifactKind::LyricsInput)
                .unwrap()
                .id,
            committed.revision.id
        );
        drop(_guard);
        std::fs::remove_dir_all(root).unwrap();
    }
}
