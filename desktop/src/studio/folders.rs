//! Folders route: watched roots, browsing, and folder actions.

use crate::studio::*;

#[derive(Clone)]
pub(crate) struct FolderContextMenu {
    pub(crate) entry: LibraryFolderEntry,
    pub(crate) position: Vec2,
    pub(crate) viewport_size: Vec2,
}

const FOLDER_CONTEXT_MENU_WIDTH: f32 = 286.0;
const FOLDER_CONTEXT_MENU_HEIGHT: f32 = 292.0;
const FOLDER_CONTEXT_MENU_EDGE: f32 = 8.0;
const FOLDER_CONTEXT_PLAYER_RESERVE: f32 = 82.0;

fn clamp_folder_context_position(position: Vec2, viewport: Vec2) -> Vec2 {
    let local = Vec2::new(
        position.x - SIDEBAR_WIDTH - FOLDER_CONTEXT_MENU_EDGE,
        position.y - WORKSPACE_TOP_BAR_MIN_HEIGHT,
    );
    let max_x = (viewport.x - SIDEBAR_WIDTH - FOLDER_CONTEXT_MENU_WIDTH - FOLDER_CONTEXT_MENU_EDGE)
        .max(FOLDER_CONTEXT_MENU_EDGE);
    let max_y = (viewport.y
        - WORKSPACE_TOP_BAR_MIN_HEIGHT
        - FOLDER_CONTEXT_PLAYER_RESERVE
        - FOLDER_CONTEXT_MENU_HEIGHT
        - FOLDER_CONTEXT_MENU_EDGE)
        .max(FOLDER_CONTEXT_MENU_EDGE);
    Vec2::new(
        local.x.clamp(FOLDER_CONTEXT_MENU_EDGE, max_x),
        local.y.clamp(FOLDER_CONTEXT_MENU_EDGE, max_y),
    )
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

#[derive(Component)]
pub(crate) struct FolderRootList;

#[allow(clippy::too_many_arguments)]
fn spawn_folder_empty_state(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    accent: Color,
    title: impl Into<String>,
    detail: impl Into<String>,
    action: Option<(&'static str, UiAction)>,
) {
    parent
        .spawn(Node {
            width: percent(100),
            min_height: px(250),
            flex_grow: 1.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(28)),
            row_gap: px(9),
            ..default()
        })
        .with_children(|empty| {
            empty
                .spawn((
                    Node {
                        width: px(58),
                        height: px(58),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::MAX,
                        margin: UiRect::bottom(px(3)),
                        ..default()
                    },
                    BackgroundColor(accent.with_alpha(0.10)),
                    BorderColor::all(accent.with_alpha(0.30)),
                ))
                .with_children(|slot| spawn_icon(slot, icons, icon, 24.0, accent));
            spawn_text(empty, font.clone(), title, 16.0, theme.foreground);
            empty
                .spawn(Node {
                    max_width: px(440),
                    ..default()
                })
                .with_children(|copy| {
                    spawn_wrapped_text(copy, font.clone(), detail, 10.0, theme.muted_foreground);
                });
            if let Some((label, action)) = action {
                spawn_compact_primary_action_button(empty, font, theme, label, action);
            }
        });
}

fn folder_entry_kind_label(kind: &str) -> &'static str {
    match kind {
        "folder" => "Folder",
        "audio" => "Audio",
        "video" => "Video",
        "playlist" => "Playlist",
        "chart" => "Chart",
        _ => "File",
    }
}

fn folder_entry_secondary_copy(entry: &LibraryFolderEntry) -> String {
    if entry.kind == "folder" {
        return folder_entry_kind_label(&entry.kind).to_string();
    }
    let extension = std::path::Path::new(&entry.path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_uppercase)
        .unwrap_or_else(|| "FILE".to_string());
    format!("{} · {}", folder_entry_kind_label(&entry.kind), extension)
}

