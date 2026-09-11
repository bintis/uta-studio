use crate::studio::*;

#[derive(SystemParam)]
pub(crate) struct ActionSystemParams<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    text_inputs: EditorTextInputs<'w, 's>,
    search_inputs: LibrarySearchInputs<'w, 's>,
    model_inputs: Query<'w, 's, (&'static ModelParameterInput, &'static EditableText)>,
    focus: Res<'w, bevy::input_focus::InputFocus>,
    windows: PrimaryWindowAndAnalysisViewport<'w, 's>,
    audio: Res<'w, NativeAudio>,
    library_audio: Res<'w, NativeLibraryAudio>,
    pitch_audition: Res<'w, NativePitchAudition>,
    shell: ResMut<'w, ShellState>,
    library: ResMut<'w, LibraryState>,
    analysis: ResMut<'w, AnalysisUiState>,
    editor: ResMut<'w, EditorUiState>,
    dialogs: ResMut<'w, DialogState>,
    jobs: ResMut<'w, AsyncJobs>,
    playback: ResMut<'w, PlaybackState>,
    setup: ResMut<'w, NativeSetup>,
    diagnostics: ResMut<'w, NativeDiagnostics>,
    debug_log: ResMut<'w, DebugLogJob>,
    authoring: ResMut<'w, NativeAuthoringJob>,
    theme: ResMut<'w, StudioTheme>,
    clear_color: ResMut<'w, ClearColor>,
    invalidated: ResMut<'w, UiInvalidated>,
    script: ResMut<'w, UiScriptState>,
    startup_banner: Res<'w, StartupBannerState>,
    screenshot: Res<'w, DebugScreenshotState>,
    app_exit: MessageWriter<'w, AppExit>,
}

pub(crate) fn handle_actions(
    mut commands: Commands,
    interactions: Query<(&Interaction, &UiAction), Changed<Interaction>>,
    mut context: ActionSystemParams,
) {
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        dispatch_action(action, &mut commands, &mut context);
    }
    if context.keys.just_pressed(KeyCode::Enter)
        && let Some(entity) = context.focus.get()
        && let Ok((input, _)) = context.model_inputs.get(entity)
    {
        let action = SettingsCommand::ApplyModelParameter(input.model.clone(), input.key.clone());
        dispatch_action(&action.into(), &mut commands, &mut context);
    }
    // Scripted interactions (`UTA_STUDIO_DEBUG_UI_SCRIPT`) take the same path
    // as a pointer press: one registered command per step, dispatched here.
    let banner_done = context.startup_banner.done;
    if let Some((index, step, parsed)) = context.script.take_due_step(banner_done) {
        let (dispatched, error) = match parsed {
            Ok(command) => {
                dispatch_action(&UiAction(command), &mut commands, &mut context);
                (true, None)
            }
            Err(error) => (false, Some(error)),
        };
        context.script.record_step(
            index,
            &step,
            dispatched,
            error,
            &context.shell,
            &context.dialogs,
        );
    }
    if context.script.finish_if_done(banner_done) && context.screenshot.path.is_none() {
        context.app_exit.write(AppExit::Success);
    }
}

/// Dispatches one UI command exactly as a pointer press on its button would.
fn dispatch_action(action: &UiAction, commands: &mut Commands, context: &mut ActionSystemParams) {
    // Apply/Enter reads the visible numeric field, then uses the same mutation
    // command as NDJSON automation and +/- controls.
    if let UiCommand::Settings(SettingsCommand::ApplyModelParameter(model, key)) = &action.0 {
        let value = context
            .model_inputs
            .iter()
            .find(|(input, _)| input.model == *model && input.key == *key)
            .map(|(_, text)| text.value().to_string());
        if let Some(value) = value {
            dispatch_action(
                &SettingsCommand::SetModelParameter(model.clone(), key.clone(), value).into(),
                commands,
                context,
            );
        } else {
            context.shell.notice =
                Some("Open this model's controls before applying an edited value".to_string());
            context.invalidated.invalidate(UiDirtyRegion::Settings);
        }
        return;
    }
    {
        let request = action.api_request();
        bevy::log::info!(
            target: "uta_studio::ui_action",
            command = %request.command,
            access = request.access,
            "UI action pressed"
        );
        // The node-menu backdrop guarantees that every action received while
        // this menu is open came from the menu or its dismiss surface. Tear
        // down that overlay before dispatching the selected command so no
        // workspace-scoped action can leave a stale menu entity behind.
        if context.dialogs.analysis_node_context.take().is_some() {
            context.invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        let graph_viewport_width = context
            .windows
            .analysis_graph_viewport
            .iter()
            .next()
            .map(|computed| computed.size().x * computed.inverse_scale_factor());
        let Ok((window_entity, mut window)) = context.windows.windows.single_mut() else {
            return;
        };
        if apply_chrome_action(
            action,
            commands,
            &context.search_inputs,
            window_entity,
            graph_viewport_width,
            ChromeActionState {
                audio: &context.audio,
                library_audio: &context.library_audio,
                state: StudioStateMut {
                    shell: &mut context.shell,
                    library: &mut context.library,
                    analysis: &mut context.analysis,
                    editor: &mut context.editor,
                    dialogs: &mut context.dialogs,
                    jobs: &mut context.jobs,
                    playback: &mut context.playback,
                },
                invalidated: &mut context.invalidated,
            },
        ) {
            return;
        }
        if apply_settings_action(
            action,
            SettingsActionContext {
                window: &mut window,
                state: StudioStateMut {
                    shell: &mut context.shell,
                    library: &mut context.library,
                    analysis: &mut context.analysis,
                    editor: &mut context.editor,
                    dialogs: &mut context.dialogs,
                    jobs: &mut context.jobs,
                    playback: &mut context.playback,
                },
                setup: &mut context.setup,
                diagnostics: &mut context.diagnostics,
                debug_log: &mut context.debug_log,
                theme: &mut context.theme,
                clear_color: &mut context.clear_color,
                invalidated: &mut context.invalidated,
            },
        ) {
            return;
        }
        apply_content_action(
            action,
            &context.keys,
            &context.text_inputs,
            ContentActionServices {
                audio: &context.audio,
                library_audio: &context.library_audio,
                pitch_audition: &context.pitch_audition,
            },
            ContentActionState {
                state: StudioStateMut {
                    shell: &mut context.shell,
                    library: &mut context.library,
                    analysis: &mut context.analysis,
                    editor: &mut context.editor,
                    dialogs: &mut context.dialogs,
                    jobs: &mut context.jobs,
                    playback: &mut context.playback,
                },
                authoring: &mut context.authoring,
                invalidated: &mut context.invalidated,
            },
        );
    }
}
