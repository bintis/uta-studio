//! Activity center: analysis session graph, stage nodes, and history.

use crate::studio::*;

#[derive(Resource)]
pub(crate) struct AnalysisRefreshTimer(pub(crate) Timer);

#[derive(Component, Clone, Copy)]
pub(crate) struct AnalysisGraphViewport {
    pub(crate) unscaled_width: f32,
    pub(crate) unscaled_height: f32,
}

#[derive(Component)]
pub(crate) struct AnalysisLogViewerScroll;

#[derive(Component)]
pub(crate) struct PlanPreviewScroll;

#[derive(Clone)]
pub(crate) struct AnalysisNodeMenuAction {
    pub(crate) label: &'static str,
    pub(crate) action: UiAction,
}

/// Context actions are capability-driven. In particular, the analyzer has
/// only stem/pitch/lyrics execution groups, so no menu item claims to run a
/// child node "only" when the backend will actually execute its whole group.
#[derive(Clone)]
pub(crate) struct AnalysisNodeContextMenu {
    pub(crate) node_id: String,
    /// Bucket stage id ("separation", "pitch", ...) for "View in
    /// inspector" -- a different string space from `node_id` (the real
    /// dotted `AnalysisNodeId`); `selected_analysis_stage` is keyed by the
    /// former throughout the rest of this module.
    pub(crate) stage_id: String,
    pub(crate) label: String,
    pub(crate) retry_action: Option<AnalysisNodeMenuAction>,
    pub(crate) run_downstream_action: Option<AnalysisNodeMenuAction>,
    /// `None` when `app_core::node_can_be_disabled_for_run` refuses this
    /// node -- the button is omitted rather than offered and guaranteed to
    /// error.
    pub(crate) disable_node_action: Option<UiAction>,
    /// `None` when `app_core::node_can_be_frozen_for_run` refuses this node
    /// -- either it has no standalone freezable output at all
    /// (`pipeline_can_honor_freeze`), or it's structurally freezable but
    /// this song doesn't have that output on disk yet.
    pub(crate) freeze_node_action: Option<UiAction>,
    /// `None` when `app_core::node_can_be_bypassed_for_run` refuses this
    /// node (i.e. it isn't `stems.separate` -- no other node has an
    /// alternate input to route through yet).
    pub(crate) bypass_node_action: Option<UiAction>,
    /// `None` when no history run is currently selected -- "Compare with
    /// previous attempt" needs a `current_run_id` to diff against, which
    /// only exists once a run is selected in the Activity/Queue view.
    pub(crate) compare_node_action: Option<UiAction>,
    /// Phase 8: `None` when `app_core::node_can_be_configured_for_run`
    /// refuses this node -- i.e. it has no real legacy profile-controlled
    /// parameter (`lyrics.transcribe`/`lyrics.align` only; BGM and vocal
    /// model selection live in the audio-processing snapshot). "Save as
    /// song profile" fires immediately (no dialog --
    /// it persists whatever value is currently in effect); "Configure for
    /// this run…" opens `NativeNodeConfigDialog` since it needs a new value
    /// picked first.
    pub(crate) save_as_song_profile_action: Option<UiAction>,
    pub(crate) open_configure_dialog_action: Option<UiAction>,
    /// §8.3 migration table's "Force transcribe -> Transcription Node ->
    /// Force Recompute": only offered for `lyrics.transcribe` (the only
    /// node "ignore online lyrics, transcribe again" is meaningful for).
    /// Reuses the existing `UiAction::ForceTranscribe`/
    /// `app_core::reanalyze_force_transcribe` Song Detail already calls --
    /// not a new backend capability, just a second, DAG-side entry point
    /// for the same real action.
    pub(crate) force_transcribe_action: Option<UiAction>,
    /// §8.3 migration table's "Refetch & align -> Lyrics Source -> LRCLIB
    /// -> Run Timing": only offered for `lyrics.align` -- distinct from
    /// that node's existing "Retry with same configuration"
    /// (`RealignSong`, realigns against whatever lyrics are already set)
    /// the way Song Detail's "Word timing" (Realign) and "Lyrics source"
    /// (Refetch & align) are two separate rows for the same reason. Reuses
    /// the existing `UiAction::ReanalyzeTranscript`/
    /// `app_core::reanalyze_transcript` Song Detail already calls.
    pub(crate) refetch_align_action: Option<UiAction>,
    /// PreprocessedAudio is ephemeral unless the user explicitly requests
    /// retention. Only the real `lyrics.preprocess` boundary offers this.
    pub(crate) capture_intermediate_action: Option<UiAction>,
    /// §7.5's last item, "View logs": always offered because a node filter is
    /// meaningful for every node. Opens the selected run's dedicated JSONL
    /// log; legacy runs without `log_path` show an explicit empty state.
    pub(crate) view_logs_action: Option<UiAction>,
    /// §7.3 "Music Analysis 支持展开": `(button label, action)`, `None` for
    /// every non-compound node. The label flips between "Expand
    /// sub-checks"/"Collapse sub-checks" depending on current state so the
    /// same button always describes what clicking it does next, rather than
    /// the state it's currently in.
    pub(crate) compound_toggle: Option<(&'static str, UiAction)>,
    pub(crate) position: Vec2,
}

/// Phase 8 "Configure for this run…": a draft one-run override for a single
/// node's profile-controlled field. Modeled directly on
/// `song_detail.rs::NativeLanguageEditor`/`spawn_language_editor` -- opened
/// pre-filled with the node's current *effective* value (same
/// `resolve_profile_field` result the inspector's PARAMETER SOURCE fact
/// uses), so the dialog and the inspector never disagree about what's
/// currently in effect.
pub(crate) struct NativeNodeConfigDialog {
    pub(crate) file_hash: String,
    pub(crate) node_id: String,
    pub(crate) field: app_core::ProfileField,
    pub(crate) value: String,
    pub(crate) picker_open: bool,
}

/// Top-level controls shown by Plan Preview. Vocal Preprocessing is an
/// internal hand-off, not a useful user choice here. The two selected vocal
/// cleanup stages are displayed directly beneath Stem Separation instead.
pub(crate) const PLAN_PREVIEW_DISABLEABLE_NODES: &[&str] = &[
    "stems.separate",
    "pitch.extract",
    "lyrics.transcribe",
    "lyrics.align",
    "lyrics.import_timed",
];

/// Phase 7/8 Plan Preview panel: a staged, not-yet-committed disabled-node
/// combination for one song. Purely additive -- doesn't change how any
/// existing immediate-fire Node Context Menu action behaves; this is a
/// separate, more deliberate multi-node staging tool.
pub(crate) struct PlanPreviewDraft {
    pub(crate) file_hash: String,
    pub(crate) disabled_nodes: std::collections::BTreeSet<app_core::AnalysisNodeId>,
}

/// Buckets a plan's nodes into the phase-plan's real, non-fabricated
/// categories (`uta-studio-analysis-dag-phases.md` §7.7's worked example --
/// "Will run: Pitch Analysis, Build Candidate Chart / Will reuse: Music
/// Analysis, Vocal Stem..."). `NotApplicable` (inactive-route) nodes are
/// omitted -- route-irrelevant noise, matching existing precedent
/// elsewhere. `Frozen`/`Stale`/`Failed` never appear here: this pure
/// function only reads what `plan` actually contains, and the Plan Preview
/// panel never stages Freeze or applies the Failed/Stale overlays the
/// canvas separately adds -- so those buckets are correctly absent, not
/// silently dropped.
pub(crate) fn plan_preview_groups(
    plan: &app_core::AnalysisPlan,
) -> Vec<(&'static str, Vec<String>)> {
    let mut will_run = Vec::new();
    let mut will_reuse = Vec::new();
    let mut blocked = Vec::new();
    let mut disabled = Vec::new();
    for node in &plan.nodes {
        let label = node.id.to_string();
        if node.will_run {
            will_run.push(label);
            continue;
        }
        match node.state {
            app_core::NodeState::Ready => will_reuse.push(label),
            app_core::NodeState::Blocked => blocked.push(label),
            app_core::NodeState::Disabled => disabled.push(label),
            _ => {}
        }
    }
    [
        ("Will run", will_run),
        ("Will reuse", will_reuse),
        ("Blocked", blocked),
        ("Disabled", disabled),
    ]
    .into_iter()
    .filter(|(_, nodes)| !nodes.is_empty())
    .collect()
}

/// §7.5's "View logs" dialog state -- which run-scoped analysis log and
/// node filter should be shown.
pub(crate) struct AnalysisLogViewerState {
    pub(crate) file_hash: String,
    pub(crate) node_id: String,
}

pub(crate) fn spawn_analysis_log_viewer(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    state: &AnalysisLogViewerState,
    selected_run_id: Option<i64>,
) {
    let lines =
        app_core::analysis_log_lines(selected_run_id, &state.file_hash, Some(&state.node_id), 240);
    let log_path = app_core::analysis_log_path_for(selected_run_id, &state.file_hash);
    let header = if log_path.is_some() {
        format!("Dedicated analysis log · node {}", state.node_id)
    } else {
        "No dedicated log was recorded for this legacy analysis run".to_string()
    };

    // Click-outside-to-close backdrop, same pattern as the Plan Preview
    // dialog's -- and a real fix, not cosmetic: the dialog previously had
    // no dismiss handler outside its own "Close" button at all, and with
    // up to 80 log lines (`get_recent_logs(80)`) that button could scroll
    // out of reach in the first place (see below).
    parent.spawn((
        Button,
        UiAction::from(AnalysisCommand::CloseAnalysisLogViewer),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(92),
    ));
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
            ZIndex(93),
            Pickable::IGNORE,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(620),
                        max_height: percent(84),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(8),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(dialog, font.clone(), "VIEW LOGS", 8.0, theme.primary);
                    spawn_text(
                        dialog,
                        font.clone(),
                        format!("{} -- {}", state.node_id, state.file_hash),
                        13.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(dialog, font.clone(), header, 10.0, theme.muted_foreground);
                    spawn_text(
                        dialog,
                        font.clone(),
                        "Ctrl + wheel to scroll",
                        8.0,
                        theme.muted_foreground,
                    );
                    dialog.spawn(Node {
                        height: px(4),
                        ..default()
                    });
                    // The scrollable region, separate from the dialog's own
                    // (unscrolled) heading and action row below -- so
                    // "Close"/"Open log file" stay reachable regardless of
                    // how many lines there are, instead of themselves being
                    // part of what has to be scrolled past.
                    dialog
                        .spawn((
                            AnalysisLogViewerScroll,
                            Node {
                                min_height: px(0),
                                flex_direction: FlexDirection::Column,
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                            ScrollPosition::default(),
                        ))
                        .with_children(|scroll| {
                            // A scrollable flex column shrinks its direct
                            // children to the viewport height before
                            // measuring overflow (same fix already used for
                            // Settings' own scrollable content) -- wrap the
                            // real lines in one intrinsic-height child so
                            // they keep their real height and the region
                            // scrolls instead of squashing everything to fit.
                            scroll
                                .spawn(Node {
                                    width: percent(100),
                                    flex_shrink: 0.0,
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(4),
                                    ..default()
                                })
                                .with_children(|body| {
                                    if lines.is_empty() {
                                        spawn_text(
                                            body,
                                            font.clone(),
                                            "No log lines captured yet.",
                                            10.0,
                                            theme.muted_foreground,
                                        );
                                    }
                                    for line in &lines {
                                        spawn_wrapped_text(
                                            body,
                                            font.clone(),
                                            line.text.clone(),
                                            9.0,
                                            theme.foreground,
                                        );
                                    }
                                });
                        });
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            flex_shrink: 0.0,
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            margin: UiRect::top(px(10)),
                            ..default()
                        })
                        .with_children(|actions| {
                            if log_path.is_some() {
                                spawn_text_button(
                                    actions,
                                    font.clone(),
                                    theme,
                                    "Open log file",
                                    10.0,
                                    UiAction::from(AnalysisCommand::OpenAnalysisLogFile),
                                );
                            }
                            spawn_action_button(
                                actions,
                                font,
                                theme,
                                "Close",
                                UiAction::from(AnalysisCommand::CloseAnalysisLogViewer),
                            );
                        });
                });
        });
}

