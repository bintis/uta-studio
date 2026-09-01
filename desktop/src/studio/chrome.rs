use crate::studio::*;

pub(crate) fn spawn_sidebar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    banner: Handle<Image>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: px(SIDEBAR_WIDTH),
                height: percent(100),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(10), px(16)),
                ..default()
            },
            BackgroundColor(theme.sidebar),
        ))
        .with_children(|sidebar| {
            sidebar
                .spawn(Node {
                    width: percent(100),
                    height: px(82),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn((
                            Button,
                            UiAction::from(AppCommand::OpenAbout),
                            Node {
                                height: px(68),
                                align_items: AlignItems::Center,
                                // Lines the wordmark's left edge up with
                                // "BROWSE"/"MY LIBRARY" below it
                                // (`spawn_section_label`'s own `left: px(8)`
                                // margin), not the sidebar's raw padding
                                // edge.
                                margin: UiRect::left(px(8)),
                                padding: UiRect::right(px(8)),
                                column_gap: px(10),
                                ..default()
                            },
                        ))
                        .with_children(|brand| {
                            spawn_text(
                                brand,
                                font.clone(),
                                "Uta! Studio",
                                18.0,
                                theme.sidebar_foreground,
                            );
                        });
                    header.spawn(Node {
                        min_width: px(4),
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_icon_button(
                        header,
                        icons.clone(),
                        theme,
                        UiIcon::Settings,
                        UiAction::from(AppCommand::Settings),
                        false,
                        false,
                        30.0,
                    );
                });

            spawn_section_label(sidebar, font.clone(), theme, "BROWSE");
            spawn_sidebar_filter_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                Some(UiIcon::Home),
                "All",
                session.meta.songs_count,
                UiAction::from(LibraryCommand::SetLibraryView(LibraryView::All)),
                session.route == StudioRoute::Library && session.library_view == LibraryView::All,
            );

            let selected_song = session.selected_song.as_deref();
            let analysis_count = active_analysis_task_count(session.analysis_tasks);
            spawn_section_label(sidebar, font.clone(), theme, "ANALYSIS");
            spawn_sidebar_filter_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                Some(UiIcon::Queue),
                "Processing Queue",
                analysis_count,
                UiAction::from(AnalysisCommand::OpenAnalysisQueue),
                session.route == StudioRoute::Queue,
            );
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Sparkles,
                "Processing Studio",
                Some(selected_song.map_or_else(
                    || UiAction::from(AnalysisCommand::OpenEmptyProcessingStudio),
                    |file_hash| {
                        UiAction::from(AnalysisCommand::OpenProcessingStudio(file_hash.to_string()))
                    },
                )),
                session.route == StudioRoute::ProcessingStudio,
            );
            let advanced_graph_action = selected_song.map_or_else(
                || UiAction::from(LibraryCommand::SetLibraryView(LibraryView::Queue)),
                view_song_analysis_action,
            );
            spawn_sidebar_filter_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                Some(UiIcon::Grid),
                "DAG Graph",
                0,
                advanced_graph_action,
                (session.route == StudioRoute::Library
                    && session.library_view == LibraryView::Queue)
                    || session.route == StudioRoute::AnalysisInspect,
            );

            spawn_section_label(sidebar, font.clone(), theme, "EDIT");
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Scissors,
                "Editor",
                Some(UiAction::from(LibraryCommand::SetLibraryView(
                    LibraryView::Completed,
                ))),
                session.route == StudioRoute::Editor,
            );

            spawn_section_label(sidebar, font.clone(), theme, "MY LIBRARY");
            for (view, icon, label, count) in [
                (
                    LibraryView::Completed,
                    UiIcon::CircleCheck,
                    "Charts",
                    session.meta.analyzed_count,
                ),
                (
                    LibraryView::Videos,
                    UiIcon::Video,
                    "Video",
                    session.meta.videos_count,
                ),
            ] {
                spawn_sidebar_filter_item(
                    sidebar,
                    font.clone(),
                    icons.clone(),
                    theme,
                    Some(icon),
                    label,
                    count,
                    UiAction::from(LibraryCommand::SetLibraryView(view)),
                    session.route == StudioRoute::Library && session.library_view == view,
                );
            }
            if !session.menu_items.playlists.is_empty() {
                spawn_sidebar_item(
                    sidebar,
                    font.clone(),
                    icons.clone(),
                    theme,
                    UiIcon::List,
                    "Playlists",
                    0,
                );
                for item in session.menu_items.playlists.iter().take(2) {
                    let facet = LibraryFacet::Playlist {
                        value: item.value.clone(),
                        label: item.label.clone(),
                    };
                    spawn_sidebar_filter_item(
                        sidebar,
                        font.clone(),
                        icons.clone(),
                        theme,
                        None,
                        format!("  {}", item.label),
                        item.count.try_into().unwrap_or(usize::MAX),
                        UiAction::from(LibraryCommand::SetLibraryFacet(facet.clone())),
                        session.route == StudioRoute::Library
                            && session.library_facet.as_ref() == Some(&facet),
                    );
                }
            }
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Folder,
                "Folders",
                Some(UiAction::from(AppCommand::Folders)),
                session.route == StudioRoute::Folders,
            );
            sidebar.spawn(Node {
                min_height: px(14),
                flex_grow: 1.0,
                ..default()
            });
            sidebar.spawn((
                Button,
                UiAction::from(AppCommand::OpenAbout),
                Node {
                    width: percent(100),
                    // Banner.png is a fixed 4:3 image (1448x1086) -- sized
                    // from the sidebar's own inner width (minus its
                    // `padding: UiRect::axes(px(10), ..)`) so it fills the
                    // sidebar edge-to-edge without stretching or
                    // letterboxing.
                    height: px((SIDEBAR_WIDTH - 20.0) * 3.0 / 4.0),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                ImageNode::new(banner),
            ));
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_sidebar_filter_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: Option<UiIcon>,
    label: impl Into<String>,
    count: usize,
    action: UiAction,
    active: bool,
) {
    let label = label.into();
    parent
        .spawn((
            Button,
            action,
            Node {
                width: percent(100),
                height: px(32),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(if active { theme.primary } else { Color::NONE }),
        ))
        .with_children(|row| {
            if let Some(icon) = icon {
                spawn_icon(
                    row,
                    icons,
                    icon,
                    15.0,
                    if active {
                        theme.primary
                    } else {
                        theme.sidebar_foreground.with_alpha(0.62)
                    },
                );
                row.spawn(Node {
                    width: px(9),
                    ..default()
                });
            }
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|label_node| {
                label_node.spawn((
                    Text::new(label),
                    ui_text_font(font.clone(), 12.0),
                    TextColor(if active {
                        theme.sidebar_foreground.with_alpha(0.68)
                    } else {
                        theme.sidebar_foreground
                    }),
                    TextLayout::no_wrap(),
                ));
            });
            if count > 0 {
                row.spawn(Node {
                    width: px(28),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_children(|count_node| {
                    spawn_text(
                        count_node,
                        font,
                        count.to_string(),
                        10.0,
                        theme.sidebar_foreground.with_alpha(0.38),
                    );
                });
            }
        });
}

pub(crate) fn spawn_sidebar_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    label: &'static str,
    count: usize,
) {
    parent
        .spawn(Node {
            width: percent(100),
            height: px(32),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            ..default()
        })
        .with_children(|row| {
            spawn_icon(
                row,
                icons,
                icon,
                15.0,
                theme.sidebar_foreground.with_alpha(0.58),
            );
            row.spawn(Node {
                width: px(9),
                ..default()
            });
            spawn_text(row, font.clone(), label, 12.0, theme.sidebar_foreground);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if count > 0 {
                spawn_text(
                    row,
                    font,
                    count.to_string(),
                    10.0,
                    theme.sidebar_foreground.with_alpha(0.38),
                );
            }
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_sidebar_nav_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    label: &'static str,
    action: Option<UiAction>,
    active: bool,
) {
    let enabled = action.is_some();
    let mut item = parent.spawn((
        Node {
            width: percent(100),
            height: px(32),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            border: UiRect::left(px(1)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(if active { theme.primary } else { Color::NONE }),
    ));
    if let Some(action) = action {
        item.insert((Button, action));
    }
    item.with_children(|row| {
        spawn_icon(
            row,
            icons,
            icon,
            15.0,
            if active {
                theme.primary
            } else if enabled {
                theme.sidebar_foreground.with_alpha(0.62)
            } else {
                theme.sidebar_foreground.with_alpha(0.28)
            },
        );
        row.spawn(Node {
            width: px(9),
            ..default()
        });
        spawn_text(
            row,
            font,
            label,
            12.0,
            if active {
                theme.sidebar_foreground.with_alpha(0.68)
            } else if enabled {
                theme.sidebar_foreground
            } else {
                theme.sidebar_foreground.with_alpha(0.38)
            },
        );
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_workspace(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSessionView<'_>,
    native_setup: &NativeSetup,
    cache_stats: &CacheStatsJob,
    icons: Handle<Image>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme.background),
        ))
        .with_children(|workspace| {
            spawn_top_bar(workspace, font.clone(), icons.clone(), session, theme);
            match session.route {
                StudioRoute::Library
                    if session.config.library_paths().is_empty()
                        && session.library_view != LibraryView::Queue =>
                {
                    spawn_empty_library(workspace, font.clone(), session.scanning, theme);
                }
                StudioRoute::Library
                    if session.library_facet.is_none()
                        && matches!(
                            session.library_view,
                            LibraryView::Artists | LibraryView::Albums
                        ) =>
                {
                    spawn_library_collection(workspace, font.clone(), icons.clone(), session, theme)
                }
                StudioRoute::Library => spawn_library(
                    workspace,
                    font.clone(),
                    asset_server,
                    images,
                    local_images,
                    session,
                    theme,
                ),
                StudioRoute::Queue => {
                    spawn_analysis_queue_page(workspace, font.clone(), session, theme)
                }
                StudioRoute::Folders => {
                    spawn_folders(workspace, font.clone(), icons.clone(), session, theme)
                }
                StudioRoute::SongDetail => spawn_song_detail(
                    workspace,
                    font.clone(),
                    icons.clone(),
                    asset_server,
                    images,
                    local_images,
                    session,
                    theme,
                ),
                StudioRoute::LyricsWorkbench => {
                    spawn_lyrics_workbench_page(workspace, font.clone(), session, theme)
                }
                StudioRoute::Documentation => {
                    spawn_documentation(workspace, font.clone(), session, theme)
                }
                StudioRoute::ProcessingStudio => {
                    spawn_processing_studio(workspace, font.clone(), session, theme)
                }
                StudioRoute::AnalysisInspect => {
                    spawn_analysis_inspect_page(workspace, font.clone(), session, theme)
                }
                StudioRoute::Settings => spawn_settings(
                    workspace,
                    font.clone(),
                    icons.clone(),
                    session,
                    native_setup,
                    cache_stats,
                    theme,
                ),
                StudioRoute::Editor => {}
            }
            spawn_library_player(
                workspace,
                font,
                icons,
                asset_server,
                images,
                local_images,
                session,
                theme,
            );
        });
}

fn active_analysis_progress(session: &StudioSessionView<'_>) -> Option<usize> {
    session
        .analysis_tasks
        .iter()
        .find(|task| matches!(task.status, app_core::QueuedStatus::Analyzing(_)))
        .map(|task| {
            task.live.as_ref().map_or_else(
                || match &task.status {
                    app_core::QueuedStatus::Analyzing(progress) => (*progress).clamp(0, 100),
                    _ => 0,
                },
                |live| live.overall_progress.clamp(0, 100),
            )
        })
}

fn spawn_top_bar_progress(
    parent: &mut ChildSpawnerCommands,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let Some(progress) = active_analysis_progress(session) else {
        return;
    };
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(0),
                height: px(3),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.border.with_alpha(0.45)),
        ))
        .with_children(|track| {
            track.spawn((
                Node {
                    width: percent(progress as f32),
                    min_width: if progress > 0 { px(2) } else { px(0) },
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(theme.primary.with_alpha(0.94)),
            ));
        });
}

struct WorkspaceTitle {
    eyebrow: String,
    title: String,
    subtitle: String,
}

fn workspace_title(session: &StudioSessionView<'_>) -> WorkspaceTitle {
    match session.route {
        StudioRoute::Queue => WorkspaceTitle {
            eyebrow: "ANALYSIS".to_string(),
            title: "Processing Queue".to_string(),
            subtitle: format!(
                "{} record{} · waiting work can be reordered; terminal runs stay until rerun or deleted",
                session.analysis_tasks.len(),
                if session.analysis_tasks.len() == 1 { "" } else { "s" }
            ),
        },
        StudioRoute::Library if session.library_view == LibraryView::Queue => {
            let current = current_analysis_header(session);
            WorkspaceTitle {
                eyebrow: current_analysis_eyebrow(session).to_string(),
                title: current
                    .as_ref()
                    .map(|(title, _, _)| title.clone())
                    .unwrap_or_else(|| "Analysis".to_string()),
                subtitle: current
                    .map(|(_, artist, progress)| format!("{artist} · {progress}%"))
                    .unwrap_or_else(|| "No analysis is running".to_string()),
            }
        }
        StudioRoute::Library => WorkspaceTitle {
            eyebrow: if session.library_facet.is_some() {
                "MY LIBRARY".to_string()
            } else {
                session.library_view.eyebrow().to_string()
            },
            title: session
                .library_search
                .as_deref()
                .map(|query| format!("Results for “{query}”"))
                .unwrap_or_else(|| session.library_title().to_string()),
            subtitle: format!(
                "{} tracks · analysis workspace{}",
                session.songs.processed_count,
                if session.scanning { " · scanning" } else { "" }
            ),
        },
        StudioRoute::SongDetail => {
            let song = session.selected_song();
            WorkspaceTitle {
                eyebrow: "SONG".to_string(),
                title: song
                    .as_ref()
                    .map(|song| song.title.clone())
                    .unwrap_or_else(|| "Song".to_string()),
                subtitle: song
                    .map(|song| {
                        if song.album.is_empty() {
                            song.artist
                        } else {
                            format!("{} · {}", song.artist, song.album)
                        }
                    })
                    .unwrap_or_else(|| "Choose a song from the library".to_string()),
            }
        }
        StudioRoute::LyricsWorkbench => {
            let song = session.selected_song();
            WorkspaceTitle {
                eyebrow: "AUTHORING".to_string(),
                title: "Lyrics Workbench".to_string(),
                subtitle: song
                    .map(|song| format!("{} · {}", song.title, song.artist))
                    .unwrap_or_else(|| "Search, review, edit, save, and align lyrics".to_string()),
            }
        }
        StudioRoute::Folders => WorkspaceTitle {
            eyebrow: "MY LIBRARY".to_string(),
            title: "Folders".to_string(),
            subtitle: "Browse watched source locations and open the configured output folder. Uta! Studio never moves or deletes source media.".to_string(),
        },
        StudioRoute::ProcessingStudio => WorkspaceTitle {
            eyebrow: "PROCESSING STUDIO".to_string(),
            title: if session.selected_song.is_some() {
                "Audio & singing workflow"
            } else {
                "Choose a song to begin"
            }
            .to_string(),
            subtitle: if session.selected_song.is_some() {
                "Edit capability topology, role-preserving transform order, typed conditions, and artifact routing. Models & runtime owns installation; exact provider/backend readiness appears in Plan Preview."
            } else {
                "Select a song before configuring its processing workflow."
            }
            .to_string(),
        },
        StudioRoute::Documentation => WorkspaceTitle {
            eyebrow: "HELP".to_string(),
            title: "Documentation".to_string(),
            subtitle: match effective_ui_locale(session.config) {
                UiLocale::English => "Offline user guide · English",
                UiLocale::SimplifiedChinese => "离线使用说明 · 简体中文",
                UiLocale::Japanese => "オフラインユーザーガイド · 日本語",
            }
            .to_string(),
        },
        StudioRoute::AnalysisInspect => {
            let node_label = session
                .selected_analysis_node
                .as_deref()
                .unwrap_or("Workflow");
            WorkspaceTitle {
                eyebrow: "AUDIT VIEW".to_string(),
                title: current_analysis_header(session)
                    .map(|(title, _, _)| format!("{title} · {node_label}"))
                    .unwrap_or_else(|| node_label.to_string()),
                subtitle: "Review node execution, data contracts, fallbacks, and recorded evidence."
                    .to_string(),
            }
        }
        StudioRoute::Settings => WorkspaceTitle {
            eyebrow: "UTA! STUDIO".to_string(),
            title: "Settings".to_string(),
            subtitle: match session.settings_tab {
                SettingsTab::General => "General",
                SettingsTab::Storage => "Storage",
                SettingsTab::Models => "Models & runtime",
                SettingsTab::Analysis => "Analysis",
            }
            .to_string(),
        },
        StudioRoute::Editor => WorkspaceTitle {
            eyebrow: "EDITOR".to_string(),
            title: "Editor".to_string(),
            subtitle: String::new(),
        },
    }
}

fn workspace_toolbar_open(session: &StudioSessionView<'_>) -> bool {
    match session.route {
        StudioRoute::Queue => false,
        StudioRoute::Library if session.library_view == LibraryView::Queue => {
            current_analysis_file_hash(session).is_some()
        }
        StudioRoute::Library => true,
        StudioRoute::SongDetail => false,
        StudioRoute::LyricsWorkbench => false,
        StudioRoute::Folders | StudioRoute::Documentation | StudioRoute::Settings => true,
        StudioRoute::ProcessingStudio => session.workflow.is_some(),
        StudioRoute::AnalysisInspect => current_analysis_file_hash(session).is_some(),
        _ => false,
    }
}

fn spawn_workspace_toolbar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    match session.route {
        StudioRoute::Queue => {}
        StudioRoute::Library if session.library_view == LibraryView::Queue => {
            if let Some(file_hash) = current_analysis_file_hash(session) {
                spawn_analysis_header_toolbar(parent, font, icons, theme, session, &file_hash);
            }
        }
        StudioRoute::Library => {
            spawn_library_header_toolbar(parent, font, icons, session, theme);
        }
        StudioRoute::Folders => {
            spawn_toolbar_button(
                parent,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Repeat,
                "Rescan all",
                UiAction::from(LibraryCommand::RescanLibrary),
                false,
            );
            spawn_toolbar_button(
                parent,
                font,
                icons,
                theme,
                UiIcon::Add,
                "Add folder",
                UiAction::from(LibraryCommand::ChooseFolder),
                false,
            );
        }
        StudioRoute::Documentation => {
            spawn_documentation_header_actions(parent, font, session, theme);
        }
        StudioRoute::Settings => {
            spawn_settings_header_toolbar(parent, font, session, theme);
        }
        StudioRoute::AnalysisInspect => {
            if let Some(file_hash) = current_analysis_file_hash(session) {
                spawn_analysis_header_toolbar(parent, font, icons, theme, session, &file_hash);
            }
        }
        StudioRoute::ProcessingStudio => {
            spawn_compact_primary_action_button(
                parent,
                font.clone(),
                theme,
                "Re-run",
                UiAction::from(AnalysisCommand::RunWorkflow),
            );
            spawn_compact_action_button(
                parent,
                font,
                theme,
                "Save",
                UiAction::from(AnalysisCommand::SaveWorkflow),
            );
        }
        _ => {}
    }
}

pub(crate) fn should_show_workspace_eyebrow(subtitle: &str) -> bool {
    subtitle.is_empty()
}

pub(crate) fn spawn_top_bar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let title = workspace_title(session);
    let toolbar_open = workspace_toolbar_open(session);
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: percent(100),
                min_height: px(WORKSPACE_TOP_BAR_MIN_HEIGHT),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(0),
                row_gap: px(6),
                padding: UiRect::axes(px(12), px(8)),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(theme.topbar),
            BorderColor::all(theme.border.with_alpha(0.4)),
        ))
        .with_children(|bar| {
            if let Some(back_action) = navigation_back_action(session) {
                spawn_icon_button(
                    bar,
                    icons.clone(),
                    theme,
                    UiIcon::ArrowLeft,
                    back_action,
                    false,
                    false,
                    34.0,
                );
            }
            bar.spawn(Node {
                width: px(10),
                flex_shrink: 0.0,
                ..default()
            });
            bar.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|copy| {
                if should_show_workspace_eyebrow(&title.subtitle) {
                    spawn_text(copy, font.clone(), title.eyebrow, 8.0, theme.primary);
                }
                spawn_text(copy, font.clone(), title.title, 18.0, theme.foreground);
                if !title.subtitle.is_empty() {
                    copy.spawn(Node {
                        width: percent(100),
                        min_width: px(0),
                        overflow: Overflow::clip(),
                        ..default()
                    })
                    .with_children(|line| {
                        spawn_text(
                            line,
                            font.clone(),
                            title.subtitle,
                            8.5,
                            theme.muted_foreground,
                        );
                    });
                }
            });
            bar.spawn(Node {
                min_width: px(0),
                flex_shrink: 1.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(6),
                row_gap: px(6),
                ..default()
            })
            .with_children(|actions| {
                if toolbar_open {
                    actions
                        .spawn(Node {
                            min_width: px(0),
                            flex_shrink: 1.0,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::FlexEnd,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(8),
                            row_gap: px(6),
                            ..default()
                        })
                        .with_children(|toolbar| {
                            spawn_workspace_toolbar(
                                toolbar,
                                font.clone(),
                                icons.clone(),
                                session,
                                theme,
                            );
                        });
                }
                actions
                    .spawn(Node {
                        position_type: PositionType::Relative,
                        width: px(34),
                        height: px(34),
                        flex_shrink: 0.0,
                        ..default()
                    })
                    .with_children(|search| {
                        spawn_icon_button(
                            search,
                            icons.clone(),
                            theme,
                            UiIcon::Search,
                            UiAction::from(AppCommand::ToggleGlobalSearch),
                            session.search_open
                                || if session.route == StudioRoute::Documentation {
                                    !session.documentation.query.trim().is_empty()
                                } else {
                                    session.library_search.is_some()
                                },
                            false,
                            34.0,
                        );
                        if session.search_open {
                            search
                                .spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        right: px(0),
                                        top: px(40),
                                        width: px(410),
                                        max_width: Val::Vw(90.0),
                                        flex_direction: FlexDirection::Column,
                                        padding: UiRect::all(px(12)),
                                        row_gap: px(9),
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(px(7)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.card.with_alpha(0.98)),
                                    BorderColor::all(theme.border.with_alpha(0.86)),
                                    BoxShadow::new(
                                        Color::srgba(0.0, 0.0, 0.0, 0.24),
                                        px(0),
                                        px(14),
                                        px(30),
                                        px(-10),
                                    ),
                                    ZIndex(75),
                                ))
                                .with_children(|popover| {
                                    if session.route == StudioRoute::Documentation {
                                        popover.spawn((
                                            DocumentationSearchInput,
                                            EditableText {
                                                visible_width: Some(38.0),
                                                max_characters: Some(120),
                                                ..EditableText::new(
                                                    session.documentation.query.as_str(),
                                                )
                                            },
                                            Node {
                                                width: percent(100),
                                                height: px(42),
                                                align_items: AlignItems::Center,
                                                padding: UiRect::horizontal(px(12)),
                                                border: UiRect::all(px(1)),
                                                border_radius: BorderRadius::all(px(5)),
                                                ..default()
                                            },
                                            ui_text_font(font.clone(), 12.0),
                                            TextColor(theme.foreground),
                                            TextLayout::no_wrap(),
                                            TextCursorStyle {
                                                color: theme.primary,
                                                selected_text_color: Some(
                                                    theme.primary_foreground,
                                                ),
                                                ..default()
                                            },
                                            BackgroundColor(
                                                theme.background.with_alpha(0.72),
                                            ),
                                            BorderColor::all(theme.border.with_alpha(0.68)),
                                            TabIndex(0),
                                            AutoFocus,
                                        ));
                                        spawn_wrapped_text(
                                            popover,
                                            font.clone(),
                                            "Search the offline user guide · results update as you type",
                                            9.0,
                                            theme.muted_foreground,
                                        );
                                        return;
                                    }
                                    popover.spawn((
                                        LibrarySearchInput,
                                        EditableText {
                                            visible_width: Some(38.0),
                                            max_characters: Some(120),
                                            ..EditableText::new(
                                                session.library_search.as_deref().unwrap_or(""),
                                            )
                                        },
                                        Node {
                                            width: percent(100),
                                            height: px(42),
                                            align_items: AlignItems::Center,
                                            padding: UiRect::horizontal(px(12)),
                                            border: UiRect::all(px(1)),
                                            border_radius: BorderRadius::all(px(5)),
                                            ..default()
                                        },
                                        ui_text_font(font.clone(), 12.0),
                                        TextColor(theme.foreground),
                                        TextLayout::no_wrap(),
                                        TextCursorStyle {
                                            color: theme.primary,
                                            selected_text_color: Some(theme.primary_foreground),
                                            ..default()
                                        },
                                        BackgroundColor(theme.background.with_alpha(0.72)),
                                        BorderColor::all(theme.border.with_alpha(0.68)),
                                        TabIndex(0),
                                        AutoFocus,
                                    ));
                                    popover
                                .spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::Center,
                                    column_gap: px(8),
                                    ..default()
                                })
                                .with_children(|footer| {
                                    spawn_wrapped_text(
                                        footer,
                                        font.clone(),
                                        "Search tracks, artists, albums, and playlists · Enter",
                                        9.0,
                                        theme.muted_foreground,
                                    );
                                    footer.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    if session.library_search.is_some() {
                                        spawn_text_button(
                                            footer,
                                            font.clone(),
                                            theme,
                                            "Clear",
                                            9.0,
                                            UiAction::from(LibraryCommand::ClearLibrarySearch),
                                        );
                                    }
                                    spawn_text_button(
                                        footer,
                                        font.clone(),
                                        theme,
                                        "Search",
                                        9.0,
                                        UiAction::from(LibraryCommand::ApplyLibrarySearch),
                                    );
                                });
                                });
                        }
                    });
                let has_active_analysis = session.analysis_tasks.iter().any(|task| {
                    matches!(
                        task.status,
                        app_core::QueuedStatus::Staged
                            | app_core::QueuedStatus::Queued
                            | app_core::QueuedStatus::Analyzing(_)
                    )
                });
                spawn_activity_button(
                    actions,
                    icons.clone(),
                    theme,
                    session.activity_open,
                    has_active_analysis,
                );
            });
            spawn_top_bar_progress(bar, session, theme);
        });
}

