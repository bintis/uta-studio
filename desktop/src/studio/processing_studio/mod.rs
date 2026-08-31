use std::collections::{BTreeMap, BTreeSet};

use crate::studio::*;

mod node_card;
mod stage_fusion;
mod stage_header;
mod status_strip;
mod workspace_sidebar;

#[derive(Component)]
pub(crate) struct ProcessingStudioScroll;

#[derive(Component)]
pub(crate) struct WorkflowReorderHandle {
    node_id: app_core::WorkflowNodeId,
}

#[derive(Component)]
pub(crate) struct WorkflowReorderTarget {
    node_id: app_core::WorkflowNodeId,
}

#[derive(Resource, Default)]
pub(crate) struct ProcessingStudioPointerCapture {
    node_id: Option<app_core::WorkflowNodeId>,
    pointer_start: Vec2,
    dragging: bool,
}

fn workflow_topological_order(
    definition: &app_core::WorkflowDefinition,
) -> Vec<app_core::WorkflowNodeId> {
    let original_index = definition
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.instance_id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut indegree = definition
        .nodes
        .iter()
        .map(|node| (node.instance_id.clone(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing =
        BTreeMap::<app_core::WorkflowNodeId, BTreeSet<app_core::WorkflowNodeId>>::new();
    for edge in &definition.edges {
        if edge.from.node == edge.to.node
            || !indegree.contains_key(&edge.from.node)
            || !indegree.contains_key(&edge.to.node)
        {
            continue;
        }
        if outgoing
            .entry(edge.from.node.clone())
            .or_default()
            .insert(edge.to.node.clone())
        {
            *indegree.entry(edge.to.node.clone()).or_default() += 1;
        }
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(node, count)| (*count == 0).then_some(node.clone()))
        .collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(definition.nodes.len());
    while !ready.is_empty() {
        ready.sort_by_key(|node| original_index.get(node).copied().unwrap_or(usize::MAX));
        let node = ready.remove(0);
        ordered.push(node.clone());
        if let Some(children) = outgoing.get(&node) {
            for child in children {
                if let Some(count) = indegree.get_mut(child) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready.push(child.clone());
                    }
                }
            }
        }
    }
    if ordered.len() != definition.nodes.len() {
        for node in &definition.nodes {
            if !ordered.contains(&node.instance_id) {
                ordered.push(node.instance_id.clone());
            }
        }
    }
    ordered
}

#[cfg(test)]
fn reorderable_workflow_nodes(
    definition: &app_core::WorkflowDefinition,
) -> Vec<app_core::WorkflowNodeId> {
    let capabilities = app_core::list_workflow_capabilities()
        .into_iter()
        .map(|capability| (capability.id, capability.preserves_audio_role))
        .collect::<BTreeMap<_, _>>();
    let nodes = definition
        .nodes
        .iter()
        .map(|node| (node.instance_id.clone(), node))
        .collect::<BTreeMap<_, _>>();
    workflow_topological_order(definition)
        .into_iter()
        .filter(|id| {
            nodes.get(id).is_some_and(|node| {
                capabilities
                    .get(&node.capability_id)
                    .copied()
                    .unwrap_or(false)
            })
        })
        .collect()
}

fn reorderable_workflow_branch(
    definition: &app_core::WorkflowDefinition,
    selected: &app_core::WorkflowNodeId,
) -> Vec<app_core::WorkflowNodeId> {
    let role_preserving = app_core::list_workflow_capabilities()
        .into_iter()
        .map(|capability| (capability.id, capability.preserves_audio_role))
        .collect::<BTreeMap<_, _>>();
    let nodes = definition
        .nodes
        .iter()
        .map(|node| (node.instance_id.clone(), node.capability_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let is_reorderable = |node: &app_core::WorkflowNodeId| {
        nodes
            .get(node)
            .and_then(|capability| role_preserving.get(capability))
            .copied()
            .unwrap_or(false)
    };
    if !is_reorderable(selected) {
        return Vec::new();
    }
    let mut first = selected.clone();
    while let Some(previous) = definition
        .edges
        .iter()
        .find(|edge| edge.to.node == first && edge.to.port == "audio")
        .map(|edge| edge.from.node.clone())
        .filter(|node| is_reorderable(node))
    {
        first = previous;
    }
    let mut branch = vec![first.clone()];
    let mut current = first;
    while let Some(next) = definition
        .edges
        .iter()
        .find(|edge| edge.from.node == current && edge.from.port == "audio")
        .map(|edge| edge.to.node.clone())
        .filter(|node| is_reorderable(node))
    {
        if branch.contains(&next) {
            break;
        }
        branch.push(next.clone());
        current = next;
    }
    branch
}

fn audio_branch_tail(
    definition: &app_core::WorkflowDefinition,
    capabilities: &BTreeMap<app_core::CapabilityId, app_core::NodeCapability>,
    start_node: &str,
    start_port: &str,
) -> (app_core::WorkflowNodeId, String) {
    let mut node = app_core::WorkflowNodeId::new(start_node);
    let mut port = start_port.to_string();
    loop {
        let next = definition.edges.iter().find(|edge| {
            edge.from.node == node
                && edge.from.port == port
                && definition
                    .nodes
                    .iter()
                    .find(|candidate| candidate.instance_id == edge.to.node)
                    .is_some_and(|candidate| {
                        capabilities
                            .get(&candidate.capability_id)
                            .is_some_and(|capability| {
                                capability.class == app_core::CapabilityClass::AudioTransformation
                            })
                    })
        });
        let Some(edge) = next else { break };
        node = edge.to.node.clone();
        let Some(instance) = definition
            .nodes
            .iter()
            .find(|candidate| candidate.instance_id == node)
        else {
            break;
        };
        let Some(capability) = capabilities.get(&instance.capability_id) else {
            break;
        };
        port = capability
            .outputs
            .iter()
            .find(|output| {
                output.port_type.is_audio()
                    && definition
                        .edges
                        .iter()
                        .any(|edge| edge.from.node == node && edge.from.port == output.id)
            })
            .or_else(|| {
                capability
                    .outputs
                    .iter()
                    .find(|output| output.port_type.is_audio())
            })
            .map(|output| output.id.clone())
            .unwrap_or_else(|| "audio".to_string());
    }
    (node, port)
}

fn stage_renders_internal_node_cards(stage: u8) -> bool {
    stage != 4
}

fn workflow_stage(capability: &app_core::NodeCapability) -> u8 {
    let id = capability.id.as_str();
    if capability.class == app_core::CapabilityClass::Source {
        0
    } else if capability.class == app_core::CapabilityClass::AudioTransformation {
        1
    } else if id == "analysis.asr"
        || id == "analysis.forced_alignment"
        || id == "fusion.transcript"
        || id == "lyrics.known"
    {
        2
    } else if capability.class == app_core::CapabilityClass::Analyzer {
        3
    } else {
        4
    }
}

pub(crate) fn processing_studio_scroll_max(viewport_height: f32, content_height: f32) -> f32 {
    (content_height - viewport_height).max(0.0)
}

pub(crate) fn handle_processing_studio_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    shell: Res<ShellState>,
    mut analysis: ResMut<AnalysisUiState>,
    mut panels: Query<(&ComputedNode, &mut ScrollPosition), With<ProcessingStudioScroll>>,
) {
    if shell.route != StudioRoute::ProcessingStudio {
        return;
    }
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 28.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            -event.y * scale
        })
        .sum::<f32>();
    let Ok((computed, mut position)) = panels.single_mut() else {
        return;
    };
    let viewport = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    let next = (position.y + delta).clamp(0.0, processing_studio_scroll_max(viewport.y, content.y));
    if (position.y - next).abs() > f32::EPSILON {
        position.y = next;
    }
    if (analysis.processing_studio_scroll_offset - next).abs() > f32::EPSILON {
        analysis.processing_studio_scroll_offset = next;
    }
}