/// Display label for a `ProfileField` -- same strings
/// `selected_stage_parameter` already uses for the PARAMETER fact row, kept
/// consistent so the dialog and the inspector call the same knob the same
/// name.
pub(crate) fn profile_field_label(field: app_core::ProfileField) -> &'static str {
    match field {
        app_core::ProfileField::Separator => "SEPARATOR",
        app_core::ProfileField::AsrEngine => "ASR ENGINE",
        app_core::ProfileField::AlignmentBackend => "ALIGNMENT BACKEND",
    }
}

/// The `SettingsSelectKind` whose option list/labels a `ProfileField`
/// reuses -- no new option lists invented for this dialog, just the
/// existing Settings tab ones (`settings_select_options`/
/// `settings_select_label`) pointed at a different value source.
pub(crate) fn profile_field_settings_kind(field: app_core::ProfileField) -> SettingsSelectKind {
    match field {
        app_core::ProfileField::Separator => SettingsSelectKind::Separator,
        app_core::ProfileField::AsrEngine => SettingsSelectKind::AsrEngine,
        app_core::ProfileField::AlignmentBackend => SettingsSelectKind::AlignBackend,
    }
}

pub(crate) fn spawn_node_config_dialog(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    dialog: &NativeNodeConfigDialog,
    intel_backend: bool,
    notice: Option<&str>,
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
            BackgroundColor(theme.background.with_alpha(0.78)),
            ZIndex(92),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(470),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(11),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|body| {
                    spawn_text(
                        body,
                        font.clone(),
                        profile_field_label(dialog.field),
                        8.0,
                        theme.primary,
                    );
                    spawn_text(
                        body,
                        font.clone(),
                        format!("Configure {} for this run", dialog.node_id),
                        17.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        body,
                        font.clone(),
                        "Applies only to the next run of this node -- it is not saved. Use \"Save as song profile\" from the node's menu to persist a choice.",
                        10.0,
                        theme.muted_foreground,
                    );
                    let kind = profile_field_settings_kind(dialog.field);
                    let options = settings_select_options(kind, intel_backend);
                    let current_label = settings_select_label(kind, &dialog.value);
                    body.spawn((
                        Button,
                        UiAction::from(AnalysisCommand::ToggleNodeConfigPicker),
                        Node {
                            width: percent(100),
                            height: px(40),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(11)),
                            column_gap: px(8),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.65)),
                        BorderColor::all(if dialog.picker_open {
                            theme.primary.with_alpha(0.64)
                        } else {
                            theme.border.with_alpha(0.72)
                        }),
                    ))
                    .with_children(|selector| {
                        spawn_text(selector, font.clone(), current_label, 11.0, theme.foreground);
                        selector.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        spawn_text(
                            selector,
                            font.clone(),
                            if dialog.picker_open { "^" } else { "v" },
                            9.0,
                            theme.primary,
                        );
                    });
                    if dialog.picker_open {
                        body.spawn((
                            Node {
                                width: percent(100),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(5)),
                                row_gap: px(2),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.82)),
                            BorderColor::all(theme.border.with_alpha(0.72)),
                        ))
                        .with_children(|picker| {
                            for (value, label) in options {
                                let selected = dialog.value == *value;
                                picker
                                    .spawn((
                                        Button,
                                        UiAction::from(AnalysisCommand::SelectNodeConfigValue((*value).into())),
                                        Node {
                                            width: percent(100),
                                            min_height: px(30),
                                            align_items: AlignItems::Center,
                                            padding: UiRect::horizontal(px(9)),
                                            border_radius: BorderRadius::all(px(4)),
                                            ..default()
                                        },
                                        BackgroundColor(if selected {
                                            theme.primary.with_alpha(0.13)
                                        } else {
                                            Color::NONE
                                        }),
                                    ))
                                    .with_children(|option| {
                                        spawn_text(
                                            option,
                                            font.clone(),
                                            *label,
                                            9.0,
                                            if selected {
                                                theme.foreground
                                            } else {
                                                theme.muted_foreground
                                            },
                                        );
                                    });
                            }
                        });
                    }
                    if let Some(notice) = notice {
                        spawn_wrapped_text(body, font.clone(), notice, 9.0, theme.destructive);
                    }
                    body.spawn(Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    })
                    .with_children(|actions| {
                        spawn_text_button(
                            actions,
                            font.clone(),
                            theme,
                            "Cancel",
                            10.0,
                            UiAction::from(AnalysisCommand::CloseNodeConfigDialog),
                        );
                        spawn_action_button(
                            actions,
                            font,
                            theme,
                            "Run with this configuration",
                            UiAction::from(AnalysisCommand::RunNodeConfigDialog),
                        );
                    });
                });
        });
}

