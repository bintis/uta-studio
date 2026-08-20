//! Native DAG Node I/O / Artifact Workbench rendering helpers.

use crate::studio::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ArtifactInspectorTab {
    #[default]
    Overview,
    Inputs,
    Outputs,
    Attempts,
    Logs,
    Help,
}

pub(crate) fn artifact_ref_from_revision(
    revision: &app_core::ArtifactRevision,
) -> app_core::ArtifactRef {
    app_core::ArtifactRef {
        file_hash: revision.file_hash.clone(),
        kind: revision.kind,
        revision_id: revision.id.clone(),
    }
}

pub(crate) fn merge_mode_from_editor_selection(
    editor: Option<&NativeEditor>,
    phrase: bool,
) -> Result<app_core::ChartRevisionMergeMode, String> {
    let Some(editor) = editor else {
        return Err(if phrase {
            "Select a phrase in the chart editor first.".to_string()
        } else {
            "Select notes in the chart editor first.".to_string()
        });
    };
    if editor.dirty {
        return Err(
            "Save the authored chart before merging a candidate into the current selection.".into(),
        );
    }
    let indices = editor.selected_note_indices();
    if indices.is_empty() {
        return Err(if phrase {
            "Select a phrase in the chart editor first.".to_string()
        } else {
            "Select notes in the chart editor first.".to_string()
        });
    }
    let track = editor.document.active_track_index();
    if phrase {
        let phrase = indices
            .iter()
            .next()
            .and_then(|index| editor.document.phrase_index_for_note(*index))
            .ok_or_else(|| "Select a phrase in the chart editor first.".to_string())?;
        Ok(app_core::ChartRevisionMergeMode::ReplacePhrase { track, phrase })
    } else {
        let (start, end) = editor
            .document
            .note_range_units(&indices)
            .ok_or_else(|| "Select notes in the chart editor first.".to_string())?;
        Ok(app_core::ChartRevisionMergeMode::ReplaceNoteRange { track, start, end })
    }
}

fn binding_state_copy(state: app_core::ArtifactBindingState) -> &'static str {
    match state {
        app_core::ArtifactBindingState::Resolved => "Resolved",
        app_core::ArtifactBindingState::Source => "Read-only source",
        app_core::ArtifactBindingState::Ephemeral => "Ephemeral",
        app_core::ArtifactBindingState::FrozenReuse => "Frozen reuse",
        app_core::ArtifactBindingState::Bypassed => "Bypassed",
        app_core::ArtifactBindingState::Missing => "Missing",
        app_core::ArtifactBindingState::LegacyUntracked => "Legacy / untracked",
        app_core::ArtifactBindingState::Invalidated => "Invalidated",
        app_core::ArtifactBindingState::NotApplicable => "Not applicable",
    }
}

fn spawn_binding_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    binding: &app_core::ArtifactBinding,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(9)),
                row_gap: px(3),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.30)),
            BorderColor::all(theme.border.with_alpha(0.38)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(8),
                ..default()
            })
            .with_children(|header| {
                spawn_text(
                    header,
                    font.clone(),
                    format!("{:?}", binding.kind),
                    9.0,
                    theme.foreground,
                );
                header.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                spawn_text(
                    header,
                    font.clone(),
                    binding_state_copy(binding.state),
                    8.0,
                    match binding.state {
                        app_core::ArtifactBindingState::Missing
                        | app_core::ArtifactBindingState::Invalidated => theme.destructive,
                        app_core::ArtifactBindingState::LegacyUntracked
                        | app_core::ArtifactBindingState::Ephemeral => theme.editor_warning,
                        _ => theme.primary,
                    },
                );
            });
            spawn_bounded_wrapped_text(
                row,
                font.clone(),
                binding.display_name.clone(),
                9.0,
                theme.muted_foreground,
            );
            let mut detail = Vec::new();
            if let Some(size) = binding.byte_size {
                detail.push(format!("{:.1} KiB", size as f64 / 1024.0));
            }
            if let Some(hash) = binding.content_hash.as_deref() {
                detail.push(format!("content {}", &hash[..hash.len().min(12)]));
            }
            if binding.active {
                detail.push("Active".to_string());
            }
            if binding.pinned {
                detail.push("Pinned".to_string());
            }
            if !detail.is_empty() {
                spawn_text(
                    row,
                    font.clone(),
                    detail.join(" · "),
                    8.0,
                    theme.muted_foreground,
                );
            }
            if let Some(explanation) = binding.explanation.as_deref() {
                spawn_wrapped_text(row, font, explanation, 8.0, theme.muted_foreground);
            }
        });
}

