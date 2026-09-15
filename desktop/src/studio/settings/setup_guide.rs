use super::*;
use crate::studio::*;

/// First-run setup guide. It opens automatically until the operator finishes
/// or skips it, and Settings > Models & runtime reopens it. Models download
/// only after a level is explicitly confirmed.
pub(crate) fn spawn_setup_guide_dialog(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    step: SetupGuideStep,
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
            BackgroundColor(theme.background.with_alpha(0.8)),
            ZIndex(110),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(match step {
                            SetupGuideStep::Welcome => 500,
                            SetupGuideStep::ChooseTier(_) => 600,
                        }),
                        max_width: percent(92),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(12),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(10)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| match step {
                    SetupGuideStep::Welcome => spawn_welcome(dialog, font, theme),
                    SetupGuideStep::ChooseTier(selected) => {
                        spawn_tier_choice(dialog, font, theme, session, selected)
                    }
                });
        });
}

fn spawn_welcome(dialog: &mut ChildSpawnerCommands, font: Handle<Font>, theme: &StudioTheme) {
    spawn_text(
        dialog,
        font.clone(),
        "Welcome to Uta! Studio",
        17.0,
        theme.foreground,
    );
    spawn_wrapped_text(
        dialog,
        font.clone(),
        "Is this your first time setting up Uta! Studio on this computer?",
        11.0,
        theme.foreground,
    );
    spawn_wrapped_text(
        dialog,
        font.clone(),
        "Setup downloads the analysis models from Hugging Face for the level you choose next. Nothing is downloaded until you confirm, and your songs are never changed.",
        10.0,
        theme.muted_foreground,
    );
    spawn_guide_buttons(
        dialog,
        font,
        theme,
        &[
            (
                "Not now",
                UiAction::from(SettingsCommand::CloseSetupGuide),
                false,
            ),
            (
                "No, skip setup",
                UiAction::from(SettingsCommand::SkipSetupGuide),
                false,
            ),
            (
                "Yes, set up",
                UiAction::from(SettingsCommand::OpenSetupTiers),
                true,
            ),
        ],
    );
}

fn tier_copy(tier: app_core::SetupTier) -> (&'static str, &'static str) {
    match tier {
        app_core::SetupTier::Standard => (
            "Standard",
            "Everything the default Balanced analysis needs: vocal separation, transcription, alignment, pitch, and notes.",
        ),
        app_core::SetupTier::Maximum => (
            "High quality",
            "Adds the note experts Maximum analysis requires, plus the FCPE pitch and FireRed transcription checks.",
        ),
        app_core::SetupTier::Complete => (
            "Complete set",
            "Every catalog model, including alternative separation, lead isolation, denoise, dereverb, and every GAME size.",
        ),
    }
}

fn tier_summary(session: &StudioSessionView<'_>, tier: app_core::SetupTier) -> String {
    let models = tier.model_ids().len();
    let Some(snapshot) = session.model_settings_job.current.as_ref() else {
        return if session.model_settings_job.receiver.is_some() {
            format!("{models} models · reading download size…")
        } else {
            format!("{models} models")
        };
    };
    let Some(option) = snapshot
        .setup_tiers
        .iter()
        .find(|option| option.tier == tier)
    else {
        return format!("{models} models");
    };
    if option.missing_model_ids.is_empty() {
        return format!("{models} models · all installed");
    }
    match option.download_bytes {
        Some(bytes) => format!(
            "{models} models · {} to download ({} missing)",
            format_bytes(bytes),
            option.missing_model_ids.len()
        ),
        None => format!(
            "{models} models · {} missing",
            option.missing_model_ids.len()
        ),
    }
}

fn spawn_tier_choice(
    dialog: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    selected: app_core::SetupTier,
) {
    spawn_text(
        dialog,
        font.clone(),
        "Choose a setup level",
        17.0,
        theme.foreground,
    );
    spawn_wrapped_text(
        dialog,
        font.clone(),
        "Uta! Studio downloads the missing models of this level. You can install others later in Settings > Models & runtime.",
        10.0,
        theme.muted_foreground,
    );
    for tier in app_core::SetupTier::ALL {
        let active = tier == selected;
        let (label, description) = tier_copy(tier);
        dialog
            .spawn((
                Button,
                UiAction::from(SettingsCommand::SelectSetupTier(tier)),
                Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(14), px(11)),
                    row_gap: px(4),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                BackgroundColor(if active {
                    theme.primary.with_alpha(0.13)
                } else {
                    theme.background.with_alpha(0.36)
                }),
                BorderColor::all(if active {
                    theme.primary.with_alpha(0.58)
                } else {
                    theme.border.with_alpha(0.45)
                }),
            ))
            .with_children(|option| {
                spawn_text(
                    option,
                    font.clone(),
                    label,
                    12.0,
                    if active {
                        theme.primary
                    } else {
                        theme.foreground
                    },
                );
                spawn_wrapped_text(
                    option,
                    font.clone(),
                    description,
                    9.5,
                    theme.muted_foreground,
                );
                spawn_text(
                    option,
                    font.clone(),
                    tier_summary(session, tier),
                    9.0,
                    theme.foreground,
                );
            });
    }
    if let Some(error) = session
        .model_settings_job
        .current
        .as_ref()
        .and_then(|snapshot| snapshot.setup_tiers_error.as_deref())
    {
        spawn_wrapped_text(
            dialog,
            font.clone(),
            format!("Model sizes are unavailable: {error}"),
            9.0,
            theme.destructive,
        );
    }
    spawn_wrapped_text(
        dialog,
        font.clone(),
        "Existing charts change only after re-analysis.",
        9.0,
        theme.muted_foreground,
    );
    spawn_guide_buttons(
        dialog,
        font,
        theme,
        &[
            (
                "Back",
                UiAction::from(SettingsCommand::CloseSetupTiers),
                false,
            ),
            (
                "Download",
                UiAction::from(SettingsCommand::ConfirmSetupTier),
                true,
            ),
        ],
    );
}

fn spawn_guide_buttons(
    dialog: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    buttons: &[(&'static str, UiAction, bool)],
) {
    dialog
        .spawn(Node {
            width: percent(100),
            justify_content: JustifyContent::FlexEnd,
            column_gap: px(8),
            margin: UiRect::top(px(4)),
            ..default()
        })
        .with_children(|row| {
            for (label, action, primary) in buttons.iter().cloned() {
                row.spawn((
                    Button,
                    action,
                    Node {
                        padding: UiRect::axes(px(13), px(8)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(if primary { theme.primary } else { Color::NONE }),
                    children![(
                        Text::new(label),
                        ui_text_font(font.clone(), 10.0),
                        TextColor(if primary {
                            theme.primary_foreground
                        } else {
                            theme.muted_foreground
                        }),
                    )],
                ));
            }
        });
}