/// Escape closes the Plan Preview dialog, same idea as
/// `handle_library_search_keyboard`'s Escape-closes-search handling.
pub(crate) fn handle_plan_preview_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut dialogs: ResMut<DialogState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if dialogs.plan_preview_draft.is_some() {
        dialogs.plan_preview_draft = None;
        invalidated.invalidate(UiDirtyRegion::Analysis);
    } else if dialogs.analysis_log_viewer.is_some() {
        dialogs.analysis_log_viewer = None;
        invalidated.invalidate(UiDirtyRegion::Analysis);
    }
}

/// The preview contains both stage choices and the resolved model plan. Keep
/// it inside short windows and make the complete contents reachable with an
/// ordinary wheel gesture while the modal is open.
pub(crate) fn handle_plan_preview_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    dialogs: Res<DialogState>,
    mut panels: Query<(&ComputedNode, &mut ScrollPosition), With<PlanPreviewScroll>>,
) {
    if dialogs.plan_preview_draft.is_none() {
        return;
    }
    let Ok((computed, mut position)) = panels.single_mut() else {
        return;
    };
    let mut delta = 0.0;
    for event in wheel.read() {
        let scale = match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 24.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
        delta -= event.y * scale;
    }
    if delta.abs() < f32::EPSILON {
        return;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
}

/// Ctrl+wheel scrolls the log viewer's line list -- same modifier
/// `handle_analysis_graph_scroll` now requires, and for the same reason:
/// this dialog sits on top of the library's own scrollable song list, so a
/// bare wheel wasn't just doing nothing here, it was reaching through to
/// scroll the list underneath instead.
pub(crate) fn handle_analysis_log_viewer_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    dialogs: Res<DialogState>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<AnalysisLogViewerScroll>>,
) {
    if dialogs.analysis_log_viewer.is_none() {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if !ctrl {
        wheel.clear();
        return;
    }
    let Ok((computed, mut position)) = lists.single_mut() else {
        wheel.clear();
        return;
    };
    let mut delta = 0.0;
    for event in wheel.read() {
        let scale = match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 22.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
        delta -= event.y * scale;
    }
    if delta.abs() < f32::EPSILON {
        return;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    let max = (content.y - size.y).max(0.0);
    position.y = (position.y + delta).clamp(0.0, max);
}

pub(crate) fn spawn_plan_preview_dialog(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    draft: &PlanPreviewDraft,
    notice: Option<&str>,
) {
    let graph = app_core::baseline_graph_spec();
    let node_label = |node_id: &str| -> String {
        graph
            .node(&app_core::AnalysisNodeId::new(node_id))
            .map(|spec| spec.label.clone())
            .unwrap_or_else(|| node_id.to_string())
    };
    let plan = app_core::preview_analysis_plan_for_selection(
        &draft.file_hash,
        draft.disabled_nodes.clone(),
    )
    .ok();

    // Click-outside-to-close backdrop: a full-screen `Button` sibling
    // *behind* the centered dialog, same pattern as
    // `spawn_analysis_node_context_menu`'s dismiss layer. The centering
    // wrapper spawned below it is `Pickable::IGNORE` so it doesn't itself
    // swallow clicks meant for this backdrop outside the 520px panel.
    parent.spawn((
        Button,
        UiAction::from(AnalysisCommand::ClosePlanPreview),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(92),
    ));

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
            ZIndex(93),
            Pickable::IGNORE,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(560),
                        height: percent(84),
                        max_height: px(760),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(11),
                        overflow: Overflow::scroll_y(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    PlanPreviewScroll,
                    ScrollPosition::default(),
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|body| {
                    spawn_text(body, font.clone(), "PLAN PREVIEW", 8.0, theme.primary);
                    spawn_text(
                        body,
                        font.clone(),
                        "Preview a hypothetical run",
                        17.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        body,
                        font.clone(),
                        "Toggle nodes off to see how the default full run would change, without queuing anything yet. Target and route stay at their defaults.",
                        10.0,
                        theme.muted_foreground,
                    );
                    for node_id in PLAN_PREVIEW_DISABLEABLE_NODES {
                        let is_disabled = draft
                            .disabled_nodes
                            .contains(&app_core::AnalysisNodeId::new(*node_id));
                        body.spawn((
                            Button,
                            UiAction::from(AnalysisCommand::TogglePlanPreviewDisabledNode((*node_id).to_string())),
                            Node {
                                width: percent(100),
                                min_height: px(36),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::SpaceBetween,
                                padding: UiRect::horizontal(px(11)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.65)),
                            BorderColor::all(theme.border.with_alpha(0.72)),
                        ))
                        .with_children(|row| {
                            spawn_text(row, font.clone(), node_label(node_id), 10.0, theme.foreground);
                            spawn_text(
                                row,
                                font.clone(),
                                if is_disabled { "Disabled" } else { "Enabled" },
                                9.0,
                                if is_disabled {
                                    theme.editor_warning
                                } else {
                                    theme.muted_foreground
                                },
                            );
                        });
                        if *node_id == "stems.separate" {
                            for (child_id, step_id) in [
                                ("vocals.denoise", "denoise_vocals"),
                                ("vocals.dereverb", "dereverb_vocals"),
                                ("instrumental.denoise", "denoise_accompaniment"),
                                ("instrumental.dereverb", "dereverb_accompaniment"),
                            ] {
                                let selected = plan
                                    .as_ref()
                                    .and_then(|plan| plan.audio_processing.as_ref())
                                    .is_some_and(|audio| {
                                        audio.steps.iter().any(|step| step.step_id == step_id)
                                    });
                                body.spawn((
                                    Node {
                                        width: percent(100),
                                        min_height: px(31),
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::SpaceBetween,
                                        padding: UiRect::new(px(28), px(11), px(0), px(0)),
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(px(5)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.background.with_alpha(0.34)),
                                    BorderColor::all(theme.border.with_alpha(0.45)),
                                    Pickable::IGNORE,
                                ))
                                .with_children(|row| {
                                    spawn_text(
                                        row,
                                        font.clone(),
                                        format!("↳ {}", node_label(child_id)),
                                        9.0,
                                        theme.foreground,
                                    );
                                    spawn_text(
                                        row,
                                        font.clone(),
                                        if selected { "In plan" } else { "Not selected" },
                                        8.0,
                                        if selected {
                                            theme.primary
                                        } else {
                                            theme.muted_foreground
                                        },
                                    );
                                });
                            }
                        }
                    }
                    body.spawn(Node {
                        height: px(5),
                        ..default()
                    });
                    match plan.as_ref() {
                        Some(plan) => {
                            if let Some(audio) = plan.audio_processing.as_ref() {
                                spawn_text(body, font.clone(), "AUDIO MODEL PLAN", 9.0, theme.primary);
                                for (index, step) in audio.steps.iter().enumerate() {
                                    let backend = &audio.requested_runtime.routing_policy;
                                    spawn_wrapped_text(
                                        body,
                                        font.clone(),
                                        format!(
                                            "{}. {} · {} · {} · {} parameter{}",
                                            index + 1,
                                            node_label(audio_step_node_id(&step.step_id).unwrap_or("stems.separate")),
                                            step.model_id,
                                            backend,
                                            step.effective_parameters.len(),
                                            if step.effective_parameters.len() == 1 { "" } else { "s" }
                                        ),
                                        9.0,
                                        theme.foreground,
                                    );
                                }
                                spawn_wrapped_text(
                                    body,
                                    font.clone(),
                                    format!(
                                        "Outputs · {} · precision {}",
                                        audio.output_bindings.iter().map(|binding| binding.artifact_role.as_str()).collect::<Vec<_>>().join(", "),
                                        audio.requested_runtime.precision_policy
                                    ),
                                    8.0,
                                    theme.muted_foreground,
                                );
                            }
                            let groups = plan_preview_groups(plan);
                            if groups.is_empty() {
                                spawn_text(
                                    body,
                                    font.clone(),
                                    "Nothing would run.",
                                    10.0,
                                    theme.muted_foreground,
                                );
                            }
                            for (heading, nodes) in groups {
                                spawn_text(body, font.clone(), heading, 9.0, theme.primary);
                                for node_id in nodes {
                                    spawn_text(
                                        body,
                                        font.clone(),
                                        format!("- {}", node_label(&node_id)),
                                        10.0,
                                        theme.foreground,
                                    );
                                }
                            }
                        }
                        None => {
                            spawn_text(
                                body,
                                font.clone(),
                                "Could not compute a preview for this combination.",
                                10.0,
                                theme.destructive,
                            );
                        }
                    }
                    if let Some(notice) = notice {
                        spawn_wrapped_text(body, font.clone(), notice, 9.0, theme.destructive);
                    }
                    body.spawn(Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        margin: UiRect::top(px(6)),
                        ..default()
                    })
                    .with_children(|actions| {
                        spawn_text_button(
                            actions,
                            font.clone(),
                            theme,
                            "Cancel",
                            10.0,
                            UiAction::from(AnalysisCommand::ClosePlanPreview),
                        );
                        spawn_action_button(
                            actions,
                            font,
                            theme,
                            "Run this plan",
                            UiAction::from(AnalysisCommand::RunPlanPreviewDraft),
                        );
                    });
                });
        });
}

