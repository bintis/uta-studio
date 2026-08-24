use crate::studio::*;

pub(crate) type LibrarySearchInputs<'w, 's> = Query<
    'w,
    's,
    &'static EditableText,
    (
        With<LibrarySearchInput>,
        Without<LyricsEditorInput>,
        Without<LanguageEditorInput>,
    ),
>;

type NavigationTargets<'w, 's> =
    Query<'w, 's, (Entity, &'static UiAction), (Added<UiAction>, With<Button>)>;

pub(crate) const NAVIGATION_INITIAL_REPEAT: Duration = Duration::from_millis(400);

pub(crate) const NAVIGATION_REPEAT_RATE: Duration = Duration::from_millis(80);

pub(crate) const NAVIGATION_STICK_DEADZONE: f32 = 0.5;

pub(crate) fn register_navigation_targets(mut commands: Commands, targets: NavigationTargets) {
    for (entity, action) in &targets {
        if !action_is_navigation_target(action) {
            continue;
        }
        commands
            .entity(entity)
            .try_insert((TabIndex(0), Outline::new(px(1), px(1), Color::NONE)));
    }
}

pub(crate) fn action_is_navigation_target(action: &UiAction) -> bool {
    !matches!(
        &action.0,
        UiCommand::App(AppCommand::CloseActivity)
            | UiCommand::Library(LibraryCommand::DismissFolderContext)
            | UiCommand::Library(LibraryCommand::DismissSongContext)
            | UiCommand::Editor(EditorCommand::DismissLyricContext)
            | UiCommand::Editor(EditorCommand::DismissNoteContext)
            | UiCommand::Editor(EditorCommand::DismissWaveformContext)
            | UiCommand::Editor(EditorCommand::DismissProblemsPanel)
            | UiCommand::Editor(EditorCommand::DismissShortcutsPanel)
            | UiCommand::Analysis(AnalysisCommand::DismissAnalysisNodeContext)
            | UiCommand::Analysis(AnalysisCommand::DismissAnalysisExportContext)
            | UiCommand::Analysis(AnalysisCommand::ClosePlanPreview)
            | UiCommand::Analysis(AnalysisCommand::CloseAnalysisLogViewer)
    )
}

pub(crate) fn navigation_repeat(
    state: &mut NavigationInputState,
    direction: Option<NavigationDirection>,
    now: Instant,
) -> Option<NavigationDirection> {
    let Some(direction) = direction else {
        state.held_direction = None;
        state.repeat_at = None;
        return None;
    };
    if state.held_direction != Some(direction) {
        state.held_direction = Some(direction);
        state.repeat_at = Some(now + NAVIGATION_INITIAL_REPEAT);
        return Some(direction);
    }
    if state.repeat_at.is_some_and(|repeat_at| now >= repeat_at) {
        state.repeat_at = Some(now + NAVIGATION_REPEAT_RATE);
        return Some(direction);
    }
    None
}

fn route_back_action(route: StudioRoute, library_view: LibraryView) -> Option<UiAction> {
    if route == StudioRoute::Documentation {
        Some(UiAction::from(AppCommand::DocumentationBack))
    } else if route != StudioRoute::Library || library_view == LibraryView::Queue {
        Some(UiAction::from(AppCommand::Back))
    } else {
        None
    }
}

pub(crate) fn navigation_back_action(session: &StudioSessionView<'_>) -> Option<UiAction> {
    if session.plan_preview_draft.is_some() {
        return Some(UiAction::from(AnalysisCommand::ClosePlanPreview));
    }
    if session.analysis_log_viewer.is_some() {
        return Some(UiAction::from(AnalysisCommand::CloseAnalysisLogViewer));
    }
    if session.artifact_diff.is_some() {
        return Some(UiAction::from(AnalysisCommand::CloseArtifactDiff));
    }
    if session.artifact_impact.is_some() {
        return Some(UiAction::from(AnalysisCommand::CloseArtifactImpact));
    }
    if session.artifact_lineage.is_some() || session.analysis_lineage_mode {
        return Some(UiAction::from(AnalysisCommand::CloseArtifactLineage));
    }
    if session.analysis_export_context.is_some() {
        return Some(UiAction::from(
            AnalysisCommand::DismissAnalysisExportContext,
        ));
    }
    if session.pending_leave.is_some() {
        return Some(UiAction::from(AppCommand::CancelLeave));
    }
    if session.pending_setup.is_some() {
        return Some(UiAction::from(SettingsCommand::CancelSetup));
    }
    if session.pending_cache_clear.is_some() {
        return Some(UiAction::from(SettingsCommand::CancelClearCache));
    }
    if session.pending_cache_delete.is_some() {
        return Some(UiAction::from(AnalysisCommand::CancelDeleteSongCache));
    }
    if session.pending_artifact_delete.is_some() {
        return Some(UiAction::from(
            AnalysisCommand::CancelDeleteArtifactRevision,
        ));
    }
    if session.pending_artifact_invalidate.is_some() {
        return Some(UiAction::from(
            AnalysisCommand::CancelInvalidateArtifactRevision,
        ));
    }
    if session.pending_artifact_active.is_some() {
        return Some(UiAction::from(
            AnalysisCommand::CancelSetActiveArtifactRevision,
        ));
    }
    if session.pending_chart_replace.is_some() {
        return Some(UiAction::from(AnalysisCommand::CancelReplaceAuthoredChart));
    }
    if session.pending_analysis_history_clear {
        return Some(UiAction::from(AnalysisCommand::CancelClearAnalysisHistory));
    }
    if session.lyrics_editor.is_some() {
        return Some(UiAction::from(EditorCommand::CloseLyricsEditor));
    }
    if session.language_editor.is_some() {
        return Some(UiAction::from(EditorCommand::CloseLanguageEditor));
    }
    if session.song_settings.is_some() {
        return Some(UiAction::from(EditorCommand::CloseSongSettings));
    }
    if session.about_open {
        return Some(UiAction::from(AppCommand::CloseAbout));
    }
    if session.activity_open {
        return Some(UiAction::from(AppCommand::CloseActivity));
    }
    if session.search_open {
        return Some(UiAction::from(AppCommand::ToggleGlobalSearch));
    }
    if session.song_context.is_some() {
        return Some(UiAction::from(LibraryCommand::DismissSongContext));
    }
    if session.analysis_artifact_context.is_some() {
        return Some(UiAction::from(
            AnalysisCommand::DismissAnalysisArtifactContext,
        ));
    }
    if session.analysis_node_context.is_some() {
        return Some(UiAction::from(AnalysisCommand::DismissAnalysisNodeContext));
    }
    if session.folder_browser.context_menu.is_some() {
        return Some(UiAction::from(LibraryCommand::DismissFolderContext));
    }
    if let Some(kind) = session.open_settings_select {
        return Some(UiAction::from(SettingsCommand::OpenSettingsSelect(kind)));
    }
    if let Some(kind) = session.open_library_select {
        return Some(UiAction::from(LibraryCommand::OpenLibrarySelect(kind)));
    }
    if session.export_all_open {
        return Some(UiAction::from(LibraryCommand::ToggleExportAllMenu));
    }
    if let Some(kind) = session.open_editor_select {
        return Some(UiAction::from(EditorCommand::OpenEditorSelect(kind)));
    }
    if session.library_playback.queue_open {
        return Some(UiAction::from(LibraryCommand::ToggleLibraryQueue));
    }
    if session
        .editor
        .as_ref()
        .is_some_and(|editor| editor.inspector_open)
    {
        return Some(UiAction::from(EditorCommand::Editor(
            EditorAction::ToggleInspector,
        )));
    }
    route_back_action(session.route, session.library_view)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn handle_accessible_navigation(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    navigation: TabNavigation,
    mut state: ResMut<NavigationInputState>,
    mut focus: ResMut<InputFocus>,
    mut focus_visible: ResMut<InputFocusVisible>,
    studio: StudioStateRead,
    editable: Query<(), With<EditableText>>,
    mut targets: Query<(Entity, &UiAction, &mut Interaction), With<Button>>,
) {
    let session = studio.view();
    if let Some(entity) = state.activated.take()
        && let Ok((_, _, mut interaction)) = targets.get_mut(entity)
        && *interaction == Interaction::Pressed
    {
        *interaction = Interaction::None;
    }

    let focused = focus.get();
    let editing = focused.is_some_and(|entity| editable.contains(entity));
    let focused_action = focused.is_some_and(|entity| targets.contains(entity));

    let gamepad_back = gamepads.iter().any(|gamepad| {
        gamepad.just_pressed(GamepadButton::East) || gamepad.just_pressed(GamepadButton::Start)
    });
    if gamepad_back
        && let Some(back_action) = navigation_back_action(&session)
        && let Some((entity, _, mut interaction)) = targets
            .iter_mut()
            .find(|(_, action, _)| **action == back_action)
    {
        *interaction = Interaction::Pressed;
        state.activated = Some(entity);
        return;
    }

    let keyboard_direction = if !editing && (session.route != StudioRoute::Editor || focused_action)
    {
        if keys.pressed(KeyCode::ArrowUp) || keys.pressed(KeyCode::ArrowLeft) {
            Some(NavigationDirection::Previous)
        } else if keys.pressed(KeyCode::ArrowDown) || keys.pressed(KeyCode::ArrowRight) {
            Some(NavigationDirection::Next)
        } else {
            None
        }
    } else {
        None
    };
    let gamepad_direction = gamepads.iter().find_map(|gamepad| {
        let dpad = gamepad.dpad();
        let stick = gamepad.left_stick();
        let direction = if dpad.length_squared() > 0.0 {
            dpad
        } else {
            stick
        };
        if direction.length_squared() < NAVIGATION_STICK_DEADZONE.powi(2) {
            None
        } else if direction.y.abs() >= direction.x.abs() {
            Some(if direction.y > 0.0 {
                NavigationDirection::Previous
            } else {
                NavigationDirection::Next
            })
        } else {
            Some(if direction.x < 0.0 {
                NavigationDirection::Previous
            } else {
                NavigationDirection::Next
            })
        }
    });
    if let Some(direction) = navigation_repeat(
        &mut state,
        keyboard_direction.or(gamepad_direction),
        Instant::now(),
    ) {
        let action = match direction {
            NavigationDirection::Previous => NavAction::Previous,
            NavigationDirection::Next => NavAction::Next,
        };
        let next =
            navigation.navigate(&focus, action).or_else(|error| {
                match error {
            bevy::input_focus::tab_navigation::TabNavigationError::NoTabGroupForCurrentFocus {
                new_focus,
                ..
            } => Ok(new_focus),
            other => Err(other),
        }
            });
        if let Ok(next) = next {
            focus.set(next, FocusCause::Navigated);
            focus_visible.0 = true;
        }
    }

    let gamepad_confirm = gamepads
        .iter()
        .any(|gamepad| gamepad.just_pressed(GamepadButton::South));
    let keyboard_confirm = keys.just_pressed(KeyCode::Enter)
        || (session.route != StudioRoute::Editor && keys.just_pressed(KeyCode::Space));
    if (gamepad_confirm || keyboard_confirm)
        && let Some(entity) = focus.get()
        && let Ok((_, _, mut interaction)) = targets.get_mut(entity)
    {
        *interaction = Interaction::Pressed;
        state.activated = Some(entity);
    }
}

/// The song-detail and whole-song lyrics textareas `handle_actions` reads on
/// save/apply. Grouped into one `SystemParam` because `handle_actions` was
/// already at Bevy's per-system parameter limit — bundling two related
/// queries here costs one slot instead of two.
#[derive(SystemParam)]
pub(crate) struct EditorTextInputs<'w, 's> {
    pub(crate) lyrics: Query<'w, 's, &'static EditableText, With<LyricsEditorInput>>,
    pub(crate) all_lyrics: Query<
        'w,
        's,
        &'static EditableText,
        (With<EditorAllLyricsInput>, Without<LyricsEditorInput>),
    >,
    pub(crate) song_settings_composer:
        Query<'w, 's, &'static EditableText, With<SongSettingsComposerInput>>,
    pub(crate) song_settings_country:
        Query<'w, 's, &'static EditableText, With<SongSettingsCountryInput>>,
    pub(crate) song_settings_bpm: Query<'w, 's, &'static EditableText, With<SongSettingsBpmInput>>,
}

// Bevy's `IntoSystem` impls top out at 16 function params -- this groups the
// primary window query with the (newly added) DAG canvas viewport query so
// `handle_actions` doesn't cross that ceiling.
#[derive(SystemParam)]
pub(crate) struct PrimaryWindowAndAnalysisViewport<'w, 's> {
    pub(crate) windows: Query<'w, 's, (Entity, &'static mut Window), With<PrimaryWindow>>,
    pub(crate) analysis_graph_viewport:
        Query<'w, 's, &'static ComputedNode, With<AnalysisGraphViewport>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_is_a_back_navigable_library_subview() {
        assert_eq!(
            route_back_action(StudioRoute::Library, LibraryView::Queue),
            Some(UiAction::from(AppCommand::Back))
        );
        assert_eq!(
            route_back_action(StudioRoute::Library, LibraryView::All),
            None
        );
    }

    #[test]
    fn documentation_keeps_its_history_aware_back_command() {
        assert_eq!(
            route_back_action(StudioRoute::Documentation, LibraryView::All),
            Some(UiAction::from(AppCommand::DocumentationBack))
        );
    }
}
