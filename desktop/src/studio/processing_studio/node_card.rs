//! Rendering for one persisted workflow capability instance card, collapsed
//! and expanded. Presentation only: every control here dispatches the same
//! `AnalysisCommand` variants used before this module existed.

use super::*;

pub(super) struct NodeCardContext<'a> {
    pub(super) selected: bool,
    pub(super) expanded: bool,
    pub(super) embedded: bool,
    pub(super) compact: bool,
    pub(super) definition: &'a app_core::WorkflowDefinition,
    pub(super) analyzer_binding: Option<&'a app_core::AnalyzerBinding>,
    pub(super) audio_sources: &'a [(app_core::WorkflowNodeId, String, String)],
}

/// Processing Studio owns execution conditions, not Runtime Manager truth.
/// Resource/backend usability is intentionally deferred to exact Plan Preview
/// instead of being inferred from model IDs or desktop-side registries.
pub(super) fn node_execution_badge(policy: &app_core::ExecutionPolicy) -> (&'static str, Color) {
    match policy {
        app_core::ExecutionPolicy::Always => ("ENABLED", Color::srgb(0.48, 0.68, 0.95)),
        app_core::ExecutionPolicy::Conditional { .. } => {
            ("CONDITIONAL", Color::srgb(0.82, 0.67, 0.34))
        }
        app_core::ExecutionPolicy::Disabled => ("DISABLED", Color::srgb(0.58, 0.60, 0.64)),
    }
}

#[cfg(test)]
pub(super) fn provider_metadata(model_id: Option<&str>) -> String {
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

pub(super) fn policy_label(policy: &app_core::ExecutionPolicy) -> &'static str {
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

pub(super) fn execution_policy_choices() -> [(&'static str, app_core::ExecutionPolicy); 5] {
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

pub(super) fn workflow_policy_availability(
    definition: &app_core::WorkflowDefinition,
    node_id: &app_core::WorkflowNodeId,
    policy: app_core::ExecutionPolicy,
) -> Result<(), String> {
    let mut candidate = definition.clone();
    app_core::set_workflow_execution_policy(&mut candidate, node_id, policy)
}

pub(super) fn workflow_reorder_availability(
    definition: &app_core::WorkflowDefinition,
    node_id: &app_core::WorkflowNodeId,
    earlier: bool,
) -> Result<(), String> {
    let mut candidate = definition.clone();
    app_core::reorder_audio_transformation(&mut candidate, node_id, earlier)
}

pub(super) fn workflow_node_can_be_removed(
    definition: &app_core::WorkflowDefinition,
    node_id: &app_core::WorkflowNodeId,
) -> bool {
    let mut candidate = definition.clone();
    app_core::remove_workflow_node(&mut candidate, node_id).is_ok()
}

pub(super) fn workflow_model_can_be_selected(
    definition: &app_core::WorkflowDefinition,
    node_id: &app_core::WorkflowNodeId,
    model_id: &str,
) -> bool {
    let mut candidate = definition.clone();
    app_core::set_workflow_node_model(&mut candidate, node_id, model_id).is_ok()
}

pub(super) fn uses_binary_preprocessing_switch(capability: &app_core::NodeCapability) -> bool {
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
pub(super) fn is_step1_cacheable(capability_id: &str) -> bool {
    matches!(
        capability_id,
        "audio.separate_vocal_bgm" | "audio.lead_isolate" | "audio.denoise" | "audio.dereverb"
    )
}

pub(super) fn policy_choice_button(
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
            min_height: px(34),
            flex_basis: px(118),
            flex_grow: 1.0,
            flex_shrink: 1.0,
            padding: UiRect::axes(px(8), px(5)),
            border: UiRect::all(px(1)),
            border_radius: studio_card_radius(),
            overflow: Overflow::clip(),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
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

fn spawn_section_heading(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
) {
    parent
        .spawn(Node {
            width: percent(100),
            align_items: AlignItems::Center,
            column_gap: px(7),
            margin: UiRect::top(px(3)),
            ..default()
        })
        .with_children(|row| {
            spawn_text(row, font, label, 6.8, theme.muted_foreground);
            row.spawn((
                Node {
                    height: px(1),
                    flex_grow: 1.0,
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.32)),
            ));
        });
}

fn spawn_detail_fact(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    value: impl Into<String>,
    value_color: Color,
) {
    parent
        .spawn((
            Node {
                min_width: px(104),
                flex_basis: px(126),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(7)),
                row_gap: px(2),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.18)),
            BorderColor::all(theme.border.with_alpha(0.3)),
        ))
        .with_children(|fact| {
            spawn_text(fact, font.clone(), label, 6.3, theme.muted_foreground);
            spawn_wrapped_text(fact, font, value, 7.8, value_color);
        });
}

fn spawn_destructive_action_button(
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
                min_width: px(92),
                min_height: px(30),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(10), px(5)),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(theme.destructive.with_alpha(0.08)),
            BorderColor::all(theme.destructive.with_alpha(0.34)),
        ))
        .with_children(|button| {
            spawn_wrapped_text(button, font, label, 8.0, theme.destructive);
        });
}