pub(crate) fn spawn_node_io_workbench(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    file_hash: &str,
    node_id: &str,
    run_id: Option<i64>,
    selected_tab: ArtifactInspectorTab,
) {
    let inspection = app_core::inspect_analysis_node_io(file_hash, node_id, run_id);
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(12)),
                row_gap: px(9),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.25)),
            BorderColor::all(theme.border.with_alpha(0.4)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|heading| {
                    spawn_text(
                        heading,
                        font.clone(),
                        "NODE I/O WORKBENCH",
                        8.0,
                        theme.primary,
                    );
                    heading.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_text_button(
                        heading,
                        font.clone(),
                        theme,
                        "Help",
                        8.0,
                        UiAction::from(AppCommand::OpenDocumentation(Some(
                            documentation_anchor_for_node(node_id).to_string(),
                        ))),
                    );
                });

            match inspection {
                Ok(inspection) => {
                    panel
                        .spawn(Node {
                            width: percent(100),
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(4),
                            row_gap: px(4),
                            ..default()
                        })
                        .with_children(|tabs| {
                            for (label, tab) in [
                                ("Overview", ArtifactInspectorTab::Overview),
                                ("Inputs", ArtifactInspectorTab::Inputs),
                                ("Outputs", ArtifactInspectorTab::Outputs),
                                ("Attempts", ArtifactInspectorTab::Attempts),
                                ("Logs", ArtifactInspectorTab::Logs),
                                ("Help", ArtifactInspectorTab::Help),
                            ] {
                                let selected_label = format!("• {label}");
                                spawn_text_button(
                                    tabs,
                                    font.clone(),
                                    theme,
                                    if tab == selected_tab {
                                        selected_label.as_str()
                                    } else {
                                        label
                                    },
                                    8.0,
                                    UiAction::from(AnalysisCommand::SelectArtifactInspectorTab(tab)),
                                );
                            }
                        });
                    spawn_text(
                        panel,
                        font.clone(),
                        if inspection.exact_run_bindings {
                            "Exact run bindings"
                        } else {
                            "Current inventory fallback · exact run lineage was not recorded"
                        },
                        8.0,
                        if inspection.exact_run_bindings {
                            theme.primary
                        } else {
                            theme.editor_warning
                        },
                    );

                    if matches!(
                        selected_tab,
                        ArtifactInspectorTab::Overview
                            | ArtifactInspectorTab::Inputs
                            | ArtifactInspectorTab::Outputs
                    ) {
                    panel
                        .spawn(Node {
                            width: percent(100),
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(10),
                            row_gap: px(10),
                            ..default()
                        })
                        .with_children(|columns| {
                            for (heading, bindings, tab) in [
                                ("INPUTS", &inspection.resolved_inputs, ArtifactInspectorTab::Inputs),
                                ("OUTPUTS", &inspection.resolved_outputs, ArtifactInspectorTab::Outputs),
                            ] {
                                if selected_tab != ArtifactInspectorTab::Overview && selected_tab != tab {
                                    continue;
                                }
                                columns
                                    .spawn(Node {
                                        min_width: px(280),
                                        flex_basis: px(380),
                                        flex_grow: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(6),
                                        ..default()
                                    })
                                    .with_children(|column| {
                                        spawn_text(
                                            column,
                                            font.clone(),
                                            heading,
                                            8.0,
                                            theme.muted_foreground,
                                        );
                                        if bindings.is_empty() {
                                            spawn_text(
                                                column,
                                                font.clone(),
                                                "None",
                                                9.0,
                                                theme.muted_foreground,
                                            );
                                        }
                                        for binding in bindings {
                                            spawn_binding_row(column, font.clone(), theme, binding);
                                        }
                                    });
                            }
                        });
                    }

                    if matches!(selected_tab, ArtifactInspectorTab::Overview | ArtifactInspectorTab::Attempts)
                        && let Some(run_id) = run_id {
                        let attempt = app_core::load_analysis_node_attempts(run_id)
                            .into_iter()
                            .find(|attempt| attempt.node_id == node_id);
                        if let Some(attempt) = attempt {
                            spawn_text(panel, font.clone(), "ATTEMPT", 8.0, theme.muted_foreground);
                            let duration = attempt
                                .started_at_ms
                                .zip(attempt.finished_at_ms)
                                .filter(|(start, end)| end >= start)
                                .map(|(start, end)| {
                                    format!("{:.2}s", (end - start) as f64 / 1000.0)
                                })
                                .unwrap_or_else(|| "unknown duration".to_string());
                            spawn_wrapped_text(
                                panel,
                                font.clone(),
                                format!(
                                    "{} · {} · {} · {} → {} · {}",
                                    attempt.status,
                                    attempt.implementation,
                                    attempt.model,
                                    attempt.requested_device,
                                    attempt.actual_device,
                                    duration
                                ),
                                9.0,
                                theme.foreground,
                            );
                        }
                    }

                    if selected_tab == ArtifactInspectorTab::Overview
                        && let Ok(impact) = app_core::preview_node_downstream_impact(node_id) {
                        spawn_text(panel, font.clone(), "IMPACT", 8.0, theme.muted_foreground);
                        let affected = if impact.affected_nodes.is_empty() {
                            "No downstream compute nodes".to_string()
                        } else {
                            impact
                                .affected_nodes
                                .iter()
                                .map(|id| id.as_str())
                                .collect::<Vec<_>>()
                                .join(" → ")
                        };
                        spawn_wrapped_text(
                            panel,
                            font.clone(),
                            format!(
                                "{} · Authored chart preserved{}",
                                affected,
                                if impact.export_may_need_regeneration {
                                    " · export may need regeneration"
                                } else {
                                    ""
                                }
                            ),
                            9.0,
                            theme.muted_foreground,
                        );
                    }

                    let mut lineage_lines = Vec::new();
                    for binding in &inspection.resolved_outputs {
                        let Some(reference) = binding.artifact_ref.as_ref() else {
                            continue;
                        };
                        if let Ok(lineage) = app_core::artifact_lineage(reference) {
                            lineage_lines.push(format!(
                                "{:?}: {} revision(s), {} gap(s)",
                                reference.kind,
                                lineage.nodes.len(),
                                lineage.missing_revision_ids.len()
                            ));
                        }
                    }
                    if selected_tab == ArtifactInspectorTab::Overview && !lineage_lines.is_empty() {
                        spawn_text(panel, font.clone(), "LINEAGE", 8.0, theme.muted_foreground);
                        spawn_wrapped_text(
                            panel,
                            font.clone(),
                            lineage_lines.join(" · "),
                            9.0,
                            theme.muted_foreground,
                        );
                    }
                    if selected_tab == ArtifactInspectorTab::Logs {
                        spawn_wrapped_text(
                            panel,
                            font.clone(),
                            "Logs are bounded to the selected node attempt. Use the node context menu's View logs action to open the timestamp-filtered log viewer.",
                            9.0,
                            theme.muted_foreground,
                        );
                    }
                    if selected_tab == ArtifactInspectorTab::Help {
                        spawn_wrapped_text(
                            panel,
                            font.clone(),
                            format!("Open the offline guide for {node_id} to learn its inputs, outputs, reuse rules, and failure recovery."),
                            9.0,
                            theme.muted_foreground,
                        );
                    }
                }
                Err(error) => {
                    spawn_wrapped_text(panel, font, error, 9.0, theme.destructive);
                }
            }
        });
}

