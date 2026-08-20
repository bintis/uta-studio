use crate::studio::*;

#[derive(SystemParam)]
pub(crate) struct ActionSystemParams<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    text_inputs: EditorTextInputs<'w, 's>,
    search_inputs: LibrarySearchInputs<'w, 's>,
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
    authoring: ResMut<'w, NativeAuthoringJob>,
    theme: ResMut<'w, StudioTheme>,
    clear_color: ResMut<'w, ClearColor>,
    invalidated: ResMut<'w, UiInvalidated>,
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
            continue;
        };
        if apply_chrome_action(
            action,
            &mut commands,
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
            continue;
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
                theme: &mut context.theme,
                clear_color: &mut context.clear_color,
                invalidated: &mut context.invalidated,
            },
        ) {
            continue;
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
