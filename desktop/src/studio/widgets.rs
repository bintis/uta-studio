//! Shared UI primitives: icons, buttons, text, and font scaling.

use crate::studio::*;

pub(crate) const ICON_ATLAS_PATH: &str = "desktop/assets/icons/ui-icons.png";

pub(crate) const ICON_CELL: f32 = 24.0;

pub(crate) const UI_FONT_SCALE_MIN_PERCENT: u32 = 80;

pub(crate) const UI_FONT_SCALE_MAX_PERCENT: u32 = 140;

pub(crate) const UI_FONT_BASE_SIZE_PX: u32 = 12;

pub(crate) const UI_FONT_SIZE_MIN_PX: u32 = 10;

pub(crate) const UI_FONT_SIZE_MAX_PX: u32 = 18;

pub(crate) const UI_FONT_SIZE_STEP_PX: u32 = 1;

pub(crate) const WINDOW_OPACITY_MIN_PERCENT: u32 = 30;

pub(crate) const WINDOW_OPACITY_MAX_PERCENT: u32 = 100;

pub(crate) const WINDOW_OPACITY_STEP_PERCENT: u32 = 5;

pub(crate) const WORKSPACE_TOP_BAR_MIN_HEIGHT: f32 = 72.0;
pub(crate) const STUDIO_CARD_RADIUS: f32 = 10.0;
pub(crate) const STUDIO_CONTROL_RADIUS: f32 = 8.0;
pub(crate) const STUDIO_POPOVER_RADIUS: f32 = 12.0;
pub(crate) const STUDIO_CARD_BACKGROUND_ALPHA: f32 = 0.40;
pub(crate) const STUDIO_CARD_BORDER_ALPHA: f32 = 0.62;
pub(crate) const STUDIO_CONTROL_HEIGHT: f32 = 36.0;
pub(crate) const STUDIO_CONTROL_BACKGROUND_ALPHA: f32 = 0.52;
pub(crate) const STUDIO_CONTROL_BORDER_ALPHA: f32 = 0.56;

pub(crate) static GLOBAL_UI_FONT_SCALE_BITS: AtomicU32 = AtomicU32::new(f32::to_bits(1.0));

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(crate) enum UiIcon {
    Home = 0,
    Queue = 1,
    CircleCheck = 2,
    Video = 3,
    Artists = 4,
    Albums = 5,
    List = 6,
    Folder = 7,
    Settings = 8,
    Monitor = 10,
    Database = 11,
    Box = 12,
    Sparkles = 13,
    ArrowLeft = 14,
    Undo = 15,
    Redo = 16,
    PanelRight = 17,
    Duet = 18,
    Save = 19,
    Play = 20,
    Pause = 21,
    Add = 22,
    Scissors = 23,
    Combine = 24,
    Copy = 25,
    Clipboard = 26,
    Trash = 27,
    Grid = 28,
    ZoomOut = 29,
    ZoomIn = 30,
    ChevronDown = 31,
    Search = 32,
    Close = 34,
    Music = 35,
    Repair = 36,
    Check = 38,
    Previous = 40,
    Next = 41,
    Shuffle = 42,
    Repeat = 43,
    Volume = 44,
}

impl UiIcon {
    pub(crate) fn rect(self) -> Rect {
        let left = f32::from(self as u8) * ICON_CELL;
        Rect::new(left, 0.0, left + ICON_CELL, ICON_CELL)
    }
}

#[derive(Resource, Default)]
pub(crate) struct LocalImages {
    pub(crate) covers: HashMap<PathBuf, Handle<Image>>,
}

/// The authored background of a button before transient hover/press feedback.
///
/// UI rebuilds create buttons with intentionally different resting surfaces
/// (transparent text actions, quiet outlined controls, primary actions, and
/// full-screen dismiss backdrops). Keeping that value prevents interaction
/// feedback from flattening every button to the same transparent background.
#[derive(Component, Clone, Copy)]
pub(crate) struct RestingButtonBackground(pub(crate) Color);

#[derive(Component, Clone, Copy)]
pub(crate) struct RestingButtonBorder(pub(crate) BorderColor);

pub(crate) fn studio_card_background(theme: &StudioTheme) -> BackgroundColor {
    BackgroundColor(theme.card.with_alpha(STUDIO_CARD_BACKGROUND_ALPHA))
}

