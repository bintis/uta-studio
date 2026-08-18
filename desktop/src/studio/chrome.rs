use crate::studio::*;

pub(crate) fn spawn_sidebar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    banner: Handle<Image>,
    session: &StudioSession,
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
                            UiAction::OpenAbout,
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
                                "Uta Studio",
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
                        UiAction::Settings,
                        false,
                        false,
                        30.0,
                    );
                });

            spawn_section_label(sidebar, font.clone(), theme, "BROWSE");
            let analysis_count = session
                .analysis_tasks
                .iter()
                .filter(|task| {
                    matches!(
                        task.status,
                        app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
                    )
                })
                .count();
            for (view, icon, label, count) in [
                (
                    LibraryView::All,
                    UiIcon::Home,
                    "All music",
                    session.meta.songs_count,
                ),
                (
                    LibraryView::Queue,
                    UiIcon::Queue,
                    "Analysis",
                    analysis_count,
                ),
                (
                    LibraryView::Completed,
                    UiIcon::CircleCheck,
                    "Completed Charts",
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
                    UiAction::SetLibraryView(view),
                    (session.route == StudioRoute::Library && session.library_view == view)
                        || (view == LibraryView::Queue
                            && session.route == StudioRoute::AnalysisInspect),
                );
            }
            spawn_section_label(sidebar, font.clone(), theme, "MY LIBRARY");
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Artists,
                "Artists",
                UiAction::SetLibraryView(LibraryView::Artists),
                session.route == StudioRoute::Library
                    && session.library_view == LibraryView::Artists
                    && session.library_facet.is_none(),
            );
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Albums,
                "Albums",
                UiAction::SetLibraryView(LibraryView::Albums),
                session.route == StudioRoute::Library
                    && session.library_view == LibraryView::Albums
                    && session.library_facet.is_none(),
            );
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
                        UiAction::SetLibraryFacet(facet.clone()),
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
                UiAction::Folders,
                session.route == StudioRoute::Folders,
            );
            sidebar.spawn(Node {
                min_height: px(14),
                flex_grow: 1.0,
                ..default()
            });
            sidebar.spawn((
                Button,
                UiAction::OpenAbout,
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
    action: UiAction,
    active: bool,
) {
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
            spawn_text(
                row,
                font,
                label,
                12.0,
                if active {
                    theme.sidebar_foreground.with_alpha(0.68)
                } else {
                    theme.sidebar_foreground
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
    session: &StudioSession,
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
            spawn_analysis_boundary_progress(workspace, session, theme);
            match session.route {
                StudioRoute::Library if session.config.library_paths().is_empty() => {
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
                    icons.clone(),
                    asset_server,
                    images,
                    local_images,
                    session,
                    theme,
                ),
                StudioRoute::Folders => {
                    spawn_folders(workspace, font.clone(), icons.clone(), session, theme)
                }
                StudioRoute::SongDetail => spawn_song_detail(
                    workspace,
                    font.clone(),
                    asset_server,
                    images,
                    local_images,
                    session,
                    theme,
                ),
                StudioRoute::Documentation => {
                    spawn_documentation(workspace, font.clone(), session, theme)
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

fn analysis_workspace_open(session: &StudioSession) -> bool {
    (session.route == StudioRoute::Library && session.library_view == LibraryView::Queue)
        || session.route == StudioRoute::AnalysisInspect
}

/// Hairline progress rail that replaces the top-bar / DAG divider on the
/// analysis pages. The page title no longer carries a second progress bar.
fn spawn_analysis_boundary_progress(
    parent: &mut ChildSpawnerCommands,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    if !analysis_workspace_open(session) {
        return;
    }
    let progress = current_analysis_header(session)
        .map(|(_, _, progress)| progress.clamp(0, 100))
        .unwrap_or(0);
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(3),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.border.with_alpha(0.55)),
        ))
        .with_children(|rail| {
            if progress > 0 {
                rail.spawn((
                    Node {
                        width: percent(progress as f32),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(theme.primary),
                ));
            }
        });
}

pub(crate) fn spawn_top_bar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(56),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect {
                    left: px(12),
                    right: px(12),
                    ..default()
                },
                border: if analysis_workspace_open(session) {
                    UiRect::ZERO
                } else {
                    UiRect::bottom(px(1))
                },
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.72)),
            BorderColor::all(theme.border.with_alpha(0.4)),
        ))
        .with_children(|bar| {
            spawn_icon_button(
                bar,
                icons.clone(),
                theme,
                UiIcon::ArrowLeft,
                UiAction::Back,
                false,
                false,
                34.0,
            );
            bar.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if let Some(title) = match session.route {
                StudioRoute::Folders => Some("Folders"),
                StudioRoute::SongDetail => Some("Song"),
                StudioRoute::Editor => Some("Editor"),
                StudioRoute::Documentation => Some("Documentation"),
                StudioRoute::AnalysisInspect => Some("Inspect view"),
                StudioRoute::Library | StudioRoute::Settings => None,
            } {
                spawn_text(bar, font.clone(), title, 11.0, theme.muted_foreground);
            }
            bar.spawn(Node {
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
                    UiAction::ToggleGlobalSearch,
                    session.search_open || session.library_search.is_some(),
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
                                            UiAction::ClearLibrarySearch,
                                        );
                                    }
                                    spawn_text_button(
                                        footer,
                                        font.clone(),
                                        theme,
                                        "Search",
                                        9.0,
                                        UiAction::ApplyLibrarySearch,
                                    );
                                });
                        });
                }
            });
            let has_active_analysis = session.analysis_tasks.iter().any(|task| {
                matches!(
                    task.status,
                    app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
                )
            });
            spawn_activity_button(
                bar,
                icons.clone(),
                theme,
                session.activity_open,
                has_active_analysis,
            );
        });
}

