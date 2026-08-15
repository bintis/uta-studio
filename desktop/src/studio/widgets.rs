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
    PanelBottom = 18,
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
        theme.foreground
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
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(if active {
                theme.foreground.with_alpha(0.07)
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
                UiAction::ToggleActivity,
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
                height: px(32),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(9)),
                column_gap: px(6),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.38)),
            BorderColor::all(theme.border.with_alpha(0.44)),
        ))
        .with_children(|button| {
            spawn_icon(button, atlas, icon, 14.0, color);
            spawn_text(button, font, label, 9.0, color);
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
            height: px(34),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(11)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.52)),
        BorderColor::all(theme.border.with_alpha(0.66)),
        children![(
            Text::new(label),
            ui_text_font(font, 9.0),
            TextColor(theme.foreground),
            TextLayout::no_wrap(),
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
            height: px(34),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(12)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.52)),
        BorderColor::all(theme.border.with_alpha(0.66)),
        children![(
            Text::new(label),
            ui_text_font(font, 10.0),
            TextColor(theme.foreground),
            TextLayout::no_wrap(),
        )],
    ));
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
        TextLayout::default(),
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

pub(crate) fn availability(available: bool) -> &'static str {
    if available { "available" } else { "missing" }
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
            min_width: px(28),
            height: px(32),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(3)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![(
            Text::new(label),
            ui_text_font(font, size),
            TextColor(theme.sidebar_foreground),
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

pub(crate) fn update_button_visuals(
    mut commands: Commands,
    theme: Res<StudioTheme>,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &UiAction,
            &mut BackgroundColor,
            Option<&RestingButtonBackground>,
        ),
        Or<(Added<Button>, Changed<Interaction>)>,
    >,
) {
    for (entity, interaction, action, mut background, resting) in &mut buttons {
        let has_recorded_background = resting.is_some();
        let resting = resting.map_or(background.0, |resting| resting.0);
        if !has_recorded_background {
            commands
                .entity(entity)
                .try_insert(RestingButtonBackground(resting));
        }
        background.0 = button_background(action, *interaction, resting, &theme);
    }
}

pub(crate) fn button_background(
    action: &UiAction,
    interaction: Interaction,
    resting: Color,
    theme: &StudioTheme,
) -> Color {
    // Full-surface dismiss targets are intentionally invisible controls. A
    // hover highlight here reads as if the obscured page itself was selected.
    if matches!(action, UiAction::CloseActivity) {
        return resting;
    }
    match interaction {
        Interaction::None => resting,
        Interaction::Hovered if resting == Color::NONE => theme.sidebar_accent.with_alpha(0.48),
        Interaction::Pressed if resting == Color::NONE => theme.sidebar_accent.with_alpha(0.72),
        Interaction::Hovered => resting.mix(&theme.foreground, 0.06),
        Interaction::Pressed => resting.mix(&theme.foreground, 0.12),
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