#[derive(Clone)]
pub(crate) struct AnalysisArtifactContextMenu {
    pub(crate) reference: app_core::ArtifactRef,
    pub(crate) label: String,
    pub(crate) position: Vec2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectedGraphEdge {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) kind: Option<app_core::ArtifactKind>,
    pub(crate) revision_id: Option<String>,
    pub(crate) state: app_core::ArtifactBindingState,
    pub(crate) producer_node: String,
}

#[derive(Clone)]
pub(crate) struct AnalysisExportContextMenu {
    pub(crate) file_hash: String,
    pub(crate) kind: app_core::ExportPackageKind,
    pub(crate) label: String,
    pub(crate) position: Vec2,
}

pub(crate) fn selected_graph_edge_from_binding(
    from: &app_core::AnalysisNodeId,
    to: &app_core::AnalysisNodeId,
    producer_node: &app_core::AnalysisNodeId,
    kind: Option<app_core::ArtifactKind>,
    binding: Option<&app_core::ArtifactBinding>,
) -> SelectedGraphEdge {
    SelectedGraphEdge {
        from: from.as_str().to_string(),
        to: to.as_str().to_string(),
        kind: kind.or_else(|| binding.map(|binding| binding.kind)),
        revision_id: binding.and_then(|binding| {
            binding
                .artifact_ref
                .as_ref()
                .map(|reference| reference.revision_id.clone())
        }),
        state: binding
            .map(|binding| binding.state)
            .unwrap_or(app_core::ArtifactBindingState::Missing),
        producer_node: producer_node.as_str().to_string(),
    }
}