/// Captures a reorderable workflow card from press through global release.
/// The drop is translated to the same semantic edge rewrites used by the
/// keyboard-accessible Earlier/Later commands; canvas coordinates never enter
/// the persisted workflow.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_processing_studio_pointer_capture(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut focus_events: MessageReader<bevy::window::WindowFocused>,
    handles: Query<(&Interaction, &WorkflowReorderHandle)>,
    targets: Query<(&WorkflowReorderTarget, &ComputedNode, &UiGlobalTransform)>,
    mut capture: ResMut<ProcessingStudioPointerCapture>,
    mut shell: ResMut<ShellState>,
    mut analysis: ResMut<AnalysisUiState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let focus_lost = focus_events.read().any(|event| !event.focused);
    if shell.route != StudioRoute::ProcessingStudio
        || focus_lost
        || keys.just_pressed(KeyCode::Escape)
    {
        capture.node_id = None;
        capture.dragging = false;
        return;
    }

    let pointer = windows.single().ok().and_then(Window::cursor_position);
    if capture.node_id.is_none() && mouse.just_pressed(MouseButton::Left) {
        let Some(pointer) = pointer else {
            return;
        };
        if let Some((_, handle)) = handles
            .iter()
            .find(|(interaction, _)| **interaction == Interaction::Pressed)
        {
            capture.node_id = Some(handle.node_id.clone());
            capture.pointer_start = pointer;
            capture.dragging = false;
        }
        return;
    }

    if mouse.pressed(MouseButton::Left) {
        if let Some(pointer) = pointer
            && capture.node_id.is_some()
            && pointer.distance(capture.pointer_start) > 7.0
        {
            capture.dragging = true;
        }
        return;
    }

    let Some(source) = capture.node_id.take() else {
        capture.dragging = false;
        return;
    };
    let was_dragging = std::mem::take(&mut capture.dragging);
    if !mouse.just_released(MouseButton::Left) || !was_dragging {
        return;
    }
    let Some(pointer) = pointer else {
        return;
    };
    let Some(target) = targets.iter().find_map(|(target, computed, transform)| {
        (target.node_id != source && ui_node_contains_pointer(computed, transform, pointer))
            .then_some(target.node_id.clone())
    }) else {
        shell.notice = Some(
            "Workflow reorder cancelled: drop on another draggable transformation.".to_string(),
        );
        invalidated.invalidate(UiDirtyRegion::Analysis);
        return;
    };
    let Some(workflow) = analysis.workflow.as_mut() else {
        return;
    };
    let order = reorderable_workflow_branch(&workflow.definition, &source);
    let Some(source_index) = order.iter().position(|node| node == &source) else {
        return;
    };
    let Some(target_index) = order.iter().position(|node| node == &target) else {
        shell.notice = Some(
            "Workflow reorder rejected: cards must stay inside the same semantic audio branch."
                .to_string(),
        );
        invalidated.invalidate(UiDirtyRegion::Analysis);
        return;
    };
    let original = workflow.definition.clone();
    let earlier = target_index < source_index;
    let steps = source_index.abs_diff(target_index);
    let mut result = Ok(());
    for _ in 0..steps {
        result = app_core::reorder_audio_transformation(&mut workflow.definition, &source, earlier);
        if result.is_err() {
            break;
        }
    }
    match result {
        Ok(()) => {
            analysis.workflow_compile_error = None;
            analysis.selected_workflow_node = Some(source);
            shell.notice = Some("Workflow order changed by drag. Save to keep it.".to_string());
        }
        Err(error) => {
            workflow.definition = original;
            analysis.workflow_compile_error = Some(error.clone());
            shell.notice = Some(format!("Workflow reorder rejected: {error}"));
        }
    }
    invalidated.invalidate(UiDirtyRegion::Analysis);
}

