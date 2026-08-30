use std::collections::{BTreeMap, BTreeSet};

use crate::studio::*;

mod stage_fusion;

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

fn uses_binary_preprocessing_switch(capability: &app_core::NodeCapability) -> bool {
    matches!(
        capability.id.as_str(),
        "audio.lead_isolate" | "audio.denoise" | "audio.dereverb"
    )
}

/// Step 1 audio-chain capabilities whose output the "skip if unchanged"
/// cache can reuse across runs (app-core resolves this when compiling the
/// Engine request). `audio.separate_vocal_bgm` is included even though it
/// can never be disabled -- unlike the other three, its own toggle only
/// controls cache reuse, not whether the step runs at all.
fn is_step1_cacheable(capability_id: &str) -> bool {
    matches!(
        capability_id,
        "audio.separate_vocal_bgm" | "audio.lead_isolate" | "audio.denoise" | "audio.dereverb"
    )
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

fn workflow_node_can_be_removed(
    definition: &app_core::WorkflowDefinition,
    node_id: &app_core::WorkflowNodeId,
) -> bool {
    let mut candidate = definition.clone();
    app_core::remove_workflow_node(&mut candidate, node_id).is_ok()
}

fn workflow_model_can_be_selected(
    definition: &app_core::WorkflowDefinition,
    node_id: &app_core::WorkflowNodeId,
    model_id: &str,
) -> bool {
    let mut candidate = definition.clone();
    app_core::set_workflow_node_model(&mut candidate, node_id, model_id).is_ok()
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

fn optional_card_add_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    definition: &app_core::WorkflowDefinition,
    source: &(app_core::WorkflowNodeId, String),
    card: app_core::OptionalWorkflowCardV1,
) {
    if app_core::workflow_has_optional_card(definition, card) {
        disabled_action_button(parent, font, theme, format!("✓ {} · present", card.label()));
    } else {
        action_button(
            parent,
            font,
            theme,
            format!("+ {}", card.label()),
            UiAction::from(AnalysisCommand::AddOptionalWorkflowCard(
                source.0.to_string(),
                source.1.clone(),
                card,
            )),
        );
    }
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

fn policy_choice_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
    action: UiAction,
    selected: bool,
    available: bool,
) {
    let background = if selected {
        theme.primary.with_alpha(0.14)
    } else if available {
        theme.card.with_alpha(0.42)
    } else {
        theme.background.with_alpha(0.28)
    };
    let border = if selected {
        theme.primary.with_alpha(0.68)
    } else if available {
        theme.border.with_alpha(0.58)
    } else {
        theme.border.with_alpha(0.32)
    };
    let mut choice = parent.spawn((
        Node {
            min_width: px(0),
            max_width: percent(100),
            min_height: px(46),
            flex_basis: px(160),
            flex_grow: 1.0,
            flex_shrink: 1.0,
            padding: UiRect::axes(px(10), px(6)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(6)),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: px(1),
            ..default()
        },
        BackgroundColor(background),
        BorderColor::all(border),
    ));
    if available && !selected {
        choice.insert((Button, action));
    } else {
        choice.insert(Pickable::IGNORE);
    }
    choice.with_children(|button| {
        button.spawn((
            Text::new(if selected {
                format!("✓  {label}")
            } else {
                label.to_string()
            }),
            ui_text_font(font.clone(), 8.5),
            TextColor(if selected {
                theme.primary
            } else if available {
                theme.foreground
            } else {
                theme.muted_foreground
            }),
            TextLayout {
                linebreak: bevy::text::LineBreak::WordOrCharacter,
                justify: Justify::Center,
            },
            Node {
                width: percent(100),
                min_width: px(0),
                min_height: px(14),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        if !available && !selected {
            spawn_text(
                button,
                font,
                "UNAVAILABLE",
                6.5,
                theme.muted_foreground.with_alpha(0.72),
            );
        }
    });
}

fn workflow_policy_availability(
    definition: &app_core::WorkflowDefinition,
    node_id: &app_core::WorkflowNodeId,
    policy: app_core::ExecutionPolicy,
) -> Result<(), String> {
    let mut candidate = definition.clone();
    app_core::set_workflow_execution_policy(&mut candidate, node_id, policy)
}

fn workflow_reorder_availability(
    definition: &app_core::WorkflowDefinition,
    node_id: &app_core::WorkflowNodeId,
    earlier: bool,
) -> Result<(), String> {
    let mut candidate = definition.clone();
    app_core::reorder_audio_transformation(&mut candidate, node_id, earlier)
}

/// Processing Studio owns execution conditions, not Runtime Manager truth.
/// Resource/backend usability is intentionally deferred to exact Plan Preview
/// instead of being inferred from model IDs or desktop-side registries.
fn node_execution_badge(policy: &app_core::ExecutionPolicy) -> (&'static str, Color) {
    match policy {
        app_core::ExecutionPolicy::Always => ("ENABLED", Color::srgb(0.48, 0.68, 0.95)),
        app_core::ExecutionPolicy::Conditional { .. } => {
            ("CONDITIONAL", Color::srgb(0.82, 0.67, 0.34))
        }
        app_core::ExecutionPolicy::Disabled => ("DISABLED", Color::srgb(0.58, 0.60, 0.64)),
    }
}

fn provider_metadata(model_id: Option<&str>) -> String {
    match model_id {
        Some(model_id) => format!(
            "Configured provider: {} ({model_id}). Actual resource/backend is resolved in Plan Preview.",
            app_core::workflow_model_label(model_id)
        ),
        None => {
            "Studio capability logic. Runtime-backed dependencies are resolved in Plan Preview."
                .to_string()
        }
    }
}

fn policy_label(policy: &app_core::ExecutionPolicy) -> &'static str {
    match policy {
        app_core::ExecutionPolicy::Always => "Always",
        app_core::ExecutionPolicy::Conditional {
            condition: app_core::ConditionalExecution::OnDisagreement,
        } => "On disagreement",
        app_core::ExecutionPolicy::Conditional {
            condition: app_core::ConditionalExecution::DisagreementWindows,
        } => "Disagreement windows",
        app_core::ExecutionPolicy::Conditional {
            condition: app_core::ConditionalExecution::MaximumOnly,
        } => "Maximum only",
        app_core::ExecutionPolicy::Disabled => "Disabled",
    }
}

fn execution_policy_choices() -> [(&'static str, app_core::ExecutionPolicy); 5] {
    [
        ("Always", app_core::ExecutionPolicy::Always),
        (
            "On disagreement",
            app_core::ExecutionPolicy::Conditional {
                condition: app_core::ConditionalExecution::OnDisagreement,
            },
        ),
        (
            "Disagreement windows",
            app_core::ExecutionPolicy::Conditional {
                condition: app_core::ConditionalExecution::DisagreementWindows,
            },
        ),
        (
            "Maximum only",
            app_core::ExecutionPolicy::Conditional {
                condition: app_core::ConditionalExecution::MaximumOnly,
            },
        ),
        ("Disabled", app_core::ExecutionPolicy::Disabled),
    ]
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

struct NodeCardContext<'a> {
    selected: bool,
    embedded: bool,
    definition: &'a app_core::WorkflowDefinition,
    analyzer_binding: Option<&'a app_core::AnalyzerBinding>,
    audio_sources: &'a [(app_core::WorkflowNodeId, String, String)],
}

fn node_card(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    node: &app_core::WorkflowNodeInstance,
    capability: &app_core::NodeCapability,
    context: NodeCardContext<'_>,
) {
    let (status, status_color) = node_execution_badge(&node.execution_policy);
    let model_options = app_core::workflow_model_options(&node.capability_id);
    let card_title = if context.embedded {
        node.model_id
            .as_deref()
            .map(app_core::workflow_model_label)
            .unwrap_or(capability.label.as_str())
            .to_string()
    } else if model_options.len() > 1 {
        node.model_id.as_deref().map_or_else(
            || capability.label.clone(),
            |model| {
                format!(
                    "{} · {}",
                    capability.label,
                    app_core::workflow_model_label(model)
                )
            },
        )
    } else {
        capability.label.clone()
    };
    let disable_availability = workflow_policy_availability(
        context.definition,
        &node.instance_id,
        app_core::ExecutionPolicy::Disabled,
    );
    let enable_availability = workflow_policy_availability(
        context.definition,
        &node.instance_id,
        app_core::ExecutionPolicy::Always,
    );
    let mut card_entity = parent.spawn((
        Node {
            width: percent(100),
            min_height: px(if context.embedded { 64 } else { 88 }),
            padding: UiRect::all(px(if context.embedded { 8 } else { 12 })),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            border: UiRect::all(px(if context.selected { 2 } else { 1 })),
            border_radius: studio_card_radius(),
            ..default()
        },
        BackgroundColor(if context.selected {
            theme.primary.with_alpha(0.12)
        } else if context.embedded {
            theme.background.with_alpha(0.24)
        } else {
            theme.card.with_alpha(STUDIO_CARD_BACKGROUND_ALPHA)
        }),
        BorderColor::all(if context.selected {
            theme.primary.with_alpha(0.72)
        } else if context.embedded {
            theme.border.with_alpha(0.32)
        } else {
            theme.border.with_alpha(STUDIO_CARD_BORDER_ALPHA)
        }),
    ));
    if capability.preserves_audio_role {
        card_entity.insert((
            Button,
            UiPointerApi(&["ui.analysis.move_workflow_node"]),
            WorkflowReorderTarget {
                node_id: node.instance_id.clone(),
            },
            WorkflowReorderHandle {
                node_id: node.instance_id.clone(),
            },
        ));
    }
    card_entity.with_children(|card| {
            let mut header = card.spawn((
                Button,
                UiAction::from(AnalysisCommand::SelectWorkflowNode(
                    node.instance_id.to_string(),
                )),
                Node {
                    width: percent(100),
                    min_height: px(28),
                    min_width: px(0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: px(8),
                    padding: UiRect::horizontal(px(2)),
                    border_radius: BorderRadius::all(px(5)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ));
            if capability.preserves_audio_role && context.selected {
                header.insert(WorkflowReorderHandle {
                    node_id: node.instance_id.clone(),
                });
            }
            header.with_children(|header| {
                spawn_wrapped_text(
                    header,
                    font.clone(),
                    &card_title,
                    11.0,
                    theme.foreground,
                );
                header
                    .spawn(Node {
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        column_gap: px(8),
                        ..default()
                    })
                    .with_children(|status_group| {
                        if capability.preserves_audio_role && context.selected {
                            spawn_text(
                                status_group,
                                font.clone(),
                                "⋮⋮ DRAG",
                                8.0,
                                theme.primary,
                            );
                        }
                        spawn_text(
                            status_group,
                            font.clone(),
                            status,
                            8.0,
                            status_color,
                        );
                    });
            });
            if node.capability_id.as_str() == "audio.separate_vocal_bgm" {
                let strategy = node
                    .separation_strategy
                    .unwrap_or(app_core::SeparationStrategyV1::IndependentSpecialists);
                let descriptor = app_core::separation_strategy_descriptor(strategy);
                spawn_wrapped_text(
                    card,
                    font.clone(),
                    format!(
                        "{} · {} providers",
                        descriptor.label,
                        descriptor.executions.len()
                    ),
                    9.0,
                    theme.muted_foreground,
                );
                if context.selected {
                    let providers = descriptor
                        .executions
                        .iter()
                        .map(|execution| {
                            format!(
                                "{} → {}",
                                app_core::workflow_model_label(execution.provider_id),
                                execution
                                    .output_roles
                                    .iter()
                                    .map(|role| role.output_port())
                                    .collect::<Vec<_>>()
                                    .join(" + ")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" · ");
                    spawn_wrapped_text(
                        card,
                        font.clone(),
                        providers,
                        8.0,
                        theme.muted_foreground,
                    );
                    spawn_text(card, font.clone(), "SEPARATION STRATEGY", 8.0, theme.primary);
                    card.spawn(Node {
                        width: percent(100),
                        column_gap: px(6),
                        row_gap: px(6),
                        flex_wrap: FlexWrap::Wrap,
                        ..default()
                    })
                    .with_children(|choices| {
                        for option in app_core::separation_strategy_options() {
                            let current = option.strategy == strategy;
                            let label = if current {
                                format!("✓ {}", option.label)
                            } else {
                                option.label.to_string()
                            };
                            if current {
                                disabled_action_button(choices, font.clone(), theme, label);
                            } else {
                                action_button(
                                    choices,
                                    font.clone(),
                                    theme,
                                    label,
                                    UiAction::from(
                                        AnalysisCommand::SetWorkflowSeparationStrategy(
                                            node.instance_id.to_string(),
                                            option.strategy,
                                        ),
                                    ),
                                );
                            }
                        }
                    });
                    spawn_wrapped_text(
                        card,
                        font.clone(),
                        "One real invocation is one execution card. Independent providers keep separate progress, logs, and model identity. Runtime readiness is resolved only in Plan Preview.",
                        7.5,
                        theme.muted_foreground,
                    );
                }
            } else if context.selected {
                spawn_wrapped_text(
                    card,
                    font.clone(),
                    provider_metadata(node.model_id.as_deref()),
                    9.0,
                    theme.muted_foreground,
                );
            }
            if context.selected && model_options.len() > 1 {
                spawn_text(card, font.clone(), "MODEL", 8.0, theme.primary);
                card.spawn(Node {
                    width: percent(100),
                    column_gap: px(6),
                    row_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|choices| {
                    for option in model_options {
                        let current = node.model_id.as_deref() == Some(option.model_id);
                        let label = if current {
                            format!("✓ {}", option.label)
                        } else {
                            option.label.to_string()
                        };
                        if !current
                            && workflow_model_can_be_selected(
                                context.definition,
                                &node.instance_id,
                                option.model_id,
                            )
                        {
                            action_button(
                                choices,
                                font.clone(),
                                theme,
                                label,
                                UiAction::from(AnalysisCommand::SetWorkflowNodeModel(
                                    node.instance_id.to_string(),
                                    option.model_id.to_string(),
                                )),
                            );
                        } else {
                            disabled_action_button(choices, font.clone(), theme, label);
                        }
                    }
                });
                spawn_wrapped_text(
                    card,
                    font.clone(),
                    "Choosing a provider already used by a sibling card swaps the providers while preserving each card's execution condition.",
                    7.5,
                    theme.muted_foreground,
                );
            }
            let inputs = capability
                .inputs
                .iter()
                .map(port_label)
                .collect::<Vec<_>>()
                .join(" + ");
            let outputs = capability
                .outputs
                .iter()
                .map(port_label)
                .collect::<Vec<_>>()
                .join(" + ");
            if context.selected {
                spawn_wrapped_text(
                    card,
                    font.clone(),
                    format!(
                        "{} → {} · {} · priority {}",
                        if inputs.is_empty() { "source" } else { &inputs },
                        outputs,
                        policy_label(&node.execution_policy),
                        node.priority,
                    ),
                    8.0,
                    theme.muted_foreground,
                );
                if !capability.hard_dependencies.is_empty() {
                    spawn_wrapped_text(
                        card,
                        font.clone(),
                        format!(
                            "Needs {}",
                            capability
                                .hard_dependencies
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        8.0,
                        theme.muted_foreground,
                    );
                }
            }
            if capability.preserves_audio_role && context.selected {
                let earlier = workflow_reorder_availability(
                    context.definition,
                    &node.instance_id,
                    true,
                );
                let later = workflow_reorder_availability(
                    context.definition,
                    &node.instance_id,
                    false,
                );
                card.spawn(Node {
                    column_gap: px(6),
                    row_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|actions| {
                    if earlier.is_ok() {
                        action_button(
                            actions,
                            font.clone(),
                            theme,
                            "Earlier",
                            UiAction::from(AnalysisCommand::MoveWorkflowNode(
                                node.instance_id.to_string(),
                                true,
                            )),
                        );
                    } else {
                        disabled_action_button(actions, font.clone(), theme, "Earlier");
                    }
                    if later.is_ok() {
                        action_button(
                            actions,
                            font.clone(),
                            theme,
                            "Later",
                            UiAction::from(AnalysisCommand::MoveWorkflowNode(
                                node.instance_id.to_string(),
                                false,
                            )),
                        );
                    } else {
                        disabled_action_button(actions, font.clone(), theme, "Later");
                    }
                });
                if earlier.is_err() && later.is_err() {
                    spawn_wrapped_text(
                        card,
                        font.clone(),
                        "Drag/reorder becomes available when another compatible role-preserving transformation is adjacent.",
                        8.0,
                        theme.muted_foreground,
                    );
                }
            }

            if context.selected {
                if workflow_node_can_be_removed(context.definition, &node.instance_id) {
                    action_button(
                        card,
                        font.clone(),
                        theme,
                        "Delete",
                        UiAction::from(AnalysisCommand::RemoveWorkflowNode(
                            node.instance_id.to_string(),
                        )),
                    );
                } else {
                    disabled_action_button(
                        card,
                        font.clone(),
                        theme,
                        "Delete · required by the current topology",
                    );
                }
            }

            if context.selected
                && capability.class == app_core::CapabilityClass::Analyzer
                && let Some(binding) = context.analyzer_binding
            {
                spawn_wrapped_text(
                    card,
                    font.clone(),
                    format!(
                        "Input artifact: {} · {}",
                        binding.source.node, binding.source.port
                    ),
                    8.0,
                    theme.primary,
                );
            }

            if context.selected {
                let preprocessing = uses_binary_preprocessing_switch(capability);
                spawn_text(
                    card,
                    font.clone(),
                    if preprocessing {
                        "PREPROCESSING SWITCH"
                    } else {
                        "WHEN THIS STEP RUNS"
                    },
                    8.0,
                    theme.primary,
                );
                spawn_wrapped_text(
                    card,
                    font.clone(),
                    if preprocessing {
                        "Turn this preprocessing step On or Off. Off is a transparent bypass; the exact analyzer input route is shown in Plan Preview."
                    } else {
                        "Choose when this step runs. The checked option is active. Dimmed options would make the current workflow invalid."
                    },
                    8.0,
                    theme.muted_foreground,
                );
                if !preprocessing {
                    spawn_wrapped_text(
                        card,
                        font.clone(),
                        "Always runs every time. On disagreement runs the full step when results conflict; Disagreement windows runs only conflicting sections. Maximum only runs in Maximum quality; Disabled skips the step.",
                        8.0,
                        theme.muted_foreground,
                    );
                }
                card.spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    column_gap: px(6),
                    row_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|actions| {
                    let currently_disabled =
                        node.execution_policy == app_core::ExecutionPolicy::Disabled;
                    let game_card = node.model_id.as_deref() == Some("game");
                    if currently_disabled {
                        if enable_availability.is_ok() {
                            action_button(
                                actions,
                                font.clone(),
                                theme,
                                if game_card {
                                    "Enable + use GAME regions"
                                } else if preprocessing {
                                    "Turn On"
                                } else {
                                    "Enable"
                                },
                                UiAction::from(AnalysisCommand::SetWorkflowPolicy(
                                    node.instance_id.to_string(),
                                    app_core::ExecutionPolicy::Always,
                                )),
                            );
                        } else {
                            disabled_action_button(
                                actions,
                                font.clone(),
                                theme,
                                "Enable · blocked",
                            );
                        }
                    } else if disable_availability.is_ok() {
                        action_button(
                            actions,
                            font.clone(),
                            theme,
                            if game_card {
                                "Disable + use F0 fallback"
                            } else if preprocessing {
                                "Turn Off"
                            } else {
                                "Disable"
                            },
                            UiAction::from(AnalysisCommand::SetWorkflowPolicy(
                                node.instance_id.to_string(),
                                app_core::ExecutionPolicy::Disabled,
                            )),
                        );
                    } else {
                        disabled_action_button(
                            actions,
                            font.clone(),
                            theme,
                            "Disable · required by downstream",
                        );
                    }
                    if capability.class == app_core::CapabilityClass::Analyzer {
                        action_button(
                            actions,
                            font.clone(),
                            theme,
                            "Priority −",
                            UiAction::from(AnalysisCommand::AdjustWorkflowPriority(
                                node.instance_id.to_string(),
                                -10,
                            )),
                        );
                        action_button(
                            actions,
                            font.clone(),
                            theme,
                            "Priority +",
                            UiAction::from(AnalysisCommand::AdjustWorkflowPriority(
                                node.instance_id.to_string(),
                                10,
                            )),
                        );
                    }
                    if is_step1_cacheable(capability.id.as_str()) {
                        policy_choice_button(
                            actions,
                            font.clone(),
                            theme,
                            "Skip if unchanged",
                            UiAction::from(AnalysisCommand::SetWorkflowSkipIfUnchanged(
                                node.instance_id.to_string(),
                                !node.skip_if_unchanged,
                            )),
                            node.skip_if_unchanged,
                            true,
                        );
                    }
                });
                if capability.class == app_core::CapabilityClass::Analyzer {
                    spawn_wrapped_text(
                        card,
                        font.clone(),
                        "Priority changes scheduling preference only; it is not a hard dependency.",
                        8.0,
                        theme.muted_foreground,
                    );
                }
                if is_step1_cacheable(capability.id.as_str()) {
                    spawn_wrapped_text(
                        card,
                        font.clone(),
                        "Skip if unchanged: on re-run, reuse this step's last successful result instead of recomputing it, as long as its inputs and settings are unchanged.",
                        8.0,
                        theme.muted_foreground,
                    );
                }
                if !preprocessing {
                    let mut unavailable_errors = BTreeSet::new();
                    card.spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    column_gap: px(6),
                    row_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|policies| {
                    for (label, policy) in execution_policy_choices() {
                        let selected = node.execution_policy == policy;
                        let availability = workflow_policy_availability(
                            context.definition,
                            &node.instance_id,
                            policy.clone(),
                        );
                        let available = availability.is_ok();
                        if !selected
                            && let Err(error) = availability
                        {
                            unavailable_errors.insert(error);
                        }
                        policy_choice_button(
                            policies,
                            font.clone(),
                            theme,
                            label,
                            UiAction::from(AnalysisCommand::SetWorkflowPolicy(
                                node.instance_id.to_string(),
                                policy,
                            )),
                            selected,
                            available,
                        );
                    }
                });
                if !unavailable_errors.is_empty() {
                    let downstream_requires_always = unavailable_errors.iter().any(|error| {
                        error.contains("depends only on conditional or disabled nodes")
                    });
                    let explanation = if downstream_requires_always {
                        "Why these options are unavailable: a downstream step requires at least one evidence producer that always runs. This step is currently that guaranteed producer; making it conditional or disabled would leave the required input unavailable. Set another compatible producer to Always first."
                            .to_string()
                    } else {
                        format!(
                            "Unavailable in the current topology · {}",
                            unavailable_errors.into_iter().collect::<Vec<_>>().join(" · ")
                        )
                    };
                    spawn_wrapped_text(
                        card,
                        font.clone(),
                        explanation,
                        8.0,
                        theme.muted_foreground,
                    );
                }
                }
            }

            if capability.class == app_core::CapabilityClass::Analyzer && context.selected {
                card.spawn(Node {
                    column_gap: px(6),
                    row_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|sources| {
                    for (source_node, source_port, label) in context.audio_sources {
                        if context.analyzer_binding.is_some_and(|binding| {
                            binding.source.node == *source_node
                                && binding.source.port == *source_port
                        }) {
                            continue;
                        }
                        action_button(
                            sources,
                            font.clone(),
                            theme,
                            format!("Use {label}"),
                            UiAction::from(AnalysisCommand::RebindWorkflowAnalyzer(
                                node.instance_id.to_string(),
                                source_node.to_string(),
                                source_port.clone(),
                            )),
                        );
                    }
                });
            }
        });
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
        .spawn(Node {
            min_width: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(18)),
            row_gap: px(14),
            ..default()
        })
        .with_children(|page| {
            if let Some(error) = session.workflow_compile_error.as_ref() {
                spawn_wrapped_text(page, font.clone(), error, 9.0, theme.destructive);
            } else {
                spawn_wrapped_text(
                    page,
                    font.clone(),
                    format!(
                        "Workflow revision {} · {:?} · local compile valid; execution still requires exact Engine preview",
                        stored.definition.revision, stored.definition.quality_mode
                    ),
                    9.0,
                    theme.primary,
                );
            }
            page.spawn(Node {
                width: percent(100),
                min_height: px(18),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: px(10),
                ..default()
            })
            .with_children(|status| {
                status
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        ..default()
                    })
                    .with_children(|notice| {
                        spawn_wrapped_text(
                            notice,
                            font.clone(),
                            session.notice.as_deref().unwrap_or(""),
                            9.0,
                            theme.foreground,
                        );
                    });
                spawn_compact_action_button(
                    status,
                    font.clone(),
                    theme,
                    "Re-run",
                    UiAction::from(AnalysisCommand::RunWorkflow),
                );
            });

            page.spawn((
                ProcessingStudioScroll,
                ScrollPosition(Vec2::new(
                    0.0,
                    session.processing_studio_scroll_offset,
                )),
                Node {
                    width: percent(100),
                    min_width: px(0),
                    min_height: px(0),
                    flex_grow: 1.0,
                    column_gap: px(8),
                    flex_wrap: FlexWrap::NoWrap,
                    align_items: AlignItems::FlexStart,
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
                for (stage, heading, description) in [
                    (1u8, "01 · PRE-PROCESSING", "Separate the source and optionally clean either audio branch."),
                    (2u8, "02 · LYRICS", "Transcribe, combine lyric evidence, then align it to the song."),
                    (3u8, "03 · PITCH & NOTE EXPERTS", "Multiple providers for the same purpose are grouped together."),
                    (4u8, "04 · FINAL FUSION", "Combine evidence into the candidate singing track."),
                ] {
                    workspace
                        .spawn((
                            Node {
                                min_width: px(0),
                                flex_basis: px(0),
                                flex_grow: 1.0,
                                flex_shrink: 1.0,
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(10)),
                                row_gap: px(8),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(10)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.32)),
                            BorderColor::all(theme.border.with_alpha(0.44)),
                        ))
                        .with_children(|lane| {
                            spawn_text(lane, font.clone(), heading, 8.0, theme.primary);
                            spawn_wrapped_text(
                                lane,
                                font.clone(),
                                description,
                                8.5,
                                theme.muted_foreground,
                            );

                            if stage == 1 {
                                lane.spawn(Node {
                                    width: percent(100),
                                    column_gap: px(6),
                                    row_gap: px(6),
                                    flex_wrap: FlexWrap::Wrap,
                                    ..default()
                                })
                                .with_children(|adds| {
                                    for (label, tail, capability, model) in [
                                        ("+ Vocal denoise", &vocal_tail, "audio.denoise", Some("melband_roformer_denoise_aufr33")),
                                        ("+ Vocal dereverb", &vocal_tail, "audio.dereverb", Some("melband_roformer_dereverb_anvuew")),
                                        ("+ BGM denoise", &bgm_tail, "audio.denoise", Some("melband_roformer_denoise_aufr33")),
                                        ("+ BGM dereverb", &bgm_tail, "audio.dereverb", Some("melband_roformer_dereverb_anvuew")),
                                    ] {
                                        action_button(
                                            adds,
                                            font.clone(),
                                            theme,
                                            label,
                                            UiAction::from(AnalysisCommand::AddWorkflowProcessor(
                                                tail.0.to_string(),
                                                tail.1.clone(),
                                                capability.to_string(),
                                                model.map(str::to_string),
                                            )),
                                        );
                                    }
                                });
                                spawn_wrapped_text(
                                    lane,
                                    font.clone(),
                                    "Vocal and BGM are required outputs. Cleanup processors are optional and can be disabled after they are added.",
                                    8.0,
                                    theme.muted_foreground,
                                );
                            } else if stage == 2 {
                                if let Some(status) = session
                                    .selected_song
                                    .as_deref()
                                    .and_then(app_core::canonical_lyrics_status)
                                {
                                    let count = status.line_count.to_string();
                                    let message = match status.source {
                                        app_core::CanonicalLyricsSource::Plain => {
                                            localized_message(
                                                session.config,
                                                UiMessage::CanonicalLyricsAvailablePlain,
                                                &[("{count}", &count)],
                                            )
                                        }
                                        app_core::CanonicalLyricsSource::TimedLrc => {
                                            localized_message(
                                                session.config,
                                                UiMessage::CanonicalLyricsAvailableTimedLrc,
                                                &[("{count}", &count)],
                                            )
                                        }
                                    };
                                    spawn_wrapped_text(lane, font.clone(), message, 8.0, theme.primary);
                                }
                                spawn_wrapped_text(
                                    lane,
                                    font.clone(),
                                    "Online lyric acquisition is an explicit Song Detail action. Plan Preview never downloads or writes lyrics; only user-confirmed supplied text becomes canonical input.",
                                    8.0,
                                    theme.muted_foreground,
                                );
                                spawn_text(
                                    lane,
                                    font.clone(),
                                    "ADD OPTIONAL LYRICS PROCESSOR",
                                    8.0,
                                    theme.primary,
                                );
                                lane.spawn(Node {
                                    width: percent(100),
                                    column_gap: px(6),
                                    row_gap: px(6),
                                    flex_wrap: FlexWrap::Wrap,
                                    ..default()
                                })
                                .with_children(|adds| {
                                    optional_card_add_button(
                                        adds,
                                        font.clone(),
                                        theme,
                                        &stored.definition,
                                        &vocal_tail,
                                        app_core::OptionalWorkflowCardV1::FireRedTranscript,
                                    );
                                });
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
                                if missing_experts.is_empty() {
                                    spawn_wrapped_text(
                                        lane,
                                        font.clone(),
                                        format!("{} experts configured", expert_cards.len()),
                                        8.0,
                                        theme.muted_foreground,
                                    );
                                } else {
                                    spawn_text(
                                        lane,
                                        font.clone(),
                                        "RESTORE MISSING EXPERTS",
                                        8.0,
                                        theme.primary,
                                    );
                                    lane.spawn(Node {
                                        width: percent(100),
                                        column_gap: px(6),
                                        row_gap: px(6),
                                        flex_wrap: FlexWrap::Wrap,
                                        ..default()
                                    })
                                    .with_children(|adds| {
                                        for card in missing_experts {
                                            optional_card_add_button(
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
                            } else if stage == 4 {
                                stage_fusion::spawn_fusion_stage_card(
                                    lane,
                                    font.clone(),
                                    theme,
                                    stored,
                                    stage_fusion::fusion_adapter_readiness(session),
                                );
                            }

                            if stage == 4 {
                                return;
                            }

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
                                    for member in &group {
                                        rendered.insert(member.instance_id.clone());
                                    }
                                    lane.spawn((
                                        Node {
                                            width: percent(100),
                                            flex_direction: FlexDirection::Column,
                                            padding: UiRect::all(px(10)),
                                            row_gap: px(7),
                                            border: UiRect::all(px(1)),
                                            border_radius: studio_card_radius(),
                                            ..default()
                                        },
                                        BackgroundColor(theme.card.with_alpha(0.24)),
                                        BorderColor::all(theme.border.with_alpha(0.48)),
                                    ))
                                    .with_children(|group_card| {
                                        spawn_text(
                                            group_card,
                                            font.clone(),
                                            &capability.label,
                                            11.0,
                                            theme.foreground,
                                        );
                                        spawn_wrapped_text(
                                            group_card,
                                            font.clone(),
                                            format!(
                                                "{} providers · choose one to edit its model and execution condition",
                                                group.len()
                                            ),
                                            8.0,
                                            theme.muted_foreground,
                                        );
                                        for member in group {
                                            node_card(
                                                group_card,
                                                font.clone(),
                                                theme,
                                                member,
                                                capability,
                                                NodeCardContext {
                                                    selected: session
                                                        .selected_workflow_node
                                                        .as_ref()
                                                        == Some(&member.instance_id),
                                                    embedded: true,
                                                    definition: &stored.definition,
                                                    analyzer_binding: analyzer_bindings
                                                        .get(&member.instance_id)
                                                        .copied(),
                                                    audio_sources: &audio_sources,
                                                },
                                            );
                                        }
                                    });
                                } else {
                                    node_card(
                                        lane,
                                        font.clone(),
                                        theme,
                                        node,
                                        capability,
                                        NodeCardContext {
                                            selected: session.selected_workflow_node.as_ref()
                                                == Some(&node.instance_id),
                                            embedded: false,
                                            definition: &stored.definition,
                                            analyzer_binding: analyzer_bindings
                                                .get(&node.instance_id)
                                                .copied(),
                                            audio_sources: &audio_sources,
                                        },
                                    );
                                }
                            }
                        });
                }
            });
        });
}

#[cfg(test)]
mod tests;
