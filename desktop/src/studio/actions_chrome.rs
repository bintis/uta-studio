use crate::studio::*;

pub(crate) fn completed_analysis_run_id(
    history: &[app_core::AnalysisRunHistory],
    file_hash: &str,
) -> Option<i64> {
    history
        .iter()
        .find(|run| run.file_hash == file_hash && run.status == "completed")
        .map(|run| run.id)
}

pub(crate) struct ChromeActionState<'a> {
    pub(crate) audio: &'a NativeAudio,
    pub(crate) library_audio: &'a NativeLibraryAudio,
    pub(crate) state: StudioStateMut<'a>,
    pub(crate) invalidated: &'a mut UiInvalidated,
}

pub(crate) fn apply_chrome_action(
    action: &UiAction,
    commands: &mut Commands,
    search_inputs: &LibrarySearchInputs,
    window_entity: Entity,
    _graph_viewport_width: Option<f32>,
    state: ChromeActionState,
) -> bool {
    let ChromeActionState {
        audio,
        library_audio,
        state: studio,
        invalidated,
    } = state;
    match &action.0 {
        UiCommand::App(AppCommand::ToggleGlobalSearch) => {
            studio.dialogs.search_open = !studio.dialogs.search_open;
            studio.dialogs.activity_open = false;
            studio.dialogs.about_open = false;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::App(AppCommand::ToggleActivity) => {
            studio.dialogs.activity_open = !studio.dialogs.activity_open;
            studio.dialogs.search_open = false;
            studio.dialogs.about_open = false;
            studio.analysis.analysis_tasks = app_core::load_analysis_tasks();
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::App(AppCommand::CloseActivity) => {
            studio.dialogs.activity_open = false;
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::Analysis(AnalysisCommand::SelectAnalysisHistory(id)) => {
            studio.analysis.selected_analysis_history = *id;
            studio.analysis.selected_analysis_stage = None;
            studio.analysis.selected_analysis_node = None;
            studio.analysis.analysis_graph_scroll_offset = 0.0;
            studio.analysis.analysis_graph_vertical_scroll_offset = 0.0;
            studio.analysis.analysis_graph_needs_fit = true;
            studio.analysis.analysis_graph_fit_active = true;
            studio.dialogs.activity_open = false;
            invalidated.invalidate(UiDirtyRegion::Analysis);
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::Analysis(AnalysisCommand::OpenSongAnalysis(file_hash)) => {
            // Refresh before resolving the run so a just-finished analysis can be
            // opened directly from the song page without waiting for the timer.
            studio.analysis.analysis_history = app_core::load_analysis_history(500);
            studio.analysis.selected_analysis_history =
                completed_analysis_run_id(&studio.analysis.analysis_history, file_hash);
            studio.analysis.selected_analysis_stage = None;
            studio.analysis.selected_analysis_node = None;
            studio.analysis.analysis_graph_scroll_offset = 0.0;
            studio.analysis.analysis_graph_vertical_scroll_offset = 0.0;
            studio.analysis.analysis_graph_needs_fit = true;
            studio.analysis.analysis_graph_fit_active = true;
            studio.library.library_view = LibraryView::Queue;
            studio.library.library_facet = None;
            studio.shell.route = StudioRoute::Library;
            studio.dialogs.activity_open = false;
            studio.shell.notice = studio
                .analysis
                .selected_analysis_history
                .is_none()
                .then(|| "No saved analysis session is available for this song.".to_string());
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::OpenProcessingStudio(file_hash)) => {
            studio.library.selected_song = Some(file_hash.clone());
            match app_core::load_song_workflow(file_hash) {
                Ok(workflow) => {
                    studio.analysis.workflow = Some(workflow);
                    studio.analysis.workflow_compile_error = None;
                    studio.analysis.selected_workflow_node = None;
                    studio.shell.route = StudioRoute::ProcessingStudio;
                    studio.shell.notice = None;
                }
                Err(error) => {
                    studio.shell.notice = Some(format!("Could not load workflow: {error}"));
                }
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::SelectWorkflowNode(node_id)) => {
            studio.analysis.selected_workflow_node = Some(app_core::WorkflowNodeId::new(node_id));
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::MoveWorkflowNode(node_id, earlier)) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                let result = app_core::reorder_audio_transformation(
                    &mut workflow.definition,
                    &app_core::WorkflowNodeId::new(node_id),
                    *earlier,
                );
                studio.analysis.workflow_compile_error = result.err();
                studio.shell.notice = studio
                    .analysis
                    .workflow_compile_error
                    .clone()
                    .or_else(|| Some("Workflow order changed. Save to keep it.".to_string()));
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::DuplicateWorkflowNode(node_id)) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                match app_core::duplicate_audio_transformation(
                    &mut workflow.definition,
                    &app_core::WorkflowNodeId::new(node_id),
                ) {
                    Ok(duplicate) => {
                        studio.analysis.selected_workflow_node = Some(duplicate);
                        studio.analysis.workflow_compile_error = None;
                        studio.shell.notice =
                            Some("Transformation duplicated in the audio dataflow.".to_string());
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(error);
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::CycleWorkflowPolicy(node_id)) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                let node_id = app_core::WorkflowNodeId::new(node_id);
                let current = workflow
                    .definition
                    .nodes
                    .iter()
                    .find(|node| node.instance_id == node_id)
                    .map(|node| node.execution_policy.clone());
                let next = match current {
                    Some(app_core::ExecutionPolicy::Always) => {
                        app_core::ExecutionPolicy::Conditional {
                            condition: app_core::ConditionalExecution::OnDisagreement,
                        }
                    }
                    Some(app_core::ExecutionPolicy::Conditional {
                        condition: app_core::ConditionalExecution::OnDisagreement,
                    }) => app_core::ExecutionPolicy::Conditional {
                        condition: app_core::ConditionalExecution::DisagreementWindows,
                    },
                    Some(app_core::ExecutionPolicy::Conditional { .. }) => {
                        app_core::ExecutionPolicy::Disabled
                    }
                    Some(app_core::ExecutionPolicy::Disabled) => app_core::ExecutionPolicy::Always,
                    None => app_core::ExecutionPolicy::Always,
                };
                match app_core::set_workflow_execution_policy(
                    &mut workflow.definition,
                    &node_id,
                    next,
                ) {
                    Ok(()) => {
                        studio.analysis.workflow_compile_error = None;
                        studio.shell.notice = Some("Execution policy changed.".to_string());
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(error);
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::AdjustWorkflowPriority(node_id, delta)) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                let node_id = app_core::WorkflowNodeId::new(node_id);
                let current = workflow
                    .definition
                    .nodes
                    .iter()
                    .find(|node| node.instance_id == node_id)
                    .map(|node| node.priority)
                    .unwrap_or_default();
                studio.shell.notice = match app_core::set_workflow_priority(
                    &mut workflow.definition,
                    &node_id,
                    current.saturating_add(*delta),
                ) {
                    Ok(()) => {
                        Some("Analyzer priority changed; dependencies are unchanged.".to_string())
                    }
                    Err(error) => Some(error),
                };
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::RebindWorkflowAnalyzer(
            analyzer,
            source_node,
            source_port,
        )) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                match app_core::bind_workflow_analyzer(
                    &mut workflow.definition,
                    &app_core::WorkflowNodeId::new(analyzer),
                    app_core::WorkflowPortRef {
                        node: app_core::WorkflowNodeId::new(source_node),
                        port: source_port.clone(),
                    },
                ) {
                    Ok(()) => {
                        studio.analysis.workflow_compile_error = None;
                        studio.shell.notice = Some("Analyzer input artifact changed.".to_string());
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(error);
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::PreviewWorkflow) => {
            if let Some(workflow) = studio.analysis.workflow.as_ref() {
                studio.analysis.workflow_compile_error =
                    app_core::preview_workflow_compile(&workflow.definition)
                        .err()
                        .map(|error| error.to_string());
                studio.shell.notice = Some(
                    studio
                        .analysis
                        .workflow_compile_error
                        .clone()
                        .unwrap_or_else(|| "Workflow compiles to a valid DAG.".to_string()),
                );
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::SaveWorkflow) => {
            if let (Some(file_hash), Some(workflow)) = (
                studio.library.selected_song.as_deref(),
                studio.analysis.workflow.as_ref(),
            ) {
                match app_core::save_song_workflow(
                    file_hash,
                    workflow.definition.clone(),
                    workflow.layout.clone(),
                ) {
                    Ok(saved) => {
                        studio.analysis.workflow = Some(saved);
                        studio.analysis.workflow_compile_error = None;
                        studio.shell.notice = Some("Workflow saved.".to_string());
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(format!("Workflow was not saved: {error}"));
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::RunWorkflow) => {
            if !app_core::analysis_runtime_status().ready {
                studio.shell.route = StudioRoute::Settings;
                studio.shell.settings_tab = SettingsTab::Models;
                studio.shell.notice = Some(
                    "Analysis is disabled until all selected native components are available."
                        .to_string(),
                );
            } else if let (Some(file_hash), Some(workflow)) = (
                studio.library.selected_song.as_ref(),
                studio.analysis.workflow.as_ref(),
            ) {
                match app_core::preview_workflow_compile(&workflow.definition) {
                    Ok(execution) => {
                        let targets = execution
                            .node_bindings
                            .iter()
                            .filter(|binding| {
                                binding.capability_id.as_str() == "finalize.canonical_singing_track"
                            })
                            .map(|binding| binding.analysis_node.clone())
                            .collect();
                        let request = app_core::AnalysisRequest {
                            file_hash: file_hash.clone(),
                            targets,
                            disabled_nodes: Default::default(),
                            frozen_artifacts: Default::default(),
                            bypassed_nodes: Default::default(),
                            lyrics_route: app_core::LyricsRoute::GeneratedLyrics,
                            model_availability: Default::default(),
                            profile_snapshot: app_core::AnalysisProfileSnapshot::from_app_config(
                                &studio.shell.config,
                                file_hash,
                            ),
                            active_stem_nodes: Default::default(),
                            audio_processing: None,
                            workflow_execution: Some(execution),
                        };
                        studio.shell.notice = Some(match app_core::run_analysis_request(request) {
                            Ok(()) => "Compiled workflow queued through the analysis request API."
                                .to_string(),
                            Err(error) => error,
                        });
                    }
                    Err(error) => studio.shell.notice = Some(error.to_string()),
                }
            }
            invalidated.invalidate(UiDirtyRegion::Chrome);
        }
        UiCommand::Analysis(AnalysisCommand::OpenAnalysisInspect(node_id, stage)) => {
            studio.analysis.selected_analysis_stage = Some(stage.clone());
            studio.analysis.selected_analysis_node = Some(node_id.clone());
            studio.dialogs.analysis_node_context = None;
            studio.library.library_view = LibraryView::Queue;
            studio.shell.route = StudioRoute::AnalysisInspect;
            invalidated.invalidate(action.0.dirty_region());
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::Analysis(AnalysisCommand::AdjustAnalysisGraphZoom(delta_percent)) => {
            studio.analysis.analysis_graph_zoom = clamp_analysis_graph_zoom(
                studio.analysis.analysis_graph_zoom + (*delta_percent as f32) / 100.0,
            );
            studio.analysis.analysis_graph_needs_fit = false;
            studio.analysis.analysis_graph_fit_active = false;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::ToggleAnalysisMiniView) => {
            studio.analysis.analysis_mini_view = !studio.analysis.analysis_mini_view;
            studio.analysis.analysis_lineage_mode = false;
            studio.analysis.selected_graph_edge = None;
            studio.dialogs.artifact_lineage = None;
            studio.analysis.analysis_graph_needs_fit = true;
            studio.analysis.analysis_graph_fit_active = true;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::ToggleAnalysisModelPanel) => {
            studio.analysis.analysis_model_panel_open = !studio.analysis.analysis_model_panel_open;
            studio.dialogs.open_settings_select = None;
            studio.analysis.analysis_graph_needs_fit = true;
            studio.analysis.analysis_graph_fit_active = true;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::CloseAnalysisModelPanel) => {
            studio.analysis.analysis_model_panel_open = false;
            studio.dialogs.open_settings_select = None;
            studio.analysis.analysis_graph_needs_fit = true;
            studio.analysis.analysis_graph_fit_active = true;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::SetAnalysisModelCategory(category)) => {
            studio.analysis.analysis_model_category = *category;
            studio.dialogs.open_settings_select = None;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::FitAnalysisGraph(_)) => {
            studio.analysis.analysis_graph_needs_fit = true;
            studio.analysis.analysis_graph_fit_active = true;
            studio.analysis.analysis_graph_scroll_offset = 0.0;
            studio.analysis.analysis_graph_vertical_scroll_offset = 0.0;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::FocusAnalysisGraphNode(scroll, stage_id)) => {
            studio.analysis.analysis_graph_scroll_offset = (*scroll).max(0) as f32;
            studio.analysis.selected_analysis_stage = Some(stage_id.clone());
            studio.analysis.selected_analysis_node = None;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::DismissAnalysisNodeContext) => {
            studio.dialogs.analysis_node_context = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::RequestClearAnalysisHistory) => {
            studio.dialogs.pending_analysis_history_clear = true;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CancelClearAnalysisHistory) => {
            studio.dialogs.pending_analysis_history_clear = false;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ConfirmClearAnalysisHistory) => {
            studio.dialogs.pending_analysis_history_clear = false;
            match app_core::clear_analysis_history() {
                Ok(()) => {
                    studio.analysis.analysis_history.clear();
                    studio.analysis.selected_analysis_history = None;
                    studio.analysis.selected_analysis_stage = None;
                    studio.analysis.selected_analysis_node = None;
                    studio.shell.notice = Some("Analysis history deleted.".into());
                }
                Err(error) => {
                    studio.shell.notice =
                        Some(format!("Could not delete analysis history: {error}"));
                }
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::App(AppCommand::OpenAbout) => {
            studio.dialogs.about_open = true;
            studio.dialogs.activity_open = false;
            studio.dialogs.search_open = false;
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::App(AppCommand::CloseAbout) => {
            studio.dialogs.about_open = false;
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::App(AppCommand::Back) => {
            studio.dialogs.open_settings_select = None;
            studio.dialogs.open_editor_select = None;
            if studio.shell.route == StudioRoute::Editor {
                if studio
                    .editor
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.dirty)
                {
                    studio.dialogs.pending_leave = Some(PendingLeave::Back);
                } else {
                    let _ = audio.0.stop();
                    studio.editor.editor = None;
                    studio.shell.route = StudioRoute::SongDetail;
                    studio.shell.notice = None;
                }
                invalidated.invalidate(action.0.dirty_region());
            } else if studio.shell.route == StudioRoute::ProcessingStudio {
                studio.shell.route = StudioRoute::SongDetail;
                studio.shell.notice = None;
                invalidated.invalidate(action.0.dirty_region());
            } else if studio.shell.route == StudioRoute::AnalysisInspect {
                studio.shell.route = StudioRoute::Library;
                studio.library.library_view = LibraryView::Queue;
                studio.analysis.analysis_graph_scroll_offset = 0.0;
                studio.analysis.analysis_graph_vertical_scroll_offset = 0.0;
                studio.analysis.analysis_graph_needs_fit = true;
                studio.analysis.analysis_graph_fit_active = true;
                studio.shell.notice = None;
                invalidated.invalidate(action.0.dirty_region());
            } else if studio.shell.route != StudioRoute::Library {
                studio.shell.route = StudioRoute::Library;
                studio.shell.notice = None;
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::App(AppCommand::Home) => {
            studio.dialogs.open_settings_select = None;
            studio.dialogs.open_editor_select = None;
            if studio.shell.route == StudioRoute::Editor {
                if studio
                    .editor
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.dirty)
                {
                    studio.dialogs.pending_leave = Some(PendingLeave::Home);
                    invalidated.invalidate(action.0.dirty_region());
                    return true;
                }
                let _ = audio.0.stop();
                studio.editor.editor = None;
            }
            if studio.shell.route != StudioRoute::Library {
                studio.shell.route = StudioRoute::Library;
                studio.shell.notice = None;
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::App(AppCommand::CancelLeave) => {
            studio.dialogs.pending_leave = None;
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::App(AppCommand::ConfirmLeave) => {
            let destination = studio.dialogs.pending_leave.take();
            let _ = audio.0.stop();
            match destination {
                Some(PendingLeave::Exit) => {
                    let _ = library_audio.0.stop();
                    commands.entity(window_entity).despawn();
                }
                Some(PendingLeave::Back) => {
                    studio.editor.editor = None;
                    studio.shell.route = StudioRoute::SongDetail;
                    studio.shell.notice = None;
                    invalidated.invalidate(action.0.dirty_region());
                }
                Some(PendingLeave::Home) => {
                    studio.editor.editor = None;
                    studio.shell.route = StudioRoute::Library;
                    studio.shell.notice = None;
                    invalidated.invalidate(action.0.dirty_region());
                }
                Some(PendingLeave::Documentation) => {
                    studio.editor.editor = None;
                    studio.shell.route = StudioRoute::Documentation;
                    studio.shell.notice = None;
                    invalidated.invalidate(action.0.dirty_region());
                }
                None => {}
            }
        }
        UiCommand::Library(LibraryCommand::SetLibraryView(view)) => {
            let view_changed = studio.library.library_view != *view;
            studio.library.library_view = *view;
            studio.library.library_status = None;
            studio.library.library_search = None;
            studio.library.library_facet = None;
            studio.shell.route = StudioRoute::Library;
            studio.dialogs.song_context = None;
            studio.dialogs.activity_open = false;
            studio.dialogs.about_open = false;
            studio.dialogs.search_open = false;
            studio.shell.notice = None;
            if view_changed {
                studio.library.library_scroll_offset = 0.0;
                studio.analysis.analysis_graph_scroll_offset = 0.0;
                studio.analysis.analysis_graph_vertical_scroll_offset = 0.0;
                if *view == LibraryView::Queue {
                    studio.analysis.analysis_graph_needs_fit = true;
                    studio.analysis.analysis_graph_fit_active = true;
                }
            }
            studio.library.refresh();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::SetLibraryFacet(facet)) => {
            studio.library.library_view = LibraryView::All;
            studio.library.library_search = None;
            studio.library.library_facet = Some(facet.clone());
            studio.shell.route = StudioRoute::Library;
            studio.dialogs.song_context = None;
            studio.shell.notice = None;
            studio.library.refresh();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::LoadMoreSongs) => {
            studio.library.load_more();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::ApplyLibrarySearch) => {
            let value = search_inputs
                .single()
                .map(|input| input.value().to_string())
                .unwrap_or_default();
            let value = value.trim();
            studio.library.library_search = (!value.is_empty()).then(|| value.to_string());
            studio.shell.route = StudioRoute::Library;
            studio.library.library_view = LibraryView::All;
            studio.library.library_facet = None;
            studio.dialogs.search_open = false;
            studio.library.refresh();
            studio.shell.notice = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::ClearLibrarySearch) => {
            studio.library.library_search = None;
            studio.shell.route = StudioRoute::Library;
            studio.library.refresh();
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::ToggleLibraryLayout) => {
            studio.shell.config.song_list_view = Some(
                if studio.shell.config.song_list_view.as_deref() == Some("grid") {
                    "table"
                } else {
                    "grid"
                }
                .to_string(),
            );
            if let Some(error) = save_config_error(&studio.shell.config) {
                studio.shell.notice = Some(error);
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::ToggleExportAllMenu) => {
            studio.dialogs.export_all_open = !studio.dialogs.export_all_open;
            studio.dialogs.open_library_select = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::ExportAllUtz)
        | UiCommand::Library(LibraryCommand::ExportAllUltraStar) => {
            studio.dialogs.export_all_open = false;
            let extension = if matches!(&action.0, UiCommand::Library(LibraryCommand::ExportAllUtz))
            {
                "utz"
            } else {
                "txt"
            };
            if let Some(export_directory) = studio.shell.config.export_path.clone() {
                studio.shell.notice = Some(start_export_all_job(
                    extension,
                    export_directory,
                    &mut studio.jobs.export_job,
                ));
            } else {
                studio.shell.route = StudioRoute::Settings;
                studio.shell.settings_tab = SettingsTab::Storage;
                studio.jobs.request_cache_stats_refresh = true;
                studio.shell.notice =
                    Some("Choose a default export folder before using Export all.".to_string());
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::OpenLibrarySelect(kind)) => {
            studio.dialogs.open_library_select =
                if studio.dialogs.open_library_select == Some(*kind) {
                    None
                } else {
                    Some(*kind)
                };
            studio.dialogs.export_all_open = false;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::SelectLibraryValue(kind, value)) => {
            let value = (value != "all").then(|| value.clone());
            match kind {
                LibrarySelectKind::Status => studio.library.library_status = value,
                LibrarySelectKind::TranscriptSource => {
                    studio.library.library_transcript_source = value
                }
            }
            studio.dialogs.open_library_select = None;
            studio.library.refresh();
            studio.shell.notice = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Library(LibraryCommand::AnalyzeAll) => {
            if app_core::analysis_runtime_status().ready {
                app_core::enqueue_all(&studio.library.filters());
                studio.analysis.analysis_tasks = app_core::load_analysis_tasks();
                studio.library.library_view = LibraryView::Queue;
                studio.library.library_facet = None;
                studio.shell.route = StudioRoute::Library;
                studio.analysis.analysis_graph_scroll_offset = 0.0;
                studio.analysis.analysis_graph_vertical_scroll_offset = 0.0;
                studio.analysis.analysis_graph_needs_fit = true;
                studio.analysis.analysis_graph_fit_active = true;
                studio.library.refresh();
                studio.shell.notice = Some("Matching unanalyzed songs were queued.".to_string());
            } else {
                studio.shell.route = StudioRoute::Settings;
                studio.shell.settings_tab = SettingsTab::Models;
                studio.shell.notice = Some(
                    "Analysis is disabled until setup is completed in Settings > Models & runtime."
                        .to_string(),
                );
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::App(AppCommand::Folders) => {
            studio.shell.route = StudioRoute::Folders;
            studio.shell.notice = None;
            studio.library.folder_browser.context_menu = None;
            if studio.library.folder_browser.root.is_none()
                && let Some(path) = studio.shell.config.library_paths().into_iter().next()
            {
                studio.library.folder_browser.select_root(path);
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::DismissAnalysisArtifactContext) => {
            studio.dialogs.analysis_artifact_context = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::DismissAnalysisExportContext) => {
            studio.dialogs.analysis_export_context = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ToggleAnalysisLineageMode) => {
            if !studio.analysis.analysis_mini_view {
                studio.analysis.analysis_lineage_mode = false;
                studio.dialogs.artifact_lineage = None;
            } else {
                studio.analysis.analysis_lineage_mode = !studio.analysis.analysis_lineage_mode;
                if !studio.analysis.analysis_lineage_mode {
                    studio.dialogs.artifact_lineage = None;
                }
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ValidateExportNode(file_hash, kind)) => {
            studio.dialogs.analysis_export_context = None;
            studio.shell.notice = Some(match app_core::validate_export_node(file_hash, *kind) {
                Ok(message) => message,
                Err(error) => error,
            });
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::RevealLastExport(file_hash, kind)) => {
            studio.dialogs.analysis_export_context = None;
            studio.shell.notice = Some(match app_core::last_export_destination(file_hash, *kind) {
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
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::App(AppCommand::Documentation) => {
            let origin = studio.shell.route;
            if origin != StudioRoute::Documentation {
                studio.shell.documentation.return_route = Some(origin);
                studio.shell.documentation.back_stack.clear();
                studio.shell.documentation.forward_stack.clear();
                studio.shell.documentation.anchor = None;
            }
            studio
                .shell
                .documentation
                .navigate(Some("guide:getting-started".to_string()));
            if studio.shell.route == StudioRoute::Editor
                && studio
                    .editor
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.dirty)
            {
                studio.dialogs.pending_leave = Some(PendingLeave::Documentation);
            } else {
                studio.shell.route = StudioRoute::Documentation;
                studio.shell.notice = None;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::App(AppCommand::OpenDocumentation(anchor)) => {
            let origin = studio.shell.route;
            if origin != StudioRoute::Documentation {
                studio.shell.documentation.return_route = Some(origin);
                studio.shell.documentation.back_stack.clear();
                studio.shell.documentation.forward_stack.clear();
                studio.shell.documentation.anchor = None;
            }
            studio.shell.documentation.navigate(anchor.clone());
            if studio.shell.route == StudioRoute::Editor
                && studio
                    .editor
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.dirty)
            {
                studio.dialogs.pending_leave = Some(PendingLeave::Documentation);
            } else {
                studio.shell.route = StudioRoute::Documentation;
                studio.shell.notice = None;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::App(AppCommand::DocumentationBack) => {
            if !studio.shell.documentation.go_back() {
                studio.shell.route = studio
                    .shell
                    .documentation
                    .return_route
                    .take()
                    .unwrap_or(StudioRoute::Library);
                studio.shell.documentation.forward_stack.clear();
            }
            studio.shell.notice = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::App(AppCommand::DocumentationForward) => {
            if studio.shell.documentation.go_forward() {
                invalidated.invalidate(action.0.dirty_region());
            }
        }
        UiCommand::Analysis(AnalysisCommand::SelectArtifactInspectorTab(tab)) => {
            studio.analysis.selected_artifact_inspector_tab = *tab;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ToggleArtifactPinned(reference)) => {
            match app_core::inspect_artifact(reference) {
                Ok(inspection) => {
                    let target = !inspection.pinned;
                    studio.shell.notice = match app_core::set_artifact_pinned(reference, target) {
                        Ok(()) => Some(if target {
                            "Artifact revision pinned. It is protected from deletion.".to_string()
                        } else {
                            "Artifact revision unpinned.".to_string()
                        }),
                        Err(error) => Some(error),
                    };
                }
                Err(error) => studio.shell.notice = Some(error),
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ShowArtifactLineage(reference)) => {
            match app_core::artifact_lineage(reference) {
                Ok(lineage) => {
                    studio.analysis.analysis_lineage_mode = true;
                    studio.dialogs.artifact_lineage = Some(ArtifactLineagePanel {
                        lineage,
                        scope: studio.analysis.analysis_lineage_scope,
                        selected: reference.clone(),
                    });
                }
                Err(error) => studio.shell.notice = Some(error),
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::SetArtifactLineageScope(scope)) => {
            studio.analysis.analysis_lineage_scope = *scope;
            if let Some(panel) = studio.dialogs.artifact_lineage.as_mut() {
                panel.scope = *scope;
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::SelectArtifactLineageRevision(reference)) => {
            if let Some(panel) = studio.dialogs.artifact_lineage.as_mut() {
                panel.selected = reference.clone();
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CloseArtifactLineage) => {
            studio.dialogs.artifact_lineage = None;
            studio.analysis.analysis_lineage_mode = false;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ShowArtifactImpact(reference)) => {
            match app_core::preview_artifact_downstream_impact(reference) {
                Ok(impact) => studio.dialogs.artifact_impact = Some(impact),
                Err(error) => studio.shell.notice = Some(error),
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::CloseArtifactImpact) => {
            studio.dialogs.artifact_impact = None;
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::ConfirmArtifactImpact) => {
            if let Some(impact) = studio.dialogs.artifact_impact.take() {
                let request = app_core::analysis_request_from_impact(&impact.file_hash, &impact);
                studio.shell.notice = Some(match app_core::run_analysis_request(request) {
                    Ok(()) => {
                        studio.analysis.analysis_tasks = app_core::load_analysis_tasks();
                        "Confirmed impact plan was queued.".to_string()
                    }
                    Err(error) => error,
                });
            }
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::OpenArtifactCompatibleEditor(reference)) => {
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
                            studio.library.selected_song = Some(reference.file_hash.clone());
                            studio.shell.route = StudioRoute::SongDetail;
                            studio.dialogs.lyrics_editor = Some(NativeLyricsEditor {
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
                                    &mut studio.jobs.lyrics_waveform_job,
                                );
                            }
                            studio.shell.notice = Some(
                                    "Opened an immutable artifact revision as an editable working copy."
                                        .to_string(),
                                );
                        }
                        Err(error) => studio.shell.notice = Some(error),
                    }
                }
                app_core::ArtifactKind::CandidateChart
                | app_core::ArtifactKind::AuthoredChart
                | app_core::ArtifactKind::PitchTrack
                | app_core::ArtifactKind::PitchNoteCandidates => {
                    studio.library.selected_song = Some(reference.file_hash.clone());
                    studio.editor.editor = None;
                    studio.shell.route = StudioRoute::Editor;
                    studio.shell.notice = Some(start_editor_revision_load_job(
                        reference.clone(),
                        Arc::clone(&audio.0),
                        &mut studio.jobs.editor_load_job,
                    ));
                }
                _ => {
                    studio.shell.notice = Some(
                        "No compatible in-app editor exists for this artifact kind.".to_string(),
                    );
                }
            }
            invalidated.invalidate(if studio.shell.route == StudioRoute::Editor {
                UiDirtyRegion::Editor
            } else {
                action.0.dirty_region()
            });
        }
        UiCommand::Analysis(AnalysisCommand::MergeCandidateChart(candidate, authored, mode)) => {
            studio.library.selected_song = Some(candidate.file_hash.clone());
            studio.editor.editor = None;
            studio.shell.route = StudioRoute::Editor;
            studio.shell.notice = Some(start_editor_merge_load_job(
                candidate.clone(),
                authored.clone(),
                *mode,
                Arc::clone(&audio.0),
                &mut studio.jobs.editor_load_job,
            ));
            invalidated.invalidate(UiDirtyRegion::Editor);
        }
        UiCommand::Analysis(AnalysisCommand::MergeSelectedCandidatePhrase(candidate, authored)) => {
            studio.dialogs.analysis_artifact_context = None;
            match merge_mode_from_editor_selection(studio.editor.editor.as_ref(), true) {
                Ok(mode) => {
                    studio.library.selected_song = Some(candidate.file_hash.clone());
                    studio.editor.editor = None;
                    studio.shell.route = StudioRoute::Editor;
                    studio.shell.notice = Some(start_editor_merge_load_job(
                        candidate.clone(),
                        authored.clone(),
                        mode,
                        Arc::clone(&audio.0),
                        &mut studio.jobs.editor_load_job,
                    ));
                }
                Err(error) => studio.shell.notice = Some(error),
            }
            invalidated.invalidate(if studio.shell.route == StudioRoute::Editor {
                UiDirtyRegion::Editor
            } else {
                action.0.dirty_region()
            });
        }
        UiCommand::Analysis(AnalysisCommand::MergeSelectedCandidateRange(candidate, authored)) => {
            studio.dialogs.analysis_artifact_context = None;
            match merge_mode_from_editor_selection(studio.editor.editor.as_ref(), false) {
                Ok(mode) => {
                    studio.library.selected_song = Some(candidate.file_hash.clone());
                    studio.editor.editor = None;
                    studio.shell.route = StudioRoute::Editor;
                    studio.shell.notice = Some(start_editor_merge_load_job(
                        candidate.clone(),
                        authored.clone(),
                        mode,
                        Arc::clone(&audio.0),
                        &mut studio.jobs.editor_load_job,
                    ));
                }
                Err(error) => studio.shell.notice = Some(error),
            }
            invalidated.invalidate(if studio.shell.route == StudioRoute::Editor {
                UiDirtyRegion::Editor
            } else {
                action.0.dirty_region()
            });
        }
        UiCommand::Analysis(AnalysisCommand::KeepAuthoredChart) => {
            studio.dialogs.analysis_artifact_context = None;
            studio.dialogs.pending_chart_replace = None;
            studio.shell.notice =
                Some("Authored chart kept. The candidate revision was not applied.".to_string());
            invalidated.invalidate(action.0.dirty_region());
        }

        _ => return false,
    }
    true
}