pub(crate) fn edge_binding_style_copy(state: app_core::ArtifactBindingState) -> &'static str {
    match state {
        app_core::ArtifactBindingState::Resolved => "Produced",
        app_core::ArtifactBindingState::Source => "Source",
        app_core::ArtifactBindingState::Ephemeral => "Ephemeral",
        app_core::ArtifactBindingState::FrozenReuse => "Frozen",
        app_core::ArtifactBindingState::Bypassed => "Bypassed",
        app_core::ArtifactBindingState::Missing => "Missing",
        app_core::ArtifactBindingState::LegacyUntracked => "Reused / untracked",
        app_core::ArtifactBindingState::Invalidated => "Invalidated",
        app_core::ArtifactBindingState::NotApplicable => "Not applicable",
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LineageScope {
    Upstream,
    Downstream,
    #[default]
    Full,
}

#[derive(Clone)]
pub(crate) struct ArtifactLineagePanel {
    pub(crate) lineage: app_core::ArtifactLineage,
    pub(crate) scope: LineageScope,
    pub(crate) selected: app_core::ArtifactRef,
}

pub(crate) fn spawn_artifact_lineage_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    panel: &ArtifactLineagePanel,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(24),
                right: px(24),
                top: px(24),
                bottom: px(24),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(18)),
                row_gap: px(8),
                overflow: Overflow::clip_y(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.99)),
            BorderColor::all(theme.border),
            ZIndex(115),
        ))
        .with_children(|body| {
            spawn_text(body, font.clone(), "LINEAGE", 8.0, theme.primary);
            spawn_text(
                body,
                font.clone(),
                format!(
                    "{:?} · {}",
                    panel.selected.kind,
                    panel
                        .selected
                        .revision_id
                        .chars()
                        .take(12)
                        .collect::<String>()
                ),
                16.0,
                theme.foreground,
            );
            body.spawn(Node {
                width: percent(100),
                column_gap: px(6),
                flex_wrap: FlexWrap::Wrap,
                ..default()
            })
            .with_children(|controls| {
                for (label, scope) in [
                    ("Upstream only", LineageScope::Upstream),
                    ("Downstream only", LineageScope::Downstream),
                    ("Full lineage", LineageScope::Full),
                ] {
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        label,
                        9.0,
                        UiAction::from(AnalysisCommand::SetArtifactLineageScope(scope)),
                    );
                }
                spawn_text_button(
                    controls,
                    font.clone(),
                    theme,
                    "Return to run view",
                    9.0,
                    UiAction::from(AnalysisCommand::CloseArtifactLineage),
                );
            });
            if let Ok(inspection) = app_core::inspect_artifact(&panel.selected) {
                spawn_wrapped_text(
                    body,
                    font.clone(),
                    format!(
                        "Selected revision · producer {} · {:?} · {} byte(s){}",
                        inspection.artifact.producer_node,
                        inspection.health.status,
                        inspection.artifact.byte_size,
                        if inspection.pinned { " · pinned" } else { "" }
                    ),
                    9.0,
                    theme.muted_foreground,
                );
            }
            if matches!(panel.scope, LineageScope::Upstream | LineageScope::Full) {
                spawn_text(
                    body,
                    font.clone(),
                    "UPSTREAM REVISIONS",
                    8.0,
                    theme.muted_foreground,
                );
                for node in &panel.lineage.nodes {
                    let reference = artifact_ref_from_revision(&node.artifact);
                    let short = node
                        .artifact
                        .id
                        .chars()
                        .rev()
                        .take(10)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>();
                    spawn_text_button(
                        body,
                        font.clone(),
                        theme,
                        format!(
                            "{}{:?} · {} · {short}",
                            "  ".repeat(node.depth),
                            node.artifact.kind,
                            node.artifact.producer_node
                        ),
                        9.0,
                        UiAction::from(AnalysisCommand::SelectArtifactLineageRevision(reference)),
                    );
                }
                for missing in &panel.lineage.missing_revision_ids {
                    spawn_wrapped_text(
                        body,
                        font.clone(),
                        format!("GAP · missing legacy revision {missing}"),
                        9.0,
                        theme.destructive,
                    );
                }
            }
            if matches!(panel.scope, LineageScope::Downstream | LineageScope::Full) {
                spawn_text(
                    body,
                    font.clone(),
                    "DOWNSTREAM CONSUMERS",
                    8.0,
                    theme.muted_foreground,
                );
                if panel.lineage.downstream_consumers.is_empty() {
                    spawn_wrapped_text(
                        body,
                        font.clone(),
                        "No exact downstream consumer binding was recorded.",
                        9.0,
                        theme.muted_foreground,
                    );
                }
                for consumer in &panel.lineage.downstream_consumers {
                    spawn_wrapped_text(
                        body,
                        font.clone(),
                        format!("{} · exact input binding", consumer.as_str()),
                        9.0,
                        theme.foreground,
                    );
                }
            }
        });
}

pub(crate) fn spawn_artifact_impact_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    impact: &app_core::DownstreamImpact,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(28), top: px(58), width: px(470), max_height: percent(82),
                flex_direction: FlexDirection::Column, padding: UiRect::all(px(18)),
                row_gap: px(8), overflow: Overflow::clip_y(), border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(9)), ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.99)), BorderColor::all(theme.border), ZIndex(114),
        ))
        .with_children(|body| {
            spawn_text(body, font.clone(), "IMPACT PREVIEW", 8.0, theme.primary);
            spawn_wrapped_text(body, font.clone(), "Read-only preview. No analysis, cache, Active selection, or export is changed here.", 9.0, theme.muted_foreground);
            let groups = [
                ("WILL RUN", impact.will_run.iter().map(|id| id.as_str().to_string()).collect::<Vec<_>>()),
                ("WILL REUSE", impact.will_reuse.iter().map(|id| id.as_str().to_string()).collect()),
                ("WILL BECOME STALE", impact.will_become_stale.iter().map(|id| id.as_str().to_string()).collect()),
                ("WILL BE BLOCKED", impact.will_be_blocked.iter().map(|id| id.as_str().to_string()).collect()),
                ("WILL REMAIN PRESERVED", impact.will_remain_preserved.clone()),
                ("EXPORTS NEEDING REGENERATION", impact.exports_needing_regeneration.clone()),
            ];
            for (label, items) in groups {
                spawn_text(body, font.clone(), label, 8.0, theme.muted_foreground);
                spawn_wrapped_text(body, font.clone(), if items.is_empty() { "None".to_string() } else { items.join(" · ") }, 9.0, theme.foreground);
            }
            body.spawn(Node {
                width: percent(100),
                column_gap: px(6),
                ..default()
            })
            .with_children(|actions| {
                if !impact.queued_targets.is_empty()
                    || !impact.queued_frozen.is_empty()
                    || !impact.queued_bypassed.is_empty()
                    || !impact.queued_disabled.is_empty()
                {
                    spawn_text_button(
                        actions,
                        font.clone(),
                        theme,
                        "Queue this plan",
                        10.0,
                        UiAction::from(AnalysisCommand::ConfirmArtifactImpact),
                    );
                }
                spawn_text_button(actions, font, theme, "Close", 10.0, UiAction::from(AnalysisCommand::CloseArtifactImpact));
            });
        });
}