pub(crate) fn spawn_about_dialog(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    logo: Handle<Image>,
    theme: &StudioTheme,
) {
    parent.spawn((
        Button,
        UiAction::CloseAbout,
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
                            spawn_text(copy, font.clone(), "Uta Studio", 24.0, theme.foreground);
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
                        UiAction::CloseAbout,
                    );
                });
            spawn_text(
                dialog,
                font.clone(),
                format!("Version {}", env!("CARGO_PKG_VERSION")),
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
                "Lyrics data · LRCLIB",
                "Stem separation · UVR / Demucs",
                "Stem architecture · audio-separator (MIT)",
                "Speech recognition · WhisperX / OpenAI Whisper / NVIDIA Parakeet",
                "Forced alignment · WhisperX / torchaudio / Qwen3-ForcedAligner / FA-Kara (MIT)",
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_window_close_requests(
    mut requests: MessageReader<bevy::window::WindowCloseRequested>,
    mut commands: Commands,
    audio: Res<NativeAudio>,
    library_audio: Res<NativeLibraryAudio>,
    mut session: ResMut<StudioSession>,
    setup: Res<NativeSetup>,
    diagnostics: Res<NativeDiagnostics>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let Some(request) = requests.read().next() else {
        return;
    };
    let has_unsaved_edits = session.editor.as_ref().is_some_and(|editor| editor.dirty);
    let background_work = session.scanning
        || session.authoring_busy
        || setup.receiver.is_some()
        || diagnostics.receiver.is_some()
        || session.export_job.receiver.is_some()
        || session.editor_load_job.receiver.is_some()
        || session.lyrics_search_job.receiver.is_some()
        || session.analysis_tasks.iter().any(|task| {
            matches!(
                task.status,
                app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
            )
        });
    if has_unsaved_edits || background_work {
        session.pending_leave = Some(PendingLeave::Exit);
        invalidated.0 = true;
    } else {
        let _ = audio.0.stop();
        let _ = library_audio.0.stop();
        commands.entity(request.window).despawn();
    }
}