fn analysis_node_execution_actions(
    node_id: &str,
    file_hash: &str,
) -> (
    Option<AnalysisNodeMenuAction>,
    Option<AnalysisNodeMenuAction>,
) {
    let targeted = || {
        UiAction::from(AnalysisCommand::RunAnalysisNodeOnly(
            file_hash.to_string(),
            node_id.to_string(),
        ))
    };
    let downstream = |label| {
        Some(AnalysisNodeMenuAction {
            label,
            action: UiAction::from(AnalysisCommand::RunAnalysisNodeDownstream(
                file_hash.to_string(),
                node_id.to_string(),
            )),
        })
    };
    let retry = match node_id {
        "music.analysis" | "music.key" | "music.rhythm" | "music.descriptors" => {
            Some(AnalysisNodeMenuAction {
                label: "Re-run music analysis route",
                action: targeted(),
            })
        }
        "stems.separate"
        | "stems.vocals"
        | "vocals.denoise"
        | "vocals.dereverb"
        | "stems.instrumental"
        | "instrumental.denoise"
        | "instrumental.dereverb"
        | "stems.karaoke"
        | "stems.multistem"
        | "stems.bind_analysis_outputs" => Some(AnalysisNodeMenuAction {
            label: "Re-run configured stem pipeline",
            action: targeted(),
        }),
        "pitch.extract" => Some(AnalysisNodeMenuAction {
            label: "Rebuild pitch evidence",
            action: UiAction::from(AnalysisCommand::ReanalyzePitch(file_hash.to_string())),
        }),
        "lyrics.transcribe" => Some(AnalysisNodeMenuAction {
            label: "Retranscribe and align",
            action: UiAction::from(AnalysisCommand::ReanalyzeTranscript(file_hash.to_string())),
        }),
        "lyrics.align" => Some(AnalysisNodeMenuAction {
            label: "Realign existing lyrics",
            action: UiAction::from(AnalysisCommand::RealignSong(file_hash.to_string())),
        }),
        "chart.build_candidate" => Some(AnalysisNodeMenuAction {
            label: "Rebuild candidate chart route",
            action: targeted(),
        }),
        _ => None,
    };
    let downstream = match node_id {
        "preflight" => Some(AnalysisNodeMenuAction {
            label: "Run full analysis pipeline",
            action: UiAction::from(AnalysisCommand::ReanalyzeFull(file_hash.to_string())),
        }),
        "stems.separate"
        | "stems.vocals"
        | "vocals.denoise"
        | "vocals.dereverb"
        | "stems.instrumental"
        | "instrumental.denoise"
        | "instrumental.dereverb"
        | "stems.karaoke"
        | "stems.multistem"
        | "stems.bind_analysis_outputs" => downstream("Run stem route and downstream"),
        "lyrics.preprocess" => downstream("Run preprocessing and lyrics route"),
        _ => None,
    };
    (retry, downstream)
}

