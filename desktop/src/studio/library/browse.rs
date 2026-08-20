use super::*;
use crate::studio::*;

pub(crate) fn spawn_export_all_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
) {
    let open = session.export_all_open;
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: px(124),
                height: px(34),
                flex_shrink: 0.0,
                ..default()
            },
            ZIndex(if open { 70 } else { 0 }),
        ))
        .with_children(|control| {
            control
                .spawn((
                    Button,
                    UiAction::from(LibraryCommand::ToggleExportAllMenu),
                    Node {
                        width: percent(100),
                        height: px(34),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(10)),
                        column_gap: px(7),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(if open { 0.82 } else { 0.46 })),
                    BorderColor::all(if open {
                        theme.primary.with_alpha(0.68)
                    } else {
                        theme.border.with_alpha(0.54)
                    }),
                ))
                .with_children(|button| {
                    spawn_icon(
                        button,
                        icons.clone(),
                        UiIcon::Save,
                        13.0,
                        theme.muted_foreground,
                    );
                    spawn_text(
                        button,
                        font.clone(),
                        if session.export_job.receiver.is_some() {
                            "Exporting…"
                        } else {
                            "Export all"
                        },
                        9.0,
                        theme.foreground,
                    );
                    button.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_icon(
                        button,
                        icons.clone(),
                        UiIcon::ChevronDown,
                        12.0,
                        theme.muted_foreground,
                    );
                });
            if open {
                control
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            right: px(0),
                            top: px(38),
                            width: px(220),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(5)),
                            row_gap: px(2),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.card),
                        BorderColor::all(theme.border.with_alpha(0.9)),
                        ZIndex(70),
                    ))
                    .with_children(|menu| {
                        for (label, detail, action) in [
                            (
                                "UTZ packages",
                                "One validated .utz per ready chart",
                                UiAction::from(LibraryCommand::ExportAllUtz),
                            ),
                            (
                                "UltraStar bundles",
                                "One .txt bundle per ready chart",
                                UiAction::from(LibraryCommand::ExportAllUltraStar),
                            ),
                        ] {
                            menu.spawn((
                                Button,
                                action,
                                Node {
                                    width: percent(100),
                                    min_height: px(46),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::FlexStart,
                                    padding: UiRect::horizontal(px(9)),
                                    row_gap: px(2),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                            ))
                            .with_children(|option| {
                                spawn_text(option, font.clone(), label, 10.0, theme.foreground);
                                spawn_text(
                                    option,
                                    font.clone(),
                                    detail,
                                    8.0,
                                    theme.muted_foreground,
                                );
                            });
                        }
                    });
            }
        });
}

