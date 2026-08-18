use crate::studio::*;

pub(crate) fn handle_actions(
    mut commands: Commands,
    interactions: Query<(&Interaction, &UiAction), Changed<Interaction>>,
    keys: Res<ButtonInput<KeyCode>>,
    text_inputs: EditorTextInputs,
    search_inputs: Query<
        &EditableText,
        (
            With<LibrarySearchInput>,
            Without<LyricsEditorInput>,
            Without<LanguageEditorInput>,
        ),
    >,
    mut windows: PrimaryWindowAndAnalysisViewport,
    audio: Res<NativeAudio>,
    library_audio: Res<NativeLibraryAudio>,
    pitch_audition: Res<NativePitchAudition>,
    mut session: ResMut<StudioSession>,
    mut setup: ResMut<NativeSetup>,
    mut diagnostics: ResMut<NativeDiagnostics>,
    mut authoring: ResMut<NativeAuthoringJob>,
    mut theme: ResMut<StudioTheme>,
    mut clear_color: ResMut<ClearColor>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let graph_viewport_width = windows
            .analysis_graph_viewport
            .iter()
            .next()
            .map(|computed| computed.size().x * computed.inverse_scale_factor());
        let Ok((window_entity, mut window)) = windows.windows.single_mut() else {
            continue;
        };
        if apply_chrome_action(
            action,
            &mut commands,
            &keys,
            &search_inputs,
            window_entity,
            &mut window,
            graph_viewport_width,
            &audio,
            &library_audio,
            &mut session,
            &mut invalidated,
        ) {
            continue;
        }
        if apply_settings_action(
            action,
            &mut window,
            &mut session,
            &mut setup,
            &mut diagnostics,
            &mut theme,
            &mut clear_color,
            &mut invalidated,
        ) {
            continue;
        }
        apply_content_action(
            action,
            &mut commands,
            &keys,
            &text_inputs,
            &search_inputs,
            &audio,
            &library_audio,
            &pitch_audition,
            &mut session,
            &mut authoring,
            &mut invalidated,
        );
    }
}