pub(crate) fn build_analysis_node_context_menu(
    node_id: &str,
    stage_id: &str,
    label: &str,
    file_hash: &str,
    selected_run_id: Option<i64>,
    is_expanded: bool,
    position: Vec2,
) -> AnalysisNodeContextMenu {
    let (retry_action, run_downstream_action) = analysis_node_execution_actions(node_id, file_hash);
    AnalysisNodeContextMenu {
        node_id: node_id.to_string(),
        stage_id: stage_id.to_string(),
        label: label.to_string(),
        retry_action,
        run_downstream_action,
        disable_node_action: app_core::node_can_be_disabled_for_run(node_id).then(|| {
            UiAction::from(AnalysisCommand::DisableAnalysisNodeForRun(
                file_hash.to_string(),
                node_id.to_string(),
            ))
        }),
        freeze_node_action: app_core::node_can_be_frozen_for_run(file_hash, node_id).then(|| {
            UiAction::from(AnalysisCommand::FreezeAnalysisNodeOutputs(
                file_hash.to_string(),
                node_id.to_string(),
            ))
        }),
        bypass_node_action: app_core::node_can_be_bypassed_for_run(node_id).then(|| {
            UiAction::from(AnalysisCommand::BypassAnalysisNodeWithOriginalMix(
                file_hash.to_string(),
                node_id.to_string(),
            ))
        }),
        compare_node_action: selected_run_id.map(|run_id| {
            UiAction::from(AnalysisCommand::CompareNodeAttemptWithPrevious(
                file_hash.to_string(),
                node_id.to_string(),
                run_id,
            ))
        }),
        save_as_song_profile_action: app_core::node_can_be_configured_for_run(node_id).then(|| {
            UiAction::from(AnalysisCommand::SaveNodeConfigAsSongProfile(
                file_hash.to_string(),
                node_id.to_string(),
            ))
        }),
        open_configure_dialog_action: app_core::node_can_be_configured_for_run(node_id).then(
            || {
                UiAction::from(AnalysisCommand::OpenNodeConfigDialog(
                    file_hash.to_string(),
                    node_id.to_string(),
                ))
            },
        ),
        force_transcribe_action: node_can_force_transcribe(node_id)
            .then(|| UiAction::from(AnalysisCommand::ForceTranscribe(file_hash.to_string()))),
        refetch_align_action: node_can_refetch_and_align(node_id)
            .then(|| UiAction::from(AnalysisCommand::ReanalyzeTranscript(file_hash.to_string()))),
        capture_intermediate_action: (node_id == "lyrics.preprocess").then(|| {
            UiAction::from(AnalysisCommand::RequestCaptureIntermediate(
                file_hash.to_string(),
            ))
        }),
        view_logs_action: Some(UiAction::from(AnalysisCommand::OpenAnalysisLogViewer(
            file_hash.to_string(),
            node_id.to_string(),
        ))),
        compound_toggle: analysis_node_compound_toggle_action(node_id, is_expanded),
        position,
    }
}