pub(crate) fn spawn_library_collection(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let artists = session.library_view == LibraryView::Artists;
    debug_assert!(artists || session.library_view == LibraryView::Albums);
    let (title, description, icon, items) = if artists {
        (
            "Artists",
            "Browse artists in the main workspace. The sidebar remains a quiet navigation surface.",
            UiIcon::Artists,
            &session.menu_items.artists,
        )
    } else {
        (
            "Albums",
            "Browse albums in the main workspace. Choose one to see all matching tracks.",
            UiIcon::Albums,
            &session.menu_items.albums,
        )
    };
    parent
        .spawn(Node {
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|page| {
            page.spawn((
                Node {
                    width: percent(100),
                    padding: UiRect::axes(px(28), px(24)),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::bottom(px(1)),
                    ..default()
                },
                BorderColor::all(theme.border.with_alpha(0.55)),
            ))
            .with_children(|header| {
                spawn_text(header, font.clone(), "MY LIBRARY", 9.0, theme.primary);
                spawn_text(header, font.clone(), title, 34.0, theme.foreground);
                spawn_wrapped_text(
                    header,
                    font.clone(),
                    format!("{description} · {} total", items.len()),
                    11.0,
                    theme.muted_foreground,
                );
            });
            page.spawn((
                LibrarySongList,
                ScrollPosition::default(),
                Node {
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    align_content: AlignContent::FlexStart,
                    padding: UiRect::all(px(22)),
                    row_gap: px(12),
                    column_gap: px(12),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|list| {
                for item in items {
                    let facet = if artists {
                        LibraryFacet::Artist {
                            value: item.value.clone(),
                            label: item.label.clone(),
                        }
                    } else {
                        LibraryFacet::Album {
                            value: item.value.clone(),
                            label: item.label.clone(),
                        }
                    };
                    list.spawn((
                        Button,
                        UiAction::from(LibraryCommand::SetLibraryFacet(facet)),
                        Node {
                            width: px(230),
                            min_height: px(72),
                            align_items: AlignItems::Center,
                            padding: UiRect::all(px(12)),
                            column_gap: px(11),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.card.with_alpha(0.36)),
                        BorderColor::all(theme.border.with_alpha(0.42)),
                    ))
                    .with_children(|card| {
                        card.spawn((
                            Node {
                                width: px(38),
                                height: px(38),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(theme.primary.with_alpha(0.09)),
                        ))
                        .with_children(|mark| {
                            spawn_icon(mark, icons.clone(), icon, 17.0, theme.primary);
                        });
                        card.spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::clip(),
                            ..default()
                        })
                        .with_children(|copy| {
                            copy.spawn((
                                Text::new(item.label.clone()),
                                ui_text_font(font.clone(), 12.0),
                                TextColor(theme.foreground),
                                TextLayout::no_wrap(),
                            ));
                            spawn_text(
                                copy,
                                font.clone(),
                                format!(
                                    "{} track{}",
                                    item.count,
                                    if item.count == 1 { "" } else { "s" }
                                ),
                                9.0,
                                theme.muted_foreground,
                            );
                        });
                    });
                }
                if items.is_empty() {
                    spawn_wrapped_text(
                        list,
                        font,
                        if artists {
                            "No artist metadata is available yet."
                        } else {
                            "No album metadata is available yet."
                        },
                        11.0,
                        theme.muted_foreground,
                    );
                }
            });
        });
}

#[expect(
    clippy::too_many_arguments,
    reason = "this declarative route renderer receives the shared UI asset set"
)]
pub(crate) fn spawn_library(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|library| {
            library
                .spawn((
                    Node {
                        width: percent(100),
                        padding: if session.library_view == LibraryView::Queue {
                            UiRect::axes(px(22), px(10))
                        } else {
                            UiRect::axes(px(28), px(24))
                        },
                        flex_direction: FlexDirection::Column,
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BorderColor::all(theme.border.with_alpha(0.55)),
                ))
                .with_children(|header| {
                    spawn_text(
                        header,
                        font.clone(),
                        if session.library_facet.is_some() {
                            "MY LIBRARY"
                        } else {
                            session.library_view.eyebrow()
                        },
                        9.0,
                        theme.muted_foreground,
                    );
                    if session.library_view == LibraryView::Queue {
                        let current = current_analysis_header(session);
                        header
                            .spawn(Node {
                                width: percent(100),
                                align_items: AlignItems::FlexEnd,
                                column_gap: px(16),
                                ..default()
                            })
                            .with_children(|title_row| {
                                title_row
                                    .spawn(Node {
                                        min_width: px(0),
                                        flex_grow: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(2),
                                        ..default()
                                    })
                                    .with_children(|song| {
                                        if let Some((title, artist, _)) = current.as_ref() {
                                            spawn_text(
                                                song,
                                                font.clone(),
                                                title.clone(),
                                                22.0,
                                                theme.foreground,
                                            );
                                            spawn_text(
                                                song,
                                                font.clone(),
                                                artist.clone(),
                                                11.0,
                                                theme.muted_foreground,
                                            );
                                        } else {
                                            spawn_text(
                                                song,
                                                font.clone(),
                                                "No analysis is running",
                                                22.0,
                                                theme.foreground,
                                            );
                                        }
                                    });
                            });
                    } else {
                        spawn_text(
                            header,
                            font.clone(),
                            session
                                .library_search
                                .as_deref()
                                .map(|query| format!("Results for “{query}”"))
                                .unwrap_or_else(|| session.library_title().to_string()),
                            34.0,
                            theme.foreground,
                        );
                    }
                    spawn_text(
                        header,
                        font.clone(),
                        if session.library_view == LibraryView::Queue {
                            let active = session
                                .analysis_tasks
                                .iter()
                                .filter(|task| {
                                    matches!(
                                        task.status,
                                        app_core::QueuedStatus::Queued
                                            | app_core::QueuedStatus::Analyzing(_)
                                    )
                                })
                                .count();
                            let failed = session
                                .analysis_tasks
                                .iter()
                                .filter(|task| {
                                    matches!(task.status, app_core::QueuedStatus::Failed(_))
                                })
                                .count();
                            format!("{active} active · {failed} failed · live updates")
                        } else {
                            format!(
                                "{} tracks · production workspace{}",
                                session.songs.processed_count,
                                if session.scanning { " · scanning" } else { "" }
                            )
                        },
                        11.0,
                        theme.muted_foreground,
                    );
                    if session.library_view == LibraryView::Queue
                        && !session.analysis_history.is_empty()
                    {
                        header
                            .spawn(Node {
                                width: percent(100),
                                align_items: AlignItems::Center,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(6),
                                row_gap: px(6),
                                margin: UiRect::top(px(6)),
                                ..default()
                            })
                            .with_children(|history| {
                                spawn_text(
                                    history,
                                    font.clone(),
                                    "RECENT",
                                    8.0,
                                    theme.muted_foreground,
                                );
                                if session.pending_analysis_history_clear {
                                    spawn_text(
                                        history,
                                        font.clone(),
                                        "Delete every saved analysis session?",
                                        8.0,
                                        theme.destructive,
                                    );
                                    spawn_text_button(
                                        history,
                                        font.clone(),
                                        theme,
                                        "Cancel",
                                        8.0,
                                        UiAction::from(AnalysisCommand::CancelClearAnalysisHistory),
                                    );
                                    spawn_text_button(
                                        history,
                                        font.clone(),
                                        theme,
                                        "Delete history",
                                        8.0,
                                        UiAction::from(AnalysisCommand::ConfirmClearAnalysisHistory),
                                    );
                                } else {
                                    spawn_text_button(
                                        history,
                                        font.clone(),
                                        theme,
                                        "Clear history…",
                                        8.0,
                                        UiAction::from(AnalysisCommand::RequestClearAnalysisHistory),
                                    );
                                }
                                for item in session.analysis_history.iter().take(5) {
                                    let selected = session.selected_analysis_history == Some(item.id);
                                    spawn_text_button(
                                        history,
                                        font.clone(),
                                        theme,
                                        if selected {
                                            format!("· {}", item.title)
                                        } else {
                                            item.title.clone()
                                        },
                                        8.0,
                                        UiAction::from(AnalysisCommand::SelectAnalysisHistory(Some(item.id))),
                                    );
                                }
                            });
                    }
                    if session.library_view != LibraryView::Queue {
                    header
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(8),
                            row_gap: px(8),
                            margin: UiRect::top(px(12)),
                            ..default()
                        })
                        .with_children(|tools| {
                            tools.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            if session.library_view == LibraryView::All
                                && session.library_facet.is_none()
                            {
                                spawn_toolbar_button(
                                    tools,
                                    font.clone(),
                                    icons.clone(),
                                    theme,
                                    UiIcon::Repeat,
                                    if session.scanning {
                                        "Scanning…"
                                    } else {
                                        "Rescan library"
                                    },
                                    UiAction::from(LibraryCommand::RescanLibrary),
                                    false,
                                );
                            }
                            spawn_library_filter_select(
                                tools,
                                font.clone(),
                                icons.clone(),
                                theme,
                                LibrarySelectKind::Status,
                                session,
                            );
                            spawn_library_filter_select(
                                tools,
                                font.clone(),
                                icons.clone(),
                                theme,
                                LibrarySelectKind::TranscriptSource,
                                session,
                            );
                            spawn_export_all_menu(
                                tools,
                                font.clone(),
                                icons.clone(),
                                theme,
                                session,
                            );
                            if app_core::analysis_runtime_status().ready {
                                spawn_toolbar_button(
                                    tools,
                                    font.clone(),
                                    icons.clone(),
                                    theme,
                                    UiIcon::Sparkles,
                                    "Analyze all",
                                    UiAction::from(LibraryCommand::AnalyzeAll),
                                    false,
                                );
                            } else {
                                spawn_toolbar_button(
                                    tools,
                                    font.clone(),
                                    icons.clone(),
                                    theme,
                                    UiIcon::Repair,
                                    "Set up analysis",
                                    UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
                                    false,
                                );
                            }
                            spawn_toolbar_button(
                                tools,
                                font.clone(),
                                icons.clone(),
                                theme,
                                if session.config.song_list_view.as_deref() == Some("grid") {
                                    UiIcon::List
                                } else {
                                    UiIcon::Grid
                                },
                                if session.config.song_list_view.as_deref() == Some("grid") {
                                    "Table view"
                                } else {
                                    "Grid view"
                                },
                                UiAction::from(LibraryCommand::ToggleLibraryLayout),
                                false,
                            );
                        });
                    }
                });

            library
                .spawn((
                    LibrarySongList,
                    ScrollPosition(Vec2::new(0.0, session.library_scroll_offset)),
                    Node {
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: if session.library_view != LibraryView::Queue
                            && session.config.song_list_view.as_deref() == Some("grid")
                        {
                            FlexDirection::Row
                        } else {
                            FlexDirection::Column
                        },
                        flex_wrap: if session.library_view != LibraryView::Queue
                            && session.config.song_list_view.as_deref() == Some("grid")
                        {
                            FlexWrap::Wrap
                        } else {
                            FlexWrap::NoWrap
                        },
                        align_content: AlignContent::FlexStart,
                        padding: if session.library_view != LibraryView::Queue
                            && session.config.song_list_view.as_deref() == Some("grid")
                        {
                            UiRect::all(px(22))
                        } else {
                            UiRect::ZERO
                        },
                        row_gap: if session.library_view == LibraryView::Queue {
                            px(0)
                        } else {
                            px(14)
                        },
                        column_gap: px(14),
                        overflow: if session.library_view == LibraryView::Queue {
                            Overflow::clip()
                        } else {
                            Overflow::scroll_y()
                        },
                        ..default()
                    },
                ))
                .with_children(|list| {
                    let grid = session.config.song_list_view.as_deref() == Some("grid");
                    if session.library_view == LibraryView::Queue {
                        spawn_analysis_session_overview(list, font.clone(), session, theme);
                        return;
                    }
                    if !grid {
                        spawn_song_header(list, font.clone(), theme);
                    }
                    for song in &session.songs.processed {
                        let cover = album_art_handle(song, asset_server, images, local_images);
                        let analysis_status = session
                            .analysis_tasks
                            .iter()
                            .find(|task| task.file_hash == song.file_hash)
                            .map(|task| &task.status);
                        if grid {
                            spawn_library_song_card(
                                list,
                                font.clone(),
                                theme,
                                song,
                                cover,
                                analysis_status,
                            );
                        } else {
                            spawn_library_song_row(
                                list,
                                font.clone(),
                                theme,
                                song,
                                cover,
                                analysis_status,
                            );
                        }
                    }
                    if session.songs.processed.is_empty() {
                        spawn_text(
                            list,
                            font.clone(),
                            if session.library_view == LibraryView::Queue
                                && session.analysis_tasks.iter().any(|task| {
                                    matches!(task.status, app_core::QueuedStatus::Failed(_))
                                })
                            {
                                "No active jobs. Open Activity to review failed analyses."
                            } else if session.library_view == LibraryView::Queue {
                                if session.analysis_history.is_empty() {
                                    "The analysis queue is empty. Choose an unanalyzed song to start."
                                } else {
                                    "No analysis is running. Select a previous session above."
                                }
                            } else if session.scanning {
                                "Scanning your library…"
                            } else {
                                "This library is empty or still being scanned."
                            },
                            13.0,
                            theme.muted_foreground,
                        );
                    } else if session.songs.processed.len() < session.songs.count {
                        spawn_action_button(
                            list,
                            font.clone(),
                            theme,
                            format!(
                                "Load more · {} of {}",
                                session.songs.processed.len(),
                                session.songs.count
                            ),
                            UiAction::from(LibraryCommand::LoadMoreSongs),
                        );
                    }
                });

            if let Some(context) = session.song_context.as_ref() {
                spawn_song_context_menu(library, font.clone(), theme, context);
            }
        });
}