fn action_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    action: UiAction,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                min_width: px(0),
                max_width: percent(100),
                min_height: px(32),
                flex_shrink: 1.0,
                padding: UiRect::axes(px(12), px(7)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.42)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|button| {
            spawn_wrapped_text(button, font, label, 9.0, theme.foreground);
        });
}

fn disabled_action_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
) {
    parent
        .spawn((
            Node {
                min_width: px(0),
                max_width: percent(100),
                min_height: px(32),
                flex_shrink: 1.0,
                padding: UiRect::axes(px(12), px(7)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.28)),
            BorderColor::all(theme.border.with_alpha(0.32)),
            Pickable::IGNORE,
        ))
        .with_children(|button| {
            spawn_wrapped_text(button, font, label, 9.0, theme.muted_foreground);
        });
}

fn port_label(port: &app_core::WorkflowPortSpec) -> String {
    let role = match &port.port_type {
        app_core::WorkflowPortType::Audio(app_core::AudioRole::LeadVocal) => Some("LeadVocal"),
        app_core::WorkflowPortType::Audio(app_core::AudioRole::VocalResidual) => {
            Some("VocalResidual")
        }
        app_core::WorkflowPortType::Audio(app_core::AudioRole::Vocal) => Some("Vocal"),
        app_core::WorkflowPortType::Audio(app_core::AudioRole::Instrumental) => {
            Some("Instrumental")
        }
        app_core::WorkflowPortType::Audio(app_core::AudioRole::SourceMix) => Some("SourceMix"),
        _ => None,
    };
    role.map_or_else(|| port.id.clone(), |role| format!("{} · {role}", port.id))
}