/// §8.3 migration table's "Force transcribe -> Transcription Node -> Force
/// Recompute": whether `node_id` has a meaningful "ignore online lyrics,
/// transcribe again" action -- only `lyrics.transcribe`. Shared by the real
/// click path and the `UTA_STUDIO_DEBUG_OPEN_NODE_CONTEXT` debug-injection
/// path, same reasoning as `analysis_node_compound_toggle_action` below.
pub(crate) fn node_can_force_transcribe(node_id: &str) -> bool {
    node_id == "lyrics.transcribe"
}

/// §8.3 migration table's "Refetch & align -> Lyrics Source -> LRCLIB ->
/// Run Timing": whether `node_id` has a meaningful "refetch online lyrics,
/// then align" action -- only `lyrics.align`.
pub(crate) fn node_can_refetch_and_align(node_id: &str) -> bool {
    node_id == "lyrics.align"
}

/// `(button label, action)` for the compound-node expand/collapse toggle,
/// or `None` when `node_id` isn't a compound node at all. Shared by the
/// real pointer path (`open_analysis_node_from_pointer`) and the
/// `UTA_STUDIO_DEBUG_OPEN_NODE_CONTEXT` debug-injection path in
/// `desktop/src/studio/mod.rs`, so the two can't drift.
pub(crate) fn analysis_node_compound_toggle_action(
    node_id: &str,
    is_expanded: bool,
) -> Option<(&'static str, UiAction)> {
    let is_compound = app_core::baseline_graph_spec()
        .node(&app_core::AnalysisNodeId::new(node_id))
        .is_some_and(|node| node.is_compound());
    if !is_compound {
        return None;
    }
    let label = if is_expanded {
        "Collapse sub-checks"
    } else {
        "Expand sub-checks"
    };
    Some((
        label,
        UiAction::from(AnalysisCommand::ToggleAnalysisCompoundNode(
            node_id.to_string(),
        )),
    ))
}

