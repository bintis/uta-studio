//! Discoverable canvas chrome for the Advanced Graph viewport. Both the
//! pan/zoom hint and the Fit/Zoom/Follow control cluster are
//! absolutely-positioned overlays inside the viewport frame -- bottom-left
//! and bottom-right respectively -- so the canvas itself keeps the full
//! available height (§12) instead of losing a row to either.

use super::*;
use crate::studio::*;

fn spawn_viewport_control_button(
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
                min_width: px(22.0),
                height: px(20.0),
                padding: UiRect::horizontal(px(6.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(theme.background.alpha() * 0.32)),
        ))
        .with_children(|button| {
            spawn_text(button, font, label, 9.0, theme.foreground);
        });
}

fn spawn_follow_toggle(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    active: bool,
    available: bool,
) {
    let mut control = parent.spawn((
        Node {
            height: px(20.0),
            padding: UiRect::horizontal(px(8.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(4.0)),
            ..default()
        },
        BackgroundColor(if active {
            theme.primary.with_alpha(0.16)
        } else {
            theme.background.with_alpha(theme.background.alpha() * 0.32)
        }),
        BorderColor::all(if active {
            theme.primary.with_alpha(0.6)
        } else {
            Color::NONE
        }),
    ));
    if available {
        control.insert((
            Button,
            UiAction::from(AnalysisCommand::ToggleAnalysisGraphFollow),
        ));
    } else {
        control.insert(Pickable::IGNORE);
    }
    control.with_children(|button| {
        spawn_text(
            button,
            font,
            "Follow",
            9.0,
            if !available {
                theme.muted_foreground.with_alpha(0.5)
            } else if active {
                theme.primary
            } else {
                theme.muted_foreground
            },
        );
    });
}

/// `[Fit] [-] [zoom%] [+] [Follow]`, floating in the canvas's own
/// bottom-right corner -- the pan hint (§ its own doc comment) occupies the
/// bottom-left, so the two never collide. Zoom buttons and Fit dispatch the
/// exact existing commands/systems (`AdjustAnalysisGraphZoom`, the
/// `fit_analysis_graph_to_viewport` system via `FitAnalysisGraph`) rather
/// than a second implementation.
pub(crate) fn spawn_analysis_graph_viewport_controls(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    zoom: f32,
    follow_active: bool,
    follow_available: bool,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(10.0),
                bottom: px(10.0),
                align_items: AlignItems::Center,
                column_gap: px(5.0),
                padding: UiRect::axes(px(7.0), px(4.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.6)),
            ZIndex(20),
        ))
        .with_children(|controls| {
            spawn_viewport_control_button(
                controls,
                font.clone(),
                theme,
                "Fit",
                UiAction::from(AnalysisCommand::FitAnalysisGraph),
            );
            spawn_viewport_control_button(
                controls,
                font.clone(),
                theme,
                "-",
                UiAction::from(AnalysisCommand::AdjustAnalysisGraphZoom(
                    -((ANALYSIS_GRAPH_ZOOM_STEP * 100.0).round() as i32),
                )),
            );
            spawn_text(
                controls,
                font.clone(),
                format!("{}%", (zoom * 100.0).round() as i32),
                8.5,
                theme.muted_foreground,
            );
            spawn_viewport_control_button(
                controls,
                font.clone(),
                theme,
                "+",
                UiAction::from(AnalysisCommand::AdjustAnalysisGraphZoom(
                    (ANALYSIS_GRAPH_ZOOM_STEP * 100.0).round() as i32,
                )),
            );
            spawn_follow_toggle(controls, font, theme, follow_active, follow_available);
        });
}

pub(crate) fn spawn_analysis_graph_pan_hint(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(10.0),
            bottom: px(10.0),
            padding: UiRect::axes(px(7.0), px(4.0)),
            border_radius: BorderRadius::all(px(4.0)),
            ..default()
        },
        BackgroundColor(theme.card.with_alpha(0.6)),
        ZIndex(20),
        Pickable::IGNORE,
        children![(
            Text::new("Drag to pan · Ctrl + wheel to zoom"),
            ui_text_font(font, 7.5),
            TextColor(theme.muted_foreground),
            TextLayout::no_wrap(),
        )],
    ));
}

// The status color key used to live here as a floating overlay on top of
// the canvas; it now renders inline in the context bar above the canvas
// (`spawn_context_bar_status_chip` in graph_summary.rs), next to the counts
// it already shared status categories with -- see that module for why.