pub(crate) fn spawn_processing_studio(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let Some(stored) = session.workflow.as_ref() else {
        parent
            .spawn(Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(12),
                padding: UiRect::all(px(28)),
                ..default()
            })
            .with_children(|empty| {
                if session.selected_song.is_none() {
                    spawn_text(
                        empty,
                        font.clone(),
                        "Choose a song to continue",
                        20.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        empty,
                        font.clone(),
                        "Processing Studio needs a song before its workflow can be configured.",
                        11.0,
                        theme.muted_foreground,
                    );
                    spawn_action_button(
                        empty,
                        font.clone(),
                        theme,
                        "Choose a song",
                        UiAction::from(LibraryCommand::SetLibraryView(LibraryView::All)),
                    );
                } else {
                    spawn_wrapped_text(
                        empty,
                        font.clone(),
                        "Workflow is unavailable. Return to the song and reopen Processing Studio.",
                        12.0,
                        theme.destructive,
                    );
                    spawn_action_button(
                        empty,
                        font.clone(),
                        theme,
                        "Back to library",
                        UiAction::from(LibraryCommand::SetLibraryView(LibraryView::All)),
                    );
                }
            });
        return;
    };
    let capabilities = app_core::list_workflow_capabilities()
        .into_iter()
        .map(|capability| (capability.id.clone(), capability))
        .collect::<BTreeMap<_, _>>();
    let audio_sources = stored
        .definition
        .nodes
        .iter()
        .flat_map(|node| {
            capabilities
                .get(&node.capability_id)
                .into_iter()
                .flat_map(move |capability| {
                    capability
                        .outputs
                        .iter()
                        .filter(|output| output.port_type.is_audio())
                        .map(move |output| {
                            (
                                node.instance_id.clone(),
                                output.id.clone(),
                                format!("{} · {}", capability.label, port_label(output)),
                            )
                        })
                })
        })
        .collect::<Vec<_>>();
    let analyzer_bindings = stored
        .definition
        .analyzer_bindings
        .iter()
        .map(|binding| (&binding.analyzer_node, binding))
        .collect::<BTreeMap<_, _>>();
    let nodes_by_id = stored
        .definition
        .nodes
        .iter()
        .map(|node| (&node.instance_id, node))
        .collect::<BTreeMap<_, _>>();
    let ordered_nodes = workflow_topological_order(&stored.definition)
        .into_iter()
        .filter_map(|node_id| nodes_by_id.get(&node_id).copied())
        .collect::<Vec<_>>();

    parent
        .spawn((
            Node {
                min_width: px(0),
                min_height: px(0),
                height: percent(100),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(12)),
                row_gap: px(8),
                ..default()
            },
            BackgroundColor(theme.background),
        ))
        .with_children(|page| {
            status_strip::spawn_workflow_status_strip(page, font.clone(), theme, session, stored);

            page.spawn((
                ProcessingStudioScroll,
                ScrollPosition(Vec2::new(
                    0.0,
                    session.processing_studio_scroll_offset,
                )),
                Node {
                    width: percent(100),
                    height: percent(100),
                    min_width: px(0),
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(8),
                    align_items: AlignItems::Stretch,
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|workspace| {
                let vocal_tail = audio_branch_tail(
                    &stored.definition,
                    &capabilities,
                    "vocal_bgm_split",
                    "vocal",
                );
                let bgm_tail = audio_branch_tail(
                    &stored.definition,
                    &capabilities,
                    "vocal_bgm_split",
                    "instrumental",
                );
                workspace
                    .spawn(Node {
                        min_width: px(0),
                        min_height: vh(60.0),
                        height: percent(100),
                        flex_basis: px(0),
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        align_self: AlignSelf::Stretch,
                        flex_direction: FlexDirection::Row,
                        column_gap: px(8),
                        align_items: AlignItems::Stretch,
                        ..default()
                    })
                    .with_children(|stages| {
                for (stage, title, description) in [
                    (
                        1u8,
                        "Pre-processing",
                        "Separate the source, then clean the vocal or instrumental branch when needed.",
                    ),
                    (
                        2u8,
                        "Lyrics & transcription",
                        "Build canonical lyric evidence, combine transcripts and align text to the song.",
                    ),
                    (
                        3u8,
                        "Pitch & note experts",
                        "Configure independent evidence providers; shared capabilities stay grouped.",
                    ),
                    (
                        4u8,
                        "Fusion & output",
                        "Choose only the final decision mode; the Engine owns candidate construction.",
                    ),
                ] {
                    let lane_weight = match stage {
                        3 => 1.18,
                        4 => 0.92,
                        _ => 1.0,
                    };
                    stages
                        .spawn((
                            Node {
                                min_width: px(0),
                                min_height: percent(100),
                                flex_basis: px(0),
                                flex_grow: lane_weight,
                                flex_shrink: 1.0,
                                align_self: AlignSelf::Stretch,
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(9)),
                                row_gap: px(8),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(8)),
                                ..default()
                            },
                            BackgroundColor(theme.card.with_alpha(if stage == 4 {
                                0.22
                            } else {
                                0.17
                            })),
                            BorderColor::all(theme.border.with_alpha(0.46)),
                        ))
                        .with_children(|lane| {
                            let stage_nodes = ordered_nodes
                                .iter()
                                .copied()
                                .filter(|node| {
                                    stage_renders_internal_node_cards(stage)
                                        && capabilities.get(&node.capability_id).is_some_and(
                                            |capability| workflow_stage(capability) == stage,
                                        )
                                })
                                .collect::<Vec<_>>();
                            let stats = (stage != 4)
                                .then(|| stage_header::StageCardStats::from_nodes(&stage_nodes));
                            stage_header::spawn_stage_header(
                                lane,
                                font.clone(),
                                theme,
                                stage,
                                title,
                                description,
                                stats,
                            );

                            if stage == 1 {
                                let optional_preprocessing_is_enabled = stage_nodes.iter().any(
                                    |node| {
                                        capabilities.get(&node.capability_id).is_some_and(
                                            |capability| {
                                                node_card::uses_binary_preprocessing_switch(
                                                    capability,
                                                ) && node.execution_policy
                                                    != app_core::ExecutionPolicy::Disabled
                                            },
                                        )
                                    },
                                );
                                stage_header::spawn_compact_toggle_row(
                                    lane,
                                    font.clone(),
                                    theme,
                                    "Optional cleanup",
                                    "Lead isolation, denoise and dereverb share this quick switch; each card remains independently editable.",
                                    optional_preprocessing_is_enabled,
                                    UiAction::from(
                                        AnalysisCommand::SetWorkflowPreprocessingEnabled(
                                            !optional_preprocessing_is_enabled,
                                        ),
                                    ),
                                );
                            } else if stage == 2
                                && let Some(status) = session
                                    .selected_song
                                    .as_deref()
                                    .and_then(app_core::canonical_lyrics_status)
                            {
                                let count = status.line_count.to_string();
                                let message = match status.source {
                                    app_core::CanonicalLyricsSource::Plain => localized_message(
                                        session.config,
                                        UiMessage::CanonicalLyricsAvailablePlain,
                                        &[("{count}", &count)],
                                    ),
                                    app_core::CanonicalLyricsSource::TimedLrc => localized_message(
                                        session.config,
                                        UiMessage::CanonicalLyricsAvailableTimedLrc,
                                        &[("{count}", &count)],
                                    ),
                                };
                                lane.spawn((
                                    Node {
                                        width: percent(100),
                                        padding: UiRect::axes(px(9), px(7)),
                                        border: UiRect::all(px(1)),
                                        border_radius: studio_card_radius(),
                                        ..default()
                                    },
                                    BackgroundColor(theme.primary.with_alpha(0.05)),
                                    BorderColor::all(theme.primary.with_alpha(0.2)),
                                ))
                                .with_children(|lyrics_status| {
                                    spawn_wrapped_text(
                                        lyrics_status,
                                        font.clone(),
                                        message,
                                        8.0,
                                        theme.primary,
                                    );
                                });
                            }

                            if stage == 4 {
                                stage_fusion::spawn_fusion_stage_card(
                                    lane,
                                    font.clone(),
                                    theme,
                                    stored,
                                    stage_fusion::fusion_adapter_readiness(session),
                                );
                                return;
                            }

                            let mut rendered = BTreeSet::new();
                            for node in &stage_nodes {
                                if !rendered.insert(node.instance_id.clone()) {
                                    continue;
                                }
                                let Some(capability) = capabilities.get(&node.capability_id) else {
                                    continue;
                                };
                                let group = if capability.class
                                    == app_core::CapabilityClass::Analyzer
                                {
                                    stage_nodes
                                        .iter()
                                        .copied()
                                        .filter(|candidate| {
                                            candidate.capability_id == node.capability_id
                                        })
                                        .collect::<Vec<_>>()
                                } else {
                                    vec![*node]
                                };
                                if group.len() > 1 {
                                    let provider_count = group.len();
                                    let group_stats =
                                        stage_header::StageCardStats::from_nodes(&group);
                                    for member in &group {
                                        rendered.insert(member.instance_id.clone());
                                    }
                                    lane.spawn((
                                        Node {
                                            width: percent(100),
                                            flex_direction: FlexDirection::Column,
                                            padding: UiRect::all(px(8)),
                                            row_gap: px(6),
                                            border: UiRect::all(px(1)),
                                            border_radius: studio_card_radius(),
                                            ..default()
                                        },
                                        BackgroundColor(theme.card.with_alpha(0.18)),
                                        BorderColor::all(theme.border.with_alpha(0.38)),
                                    ))
                                    .with_children(|group_card| {
                                        group_card
                                            .spawn(Node {
                                                width: percent(100),
                                                min_width: px(0),
                                                align_items: AlignItems::Center,
                                                justify_content: JustifyContent::SpaceBetween,
                                                column_gap: px(8),
                                                ..default()
                                            })
                                            .with_children(|group_header| {
                                                group_header
                                                    .spawn(Node {
                                                        min_width: px(0),
                                                        flex_grow: 1.0,
                                                        flex_direction: FlexDirection::Column,
                                                        row_gap: px(1),
                                                        ..default()
                                                    })
                                                    .with_children(|copy| {
                                                        spawn_wrapped_text(
                                                            copy,
                                                            font.clone(),
                                                            &capability.label,
                                                            9.5,
                                                            theme.foreground,
                                                        );

                                                        spawn_wrapped_text(
                                                            copy,
                                                            font.clone(),
                                                            node_card::capability_summary(
                                                                capability.id.as_str(),
                                                            ),
                                                            6.5,
                                                            theme.muted_foreground,
                                                        );
                                                        spawn_wrapped_text(
                                                            copy,
                                                            font.clone(),
                                                            format!(
                                                                "{} providers · {} enabled · {} conditional",
                                                                provider_count,
                                                                group_stats.enabled,
                                                                group_stats.conditional
                                                            ),
                                                            6.8,
                                                            theme.muted_foreground,
                                                        );
                                                    });
                                            });
                                        group_card
                                            .spawn(Node {
                                                width: percent(100),
                                                min_width: px(0),
                                                flex_direction: FlexDirection::Row,
                                                flex_wrap: FlexWrap::Wrap,
                                                align_items: AlignItems::FlexStart,
                                                column_gap: px(5),
                                                row_gap: px(5),
                                                ..default()
                                            })
                                            .with_children(|providers| {
                                                for member in group.iter().copied() {
                                                    node_card::spawn_node_card(
                                                        providers,
                                                        font.clone(),
                                                        theme,
                                                        member,
                                                        capability,
                                                        node_card::NodeCardContext {
                                                            selected: session
                                                                .selected_workflow_node
                                                                .as_ref()
                                                                == Some(&member.instance_id),
                                                            expanded: false,
                                                            embedded: true,
                                                            compact: true,
                                                            allow_drag_reorder: true,
                                                            definition: &stored.definition,
                                                            analyzer_binding: analyzer_bindings
                                                                .get(&member.instance_id)
                                                                .copied(),
                                                            audio_sources: &audio_sources,
                                                        },
                                                    );
                                                }
                                            });
                                    });
                                } else {
                                    let selected = session.selected_workflow_node.as_ref()
                                        == Some(&node.instance_id);
                                    node_card::spawn_node_card(
                                        lane,
                                        font.clone(),
                                        theme,
                                        node,
                                        capability,
                                        node_card::NodeCardContext {
                                            selected,
                                            expanded: false,
                                            embedded: false,
                                            compact: stage == 3,
                                            allow_drag_reorder: true,
                                            definition: &stored.definition,
                                            analyzer_binding: analyzer_bindings
                                                .get(&node.instance_id)
                                                .copied(),
                                            audio_sources: &audio_sources,
                                        },
                                    );
                                }
                            }

                            // Keep stage-level add/restore controls at the bottom of a
                            // full-height lane. This presentation-only spacer collapses
                            // naturally when the cards need more room.
                            lane.spawn(Node {
                                min_height: px(8),
                                flex_grow: 1.0,
                                ..default()
                            });

                            if stage == 1 {
                                stage_header::spawn_lane_section_label(
                                    lane,
                                    font.clone(),
                                    theme,
                                    "ADD PROCESSOR",
                                );
                                lane.spawn(Node {
                                    width: percent(100),
                                    column_gap: px(5),
                                    row_gap: px(5),
                                    flex_wrap: FlexWrap::Wrap,
                                    ..default()
                                })
                                .with_children(|adds| {
                                    for (label, tail, capability, model) in [
                                        (
                                            "+ Vocal denoise",
                                            &vocal_tail,
                                            "audio.denoise",
                                            Some("melband_roformer_denoise_aufr33"),
                                        ),
                                        (
                                            "+ Vocal dereverb",
                                            &vocal_tail,
                                            "audio.dereverb",
                                            Some("melband_roformer_dereverb_anvuew"),
                                        ),
                                        (
                                            "+ BGM denoise",
                                            &bgm_tail,
                                            "audio.denoise",
                                            Some("melband_roformer_denoise_aufr33"),
                                        ),
                                        (
                                            "+ BGM dereverb",
                                            &bgm_tail,
                                            "audio.dereverb",
                                            Some("melband_roformer_dereverb_anvuew"),
                                        ),
                                    ] {
                                        stage_header::spawn_quiet_add_button(
                                            adds,
                                            font.clone(),
                                            theme,
                                            label,
                                            UiAction::from(
                                                AnalysisCommand::AddWorkflowProcessor(
                                                    tail.0.to_string(),
                                                    tail.1.clone(),
                                                    capability.to_string(),
                                                    model.map(str::to_string),
                                                ),
                                            ),
                                        );
                                    }
                                });
                                spawn_wrapped_text(
                                    lane,
                                    font.clone(),
                                    "Vocal and instrumental outputs are required. Added cleanup cards remain optional and may be bypassed individually.",
                                    7.0,
                                    theme.muted_foreground,
                                );
                            } else if stage == 2 {
                                stage_header::spawn_lane_section_label(
                                    lane,
                                    font.clone(),
                                    theme,
                                    "OPTIONAL TRANSCRIPTION",
                                );
                                lane.spawn(Node {
                                    width: percent(100),
                                    column_gap: px(5),
                                    row_gap: px(5),
                                    flex_wrap: FlexWrap::Wrap,
                                    ..default()
                                })
                                .with_children(|adds| {
                                    stage_header::optional_card_add_button(
                                        adds,
                                        font.clone(),
                                        theme,
                                        &stored.definition,
                                        &vocal_tail,
                                        app_core::OptionalWorkflowCardV1::FireRedTranscript,
                                    );
                                });
                                spawn_wrapped_text(
                                    lane,
                                    font.clone(),
                                    "Online lyric acquisition remains an explicit Song Detail action. Plan Preview never downloads or writes lyrics.",
                                    7.0,
                                    theme.muted_foreground,
                                );
                            } else if stage == 3 {
                                let expert_cards = [
                                    app_core::OptionalWorkflowCardV1::RmvpePitch,
                                    app_core::OptionalWorkflowCardV1::FcpePitch,
                                    app_core::OptionalWorkflowCardV1::GameBoundary,
                                    app_core::OptionalWorkflowCardV1::BasicPitchBoundary,
                                    app_core::OptionalWorkflowCardV1::RosvotBoundary,
                                    app_core::OptionalWorkflowCardV1::StarsBoundary,
                                    app_core::OptionalWorkflowCardV1::Jbm555Boundary,
                                    app_core::OptionalWorkflowCardV1::StarsTechnique,
                                    app_core::OptionalWorkflowCardV1::AcousticDsp,
                                ];
                                let missing_experts = expert_cards
                                    .into_iter()
                                    .filter(|card| {
                                        !app_core::workflow_has_optional_card(
                                            &stored.definition,
                                            *card,
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                if !missing_experts.is_empty() {
                                    stage_header::spawn_lane_section_label(
                                        lane,
                                        font.clone(),
                                        theme,
                                        "RESTORE EXPERT",
                                    );
                                    lane.spawn(Node {
                                        width: percent(100),
                                        column_gap: px(5),
                                        row_gap: px(5),
                                        flex_wrap: FlexWrap::Wrap,
                                        ..default()
                                    })
                                    .with_children(|adds| {
                                        for card in missing_experts {
                                            stage_header::optional_card_add_button(
                                                adds,
                                                font.clone(),
                                                theme,
                                                &stored.definition,
                                                &vocal_tail,
                                                card,
                                            );
                                        }
                                    });
                                }
                            }
                        });
                }
                    });
                workspace_sidebar::spawn_workflow_sidebar(
                    workspace,
                    font.clone(),
                    theme,
                    session,
                    stored,
                    &capabilities,
                    &audio_sources,
                );
            });
        });
}

#[cfg(test)]
mod tests;