pub(crate) fn spawn_song_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    context: &SongContextMenu,
) {
    parent.spawn((
        Button,
        UiAction::from(LibraryCommand::DismissSongContext),
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
    let left = (context.position.x - SIDEBAR_WIDTH - 14.0).max(8.0);
    let top = (context.position.y - 58.0).max(8.0);
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(270),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(2),
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
                context.song.title.clone(),
                11.0,
                theme.foreground,
            );
            spawn_text(
                menu,
                font.clone(),
                format!("{} · Track actions", context.song.artist),
                8.0,
                theme.muted_foreground,
            );
            menu.spawn(Node {
                height: px(5),
                ..default()
            });
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Play original",
                11.0,
                UiAction::from(LibraryCommand::PlayLibrarySong(
                    context.song.file_hash.clone(),
                )),
            );
            if context.song.editor_ready {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Edit chart",
                    11.0,
                    UiAction::from(LibraryCommand::OpenEditor(context.song.file_hash.clone())),
                );
            } else if !context.song.authoring_ready {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Analyze song",
                    11.0,
                    UiAction::from(LibraryCommand::AnalyzeSong(context.song.file_hash.clone())),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Open track page",
                11.0,
                UiAction::from(LibraryCommand::OpenSong(context.song.file_hash.clone())),
            );
            if context.song.authoring_ready {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Export Uta package (.utz)",
                    11.0,
                    UiAction::from(LibraryCommand::ExportUtz(context.song.file_hash.clone())),
                );
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Export UltraStar (.txt)",
                    11.0,
                    UiAction::from(LibraryCommand::ExportUltraStar(
                        context.song.file_hash.clone(),
                    )),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Open with default app",
                11.0,
                UiAction::from(LibraryCommand::OpenSource(context.song.path.clone())),
            );
            spawn_text_button(
                menu,
                font,
                theme,
                "Show in file manager",
                11.0,
                UiAction::from(LibraryCommand::RevealSource(context.song.path.clone())),
            );
        });
}

