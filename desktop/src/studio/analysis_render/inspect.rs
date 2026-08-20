use super::*;
use crate::studio::*;

#[derive(Component)]
pub(crate) struct AnalysisInspectPage;

pub(crate) fn spawn_analysis_inspect_page(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let current = current_analysis_header(session);
    let stage = session
        .selected_analysis_stage
        .as_deref()
        .unwrap_or("preparing");
    let (stage_label, _, _, _) = analysis_stage_details(stage);
    parent
        .spawn((
            AnalysisInspectPage,
            ScrollPosition::default(),
            Node {
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .with_children(|page| {
            page.spawn((
                Node {
                    width: percent(100),
                    padding: UiRect::axes(px(28), px(18)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    border: UiRect::bottom(px(1)),
                    ..default()
                },
                BorderColor::all(theme.border.with_alpha(0.55)),
            ))
            .with_children(|header| {
                spawn_text(
                    header,
                    font.clone(),
                    "INSPECT VIEW",
                    9.0,
                    theme.muted_foreground,
                );
                spawn_text(
                    header,
                    font.clone(),
                    if let Some((title, _, _)) = current.as_ref() {
                        format!("{title} · {stage_label}")
                    } else {
                        stage_label.to_string()
                    },
                    24.0,
                    theme.foreground,
                );
                spawn_text(
                    header,
                    font.clone(),
                    "Right-click a DAG node and choose Inspect view to open this page. Back returns to the analysis graph.",
                    10.0,
                    theme.muted_foreground,
                );
            });
            spawn_analysis_inspect_surface(page, font.clone(), session, theme);
        });
}

pub(crate) fn handle_analysis_inspect_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    shell: Res<ShellState>,
    mut pages: Query<(&ComputedNode, &mut ScrollPosition), With<AnalysisInspectPage>>,
) {
    if shell.route != StudioRoute::AnalysisInspect {
        return;
    }
    let Ok((computed, mut position)) = pages.single_mut() else {
        return;
    };
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 28.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            -event.y * scale
        })
        .sum::<f32>();
    if delta.abs() < f32::EPSILON {
        return;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
}