pub(crate) fn spawn_about_dialog(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    logo: Handle<Image>,
    config: &AppConfig,
    theme: &StudioTheme,
) {
    parent.spawn((
        Button,
        UiAction::from(AppCommand::CloseAbout),
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
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(110),
    ));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                width: px(520),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(12),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            UiTransform::from_xy(percent(-50), percent(-50)),
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            ZIndex(111),
        ))
        .with_children(|dialog| {
            dialog
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(16),
                    ..default()
                })
                .with_children(|identity| {
                    identity.spawn((
                        Node {
                            width: px(72),
                            height: px(72),
                            overflow: Overflow::clip(),
                            border_radius: BorderRadius::all(px(16)),
                            ..default()
                        },
                        ImageNode::new(logo),
                    ));
                    identity
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(copy, font.clone(), "Uta! Studio", 24.0, theme.foreground);
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                "AI generation · precise chart editing · interoperable export",
                                10.0,
                                theme.muted_foreground,
                            );
                        });
                    spawn_text_button(
                        identity,
                        font.clone(),
                        theme,
                        "Close",
                        10.0,
                        UiAction::from(AppCommand::CloseAbout),
                    );
                });
            spawn_text(
                dialog,
                font.clone(),
                localized_message(
                    config,
                    UiMessage::AppVersion,
                    &[("{version}", env!("CARGO_PKG_VERSION"))],
                ),
                11.0,
                theme.foreground,
            );
            spawn_text(
                dialog,
                font.clone(),
                "License · GPL-3.0-or-later",
                10.0,
                theme.muted_foreground,
            );
            dialog.spawn((
                Node {
                    width: percent(100),
                    height: px(1),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.7)),
            ));
            spawn_text(dialog, font.clone(), "ATTRIBUTIONS", 9.0, theme.primary);
            for attribution in [
                "Lyrics data · LRCLIB / QQ Music / Kugou / NetEase",
                "Stem separation · native RoFormer",
                "Stem architecture · audio-separator (MIT)",
                "Transcript fusion · FireRedASR2-AED / Qwen3-ASR",
                "Forced alignment · pinned Qwen3 Forced Aligner",
                "Optional Japanese model · NextFire MMS Karaoke (AGPL-3.0)",
                "CJK romanization · fugashi / pypinyin / hangul-romanize / ToJyutping",
            ] {
                spawn_wrapped_text(
                    dialog,
                    font.clone(),
                    attribution,
                    9.0,
                    theme.muted_foreground,
                );
            }
        });
}