pub(crate) fn render_artifact_kind(node_id: &str) -> Option<app_core::ArtifactKind> {
    match node_id {
        "artifact.vocal_stem" => Some(app_core::ArtifactKind::VocalStem),
        "artifact.instrumental_stem" => Some(app_core::ArtifactKind::InstrumentalStem),
        "artifact.note_guide" => Some(app_core::ArtifactKind::PitchTrack),
        "artifact.timed_lyrics" => Some(app_core::ArtifactKind::TimedTranscript),
        "artifact.chart" => Some(app_core::ArtifactKind::AuthoredChart),
        _ => None,
    }
}

fn best_artifact_revision(
    file_hash: &str,
    kind: app_core::ArtifactKind,
) -> Option<app_core::ArtifactRevision> {
    app_core::load_active_artifact(file_hash, kind).or_else(|| {
        app_core::load_artifact_revisions(file_hash, kind)
            .into_iter()
            .find(|revision| !revision.invalidated)
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_workbench_artifact_node(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    bounds: AnalysisGraphBox,
    title: &str,
    detail: &str,
    ready: bool,
    node_id: &str,
    file_hash: &str,
    run_id: Option<i64>,
    dimmed: bool,
    selected: bool,
) {
    let kind = render_artifact_kind(node_id);
    let revision = kind.and_then(|kind| match run_id {
        Some(run_id) => app_core::resolve_artifact_for_run(file_hash, run_id, kind),
        None => best_artifact_revision(file_hash, kind),
    });
    let reference = revision.as_ref().map(artifact_ref_from_revision);
    let title_owned = title.to_string();
    let file_hash_owned = file_hash.to_string();

    let mut entity = parent.spawn((
        Button,
        UiPointerApi(&[
            "ui.pointer.analysis_artifact.primary",
            "ui.pointer.analysis_artifact.secondary",
        ]),
        Node {
            position_type: PositionType::Absolute,
            left: px(bounds.x),
            top: px(bounds.y),
            width: px(bounds.width),
            height: px(bounds.height),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(12), px(8)),
            row_gap: px(2),
            overflow: Overflow::clip(),
            border: UiRect::all(px(if selected { 2 } else { 1 })),
            border_radius: BorderRadius::all(px(18)),
            ..default()
        },
        BackgroundColor(if ready {
            theme
                .pitch_contour
                .with_alpha(if dimmed { 0.04 } else { 0.1 })
        } else {
            theme
                .background
                .with_alpha(if dimmed { 0.28 } else { 0.72 })
        }),
        BorderColor::all(if selected {
            theme.primary.with_alpha(0.92)
        } else if ready {
            theme
                .pitch_contour
                .with_alpha(if dimmed { 0.22 } else { 0.62 })
        } else {
            theme.border.with_alpha(if dimmed { 0.28 } else { 0.62 })
        }),
        ZIndex(2),
    ));
    entity.with_children(|node| {
        spawn_analysis_graph_ports(node, theme, ready);
        spawn_text(
            node,
            font.clone(),
            format!("ARTIFACT · {}", if ready { "READY" } else { "PENDING" }),
            6.5,
            if ready {
                theme.pitch_contour
            } else {
                theme.muted_foreground
            },
        );
        spawn_text(node, font.clone(), title, 9.0, theme.foreground);
        spawn_bounded_wrapped_text(node, font, detail, 7.0, theme.muted_foreground);
    });

    entity.observe(
        move |mut event: On<Pointer<Click>>,
              mut shell: ResMut<ShellState>,
              library: Res<LibraryState>,
              analysis: Res<AnalysisUiState>,
              mut dialogs: ResMut<DialogState>,
              mut invalidated: ResMut<UiInvalidated>,
              lists: Query<(&ComputedNode, &UiGlobalTransform), With<LibrarySongList>>| {
            event.propagate(false);
            let Some(reference) = reference.clone() else {
                if event.button == PointerButton::Primary {
                    shell.notice = Some(format!(
                        "{title_owned}: no persisted artifact revision is available yet."
                    ));
                    invalidated.invalidate(UiDirtyRegion::Analysis);
                }
                return;
            };
            match event.button {
                PointerButton::Primary => {
                    shell.notice = Some(match app_core::inspect_artifact(&reference) {
                        Ok(inspection) => format!(
                            "{title_owned} · {:?} · {:?}{}",
                            inspection.media_type,
                            inspection.health.status,
                            if inspection.pinned { " · pinned" } else { "" }
                        ),
                        Err(error) => error,
                    });
                    dialogs.analysis_artifact_context = None;
                    if analysis.analysis_lineage_mode
                        && let Ok(lineage) = app_core::artifact_lineage(&reference)
                    {
                        dialogs.artifact_lineage = Some(ArtifactLineagePanel {
                            lineage,
                            scope: analysis.analysis_lineage_scope,
                            selected: reference,
                        });
                    }
                }
                PointerButton::Secondary => {
                    let menu_position = analysis_context_menu_position(
                        event.pointer_location.position,
                        library.library_scroll_offset,
                        &lists,
                    );
                    dialogs.analysis_artifact_context = Some(AnalysisArtifactContextMenu {
                        reference,
                        label: title_owned.clone(),
                        position: menu_position,
                    });
                }
                PointerButton::Middle => return,
            }
            let _ = &file_hash_owned;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        },
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_workbench_export_node(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    bounds: AnalysisGraphBox,
    title: &str,
    file_hash: &str,
    node_id: &str,
    ready: bool,
    lineage_dimmed: bool,
    selected: bool,
) {
    let Some(kind) = app_core::ExportPackageKind::from_node_id(node_id) else {
        return;
    };
    let inspection = app_core::inspect_export_node(file_hash, kind).ok();
    let ready = inspection.as_ref().map(|item| item.ready).unwrap_or(ready);
    let last = inspection
        .as_ref()
        .and_then(|item| item.last_destination.as_ref())
        .map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string())
        });
    let detail = match (ready, last.as_deref()) {
        (true, Some(name)) => format!("Ready · last {name}"),
        (true, None) => "Ready · no last export tracked".to_string(),
        (false, Some(name)) => format!("Blocked · last {name}"),
        (false, None) => inspection
            .as_ref()
            .map(|item| {
                if item.missing.is_empty() {
                    "Pending".to_string()
                } else {
                    format!("Missing {}", item.missing.join(", "))
                }
            })
            .unwrap_or_else(|| "Pending".to_string()),
    };
    let title_owned = title.to_string();
    let file_hash_owned = file_hash.to_string();
    let accent = theme.primary;
    let alpha = if lineage_dimmed { 0.28 } else { 1.0 };
    let mut entity = parent.spawn((
        Button,
        UiPointerApi(&[
            "ui.pointer.analysis_export.primary",
            "ui.pointer.analysis_export.secondary",
        ]),
        Node {
            position_type: PositionType::Absolute,
            left: px(bounds.x),
            top: px(bounds.y),
            width: px(bounds.width),
            height: px(bounds.height),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(12), px(8)),
            row_gap: px(2),
            overflow: Overflow::clip(),
            border: UiRect::all(px(if selected { 2 } else { 1 })),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(if ready {
            accent.with_alpha(0.1 * alpha)
        } else {
            theme.background.with_alpha(0.72 * alpha)
        }),
        BorderColor::all(if selected {
            theme.primary.with_alpha(0.92)
        } else if ready {
            accent.with_alpha(0.62 * alpha)
        } else {
            theme.border.with_alpha(0.62 * alpha)
        }),
        ZIndex(2),
    ));
    entity.with_children(|node| {
        spawn_analysis_graph_ports(node, theme, ready);
        spawn_text(
            node,
            font.clone(),
            format!("EXPORT · {}", if ready { "READY" } else { "PENDING" }),
            6.5,
            if ready {
                accent
            } else {
                theme.muted_foreground
            },
        );
        spawn_text(node, font.clone(), title, 9.0, theme.foreground);
        spawn_bounded_wrapped_text(node, font, detail, 7.0, theme.muted_foreground);
    });
    entity.observe(
        move |mut event: On<Pointer<Click>>,
              mut shell: ResMut<ShellState>,
              library: Res<LibraryState>,
              mut dialogs: ResMut<DialogState>,
              mut invalidated: ResMut<UiInvalidated>,
              lists: Query<(&ComputedNode, &UiGlobalTransform), With<LibrarySongList>>| {
            event.propagate(false);
            match event.button {
                PointerButton::Primary => {
                    shell.notice = Some(
                        match app_core::inspect_export_node(&file_hash_owned, kind) {
                            Ok(inspection) => {
                                let dest = inspection
                                    .last_destination
                                    .as_ref()
                                    .map(|path| path.display().to_string())
                                    .unwrap_or_else(|| "no last export tracked".to_string());
                                format!(
                                    "{title_owned} · {} · {dest}",
                                    if inspection.ready {
                                        "ready"
                                    } else {
                                        "not ready"
                                    }
                                )
                            }
                            Err(error) => error,
                        },
                    );
                    dialogs.analysis_export_context = None;
                }
                PointerButton::Secondary => {
                    let menu_position = analysis_context_menu_position(
                        event.pointer_location.position,
                        library.library_scroll_offset,
                        &lists,
                    );
                    dialogs.analysis_export_context = Some(AnalysisExportContextMenu {
                        file_hash: file_hash_owned.clone(),
                        kind,
                        label: title_owned.clone(),
                        position: menu_position,
                    });
                }
                PointerButton::Middle => return,
            }
            invalidated.invalidate(UiDirtyRegion::Analysis);
        },
    );
}

pub(crate) fn spawn_analysis_export_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    context: &AnalysisExportContextMenu,
) {
    let inspection = app_core::inspect_export_node(&context.file_hash, context.kind).ok();
    parent.spawn((
        Button,
        UiAction::from(AnalysisCommand::DismissAnalysisExportContext),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ZIndex(44),
    ));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(context.position.x.max(8.0)),
                top: px(context.position.y.max(8.0)),
                width: px(270),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(2),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.99)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(45),
        ))
        .with_children(|menu| {
            spawn_text(
                menu,
                font.clone(),
                context.label.clone(),
                11.0,
                theme.foreground,
            );
            spawn_text(
                menu,
                font.clone(),
                "Export actions",
                8.0,
                theme.muted_foreground,
            );
            menu.spawn(Node {
                height: px(5),
                ..default()
            });
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Validate export",
                11.0,
                UiAction::from(AnalysisCommand::ValidateExportNode(
                    context.file_hash.clone(),
                    context.kind,
                )),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Re-export",
                11.0,
                match context.kind {
                    app_core::ExportPackageKind::Utz => {
                        UiAction::from(LibraryCommand::ExportUtz(context.file_hash.clone()))
                    }
                    app_core::ExportPackageKind::UltraStar => {
                        UiAction::from(LibraryCommand::ExportUltraStar(context.file_hash.clone()))
                    }
                },
            );
            if inspection
                .as_ref()
                .is_some_and(|item| item.last_destination_exists)
            {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Reveal last export",
                    11.0,
                    UiAction::from(AnalysisCommand::RevealLastExport(
                        context.file_hash.clone(),
                        context.kind,
                    )),
                );
            }
            spawn_text_button(
                menu,
                font,
                theme,
                "Export documentation",
                11.0,
                UiAction::from(AppCommand::OpenDocumentation(Some(
                    "guide:export".to_string(),
                ))),
            );
        });
}

