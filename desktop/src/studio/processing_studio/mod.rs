use std::collections::BTreeMap;

use crate::studio::*;

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
                min_height: px(32),
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
            spawn_text(button, font, label, 9.0, theme.foreground);
        });
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
            "Configured implementation metadata: {model_id}. Actual resource/backend is resolved in Plan Preview."
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
    parent
        .spawn((
            Button,
            UiAction::from(AnalysisCommand::SelectWorkflowNode(
                node.instance_id.to_string(),
            )),
            Node {
                width: percent(100),
                min_height: px(112),
                padding: UiRect::all(px(12)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                border: UiRect::all(px(if context.selected { 2 } else { 1 })),
                border_radius: BorderRadius::all(px(9)),
                ..default()
            },
            BackgroundColor(
                theme
                    .card
                    .with_alpha(if context.selected { 0.72 } else { 0.4 }),
            ),
            BorderColor::all(if context.selected {
                theme.primary.with_alpha(0.72)
            } else {
                theme.border.with_alpha(0.48)
            }),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: percent(100),
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|header| {
                spawn_text(
                    header,
                    font.clone(),
                    &capability.label,
                    11.0,
                    theme.foreground,
                );
                spawn_text(header, font.clone(), status, 8.0, status_color);
            });
            spawn_wrapped_text(
                card,
                font.clone(),
                provider_metadata(node.model_id.as_deref()),
                9.0,
                theme.muted_foreground,
            );
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
            spawn_wrapped_text(
                card,
                font.clone(),
                format!(
                    "{} → {} · execution condition: {} · priority {}",
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
                        "Hard dependencies · {}",
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
            if capability.preserves_audio_role {
                card.spawn(Node {
                    column_gap: px(6),
                    ..default()
                })
                .with_children(|actions| {
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
                    action_button(
                        actions,
                        font.clone(),
                        theme,
                        "Duplicate",
                        UiAction::from(AnalysisCommand::DuplicateWorkflowNode(
                            node.instance_id.to_string(),
                        )),
                    );
                });
            }
            if capability.class == app_core::CapabilityClass::Analyzer {
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
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|actions| {
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
                });
                spawn_wrapped_text(
                    card,
                    font.clone(),
                    "Priority changes scheduling preference only; it is not a hard dependency.",
                    8.0,
                    theme.muted_foreground,
                );
                if context.selected {
                    spawn_text(
                        card,
                        font.clone(),
                        "EXECUTION CONDITION",
                        8.0,
                        theme.primary,
                    );
                    card.spawn(Node {
                        column_gap: px(6),
                        row_gap: px(6),
                        flex_wrap: FlexWrap::Wrap,
                        ..default()
                    })
                    .with_children(|policies| {
                        for (label, policy) in execution_policy_choices() {
                            let selected = node.execution_policy == policy;
                            action_button(
                                policies,
                                font.clone(),
                                theme,
                                if selected {
                                    format!("✓ {label}")
                                } else {
                                    label.to_string()
                                },
                                UiAction::from(AnalysisCommand::SetWorkflowPolicy(
                                    node.instance_id.to_string(),
                                    policy,
                                )),
                            );
                        }
                    });
                }
                if context.selected {
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
        spawn_wrapped_text(
            parent,
            font,
            "Workflow is unavailable. Return to the song and reopen Processing Studio.",
            12.0,
            theme.destructive,
        );
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
            page.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|header| {
                header
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_text(copy, font.clone(), "PROCESSING STUDIO", 9.0, theme.primary);
                        spawn_text(copy, font.clone(), "Audio & singing workflow", 18.0, theme.foreground);
                        spawn_wrapped_text(
                            copy,
                            font.clone(),
                            "Edit capability topology, role-preserving transform order, typed conditions, and artifact routing. Models & runtime owns installation; exact provider/backend readiness appears in Plan Preview.",
                            9.0,
                            theme.muted_foreground,
                        );
                    });
                header
                    .spawn(Node {
                        column_gap: px(8),
                        ..default()
                    })
                    .with_children(|actions| {
                        action_button(actions, font.clone(), theme, "Validate", UiAction::from(AnalysisCommand::PreviewWorkflow));
                        action_button(actions, font.clone(), theme, "Save workflow", UiAction::from(AnalysisCommand::SaveWorkflow));
                        action_button(actions, font.clone(), theme, "Preview run…", UiAction::from(AnalysisCommand::RunWorkflow));
                    });
            });

            page.spawn(Node {
                width: percent(100),
                column_gap: px(8),
                ..default()
            })
            .with_children(|tabs| {
                action_button(tabs, font.clone(), theme, "Processing", UiAction::from(AnalysisCommand::PreviewWorkflow));
                if let Some(hash) = session.selected_song.as_ref() {
                    action_button(tabs, font.clone(), theme, "Graph", UiAction::from(AnalysisCommand::OpenSongAnalysis(hash.clone())));
                    action_button(tabs, font.clone(), theme, "Editor", UiAction::from(LibraryCommand::OpenEditor(hash.clone())));
                }
                action_button(tabs, font.clone(), theme, "Results", UiAction::from(AppCommand::Back));
            });

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
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                column_gap: px(12),
                overflow: Overflow::scroll_y(),
                ..default()
            })
            .with_children(|workspace| {
                for (heading, class) in [
                    ("AUDIO WORKFLOW · VOCAL / BGM LANES", app_core::CapabilityClass::AudioTransformation),
                    ("ANALYSIS ATTACHMENTS", app_core::CapabilityClass::Analyzer),
                    ("FUSION & FINALIZATION", app_core::CapabilityClass::Fusion),
                ] {
                    workspace
                        .spawn((
                            Node {
                                min_width: px(260),
                                flex_basis: px(340),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(12)),
                                row_gap: px(9),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(10)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.32)),
                            BorderColor::all(theme.border.with_alpha(0.44)),
                        ))
                        .with_children(|lane| {
                            spawn_text(lane, font.clone(), heading, 8.0, theme.primary);
                            for node in stored.definition.nodes.iter().filter(|node| {
                                capabilities.get(&node.capability_id).is_some_and(|capability| {
                                    capability.class == class
                                        || (class == app_core::CapabilityClass::Fusion
                                            && capability.class == app_core::CapabilityClass::Finalization)
                                })
                            }) {
                                if let Some(capability) = capabilities.get(&node.capability_id) {
                                    node_card(
                                        lane,
                                        font.clone(),
                                        theme,
                                        node,
                                        capability,
                                        NodeCardContext {
                                            selected: session.selected_workflow_node.as_ref()
                                                == Some(&node.instance_id),
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
mod tests {
    use super::{execution_policy_choices, node_execution_badge, port_label, provider_metadata};

    #[test]
    fn processing_cards_report_typed_condition_not_resource_readiness() {
        let (always, _) = node_execution_badge(&app_core::ExecutionPolicy::Always);
        let (conditional, _) = node_execution_badge(&app_core::ExecutionPolicy::Conditional {
            condition: app_core::ConditionalExecution::DisagreementWindows,
        });
        let (disabled, _) = node_execution_badge(&app_core::ExecutionPolicy::Disabled);
        assert_eq!(always, "ENABLED");
        assert_eq!(conditional, "CONDITIONAL");
        assert_eq!(disabled, "DISABLED");
    }

    #[test]
    fn provider_is_secondary_metadata_and_plan_preview_remains_authoritative() {
        let metadata = provider_metadata(Some("rmvpe"));
        assert!(metadata.starts_with("Configured implementation metadata:"));
        assert!(metadata.contains("resolved in Plan Preview"));
        assert!(!metadata.starts_with("RMVPE"));

        let native = provider_metadata(None);
        assert!(native.contains("Studio capability logic"));
        assert!(native.contains("Plan Preview"));
    }

    #[test]
    fn every_backend_execution_condition_is_explicitly_selectable() {
        let choices = execution_policy_choices();
        assert_eq!(choices.len(), 5);
        assert!(choices.iter().any(|(_, policy)| {
            matches!(
                policy,
                app_core::ExecutionPolicy::Conditional {
                    condition: app_core::ConditionalExecution::MaximumOnly
                }
            )
        }));
        assert_eq!(choices[0].0, "Always");
        assert_eq!(choices[4].0, "Disabled");
    }

    #[test]
    fn executable_lead_isolation_labels_lead_and_residual_without_fake_stems() {
        let capability = app_core::list_workflow_capabilities()
            .into_iter()
            .find(|capability| capability.id.as_str() == "audio.lead_isolate")
            .unwrap();
        let labels = capability
            .outputs
            .iter()
            .map(port_label)
            .collect::<Vec<_>>();
        assert_eq!(labels, ["lead · LeadVocal", "residual · VocalResidual"]);
        assert!(
            !labels
                .iter()
                .any(|label| { label.contains("BackingVocal") || label.contains("HarmonyVocal") })
        );
    }
}