pub(crate) fn studio_card_border(theme: &StudioTheme) -> BorderColor {
    BorderColor::all(theme.border.with_alpha(STUDIO_CARD_BORDER_ALPHA))
}

pub(crate) fn studio_card_radius() -> BorderRadius {
    BorderRadius::all(px(STUDIO_CARD_RADIUS))
}

pub(crate) fn studio_control_radius() -> BorderRadius {
    BorderRadius::all(px(STUDIO_CONTROL_RADIUS))
}

pub(crate) fn studio_popover_radius() -> BorderRadius {
    BorderRadius::all(px(STUDIO_POPOVER_RADIUS))
}

pub(crate) fn studio_card_shadow(theme: &StudioTheme) -> BoxShadow {
    BoxShadow::new(
        Color::srgba(0.0, 0.0, 0.0, if theme.dark { 0.22 } else { 0.10 }),
        px(0),
        px(10),
        px(28),
        px(-12),
    )
}

pub(crate) fn studio_popover_shadow(theme: &StudioTheme) -> BoxShadow {
    BoxShadow::new(
        Color::srgba(0.0, 0.0, 0.0, if theme.dark { 0.34 } else { 0.16 }),
        px(0),
        px(14),
        px(34),
        px(-10),
    )
}

pub(crate) fn spawn_icon(
    parent: &mut ChildSpawnerCommands,
    atlas: Handle<Image>,
    icon: UiIcon,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Node {
            width: px(size),
            height: px(size),
            flex_shrink: 0.0,
            ..default()
        },
        ImageNode::new(atlas)
            .with_rect(icon.rect())
            .with_color(color),
        Pickable::IGNORE,
    ));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_icon_button(
    parent: &mut ChildSpawnerCommands,
    atlas: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    action: UiAction,
    active: bool,
    destructive: bool,
    size: f32,
) {
    let color = if destructive {
        theme.destructive
    } else if active {
        theme.primary
    } else {
        theme.muted_foreground
    };
    parent
        .spawn((
            Button,
            action,
            Node {
                width: px(size),
                height: px(size),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1)),
                border_radius: studio_control_radius(),
                ..default()
            },
            BackgroundColor(if active {
                theme.primary.with_alpha(0.12)
            } else {
                Color::NONE
            }),
            BorderColor::all(if active {
                theme.primary.with_alpha(0.42)
            } else {
                Color::NONE
            }),
        ))
        .with_children(|button| spawn_icon(button, atlas, icon, 16.0, color));
}