pub(crate) fn spawn_folders(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let current_name = session
        .folder_browser
        .current
        .as_deref()
        .map(folder_name)
        .unwrap_or_else(|| "No folder selected".to_string());
    let current_path = session
        .folder_browser
        .current
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Choose a library location to browse its files".to_string());
    let can_go_up = session.folder_browser.parent().is_some();

    parent
        .spawn(Node {
            position_type: PositionType::Relative,
            min_width: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(26), px(16)),
            row_gap: px(12),
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|page| {
            page.spawn((
                FolderRootList,
                ScrollPosition::default(),
                Node {
                    width: percent(100),
                    max_height: px(140),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(12)),
                    row_gap: px(8),
                    overflow: Overflow::scroll_y(),
                    border: UiRect::all(px(1)),
                    border_radius: studio_card_radius(),
                    ..default()
                },
                studio_card_background(theme),
                studio_card_border(theme),
                studio_card_shadow(theme),
            ))
            .with_children(|locations| {
                locations
                    .spawn(Node {
                        width: percent(100),
                        align_items: AlignItems::Center,
                        column_gap: px(8),
                        ..default()
                    })
                    .with_children(|header| {
                        spawn_text(
                            header,
                            font.clone(),
                            "LIBRARY LOCATIONS",
                            7.5,
                            theme.primary,
                        );
                        header.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        spawn_status_pill(
                            header,
                            font.clone(),
                            format!("{} watched", session.config.library_paths().len()),
                            theme.muted_foreground,
                        );
                    });

                locations
                    .spawn(Node {
                        width: percent(100),
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(8),
                        row_gap: px(8),
                        ..default()
                    })
                    .with_children(|chips| {
                        for root in session.config.library_paths() {
                            let selected = session.folder_browser.root.as_ref() == Some(&root);
                            let root_label = folder_name(&root);
                            let root_path = root.to_string_lossy().into_owned();
                            let select_path = root.clone();
                            let remove_path = root;
                            chips
                                .spawn((
                                    Node {
                                        min_width: px(190),
                                        flex_basis: px(230),
                                        min_height: px(50),
                                        flex_grow: 1.0,
                                        align_items: AlignItems::Center,
                                        padding: UiRect::right(px(4)),
                                        border: UiRect::all(px(1)),
                                        border_radius: studio_control_radius(),
                                        ..default()
                                    },
                                    BackgroundColor(if selected {
                                        theme.primary.with_alpha(0.09)
                                    } else {
                                        theme.background.with_alpha(0.18)
                                    }),
                                    BorderColor::all(if selected {
                                        theme.primary.with_alpha(0.44)
                                    } else {
                                        theme.border.with_alpha(0.34)
                                    }),
                                ))
                                .with_children(|chip| {
                                    chip.spawn((
                                        Button,
                                        UiAction::from(LibraryCommand::SelectFolderRoot(select_path)),
                                        Node {
                                            min_width: px(0),
                                            height: percent(100),
                                            flex_grow: 1.0,
                                            align_items: AlignItems::Center,
                                            padding: UiRect::horizontal(px(10)),
                                            column_gap: px(9),
                                            border_radius: studio_control_radius(),
                                            ..default()
                                        },
                                        BackgroundColor(Color::NONE),
                                    ))
                                    .with_children(|select| {
                                        select
                                            .spawn((
                                                Node {
                                                    width: px(30),
                                                    height: px(30),
                                                    flex_shrink: 0.0,
                                                    align_items: AlignItems::Center,
                                                    justify_content: JustifyContent::Center,
                                                    border_radius: BorderRadius::all(px(7)),
                                                    ..default()
                                                },
                                                BackgroundColor(
                                                    theme.primary.with_alpha(if selected {
                                                        0.15
                                                    } else {
                                                        0.07
                                                    }),
                                                ),
                                            ))
                                            .with_children(|slot| {
                                                spawn_icon(
                                                    slot,
                                                    icons.clone(),
                                                    UiIcon::Folder,
                                                    15.0,
                                                    if selected {
                                                        theme.primary
                                                    } else {
                                                        theme.muted_foreground
                                                    },
                                                );
                                            });
                                        select
                                            .spawn(Node {
                                                min_width: px(0),
                                                flex_grow: 1.0,
                                                flex_direction: FlexDirection::Column,
                                                justify_content: JustifyContent::Center,
                                                row_gap: px(2),
                                                overflow: Overflow::clip(),
                                                ..default()
                                            })
                                            .with_children(|copy| {
                                                copy.spawn((
                                                    Text::new(root_label),
                                                    ui_text_font(font.clone(), 10.5),
                                                    TextColor(theme.foreground),
                                                    TextLayout::no_wrap(),
                                                ));
                                                copy.spawn((
                                                    Text::new(root_path),
                                                    ui_text_font(font.clone(), 7.5),
                                                    TextColor(
                                                        theme.muted_foreground.with_alpha(0.72),
                                                    ),
                                                    TextLayout::no_wrap(),
                                                ));
                                            });
                                    });
                                    spawn_icon_button(
                                        chip,
                                        icons.clone(),
                                        theme,
                                        UiIcon::Trash,
                                        UiAction::from(LibraryCommand::RequestRemoveFolder(
                                            remove_path,
                                        )),
                                        false,
                                        false,
                                        30.0,
                                    );
                                });
                        }

                        if session.config.library_paths().is_empty() {
                            chips
                                .spawn((
                                    Node {
                                        min_width: px(220),
                                        flex_basis: px(280),
                                        min_height: px(50),
                                        flex_grow: 1.0,
                                        align_items: AlignItems::Center,
                                        padding: UiRect::axes(px(10), px(7)),
                                        column_gap: px(10),
                                        border: UiRect::all(px(1)),
                                        border_radius: studio_control_radius(),
                                        ..default()
                                    },
                                    BackgroundColor(theme.background.with_alpha(0.18)),
                                    BorderColor::all(theme.border.with_alpha(0.34)),
                                ))
                                .with_children(|empty| {
                                    spawn_icon(
                                        empty,
                                        icons.clone(),
                                        UiIcon::Folder,
                                        17.0,
                                        theme.muted_foreground,
                                    );
                                    empty
                                        .spawn(Node {
                                            min_width: px(0),
                                            flex_grow: 1.0,
                                            flex_direction: FlexDirection::Column,
                                            ..default()
                                        })
                                        .with_children(|copy| {
                                            spawn_text(
                                                copy,
                                                font.clone(),
                                                "No library locations",
                                                10.0,
                                                theme.foreground,
                                            );
                                            spawn_text(
                                                copy,
                                                font.clone(),
                                                "Add a folder to begin browsing",
                                                7.5,
                                                theme.muted_foreground,
                                            );
                                        });
                                    spawn_compact_primary_action_button(
                                        empty,
                                        font.clone(),
                                        theme,
                                        "Add folder",
                                        UiAction::from(LibraryCommand::ChooseFolder),
                                    );
                                });
                        }

                        let output_selected = session
                            .config
                            .export_path
                            .as_ref()
                            .is_some_and(|path| session.folder_browser.root.as_ref() == Some(path));
                        chips
                            .spawn((
                                Node {
                                    min_width: px(190),
                                    flex_basis: px(230),
                                    min_height: px(50),
                                    flex_grow: 1.0,
                                    align_items: AlignItems::Center,
                                    padding: UiRect::right(px(4)),
                                    border: UiRect::all(px(1)),
                                    border_radius: studio_control_radius(),
                                    ..default()
                                },
                                BackgroundColor(if output_selected {
                                    theme.primary.with_alpha(0.09)
                                } else {
                                    theme.background.with_alpha(0.18)
                                }),
                                BorderColor::all(if output_selected {
                                    theme.primary.with_alpha(0.44)
                                } else {
                                    theme.border.with_alpha(0.34)
                                }),
                            ))
                            .with_children(|chip| {
                                let (action, output_label, output_path) =
                                    if let Some(path) = session.config.export_path.as_ref() {
                                        (
                                            UiAction::from(LibraryCommand::SelectFolderRoot(
                                                path.clone(),
                                            )),
                                            folder_name(path),
                                            path.to_string_lossy().into_owned(),
                                        )
                                    } else {
                                        (
                                            UiAction::from(LibraryCommand::ChooseExportFolder),
                                            "Output folder".to_string(),
                                            "Choose where processed files are written".to_string(),
                                        )
                                    };
                                chip.spawn((
                                    Button,
                                    action,
                                    Node {
                                        min_width: px(0),
                                        height: percent(100),
                                        flex_grow: 1.0,
                                        align_items: AlignItems::Center,
                                        padding: UiRect::horizontal(px(10)),
                                        column_gap: px(9),
                                        border_radius: studio_control_radius(),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                ))
                                .with_children(|select| {
                                    select
                                        .spawn((
                                            Node {
                                                width: px(30),
                                                height: px(30),
                                                flex_shrink: 0.0,
                                                align_items: AlignItems::Center,
                                                justify_content: JustifyContent::Center,
                                                border_radius: BorderRadius::all(px(7)),
                                                ..default()
                                            },
                                            BackgroundColor(theme.primary.with_alpha(0.08)),
                                        ))
                                        .with_children(|slot| {
                                            spawn_icon(
                                                slot,
                                                icons.clone(),
                                                UiIcon::Save,
                                                15.0,
                                                if output_selected {
                                                    theme.primary
                                                } else {
                                                    theme.muted_foreground
                                                },
                                            );
                                        });
                                    select
                                        .spawn(Node {
                                            min_width: px(0),
                                            flex_grow: 1.0,
                                            flex_direction: FlexDirection::Column,
                                            justify_content: JustifyContent::Center,
                                            row_gap: px(2),
                                            overflow: Overflow::clip(),
                                            ..default()
                                        })
                                        .with_children(|copy| {
                                            copy.spawn((
                                                Text::new(format!("Output · {output_label}")),
                                                ui_text_font(font.clone(), 10.5),
                                                TextColor(theme.foreground),
                                                TextLayout::no_wrap(),
                                            ));
                                            copy.spawn((
                                                Text::new(output_path),
                                                ui_text_font(font.clone(), 7.5),
                                                TextColor(
                                                    theme.muted_foreground.with_alpha(0.72),
                                                ),
                                                TextLayout::no_wrap(),
                                            ));
                                        });
                                });
                                if session.config.export_path.is_some() {
                                    spawn_icon_button(
                                        chip,
                                        icons.clone(),
                                        theme,
                                        UiIcon::Close,
                                        UiAction::from(LibraryCommand::ClearExportFolder),
                                        false,
                                        false,
                                        30.0,
                                    );
                                }
                            });
                    });
            });

            page.spawn((
                Node {
                    min_width: px(0),
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip(),
                    border: UiRect::all(px(1)),
                    border_radius: studio_card_radius(),
                    ..default()
                },
                studio_card_background(theme),
                studio_card_border(theme),
                studio_card_shadow(theme),
            ))
            .with_children(|browser| {
                browser
                    .spawn((
                        Node {
                            width: percent(100),
                            height: px(64),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(14)),
                            column_gap: px(10),
                            border: UiRect::bottom(px(1)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.16)),
                        BorderColor::all(theme.border.with_alpha(0.46)),
                    ))
                    .with_children(|path_bar| {
                        if can_go_up {
                            spawn_icon_button(
                                path_bar,
                                icons.clone(),
                                theme,
                                UiIcon::ArrowLeft,
                                UiAction::from(LibraryCommand::FolderUp),
                                false,
                                false,
                                34.0,
                            );
                        } else {
                            path_bar
                                .spawn(Node {
                                    width: px(34),
                                    height: px(34),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border_radius: studio_control_radius(),
                                    ..default()
                                })
                                .with_children(|slot| {
                                    spawn_icon(
                                        slot,
                                        icons.clone(),
                                        UiIcon::ArrowLeft,
                                        15.0,
                                        theme.muted_foreground.with_alpha(0.26),
                                    );
                                });
                        }
                        path_bar
                            .spawn((
                                Node {
                                    width: px(34),
                                    height: px(34),
                                    flex_shrink: 0.0,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border_radius: BorderRadius::all(px(8)),
                                    ..default()
                                },
                                BackgroundColor(theme.primary.with_alpha(0.10)),
                            ))
                            .with_children(|slot| {
                                spawn_icon(
                                    slot,
                                    icons.clone(),
                                    UiIcon::Folder,
                                    16.0,
                                    theme.primary,
                                );
                            });
                        path_bar
                            .spawn(Node {
                                min_width: px(0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Center,
                                row_gap: px(2),
                                overflow: Overflow::clip(),
                                ..default()
                            })
                            .with_children(|copy| {
                                copy.spawn((
                                    Text::new(current_name),
                                    ui_text_font(font.clone(), 12.0),
                                    TextColor(theme.foreground),
                                    TextLayout::no_wrap(),
                                ));
                                copy.spawn((
                                    Text::new(current_path),
                                    ui_text_font(font.clone(), 8.0),
                                    TextColor(theme.muted_foreground),
                                    TextLayout::no_wrap(),
                                ));
                            });
                        spawn_status_pill(
                            path_bar,
                            font.clone(),
                            format!("{} items", session.folder_browser.entries.len()),
                            theme.muted_foreground,
                        );
                    });

                browser
                    .spawn((
                        Node {
                            width: percent(100),
                            height: px(34),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(14)),
                            border: UiRect::bottom(px(1)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.10)),
                        BorderColor::all(theme.border.with_alpha(0.30)),
                    ))
                    .with_children(|columns| {
                        columns
                            .spawn(Node {
                                min_width: px(0),
                                flex_grow: 1.0,
                                ..default()
                            })
                            .with_children(|name| {
                                spawn_text(
                                    name,
                                    font.clone(),
                                    "NAME",
                                    7.5,
                                    theme.muted_foreground,
                                );
                            });
                        columns
                            .spawn(Node {
                                width: px(82),
                                ..default()
                            })
                            .with_children(|kind| {
                                spawn_text(
                                    kind,
                                    font.clone(),
                                    "KIND",
                                    7.5,
                                    theme.muted_foreground,
                                );
                            });
                        columns
                            .spawn(Node {
                                width: px(68),
                                justify_content: JustifyContent::FlexEnd,
                                ..default()
                            })
                            .with_children(|size| {
                                spawn_text(
                                    size,
                                    font.clone(),
                                    "SIZE",
                                    7.5,
                                    theme.muted_foreground,
                                );
                            });
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
                            spawn_folder_empty_state(
                                list,
                                font.clone(),
                                icons.clone(),
                                theme,
                                UiIcon::Repair,
                                theme.destructive,
                                "This folder could not be read",
                                localized_message(
                                    session.config,
                                    UiMessage::FolderReadFailed,
                                    &[("{error}", error)],
                                ),
                                None,
                            );
                        } else if session.folder_browser.current.is_none() {
                            spawn_folder_empty_state(
                                list,
                                font.clone(),
                                icons.clone(),
                                theme,
                                UiIcon::Folder,
                                theme.primary,
                                "Choose a library location",
                                "Select one of the locations above, or add a folder to begin.",
                                Some((
                                    "Add folder",
                                    UiAction::from(LibraryCommand::ChooseFolder),
                                )),
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
                                spawn_folder_empty_state(
                                    list,
                                    font.clone(),
                                    icons.clone(),
                                    theme,
                                    UiIcon::Folder,
                                    theme.muted_foreground,
                                    "This folder is empty",
                                    "There are no supported media files or subfolders in this location.",
                                    None,
                                );
                            }
                        }
                    });

                browser
                    .spawn((
                        Node {
                            width: percent(100),
                            height: px(36),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(14)),
                            column_gap: px(10),
                            border: UiRect::top(px(1)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.14)),
                        BorderColor::all(theme.border.with_alpha(0.30)),
                    ))
                    .with_children(|footer| {
                        spawn_text(
                            footer,
                            font.clone(),
                            "Source media is read-only",
                            8.0,
                            theme.muted_foreground,
                        );
                        footer.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        spawn_text(
                            footer,
                            font.clone(),
                            "Double-click files to open · Right-click for actions",
                            8.0,
                            theme.muted_foreground.with_alpha(0.78),
                        );
                    });
            });

            if let Some(context) = session.folder_browser.context_menu.as_ref() {
                spawn_folder_context_menu(page, font.clone(), icons.clone(), theme, context);
            }
            if let Some(path) = session.folder_browser.pending_remove.as_ref() {
                spawn_remove_folder_confirmation(page, font.clone(), theme, path);
            }
            if let Some(notice) = session.notice.as_deref() {
                page.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(22),
                        right: px(22),
                        bottom: px(16),
                        min_height: px(40),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(14)),
                        border: UiRect::all(px(1)),
                        border_radius: studio_control_radius(),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.96)),
                    BorderColor::all(theme.primary.with_alpha(0.28)),
                    studio_popover_shadow(theme),
                    ZIndex(60),
                    children![(
                        Text::new(notice),
                        ui_text_font(font, 9.0),
                        TextColor(theme.foreground),
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
    let secondary = folder_entry_secondary_copy(entry);
    let accent = folder_entry_color(&entry.kind, theme);
    parent
        .spawn((
            Button,
            UiPointerApi(&[
                "ui.pointer.folder_entry.primary",
                "ui.pointer.folder_entry.double_primary",
                "ui.pointer.folder_entry.secondary",
            ]),
            Node {
                width: percent(100),
                min_height: px(56),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(14)),
                column_gap: px(11),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(theme.border.with_alpha(0.20)),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(38),
                    height: px(38),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(9)),
                    ..default()
                },
                BackgroundColor(accent.with_alpha(0.10)),
                BorderColor::all(accent.with_alpha(0.24)),
            ))
            .with_children(|slot| {
                spawn_icon(
                    slot,
                    icons.clone(),
                    folder_entry_icon(&entry.kind),
                    17.0,
                    accent,
                );
            });
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                row_gap: px(2),
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn((
                    Text::new(entry.name.clone()),
                    ui_text_font(font.clone(), 11.0),
                    TextColor(theme.foreground),
                    TextLayout::no_wrap(),
                ));
                copy.spawn((
                    Text::new(secondary),
                    ui_text_font(font.clone(), 8.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::no_wrap(),
                ));
            });
            row.spawn(Node {
                width: px(78),
                flex_shrink: 0.0,
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
                  mut shell: ResMut<ShellState>,
                  mut library: ResMut<LibraryState>,
                  windows: Query<&Window, With<PrimaryWindow>>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                match event.button {
                    PointerButton::Primary => {
                        let path = PathBuf::from(&context_entry.path);
                        if context_entry.kind == "folder" {
                            library.folder_browser.current = Some(path);
                            library.folder_browser.context_menu = None;
                            library.folder_browser.refresh();
                            shell.notice = None;
                        } else if event.count >= 2 {
                            shell.notice = Some(open_library_entry(&path, &shell.config));
                        }
                        invalidated.invalidate(UiDirtyRegion::Library);
                    }
                    PointerButton::Secondary => {
                        let viewport_size = windows
                            .single()
                            .map(|window| Vec2::new(window.width(), window.height()))
                            .unwrap_or(Vec2::new(1280.0, 720.0));
                        library.folder_browser.context_menu = Some(FolderContextMenu {
                            entry: context_entry.clone(),
                            position: event.pointer_location.position,
                            viewport_size,
                        });
                        invalidated.invalidate(UiDirtyRegion::Library);
                    }
                    PointerButton::Middle => {}
                }
            },
        );
}

