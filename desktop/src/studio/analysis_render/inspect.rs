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