pub(crate) fn spawn_activity_button(
    parent: &mut ChildSpawnerCommands,
    atlas: Handle<Image>,
    theme: &StudioTheme,
    panel_open: bool,
    has_active_analysis: bool,
) {
    let emphasized = panel_open || has_active_analysis;
    let color = if has_active_analysis {
        theme.primary
    } else if panel_open {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    parent
        .spawn((
            Node {
                width: px(34),
                height: px(34),
                flex_shrink: 0.0,
                border: UiRect::all(px(if emphasized { 1 } else { 0 })),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(if has_active_analysis {
                theme.primary.with_alpha(0.075)
            } else if panel_open {
                theme.foreground.with_alpha(0.07)
            } else {
                Color::NONE
            }),
            BorderColor::all(if has_active_analysis {
                theme.primary.with_alpha(0.18)
            } else {
                theme.border.with_alpha(0.42)
            }),
        ))
        .with_children(|slot| {
            slot.spawn((
                Button,
                UiAction::from(AppCommand::ToggleActivity),
                Node {
                    width: percent(100),
                    height: percent(100),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(px(5)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|button| spawn_icon(button, atlas, UiIcon::Queue, 16.0, color));
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_toolbar_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    atlas: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    label: impl Into<String>,
    action: UiAction,
    destructive: bool,
) {
    let color = if destructive {
        theme.destructive
    } else {
        theme.foreground
    };
    parent
        .spawn((
            Button,
            action,
            Node {
                height: px(STUDIO_CONTROL_HEIGHT),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(11)),
                column_gap: px(7),
                border: UiRect::all(px(1)),
                border_radius: studio_control_radius(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(STUDIO_CONTROL_BACKGROUND_ALPHA)),
            BorderColor::all(theme.border.with_alpha(STUDIO_CONTROL_BORDER_ALPHA)),
        ))
        .with_children(|button| {
            spawn_icon(button, atlas, icon, 14.0, color);
            spawn_text(button, font, label, 9.5, color);
        });
}

pub(crate) fn spawn_section_label(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
) {
    parent.spawn((
        Node {
            margin: UiRect::new(px(8), px(0), px(18), px(8)),
            ..default()
        },
        children![(
            Text::new(label),
            ui_text_font(font, 9.0),
            TextColor(theme.sidebar_foreground.with_alpha(0.42)),
        )],
    ));
}

pub(crate) fn spawn_compact_action_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    action: UiAction,
) {
    parent.spawn((
        Button,
        action,
        Node {
            min_width: px(0),
            max_width: percent(100),
            min_height: px(STUDIO_CONTROL_HEIGHT),
            flex_shrink: 1.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(12), px(8)),
            border: UiRect::all(px(1)),
            border_radius: studio_control_radius(),
            ..default()
        },
        BackgroundColor(theme.card.with_alpha(STUDIO_CONTROL_BACKGROUND_ALPHA)),
        BorderColor::all(theme.border.with_alpha(STUDIO_CONTROL_BORDER_ALPHA)),
        children![(
            Text::new(label),
            ui_text_font(font, 9.5),
            TextColor(theme.foreground),
            TextLayout {
                linebreak: bevy::text::LineBreak::WordOrCharacter,
                justify: Justify::Center,
            },
        )],
    ));
}

/// A `spawn_compact_action_button` variant reserved for the single primary
/// action on a page. The solid accent surface gives the page one obvious next
/// step while secondary controls remain quiet and outlined.
pub(crate) fn spawn_compact_primary_action_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    action: UiAction,
) {
    parent.spawn((
        Button,
        action,
        Node {
            min_width: px(0),
            max_width: percent(100),
            min_height: px(STUDIO_CONTROL_HEIGHT),
            flex_shrink: 1.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(13), px(8)),
            border: UiRect::all(px(1)),
            border_radius: studio_control_radius(),
            ..default()
        },
        BackgroundColor(theme.primary.with_alpha(0.92)),
        BorderColor::all(theme.primary),
        children![(
            Text::new(label),
            ui_text_font(font, 9.5),
            TextColor(theme.primary_foreground),
            TextLayout {
                linebreak: bevy::text::LineBreak::WordOrCharacter,
                justify: Justify::Center,
            },
        )],
    ));
}

pub(crate) fn spawn_action_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    action: UiAction,
) {
    parent.spawn((
        Button,
        action,
        Node {
            min_width: px(136),
            max_width: percent(100),
            min_height: px(STUDIO_CONTROL_HEIGHT),
            flex_shrink: 1.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(13), px(8)),
            border: UiRect::all(px(1)),
            border_radius: studio_control_radius(),
            ..default()
        },
        BackgroundColor(theme.card.with_alpha(STUDIO_CONTROL_BACKGROUND_ALPHA)),
        BorderColor::all(theme.border.with_alpha(STUDIO_CONTROL_BORDER_ALPHA)),
        children![(
            Text::new(label),
            ui_text_font(font, 10.0),
            TextColor(theme.foreground),
            TextLayout {
                linebreak: bevy::text::LineBreak::WordOrCharacter,
                justify: Justify::Center,
            },
        )],
    ));
}

pub(crate) fn spawn_status_pill(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    label: impl Into<String>,
    color: Color,
) {
    parent.spawn((
        Node {
            min_height: px(24),
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(9), px(4)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(color.with_alpha(0.10)),
        BorderColor::all(color.with_alpha(0.34)),
        children![(
            Text::new(label),
            ui_text_font(font, 8.0),
            TextColor(color),
            TextLayout::no_wrap(),
        )],
    ));
}

pub(crate) fn spawn_progress_bar(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    progress: usize,
    accent: Color,
) {
    let progress = progress.clamp(0, 100);
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(7),
                overflow: Overflow::clip(),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(theme.border.with_alpha(0.34)),
        ))
        .with_children(|track| {
            track.spawn((
                Node {
                    width: percent(progress as f32),
                    min_width: if progress > 0 { px(3) } else { px(0) },
                    height: percent(100),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(accent),
            ));
        });
}

#[allow(dead_code)]
pub(crate) fn spawn_metric_tile(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    value: impl Into<String>,
    accent: Color,
) {
    parent
        .spawn((
            Node {
                min_width: px(118),
                flex_basis: px(150),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(11)),
                row_gap: px(3),
                border: UiRect::all(px(1)),
                border_radius: studio_control_radius(),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.34)),
            BorderColor::all(theme.border.with_alpha(0.44)),
        ))
        .with_children(|tile| {
            spawn_text(tile, font.clone(), label, 7.5, theme.muted_foreground);
            spawn_bounded_wrapped_text(tile, font, value, 10.0, accent);
        });
}