/// §7.6 "Play audio artifact": whether an artifact revision's `kind` is a
/// real waveform file `uta_studio_audio::EditorAudioPlayer::load_path` can
/// actually open, as opposed to a JSON/text artifact (transcripts, pitch
/// data, music analysis) that a "Play" button would just fail against.
pub(crate) fn artifact_kind_is_playable(kind: app_core::ArtifactKind) -> bool {
    matches!(
        kind,
        app_core::ArtifactKind::VocalStem
            | app_core::ArtifactKind::InstrumentalStem
            | app_core::ArtifactKind::RawVocalStem
            | app_core::ArtifactKind::DenoisedVocalStem
            | app_core::ArtifactKind::DereverbedVocalStem
            | app_core::ArtifactKind::AnalysisVocalStem
            | app_core::ArtifactKind::HighQualityInstrumentalStem
            | app_core::ArtifactKind::DenoisedInstrumentalStem
            | app_core::ArtifactKind::DereverbedInstrumentalStem
            | app_core::ArtifactKind::KaraokeInstrumentalStem
            | app_core::ArtifactKind::PreprocessedAudio
    )
}

/// Overlays real execution failures from `analysis_node_attempts` onto a
/// Phase 1 plan preview, so §7.8/§9.3's "Focus Failed" button has something
/// real to find -- `analysis_plan::build_plan` itself never produces
/// `NodeState::Failed` (only `Ready`/`Frozen`/`Disabled`/`Blocked`/
/// `NotApplicable`, per that module's own doc comment), so without this the
/// button's search always came back empty and it silently never appeared.
/// Only overlays a `Ready` node: `Blocked`/`Disabled`/`NotApplicable`/
/// `Frozen` already have a more specific, intentional explanation and must
/// not be overwritten just because a node-attempt row with that id also
/// happens to exist (e.g. from an earlier run, before the current plan
/// decided to skip the node this time).
pub(crate) fn overlay_failed_node_attempts(
    mut plan: app_core::AnalysisPlan,
    attempts: &[app_core::NodeAttempt],
) -> app_core::AnalysisPlan {
    let failed_ids: std::collections::BTreeSet<app_core::AnalysisNodeId> = attempts
        .iter()
        .filter(|attempt| attempt.status == "failed")
        .map(|attempt| app_core::AnalysisNodeId::new(attempt.node_id.clone()))
        .collect();
    for node in &mut plan.nodes {
        if node.state == app_core::NodeState::Ready && failed_ids.contains(&node.id) {
            node.state = app_core::NodeState::Failed;
        }
    }
    plan
}

/// Phase 5 §5.5 "Stale Evidence" / §7's "GraphNodeState has no Stale
/// variant" gap: overlays `app_core::candidate_chart_status`'s real
/// mtime-based staleness comparison onto `chart.build_candidate` -- the one
/// node a Candidate/Authored distinction actually applies to. Same
/// "only overwrite Ready" rule as `overlay_failed_node_attempts`: a
/// Blocked/Disabled/NotApplicable/Frozen `chart.build_candidate` already
/// has a more specific, intentional explanation for why it isn't running
/// this pass, which staleness (a fact about the *last* successful run's
/// output, not this plan) must not override.
pub(crate) fn overlay_stale_candidate_chart(
    mut plan: app_core::AnalysisPlan,
    candidate_status: &app_core::CandidateChartStatus,
) -> app_core::AnalysisPlan {
    if !matches!(
        candidate_status,
        app_core::CandidateChartStatus::CandidateAvailable(_)
    ) {
        return plan;
    }
    let chart_build_id = app_core::AnalysisNodeId::new("chart.build_candidate");
    for node in &mut plan.nodes {
        if node.id == chart_build_id && node.state == app_core::NodeState::Ready {
            node.state = app_core::NodeState::Stale;
        }
    }
    plan
}

pub(crate) struct AnalysisNodeClickTarget<'a> {
    pub(crate) node_id: &'a str,
    pub(crate) label: &'a str,
    pub(crate) file_hash: &'a str,
    pub(crate) stage_id: &'a str,
}

pub(crate) fn clamp_analysis_node_context_position(position: Vec2, viewport: Vec2) -> Vec2 {
    const MENU_WIDTH: f32 = 278.0;
    const MENU_MAX_HEIGHT: f32 = 620.0;
    const EDGE: f32 = 8.0;
    let available_height = (viewport.y * 0.86).min(MENU_MAX_HEIGHT);
    Vec2::new(
        position
            .x
            .clamp(EDGE, (viewport.x - MENU_WIDTH - EDGE).max(EDGE)),
        position
            .y
            .clamp(EDGE, (viewport.y - available_height - EDGE).max(EDGE)),
    )
}