pub(crate) fn spawn_folder_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    context: &FolderContextMenu,
) {
    parent.spawn((
        Button,
        UiAction::from(LibraryCommand::DismissFolderContext),
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
    let position = clamp_folder_context_position(context.position, context.viewport_size);
    let left = position.x;
    let top = position.y;
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(FOLDER_CONTEXT_MENU_WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(4),
                border: UiRect::all(px(1)),
                border_radius: studio_popover_radius(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.78)),
            studio_popover_shadow(theme),
            ZIndex(41),
        ))
        .with_children(|menu| {
            menu.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(4), px(5)),
                column_gap: px(10),
                ..default()
            })
            .with_children(|header| {
                header
                    .spawn((
                        Node {
                            width: px(38),
                            height: px(38),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(px(9)),
                            ..default()
                        },
                        BackgroundColor(
                            folder_entry_color(&context.entry.kind, theme).with_alpha(0.10),
                        ),
                    ))
                    .with_children(|slot| {
                        spawn_icon(
                            slot,
                            icons,
                            folder_entry_icon(&context.entry.kind),
                            18.0,
                            folder_entry_color(&context.entry.kind, theme),
                        );
                    });
                header
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(2),
                        overflow: Overflow::clip(),
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_text(
                            copy,
                            font.clone(),
                            context.entry.name.clone(),
                            11.5,
                            theme.foreground,
                        );
                        spawn_text(
                            copy,
                            font.clone(),
                            folder_entry_kind_label(&context.entry.kind),
                            7.5,
                            theme.muted_foreground,
                        );
                    });
            });
            menu.spawn((
                Node {
                    width: percent(100),
                    padding: UiRect::all(px(8)),
                    margin: UiRect::bottom(px(4)),
                    overflow: Overflow::clip(),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(7)),
                    ..default()
                },
                BackgroundColor(theme.background.with_alpha(0.28)),
                BorderColor::all(theme.border.with_alpha(0.36)),
                children![(
                    Text::new(context.entry.path.clone()),
                    ui_text_font(font.clone(), 7.5),
                    TextColor(theme.muted_foreground),
                    TextLayout::no_wrap(),
                )],
            ));
            spawn_text(menu, font.clone(), "ACTIONS", 7.0, theme.primary);
            spawn_menu_text_button(
                menu,
                font.clone(),
                theme,
                if context.entry.kind == "folder" {
                    "Open folder"
                } else {
                    "Open file"
                },
                10.5,
                UiAction::from(LibraryCommand::OpenFolderEntry(PathBuf::from(
                    &context.entry.path,
                ))),
            );
            spawn_menu_text_button(
                menu,
                font.clone(),
                theme,
                "Reveal in system file manager",
                10.5,
                UiAction::from(LibraryCommand::RevealFolderEntry(PathBuf::from(
                    &context.entry.path,
                ))),
            );
            spawn_wrapped_text(
                menu,
                font,
                "Source files remain read-only when opened from Uta! Studio.",
                7.5,
                theme.muted_foreground.with_alpha(0.78),
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
                width: px(460),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(12),
                border: UiRect::all(px(1)),
                border_radius: studio_popover_radius(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.82)),
            studio_popover_shadow(theme),
            children![
                (
                    Text::new("Stop watching this folder?"),
                    ui_text_font(font.clone(), 16.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "{}\n\nUta! Studio will update its library index but will not move or delete any source media.",
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
                            UiAction::from(LibraryCommand::CancelRemoveFolder),
                            Node {
                                min_height: px(STUDIO_CONTROL_HEIGHT),
                                padding: UiRect::axes(px(13), px(8)),
                                border: UiRect::all(px(1)),
                                border_radius: studio_control_radius(),
                                ..default()
                            },
                            BackgroundColor(theme.card.with_alpha(0.50)),
                            BorderColor::all(theme.border.with_alpha(0.54)),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::from(LibraryCommand::ConfirmRemoveFolder),
                            Node {
                                min_height: px(STUDIO_CONTROL_HEIGHT),
                                padding: UiRect::axes(px(13), px(8)),
                                border: UiRect::all(px(1)),
                                border_radius: studio_control_radius(),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.16)),
                            BorderColor::all(theme.destructive.with_alpha(0.54)),
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

#[allow(clippy::type_complexity)]
pub(crate) fn handle_folder_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    shell: Res<ShellState>,
    mut roots: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        (With<FolderRootList>, Without<FolderEntryList>),
    >,
    mut entries: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        (With<FolderEntryList>, Without<FolderRootList>),
    >,
) {
    if shell.route != StudioRoute::Folders {
        return;
    }
    let Ok(window) = windows.single() else {
        wheel.clear();
        return;
    };
    let Some(pointer) = window.cursor_position() else {
        wheel.clear();
        return;
    };
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 22.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            -event.y * scale
        })
        .sum::<f32>();
    if delta.abs() < f32::EPSILON {
        return;
    }

    if let Some((computed, _, mut position)) = roots
        .iter_mut()
        .find(|(computed, transform, _)| ui_node_contains_pointer(computed, transform, pointer))
    {
        let size = computed.size() * computed.inverse_scale_factor();
        let content = computed.content_size() * computed.inverse_scale_factor();
        position.x = (position.x + delta).clamp(0.0, (content.x - size.x).max(0.0));
        return;
    }

    if let Some((computed, _, mut position)) = entries
        .iter_mut()
        .find(|(computed, transform, _)| ui_node_contains_pointer(computed, transform, pointer))
    {
        let size = computed.size() * computed.inverse_scale_factor();
        let content = computed.content_size() * computed.inverse_scale_factor();
        position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
    }
}
