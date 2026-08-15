//! Folders route: watched roots, browsing, and folder actions.

use crate::studio::*;

#[derive(Clone)]
pub(crate) struct FolderContextMenu {
    pub(crate) entry: LibraryFolderEntry,
    pub(crate) position: Vec2,
}

#[derive(Default)]
pub(crate) struct FolderBrowser {
    pub(crate) root: Option<PathBuf>,
    pub(crate) current: Option<PathBuf>,
    pub(crate) entries: Vec<LibraryFolderEntry>,
    pub(crate) error: Option<String>,
    pub(crate) context_menu: Option<FolderContextMenu>,
    pub(crate) pending_remove: Option<PathBuf>,
}

impl FolderBrowser {
    pub(crate) fn new(config: &AppConfig) -> Self {
        let root = config.library_paths().into_iter().next();
        let mut browser = Self {
            root: root.clone(),
            current: root,
            ..default()
        };
        browser.refresh();
        browser
    }

    pub(crate) fn select_root(&mut self, path: PathBuf) {
        self.root = Some(path.clone());
        self.current = Some(path);
        self.context_menu = None;
        self.refresh();
    }

    pub(crate) fn refresh(&mut self) {
        self.entries.clear();
        self.error = None;
        let Some(current) = self.current.as_deref() else {
            return;
        };
        match app_core::list_library_folder(current) {
            Ok(entries) => self.entries = entries,
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub(crate) fn parent(&self) -> Option<PathBuf> {
        let root = self.root.as_deref()?;
        let current = self.current.as_deref()?;
        if current == root {
            return None;
        }
        let parent = current.parent()?;
        parent.starts_with(root).then(|| parent.to_path_buf())
    }
}

#[derive(Component)]
pub(crate) struct FolderEntryList;

pub(crate) fn spawn_folders(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            position_type: PositionType::Relative,
            min_width: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(34), px(26)),
            row_gap: px(18),
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|page| {
            page.spawn((
                Node {
                    width: percent(100),
                    min_height: px(70),
                    flex_shrink: 0.0,
                    align_items: AlignItems::FlexEnd,
                    padding: UiRect::bottom(px(16)),
                    border: UiRect::bottom(px(1)),
                    ..default()
                },
                BorderColor::all(theme.border.with_alpha(0.45)),
            ))
            .with_children(|header| {
                header
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_text(copy, font.clone(), "MY LIBRARY", 8.0, theme.primary);
                        spawn_text(copy, font.clone(), "Folders", 24.0, theme.foreground);
                        spawn_wrapped_text(
                            copy,
                            font.clone(),
                            "Browse watched source locations and open the configured output folder. Uta Studio never moves or deletes source media.",
                            10.0,
                            theme.muted_foreground,
                        );
                    });
                spawn_toolbar_button(
                    header,
                    font.clone(),
                    icons.clone(),
                    theme,
                    UiIcon::Repeat,
                    "Rescan all",
                    UiAction::RescanLibrary,
                    false,
                );
                spawn_toolbar_button(
                    header,
                    font.clone(),
                    icons.clone(),
                    theme,
                    UiIcon::Add,
                    "Add folder",
                    UiAction::ChooseFolder,
                    false,
                );
            });