pub(crate) fn song_status_copy(
    song: &Song,
    analysis_status: Option<&app_core::QueuedStatus>,
    theme: &StudioTheme,
) -> (String, Color) {
    match analysis_status {
        Some(app_core::QueuedStatus::Queued) => ("Queued".to_string(), theme.primary),
        Some(app_core::QueuedStatus::Analyzing(progress)) => {
            (format!("Analyzing · {progress}%"), theme.primary)
        }
        Some(app_core::QueuedStatus::Failed(_)) => ("Failed".to_string(), theme.destructive),
        None if song.authoring_ready => ("Ready to author".to_string(), theme.pitch_contour),
        None if song.is_analyzed => ("Analysis incomplete".to_string(), theme.editor_warning),
        None => ("Not analyzed".to_string(), theme.muted_foreground),
    }
}

pub(crate) fn spawn_library_song_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    song: &Song,
    cover: Handle<Image>,
    analysis_status: Option<&app_core::QueuedStatus>,
) {
    let context_song = song.clone();
    parent
        .spawn((
            Button,
            UiPointerApi(&["ui.pointer.song.primary", "ui.pointer.song.secondary"]),
            Node {
                width: percent(100),
                min_height: px(60),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(22), px(8)),
                border: UiRect::bottom(px(1)),
                column_gap: px(12),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(theme.border.with_alpha(0.28)),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(44),
                    height: px(44),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                ImageNode::new(cover),
                BorderColor::all(theme.border.with_alpha(0.42)),
                Pickable::IGNORE,
            ));
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                row_gap: px(2),
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|metadata| {
                metadata.spawn((
                    Text::new(song.title.clone()),
                    ui_text_font(font.clone(), 11.0),
                    TextColor(theme.foreground),
                    TextLayout::no_wrap(),
                ));
                metadata.spawn((
                    Text::new(song.language.as_deref().unwrap_or("Language unknown")),
                    ui_text_font(font.clone(), 9.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::no_wrap(),
                ));
            });
            row.spawn(Node {
                width: px(150),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|artist| {
                spawn_text(
                    artist,
                    font.clone(),
                    if song.artist.trim().is_empty() {
                        "Unknown artist".to_string()
                    } else {
                        song.artist.clone()
                    },
                    9.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(180),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|album| {
                spawn_text(
                    album,
                    font.clone(),
                    if song.album.trim().is_empty() {
                        "—".to_string()
                    } else {
                        song.album.clone()
                    },
                    9.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(64),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|duration| {
                spawn_text(
                    duration,
                    font.clone(),
                    format_duration(song.duration_secs),
                    10.0,
                    theme.muted_foreground,
                );
            });
            let (status_label, status_color) = song_status_copy(song, analysis_status, theme);
            row.spawn(Node {
                width: px(150),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|status| {
                status
                    .spawn((
                        Node {
                            min_height: px(24),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(8)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(status_color.with_alpha(0.08)),
                        BorderColor::all(status_color.with_alpha(0.34)),
                    ))
                    .with_children(|badge| {
                        spawn_text(badge, font.clone(), status_label, 8.0, status_color);
                    });
            });
        })
        .observe(
            move |mut event: On<Pointer<Press>>,
                  mut shell: ResMut<ShellState>,
                  mut library: ResMut<LibraryState>,
                  mut dialogs: ResMut<DialogState>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                open_song_from_pointer(
                    event.button,
                    event.pointer_location.position,
                    &context_song,
                    &mut shell,
                    &mut library,
                    &mut dialogs,
                    &mut invalidated,
                );
            },
        );
}

pub(crate) fn spawn_library_song_card(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    song: &Song,
    cover: Handle<Image>,
    analysis_status: Option<&app_core::QueuedStatus>,
) {
    let context_song = song.clone();
    let (status_label, status_color) = song_status_copy(song, analysis_status, theme);
    parent
        .spawn((
            Button,
            UiPointerApi(&["ui.pointer.song.primary", "ui.pointer.song.secondary"]),
            Node {
                width: px(172),
                min_height: px(226),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(6),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.34)),
            BorderColor::all(theme.border.with_alpha(0.38)),
        ))
        .with_children(|card| {
            card.spawn((
                Node {
                    width: px(154),
                    height: px(154),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                ImageNode::new(cover),
                Pickable::IGNORE,
            ));
            for (text, size, color) in [
                (song.title.clone(), 11.0, theme.foreground),
                (song.artist.clone(), 9.0, theme.muted_foreground),
                (status_label, 8.0, status_color),
            ] {
                card.spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    height: px(size + 5.0),
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|line| {
                    line.spawn((
                        Text::new(text),
                        ui_text_font(font.clone(), size),
                        TextColor(color),
                        TextLayout::no_wrap(),
                    ));
                });
            }
        })
        .observe(
            move |mut event: On<Pointer<Press>>,
                  mut shell: ResMut<ShellState>,
                  mut library: ResMut<LibraryState>,
                  mut dialogs: ResMut<DialogState>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                open_song_from_pointer(
                    event.button,
                    event.pointer_location.position,
                    &context_song,
                    &mut shell,
                    &mut library,
                    &mut dialogs,
                    &mut invalidated,
                );
            },
        );
}

pub(crate) fn open_song_from_pointer(
    button: PointerButton,
    position: Vec2,
    song: &Song,
    shell: &mut ShellState,
    library: &mut LibraryState,
    dialogs: &mut DialogState,
    invalidated: &mut UiInvalidated,
) {
    match button {
        PointerButton::Primary => {
            library.selected_song = Some(song.file_hash.clone());
            shell.route = StudioRoute::SongDetail;
            dialogs.song_context = None;
            shell.notice = None;
        }
        PointerButton::Secondary => {
            dialogs.song_context = Some(SongContextMenu {
                song: song.clone(),
                position,
            });
        }
        PointerButton::Middle => return,
    }
    invalidated.invalidate(UiDirtyRegion::Library);
}
