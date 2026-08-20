use crate::studio::*;

pub(crate) struct ContentActionServices<'a> {
    pub(crate) audio: &'a NativeAudio,
    pub(crate) library_audio: &'a NativeLibraryAudio,
    pub(crate) pitch_audition: &'a NativePitchAudition,
}

pub(crate) struct ContentActionState<'a> {
    pub(crate) state: StudioStateMut<'a>,
    pub(crate) authoring: &'a mut NativeAuthoringJob,
    pub(crate) invalidated: &'a mut UiInvalidated,
}

pub(crate) fn apply_content_action(
    action: &UiAction,
    keys: &ButtonInput<KeyCode>,
    text_inputs: &EditorTextInputs,
    services: ContentActionServices,
    state: ContentActionState,
) {
    let ContentActionServices {
        audio,
        library_audio,
        pitch_audition,
    } = services;
    let ContentActionState {
        state: studio,
        authoring,
        invalidated,
    } = state;
    match &action.0 {
        UiCommand::Library(LibraryCommand::OpenSong(file_hash)) => {
            studio.dialogs.song_context = None;
            studio.library.selected_song = Some(file_hash.clone());
            studio.shell.route = StudioRoute::SongDetail;
            studio.shell.notice = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::AnalyzeSong(file_hash)) => {
            studio.dialogs.song_context = None;
            if app_core::analysis_runtime_status().ready {
                app_core::enqueue_one(file_hash);
                studio.analysis.analysis_tasks = app_core::load_analysis_tasks();
                studio.library.library_view = LibraryView::Queue;
                studio.library.library_facet = None;
                studio.shell.route = StudioRoute::Library;
                studio.analysis.analysis_graph_needs_fit = true;
                studio.library.refresh();
                studio.shell.notice = Some("Song queued for analysis.".to_string());
            } else {
                studio.shell.route = StudioRoute::Settings;
                studio.shell.settings_tab = SettingsTab::Models;
                studio.shell.notice = Some(
                    "Analysis is disabled until the runtime and selected models are installed."
                        .to_string(),
                );
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::OpenEditor(file_hash)) => {
            studio.dialogs.song_context = None;
            studio.library.selected_song = Some(file_hash.clone());
            studio.editor.editor = None;
            studio.shell.route = StudioRoute::Editor;
            if studio.playback.library_playback.status.playing
                && let Ok(status) = library_audio.0.pause()
            {
                studio.playback.library_playback.visible_position = status.position_secs;
                studio.playback.library_playback.status = status;
                studio.playback.library_playback.last_audio_sync = Instant::now();
            }
            studio.shell.notice = Some(start_editor_load_job(
                file_hash,
                Arc::clone(&audio.0),
                &mut studio.jobs.editor_load_job,
            ));
            invalidated.invalidate(UiDirtyRegion::Editor);
        }
        UiCommand::Library(LibraryCommand::ExportUtz(file_hash)) => {
            studio.dialogs.song_context = None;
            let export_directory = studio.shell.config.export_path.clone();
            studio.shell.notice = Some(start_export_job(
                file_hash,
                "utz",
                export_directory,
                &mut studio.jobs.export_job,
            ));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::ExportUltraStar(file_hash)) => {
            studio.dialogs.song_context = None;
            let export_directory = studio.shell.config.export_path.clone();
            studio.shell.notice = Some(start_export_job(
                file_hash,
                "txt",
                export_directory,
                &mut studio.jobs.export_job,
            ));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::OpenSource(path)) => {
            studio.dialogs.song_context = None;
            studio.shell.notice = Some(match validate_source_path(path, &studio.shell.config) {
                Ok(path) => match open::that_detached(&path) {
                    Ok(()) => localized_message(
                        &studio.shell.config,
                        UiMessage::PathOpened,
                        &[("{path}", &path.display().to_string())],
                    ),
                    Err(error) => format!("Could not open {}: {error}", path.display()),
                },
                Err(error) => error,
            });
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::RevealSource(path)) => {
            studio.dialogs.song_context = None;
            studio.shell.notice = Some(reveal_library_entry(path, &studio.shell.config));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::DismissSongContext) => {
            studio.dialogs.song_context = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::OpenLyricsEditor(file_hash)) => {
            let song = app_core::load_song_by_hash(file_hash).ok().flatten();
            let mode = if song.as_ref().is_some_and(|song| {
                matches!(
                    song.transcript_source,
                    Some(app_core::TranscriptSource::Lrc)
                )
            }) {
                LyricsInputMode::TimedLrc
            } else {
                LyricsInputMode::Plain
            };
            studio.dialogs.lyrics_editor = Some(NativeLyricsEditor {
                file_hash: file_hash.clone(),
                mode,
                separate_stems: true,
                initial_text: lyrics_text(file_hash, mode),
                candidates: Vec::new(),
                candidate_index: 0,
                searching: false,
                artifact_draft: None,
                waveform: app_core::ChartWaveform::default(),
            });
            studio.shell.notice = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::CloseLyricsEditor) => {
            studio.dialogs.lyrics_editor = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::ToggleLyricsInputMode) => {
            if let Some(editor) = studio.dialogs.lyrics_editor.as_mut() {
                if let Ok(input) = text_inputs.lyrics.single() {
                    editor.initial_text = input.value().to_string();
                }
                editor.mode = if editor.mode == LyricsInputMode::Plain {
                    LyricsInputMode::TimedLrc
                } else {
                    LyricsInputMode::Plain
                };
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::ToggleLyricsSeparateStems) => {
            if let Some(editor) = studio.dialogs.lyrics_editor.as_mut() {
                if let Ok(input) = text_inputs.lyrics.single() {
                    editor.initial_text = input.value().to_string();
                }
                editor.separate_stems = !editor.separate_stems;
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::SearchLrclibLyrics) => {
            if studio.jobs.lyrics_search_job.receiver.is_none()
                && let Some(editor) = studio.dialogs.lyrics_editor.as_mut()
            {
                if let Ok(input) = text_inputs.lyrics.single() {
                    editor.initial_text = input.value().to_string();
                }
                editor.searching = true;
                editor.candidates.clear();
                editor.candidate_index = 0;
                let file_hash = editor.file_hash.clone();
                let (sender, receiver) = mpsc::channel();
                std::thread::spawn(move || {
                    let candidates = app_core::search_lrclib_for_hash(&file_hash);
                    let _ = sender.send(candidates);
                });
                studio.jobs.lyrics_search_job.receiver = Some(Mutex::new(receiver));
                studio.shell.notice = Some("Searching LRCLIB…".to_string());
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::PreviousLrclibCandidate) => {
            if let Some(editor) = studio.dialogs.lyrics_editor.as_mut() {
                if let Ok(input) = text_inputs.lyrics.single() {
                    editor.initial_text = input.value().to_string();
                }
                editor.candidate_index = editor.candidate_index.saturating_sub(1);
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::NextLrclibCandidate) => {
            if let Some(editor) = studio.dialogs.lyrics_editor.as_mut() {
                if let Ok(input) = text_inputs.lyrics.single() {
                    editor.initial_text = input.value().to_string();
                }
                editor.candidate_index =
                    (editor.candidate_index + 1).min(editor.candidates.len().saturating_sub(1));
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::UseLrclibPlain) => {
            if let Some(editor) = studio.dialogs.lyrics_editor.as_mut()
                && let Some(candidate) = editor.candidates.get(editor.candidate_index)
            {
                editor.initial_text = candidate.lines.join("\n");
                editor.mode = LyricsInputMode::Plain;
                studio.shell.notice = Some("LRCLIB plain lyrics loaded for review.".to_string());
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::UseLrclibTimed) => {
            if let Some(editor) = studio.dialogs.lyrics_editor.as_mut()
                && let Some(candidate) = editor.candidates.get(editor.candidate_index)
                && let Some(lrc) = candidate.synced_lyrics.as_ref()
            {
                editor.initial_text = lrc.clone();
                editor.mode = LyricsInputMode::TimedLrc;
                studio.shell.notice = Some("LRCLIB timed lyrics loaded for review.".to_string());
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::AdjustTranscriptBoundary(target, edge, delta_ms)) => {
            let current = text_inputs
                .lyrics
                .single()
                .map(|input| input.value().to_string())
                .unwrap_or_default();
            if let Some(editor) = studio.dialogs.lyrics_editor.as_mut()
                && let Some(mut draft) = editor.artifact_draft.clone()
            {
                let result = serde_json::from_str::<serde_json::Value>(&current)
                    .map_err(|error| format!("Invalid JSON: {error}"))
                    .and_then(|mut value| {
                        adjust_transcript_boundary_value(
                            &mut value,
                            *target,
                            *edge,
                            f64::from(*delta_ms) / 1000.0,
                            &studio.shell.config,
                        )?;
                        draft.replace_json(value.clone())?;
                        editor.initial_text =
                            serde_json::to_string_pretty(&value).unwrap_or_default();
                        editor.artifact_draft = Some(draft);
                        Ok(())
                    });
                studio.shell.notice = result.err();
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::PreviewTranscriptAt(file_hash, position_ms)) => {
            studio.shell.notice = Some(match app_core::load_chart(file_hash) {
                Ok(chart) => {
                    let path = std::path::Path::new(&chart.audio.instrumental);
                    match audio
                        .0
                        .load_path(path)
                        .and_then(|_| audio.0.seek(*position_ms as f64 / 1000.0))
                        .and_then(|_| audio.0.play())
                    {
                        Ok(_) => localized_message(
                            &studio.shell.config,
                            UiMessage::TranscriptPreviewing,
                            &[("{position}", &format_duration(*position_ms as f64 / 1000.0))],
                        ),
                        Err(error) => localized_message(
                            &studio.shell.config,
                            UiMessage::TranscriptPreviewFailed,
                            &[("{error}", &error)],
                        ),
                    }
                }
                Err(error) => {
                    let error = error.to_string();
                    localized_message(
                        &studio.shell.config,
                        UiMessage::TranscriptLoadFailed,
                        &[("{error}", &error)],
                    )
                }
            });
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::SaveLyricsEditor)
        | UiCommand::Editor(EditorCommand::SaveLyricsEditorAndRunDownstream) => {
            let run_downstream = matches!(
                &action.0,
                &UiCommand::Editor(EditorCommand::SaveLyricsEditorAndRunDownstream)
            );
            let value = text_inputs
                .lyrics
                .single()
                .map(|input| input.value().to_string())
                .unwrap_or_default();
            if let Some(editor) = studio.dialogs.lyrics_editor.as_mut() {
                editor.initial_text = value.clone();
                if let Some(mut draft) = editor.artifact_draft.clone() {
                    let update = match draft.draft_kind {
                        app_core::ArtifactDraftKind::Lyrics => draft.replace_text(value),
                        app_core::ArtifactDraftKind::TimedTranscript
                        | app_core::ArtifactDraftKind::StructuredJson => {
                            serde_json::from_str(&value)
                                .map_err(|error| format!("Invalid JSON: {error}"))
                                .and_then(|value| draft.replace_json(value))
                        }
                    };
                    let result = update.and_then(|()| {
                        app_core::commit_artifact_edit(
                            &app_core::CacheDir::new(),
                            &draft,
                            app_core::ArtifactSaveOptions {
                                mode: if run_downstream {
                                    app_core::ArtifactSaveMode::SaveAndRunDownstream
                                } else {
                                    app_core::ArtifactSaveMode::SaveOnly
                                },
                                set_active: true,
                                fork_from_old_revision: false,
                            },
                        )
                    });
                    match result {
                        Ok(commit) => {
                            let queued = if run_downstream {
                                commit.downstream_impact.as_ref().map_or(Ok(()), |impact| {
                                        let request = app_core::analysis_request_from_impact(
                                            &commit.revision.file_hash,
                                            impact,
                                        );
                                        if !app_core::queued_request_matches_preview(impact, &request)
                                        {
                                            return Err(
                                                "impact preview no longer matches the current analysis request"
                                                    .to_string(),
                                            );
                                        }
                                        app_core::run_analysis_request(request)
                                    })
                            } else {
                                Ok(())
                            };
                            match queued {
                                Ok(()) => {
                                    studio.dialogs.lyrics_editor = None;
                                    studio.library.refresh();
                                    studio.shell.notice = Some(if run_downstream {
                                            "Artifact revision saved; confirmed downstream work was queued."
                                        } else {
                                            "Artifact revision saved without queueing analysis."
                                        }.to_string());
                                }
                                Err(error) => {
                                    studio.shell.notice = Some(format!(
                                        "Revision was saved, but downstream work could not be queued: {error}"
                                    ));
                                }
                            }
                        }
                        Err(error) => {
                            editor.artifact_draft = Some(draft);
                            studio.shell.notice = Some(format!("Could not save artifact: {error}"));
                        }
                    }
                    invalidated.invalidate(action.0.dirty_region());
                    return;
                }
                let song = app_core::load_song_by_hash(&editor.file_hash)
                    .ok()
                    .flatten();
                let requires_runtime = editor.mode == LyricsInputMode::Plain
                    || (editor.mode == LyricsInputMode::TimedLrc
                        && editor.separate_stems
                        && song.as_ref().is_some_and(|song| !song.is_analyzed));
                let result = if requires_runtime && !app_core::analysis_runtime_status().ready {
                    Err("Analysis runtime is unavailable. Choose authoring on the original mix for timed LRC, or finish setup in Settings > Models & runtime.".to_string())
                } else if editor.mode == LyricsInputMode::TimedLrc {
                    if song.as_ref().is_some_and(|song| song.is_analyzed) {
                        app_core::apply_timed_lyrics(&editor.file_hash, &value)
                    } else {
                        app_core::provide_lrc(&editor.file_hash, &value, editor.separate_stems)
                    }
                } else {
                    app_core::save_lyrics_and_realign(
                        &editor.file_hash,
                        value.lines().map(str::to_string).collect(),
                    )
                };
                match result {
                    Ok(()) => {
                        studio.dialogs.lyrics_editor = None;
                        studio.library.refresh();
                        studio.shell.notice = Some(
                            if requires_runtime {
                                "Lyrics saved and alignment queued."
                            } else {
                                "Timed lyrics saved."
                            }
                            .to_string(),
                        );
                    }
                    Err(error) => {
                        studio.shell.notice = Some(format!("Could not save lyrics: {error}"))
                    }
                }
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::OpenLanguageEditor(file_hash)) => {
            let config = AppConfig::load();
            let initial_language = config
                .language_override(file_hash)
                .map(canonical_analysis_language)
                .unwrap_or_else(|| "auto".into());
            studio.dialogs.language_editor = Some(NativeLanguageEditor {
                file_hash: file_hash.clone(),
                initial_language,
                force_transcribe: false,
                picker_open: false,
            });
            studio.shell.notice = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::CloseLanguageEditor) => {
            studio.dialogs.language_editor = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::ToggleLanguageReprocess) => {
            if let Some(editor) = studio.dialogs.language_editor.as_mut() {
                editor.force_transcribe = !editor.force_transcribe;
                editor.picker_open = false;
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::ToggleLanguagePicker) => {
            if let Some(editor) = studio.dialogs.language_editor.as_mut() {
                editor.picker_open = !editor.picker_open;
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::SelectAnalysisLanguage(language)) => {
            if let Some(editor) = studio.dialogs.language_editor.as_mut() {
                editor.initial_language = canonical_analysis_language(language);
                editor.picker_open = false;
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::SaveLanguageEditor) => {
            if let Some(editor) = studio.dialogs.language_editor.as_mut() {
                let language = editor.initial_language.clone();
                if !app_core::analysis_runtime_status().ready {
                    studio.shell.notice = Some(
                            "Analysis is disabled until setup is completed in Settings > Models & runtime."
                                .to_string(),
                        );
                } else {
                    if language == "auto" {
                        let mut config = AppConfig::load();
                        config.clear_language_override(&editor.file_hash);
                        if let Err(error) = config.save() {
                            studio.shell.notice =
                                Some(format!("Could not save the language setting: {error}"));
                            invalidated.invalidate(action.0.dirty_region());
                            return;
                        }
                        if editor.force_transcribe {
                            app_core::reanalyze_force_transcribe(&editor.file_hash);
                        } else {
                            app_core::realign(&editor.file_hash, None);
                        }
                    } else if editor.force_transcribe {
                        let mut config = AppConfig::load();
                        config.set_language_override(editor.file_hash.clone(), language.clone());
                        if let Err(error) = config.save() {
                            studio.shell.notice =
                                Some(format!("Could not save the language setting: {error}"));
                            invalidated.invalidate(action.0.dirty_region());
                            return;
                        }
                        app_core::reanalyze_force_transcribe(&editor.file_hash);
                    } else {
                        app_core::realign(&editor.file_hash, Some(language.clone()));
                    }
                    studio.dialogs.language_editor = None;
                    studio.shell.config = AppConfig::load();
                    studio.shell.notice = Some(if language == "auto" {
                        "Automatic language detection enabled; reprocessing queued.".into()
                    } else {
                        localized_message(
                            &studio.shell.config,
                            UiMessage::LanguageReprocessQueued,
                            &[("{language}", &language)],
                        )
                    });
                }
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::SaveNodeConfigAsSongProfile(file_hash, node_id)) => {
            // Same immediacy as Freeze/Disable/Bypass above -- fires
            // right away and leaves the context menu open, rather than
            // dismissing it, matching that established pattern.
            studio.shell.notice = Some(
                match app_core::save_node_config_as_song_profile(file_hash, node_id) {
                    Ok(()) => {
                        format!("Saved {node_id}'s current configuration as this song's profile.")
                    }
                    Err(error) => format!("Could not save song profile: {error}"),
                },
            );
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::OpenNodeConfigDialog(file_hash, node_id)) => {
            if let Some(field) = node_config_profile_field(node_id) {
                let global = app_core::AnalysisProfileSnapshot::from_app_config(
                    &AppConfig::load(),
                    file_hash,
                );
                let song = app_core::get_song_analysis_profile(file_hash);
                let run_override = app_core::pending_run_override_for(file_hash, node_id);
                let value = app_core::resolve_profile_field(
                    field,
                    &global,
                    song.as_ref(),
                    run_override.as_deref(),
                )
                .value;
                studio.dialogs.node_config_dialog = Some(NativeNodeConfigDialog {
                    file_hash: file_hash.clone(),
                    node_id: node_id.clone(),
                    field,
                    value,
                    picker_open: false,
                });
            } else {
                studio.shell.notice = Some(format!(
                    "{node_id} has no parameter to configure for a single run."
                ));
            }
            studio.dialogs.analysis_node_context = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CloseNodeConfigDialog) => {
            studio.dialogs.node_config_dialog = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ToggleNodeConfigPicker) => {
            if let Some(dialog) = studio.dialogs.node_config_dialog.as_mut() {
                dialog.picker_open = !dialog.picker_open;
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::SelectNodeConfigValue(value)) => {
            if let Some(dialog) = studio.dialogs.node_config_dialog.as_mut() {
                dialog.value = value.clone();
                dialog.picker_open = false;
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::RunNodeConfigDialog) => {
            if let Some(dialog) = studio.dialogs.node_config_dialog.take() {
                studio.shell.notice = Some(
                    match app_core::configure_analysis_node_for_run(
                        &dialog.file_hash,
                        &dialog.node_id,
                        dialog.value.clone(),
                    ) {
                        Ok(()) => format!(
                            "Running {} with {} for this run only.",
                            dialog.node_id, dialog.value
                        ),
                        Err(error) => format!("Could not configure this run: {error}"),
                    },
                );
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::OpenPlanPreview(file_hash)) => {
            studio.dialogs.plan_preview_draft = Some(PlanPreviewDraft {
                file_hash: file_hash.clone(),
                disabled_nodes: std::collections::BTreeSet::new(),
            });
            studio.shell.notice = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ClosePlanPreview) => {
            studio.dialogs.plan_preview_draft = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::TogglePlanPreviewDisabledNode(node_id)) => {
            if let Some(draft) = studio.dialogs.plan_preview_draft.as_mut() {
                let id = app_core::AnalysisNodeId::new(node_id.clone());
                if !draft.disabled_nodes.remove(&id) {
                    draft.disabled_nodes.insert(id);
                }
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::RunPlanPreviewDraft) => {
            if let Some(draft) = studio.dialogs.plan_preview_draft.take() {
                studio.shell.notice = Some(run_analysis_action_checked(&draft.file_hash, || {
                    app_core::run_analysis_plan(
                        &draft.file_hash,
                        std::collections::BTreeSet::new(),
                        draft.disabled_nodes.clone(),
                    )
                }));
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::OpenAppLogViewer(file_hash, node_id)) => {
            studio.dialogs.app_log_viewer = Some(AppLogViewerState {
                file_hash: file_hash.clone(),
                node_id: node_id.clone(),
            });
            studio.dialogs.analysis_node_context = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CloseAppLogViewer) => {
            studio.dialogs.app_log_viewer = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::OpenAppLogFile) => {
            studio.shell.notice = Some(match app_core::get_log_path() {
                Some(path) => match open::that_detached(&path) {
                    Ok(()) => localized_message(
                        &studio.shell.config,
                        UiMessage::PathOpened,
                        &[("{path}", &path.display().to_string())],
                    ),
                    Err(error) => format!("Could not open {}: {error}", path.display()),
                },
                None => "The app log is not available in this environment.".to_string(),
            });
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::OpenSongSettings(file_hash)) => {
            studio.dialogs.song_settings = open_song_settings(file_hash);
            if studio.dialogs.song_settings.is_none() {
                studio.shell.notice = Some("Could not load this song's settings.".to_string());
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::CloseSongSettings) => {
            studio.dialogs.song_settings = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::ChooseBackgroundVideo) => {
            if studio.dialogs.song_settings.is_some()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Video", &["mp4", "webm", "mkv", "mov", "avi"])
                    .pick_file()
                && let Some(panel) = studio.dialogs.song_settings.as_mut()
            {
                panel.background_video_path = Some(path);
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::ClearBackgroundVideo) => {
            if let Some(panel) = studio.dialogs.song_settings.as_mut() {
                panel.background_video_path = None;
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Editor(EditorCommand::SaveSongSettings) => {
            if let Some(panel) = studio.dialogs.song_settings.take() {
                let composer = text_inputs
                    .song_settings_composer
                    .single()
                    .map(|input| input.value().to_string().trim().to_string())
                    .unwrap_or_default();
                let country = text_inputs
                    .song_settings_country
                    .single()
                    .map(|input| input.value().to_string().trim().to_string())
                    .unwrap_or_default();
                let bpm_text = text_inputs
                    .song_settings_bpm
                    .single()
                    .map(|input| input.value().to_string().trim().to_string())
                    .unwrap_or_default();
                let bpm = if bpm_text.is_empty() {
                    None
                } else {
                    match bpm_text.parse::<f64>() {
                        Ok(value) if value.is_finite() && value > 0.0 => Some(value),
                        _ => {
                            studio.shell.notice =
                                Some("BPM must be a positive number, or left blank.".to_string());
                            studio.dialogs.song_settings = Some(panel);
                            invalidated.invalidate(action.0.dirty_region());
                            return;
                        }
                    }
                };
                match app_core::update_song_settings(
                    &panel.file_hash,
                    (!composer.is_empty()).then_some(composer),
                    (!country.is_empty()).then_some(country),
                    bpm,
                    panel.background_video_path.clone(),
                ) {
                    Ok(()) => {
                        studio.shell.notice = Some("Song settings saved.".to_string());
                        studio.library.refresh();
                    }
                    Err(error) => {
                        studio.shell.notice =
                            Some(format!("Could not save song settings: {error}"));
                    }
                }
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::RealignSong(file_hash)) => {
            studio.shell.notice = Some(run_analysis_action(file_hash, || {
                app_core::realign(file_hash, None)
            }));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ReanalyzeTranscript(file_hash)) => {
            studio.shell.notice = Some(run_analysis_action(file_hash, || {
                app_core::reanalyze_transcript(file_hash, None)
            }));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ForceTranscribe(file_hash)) => {
            studio.shell.notice = Some(run_analysis_action(file_hash, || {
                app_core::reanalyze_force_transcribe(file_hash)
            }));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ReanalyzePitch(file_hash)) => {
            studio.shell.notice = Some(run_analysis_action(file_hash, || {
                app_core::reanalyze_pitch(file_hash)
            }));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ReanalyzeFull(file_hash)) => {
            studio.shell.notice = Some(run_analysis_action(file_hash, || {
                app_core::reanalyze_full(file_hash)
            }));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::RunAnalysisNodeOnly(file_hash, node_id)) => {
            studio.shell.notice = Some(run_analysis_action_checked(file_hash, || {
                app_core::run_analysis_node(file_hash, node_id)
            }));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::RunAnalysisNodeDownstream(file_hash, node_id)) => {
            studio.shell.notice = Some(run_analysis_action_checked(file_hash, || {
                app_core::run_analysis_node_downstream(file_hash, node_id)
            }));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::DisableAnalysisNodeForRun(file_hash, node_id)) => {
            studio.shell.notice = Some(run_analysis_action_checked(file_hash, || {
                app_core::disable_analysis_node_for_run(file_hash, node_id)
            }));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::FreezeAnalysisNodeOutputs(file_hash, node_id)) => {
            studio.shell.notice = Some(run_analysis_action_checked(file_hash, || {
                app_core::freeze_analysis_node_outputs_for_run(file_hash, node_id)
            }));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::BypassAnalysisNodeWithOriginalMix(
            file_hash,
            node_id,
        )) => {
            studio.shell.notice = Some(run_analysis_action_checked(file_hash, || {
                app_core::bypass_analysis_node_with_original_mix_for_run(file_hash, node_id)
            }));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CompareNodeAttemptWithPrevious(
            file_hash,
            node_id,
            current_run_id,
        )) => {
            studio.shell.notice = Some(
                match app_core::compare_node_attempt_with_previous_run(
                    file_hash,
                    node_id,
                    *current_run_id,
                ) {
                    Ok(comparison) => format_node_attempt_comparison(&comparison),
                    Err(error) => format!("Could not compare attempts: {error}"),
                },
            );
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ToggleAnalysisCompoundNode(node_id)) => {
            let id = app_core::AnalysisNodeId::new(node_id.clone());
            if !studio.analysis.expanded_compound_nodes.remove(&id) {
                studio.analysis.expanded_compound_nodes.insert(id);
            }
            studio.dialogs.analysis_node_context = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::RequestDeleteSongCache(file_hash)) => {
            studio.dialogs.pending_cache_delete = Some(file_hash.clone());
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CancelDeleteSongCache) => {
            studio.dialogs.pending_cache_delete = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CancelAnalysisRun(file_hash)) => {
            studio.shell.notice = Some(match app_core::cancel_analysis_run(file_hash) {
                Ok(()) => "Removed from the analysis queue.".to_string(),
                Err(error) => format!("Could not cancel: {error}"),
            });
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ConfirmDeleteSongCache) => {
            if let Some(file_hash) = studio.dialogs.pending_cache_delete.take() {
                app_core::delete_cache(&file_hash);
                studio.library.refresh();
                studio.shell.notice =
                    Some("Generated song data deleted. Source media was not changed.".to_string());
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::RequestReplaceAuthoredChart(file_hash)) => {
            studio.dialogs.analysis_artifact_context = None;
            studio.dialogs.pending_chart_replace = Some(file_hash.clone());
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CancelReplaceAuthoredChart) => {
            studio.dialogs.pending_chart_replace = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ConfirmReplaceAuthoredChart) => {
            if let Some(file_hash) = studio.dialogs.pending_chart_replace.take() {
                studio.shell.notice = Some(
                        match app_core::replace_authored_chart_with_fresh_analysis(&file_hash) {
                            Ok(()) => {
                                "Authored chart discarded. It will be rebuilt from the latest analysis the next time you open the editor.".to_string()
                            }
                            Err(error) => format!("Could not replace the chart: {error}"),
                        },
                    );
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::SyncArtifactRevisions(file_hash)) => {
            let imported = app_core::import_legacy_artifacts(&app_core::CacheDir::new(), file_hash);
            studio.shell.notice = Some(if imported.is_empty() {
                "No new artifact revisions found on disk.".to_string()
            } else {
                localized_message(
                    &studio.shell.config,
                    UiMessage::ArtifactRevisionsRecorded,
                    &[("{count}", &imported.len().to_string())],
                )
            });
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::SetActiveArtifactRevision(revision)) => {
            studio.dialogs.pending_artifact_active = Some(revision.clone());
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CancelSetActiveArtifactRevision) => {
            studio.dialogs.pending_artifact_active = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ConfirmSetActiveArtifactRevision) => {
            let Some(revision) = studio.dialogs.pending_artifact_active.take() else {
                return;
            };
            let cache_root = app_core::CacheDir::new().path;
            studio.shell.notice = Some(
                match app_core::set_active_artifact_revision(
                    &cache_root,
                    &revision.file_hash,
                    revision.kind,
                    &revision.id,
                ) {
                    Ok(()) => "Active artifact revision updated.".to_string(),
                    Err(error) => format!("Could not set active artifact revision: {error}"),
                },
            );
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::RequestCaptureIntermediate(file_hash)) => {
            studio.dialogs.pending_intermediate_capture = Some(file_hash.clone());
            studio.dialogs.analysis_node_context = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CancelCaptureIntermediate) => {
            studio.dialogs.pending_intermediate_capture = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ConfirmCaptureIntermediateOnce)
        | UiCommand::Analysis(AnalysisCommand::ConfirmCaptureIntermediatePersistent)
        | UiCommand::Analysis(AnalysisCommand::ConfirmDisableIntermediateCapture) => {
            let Some(file_hash) = studio.dialogs.pending_intermediate_capture.take() else {
                return;
            };
            let enabled = !matches!(
                &action.0,
                UiCommand::Analysis(AnalysisCommand::ConfirmDisableIntermediateCapture)
            );
            let persistent = matches!(
                &action.0,
                UiCommand::Analysis(AnalysisCommand::ConfirmCaptureIntermediatePersistent)
            );
            let request = app_core::CaptureIntermediateRequest {
                file_hash,
                node_id: app_core::AnalysisNodeId::new("lyrics.preprocess"),
                kind: app_core::ArtifactKind::PreprocessedAudio,
                enabled,
                persistent,
            };
            studio.shell.notice = Some(match app_core::set_intermediate_capture_request(&request) {
                Ok(()) if !enabled => "Preprocessed-audio capture disabled.".to_string(),
                Ok(()) if persistent => {
                    "Preprocessed audio will be retained on future analysis runs until disabled."
                        .to_string()
                }
                Ok(()) => {
                    "Preprocessed audio will be retained once on the next successful analysis run."
                        .to_string()
                }
                Err(error) => format!("Could not update intermediate capture: {error}"),
            });
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::OpenArtifactRevision(path)) => {
            studio.shell.notice = Some(open_artifact_entry(path, &studio.shell.config));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::PreviewArtifactRevision(path)) => {
            studio.shell.notice = Some(preview_artifact_entry(path));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::RevealArtifactRevision(path)) => {
            studio.shell.notice = Some(reveal_artifact_entry(path));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::RequestDeleteArtifactRevision(revision)) => {
            studio.dialogs.pending_artifact_delete = Some(revision.clone());
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CancelDeleteArtifactRevision) => {
            studio.dialogs.pending_artifact_delete = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ConfirmDeleteArtifactRevision) => {
            if let Some(revision) = studio.dialogs.pending_artifact_delete.take() {
                let cache_root = app_core::CacheDir::new().path;
                studio.shell.notice = Some(
                    match app_core::delete_artifact_revision(&cache_root, &revision) {
                        Ok(()) => "Artifact revision deleted.".to_string(),
                        Err(error) => format!("Could not delete artifact revision: {error}"),
                    },
                );
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::RequestInvalidateArtifactRevision(revision)) => {
            studio.dialogs.pending_artifact_invalidate = Some(revision.clone());
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CancelInvalidateArtifactRevision) => {
            studio.dialogs.pending_artifact_invalidate = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ConfirmInvalidateArtifactRevision) => {
            if let Some(revision) = studio.dialogs.pending_artifact_invalidate.take() {
                let cache_root = app_core::CacheDir::new().path;
                studio.shell.notice = Some(
                        match app_core::invalidate_artifact_revision(
                            &cache_root,
                            &revision.file_hash,
                            revision.kind,
                            &revision.id,
                        ) {
                            Ok(()) => {
                                "Artifact revision invalidated. It's no longer Active but the file is kept.".to_string()
                            }
                            Err(error) => format!("Could not invalidate artifact revision: {error}"),
                        },
                    );
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::InspectArtifactProvenance(revision)) => {
            studio.shell.notice = Some(format_artifact_provenance(revision));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CompareArtifactRevisions(revision, other)) => {
            let left = artifact_ref_from_revision(revision);
            match app_core::compare_artifacts_typed(&left, other) {
                Ok(diff) => {
                    studio.dialogs.artifact_diff = Some(diff);
                    studio.shell.notice = None;
                }
                Err(error) => {
                    studio.shell.notice = Some(format!("Could not compare revisions: {error}"));
                }
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CloseArtifactDiff) => {
            studio.dialogs.artifact_diff = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::ShiftSongKey(file_hash, delta)) => {
            studio.shell.notice = Some(start_key_shift(
                file_hash,
                *delta,
                authoring,
                &mut studio.jobs.authoring_busy,
            ));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::ShiftSongTempo(file_hash, delta)) => {
            studio.shell.notice = Some(start_tempo_shift(
                file_hash,
                *delta,
                authoring,
                &mut studio.jobs.authoring_busy,
            ));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::PlayLibrarySong(file_hash)) => {
            let queue = studio.library.songs.processed.clone();
            prepare_library_queue(&queue, file_hash, &mut studio.playback.library_playback);
            studio.shell.notice = play_library_song(
                &library_audio.0,
                file_hash,
                &mut studio.playback.library_playback,
            )
            .err();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::PlayArtifactRevision(path)) => {
            studio.shell.notice = play_artifact_revision(
                &library_audio.0,
                path,
                &mut studio.playback.library_playback,
            )
            .err();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::ToggleLibraryPlayback) => {
            studio.shell.notice =
                toggle_library_playback(&library_audio.0, &mut studio.playback.library_playback)
                    .err();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::SeekLibraryRelative(delta)) => {
            studio.shell.notice = seek_library_relative(
                &library_audio.0,
                &mut studio.playback.library_playback,
                f64::from(*delta),
            )
            .err();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::PreviousLibrarySong) => {
            studio.shell.notice = if library_visible_position(&studio.playback.library_playback)
                > 5.0
            {
                restart_library_song(&library_audio.0, &mut studio.playback.library_playback).err()
            } else {
                let wrap = studio.playback.library_playback.repeat == LibraryRepeatMode::All;
                advance_library_queue(
                    &library_audio.0,
                    &mut studio.playback.library_playback,
                    -1,
                    wrap,
                )
                .err()
            };
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::NextLibrarySong) => {
            let wrap = studio.playback.library_playback.repeat == LibraryRepeatMode::All;
            studio.shell.notice = advance_library_queue(
                &library_audio.0,
                &mut studio.playback.library_playback,
                1,
                wrap,
            )
            .err();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::ToggleLibraryShuffle) => {
            studio.playback.library_playback.shuffle = !studio.playback.library_playback.shuffle;
            studio.shell.notice = Some(if studio.playback.library_playback.shuffle {
                "Shuffle enabled for the playback queue.".to_string()
            } else {
                "Shuffle disabled.".to_string()
            });
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::CycleLibraryRepeat) => {
            studio.playback.library_playback.repeat =
                studio.playback.library_playback.repeat.next();
            studio.shell.notice = Some(studio.playback.library_playback.repeat.label().to_string());
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::AdjustLibraryVolume(delta)) => {
            let volume = studio.playback.library_playback.volume + f64::from(*delta) / 100.0;
            studio.shell.notice = set_library_volume(
                &library_audio.0,
                &mut studio.playback.library_playback,
                volume,
            )
            .err();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::ToggleLibraryMute) => {
            let volume = if studio.playback.library_playback.volume > 0.0 {
                studio.playback.library_playback.volume_before_mute =
                    studio.playback.library_playback.volume;
                0.0
            } else {
                studio.playback.library_playback.volume_before_mute.max(0.1)
            };
            studio.shell.notice = set_library_volume(
                &library_audio.0,
                &mut studio.playback.library_playback,
                volume,
            )
            .err();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::ToggleLibraryQueue) => {
            studio.playback.library_playback.queue_open =
                !studio.playback.library_playback.queue_open;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::DismissLyricContext) => {
            if let Some(editor) = studio.editor.editor.as_mut() {
                editor.lyric_context = None;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::DismissNoteContext) => {
            if let Some(editor) = studio.editor.editor.as_mut() {
                editor.note_context = None;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::SelectWaveformSource(source)) => {
            let mut notice = None;
            if let Some(editor) = studio.editor.editor.as_mut() {
                if *source == WaveformSource::Vocals && editor.chart.audio.vocals.is_none() {
                    notice = Some("This chart has no separate vocal source.".to_string());
                } else {
                    set_editor_waveform_source(editor, *source);
                }
                editor.waveform_context = None;
            }
            if notice.is_some() {
                studio.shell.notice = notice;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::SelectWaveformStyle(style)) => {
            if let Some(editor) = studio.editor.editor.as_mut() {
                editor.waveform_style = *style;
                editor.waveform_context = None;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::DismissWaveformContext) => {
            if let Some(editor) = studio.editor.editor.as_mut() {
                editor.waveform_context = None;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::DismissProblemsPanel) => {
            if let Some(editor) = studio.editor.editor.as_mut() {
                editor.problems_panel_open = false;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::DismissShortcutsPanel) => {
            if let Some(editor) = studio.editor.editor.as_mut() {
                editor.shortcuts_panel_open = false;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::SetProblemsFilter(filter)) => {
            if let Some(editor) = studio.editor.editor.as_mut() {
                editor.problems_filter = *filter;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::ApplyAllLyricsEdit) => {
            let mut notice = None;
            if let Some(editor) = studio.editor.editor.as_mut()
                && let Ok(input) = text_inputs.all_lyrics.single()
            {
                let text = input.value().to_string();
                let lines = text.split('\n').collect::<Vec<_>>();
                let phrase_count = editor.document.phrase_count();
                if lines.len() != phrase_count {
                    notice = Some(format!(
                        "This chart has {phrase_count} phrase(s) but the text has {} line(s) — keep exactly one line per phrase (blank lines are fine), then apply again.",
                        lines.len()
                    ));
                } else {
                    editor.checkpoint("Retype all lyrics");
                    let mut changed = false;
                    for (index, line) in lines.iter().enumerate() {
                        changed |= editor.document.set_phrase_token_text(index, line);
                    }
                    if changed {
                        editor.dirty = true;
                        notice = Some("Updated all lyrics.".to_string());
                    } else {
                        editor.undo.pop();
                    }
                    editor.all_lyrics_editor_open = false;
                }
            }
            if notice.is_some() {
                studio.shell.notice = notice;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::ExtendLyricOverNote(word, note_index)) => {
            if let Some(editor) = studio.editor.editor.as_mut() {
                editor.checkpoint("Continue syllable");
                if extend_editor_lyric(&mut editor.document, *word, *note_index) {
                    editor.dirty = true;
                    editor.note_context = None;
                    studio.shell.notice = Some("Extended the syllable onto this note.".to_string());
                } else {
                    editor.undo.pop();
                    studio.shell.notice = Some(
                            "That note can't continue the syllable — it needs to sit right after it, in the same phrase, with no lyric of its own.".to_string(),
                        );
                }
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Editor(EditorCommand::Editor(_))
        | UiCommand::Editor(EditorCommand::FocusChartProblem(..))
        | UiCommand::Editor(EditorCommand::OpenEditorSelect(_))
        | UiCommand::Editor(EditorCommand::SelectEditorValue(..))
        | UiCommand::Editor(EditorCommand::SelectEditorWord(..))
        | UiCommand::Editor(EditorCommand::SelectEditorTrack(_))
        | UiCommand::Editor(EditorCommand::MoveSelectionToTrack(_))
        | UiCommand::Editor(EditorCommand::SetNoteKind(_)) => {
            handle_editor_ui_action(
                action,
                keys,
                &mut EditorActionContext {
                    audio: &audio.0,
                    tones: &pitch_audition.0,
                    shell: studio.shell,
                    editor: studio.editor,
                    dialogs: studio.dialogs,
                    invalidated,
                },
            );
        }

        _ => {}
    }
}