            page.spawn(Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                column_gap: px(16),
                ..default()
            })
            .with_children(|body| {
                body.spawn((
                    Node {
                        width: px(240),
                        height: percent(100),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(10)),
                        row_gap: px(3),
                        overflow: Overflow::clip(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.44)),
                    BorderColor::all(theme.border.with_alpha(0.46)),
                ))
                .with_children(|roots| {
                    spawn_text(
                        roots,
                        font.clone(),
                        format!(
                            "WATCHED LOCATIONS · {}",
                            session.config.library_paths().len()
                        ),
                        8.0,
                        theme.muted_foreground,
                    );
                    roots.spawn(Node {
                        height: px(5),
                        ..default()
                    });
                    for root in session.config.library_paths() {
                        let selected = session.folder_browser.root.as_ref() == Some(&root);
                        roots
                            .spawn((
                                Node {
                                    width: percent(100),
                                    min_height: px(46),
                                    align_items: AlignItems::Center,
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(if selected {
                                    theme.foreground.with_alpha(0.07)
                                } else {
                                    Color::NONE
                                }),
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    Button,
                                    UiAction::SelectFolderRoot(root.clone()),
                                    Node {
                                        min_width: px(0),
                                        height: percent(100),
                                        flex_grow: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Center,
                                        padding: UiRect::horizontal(px(8)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                    children![
                                        (
                                            Text::new(folder_name(&root)),
                                            ui_text_font(font.clone(), 11.0),
                                            TextColor(theme.foreground),
                                            TextLayout::no_wrap(),
                                        ),
                                        (
                                            Text::new(root.to_string_lossy().into_owned()),
                                            ui_text_font(font.clone(), 8.0),
                                            TextColor(theme.muted_foreground.with_alpha(0.64)),
                                            TextLayout::no_wrap(),
                                        )
                                    ],
                                ));
                                spawn_text_button(
                                    row,
                                    font.clone(),
                                    theme,
                                    "×",
                                    13.0,
                                    UiAction::RequestRemoveFolder(root),
                                );
                            });
                    }
                    if session.config.library_paths().is_empty() {
                        spawn_wrapped_text(
                            roots,
                            font.clone(),
                            "No folders added yet.",
                            10.0,
                            theme.muted_foreground,
                        );
                    }
                    roots.spawn((
                        Node {
                            width: percent(100),
                            margin: UiRect::top(px(12)),
                            padding: UiRect::top(px(12)),
                            border: UiRect::top(px(1)),
                            ..default()
                        },
                        BorderColor::all(theme.border.with_alpha(0.42)),
                    ));
                    spawn_text(
                        roots,
                        font.clone(),
                        "OUTPUT FOLDER",
                        8.0,
                        theme.muted_foreground,
                    );
                    roots.spawn(Node {
                        height: px(5),
                        ..default()
                    });
                    if let Some(path) = session.config.export_path.as_ref() {
                        let selected = session.folder_browser.root.as_ref() == Some(path);
                        roots
                            .spawn((
                                Node {
                                    width: percent(100),
                                    min_height: px(52),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(if selected {
                                    theme.foreground.with_alpha(0.07)
                                } else {
                                    Color::NONE
                                }),
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    Button,
                                    UiAction::SelectFolderRoot(path.clone()),
                                    Node {
                                        width: percent(100),
                                        min_height: px(52),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::FlexStart,
                                        padding: UiRect::horizontal(px(8)),
                                        row_gap: px(2),
                                        border_radius: BorderRadius::all(px(4)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                ))
                                .with_children(|output| {
                                    spawn_text(
                                        output,
                                        font.clone(),
                                        folder_name(path),
                                        11.0,
                                        theme.foreground,
                                    );
                                    output
                                        .spawn(Node {
                                            width: percent(100),
                                            overflow: Overflow::clip(),
                                            ..default()
                                        })
                                        .with_children(|path_copy| {
                                            path_copy.spawn((
                                                Text::new(path.to_string_lossy().into_owned()),
                                                ui_text_font(font.clone(), 8.0),
                                                TextColor(
                                                    theme.muted_foreground.with_alpha(0.64),
                                                ),
                                                TextLayout::no_wrap(),
                                            ));
                                        });
                                });
                            });
                    } else {
                        roots
                            .spawn((
                                Button,
                                UiAction::ChooseExportFolder,
                                Node {
                                    width: percent(100),
                                    min_height: px(52),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::FlexStart,
                                    padding: UiRect::horizontal(px(8)),
                                    row_gap: px(2),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(theme.foreground.with_alpha(0.035)),
                            ))
                            .with_children(|output| {
                                spawn_text(
                                    output,
                                    font.clone(),
                                    "System default",
                                    11.0,
                                    theme.foreground,
                                );
                                spawn_wrapped_text(
                                    output,
                                    font.clone(),
                                    "Choose an output folder",
                                    8.0,
                                    theme.muted_foreground,
                                );
                            });
                    }
                });

                body.spawn((
                    Node {
                        min_width: px(0),
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::clip(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.42)),
                    BorderColor::all(theme.border.with_alpha(0.46)),
                ))
                .with_children(|browser| {
                    browser
                        .spawn((
                            Node {
                                width: percent(100),
                                height: px(48),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(12)),
                                column_gap: px(8),
                                border: UiRect::bottom(px(1)),
                                ..default()
                            },
                            BorderColor::all(theme.border.with_alpha(0.45)),
                        ))
                        .with_children(|path_bar| {
                            spawn_icon_button(
                                path_bar,
                                icons.clone(),
                                theme,
                                UiIcon::ArrowLeft,
                                UiAction::FolderUp,
                                false,
                                false,
                                32.0,
                            );
                            spawn_icon(
                                path_bar,
                                icons.clone(),
                                UiIcon::Folder,
                                15.0,
                                theme.primary,
                            );
                            spawn_text(
                                path_bar,
                                font.clone(),
                                session
                                    .folder_browser
                                    .current
                                    .as_ref()
                                    .map(|path| path.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "Choose a watched folder".to_string()),
                                10.0,
                                theme.muted_foreground,
                            );
                        });

                    browser
                        .spawn((
                            Node {
                                width: percent(100),
                                height: px(30),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(14)),
                                border: UiRect::bottom(px(1)),
                                ..default()
                            },
                            BorderColor::all(theme.border.with_alpha(0.32)),
                        ))
                        .with_children(|columns| {
                            spawn_text(columns, font.clone(), "Name", 9.0, theme.muted_foreground);
                            columns.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_text(columns, font.clone(), "Kind", 9.0, theme.muted_foreground);
                            columns.spawn(Node {
                                width: px(50),
                                ..default()
                            });
                            spawn_text(columns, font.clone(), "Size", 9.0, theme.muted_foreground);
                        });

                    browser
                        .spawn((
                            FolderEntryList,
                            ScrollPosition::default(),
                            Node {
                                min_width: px(0),
                                min_height: px(0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ))
                        .with_children(|list| {
                            if let Some(error) = session.folder_browser.error.as_deref() {
                                spawn_wrapped_text(
                                    list,
                                    font.clone(),
                                    format!("Could not read this folder: {error}"),
                                    10.0,
                                    theme.destructive,
                                );
                            } else if session.folder_browser.current.is_none() {
                                spawn_wrapped_text(
                                    list,
                                    font.clone(),
                                    "Add a music folder to begin.",
                                    11.0,
                                    theme.muted_foreground,
                                );
                            } else {
                                for entry in &session.folder_browser.entries {
                                    spawn_folder_entry(
                                        list,
                                        font.clone(),
                                        icons.clone(),
                                        theme,
                                        entry,
                                    );
                                }
                                if session.folder_browser.entries.is_empty() {
                                    spawn_wrapped_text(
                                        list,
                                        font.clone(),
                                        "This folder is empty.",
                                        10.0,
                                        theme.muted_foreground,
                                    );
                                }
                            }
                        });
                });
            });

            if let Some(context) = session.folder_browser.context_menu.as_ref() {
                spawn_folder_context_menu(page, font.clone(), theme, context);
            }
            if let Some(path) = session.folder_browser.pending_remove.as_ref() {
                spawn_remove_folder_confirmation(page, font.clone(), theme, path);
            }
            if let Some(notice) = session.notice.as_deref() {
                page.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(14),
                        right: px(14),
                        bottom: px(10),
                        min_height: px(30),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(12)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(theme.muted.with_alpha(0.88)),
                    ZIndex(60),
                    children![(
                        Text::new(notice),
                        ui_text_font(font, 9.0),
                        TextColor(theme.muted_foreground),
                    )],
                ));
            }
        });
}

