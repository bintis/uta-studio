use super::*;
use crate::studio::*;
use app_core::model_settings::{MODELS, Parameter};

#[derive(Component)]
pub(crate) struct ModelParameterInput {
    pub(crate) model: String,
    pub(crate) key: String,
}

pub(crate) fn change_model_parameter(
    config: &mut AppConfig,
    model: &str,
    key: &str,
    text: &str,
    save: impl FnOnce(&AppConfig) -> Result<(), String>,
) -> Result<(), String> {
    if text.trim().eq_ignore_ascii_case("default") && key == "overlap" {
        return save_config_change(config, |proposed| {
            if let Some(values) = proposed.model_settings.get_mut(model) { values.remove(key); }
        }, save);
    }
    let number = text.trim().parse::<f64>().map_err(|_| "Enter a number for this model parameter".to_string())?;
    let mut proposed = config.clone();
    app_core::model_settings::set(&mut proposed.model_settings, model, key, number)?;
    save(&proposed)?;
    *config = proposed;
    Ok(())
}

pub(crate) fn model_parameter_value(config: &AppConfig, model: &str, parameter: Parameter) -> f64 {
    config.model_settings.get(model).and_then(|values| values.get(parameter.key)).and_then(serde_json::Value::as_f64).unwrap_or(parameter.default)
}

pub(crate) fn spawn_model_tuning(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let selected = MODELS.iter().find(|model| model.id == session.model_tuning).unwrap_or(&MODELS[0]);
    parent.spawn(Node {
        width: percent(100), flex_wrap: FlexWrap::Wrap,
        column_gap: px(6), row_gap: px(6), padding: UiRect::all(px(16)), ..default()
    }).with_children(|choices| {
        for model in MODELS {
            let active = model.id == selected.id;
            choices.spawn((
                Button, UiAction::from(SettingsCommand::SelectModelTuning(model.id.to_string())),
                Node { min_height: px(30), max_width: px(260), align_items: AlignItems::Center,
                    padding: UiRect::axes(px(10), px(6)), border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(5)), ..default() },
                BackgroundColor(if active { theme.primary.with_alpha(0.13) } else { theme.background.with_alpha(0.2) }),
                BorderColor::all(if active { theme.primary.with_alpha(0.6) } else { theme.border.with_alpha(0.4) }),
            )).with_children(|button| { spawn_wrapped_text(button, font.clone(), model.label, 9.0, if active { theme.primary } else { theme.foreground }); });
        }
    });
    spawn_settings_section(parent, font.clone(), theme, selected.label, selected.note);
    for parameter in selected.parameters {
        spawn_parameter_row(parent, font.clone(), theme, session.config, selected.id, *parameter);
    }
    if !selected.parameters.is_empty() {
        spawn_setting_row(parent, font, theme, "Model defaults",
            "Restore only this model's controls. Overlap returns to the model file's default; no source media, installed model, running job or existing chart is changed.",
            Some(("Reset model", UiAction::from(SettingsCommand::ResetModelParameters(selected.id.to_string())))));
    }
}

fn spawn_parameter_row(
    parent: &mut ChildSpawnerCommands, font: Handle<Font>, theme: &StudioTheme,
    config: &AppConfig, model: &str, parameter: Parameter,
) {
    let value = model_parameter_value(config, model, parameter);
    let overridden = config.model_settings.get(model).is_some_and(|values| values.contains_key(parameter.key));
    let display = if parameter.key == "overlap" && !overridden { "Default".to_string() } else { value.to_string() };
    let description = if parameter.key == "overlap" {
        if overridden {
            format!("{} Current overlap: {:.1}%.", parameter.description, (1.0 - 1.0 / value) * 100.0)
        } else {
            format!("{} Model default is active; enter or step a value to override it.", parameter.description)
        }
    } else { parameter.description.to_string() };
    parent.spawn((Node {
        width: percent(100), min_height: px(84), flex_shrink: 0.0, align_items: AlignItems::FlexStart,
        flex_wrap: FlexWrap::Wrap, padding: UiRect::axes(px(SETTINGS_ROW_HORIZONTAL_PADDING), px(SETTINGS_ROW_VERTICAL_PADDING)),
        column_gap: px(24), row_gap: px(12), border: UiRect::bottom(px(1)), ..default()
    }, BorderColor::all(theme.border.with_alpha(0.42)))).with_children(|row| {
        row.spawn(Node { min_width: px(SETTINGS_COPY_MIN_WIDTH), flex_basis: px(SETTINGS_COPY_BASIS), flex_grow: 1.0,
            flex_direction: FlexDirection::Column, row_gap: px(4), ..default() }).with_children(|copy| {
            spawn_text(copy, font.clone(), parameter.label, 11.5, theme.foreground);
            spawn_wrapped_text(copy, font.clone(), description, 9.2, theme.muted_foreground);
        });
        row.spawn(Node { min_width: px(180), max_width: px(SETTINGS_CONTROL_WIDTH), flex_basis: px(SETTINGS_CONTROL_WIDTH),
            flex_grow: 0.0, justify_content: JustifyContent::FlexEnd, align_items: AlignItems::Center, column_gap: px(6), ..default()
        }).with_children(|controls| {
            spawn_text_button(controls, font.clone(), theme, "−", 15.0,
                SettingsCommand::AdjustModelParameter(model.to_string(), parameter.key.to_string(), -1).into());
            controls.spawn((
                ModelParameterInput { model: model.to_string(), key: parameter.key.to_string() },
                EditableText { visible_width: Some(7.0), ..EditableText::new(&display) },
                Node { width: px(72), height: px(32), padding: UiRect::axes(px(8), px(5)), border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)), overflow: Overflow::clip_x(), ..default() },
                ui_text_font(font.clone(), 10.0), TextColor(theme.foreground),
                TextCursorStyle { color: theme.primary, selected_text_color: Some(theme.primary_foreground), ..default() },
                BackgroundColor(theme.background.with_alpha(0.65)), BorderColor::all(theme.border.with_alpha(0.72)), TabIndex(0),
            ));
            spawn_text_button(controls, font.clone(), theme, "+", 15.0,
                SettingsCommand::AdjustModelParameter(model.to_string(), parameter.key.to_string(), 1).into());
            spawn_text_button(controls, font.clone(), theme, "Apply", 9.0,
                SettingsCommand::ApplyModelParameter(model.to_string(), parameter.key.to_string()).into());
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parameter_save_is_atomic_and_super_does_not_override_quality() {
        let mut config = AppConfig { turbo_acceleration: Some(true), ..Default::default() };
        assert!(change_model_parameter(&mut config, "bs_roformer_leap_xe90_vocals", "overlap", "8", |_| Err("disk full".into())).is_err());
        assert!(config.model_settings.is_empty());
        change_model_parameter(&mut config, "bs_roformer_leap_xe90_vocals", "overlap", "8", |_| Ok(())).unwrap();
        assert_eq!(config.model_settings["bs_roformer_leap_xe90_vocals"]["overlap"], 8);
        assert_eq!(config.turbo_acceleration, Some(true));
        change_model_parameter(&mut config, "bs_roformer_leap_xe90_vocals", "overlap", "-10", |_| Ok(())).unwrap();
        assert_eq!(config.model_settings["bs_roformer_leap_xe90_vocals"]["overlap"], 1);
        assert!(change_model_parameter(&mut config, "rmvpe", "voiced_threshold", "NaN", |_| Ok(())).is_err());
    }
}