#[derive(SystemParam)]
pub(crate) struct CloseRequestState<'w> {
    library: Res<'w, LibraryState>,
    analysis: Res<'w, AnalysisUiState>,
    editor: Res<'w, EditorUiState>,
    dialogs: ResMut<'w, DialogState>,
    jobs: Res<'w, AsyncJobs>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_window_close_requests(
    mut requests: MessageReader<bevy::window::WindowCloseRequested>,
    mut commands: Commands,
    audio: Res<NativeAudio>,
    library_audio: Res<NativeLibraryAudio>,
    mut state: CloseRequestState,
    setup: Res<NativeSetup>,
    diagnostics: Res<NativeDiagnostics>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let Some(request) = requests.read().next() else {
        return;
    };
    let has_unsaved_edits = state
        .editor
        .editor
        .as_ref()
        .is_some_and(|editor| editor.dirty);
    let background_work = state.library.scanning
        || state.jobs.authoring_busy
        || setup.receiver.is_some()
        || diagnostics.receiver.is_some()
        || state.jobs.export_job.receiver.is_some()
        || state.jobs.editor_load_job.receiver.is_some()
        || state.jobs.lyrics_search_job.receiver.is_some()
        || state.analysis.analysis_tasks.iter().any(|task| {
            matches!(
                task.status,
                app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
            )
        });
    if has_unsaved_edits || background_work {
        state.dialogs.pending_leave = Some(PendingLeave::Exit);
        invalidated.invalidate(UiDirtyRegion::Dialog);
    } else {
        let _ = audio.0.stop();
        let _ = library_audio.0.stop();
        commands.entity(request.window).despawn();
    }
}