pub(crate) fn open_analysis_node_from_pointer(
    button: PointerButton,
    menu_position: Vec2,
    viewport_size: Vec2,
    target: AnalysisNodeClickTarget,
    analysis: &mut AnalysisUiState,
    dialogs: &mut DialogState,
    invalidated: &mut UiInvalidated,
) {
    let AnalysisNodeClickTarget {
        node_id,
        label,
        file_hash,
        stage_id,
    } = target;
    match button {
        PointerButton::Primary => {
            analysis.selected_analysis_stage = Some(stage_id.to_string());
            analysis.selected_analysis_node = Some(node_id.to_string());
            dialogs.analysis_node_context = None;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        PointerButton::Secondary => {
            let is_expanded = analysis
                .expanded_compound_nodes
                .contains(&app_core::AnalysisNodeId::new(node_id));
            dialogs.analysis_node_context = Some(build_analysis_node_context_menu(
                node_id,
                stage_id,
                label,
                file_hash,
                analysis.selected_analysis_history,
                is_expanded,
                clamp_analysis_node_context_position(menu_position, viewport_size),
            ));
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        PointerButton::Middle => {}
    }
}

pub(crate) fn spawn_analysis_node_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    context: &AnalysisNodeContextMenu,
) {
    parent.spawn((
        Button,
        UiAction::from(AnalysisCommand::DismissAnalysisNodeContext),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ZIndex(40),
    ));
    // Node context menus live in the window-sized overlay region, so the
    // raw window position can be used directly without rebuilding the DAG.
    let left = context.position.x.max(8.0);
    let top = context.position.y.max(8.0);
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(278),
                max_height: percent(86),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(2),
                align_items: AlignItems::Stretch,
                overflow: Overflow::scroll_y(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(41),
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
                format!("{} · Node actions", context.node_id),
                8.0,
                theme.muted_foreground,
            );
            menu.spawn(Node {
                height: px(5),
                ..default()
            });
            spawn_menu_text_button(
                menu,
                font.clone(),
                theme,
                "Inspect view",
                11.0,
                UiAction::from(AnalysisCommand::OpenAnalysisInspect(
                    context.node_id.clone(),
                    context.stage_id.clone(),
                )),
            );
            if context.retry_action.is_some() || context.run_downstream_action.is_some() {
                menu.spawn(Node {
                    height: px(5),
                    ..default()
                });
                spawn_text(menu, font.clone(), "EXECUTION", 7.0, theme.muted_foreground);
            }
            if let Some(retry) = context.retry_action.clone() {
                spawn_menu_text_button(menu, font.clone(), theme, retry.label, 11.0, retry.action);
            }
            if let Some(downstream) = context.run_downstream_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    downstream.label,
                    11.0,
                    downstream.action,
                );
            }
            if context.disable_node_action.is_some()
                || context.freeze_node_action.is_some()
                || context.bypass_node_action.is_some()
                || context.open_configure_dialog_action.is_some()
                || context.save_as_song_profile_action.is_some()
                || context.force_transcribe_action.is_some()
                || context.refetch_align_action.is_some()
                || context.capture_intermediate_action.is_some()
            {
                menu.spawn(Node {
                    height: px(5),
                    ..default()
                });
                spawn_text(
                    menu,
                    font.clone(),
                    "RUN OPTIONS",
                    7.0,
                    theme.muted_foreground,
                );
            }
            if let Some(disable_action) = context.disable_node_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Disable for this run",
                    11.0,
                    disable_action,
                );
            }
            if let Some(freeze_action) = context.freeze_node_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Freeze current outputs",
                    11.0,
                    freeze_action,
                );
            }
            if let Some(bypass_action) = context.bypass_node_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Bypass with original mix",
                    11.0,
                    bypass_action,
                );
            }
            if let Some(configure_action) = context.open_configure_dialog_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Configure for this run…",
                    11.0,
                    configure_action,
                );
            }
            if let Some(save_profile_action) = context.save_as_song_profile_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Save as song profile",
                    11.0,
                    save_profile_action,
                );
            }
            if let Some(force_transcribe_action) = context.force_transcribe_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Force transcribe",
                    11.0,
                    force_transcribe_action,
                );
            }
            if let Some(refetch_align_action) = context.refetch_align_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Refetch lyrics & align",
                    11.0,
                    refetch_align_action,
                );
            }
            if let Some(capture_action) = context.capture_intermediate_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Capture intermediate output on next run…",
                    11.0,
                    capture_action,
                );
            }
            if let Some((toggle_label, toggle_action)) = context.compound_toggle.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    toggle_label,
                    11.0,
                    toggle_action,
                );
            }
            menu.spawn(Node {
                height: px(5),
                ..default()
            });
            spawn_text(menu, font.clone(), "EVIDENCE", 7.0, theme.muted_foreground);
            if let Some(compare_action) = context.compare_node_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Compare with previous attempt",
                    11.0,
                    compare_action,
                );
            }
            spawn_menu_text_button(
                menu,
                font.clone(),
                theme,
                "Open node documentation",
                11.0,
                UiAction::from(AppCommand::OpenDocumentation(Some(
                    documentation_anchor_for_node(&context.node_id).to_string(),
                ))),
            );
            if let Some(view_logs_action) = context.view_logs_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "View logs",
                    11.0,
                    view_logs_action,
                );
            }
        });
}