pub(crate) fn spawn_analysis_artifact_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    context: &AnalysisArtifactContextMenu,
) {
    let Ok(inspection) = app_core::inspect_artifact(&context.reference) else {
        return;
    };
    let revision = &inspection.artifact;

    parent.spawn((
        Button,
        UiAction::from(AnalysisCommand::DismissAnalysisArtifactContext),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ZIndex(44),
    ));

    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(context.position.x.max(8.0)),
                top: px(context.position.y.max(8.0)),
                width: px(270),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(2),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.99)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(45),
        ))
        .with_children(|menu| {
            spawn_text(
                menu,
                font.clone(),
                context.label.clone(),
                11.0,
                theme.foreground,
            );
            spawn_text(
                menu,
                font.clone(),
                format!("{:?} · Artifact actions", revision.kind),
                8.0,
                theme.muted_foreground,
            );
            menu.spawn(Node {
                height: px(5),
                ..default()
            });

            if artifact_kind_is_playable(revision.kind) {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Play",
                    11.0,
                    UiAction::from(LibraryCommand::PlayArtifactRevision(revision.path.clone())),
                );
            } else {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Preview",
                    11.0,
                    UiAction::from(AnalysisCommand::PreviewArtifactRevision(
                        revision.path.clone(),
                    )),
                );
            }

            if inspection.capabilities.iter().any(|capability| {
                matches!(
                    capability,
                    app_core::ArtifactCapability::OpenLyricsEditor
                        | app_core::ArtifactCapability::OpenChartEditor
                )
            }) {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Open in compatible editor…",
                    11.0,
                    UiAction::from(AnalysisCommand::OpenArtifactCompatibleEditor(
                        context.reference.clone(),
                    )),
                );
            }

            if inspection
                .capabilities
                .contains(&app_core::ArtifactCapability::SetActive)
            {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Set Active…",
                    11.0,
                    UiAction::from(AnalysisCommand::SetActiveArtifactRevision(revision.clone())),
                );
            }
            if !revision.active
                && let Some(active) =
                    app_core::load_active_artifact(&revision.file_hash, revision.kind)
            {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Compare with Active…",
                    11.0,
                    UiAction::from(AnalysisCommand::CompareArtifactRevisions(
                        revision.clone(),
                        artifact_ref_from_revision(&active),
                    )),
                );
            }
            if revision.kind == app_core::ArtifactKind::CandidateChart
                && let Some(authored) = app_core::load_active_artifact(
                    &revision.file_hash,
                    app_core::ArtifactKind::AuthoredChart,
                )
            {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Compare with Authored…",
                    11.0,
                    UiAction::from(AnalysisCommand::CompareArtifactRevisions(
                        revision.clone(),
                        artifact_ref_from_revision(&authored),
                    )),
                );
                let candidate_ref = artifact_ref_from_revision(revision);
                let authored_ref = artifact_ref_from_revision(&authored);
                for (label, mode) in [
                    (
                        "Merge candidate into working copy…",
                        app_core::ChartRevisionMergeMode::ReplaceAll,
                    ),
                    (
                        "Take candidate lyrics timing…",
                        app_core::ChartRevisionMergeMode::TakeCandidateLyricsTiming,
                    ),
                    (
                        "Take candidate pitch only…",
                        app_core::ChartRevisionMergeMode::TakeCandidatePitch,
                    ),
                ] {
                    spawn_text_button(
                        menu,
                        font.clone(),
                        theme,
                        label,
                        11.0,
                        UiAction::from(AnalysisCommand::MergeCandidateChart(
                            candidate_ref.clone(),
                            authored_ref.clone(),
                            mode,
                        )),
                    );
                }
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Merge selected phrase…",
                    11.0,
                    UiAction::from(AnalysisCommand::MergeSelectedCandidatePhrase(
                        candidate_ref.clone(),
                        authored_ref.clone(),
                    )),
                );
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Merge selected note range…",
                    11.0,
                    UiAction::from(AnalysisCommand::MergeSelectedCandidateRange(
                        candidate_ref.clone(),
                        authored_ref.clone(),
                    )),
                );
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Replace Authored with this candidate…",
                    11.0,
                    UiAction::from(AnalysisCommand::RequestReplaceAuthoredChart(
                        revision.file_hash.clone(),
                    )),
                );
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Keep Authored",
                    11.0,
                    UiAction::from(AnalysisCommand::KeepAuthoredChart),
                );
            }

            spawn_text_button(
                menu,
                font.clone(),
                theme,
                if inspection.pinned { "Unpin" } else { "Pin" },
                11.0,
                UiAction::from(AnalysisCommand::ToggleArtifactPinned(
                    context.reference.clone(),
                )),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Source",
                11.0,
                UiAction::from(AnalysisCommand::ShowArtifactLineage(
                    context.reference.clone(),
                )),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Impact",
                11.0,
                UiAction::from(AnalysisCommand::ShowArtifactImpact(
                    context.reference.clone(),
                )),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Inspect provenance",
                11.0,
                UiAction::from(AnalysisCommand::InspectArtifactProvenance(revision.clone())),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Reveal",
                11.0,
                UiAction::from(AnalysisCommand::RevealArtifactRevision(
                    revision.path.clone(),
                )),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "About this artifact",
                11.0,
                UiAction::from(AppCommand::OpenDocumentation(Some(format!(
                    "artifact:{:?}",
                    revision.kind
                )))),
            );
            if inspection
                .capabilities
                .contains(&app_core::ArtifactCapability::Invalidate)
            {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Invalidate…",
                    11.0,
                    UiAction::from(AnalysisCommand::RequestInvalidateArtifactRevision(
                        revision.clone(),
                    )),
                );
            }
            if !inspection.pinned {
                spawn_text_button(
                    menu,
                    font,
                    theme,
                    "Delete",
                    11.0,
                    UiAction::from(AnalysisCommand::RequestDeleteArtifactRevision(
                        revision.clone(),
                    )),
                );
            }
        });
}