pub(crate) fn spawn_folder_entry(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    entry: &LibraryFolderEntry,
) {
    let context_entry = entry.clone();
    parent
        .spawn((
            Button,
            Node {
                width: percent(100),
                min_height: px(38),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(14)),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                align_items: AlignItems::Center,
                column_gap: px(8),
                ..default()
            })
            .with_children(|name| {
                spawn_icon(
                    name,
                    icons,
                    folder_entry_icon(&entry.kind),
                    14.0,
                    folder_entry_color(&entry.kind, theme),
                );
                spawn_text(
                    name,
                    font.clone(),
                    entry.name.clone(),
                    10.0,
                    theme.foreground,
                );
            });
            row.spawn(Node {
                width: px(82),
                ..default()
            })
            .with_children(|kind| {
                spawn_text(
                    kind,
                    font.clone(),
                    entry.kind.clone(),
                    9.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(68),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|size| {
                spawn_text(
                    size,
                    font,
                    if entry.size_bytes == 0 {
                        "—".to_string()
                    } else {
                        format_bytes(entry.size_bytes)
                    },
                    9.0,
                    theme.muted_foreground,
                );
            });
        })
        .observe(
            move |mut event: On<Pointer<Click>>,
                  mut session: ResMut<StudioSession>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                match event.button {
                    PointerButton::Primary => {
                        let path = PathBuf::from(&context_entry.path);
                        if context_entry.kind == "folder" {
                            session.folder_browser.current = Some(path);
                            session.folder_browser.context_menu = None;
                            session.folder_browser.refresh();
                            session.notice = None;
                        } else if event.count >= 2 {
                            session.notice = Some(open_library_entry(&path, &session.config));
                        }
                        invalidated.0 = true;
                    }
                    PointerButton::Secondary => {
                        session.folder_browser.context_menu = Some(FolderContextMenu {
                            entry: context_entry.clone(),
                            position: event.pointer_location.position,
                        });
                        invalidated.0 = true;
                    }
                    PointerButton::Middle => {}
                }
            },
        );
}

