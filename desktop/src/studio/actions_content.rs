use crate::studio::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_content_action(
    action: &UiAction,
    commands: &mut Commands,
    keys: &Res<ButtonInput<KeyCode>>,
    text_inputs: &EditorTextInputs,
    search_inputs: &Query<
        &EditableText,
        (
            With<LibrarySearchInput>,
            Without<LyricsEditorInput>,
            Without<LanguageEditorInput>,
        ),
    >,
    audio: &Res<NativeAudio>,
    library_audio: &Res<NativeLibraryAudio>,
    pitch_audition: &Res<NativePitchAudition>,
    mut session: &mut StudioSession,
    mut authoring: &mut NativeAuthoringJob,
    mut invalidated: &mut UiInvalidated,
) {
    match action {
        UiAction::OpenSong(file_hash) => {
            session.song_context = None;
            session.selected_song = Some(file_hash.clone());
            session.route = StudioRoute::SongDetail;
            session.notice = None;
            invalidated.0 = true;
        }
        UiAction::AnalyzeSong(file_hash) => {
            session.song_context = None;
            if app_core::analysis_runtime_status().ready {
                app_core::enqueue_one(file_hash);
                session.analysis_tasks = app_core::load_analysis_tasks();
                session.library_view = LibraryView::Queue;
                session.library_facet = None;
                session.route = StudioRoute::Library;
                session.analysis_graph_needs_fit = true;
                session.refresh_library();
                session.notice = Some("Song queued for analysis.".to_string());
            } else {
                session.route = StudioRoute::Settings;
                session.settings_tab = SettingsTab::Models;
                session.notice = Some(
                    "Analysis is disabled until the runtime and selected models are installed."
                        .to_string(),
                );
            }
            invalidated.0 = true;
        }
        UiAction::OpenEditor(file_hash) => {
            session.song_context = None;
            session.selected_song = Some(file_hash.clone());
            session.editor = None;
            session.route = StudioRoute::Editor;
            if session.library_playback.status.playing
                && let Ok(status) = library_audio.0.pause()
            {
                session.library_playback.visible_position = status.position_secs;
                session.library_playback.status = status;
                session.library_playback.last_audio_sync = Instant::now();
            }
            session.notice = Some(start_editor_load_job(
                file_hash,
                Arc::clone(&audio.0),
                &mut session.editor_load_job,
            ));
            invalidated.0 = true;
        }
        UiAction::ExportUtz(file_hash) => {
            session.song_context = None;
            let export_directory = session.config.export_path.clone();
            session.notice = Some(start_export_job(
                file_hash,
                "utz",
                export_directory,
                &mut session.export_job,
            ));
            invalidated.0 = true;
        }
        UiAction::ExportUltraStar(file_hash) => {
            session.song_context = None;
            let export_directory = session.config.export_path.clone();
            session.notice = Some(start_export_job(
                file_hash,
                "txt",
                export_directory,
                &mut session.export_job,
            ));
            invalidated.0 = true;
        }
        UiAction::OpenSource(path) => {
            session.song_context = None;
            session.notice = Some(match validate_source_path(path, &session.config) {
                Ok(path) => match open::that_detached(&path) {
                    Ok(()) => format!("Opened {}", path.display()),
                    Err(error) => format!("Could not open {}: {error}", path.display()),
                },
                Err(error) => error,
            });
            invalidated.0 = true;
        }
        UiAction::RevealSource(path) => {
            session.song_context = None;
            session.notice = Some(reveal_library_entry(path, &session.config));
            invalidated.0 = true;
        }
        UiAction::DismissSongContext => {
            session.song_context = None;
            invalidated.0 = true;
        }
        UiAction::OpenLyricsEditor(file_hash) => {
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
            session.lyrics_editor = Some(NativeLyricsEditor {
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
            session.notice = None;
            invalidated.0 = true;
        }
        UiAction::CloseLyricsEditor => {
            session.lyrics_editor = None;
            invalidated.0 = true;
        }
        UiAction::ToggleLyricsInputMode => {
            if let Some(editor) = session.lyrics_editor.as_mut() {
                if let Ok(input) = text_inputs.lyrics.single() {
                    editor.initial_text = input.value().to_string();
                }
                editor.mode = if editor.mode == LyricsInputMode::Plain {
                    LyricsInputMode::TimedLrc
                } else {
                    LyricsInputMode::Plain
                };
                invalidated.0 = true;
            }
        }
        UiAction::ToggleLyricsSeparateStems => {
            if let Some(editor) = session.lyrics_editor.as_mut() {
                if let Ok(input) = text_inputs.lyrics.single() {
                    editor.initial_text = input.value().to_string();
                }
                editor.separate_stems = !editor.separate_stems;
                invalidated.0 = true;
            }
        }
        UiAction::SearchLrclibLyrics => {
            if session.lyrics_search_job.receiver.is_none()
                && let Some(editor) = session.lyrics_editor.as_mut()
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
                session.lyrics_search_job.receiver = Some(Mutex::new(receiver));
                session.notice = Some("Searching LRCLIB…".to_string());
                invalidated.0 = true;
            }
        }
        UiAction::PreviousLrclibCandidate => {
            if let Some(editor) = session.lyrics_editor.as_mut() {
                if let Ok(input) = text_inputs.lyrics.single() {
                    editor.initial_text = input.value().to_string();
                }
                editor.candidate_index = editor.candidate_index.saturating_sub(1);
                invalidated.0 = true;
            }
        }
        UiAction::NextLrclibCandidate => {
            if let Some(editor) = session.lyrics_editor.as_mut() {
                if let Ok(input) = text_inputs.lyrics.single() {
                    editor.initial_text = input.value().to_string();
                }
                editor.candidate_index =
                    (editor.candidate_index + 1).min(editor.candidates.len().saturating_sub(1));
                invalidated.0 = true;
            }
        }
        UiAction::UseLrclibPlain => {
            if let Some(editor) = session.lyrics_editor.as_mut()
                && let Some(candidate) = editor.candidates.get(editor.candidate_index)
            {
                editor.initial_text = candidate.lines.join("\n");
                editor.mode = LyricsInputMode::Plain;
                session.notice = Some("LRCLIB plain lyrics loaded for review.".to_string());
                invalidated.0 = true;
            }
        }
        UiAction::UseLrclibTimed => {
            if let Some(editor) = session.lyrics_editor.as_mut()
                && let Some(candidate) = editor.candidates.get(editor.candidate_index)
                && let Some(lrc) = candidate.synced_lyrics.as_ref()
            {
                editor.initial_text = lrc.clone();
                editor.mode = LyricsInputMode::TimedLrc;
                session.notice = Some("LRCLIB timed lyrics loaded for review.".to_string());
                invalidated.0 = true;
            }
        }
        UiAction::AdjustTranscriptBoundary(target, edge, delta_ms) => {
            let current = text_inputs
                .lyrics
                .single()
                .map(|input| input.value().to_string())
                .unwrap_or_default();
            if let Some(editor) = session.lyrics_editor.as_mut()
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
                        )?;
                        draft.replace_json(value.clone())?;
                        editor.initial_text =
                            serde_json::to_string_pretty(&value).unwrap_or_default();
                        editor.artifact_draft = Some(draft);
                        Ok(())
                    });
                session.notice = result.err();
            }
            invalidated.0 = true;
        }
        UiAction::PreviewTranscriptAt(file_hash, position_ms) => {
            session.notice = Some(match app_core::load_chart(file_hash) {
                Ok(chart) => {
                    let path = std::path::Path::new(&chart.audio.instrumental);
                    match audio
                        .0
                        .load_path(path)
                        .and_then(|_| audio.0.seek(*position_ms as f64 / 1000.0))
                        .and_then(|_| audio.0.play())
                    {
                        Ok(_) => format!(
                            "Previewing transcript at {}.",
                            format_duration(*position_ms as f64 / 1000.0)
                        ),
                        Err(error) => format!("Could not preview transcript audio: {error}"),
                    }
                }
                Err(error) => format!("Could not load transcript audio: {error}"),
            });
            invalidated.0 = true;
        }
        UiAction::SaveLyricsEditor | UiAction::SaveLyricsEditorAndRunDownstream => {
            let run_downstream = matches!(action, &UiAction::SaveLyricsEditorAndRunDownstream);
            let value = text_inputs
                .lyrics
                .single()
                .map(|input| input.value().to_string())
                .unwrap_or_default();
            if let Some(editor) = session.lyrics_editor.as_mut() {
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
                                    session.lyrics_editor = None;
                                    session.refresh_library();
                                    session.notice = Some(if run_downstream {
                                            "Artifact revision saved; confirmed downstream work was queued."
                                        } else {
                                            "Artifact revision saved without queueing analysis."
                                        }.to_string());
                                }
                                Err(error) => {
                                    session.notice = Some(format!(
                                        "Revision was saved, but downstream work could not be queued: {error}"
                                    ));
                                }
                            }
                        }
                        Err(error) => {
                            editor.artifact_draft = Some(draft);
                            session.notice = Some(format!("Could not save artifact: {error}"));
                        }
                    }
                    invalidated.0 = true;
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
                        session.lyrics_editor = None;
                        session.refresh_library();
                        session.notice = Some(
                            if requires_runtime {
                                "Lyrics saved and alignment queued."
                            } else {
                                "Timed lyrics saved."
                            }
                            .to_string(),
                        );
                    }
                    Err(error) => session.notice = Some(format!("Could not save lyrics: {error}")),
                }
                invalidated.0 = true;
            }
        }
        UiAction::OpenLanguageEditor(file_hash) => {
            let config = AppConfig::load();
            let initial_language = config
                .language_override(file_hash)
                .map(canonical_analysis_language)
                .unwrap_or_else(|| "auto".into());
            session.language_editor = Some(NativeLanguageEditor {
                file_hash: file_hash.clone(),
                initial_language,
                force_transcribe: false,
                picker_open: false,
            });
            session.notice = None;
            invalidated.0 = true;
        }
        UiAction::CloseLanguageEditor => {
            session.language_editor = None;
            invalidated.0 = true;
        }
        UiAction::ToggleLanguageReprocess => {
            if let Some(editor) = session.language_editor.as_mut() {
                editor.force_transcribe = !editor.force_transcribe;
                editor.picker_open = false;
                invalidated.0 = true;
            }
        }
        UiAction::ToggleLanguagePicker => {
            if let Some(editor) = session.language_editor.as_mut() {
                editor.picker_open = !editor.picker_open;
                invalidated.0 = true;
            }
        }
        UiAction::SelectAnalysisLanguage(language) => {
            if let Some(editor) = session.language_editor.as_mut() {
                editor.initial_language = canonical_analysis_language(language);
                editor.picker_open = false;
                invalidated.0 = true;
            }
        }
        UiAction::SaveLanguageEditor => {
            if let Some(editor) = session.language_editor.as_mut() {
                let language = editor.initial_language.clone();
                if !app_core::analysis_runtime_status().ready {
                    session.notice = Some(
                            "Analysis is disabled until setup is completed in Settings > Models & runtime."
                                .to_string(),
                        );
                } else {
                    if language == "auto" {
                        let mut config = AppConfig::load();
                        config.clear_language_override(&editor.file_hash);
                        if let Err(error) = config.save() {
                            session.notice =
                                Some(format!("Could not save the language setting: {error}"));
                            invalidated.0 = true;
                            return;
                        }
                        if editor.force_transcribe {
                            app_core::reanalyze_force_transcribe(&editor.file_hash);
                        } else {
                            let _ = app_core::realign(&editor.file_hash, None);
                        }
                    } else if editor.force_transcribe {
                        let mut config = AppConfig::load();
                        config.set_language_override(editor.file_hash.clone(), language.clone());
                        if let Err(error) = config.save() {
                            session.notice =
                                Some(format!("Could not save the language setting: {error}"));
                            invalidated.0 = true;
                            return;
                        }
                        app_core::reanalyze_force_transcribe(&editor.file_hash);
                    } else {
                        let _ = app_core::realign(&editor.file_hash, Some(language.clone()));
                    }
                    session.language_editor = None;
                    session.config = AppConfig::load();
                    session.notice = Some(if language == "auto" {
                        "Automatic language detection enabled; reprocessing queued.".into()
                    } else {
                        format!("Language set to {language}; reprocessing queued.")
                    });
                }
                invalidated.0 = true;
            }
        }
        UiAction::SaveNodeConfigAsSongProfile(file_hash, node_id) => {
            // Same immediacy as Freeze/Disable/Bypass above -- fires
            // right away and leaves the context menu open, rather than
            // dismissing it, matching that established pattern.
            session.notice = Some(
                match app_core::save_node_config_as_song_profile(file_hash, node_id) {
                    Ok(()) => {
                        format!("Saved {node_id}'s current configuration as this song's profile.")
                    }
                    Err(error) => format!("Could not save song profile: {error}"),
                },
            );
            invalidated.0 = true;
        }
        UiAction::OpenNodeConfigDialog(file_hash, node_id) => {
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
                session.node_config_dialog = Some(NativeNodeConfigDialog {
                    file_hash: file_hash.clone(),
                    node_id: node_id.clone(),
                    field,
                    value,
                    picker_open: false,
                });
            } else {
                session.notice = Some(format!(
                    "{node_id} has no parameter to configure for a single run."
                ));
            }
            session.analysis_node_context = None;
            invalidated.0 = true;
        }
        UiAction::CloseNodeConfigDialog => {
            session.node_config_dialog = None;
            invalidated.0 = true;
        }
        UiAction::ToggleNodeConfigPicker => {
            if let Some(dialog) = session.node_config_dialog.as_mut() {
                dialog.picker_open = !dialog.picker_open;
                invalidated.0 = true;
            }
        }
        UiAction::SelectNodeConfigValue(value) => {
            if let Some(dialog) = session.node_config_dialog.as_mut() {
                dialog.value = value.clone();
                dialog.picker_open = false;
                invalidated.0 = true;
            }
        }
        UiAction::RunNodeConfigDialog => {
            if let Some(dialog) = session.node_config_dialog.take() {
                session.notice = Some(
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
                invalidated.0 = true;
            }
        }
        UiAction::OpenPlanPreview(file_hash) => {
            session.plan_preview_draft = Some(PlanPreviewDraft {
                file_hash: file_hash.clone(),
                disabled_nodes: std::collections::BTreeSet::new(),
            });
            session.notice = None;
            invalidated.0 = true;
        }
        UiAction::ClosePlanPreview => {
            session.plan_preview_draft = None;
            invalidated.0 = true;
        }
        UiAction::TogglePlanPreviewDisabledNode(node_id) => {
            if let Some(draft) = session.plan_preview_draft.as_mut() {
                let id = app_core::AnalysisNodeId::new(node_id.clone());
                if !draft.disabled_nodes.remove(&id) {
                    draft.disabled_nodes.insert(id);
                }
                invalidated.0 = true;
            }
        }
        UiAction::RunPlanPreviewDraft => {
            if let Some(draft) = session.plan_preview_draft.take() {
                session.notice = Some(run_analysis_action_checked(&draft.file_hash, || {
                    app_core::run_analysis_plan(
                        &draft.file_hash,
                        std::collections::BTreeSet::new(),
                        draft.disabled_nodes.clone(),
                    )
                }));
                invalidated.0 = true;
            }
        }
        UiAction::OpenAppLogViewer(file_hash, node_id) => {
            session.app_log_viewer = Some(AppLogViewerState {
                file_hash: file_hash.clone(),
                node_id: node_id.clone(),
            });
            session.analysis_node_context = None;
            invalidated.0 = true;
        }
        UiAction::CloseAppLogViewer => {
            session.app_log_viewer = None;
            invalidated.0 = true;
        }
        UiAction::OpenAppLogFile => {
            session.notice = Some(match app_core::get_log_path() {
                Some(path) => match open::that_detached(&path) {
                    Ok(()) => format!("Opened {}", path.display()),
                    Err(error) => format!("Could not open {}: {error}", path.display()),
                },
                None => "The app log is not available in this environment.".to_string(),
            });
            invalidated.0 = true;
        }
        UiAction::OpenSongSettings(file_hash) => {
            session.song_settings = open_song_settings(file_hash);
            if session.song_settings.is_none() {
                session.notice = Some("Could not load this song's settings.".to_string());
            }
            invalidated.0 = true;
        }
        UiAction::CloseSongSettings => {
            session.song_settings = None;
            invalidated.0 = true;
        }
        UiAction::ChooseBackgroundVideo => {
            if session.song_settings.is_some()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Video", &["mp4", "webm", "mkv", "mov", "avi"])
                    .pick_file()
                && let Some(panel) = session.song_settings.as_mut()
            {
                panel.background_video_path = Some(path);
                invalidated.0 = true;
            }
        }
        UiAction::ClearBackgroundVideo => {
            if let Some(panel) = session.song_settings.as_mut() {
                panel.background_video_path = None;
                invalidated.0 = true;
            }
        }
        UiAction::SaveSongSettings => {
            if let Some(panel) = session.song_settings.take() {
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
                            session.notice =
                                Some("BPM must be a positive number, or left blank.".to_string());
                            session.song_settings = Some(panel);
                            invalidated.0 = true;
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
                        session.notice = Some("Song settings saved.".to_string());
                        session.refresh_library();
                    }
                    Err(error) => {
                        session.notice = Some(format!("Could not save song settings: {error}"));
                    }
                }
                invalidated.0 = true;
            }
        }
        UiAction::RealignSong(file_hash) => {
            session.notice = Some(run_analysis_action(file_hash, || {
                app_core::realign(file_hash, None)
            }));
            invalidated.0 = true;
        }
        UiAction::ReanalyzeTranscript(file_hash) => {
            session.notice = Some(run_analysis_action(file_hash, || {
                app_core::reanalyze_transcript(file_hash, None)
            }));
            invalidated.0 = true;
        }
        UiAction::ForceTranscribe(file_hash) => {
            session.notice = Some(run_analysis_action(file_hash, || {
                app_core::reanalyze_force_transcribe(file_hash)
            }));
            invalidated.0 = true;
        }
        UiAction::ReanalyzePitch(file_hash) => {
            session.notice = Some(run_analysis_action(file_hash, || {
                app_core::reanalyze_pitch(file_hash)
            }));
            invalidated.0 = true;
        }
        UiAction::ReanalyzeFull(file_hash) => {
            session.notice = Some(run_analysis_action(file_hash, || {
                app_core::reanalyze_full(file_hash)
            }));
            invalidated.0 = true;
        }
        UiAction::RunAnalysisNodeOnly(file_hash, node_id) => {
            session.notice = Some(run_analysis_action_checked(file_hash, || {
                app_core::run_analysis_node(file_hash, node_id)
            }));
            invalidated.0 = true;
        }
        UiAction::RunAnalysisNodeDownstream(file_hash, node_id) => {
            session.notice = Some(run_analysis_action_checked(file_hash, || {
                app_core::run_analysis_node_downstream(file_hash, node_id)
            }));
            invalidated.0 = true;
        }
        UiAction::DisableAnalysisNodeForRun(file_hash, node_id) => {
            session.notice = Some(run_analysis_action_checked(file_hash, || {
                app_core::disable_analysis_node_for_run(file_hash, node_id)
            }));
            invalidated.0 = true;
        }
        UiAction::FreezeAnalysisNodeOutputs(file_hash, node_id) => {
            session.notice = Some(run_analysis_action_checked(file_hash, || {
                app_core::freeze_analysis_node_outputs_for_run(file_hash, node_id)
            }));
            invalidated.0 = true;
        }
        UiAction::BypassAnalysisNodeWithOriginalMix(file_hash, node_id) => {
            session.notice = Some(run_analysis_action_checked(file_hash, || {
                app_core::bypass_analysis_node_with_original_mix_for_run(file_hash, node_id)
            }));
            invalidated.0 = true;
        }
        UiAction::CompareNodeAttemptWithPrevious(file_hash, node_id, current_run_id) => {
            session.notice = Some(
                match app_core::compare_node_attempt_with_previous_run(
                    file_hash,
                    node_id,
                    *current_run_id,
                ) {
                    Ok(comparison) => format_node_attempt_comparison(&comparison),
                    Err(error) => format!("Could not compare attempts: {error}"),
                },
            );
            invalidated.0 = true;
        }
        UiAction::ToggleAnalysisCompoundNode(node_id) => {
            let id = app_core::AnalysisNodeId::new(node_id.clone());
            if !session.expanded_compound_nodes.remove(&id) {
                session.expanded_compound_nodes.insert(id);
            }
            session.analysis_node_context = None;
            invalidated.0 = true;
        }
        UiAction::RequestDeleteSongCache(file_hash) => {
            session.pending_cache_delete = Some(file_hash.clone());
            invalidated.0 = true;
        }
        UiAction::CancelDeleteSongCache => {
            session.pending_cache_delete = None;
            invalidated.0 = true;
        }
        UiAction::CancelAnalysisRun(file_hash) => {
            session.notice = Some(match app_core::cancel_analysis_run(file_hash) {
                Ok(()) => "Removed from the analysis queue.".to_string(),
                Err(error) => format!("Could not cancel: {error}"),
            });
            invalidated.0 = true;
        }
        UiAction::ConfirmDeleteSongCache => {
            if let Some(file_hash) = session.pending_cache_delete.take() {
                app_core::delete_cache(&file_hash);
                session.refresh_library();
                session.notice =
                    Some("Generated song data deleted. Source media was not changed.".to_string());
                invalidated.0 = true;
            }
        }
        UiAction::RequestReplaceAuthoredChart(file_hash) => {
            session.analysis_artifact_context = None;
            session.pending_chart_replace = Some(file_hash.clone());
            invalidated.0 = true;
        }
        UiAction::CancelReplaceAuthoredChart => {
            session.pending_chart_replace = None;
            invalidated.0 = true;
        }
        UiAction::ConfirmReplaceAuthoredChart => {
            if let Some(file_hash) = session.pending_chart_replace.take() {
                session.notice = Some(
                        match app_core::replace_authored_chart_with_fresh_analysis(&file_hash) {
                            Ok(()) => {
                                "Authored chart discarded. It will be rebuilt from the latest analysis the next time you open the editor.".to_string()
                            }
                            Err(error) => format!("Could not replace the chart: {error}"),
                        },
                    );
                invalidated.0 = true;
            }
        }
        UiAction::SyncArtifactRevisions(file_hash) => {
            let imported = app_core::import_legacy_artifacts(&app_core::CacheDir::new(), file_hash);
            session.notice = Some(if imported.is_empty() {
                "No new artifact revisions found on disk.".to_string()
            } else {
                format!(
                    "Recorded {} artifact revision(s) from disk.",
                    imported.len()
                )
            });
            invalidated.0 = true;
        }
        UiAction::SetActiveArtifactRevision(revision) => {
            session.pending_artifact_active = Some(revision.clone());
            invalidated.0 = true;
        }
        UiAction::CancelSetActiveArtifactRevision => {
            session.pending_artifact_active = None;
            invalidated.0 = true;
        }
        UiAction::ConfirmSetActiveArtifactRevision => {
            let Some(revision) = session.pending_artifact_active.take() else {
                return;
            };
            let cache_root = app_core::CacheDir::new().path;
            session.notice = Some(
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
            invalidated.0 = true;
        }
        UiAction::RequestCaptureIntermediate(file_hash) => {
            session.pending_intermediate_capture = Some(file_hash.clone());
            session.analysis_node_context = None;
            invalidated.0 = true;
        }
        UiAction::CancelCaptureIntermediate => {
            session.pending_intermediate_capture = None;
            invalidated.0 = true;
        }
        UiAction::ConfirmCaptureIntermediateOnce
        | UiAction::ConfirmCaptureIntermediatePersistent
        | UiAction::ConfirmDisableIntermediateCapture => {
            let Some(file_hash) = session.pending_intermediate_capture.take() else {
                return;
            };
            let enabled = !matches!(action, UiAction::ConfirmDisableIntermediateCapture);
            let persistent = matches!(action, UiAction::ConfirmCaptureIntermediatePersistent);
            let request = app_core::CaptureIntermediateRequest {
                file_hash,
                node_id: app_core::AnalysisNodeId::new("lyrics.preprocess"),
                kind: app_core::ArtifactKind::PreprocessedAudio,
                enabled,
                persistent,
            };
            session.notice = Some(match app_core::set_intermediate_capture_request(&request) {
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
            invalidated.0 = true;
        }
        UiAction::OpenArtifactRevision(path) => {
            session.notice = Some(open_artifact_entry(path));
            invalidated.0 = true;
        }
        UiAction::PreviewArtifactRevision(path) => {
            session.notice = Some(preview_artifact_entry(path));
            invalidated.0 = true;
        }
        UiAction::RevealArtifactRevision(path) => {
            session.notice = Some(reveal_artifact_entry(path));
            invalidated.0 = true;
        }
        UiAction::RequestDeleteArtifactRevision(revision) => {
            session.pending_artifact_delete = Some(revision.clone());
            invalidated.0 = true;
        }
        UiAction::CancelDeleteArtifactRevision => {
            session.pending_artifact_delete = None;
            invalidated.0 = true;
        }
        UiAction::ConfirmDeleteArtifactRevision => {
            if let Some(revision) = session.pending_artifact_delete.take() {
                let cache_root = app_core::CacheDir::new().path;
                session.notice = Some(
                    match app_core::delete_artifact_revision(&cache_root, &revision) {
                        Ok(()) => "Artifact revision deleted.".to_string(),
                        Err(error) => format!("Could not delete artifact revision: {error}"),
                    },
                );
                invalidated.0 = true;
            }
        }
        UiAction::RequestInvalidateArtifactRevision(revision) => {
            session.pending_artifact_invalidate = Some(revision.clone());
            invalidated.0 = true;
        }
        UiAction::CancelInvalidateArtifactRevision => {
            session.pending_artifact_invalidate = None;
            invalidated.0 = true;
        }
        UiAction::ConfirmInvalidateArtifactRevision => {
            if let Some(revision) = session.pending_artifact_invalidate.take() {
                let cache_root = app_core::CacheDir::new().path;
                session.notice = Some(
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
                invalidated.0 = true;
            }
        }
        UiAction::InspectArtifactProvenance(revision) => {
            session.notice = Some(format_artifact_provenance(revision));
            invalidated.0 = true;
        }
        UiAction::CompareArtifactRevisions(revision, other) => {
            let left = artifact_ref_from_revision(revision);
            match app_core::compare_artifacts_typed(&left, other) {
                Ok(diff) => {
                    session.artifact_diff = Some(diff);
                    session.notice = None;
                }
                Err(error) => {
                    session.notice = Some(format!("Could not compare revisions: {error}"));
                }
            }
            invalidated.0 = true;
        }
        UiAction::CloseArtifactDiff => {
            session.artifact_diff = None;
            invalidated.0 = true;
        }
        UiAction::ShiftSongKey(file_hash, delta) => {
            session.notice = Some(start_key_shift(
                file_hash,
                *delta,
                &mut authoring,
                &mut session.authoring_busy,
            ));
            invalidated.0 = true;
        }
        UiAction::ShiftSongTempo(file_hash, delta) => {
            session.notice = Some(start_tempo_shift(
                file_hash,
                *delta,
                &mut authoring,
                &mut session.authoring_busy,
            ));
            invalidated.0 = true;
        }
        UiAction::PlayLibrarySong(file_hash) => {
            let queue = session.songs.processed.clone();
            prepare_library_queue(&queue, file_hash, &mut session.library_playback);
            session.notice =
                play_library_song(&library_audio.0, file_hash, &mut session.library_playback).err();
            invalidated.0 = true;
        }
        UiAction::PlayArtifactRevision(path) => {
            session.notice =
                play_artifact_revision(&library_audio.0, path, &mut session.library_playback).err();
            invalidated.0 = true;
        }
        UiAction::ToggleLibraryPlayback => {
            session.notice =
                toggle_library_playback(&library_audio.0, &mut session.library_playback).err();
            invalidated.0 = true;
        }
        UiAction::SeekLibraryRelative(delta) => {
            session.notice = seek_library_relative(
                &library_audio.0,
                &mut session.library_playback,
                f64::from(*delta),
            )
            .err();
            invalidated.0 = true;
        }
        UiAction::PreviousLibrarySong => {
            session.notice = if library_visible_position(&session.library_playback) > 5.0 {
                restart_library_song(&library_audio.0, &mut session.library_playback).err()
            } else {
                let wrap = session.library_playback.repeat == LibraryRepeatMode::All;
                advance_library_queue(&library_audio.0, &mut session.library_playback, -1, wrap)
                    .err()
            };
            invalidated.0 = true;
        }
        UiAction::NextLibrarySong => {
            let wrap = session.library_playback.repeat == LibraryRepeatMode::All;
            session.notice =
                advance_library_queue(&library_audio.0, &mut session.library_playback, 1, wrap)
                    .err();
            invalidated.0 = true;
        }
        UiAction::ToggleLibraryShuffle => {
            session.library_playback.shuffle = !session.library_playback.shuffle;
            session.notice = Some(if session.library_playback.shuffle {
                "Shuffle enabled for the playback queue.".to_string()
            } else {
                "Shuffle disabled.".to_string()
            });
            invalidated.0 = true;
        }
        UiAction::CycleLibraryRepeat => {
            session.library_playback.repeat = session.library_playback.repeat.next();
            session.notice = Some(session.library_playback.repeat.label().to_string());
            invalidated.0 = true;
        }
        UiAction::AdjustLibraryVolume(delta) => {
            let volume = session.library_playback.volume + f64::from(*delta) / 100.0;
            session.notice =
                set_library_volume(&library_audio.0, &mut session.library_playback, volume).err();
            invalidated.0 = true;
        }
        UiAction::ToggleLibraryMute => {
            let volume = if session.library_playback.volume > 0.0 {
                session.library_playback.volume_before_mute = session.library_playback.volume;
                0.0
            } else {
                session.library_playback.volume_before_mute.max(0.1)
            };
            session.notice =
                set_library_volume(&library_audio.0, &mut session.library_playback, volume).err();
            invalidated.0 = true;
        }
        UiAction::ToggleLibraryQueue => {
            session.library_playback.queue_open = !session.library_playback.queue_open;
            invalidated.0 = true;
        }
        UiAction::DismissLyricContext => {
            if let Some(editor) = session.editor.as_mut() {
                editor.lyric_context = None;
            }
            invalidated.0 = true;
        }
        UiAction::DismissNoteContext => {
            if let Some(editor) = session.editor.as_mut() {
                editor.note_context = None;
            }
            invalidated.0 = true;
        }
        UiAction::SelectWaveformSource(source) => {
            let mut notice = None;
            if let Some(editor) = session.editor.as_mut() {
                if *source == WaveformSource::Vocals && editor.chart.audio.vocals.is_none() {
                    notice = Some("This chart has no separate vocal source.".to_string());
                } else {
                    set_editor_waveform_source(editor, *source);
                }
                editor.waveform_context = None;
            }
            if notice.is_some() {
                session.notice = notice;
            }
            invalidated.0 = true;
        }
        UiAction::SelectWaveformStyle(style) => {
            if let Some(editor) = session.editor.as_mut() {
                editor.waveform_style = *style;
                editor.waveform_context = None;
            }
            invalidated.0 = true;
        }
        UiAction::DismissWaveformContext => {
            if let Some(editor) = session.editor.as_mut() {
                editor.waveform_context = None;
            }
            invalidated.0 = true;
        }
        UiAction::DismissProblemsPanel => {
            if let Some(editor) = session.editor.as_mut() {
                editor.problems_panel_open = false;
            }
            invalidated.0 = true;
        }
        UiAction::DismissShortcutsPanel => {
            if let Some(editor) = session.editor.as_mut() {
                editor.shortcuts_panel_open = false;
            }
            invalidated.0 = true;
        }
        UiAction::SetProblemsFilter(filter) => {
            if let Some(editor) = session.editor.as_mut() {
                editor.problems_filter = *filter;
            }
            invalidated.0 = true;
        }
        UiAction::ApplyAllLyricsEdit => {
            let mut notice = None;
            if let Some(editor) = session.editor.as_mut()
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
                session.notice = notice;
            }
            invalidated.0 = true;
        }
        UiAction::ExtendLyricOverNote(word, note_index) => {
            if let Some(editor) = session.editor.as_mut() {
                editor.checkpoint("Continue syllable");
                if extend_editor_lyric(&mut editor.document, *word, *note_index) {
                    editor.dirty = true;
                    editor.note_context = None;
                    session.notice = Some("Extended the syllable onto this note.".to_string());
                } else {
                    editor.undo.pop();
                    session.notice = Some(
                            "That note can't continue the syllable — it needs to sit right after it, in the same phrase, with no lyric of its own.".to_string(),
                        );
                }
            }
            invalidated.0 = true;
        }
        UiAction::Editor(_)
        | UiAction::FocusChartProblem(..)
        | UiAction::OpenEditorSelect(_)
        | UiAction::SelectEditorValue(..)
        | UiAction::SelectEditorWord(..)
        | UiAction::SelectEditorTrack(_)
        | UiAction::MoveSelectionToTrack(_)
        | UiAction::SetNoteKind(_) => {
            handle_editor_ui_action(
                action,
                &keys,
                &mut EditorActionContext {
                    audio: &audio.0,
                    tones: &pitch_audition.0,
                    session: &mut session,
                    invalidated: &mut invalidated,
                },
            );
        }

        _ => {}
    }
}
