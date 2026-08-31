mod tests {
    use super::*;
    use crate::studio::startup::{asset_root, studio_log_filter, studio_window};

    /// Builds an editable document from (start, end, midi, syllable) tuples.
    fn document_fixture(notes: &[(f64, f64, u8, &str)]) -> app_core::EditorDocument {
        let transcript = serde_json::json!({
            "language": "en",
            "segments": [{
                "start": notes.first().map(|note| note.0).unwrap_or(0.0),
                "end": notes.last().map(|note| note.1).unwrap_or(0.0),
                "text": notes.iter().map(|note| note.3).collect::<Vec<_>>().join(" "),
                "words": notes
                    .iter()
                    .map(|(start, end, _, text)| serde_json::json!({
                        "word": text,
                        "start": start,
                        "end": end,
                    }))
                    .collect::<Vec<_>>(),
            }]
        });
        let pitch_notes = serde_json::json!({
            "notes": notes
                .iter()
                .map(|(start, end, midi, _)| serde_json::json!({
                    "start": start,
                    "end": end,
                    "midi": midi,
                    "confidence": 1.0,
                }))
                .collect::<Vec<_>>(),
        });
        app_core::EditorDocument::new(
            app_core::migrate_analyzer_chart(&transcript, &pitch_notes).unwrap(),
        )
    }

    fn chart_fixture(notes: &[(f64, f64, u8, &str)]) -> app_core::ChartDocument {
        app_core::ChartDocument {
            file_hash: "fixture".to_string(),
            vocal_chart: document_fixture(notes).to_chart(),
            pitch_track: serde_json::json!({}),
            audio: app_core::ChartAudio {
                instrumental: "instrumental.flac".to_string(),
                vocals: None,
                original: "original.flac".to_string(),
            },
            repaired_issues: Vec::new(),
        }
    }

    #[test]
    fn native_window_preserves_existing_desktop_geometry() {
        let window = studio_window(&AppConfig::default(), true);
        assert_eq!(window.title, "Uta! Studio");
        assert_eq!(window.width(), 1280.0);
        assert_eq!(window.height(), 720.0);
        assert!(!window.decorations);
        assert!(window.transparent);
        assert_eq!(
            window.composite_alpha_mode,
            CompositeAlphaMode::PreMultiplied
        );
        assert_eq!(window.window_theme, Some(WindowTheme::Dark));
    }

    #[test]
    fn duration_format_matches_the_library_table() {
        assert_eq!(format_duration(0.0), "0:00");
        assert_eq!(format_duration(65.2), "1:05");
        assert_eq!(format_duration(f64::NAN), "0:00");
    }

    #[test]
    fn workspace_eyebrow_is_only_a_fallback_when_there_is_no_subtitle() {
        assert!(should_show_workspace_eyebrow(""));
        assert!(!should_show_workspace_eyebrow("Rena · 0%"));
    }

    #[test]
    fn navigation_repeat_matches_the_restored_controller_cadence() {
        let started = Instant::now();
        let mut state = NavigationInputState::default();
        assert_eq!(
            navigation_repeat(&mut state, Some(NavigationDirection::Next), started),
            Some(NavigationDirection::Next)
        );
        assert_eq!(
            navigation_repeat(
                &mut state,
                Some(NavigationDirection::Next),
                started + NAVIGATION_INITIAL_REPEAT - Duration::from_millis(1),
            ),
            None
        );
        assert_eq!(
            navigation_repeat(
                &mut state,
                Some(NavigationDirection::Next),
                started + NAVIGATION_INITIAL_REPEAT,
            ),
            Some(NavigationDirection::Next)
        );
        assert_eq!(
            navigation_repeat(
                &mut state,
                Some(NavigationDirection::Previous),
                started + NAVIGATION_INITIAL_REPEAT,
            ),
            Some(NavigationDirection::Previous)
        );
        assert_eq!(navigation_repeat(&mut state, None, started), None);
        assert_eq!(state.held_direction, None);
        assert_eq!(state.repeat_at, None);
    }

    #[test]
    fn navigation_skips_invisible_dismiss_backdrops() {
        assert!(!action_is_navigation_target(&UiAction::from(
            AppCommand::CloseActivity
        )));
        assert!(!action_is_navigation_target(&UiAction::from(
            LibraryCommand::DismissSongContext
        )));
        assert!(action_is_navigation_target(&UiAction::from(
            AppCommand::OpenAbout
        )));
    }

    #[test]
    fn button_feedback_preserves_authored_surfaces_and_activity_backdrop() {
        let theme = StudioTheme::new(false);
        let resting = theme.card.with_alpha(0.46);
        assert_eq!(
            button_background(
                &UiAction::from(LibraryCommand::ToggleLibraryLayout),
                Interaction::None,
                resting,
                &theme,
            ),
            resting
        );
        assert_ne!(
            button_background(
                &UiAction::from(LibraryCommand::ToggleLibraryLayout),
                Interaction::Hovered,
                resting,
                &theme,
            ),
            Color::NONE
        );
        assert_eq!(
            button_background(
                &UiAction::from(AppCommand::CloseActivity),
                Interaction::Hovered,
                theme.background.with_alpha(0.54),
                &theme,
            ),
            theme.background.with_alpha(0.54)
        );

        let resting_border = BorderColor::all(theme.border.with_alpha(0.44));
        assert_eq!(
            button_border(
                &UiAction::from(LibraryCommand::ToggleLibraryLayout),
                Interaction::None,
                resting_border,
                &theme,
            ),
            resting_border
        );
        assert_ne!(
            button_border(
                &UiAction::from(LibraryCommand::ToggleLibraryLayout),
                Interaction::Pressed,
                resting_border,
                &theme,
            ),
            resting_border
        );
        assert_eq!(
            button_border(
                &UiAction::from(AppCommand::CloseActivity),
                Interaction::Pressed,
                resting_border,
                &theme,
            ),
            resting_border
        );
    }

    #[test]
    fn editor_audio_failure_does_not_block_chart_authoring() {
        let status = editor::audition::editor_audio_status(Err("missing typefind".to_string()));
        assert!(!status.loaded);
        assert_eq!(status.error.as_deref(), Some("missing typefind"));
    }

    #[test]
    fn setup_request_preserves_the_selected_backend_and_artifact() {
        let mut config = AppConfig {
            compute_backend: Some("openvino".to_string()),
            ..AppConfig::default()
        };
        let folders = setup_folders(
            &config,
            SetupRequest {
                target: Some(app_core::ModelDownloadTarget::Pitch),
            },
        );
        assert_eq!(folders.compute_backend, app_core::ComputeBackend::OpenVino);
        assert_eq!(
            folders.model_target,
            Some(app_core::ModelDownloadTarget::Pitch)
        );

        config.compute_backend = Some("vulkan".to_string());
        let folders = setup_folders(&config, SetupRequest { target: None });
        assert_eq!(folders.compute_backend, app_core::ComputeBackend::Vulkan);
        assert_eq!(folders.model_target, None);
    }

    #[test]
    fn setup_request_treats_unrecognized_backend_strings_as_auto() {
        let config = AppConfig {
            compute_backend: Some("intel".to_string()),
            ..AppConfig::default()
        };
        let folders = setup_folders(&config, SetupRequest { target: None });
        assert_eq!(folders.compute_backend, app_core::ComputeBackend::Auto);
    }

    #[test]
    fn development_asset_root_contains_the_canonical_brand_assets() {
        let root = std::path::PathBuf::from(asset_root());
        assert!(root.join(LOGO_PATH).is_file());
        assert!(root.join(FONT_PATH).is_file());
        assert!(root.join(ICON_ATLAS_PATH).is_file());
        assert!(root.join(MUSIC_PLACEHOLDER_PATH).is_file());
        assert!(
            root.join("desktop/assets/icons/music-placeholder.svg")
                .is_file()
        );
        // Baked in via `include_bytes!`, not loaded from `asset_root()` --
        // a missing file would already be a compile error, but a real PNG
        // signature is worth confirming rather than assuming.
        assert!(LOGO_BYTES.starts_with(b"\x89PNG"));
        assert!(BANNER_BYTES.starts_with(b"\x89PNG"));
        assert!(STARTUP_BANNER_BYTES.starts_with(b"\x89PNG"));
        assert!(root.join("desktop/assets/icons/ui-icons.svg").is_file());
    }

    #[test]
    fn expected_icu_cjk_fallback_does_not_flood_desktop_logs() {
        assert!(studio_log_filter().contains("icu_provider=error"));
    }

    #[test]
    fn export_stem_cannot_escape_the_selected_directory() {
        assert_eq!(safe_file_stem("../A/B: C?.utz"), "_A_B_ C_.utz");
        assert_eq!(safe_file_stem("..."), "Uta! Studio Export");
    }

    #[test]
    fn lyric_drag_snaps_its_closest_edge_to_a_note_boundary() {
        let words = vec![EditorWordOriginal {
            selection: WordSelection {
                segment: 0,
                word: 0,
            },
            start: 1.0,
            end: 1.4,
        }];
        let notes = vec![ChartNoteView {
            index: 0,
            start: 1.3,
            end: 1.8,
            midi: 60.0,
            pitched: true,
            placeholder: false,
            kind: app_core::NoteKind::Normal,
            lyric: None,
            continues_lyric: false,
        }];

        let snap = snap_lyric_move_to_notes(&words, 0.27, &notes, 0.05).unwrap();
        assert!((snap.delta - 0.3).abs() < f64::EPSILON);
        assert!((snap.target - 1.3).abs() < f64::EPSILON);
        assert!(snap_lyric_move_to_notes(&words, 0.2, &notes, 0.05).is_none());
    }

    #[test]
    fn lyric_note_snap_never_moves_a_group_before_zero() {
        let words = vec![EditorWordOriginal {
            selection: WordSelection {
                segment: 0,
                word: 0,
            },
            start: 0.1,
            end: 0.4,
        }];
        let notes = vec![ChartNoteView {
            index: 0,
            start: 0.0,
            end: 0.2,
            midi: 60.0,
            pitched: true,
            placeholder: false,
            kind: app_core::NoteKind::Normal,
            lyric: None,
            continues_lyric: false,
        }];

        let snap = snap_lyric_move_to_notes(&words, -0.05, &notes, 0.2).unwrap();
        assert!(snap.delta >= -0.1);
    }

    #[test]
    fn overlapping_lyrics_use_separate_lanes_and_mark_missing_guidance() {
        let mut document = document_fixture(&[(0.0, 0.7, 60, "one"), (0.8, 1.2, 62, "two")]);
        // A lyric with no pitch target is the format's way of holding an
        // unguided word, and the lane must still show it.
        let unguided = document.insert_lyric(None, 3.0).unwrap();
        document.set_lyric_text(unguided, "three");
        let lyrics = chart_lyrics(&document);
        assert_eq!(lyrics.len(), 3);
        assert!(lyrics[0].guided);
        assert!(lyrics[1].guided);
        assert!(!lyrics[2].guided);
    }

    #[test]
    fn artifact_waveform_read_is_blocked_while_playback_is_running() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a")]),
            uta_studio_audio::EditorAudioStatus {
                playing: true,
                ..default()
            },
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        let audio = uta_studio_audio::EditorAudioPlayer::new();
        let error = set_editor_artifact_waveform(
            &audio,
            &mut editor,
            app_core::ArtifactRef {
                file_hash: "fixture".to_string(),
                kind: app_core::ArtifactKind::AudioStem,
                revision_id: "revision".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(error, "Stop playback before reading an artifact waveform");
        assert_eq!(
            set_editor_waveform_source(&audio, &mut editor, WaveformSource::Original).unwrap_err(),
            "Stop playback before reading a waveform"
        );
    }

    #[test]
    fn unknown_audio_status_conservatively_blocks_waveform_reads() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        let error = confirm_waveform_status(
            &mut editor,
            Err("transport status unavailable".to_string()),
            "Stop playback before reading a waveform",
        )
        .unwrap_err();
        assert_eq!(
            error,
            "Could not confirm playback was stopped: transport status unavailable"
        );
        assert!(editor.audio_status.playing);
    }

    #[test]
    fn history_names_each_edit_and_survives_undo_redo() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a"), (1.0, 2.0, 62, "b")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.checkpoint("Move note");
        editor.document.move_note(0, 3.0, 3.5, 64.0);
        editor.checkpoint("Delete notes");
        editor.document.remove_notes(&BTreeSet::from([1]));
        assert_eq!(editor.history().0, ["Move note", "Delete notes"]);

        assert_eq!(editor.undo(), Some("Delete notes"));
        assert_eq!(editor.document.note_count(), 2);
        assert_eq!(editor.undo(), Some("Move note"));
        assert!((editor.document.notes()[0].start - 0.0).abs() < 1e-9);
        assert_eq!(editor.undo(), None);

        assert_eq!(editor.redo(), Some("Move note"));
        assert!((editor.document.notes()[0].start - 3.0).abs() < 1e-9);
        assert_eq!(editor.redo(), Some("Delete notes"));
        assert_eq!(editor.document.note_count(), 1);
        assert_eq!(editor.redo(), None);
    }

    /// Runs one tap: hold at `down`, release at `up`.
    fn tap(editor: &mut NativeEditor, down: f64, up: f64) {
        editor.visible_position = down;
        let at = editor.visible_position.max(0.0);
        match editor.tap.next_retarget() {
            Some(index) => {
                let note = chart_notes(&editor.document)[index].clone();
                let length = (note.end - note.start).max(app_core::MIN_NOTE_SECONDS);
                editor::commands::move_chart_note(
                    &mut editor.document,
                    index,
                    at,
                    at + length,
                    note.midi,
                );
                editor.tap.holding = Some((index, at));
            }
            None => {
                let index = editor::commands::insert_chart_note(
                    &mut editor.document,
                    at,
                    at + app_core::MIN_NOTE_SECONDS,
                    60.0,
                )
                .unwrap();
                editor.select_only_note(index);
                editor.tap.holding = Some((index, at));
            }
        }
        editor.visible_position = up;
        editor::actions::finish_tap(editor);
    }

    #[test]
    fn taps_retime_the_queued_notes_in_order_then_stop() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a"), (2.0, 3.0, 62, "b")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.tap_mode = true;
        editor.tap.retiming = vec![0, 1];

        tap(&mut editor, 5.0, 5.4);
        assert_eq!(editor.tap.remaining(), 1);
        tap(&mut editor, 6.0, 6.5);
        assert_eq!(editor.tap.remaining(), 0);

        let notes = chart_notes(&editor.document);
        assert!((notes[0].start - 5.0).abs() < 1e-9);
        assert!((notes[0].end - 5.4).abs() < 1e-9);
        assert!((notes[1].start - 6.0).abs() < 1e-9);
        // Re-timing keeps the pitch that was authored.
        assert!((notes[1].midi - 62.0).abs() < 1e-9);
        assert_eq!(notes.len(), 2);
    }

    #[test]
    fn taps_with_nothing_queued_lay_down_new_notes() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.tap_mode = true;
        tap(&mut editor, 2.0, 2.3);
        tap(&mut editor, 3.0, 3.2);
        let notes = chart_notes(&editor.document);
        assert_eq!(notes.len(), 3);
        assert!((notes[1].end - notes[1].start - 0.3).abs() < 1e-3);
        assert!((notes[2].start - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_tap_shorter_than_the_minimum_still_makes_a_valid_note() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.tap_mode = true;
        tap(&mut editor, 4.0, 4.0);
        let note = chart_notes(&editor.document)[1].clone();
        assert!(note.end - note.start >= app_core::MIN_NOTE_SECONDS - 1e-9);
    }

    #[test]
    fn a_ranged_audition_covers_the_selection_and_its_approaches() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[
                (4.0, 5.0, 60, "a"),
                (5.0, 6.0, 62, "b"),
                (9.0, 10.0, 64, "c"),
            ]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.viewport_start = 2.0;
        editor.viewport_duration = 6.0;
        editor.selected_notes = BTreeSet::from([0, 1]);

        let selection =
            editor::actions::audition_range(EditorAction::AuditionSelection, &editor).unwrap();
        assert!((selection.0 - 4.0).abs() < 1e-9);
        assert!((selection.1 - 6.0).abs() < 1e-9);
        // The lead-in stops where the selection starts, and the lead-out picks
        // up where it ends, so a transition is heard from both sides.
        let before =
            editor::actions::audition_range(EditorAction::AuditionBeforeSelection, &editor)
                .unwrap();
        assert!((before.1 - 4.0).abs() < 1e-9);
        assert!(before.0 < before.1);
        let after =
            editor::actions::audition_range(EditorAction::AuditionAfterSelection, &editor).unwrap();
        assert!((after.0 - 6.0).abs() < 1e-9);
        assert!(after.1 > after.0);
        assert_eq!(
            editor::actions::audition_range(EditorAction::AuditionVisible, &editor),
            Some((2.0, 8.0))
        );

        editor.selected_notes.clear();
        assert!(
            editor::actions::audition_range(EditorAction::AuditionSelection, &editor).is_none()
        );
    }

    #[test]
    fn pitch_audition_sounds_only_the_notes_in_range() {
        let editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a"), (4.0, 5.0, 62, "b")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        let tones = editor::actions::pitch_tones(&editor.document, 3.5, 6.0);
        assert_eq!(tones.len(), 1);
        // Tones are positioned against the start of the audition, and clipped
        // to it, so the preview lines up with the transport.
        assert!((tones[0].start_secs - 0.5).abs() < 1e-9);
        assert!((tones[0].duration_secs - 1.0).abs() < 1e-9);
        assert!((tones[0].midi - 62.0).abs() < 1e-9);
    }

    #[test]
    fn ghost_notes_show_the_other_tracks_and_never_the_active_one() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a"), (2.0, 3.0, 62, "b")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        assert!(other_track_notes(&editor.document).is_empty());
        editor.document.add_track(app_core::TrackRole::Lead);
        editor.document.set_active_track(0);
        editor.document.move_notes_to_track(&BTreeSet::from([1]), 1);

        let notes = chart_notes(&editor.document);
        let ghosts = other_track_notes(&editor.document);
        assert_eq!(notes.len(), 1);
        assert_eq!(ghosts.len(), 1);
        assert!((ghosts[0].start - 2.0).abs() < 1e-9);
        // Switching tracks swaps which side is editable.
        editor.document.set_active_track(1);
        assert_eq!(chart_notes(&editor.document).len(), 1);
        assert!((other_track_notes(&editor.document)[0].start - 0.0).abs() < 1e-9);
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack_and_bounds_history() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.checkpoint("Move note");
        assert_eq!(editor.undo(), Some("Move note"));
        editor.checkpoint("Add note");
        assert!(editor.history().1.is_empty());

        for _ in 0..120 {
            editor.checkpoint("Nudge notes");
        }
        assert_eq!(editor.history().0.len(), 100);
    }

    #[test]
    fn editor_viewport_maps_time_and_pitch_independently() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.viewport_start = 10.0;
        editor.viewport_duration = 20.0;
        editor.pitch_min = 40.0;
        editor.pitch_max = 80.0;
        assert_eq!(time_percent(20.0, &editor), 50.0);
        assert_eq!(pitch_percent(60.0, &editor), 58.0);
        assert_eq!(time_percent(5.0, &editor), 0.0);
        assert_eq!(pitch_percent(100.0, &editor), 20.0);
        assert_eq!(surface_pitch_fraction(0.2, false), 0.0);
        assert_eq!(surface_pitch_fraction(0.96, false), 1.0);
        assert_eq!(surface_pitch_fraction(0.0, true), 0.0);
        assert_eq!(surface_pitch_fraction(0.96, true), 1.0);
        set_editor_pitch_span(&mut editor, 999.0);
        assert_eq!(editor.pitch_min, 0.0);
        assert_eq!(editor.pitch_max, 127.0);
    }

    #[test]
    fn quantization_and_safe_repair_keep_valid_note_ranges() {
        let mut document =
            document_fixture(&[(1.023, 1.071, 60, "hello"), (1.2, 1.3, 61, "world")]);
        assert_eq!(
            editor::commands::quantize_chart_notes(&mut document, None, 0.05),
            2
        );
        let notes = chart_notes(&document);
        assert!((notes[0].start - 1.0).abs() < 1e-9);
        assert!((notes[0].end - 1.05).abs() < 1e-9);
        assert!(editor::commands::repair_editor_chart(&mut document));
        let notes = chart_notes(&document);
        assert!(notes[0].end <= notes[1].start);
        assert!(!document.problems().blocks_saving());
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn pitch_contour_is_bounded_and_confidence_weighted() {
        let frames = (0..100)
            .map(|index| ChartPitchFrame {
                time: f64::from(index) * 0.01,
                midi: 60.0 + f64::from(index % 3),
                confidence: if index % 2 == 0 { 1.0 } else { 0.2 },
            })
            .collect::<Vec<_>>();
        let contour = abstract_pitch_contour(&frames, 20);
        assert!(contour.len() <= 20);
        assert!(contour.iter().all(|frame| frame.midi.is_finite()));
        assert!(contour.windows(2).all(|pair| pair[0].time < pair[1].time));
    }

    #[test]
    fn pitch_evidence_converts_only_voiced_finite_frames() {
        let mut chart = chart_fixture(&[(0.0, 1.0, 60, "a")]);
        chart.pitch_track = serde_json::json!({
            "frames": [
                {"time": 1.0, "hz": 440.0, "confidence": 0.9},
                {"time": 1.1, "hz": null, "confidence": 0.1}
            ]
        });
        let frames = chart_pitch_frames(&chart);
        assert_eq!(frames.len(), 1);
        assert!((frames[0].midi - 69.0).abs() < f64::EPSILON);
    }
}
