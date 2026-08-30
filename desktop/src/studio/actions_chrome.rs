use crate::studio::*;

fn pending_editor_leave(
    route: StudioRoute,
    editor_dirty: bool,
    destination: PendingLeave,
) -> Option<PendingLeave> {
    (route == StudioRoute::Editor && editor_dirty).then_some(destination)
}

fn refresh_workflow_snapshot(analysis: &mut AnalysisUiState) {
    let result = analysis
        .workflow
        .as_ref()
        .ok_or_else(|| "workflow is unavailable".to_string())
        .and_then(|workflow| {
            app_core::preview_workflow_compile(&workflow.definition)
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(snapshot) => {
            analysis.workflow_snapshot = Some(snapshot);
            analysis.workflow_compile_error = None;
        }
        Err(error) => {
            analysis.workflow_snapshot = None;
            analysis.workflow_compile_error = Some(error);
        }
    }
}

pub(crate) fn completed_analysis_run_id(
    history: &[app_core::AnalysisRunHistory],
    file_hash: &str,
) -> Option<i64> {
    history
        .iter()
        .find(|run| run.file_hash == file_hash && run.status == "completed")
        .map(|run| run.id)
}

fn open_song_analysis(studio: &mut StudioStateMut<'_>, file_hash: &str) {
    // Advanced Graph opens on the current compiled workflow. A frozen
    // historical plan becomes authoritative only after the user explicitly
    // selects that run from the history strip.
    studio.library.selected_song = Some(file_hash.to_string());
    studio.analysis.analysis_history = app_core::load_analysis_history(500);
    studio.analysis.selected_analysis_history = None;
    let expected_workflow_id = format!("song:{file_hash}:workflow");
    let current_matches = studio
        .analysis
        .workflow
        .as_ref()
        .is_some_and(|workflow| workflow.definition.workflow_id.0 == expected_workflow_id);
    if !current_matches {
        match app_core::load_song_workflow(file_hash) {
            Ok(workflow) => studio.analysis.workflow = Some(workflow),
            Err(error) => studio.shell.notice = Some(format!("Could not load workflow: {error}")),
        }
    }
    refresh_workflow_snapshot(studio.analysis);
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
        .workflow_compile_error
        .as_ref()
        .map(|error| format!("Workflow graph unavailable: {error}"))
        .or_else(|| Some("Showing the current compiled workflow.".to_string()));
}

fn open_song_model_selection(studio: &mut StudioStateMut<'_>, file_hash: &str) {
    studio.library.selected_song = Some(file_hash.to_string());
    studio.analysis.analysis_history = app_core::load_analysis_history(500);
    studio.analysis.selected_analysis_history =
        completed_analysis_run_id(&studio.analysis.analysis_history, file_hash);
    studio.analysis.selected_analysis_node = None;
    studio.analysis.analysis_graph_scroll_offset = 0.0;
    studio.analysis.analysis_graph_vertical_scroll_offset = 0.0;
    studio.analysis.analysis_graph_needs_fit = true;
    studio.analysis.analysis_graph_fit_active = true;
    studio.analysis.analysis_model_panel_open = true;
    if studio.jobs.model_settings_job.current.is_none() {
        studio.jobs.request_model_settings_refresh = true;
    }
    studio.library.library_view = LibraryView::Queue;
    studio.library.library_facet = None;
    studio.shell.route = StudioRoute::Library;
    studio.dialogs.open_settings_select = None;
    studio.dialogs.activity_open = false;
    studio.shell.notice = None;
}

fn open_processing_studio(studio: &mut StudioStateMut<'_>, file_hash: Option<&str>) {
    studio.jobs.request_model_settings_refresh = true;
    let Some(file_hash) = file_hash else {
        studio.library.selected_song = None;
        studio.analysis.workflow = None;
        studio.analysis.workflow_snapshot = None;
        studio.analysis.workflow_compile_error = None;
        studio.analysis.selected_workflow_node = None;
        studio.analysis.processing_studio_scroll_offset = 0.0;
        studio.shell.route = StudioRoute::ProcessingStudio;
        studio.shell.notice = None;
        return;
    };
    studio.library.selected_song = Some(file_hash.to_string());
    match app_core::load_song_workflow(file_hash) {
        Ok(workflow) => {
            studio.analysis.workflow = Some(workflow);
            refresh_workflow_snapshot(studio.analysis);
            studio.analysis.selected_workflow_node = None;
            studio.analysis.processing_studio_scroll_offset = 0.0;
            studio.shell.route = StudioRoute::ProcessingStudio;
            studio.shell.notice = None;
        }
        Err(error) => {
            studio.shell.notice = Some(format!("Could not load workflow: {error}"));
        }
    }
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
        state: mut studio,
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
            if let Some(pending) = pending_editor_leave(
                studio.shell.route,
                studio
                    .editor
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.dirty),
                PendingLeave::OpenSongAnalysis(file_hash.clone()),
            ) {
                studio.dialogs.pending_leave = Some(pending);
                invalidated.invalidate(UiDirtyRegion::Dialog);
                return true;
            }
            if studio.shell.route == StudioRoute::Editor {
                let _ = audio.0.stop();
                studio.editor.editor = None;
            }
            open_song_analysis(&mut studio, file_hash);
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::OpenSongModelSelection(file_hash)) => {
            if let Some(pending) = pending_editor_leave(
                studio.shell.route,
                studio
                    .editor
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.dirty),
                PendingLeave::OpenSongModelSelection(file_hash.clone()),
            ) {
                studio.dialogs.pending_leave = Some(pending);
                invalidated.invalidate(UiDirtyRegion::Dialog);
                return true;
            }
            if studio.shell.route == StudioRoute::Editor {
                let _ = audio.0.stop();
                studio.editor.editor = None;
            }
            open_song_model_selection(&mut studio, file_hash);
            invalidated.invalidate(action.0.dirty_region());
            invalidated.invalidate(UiDirtyRegion::Analysis);
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::Analysis(AnalysisCommand::OpenProcessingStudio(file_hash)) => {
            if let Some(pending) = pending_editor_leave(
                studio.shell.route,
                studio
                    .editor
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.dirty),
                PendingLeave::OpenProcessingStudio(Some(file_hash.clone())),
            ) {
                studio.dialogs.pending_leave = Some(pending);
                invalidated.invalidate(UiDirtyRegion::Dialog);
                return true;
            }
            if studio.shell.route == StudioRoute::Editor {
                let _ = audio.0.stop();
                studio.editor.editor = None;
            }
            open_processing_studio(&mut studio, Some(file_hash));
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::OpenEmptyProcessingStudio) => {
            if let Some(pending) = pending_editor_leave(
                studio.shell.route,
                studio
                    .editor
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.dirty),
                PendingLeave::OpenProcessingStudio(None),
            ) {
                studio.dialogs.pending_leave = Some(pending);
                invalidated.invalidate(UiDirtyRegion::Dialog);
                return true;
            }
            if studio.shell.route == StudioRoute::Editor {
                let _ = audio.0.stop();
                studio.editor.editor = None;
            }
            open_processing_studio(&mut studio, None);
            invalidated.invalidate(action.0.dirty_region());
        }
        UiCommand::Analysis(AnalysisCommand::SelectWorkflowNode(node_id)) => {
            let node_id = app_core::WorkflowNodeId::new(node_id);
            studio.analysis.selected_workflow_node =
                (studio.analysis.selected_workflow_node.as_ref() != Some(&node_id))
                    .then_some(node_id);
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
                if studio.analysis.workflow_compile_error.is_none() {
                    refresh_workflow_snapshot(studio.analysis);
                }
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
                        refresh_workflow_snapshot(studio.analysis);
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
        UiCommand::Analysis(AnalysisCommand::RemoveWorkflowNode(node_id)) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                match app_core::remove_workflow_node(
                    &mut workflow.definition,
                    &app_core::WorkflowNodeId::new(node_id.clone()),
                ) {
                    Ok(()) => {
                        studio.analysis.selected_workflow_node = None;
                        refresh_workflow_snapshot(studio.analysis);
                        studio.shell.notice = Some(
                            "Workflow card deleted. Save Workflow to keep this change.".to_string(),
                        );
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(error);
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::SetWorkflowNodeModel(node_id, model_id)) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                match app_core::set_workflow_node_model(
                    &mut workflow.definition,
                    &app_core::WorkflowNodeId::new(node_id),
                    model_id,
                ) {
                    Ok(()) => {
                        refresh_workflow_snapshot(studio.analysis);
                        studio.shell.notice = Some(format!(
                            "Model changed to {model_id}. Save Workflow to keep this change."
                        ));
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(error);
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::SetWorkflowSeparationStrategy(node_id, strategy)) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                match app_core::set_workflow_separation_strategy(
                    &mut workflow.definition,
                    &app_core::WorkflowNodeId::new(node_id),
                    *strategy,
                ) {
                    Ok(()) => {
                        studio.analysis.workflow_compile_error = None;
                        refresh_workflow_snapshot(studio.analysis);
                        studio.shell.notice = Some(
                            "Separation strategy changed. Save Workflow to keep this change."
                                .to_string(),
                        );
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(error);
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::AddWorkflowProcessor(
            source_node,
            source_port,
            capability_id,
            model_id,
        )) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                match app_core::insert_audio_transformation_after_output(
                    &mut workflow.definition,
                    &app_core::WorkflowNodeId::new(source_node),
                    source_port,
                    &app_core::CapabilityId::new(capability_id),
                    model_id.clone(),
                ) {
                    Ok(inserted) => {
                        studio.analysis.selected_workflow_node = Some(inserted);
                        studio.analysis.workflow_compile_error = None;
                        refresh_workflow_snapshot(studio.analysis);
                        studio.shell.notice = Some(
                            "Processor added to the real audio dataflow. Save to keep it."
                                .to_string(),
                        );
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(error);
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::AddOptionalWorkflowCard(
            source_node,
            source_port,
            card,
        )) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                match app_core::add_optional_workflow_card(
                    &mut workflow.definition,
                    app_core::WorkflowPortRef {
                        node: app_core::WorkflowNodeId::new(source_node),
                        port: source_port.clone(),
                    },
                    *card,
                ) {
                    Ok(inserted) => {
                        studio.analysis.selected_workflow_node = Some(inserted);
                        studio.analysis.workflow_compile_error = None;
                        refresh_workflow_snapshot(studio.analysis);
                        studio.shell.notice = Some(format!(
                            "{} added and wired to the analysis input. Save to keep it.",
                            card.label()
                        ));
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(error);
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::SetWorkflowParameter(node_id, key, value)) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                match app_core::set_workflow_parameter(
                    &mut workflow.definition,
                    &app_core::WorkflowNodeId::new(node_id),
                    key.clone(),
                    value.clone(),
                ) {
                    Ok(()) => {
                        studio.analysis.workflow_compile_error = None;
                        refresh_workflow_snapshot(studio.analysis);
                        studio.shell.notice =
                            Some("Workflow preference changed. Save to keep it.".to_string());
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(error);
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::SetWorkflowPolicy(node_id, policy)) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                match app_core::set_workflow_execution_policy(
                    &mut workflow.definition,
                    &app_core::WorkflowNodeId::new(node_id),
                    policy.clone(),
                ) {
                    Ok(()) => {
                        refresh_workflow_snapshot(studio.analysis);
                        studio.shell.notice = Some(
                            "Execution condition changed; priority and dependencies are unchanged."
                                .to_string(),
                        );
                    }
                    Err(error) => {
                        studio.analysis.workflow_compile_error = Some(error.clone());
                        studio.shell.notice = Some(error);
                    }
                }
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::SetWorkflowSkipIfUnchanged(node_id, skip_if_unchanged)) => {
            if let Some(workflow) = studio.analysis.workflow.as_mut() {
                match app_core::set_workflow_skip_if_unchanged(
                    &mut workflow.definition,
                    &app_core::WorkflowNodeId::new(node_id),
                    *skip_if_unchanged,
                ) {
                    Ok(()) => {
                        refresh_workflow_snapshot(studio.analysis);
                        studio.shell.notice = Some(if *skip_if_unchanged {
                            "This step will be skipped and its last successful result reused if nothing about it changed.".to_string()
                        } else {
                            "This step will always recompute on the next run.".to_string()
                        });
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
                        refresh_workflow_snapshot(studio.analysis);
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
                        refresh_workflow_snapshot(studio.analysis);
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
            refresh_workflow_snapshot(studio.analysis);
            studio.shell.notice = Some(
                studio
                    .analysis
                    .workflow_compile_error
                    .clone()
                    .unwrap_or_else(|| "Workflow compiles to a valid DAG.".to_string()),
            );
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
                        refresh_workflow_snapshot(studio.analysis);
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
            let file_hash = studio.library.selected_song.clone();
            let workflow = studio
                .analysis
                .workflow
                .as_ref()
                .map(|workflow| (workflow.definition.clone(), workflow.layout.clone()));
            match (file_hash, workflow) {
                (Some(file_hash), Some((definition, layout))) => {
                    let persisted =
                        app_core::load_song_workflow(&file_hash)
                            .ok()
                            .filter(|stored| {
                                stored.definition == definition && stored.layout == layout
                            });
                    let saved = if let Some(persisted) = persisted {
                        Ok(persisted)
                    } else {
                        app_core::save_song_workflow(&file_hash, definition, layout)
                    };
                    match saved {
                        Ok(saved) => {
                            let revision = saved.definition.revision;
                            studio.analysis.workflow = Some(saved);
                            refresh_workflow_snapshot(studio.analysis);
                            let mut draft = PlanPreviewDraft {
                                file_hash: file_hash.clone(),
                                outputs: app_core::AnalysisOutputSelection::from_target(
                                    studio.shell.config.analysis_default_target(),
                                ),
                                outputs_overridden: false,
                                run_override: app_core::AnalysisExperienceOverride::default(),
                                effective_settings: None,
                                engine_preview: Err(
                                    "Preview has not been compiled yet.".to_string()
                                ),
                            };
                            rebuild_engine_plan_preview(&mut draft, &studio.shell.config);
                            let ready = draft
                                .engine_preview
                                .as_ref()
                                .is_ok_and(|preview| preview.ready);
                            bevy::log::info!(
                                target: "uta_studio::workflow",
                                file_hash = %file_hash,
                                revision,
                                ready,
                                "Processing Studio opened exact Engine plan preview"
                            );
                            studio.dialogs.plan_preview_draft = Some(draft);
                            studio.shell.notice = None;
                            invalidated.invalidate(UiDirtyRegion::Dialog);
                        }
                        Err(error) => {
                            bevy::log::error!(
                                target: "uta_studio::workflow",
                                file_hash = %file_hash,
                                error = %error,
                                "Processing Studio workflow could not be saved for execution"
                            );
                            studio.analysis.workflow_compile_error = Some(error.clone());
                            studio.shell.notice = Some(format!(
                                "Workflow was not run because it could not be saved: {error}"
                            ));
                            invalidated.invalidate(UiDirtyRegion::Chrome);
                            invalidated.invalidate(UiDirtyRegion::Analysis);
                        }
                    }
                }
                (None, _) => {
                    studio.shell.notice =
                        Some("Choose a song before running its workflow.".to_string());
                    invalidated.invalidate(UiDirtyRegion::Chrome);
                }
                (_, None) => {
                    studio.shell.notice = Some(
                        "Workflow is unavailable. Return to the song and reopen Processing Studio."
                            .to_string(),
                    );
                    invalidated.invalidate(UiDirtyRegion::Chrome);
                }
            }
        }
        UiCommand::Analysis(AnalysisCommand::OpenAnalysisInspect(node_id, _capability)) => {
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
            studio.analysis.analysis_graph_needs_fit = true;
            studio.analysis.analysis_graph_fit_active = true;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Analysis(AnalysisCommand::ToggleAnalysisModelPanel) => {
            studio.analysis.analysis_model_panel_open = !studio.analysis.analysis_model_panel_open;
            if studio.analysis.analysis_model_panel_open
                && studio.jobs.model_settings_job.current.is_none()
            {
                studio.jobs.request_model_settings_refresh = true;
            }
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
        UiCommand::Analysis(AnalysisCommand::FitAnalysisGraph(_)) => {
            studio.analysis.analysis_graph_needs_fit = true;
            studio.analysis.analysis_graph_fit_active = true;
            studio.analysis.analysis_graph_scroll_offset = 0.0;
            studio.analysis.analysis_graph_vertical_scroll_offset = 0.0;
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
            } else if studio.library.library_view == LibraryView::Queue {
                if studio.library.selected_song.is_some() {
                    studio.shell.route = StudioRoute::SongDetail;
                } else {
                    studio.library.library_view = LibraryView::All;
                    studio.library.refresh();
                }
                studio.library.library_facet = None;
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
                Some(PendingLeave::Library(view)) => {
                    studio.editor.editor = None;
                    let view_changed = studio.library.library_view != view;
                    studio.library.library_view = view;
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
                        if view == LibraryView::Queue {
                            studio.analysis.analysis_graph_needs_fit = true;
                            studio.analysis.analysis_graph_fit_active = true;
                        }
                    }
                    studio.library.refresh();
                    invalidated.invalidate(action.0.dirty_region());
                }
                Some(PendingLeave::OpenSongAnalysis(file_hash)) => {
                    studio.editor.editor = None;
                    open_song_analysis(&mut studio, &file_hash);
                    invalidated.invalidate(UiDirtyRegion::Chrome);
                    invalidated.invalidate(UiDirtyRegion::Analysis);
                }
                Some(PendingLeave::OpenSongModelSelection(file_hash)) => {
                    studio.editor.editor = None;
                    open_song_model_selection(&mut studio, &file_hash);
                    invalidated.invalidate(UiDirtyRegion::Chrome);
                    invalidated.invalidate(UiDirtyRegion::Analysis);
                }
                Some(PendingLeave::OpenProcessingStudio(file_hash)) => {
                    studio.editor.editor = None;
                    open_processing_studio(&mut studio, file_hash.as_deref());
                    invalidated.invalidate(UiDirtyRegion::Chrome);
                    invalidated.invalidate(UiDirtyRegion::Analysis);
                }
                None => {}
            }
        }
        UiCommand::Library(LibraryCommand::SetLibraryView(view)) => {
            if let Some(pending_leave) = pending_editor_leave(
                studio.shell.route,
                studio
                    .editor
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.dirty),
                PendingLeave::Library(*view),
            ) {
                studio.dialogs.pending_leave = Some(pending_leave);
                invalidated.invalidate(action.0.dirty_region());
                return true;
            }
            if studio.shell.route == StudioRoute::Editor {
                let _ = audio.0.stop();
                studio.editor.editor = None;
            }
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
            let queued = app_core::enqueue_all(&studio.library.filters());
            studio.analysis.analysis_tasks = app_core::load_analysis_tasks();
            studio.library.library_view = LibraryView::Queue;
            studio.library.library_facet = None;
            studio.shell.route = StudioRoute::Library;
            studio.analysis.analysis_graph_scroll_offset = 0.0;
            studio.analysis.analysis_graph_vertical_scroll_offset = 0.0;
            studio.analysis.analysis_graph_needs_fit = true;
            studio.analysis.analysis_graph_fit_active = true;
            studio.library.refresh();
            studio.shell.notice = Some(format!(
                "Queued {} exact Engine request(s); {} request(s) were blocked during Plan Preview.",
                queued.queued, queued.blocked
            ));
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
        UiCommand::Analysis(AnalysisCommand::MergeSelectedCandidatePhrase(candidate, authored)) => {
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
            studio.dialogs.pending_chart_replace = None;
            studio.shell.notice =
                Some("Authored chart kept. The candidate revision was not applied.".to_string());
            invalidated.invalidate(action.0.dirty_region());
        }

        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_editor_requires_confirmation_before_library_navigation() {
        assert_eq!(
            pending_editor_leave(
                StudioRoute::Editor,
                true,
                PendingLeave::Library(LibraryView::Completed)
            ),
            Some(PendingLeave::Library(LibraryView::Completed))
        );
        assert_eq!(
            pending_editor_leave(
                StudioRoute::Editor,
                false,
                PendingLeave::Library(LibraryView::Completed)
            ),
            None
        );
        assert_eq!(
            pending_editor_leave(
                StudioRoute::Library,
                true,
                PendingLeave::Library(LibraryView::Completed)
            ),
            None
        );
        assert_eq!(
            pending_editor_leave(
                StudioRoute::Editor,
                true,
                PendingLeave::OpenProcessingStudio(Some("song".to_string()))
            ),
            Some(PendingLeave::OpenProcessingStudio(Some("song".to_string())))
        );
    }
}
