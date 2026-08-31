//! Discoverable canvas chrome for the Advanced Graph viewport: the
//! Fit/Zoom/Follow control cluster, a compact status legend, and the
//! pan/zoom hint. All three are absolutely-positioned overlays inside the
//! viewport so the canvas itself keeps the full available height (§12).

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
            BackgroundColor(theme.background.with_alpha(0.32)),
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
            theme.background.with_alpha(0.32)
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

/// Bottom-right cluster: `[Fit] [-] [zoom%] [+] [Follow]`. Zoom buttons and
/// Fit dispatch the exact existing commands/systems
/// (`AdjustAnalysisGraphZoom`, the `fit_analysis_graph_to_viewport` system
/// via `FitAnalysisGraph`) rather than a second implementation.
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
                padding: UiRect::axes(px(6.0), px(4.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.88)),
            BorderColor::all(theme.border.with_alpha(0.5)),
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

fn spawn_analysis_graph_legend_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    accent: Color,
) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(4.0),
            ..default()
        })
        .with_children(|item| {
            item.spawn((
                Node {
                    width: px(7.0),
                    height: px(7.0),
                    flex_shrink: 0.0,
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(accent),
            ));
            spawn_text(item, font, label, 7.0, theme.muted_foreground);
        });
}

/// Compact status-only overlay (§11). Category color already appears on
/// every card's top accent strip/glyph/model text, so this legend does not
/// repeat it -- repeating the same color for both category and status was
/// the exact confusion the previous full-width legend caused.
pub(crate) fn spawn_analysis_graph_legend(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(10.0),
                top: px(10.0),
                align_items: AlignItems::Center,
                column_gap: px(9.0),
                flex_wrap: FlexWrap::Wrap,
                row_gap: px(4.0),
                padding: UiRect::axes(px(8.0), px(5.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(5.0)),
                max_width: px(320.0),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.82)),
            BorderColor::all(theme.border.with_alpha(0.42)),
            ZIndex(20),
            Pickable::IGNORE,
        ))
        .with_children(|legend| {
            // Same fixed status colors `spawn_workflow_graph_node` assigns
            // to every card regardless of its own category, so the legend
            // and the canvas always agree.
            let complete = analysis_graph_category_accent(GraphNodeCategory::Output, theme);
            for (label, accent) in [
                ("Complete", complete),
                ("Running", theme.primary),
                ("Waiting", theme.muted_foreground),
                ("Failed", theme.destructive),
                ("Deferred", theme.editor_warning),
                ("Not requested", theme.muted_foreground.with_alpha(0.6)),
            ] {
                spawn_analysis_graph_legend_item(legend, font.clone(), theme, label, accent);
            }
        });
}