pub(crate) fn spawn_artifact_diff_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    diff: &app_core::ArtifactTypedDiff,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.82)),
            ZIndex(90),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: percent(72),
                        max_width: px(820),
                        height: percent(78),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(20)),
                        row_gap: px(9),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|panel| {
                    spawn_text(
                        panel,
                        font.clone(),
                        "SEMANTIC REVISION DIFF",
                        8.0,
                        theme.primary,
                    );
                    spawn_wrapped_text(
                        panel,
                        font.clone(),
                        diff.summary.clone(),
                        13.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        panel,
                        font.clone(),
                        format!(
                            "{}  ↔  {}",
                            diff.revision_a.revision_id, diff.revision_b.revision_id
                        ),
                        8.0,
                        theme.muted_foreground,
                    );
                    panel
                        .spawn(Node {
                            width: percent(100),
                            min_height: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(5),
                            padding: UiRect::all(px(9)),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        })
                        .with_children(|changes| {
                            if diff.changed_fields.is_empty() {
                                spawn_text(
                                    changes,
                                    font.clone(),
                                    if diff.same_content {
                                        "No semantic or byte-level changes."
                                    } else {
                                        "No bounded structured detail is available."
                                    },
                                    10.0,
                                    theme.muted_foreground,
                                );
                            } else {
                                for change in &diff.changed_fields {
                                    spawn_wrapped_text(
                                        changes,
                                        font.clone(),
                                        format!("• {change}"),
                                        10.0,
                                        theme.foreground,
                                    );
                                }
                            }
                        });
                    panel
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            ..default()
                        })
                        .with_children(|actions| {
                            spawn_action_button(
                                actions,
                                font,
                                theme,
                                "Close",
                                UiAction::from(AnalysisCommand::CloseArtifactDiff),
                            );
                        });
                });
        });
}
