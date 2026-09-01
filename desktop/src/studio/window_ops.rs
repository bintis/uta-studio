use crate::studio::*;

pub(crate) fn update_navigation_focus_visuals(
    focus: Res<InputFocus>,
    focus_visible: Res<InputFocusVisible>,
    theme: Res<StudioTheme>,
    mut buttons: Query<(Entity, &mut Outline), With<UiAction>>,
) {
    if !focus.is_changed() && !focus_visible.is_changed() && !theme.is_changed() {
        return;
    }
    for (entity, mut outline) in &mut buttons {
        outline.color = if focus_visible.0 && focus.get() == Some(entity) {
            theme.primary.with_alpha(0.88)
        } else {
            Color::NONE
        };
    }
}

pub(crate) fn handle_fullscreen_shortcut(
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut shell: ResMut<ShellState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !keys.just_pressed(KeyCode::F11) {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    if let Some(error) = toggle_fullscreen(&mut window, &mut shell.config) {
        shell.notice = Some(error);
    }
    invalidated.invalidate(UiDirtyRegion::Chrome);
}

pub(crate) fn toggle_fullscreen(window: &mut Window, config: &mut AppConfig) -> Option<String> {
    let fullscreen = matches!(window.mode, WindowMode::Windowed);
    window.mode = if fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    config.fullscreen = Some(fullscreen);
    save_config_error(config)
}