pub(crate) fn spawn_wrapped_text(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    text: impl Into<String>,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Text::new(text),
        ui_text_font(font, size),
        TextColor(color),
        TextLayout {
            linebreak: bevy::text::LineBreak::WordOrCharacter,
            ..default()
        },
    ));
}

pub(crate) fn spawn_bounded_wrapped_text(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    text: impl Into<String>,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Node {
            width: percent(100),
            min_width: px(0),
            ..default()
        },
        Text::new(text),
        ui_text_font(font, size),
        TextColor(color),
        TextLayout {
            linebreak: bevy::text::LineBreak::WordOrCharacter,
            ..default()
        },
    ));
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(crate) fn ui_font_scale() -> f32 {
    if let Ok(value) = std::env::var("UTA_STUDIO_DEBUG_UI_SCALE")
        && let Ok(scale) = value.parse::<f32>()
    {
        return scale.clamp(0.8, 1.4);
    }
    f32::from_bits(GLOBAL_UI_FONT_SCALE_BITS.load(Ordering::SeqCst))
}

pub(crate) fn ui_font_size_percent_to_points(scale_percent: u32) -> u32 {
    let size = (scale_percent as f32) * (UI_FONT_BASE_SIZE_PX as f32) / 100.0;
    size.round()
        .clamp(UI_FONT_SIZE_MIN_PX as f32, UI_FONT_SIZE_MAX_PX as f32) as u32
}

pub(crate) fn ui_font_points_to_scale_percent(size_px: u32) -> u32 {
    let clamped = size_px.clamp(UI_FONT_SIZE_MIN_PX, UI_FONT_SIZE_MAX_PX);
    let percent = (clamped as f32) * 100.0 / (UI_FONT_BASE_SIZE_PX as f32);
    percent.round().clamp(
        UI_FONT_SCALE_MIN_PERCENT as f32,
        UI_FONT_SCALE_MAX_PERCENT as f32,
    ) as u32
}

pub(crate) fn set_ui_font_scale(scale: f32) {
    let scale = scale.clamp(0.25, 2.0);
    GLOBAL_UI_FONT_SCALE_BITS.store(scale.to_bits(), Ordering::SeqCst);
}

pub(crate) fn ui_font_size(size: f32) -> f32 {
    size * ui_font_scale()
}

pub(crate) fn ui_text_font(font: Handle<Font>, size: f32) -> TextFont {
    TextFont::from(font).with_font_size(ui_font_size(size))
}

pub(crate) fn spawn_menu_text_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    size: f32,
    action: UiAction,
) {
    let label = label.into();
    parent.spawn((
        Button,
        action,
        Node {
            width: percent(100),
            min_height: px(34),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::FlexStart,
            padding: UiRect::axes(px(10), px(6)),
            border_radius: BorderRadius::all(px(7)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![(
            Text::new(label),
            ui_text_font(font, size),
            TextColor(theme.foreground),
            TextLayout::no_wrap(),
        )],
    ));
}

pub(crate) fn spawn_text_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    size: f32,
    action: UiAction,
) {
    let label = label.into();
    parent.spawn((
        Button,
        action,
        Node {
            min_width: px(32),
            height: px(32),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(6)),
            border_radius: studio_control_radius(),
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![(
            Text::new(label),
            ui_text_font(font, size),
            TextColor(theme.foreground),
        )],
    ));
}

pub(crate) fn spawn_text(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    text: impl Into<String>,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Text::new(text),
        ui_text_font(font, size),
        TextColor(color),
        TextLayout::no_wrap(),
    ));
}

