use crate::studio::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_chrome_action(
    action: &UiAction,
    commands: &mut Commands,
    keys: &Res<ButtonInput<KeyCode>>,
    search_inputs: &Query<
        &EditableText,
        (
            With<LibrarySearchInput>,
            Without<LyricsEditorInput>,
            Without<LanguageEditorInput>,
        ),
    >,
    window_entity: Entity,
    window: &mut Window,
    _graph_viewport_width: Option<f32>,
    audio: &Res<NativeAudio>,
    library_audio: &Res<NativeLibraryAudio>,
    session: &mut StudioSession,
    invalidated: &mut UiInvalidated,
) -> bool {
    match action {
        UiAction::ToggleGlobalSearch => {
            session.search_open = !session.search_open;
            session.activity_open = false;
            session.about_open = false;
            invalidated.0 = true;
        }
        UiAction::ToggleActivity => {
            session.activity_open = !session.activity_open;
            session.search_open = false;
            session.about_open = false;
            session.analysis_tasks = app_core::load_analysis_tasks();
            invalidated.0 = true;
        }
        UiAction::CloseActivity => {
            session.activity_open = false;
            invalidated.0 = true;
        }
        UiAction::SelectAnalysisHistory(id) => {
            session.selected_analysis_history = *id;
            session.selected_analysis_stage = None;
            session.activity_open = false;
            invalidated.0 = true;
        }
        UiAction::SelectAnalysisStage(stage) => {
            session.selected_analysis_stage = Some(stage.clone());
            session.analysis_node_context = None;
            invalidated.0 = true;
        }
        UiAction::OpenAnalysisInspect(stage) => {
            session.selected_analysis_stage = Some(stage.clone());
            session.analysis_node_context = None;
            session.library_view = LibraryView::Queue;
            session.route = StudioRoute::AnalysisInspect;
            invalidated.0 = true;
        }
        UiAction::AdjustAnalysisGraphZoom(delta_percent) => {
            session.analysis_graph_zoom = clamp_analysis_graph_zoom(
                session.analysis_graph_zoom + (*delta_percent as f32) / 100.0,
            );
            session.analysis_graph_needs_fit = false;
            invalidated.0 = true;
        }
        UiAction::ToggleAnalysisMiniView => {
            session.analysis_mini_view = !session.analysis_mini_view;
            session.analysis_graph_needs_fit = true;
            invalidated.0 = true;
        }
        UiAction::FitAnalysisGraph(_) => {
            session.analysis_graph_needs_fit = true;
            session.analysis_graph_scroll_offset = 0.0;
            invalidated.0 = true;
        }
        UiAction::FocusAnalysisGraphNode(scroll, stage_id) => {
            session.analysis_graph_scroll_offset = (*scroll).max(0) as f32;
            session.selected_analysis_stage = Some(stage_id.clone());
            invalidated.0 = true;
        }
        UiAction::DismissAnalysisNodeContext => {
            session.analysis_node_context = None;
            invalidated.0 = true;
        }
        UiAction::RequestClearAnalysisHistory => {
            session.pending_analysis_history_clear = true;
            invalidated.0 = true;
        }
        UiAction::CancelClearAnalysisHistory => {
            session.pending_analysis_history_clear = false;
            invalidated.0 = true;
        }
        UiAction::ConfirmClearAnalysisHistory => {
            session.pending_analysis_history_clear = false;
            match app_core::clear_analysis_history() {
                Ok(()) => {
                    session.analysis_history.clear();
                    session.selected_analysis_history = None;
                    session.selected_analysis_stage = None;
                    session.notice = Some("Analysis history deleted.".into());
                }
                Err(error) => {
                    session.notice = Some(format!("Could not delete analysis history: {error}"));
                }
            }
            invalidated.0 = true;
        }
        UiAction::OpenAbout => {
            session.about_open = true;
            session.activity_open = false;
            session.search_open = false;
            invalidated.0 = true;
        }
        UiAction::CloseAbout => {
            session.about_open = false;
            invalidated.0 = true;
        }
        UiAction::Back => {
            session.open_settings_select = None;
            session.open_editor_select = None;
            if session.route == StudioRoute::Editor {
                if session.editor.as_ref().is_some_and(|editor| editor.dirty) {
                    session.pending_leave = Some(PendingLeave::Back);
                } else {
                    let _ = audio.0.stop();
                    session.editor = None;
                    session.route = StudioRoute::SongDetail;
                    session.notice = None;
                }
                invalidated.0 = true;
            } else if session.route == StudioRoute::AnalysisInspect {
                session.route = StudioRoute::Library;
                session.library_view = LibraryView::Queue;
                session.notice = None;
                invalidated.0 = true;
            } else if session.route != StudioRoute::Library {
                session.route = StudioRoute::Library;
                session.notice = None;
                invalidated.0 = true;
            }
        }
        UiAction::Home => {
            session.open_settings_select = None;
            session.open_editor_select = None;
            if session.route == StudioRoute::Editor {
                if session.editor.as_ref().is_some_and(|editor| editor.dirty) {
                    session.pending_leave = Some(PendingLeave::Home);
                    invalidated.0 = true;
                    return true;
                }
                let _ = audio.0.stop();
                session.editor = None;
            }
            if session.route != StudioRoute::Library {
                session.route = StudioRoute::Library;
                session.notice = None;
                invalidated.0 = true;
            }
        }
        UiAction::CancelLeave => {
            session.pending_leave = None;
            invalidated.0 = true;
        }
        UiAction::ConfirmLeave => {
            let destination = session.pending_leave.take();
            let _ = audio.0.stop();
            match destination {
                Some(PendingLeave::Exit) => {
                    let _ = library_audio.0.stop();
                    commands.entity(window_entity).despawn();
                }
                Some(PendingLeave::Back) => {
                    session.editor = None;
                    session.route = StudioRoute::SongDetail;
                    session.notice = None;
                    invalidated.0 = true;
                }
                Some(PendingLeave::Home) => {
                    session.editor = None;
                    session.route = StudioRoute::Library;
                    session.notice = None;
                    invalidated.0 = true;
                }
                Some(PendingLeave::Documentation) => {
                    session.editor = None;
                    session.route = StudioRoute::Documentation;
                    session.notice = None;
                    invalidated.0 = true;
                }
                None => {}
            }
        }
        UiAction::SetLibraryView(view) => {
            let view_changed = session.library_view != *view;
            session.library_view = *view;
            session.library_status = None;
            session.library_search = None;
            session.library_facet = None;
            session.route = StudioRoute::Library;
            session.song_context = None;
            session.activity_open = false;
            session.about_open = false;
            session.search_open = false;
            session.notice = None;
            if view_changed {
                session.library_scroll_offset = 0.0;
                session.analysis_graph_scroll_offset = 0.0;
                if *view == LibraryView::Queue {
                    session.analysis_graph_needs_fit = true;
                }
            }
            session.refresh_library();
            invalidated.0 = true;
        }
        UiAction::SetLibraryFacet(facet) => {
            session.library_view = LibraryView::All;
            session.library_search = None;
            session.library_facet = Some(facet.clone());
            session.route = StudioRoute::Library;
            session.song_context = None;
            session.notice = None;
            session.refresh_library();
            invalidated.0 = true;
        }
        UiAction::LoadMoreSongs => {
            session.load_more_songs();
            invalidated.0 = true;
        }
        UiAction::ApplyLibrarySearch => {
            let value = search_inputs
                .single()
                .map(|input| input.value().to_string())
                .unwrap_or_default();
            let value = value.trim();
            session.library_search = (!value.is_empty()).then(|| value.to_string());
            session.route = StudioRoute::Library;
            session.library_view = LibraryView::All;
            session.library_facet = None;
            session.search_open = false;
            session.refresh_library();
            session.notice = None;
            invalidated.0 = true;
        }
        UiAction::ClearLibrarySearch => {
            session.library_search = None;
            session.route = StudioRoute::Library;
            session.refresh_library();
            invalidated.0 = true;
        }
        UiAction::ToggleLibraryLayout => {
            session.config.song_list_view = Some(
                if session.config.song_list_view.as_deref() == Some("grid") {
                    "table"
                } else {
                    "grid"
                }
                .to_string(),
            );
            if let Some(error) = save_config_error(&session.config) {
                session.notice = Some(error);
            }
            invalidated.0 = true;
        }
        UiAction::ToggleExportAllMenu => {
            session.export_all_open = !session.export_all_open;
            session.open_library_select = None;
            invalidated.0 = true;
        }
        UiAction::ExportAllUtz | UiAction::ExportAllUltraStar => {
            session.export_all_open = false;
            let extension = if matches!(action, UiAction::ExportAllUtz) {
                "utz"
            } else {
                "txt"
            };
            if let Some(export_directory) = session.config.export_path.clone() {
                session.notice = Some(start_export_all_job(
                    extension,
                    export_directory,
                    &mut session.export_job,
                ));
            } else {
                session.route = StudioRoute::Settings;
                session.settings_tab = SettingsTab::Storage;
                session.request_cache_stats_refresh = true;
                session.notice =
                    Some("Choose a default export folder before using Export all.".to_string());
            }
            invalidated.0 = true;
        }
        UiAction::OpenLibrarySelect(kind) => {
            session.open_library_select = if session.open_library_select == Some(*kind) {
                None
            } else {
                Some(*kind)
            };
            session.export_all_open = false;
            invalidated.0 = true;
        }
        UiAction::SelectLibraryValue(kind, value) => {
            let value = (value != "all").then(|| value.clone());
            match kind {
                LibrarySelectKind::Status => session.library_status = value,
                LibrarySelectKind::TranscriptSource => session.library_transcript_source = value,
            }
            session.open_library_select = None;
            session.refresh_library();
            session.notice = None;
            invalidated.0 = true;
        }
        UiAction::AnalyzeAll => {
            if app_core::analysis_runtime_status().ready {
                app_core::enqueue_all(&session.library_filters());
                session.analysis_tasks = app_core::load_analysis_tasks();
                session.library_view = LibraryView::Queue;
                session.library_facet = None;
                session.route = StudioRoute::Library;
                session.refresh_library();
                session.notice = Some("Matching unanalyzed songs were queued.".to_string());
            } else {
                session.route = StudioRoute::Settings;
                session.settings_tab = SettingsTab::Models;
                session.notice = Some(
                    "Analysis is disabled until setup is completed in Settings > Models & runtime."
                        .to_string(),
                );
            }
            invalidated.0 = true;
        }
        UiAction::Folders => {
            session.route = StudioRoute::Folders;
            session.notice = None;
            session.folder_browser.context_menu = None;
            if session.folder_browser.root.is_none()
                && let Some(path) = session.config.library_paths().into_iter().next()
            {
                session.folder_browser.select_root(path);
            }
            invalidated.0 = true;
        }
        UiAction::DismissAnalysisArtifactContext => {
            session.analysis_artifact_context = None;
            invalidated.0 = true;
        }
        UiAction::DismissAnalysisExportContext => {
            session.analysis_export_context = None;
            invalidated.0 = true;
        }
        UiAction::ToggleAnalysisLineageMode => {
            session.analysis_lineage_mode = !session.analysis_lineage_mode;
            if !session.analysis_lineage_mode {
                session.artifact_lineage = None;
            }
            invalidated.0 = true;
        }
        UiAction::ValidateExportNode(file_hash, kind) => {
            session.analysis_export_context = None;
            session.notice = Some(match app_core::validate_export_node(file_hash, *kind) {
                Ok(message) => message,
                Err(error) => error,
            });
            invalidated.0 = true;
        }
        UiAction::RevealLastExport(file_hash, kind) => {
            session.analysis_export_context = None;
            session.notice = Some(match app_core::last_export_destination(file_hash, *kind) {
                Some(path) if path.is_file() => {
                    let target = path.parent().unwrap_or(path.as_path());
                    match open::that_detached(target) {
                        Ok(()) => format!("Revealed {}", path.display()),
                        Err(error) => format!("Could not reveal {}: {error}", path.display()),
                    }
                }
                Some(path) => format!("last export is missing: {}", path.display()),
                None => "No last export is tracked yet.".to_string(),
            });
            invalidated.0 = true;
        }
        UiAction::Documentation => {
            let origin = session.route;
            if origin != StudioRoute::Documentation {
                session.documentation.return_route = Some(origin);
                session.documentation.back_stack.clear();
                session.documentation.forward_stack.clear();
                session.documentation.anchor = None;
            }
            session
                .documentation
                .navigate(Some("guide:getting-started".to_string()));
            if session.route == StudioRoute::Editor
                && session.editor.as_ref().is_some_and(|editor| editor.dirty)
            {
                session.pending_leave = Some(PendingLeave::Documentation);
            } else {
                session.route = StudioRoute::Documentation;
                session.notice = None;
            }
            invalidated.0 = true;
        }
        UiAction::OpenDocumentation(anchor) => {
            let origin = session.route;
            if origin != StudioRoute::Documentation {
                session.documentation.return_route = Some(origin);
                session.documentation.back_stack.clear();
                session.documentation.forward_stack.clear();
                session.documentation.anchor = None;
            }
            session.documentation.navigate(anchor.clone());
            if session.route == StudioRoute::Editor
                && session.editor.as_ref().is_some_and(|editor| editor.dirty)
            {
                session.pending_leave = Some(PendingLeave::Documentation);
            } else {
                session.route = StudioRoute::Documentation;
                session.notice = None;
            }
            invalidated.0 = true;
        }
        UiAction::DocumentationBack => {
            if !session.documentation.go_back() {
                session.route = session
                    .documentation
                    .return_route
                    .take()
                    .unwrap_or(StudioRoute::Library);
                session.documentation.forward_stack.clear();
            }
            session.notice = None;
            invalidated.0 = true;
        }
        UiAction::DocumentationForward => {
            if session.documentation.go_forward() {
                invalidated.0 = true;
            }
        }
        UiAction::SelectArtifactInspectorTab(tab) => {
            session.selected_artifact_inspector_tab = *tab;
            invalidated.0 = true;
        }
        UiAction::ToggleArtifactPinned(reference) => {
            match app_core::inspect_artifact(reference) {
                Ok(inspection) => {
                    let target = !inspection.pinned;
                    session.notice = match app_core::set_artifact_pinned(reference, target) {
                        Ok(()) => Some(if target {
                            "Artifact revision pinned. It is protected from deletion.".to_string()
                        } else {
                            "Artifact revision unpinned.".to_string()
                        }),
                        Err(error) => Some(error),
                    };
                }
                Err(error) => session.notice = Some(error),
            }
            invalidated.0 = true;
        }
        UiAction::ShowArtifactLineage(reference) => {
            match app_core::artifact_lineage(reference) {
                Ok(lineage) => {
                    session.analysis_lineage_mode = true;
                    session.artifact_lineage = Some(ArtifactLineagePanel {
                        lineage,
                        scope: session.analysis_lineage_scope,
                        selected: reference.clone(),
                    });
                }
                Err(error) => session.notice = Some(error),
            }
            invalidated.0 = true;
        }
        UiAction::SetArtifactLineageScope(scope) => {
            session.analysis_lineage_scope = *scope;
            if let Some(panel) = session.artifact_lineage.as_mut() {
                panel.scope = *scope;
            }
            invalidated.0 = true;
        }
        UiAction::SelectArtifactLineageRevision(reference) => {
            if let Some(panel) = session.artifact_lineage.as_mut() {
                panel.selected = reference.clone();
            }
            invalidated.0 = true;
        }
        UiAction::CloseArtifactLineage => {
            session.artifact_lineage = None;
            session.analysis_lineage_mode = false;
            invalidated.0 = true;
        }
        UiAction::ShowArtifactImpact(reference) => {
            match app_core::preview_artifact_downstream_impact(reference) {
                Ok(impact) => session.artifact_impact = Some(impact),
                Err(error) => session.notice = Some(error),
            }
            invalidated.0 = true;
        }
        UiAction::CloseArtifactImpact => {
            session.artifact_impact = None;
            invalidated.0 = true;
        }
        UiAction::ConfirmArtifactImpact => {
            if let Some(impact) = session.artifact_impact.take() {
                let request = app_core::analysis_request_from_impact(&impact.file_hash, &impact);
                session.notice = Some(match app_core::run_analysis_request(request) {
                    Ok(()) => {
                        session.analysis_tasks = app_core::load_analysis_tasks();
                        "Confirmed impact plan was queued.".to_string()
                    }
                    Err(error) => error,
                });
            }
            invalidated.0 = true;
        }
        UiAction::OpenArtifactCompatibleEditor(reference) => {
            match reference.kind {
                app_core::ArtifactKind::LyricsInput
                | app_core::ArtifactKind::RecognizedText
                | app_core::ArtifactKind::TimedTranscript
                | app_core::ArtifactKind::AsrSegments => {
                    match app_core::begin_artifact_edit(reference) {
                        Ok(draft) => {
                            let (mode, initial_text) = match &draft.working_copy {
                                app_core::ArtifactDraftContent::Text(text) => {
                                    (LyricsInputMode::Plain, text.clone())
                                }
                                app_core::ArtifactDraftContent::Json(value) => (
                                    LyricsInputMode::StructuredTimedTranscript,
                                    serde_json::to_string_pretty(value).unwrap_or_default(),
                                ),
                            };
                            session.selected_song = Some(reference.file_hash.clone());
                            session.route = StudioRoute::SongDetail;
                            session.lyrics_editor = Some(NativeLyricsEditor {
                                file_hash: reference.file_hash.clone(),
                                mode,
                                separate_stems: true,
                                initial_text,
                                candidates: Vec::new(),
                                candidate_index: 0,
                                searching: false,
                                artifact_draft: Some(draft),
                                waveform: app_core::ChartWaveform::default(),
                            });
                            if mode == LyricsInputMode::StructuredTimedTranscript {
                                let _ = audio.0.stop();
                                start_lyrics_waveform_job(
                                    &reference.file_hash,
                                    &mut session.lyrics_waveform_job,
                                );
                            }
                            session.notice = Some(
                                    "Opened an immutable artifact revision as an editable working copy."
                                        .to_string(),
                                );
                        }
                        Err(error) => session.notice = Some(error),
                    }
                }
                app_core::ArtifactKind::CandidateChart
                | app_core::ArtifactKind::AuthoredChart
                | app_core::ArtifactKind::PitchTrack
                | app_core::ArtifactKind::PitchNoteCandidates => {
                    session.selected_song = Some(reference.file_hash.clone());
                    session.editor = None;
                    session.route = StudioRoute::Editor;
                    session.notice = Some(start_editor_revision_load_job(
                        reference.clone(),
                        Arc::clone(&audio.0),
                        &mut session.editor_load_job,
                    ));
                }
                _ => {
                    session.notice = Some(
                        "No compatible in-app editor exists for this artifact kind.".to_string(),
                    );
                }
            }
            invalidated.0 = true;
        }
        UiAction::MergeCandidateChart(candidate, authored, mode) => {
            session.selected_song = Some(candidate.file_hash.clone());
            session.editor = None;
            session.route = StudioRoute::Editor;
            session.notice = Some(start_editor_merge_load_job(
                candidate.clone(),
                authored.clone(),
                *mode,
                Arc::clone(&audio.0),
                &mut session.editor_load_job,
            ));
            invalidated.0 = true;
        }
        UiAction::MergeSelectedCandidatePhrase(candidate, authored) => {
            session.analysis_artifact_context = None;
            match merge_mode_from_editor_selection(session.editor.as_ref(), true) {
                Ok(mode) => {
                    session.selected_song = Some(candidate.file_hash.clone());
                    session.editor = None;
                    session.route = StudioRoute::Editor;
                    session.notice = Some(start_editor_merge_load_job(
                        candidate.clone(),
                        authored.clone(),
                        mode,
                        Arc::clone(&audio.0),
                        &mut session.editor_load_job,
                    ));
                }
                Err(error) => session.notice = Some(error),
            }
            invalidated.0 = true;
        }
        UiAction::MergeSelectedCandidateRange(candidate, authored) => {
            session.analysis_artifact_context = None;
            match merge_mode_from_editor_selection(session.editor.as_ref(), false) {
                Ok(mode) => {
                    session.selected_song = Some(candidate.file_hash.clone());
                    session.editor = None;
                    session.route = StudioRoute::Editor;
                    session.notice = Some(start_editor_merge_load_job(
                        candidate.clone(),
                        authored.clone(),
                        mode,
                        Arc::clone(&audio.0),
                        &mut session.editor_load_job,
                    ));
                }
                Err(error) => session.notice = Some(error),
            }
            invalidated.0 = true;
        }
        UiAction::KeepAuthoredChart => {
            session.analysis_artifact_context = None;
            session.pending_chart_replace = None;
            session.notice =
                Some("Authored chart kept. The candidate revision was not applied.".to_string());
            invalidated.0 = true;
        }

        _ => return false,
    }
    true
}