pub(super) fn spawn_node_card(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    node: &app_core::WorkflowNodeInstance,
    capability: &app_core::NodeCapability,
    context: NodeCardContext<'_>,
) {
    let expanded = context.selected && context.expanded;
    let (status, status_color) = node_execution_badge(&node.execution_policy);
    let model_options = app_core::workflow_model_options(&node.capability_id);
    // First-tier collapsed identity is always the capability; the configured
    // provider/model is a weaker second-tier line below it (see
    // `secondary_label`) instead of being fused into one equal-weight title.
    let card_title = if context.embedded {
        node.model_id
            .as_deref()
            .map(app_core::workflow_model_label)
            .unwrap_or(capability.label.as_str())
            .to_string()
    } else {
        capability.label.clone()
    };
    let secondary_label = (!context.embedded && !expanded)
        .then_some(node.model_id.as_deref())
        .flatten()
        .map(app_core::workflow_model_label);
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
    let collapsed_min_height = if node.capability_id.as_str() == "audio.separate_vocal_bgm" {
        68
    } else if context.compact {
        50
    } else {
        58
    };
    let mut card_entity = parent.spawn((
        Node {
            position_type: PositionType::Relative,
            width: if context.embedded && context.compact && !expanded {
                percent(49)
            } else {
                percent(100)
            },
            min_width: px(0),
            min_height: px(if expanded {
                if context.compact { 54 } else { 92 }
            } else {
                collapsed_min_height
            }),
            flex_grow: if context.embedded && context.compact && !expanded {
                1.0
            } else {
                0.0
            },
            flex_basis: if context.embedded && context.compact && !expanded {
                px(150)
            } else {
                Val::Auto
            },
            padding: UiRect::all(px(if context.compact || !expanded {
                8
            } else if context.embedded {
                9
            } else {
                11
            })),
            flex_direction: FlexDirection::Column,
            row_gap: px(if context.compact { 4 } else { 6 }),
            border: UiRect::all(px(if context.selected { 2 } else { 1 })),
            border_radius: studio_card_radius(),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(if context.selected {
            theme.primary.with_alpha(0.055)
        } else if context.embedded {
            theme.background.with_alpha(0.2)
        } else {
            theme.card.with_alpha(0.24)
        }),
        BorderColor::all(if context.selected {
            theme.primary.with_alpha(0.58)
        } else if context.embedded {
            theme.border.with_alpha(0.3)
        } else {
            theme.border.with_alpha(0.4)
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
        card.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(7),
                bottom: px(7),
                width: px(if context.selected { 3 } else { 2 }),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(status_color.with_alpha(if context.selected { 0.84 } else { 0.28 })),
            Pickable::IGNORE,
        ));
            let mut header = card.spawn((
                Button,
                UiAction::from(AnalysisCommand::SelectWorkflowNode(
                    node.instance_id.to_string(),
                )),
                Node {
                    width: percent(100),
                    min_height: px(if context.compact { 24 } else { 28 }),
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
            if capability.preserves_audio_role && expanded {
                header.insert(WorkflowReorderHandle {
                    node_id: node.instance_id.clone(),
                });
            }
            header.with_children(|header| {
                spawn_wrapped_text(
                    header,
                    font.clone(),
                    &card_title,
                    if context.compact { 10.0 } else { 11.0 },
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
                        if capability.preserves_audio_role && expanded {
                            spawn_text(
                                status_group,
                                font.clone(),
                                "⋮⋮ DRAG",
                                8.0,
                                theme.primary,
                            );
                        }
                        status_group
                            .spawn((
                                Node {
                                    padding: UiRect::axes(px(6), px(2)),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(status_color.with_alpha(0.14)),
                                BorderColor::all(status_color.with_alpha(0.4)),
                            ))
                            .with_children(|badge| {
                                spawn_text(
                                    badge,
                                    font.clone(),
                                    status,
                                    if context.compact { 7.0 } else { 8.0 },
                                    status_color,
                                );
                            });
                    });
            });
            if let Some(label) = secondary_label {
                spawn_wrapped_text(card, font.clone(), label, 8.5, theme.muted_foreground);
            }
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
                if expanded {
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
                    spawn_section_heading(card, font.clone(), theme, "SEPARATION STRATEGY");
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
            } else if expanded {
                spawn_section_heading(card, font.clone(), theme, "PROVIDER / MODEL");
                card.spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    column_gap: px(6),
                    row_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|facts| {
                    spawn_detail_fact(
                        facts,
                        font.clone(),
                        theme,
                        "CONFIGURED PROVIDER",
                        node.model_id
                            .as_deref()
                            .map(app_core::workflow_model_label)
                            .unwrap_or("Studio capability logic"),
                        theme.foreground,
                    );
                    spawn_detail_fact(
                        facts,
                        font.clone(),
                        theme,
                        "RUNTIME RESOLUTION",
                        "Exact Plan Preview",
                        theme.primary,
                    );
                });
                if let Some(model_id) = node.model_id.as_deref() {
                    spawn_wrapped_text(
                        card,
                        font.clone(),
                        format!("Provider ID · {model_id}"),
                        6.8,
                        theme.muted_foreground,
                    );
                }
            }
            if expanded && model_options.len() > 1 {
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
            if expanded {
                spawn_section_heading(card, font.clone(), theme, "ROUTING");
                card.spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    column_gap: px(6),
                    row_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|facts| {
                    spawn_detail_fact(
                        facts,
                        font.clone(),
                        theme,
                        "INPUT",
                        if inputs.is_empty() { "source" } else { &inputs },
                        theme.foreground,
                    );
                    spawn_detail_fact(
                        facts,
                        font.clone(),
                        theme,
                        "OUTPUT",
                        &outputs,
                        theme.foreground,
                    );
                    spawn_detail_fact(
                        facts,
                        font.clone(),
                        theme,
                        "CONDITION",
                        policy_label(&node.execution_policy),
                        status_color,
                    );
                    spawn_detail_fact(
                        facts,
                        font.clone(),
                        theme,
                        "PRIORITY",
                        node.priority.to_string(),
                        theme.foreground,
                    );
                });
                if !capability.hard_dependencies.is_empty() {
                    spawn_wrapped_text(
                        card,
                        font.clone(),
                        format!(
                            "Requires · {}",
                            capability
                                .hard_dependencies
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        7.2,
                        theme.muted_foreground,
                    );
                }
            }
            if capability.preserves_audio_role && expanded {
                spawn_section_heading(card, font.clone(), theme, "ORDER");
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
                    width: percent(100),
                    justify_content: JustifyContent::FlexEnd,
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


            if expanded {
                let preprocessing = uses_binary_preprocessing_switch(capability);
                spawn_section_heading(
                    card,
                    font.clone(),
                    theme,
                    if preprocessing {
                        "PREPROCESSING"
                    } else {
                        "EXECUTION CONDITION"
                    },
                );
                spawn_wrapped_text(
                    card,
                    font.clone(),
                    if preprocessing {
                        "Off is a transparent bypass. Exact analyzer routing remains visible in Plan Preview."
                    } else {
                        "Choose when this expert participates. Unavailable choices would invalidate the current topology."
                    },
                    7.4,
                    theme.muted_foreground,
                );
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
                    spawn_section_heading(card, font.clone(), theme, "CACHE");
                    card.spawn(Node {
                        width: percent(100),
                        column_gap: px(6),
                        row_gap: px(6),
                        flex_wrap: FlexWrap::Wrap,
                        ..default()
                    })
                    .with_children(|cache_row| {
                        policy_choice_button(
                            cache_row,
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
                    });
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

            if capability.class == app_core::CapabilityClass::Analyzer && expanded {
                spawn_section_heading(card, font.clone(), theme, "ANALYZER SOURCE");
                if let Some(binding) = context.analyzer_binding {
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

            if expanded {
                spawn_section_heading(card, font.clone(), theme, "REMOVE CARD");
                card.spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_children(|actions| {
                    if workflow_node_can_be_removed(context.definition, &node.instance_id) {
                        spawn_destructive_action_button(
                            actions,
                            font.clone(),
                            theme,
                            "Delete",
                            UiAction::from(AnalysisCommand::RemoveWorkflowNode(
                                node.instance_id.to_string(),
                            )),
                        );
                    } else {
                        disabled_action_button(
                            actions,
                            font.clone(),
                            theme,
                            "Delete · required by the current topology",
                        );
                    }
                });
            }
        });
}