type ActionButtons<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Interaction,
        Option<&'static UiAction>,
        &'static mut BackgroundColor,
        Option<&'static RestingButtonBackground>,
        Option<&'static mut BorderColor>,
        Option<&'static RestingButtonBorder>,
    ),
    Or<(Added<Button>, Changed<Interaction>)>,
>;

pub(crate) fn update_button_visuals(
    mut commands: Commands,
    theme: Res<StudioTheme>,
    mut buttons: ActionButtons,
) {
    for (entity, interaction, action, mut background, resting_background, border, resting_border) in
        &mut buttons
    {
        let has_recorded_background = resting_background.is_some();
        let resting_background = resting_background.map_or(background.0, |resting| resting.0);
        if !has_recorded_background {
            commands
                .entity(entity)
                .try_insert(RestingButtonBackground(resting_background));
        }
        background.0 = button_background_for_target(
            action.is_none_or(action_is_navigation_target),
            *interaction,
            resting_background,
            &theme,
        );

        if let Some(mut border) = border {
            let has_recorded_border = resting_border.is_some();
            let resting_border = resting_border.map_or(*border, |resting| resting.0);
            if !has_recorded_border {
                commands
                    .entity(entity)
                    .try_insert(RestingButtonBorder(resting_border));
            }
            *border = button_border_for_target(
                action.is_none_or(action_is_navigation_target),
                *interaction,
                resting_border,
                &theme,
            );
        }
    }
}

#[cfg(test)]
pub(crate) fn button_background(
    action: &UiAction,
    interaction: Interaction,
    resting: Color,
    theme: &StudioTheme,
) -> Color {
    button_background_for_target(
        action_is_navigation_target(action),
        interaction,
        resting,
        theme,
    )
}

fn button_background_for_target(
    interactive: bool,
    interaction: Interaction,
    resting: Color,
    theme: &StudioTheme,
) -> Color {
    // Full-surface dismiss targets are intentionally invisible controls that
    // cover the whole screen — the same set `action_is_navigation_target`
    // excludes from tab order. A hover highlight on one of these isn't a
    // subtle accent under the cursor: since the pointer is always somewhere
    // on screen, it's an immediate solid tint over everything the moment the
    // backdrop exists.
    if !interactive {
        return resting;
    }
    match interaction {
        Interaction::None => resting,
        Interaction::Hovered if resting == Color::NONE => theme
            .sidebar_accent
            .mix(&theme.primary, 0.24)
            .with_alpha(if theme.dark { 0.62 } else { 0.72 }),
        Interaction::Pressed if resting == Color::NONE => theme
            .sidebar_accent
            .mix(&theme.primary, 0.34)
            .with_alpha(if theme.dark { 0.82 } else { 0.90 }),
        Interaction::Hovered => resting.mix(&theme.foreground, 0.08),
        Interaction::Pressed => resting.mix(&theme.foreground, 0.16),
    }
}

#[cfg(test)]
pub(crate) fn button_border(
    action: &UiAction,
    interaction: Interaction,
    resting: BorderColor,
    theme: &StudioTheme,
) -> BorderColor {
    button_border_for_target(
        action_is_navigation_target(action),
        interaction,
        resting,
        theme,
    )
}

fn button_border_for_target(
    interactive: bool,
    interaction: Interaction,
    resting: BorderColor,
    theme: &StudioTheme,
) -> BorderColor {
    if !interactive {
        return resting;
    }
    let mix = match interaction {
        Interaction::None => return resting,
        Interaction::Hovered => 0.34,
        Interaction::Pressed => 0.58,
    };
    BorderColor {
        top: resting.top.mix(&theme.foreground, mix),
        right: resting.right.mix(&theme.foreground, mix),
        bottom: resting.bottom.mix(&theme.foreground, mix),
        left: resting.left.mix(&theme.foreground, mix),
    }
}

pub(crate) fn ui_node_contains_pointer(
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
    pointer: Vec2,
) -> bool {
    let size = computed.size() * computed.inverse_scale_factor();
    let local = transform.affine().inverse().transform_point2(pointer);
    local.x.abs() <= size.x / 2.0 && local.y.abs() <= size.y / 2.0
}

pub(crate) fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0:00".to_string();
    }
    let total = seconds.round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}