pub(crate) fn spawn_folder_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    context: &FolderContextMenu,
) {
    parent.spawn((
        Button,
        UiAction::DismissFolderContext,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ZIndex(40),
    ));
    let left = (context.position.x - SIDEBAR_WIDTH - 12.0).max(8.0);
    let top = (context.position.y - 58.0).max(8.0);
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(220),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(41),
        ))
        .with_children(|menu| {
            spawn_text(
                menu,
                font.clone(),
                context.entry.name.clone(),
                9.0,
                theme.muted_foreground,
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                if context.entry.kind == "folder" {
                    "Open folder"
                } else {
                    "Open"
                },
                11.0,
                UiAction::OpenFolderEntry(PathBuf::from(&context.entry.path)),
            );
            spawn_text_button(
                menu,
                font,
                theme,
                "Reveal in folder",
                11.0,
                UiAction::RevealFolderEntry(PathBuf::from(&context.entry.path)),
            );
        });
}

pub(crate) fn spawn_remove_folder_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    path: &std::path::Path,
) {
    parent.spawn((
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
        BackgroundColor(theme.background.with_alpha(0.72)),
        ZIndex(70),
        children![(
            Node {
                width: px(430),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(22)),
                row_gap: px(10),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new("Stop watching this folder?"),
                    ui_text_font(font.clone(), 16.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "{}\n\nUta Studio will update its library index but will not move or delete any source media.",
                        path.display()
                    )),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelRemoveFolder,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmRemoveFolder,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Stop watching"),
                                ui_text_font(font, 10.0),
                                TextColor(theme.destructive),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

pub(crate) fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

pub(crate) fn folder_entry_icon(kind: &str) -> UiIcon {
    match kind {
        "folder" => UiIcon::Folder,
        "video" => UiIcon::Video,
        "playlist" => UiIcon::List,
        "chart" => UiIcon::Queue,
        _ => UiIcon::Music,
    }
}

pub(crate) fn folder_entry_color(kind: &str, theme: &StudioTheme) -> Color {
    match kind {
        "folder" => Color::srgb(0.82, 0.59, 0.22),
        "video" => Color::srgb(0.24, 0.7, 0.75),
        "playlist" => Color::srgb(0.24, 0.68, 0.48),
        "chart" => Color::srgb(0.58, 0.42, 0.78),
        _ => theme.primary,
    }
}

pub(crate) fn handle_folder_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    session: Res<StudioSession>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<FolderEntryList>>,
) {
    if session.route != StudioRoute::Folders {
        wheel.clear();
        return;
    }
    let Ok((computed, mut position)) = lists.single_mut() else {
        wheel.clear();
        return;
    };
    let mut delta = 0.0;
    for event in wheel.read() {
        let scale = match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 22.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
        delta -= event.y * scale;
    }
    if delta.abs() < f32::EPSILON {
        return;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    let max = (content.y - size.y).max(0.0);
    position.y = (position.y + delta).clamp(0.0, max);
}
