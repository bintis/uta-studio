//! Library route: browsing, filters, song rows, player, and export.

use crate::studio::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibrarySelectKind {
    Status,
    TranscriptSource,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LibraryView {
    #[default]
    All,
    Queue,
    Completed,
    Videos,
    Artists,
    Albums,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LibraryFacet {
    Artist { value: String, label: String },
    Album { value: String, label: String },
    Playlist { value: String, label: String },
}

impl LibraryFacet {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Artist { label, .. }
            | Self::Album { label, .. }
            | Self::Playlist { label, .. } => label,
        }
    }
}

impl LibraryView {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::All => "Song Library",
            Self::Queue => "Analysis",
            Self::Completed => "Completed Charts",
            Self::Videos => "Video",
            Self::Artists => "Artists",
            Self::Albums => "Albums",
        }
    }

    pub(crate) fn eyebrow(self) -> &'static str {
        match self {
            Self::All => "ALL MUSIC",
            Self::Queue => "IN PROGRESS",
            Self::Completed => "READY TO AUTHOR",
            Self::Videos => "VIDEO SOURCES",
            Self::Artists | Self::Albums => "MY LIBRARY",
        }
    }

    pub(crate) fn filters(self) -> LibraryMenuFilters {
        LibraryMenuFilters {
            query: match self {
                Self::Queue => Some("queued".to_string()),
                Self::Completed => Some("analysed".to_string()),
                Self::Videos => Some("videos".to_string()),
                Self::All | Self::Artists | Self::Albums => None,
            },
            ..default()
        }
    }
}

#[derive(Clone)]
pub(crate) struct SongContextMenu {
    pub(crate) song: Song,
    pub(crate) position: Vec2,
}

pub(crate) struct LibraryPlayback {
    pub(crate) file_hash: Option<String>,
    pub(crate) visible_position: f64,
    pub(crate) status: uta_studio_audio::EditorAudioStatus,
    pub(crate) last_audio_sync: Instant,
    pub(crate) queue: Vec<String>,
    pub(crate) queue_index: Option<usize>,
    pub(crate) queue_open: bool,
    pub(crate) shuffle: bool,
    pub(crate) shuffle_seed: u64,
    pub(crate) repeat: LibraryRepeatMode,
    pub(crate) volume: f64,
    pub(crate) volume_before_mute: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LibraryRepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl LibraryRepeatMode {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat off",
            Self::All => "Repeat queue",
            Self::One => "Repeat one",
        }
    }
}

impl Default for LibraryPlayback {
    fn default() -> Self {
        Self {
            file_hash: None,
            visible_position: 0.0,
            status: uta_studio_audio::EditorAudioStatus::default(),
            last_audio_sync: Instant::now(),
            queue: Vec::new(),
            queue_index: None,
            queue_open: false,
            shuffle: false,
            shuffle_seed: 0x9e37_79b9_7f4a_7c15,
            repeat: LibraryRepeatMode::Off,
            volume: 0.8,
            volume_before_mute: 0.8,
        }
    }
}

pub(crate) fn load_songs(filters: LibraryMenuFilters) -> SongsStore {
    SongsStore::load(&LoadSongsParams {
        search: None,
        filters,
        skip: 0,
        take: 500,
    })
}

#[derive(Resource)]
pub(crate) struct NativeLibraryAudio(pub(crate) Arc<uta_studio_audio::EditorAudioPlayer>);

#[derive(Resource)]
pub(crate) struct LibraryRefreshTimer(pub(crate) Timer);

#[derive(Resource)]
pub(crate) struct LibraryAudioSyncTimer(pub(crate) Timer);

#[derive(Default)]
pub(crate) struct NativeExportJob {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<String>>>,
}

#[derive(Component)]
pub(crate) struct LibraryPlayerProgress;

#[derive(Component)]
pub(crate) struct LibraryPlayerClockText;

#[derive(Component)]
pub(crate) struct LibrarySongList;

#[derive(Component)]
pub(crate) struct LibrarySearchInput;

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_library_player(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let playback_song = session
        .library_playback
        .file_hash
        .as_deref()
        .and_then(|hash| app_core::load_song_by_hash(hash).ok().flatten());
    let Some(song) = playback_song.or_else(|| session.selected_song()) else {
        return;
    };
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(82),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(18)),
                column_gap: px(14),
                border: UiRect::top(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.72)),
            BorderColor::all(theme.border.with_alpha(0.52)),
        ))
        .with_children(|player| {
            let cover = album_art_handle(&song, asset_server, images, local_images);
            let current = session.library_playback.file_hash.as_deref()
                == Some(song.file_hash.as_str())
                && session.library_playback.status.loaded;
            let position = if current {
                library_visible_position(&session.library_playback)
            } else {
                0.0
            };
            let duration = if current && session.library_playback.status.duration_secs > 0.0 {
                session.library_playback.status.duration_secs
            } else {
                song.duration_secs
            };
            player
                .spawn(Node {
                    width: px(300),
                    min_width: px(180),
                    flex_shrink: 1.0,
                    align_items: AlignItems::Center,
                    column_gap: px(12),
                    ..default()
                })
                .with_children(|now_playing| {
                    now_playing.spawn((
                        Node {
                            width: px(52),
                            height: px(52),
                            flex_shrink: 0.0,
                            overflow: Overflow::clip(),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(4)),
                            ..default()
                        },
                        ImageNode::new(cover),
                        BorderColor::all(theme.border.with_alpha(0.58)),
                    ));
                    now_playing
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            overflow: Overflow::clip(),
                            ..default()
                        })
                        .with_children(|identity| {
                            identity.spawn((
                                Text::new(song.title.clone()),
                                ui_text_font(font.clone(), 11.0),
                                TextColor(theme.foreground),
                                TextLayout::no_wrap(),
                            ));
                            identity.spawn((
                                Text::new(if song.artist.trim().is_empty() {
                                    "Unknown artist".to_string()
                                } else {
                                    song.artist.clone()
                                }),
                                ui_text_font(font.clone(), 9.0),
                                TextColor(theme.muted_foreground),
                                TextLayout::no_wrap(),
                            ));
                        });
                });
            player
                .spawn(Node {
                    min_width: px(280),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    row_gap: px(6),
                    ..default()
                })
                .with_children(|transport| {
                    transport
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            column_gap: px(6),
                            ..default()
                        })
                        .with_children(|controls| {
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Shuffle,
                                UiAction::ToggleLibraryShuffle,
                                session.library_playback.shuffle,
                                false,
                                30.0,
                            );
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Previous,
                                UiAction::PreviousLibrarySong,
                                false,
                                false,
                                30.0,
                            );
                            if current {
                                spawn_text_button(
                                    controls,
                                    font.clone(),
                                    theme,
                                    "−10",
                                    9.0,
                                    UiAction::SeekLibraryRelative(-10),
                                );
                            }
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                if current && session.library_playback.status.playing {
                                    UiIcon::Pause
                                } else {
                                    UiIcon::Play
                                },
                                if current {
                                    UiAction::ToggleLibraryPlayback
                                } else {
                                    UiAction::PlayLibrarySong(song.file_hash.clone())
                                },
                                true,
                                false,
                                34.0,
                            );
                            if current {
                                spawn_text_button(
                                    controls,
                                    font.clone(),
                                    theme,
                                    "+10",
                                    9.0,
                                    UiAction::SeekLibraryRelative(10),
                                );
                            }
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Next,
                                UiAction::NextLibrarySong,
                                false,
                                false,
                                30.0,
                            );
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Repeat,
                                UiAction::CycleLibraryRepeat,
                                session.library_playback.repeat != LibraryRepeatMode::Off,
                                false,
                                30.0,
                            );
                            if session.library_playback.repeat == LibraryRepeatMode::One {
                                spawn_text(controls, font.clone(), "1", 7.0, theme.primary);
                            }
                        });
                    transport
                        .spawn(Node {
                            width: percent(100),
                            max_width: px(560),
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                            ..default()
                        })
                        .with_children(|timeline| {
                            timeline
                                .spawn((
                                    Node {
                                        min_width: px(100),
                                        height: px(3),
                                        flex_grow: 1.0,
                                        overflow: Overflow::clip(),
                                        border_radius: BorderRadius::MAX,
                                        ..default()
                                    },
                                    BackgroundColor(theme.muted.with_alpha(0.82)),
                                ))
                                .with_children(|track| {
                                    track.spawn((
                                        LibraryPlayerProgress,
                                        Node {
                                            width: percent(
                                                ((position / duration.max(0.001)) * 100.0)
                                                    .clamp(0.0, 100.0)
                                                    as f32,
                                            ),
                                            height: percent(100),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(theme.primary.with_alpha(0.88)),
                                    ));
                                });
                            timeline.spawn((
                                LibraryPlayerClockText,
                                Text::new(format_editor_clock(position, duration)),
                                ui_text_font(font.clone(), 8.0),
                                TextColor(theme.muted_foreground),
                                TextLayout::no_wrap(),
                            ));
                        });
                });
            let source_format = song
                .path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .map(str::to_ascii_uppercase)
                .unwrap_or_else(|| "AUDIO".to_string());
            player
                .spawn(Node {
                    width: px(270),
                    min_width: px(190),
                    flex_shrink: 1.0,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    row_gap: px(5),
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|source| {
                    source
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            align_items: AlignItems::Center,
                            column_gap: px(5),
                            ..default()
                        })
                        .with_children(|quality| {
                            spawn_text(
                                quality,
                                font.clone(),
                                "ORIGINAL",
                                7.0,
                                theme.muted_foreground.with_alpha(0.72),
                            );
                            spawn_text(
                                quality,
                                font.clone(),
                                source_format,
                                9.0,
                                theme.muted_foreground,
                            );
                        });
                    source
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            align_items: AlignItems::Center,
                            column_gap: px(2),
                            ..default()
                        })
                        .with_children(|output| {
                            spawn_icon_button(
                                output,
                                icons.clone(),
                                theme,
                                UiIcon::Volume,
                                UiAction::ToggleLibraryMute,
                                session.library_playback.volume == 0.0,
                                false,
                                28.0,
                            );
                            spawn_text_button(
                                output,
                                font.clone(),
                                theme,
                                "−",
                                10.0,
                                UiAction::AdjustLibraryVolume(-5),
                            );
                            spawn_text(
                                output,
                                font.clone(),
                                format!("{}%", (session.library_playback.volume * 100.0).round()),
                                8.0,
                                theme.muted_foreground,
                            );
                            spawn_text_button(
                                output,
                                font.clone(),
                                theme,
                                "+",
                                10.0,
                                UiAction::AdjustLibraryVolume(5),
                            );
                            spawn_icon_button(
                                output,
                                icons.clone(),
                                theme,
                                UiIcon::Queue,
                                UiAction::ToggleLibraryQueue,
                                session.library_playback.queue_open,
                                false,
                                30.0,
                            );
                        });
                });
        });

    if session.library_playback.queue_open {
        spawn_library_play_queue(parent, font, icons, session, theme);
    }
}

pub(crate) fn spawn_library_play_queue(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(14),
                bottom: px(90),
                width: px(390),
                max_height: px(390),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(12)),
                row_gap: px(7),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(7)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.88)),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.26),
                px(0),
                px(14),
                px(32),
                px(-10),
            ),
            ZIndex(85),
        ))
        .with_children(|queue| {
            queue
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|header| {
                    spawn_icon(header, icons.clone(), UiIcon::Queue, 15.0, theme.primary);
                    spawn_text(header, font.clone(), "Play queue", 14.0, theme.foreground);
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_text(
                        header,
                        font.clone(),
                        format!("{} tracks", session.library_playback.queue.len()),
                        8.0,
                        theme.muted_foreground,
                    );
                    spawn_icon_button(
                        header,
                        icons.clone(),
                        theme,
                        UiIcon::Close,
                        UiAction::ToggleLibraryQueue,
                        false,
                        false,
                        28.0,
                    );
                });
            if session.library_playback.queue.is_empty() {
                spawn_wrapped_text(
                    queue,
                    font,
                    "Play a song to create a queue from the current library view.",
                    10.0,
                    theme.muted_foreground,
                );
                return;
            }
            let current = session.library_playback.queue_index.unwrap_or(0);
            let first = current.saturating_sub(1);
            for (index, file_hash) in session
                .library_playback
                .queue
                .iter()
                .enumerate()
                .skip(first)
                .take(8)
            {
                let song = session
                    .songs
                    .processed
                    .iter()
                    .find(|song| song.file_hash == *file_hash)
                    .cloned()
                    .or_else(|| app_core::load_song_by_hash(file_hash).ok().flatten());
                let (title, artist) = song
                    .map(|song| (song.title, song.artist))
                    .unwrap_or_else(|| ("Unavailable track".to_string(), String::new()));
                let active = index == current;
                queue
                    .spawn((
                        Button,
                        UiAction::PlayLibrarySong(file_hash.clone()),
                        Node {
                            width: percent(100),
                            min_height: px(38),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(9)),
                            column_gap: px(9),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(if active {
                            theme.primary.with_alpha(0.1)
                        } else {
                            Color::NONE
                        }),
                    ))
                    .with_children(|row| {
                        spawn_text(
                            row,
                            font.clone(),
                            if active { "NOW" } else { "NEXT" },
                            7.0,
                            if active {
                                theme.primary
                            } else {
                                theme.muted_foreground.with_alpha(0.7)
                            },
                        );
                        row.spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::clip(),
                            ..default()
                        })
                        .with_children(|identity| {
                            identity.spawn((
                                Text::new(title),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.foreground),
                                TextLayout::no_wrap(),
                            ));
                            identity.spawn((
                                Text::new(artist),
                                ui_text_font(font.clone(), 8.0),
                                TextColor(theme.muted_foreground),
                                TextLayout::no_wrap(),
                            ));
                        });
                    });
            }
        });
}

pub(crate) fn spawn_empty_library(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    scanning: bool,
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            min_height: px(0),
            flex_grow: 1.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::all(px(20)),
            ..default()
        })
        .with_children(|stage| {
            stage
                .spawn((
                    Node {
                        width: percent(100),
                        max_width: px(720),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(px(40), px(48)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        row_gap: px(14),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.76)),
                    BorderColor::all(theme.border.with_alpha(0.55)),
                    BoxShadow::new(
                        Color::srgba(0.0, 0.0, 0.0, 0.12),
                        px(0),
                        px(18),
                        px(28),
                        px(-12),
                    ),
                ))
                .with_children(|card| {
                    card.spawn((
                        Node {
                            width: px(48),
                            height: px(48),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(px(8)),
                            ..default()
                        },
                        BackgroundColor(theme.primary.with_alpha(0.12)),
                        children![(
                            Text::new("♫"),
                            ui_text_font(font.clone(), 24.0),
                            TextColor(theme.primary),
                        )],
                    ));
                    spawn_text(card, font.clone(), "FIRST STEP", 10.0, theme.primary);
                    spawn_text(
                        card,
                        font.clone(),
                        "Choose your song library",
                        24.0,
                        theme.foreground,
                    );
                    card.spawn((
                        Node {
                            max_width: px(570),
                            ..default()
                        },
                        children![(
                            Text::new(
                                "Pick a local folder. Uta Studio will scan it, generate stems and charts with AI, then let you correct every word and note before exporting.",
                            ),
                            ui_text_font(font.clone(), 13.0),
                            TextColor(theme.muted_foreground),
                            TextLayout::justify(Justify::Center),
                        )],
                    ));
                    card.spawn((
                        Button,
                        UiAction::ChooseFolder,
                        Node {
                            height: px(42),
                            padding: UiRect::horizontal(px(18)),
                            margin: UiRect::top(px(8)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(px(8)),
                            ..default()
                        },
                        BackgroundColor(theme.primary),
                        children![(
                            Text::new(if scanning {
                                "Scanning song folder…"
                            } else {
                                "□  Choose song folder"
                            }),
                            ui_text_font(font, 13.0),
                            TextColor(theme.primary_foreground),
                        )],
                    ));
                });
        });
}

pub(crate) fn library_select_options(
    kind: LibrarySelectKind,
) -> &'static [(&'static str, &'static str)] {
    match kind {
        LibrarySelectKind::Status => &[
            ("all", "All statuses"),
            ("not_analyzed", "Not analyzed"),
            ("queued", "Queued"),
            ("analyzing", "Analyzing"),
            ("analyzed", "Analyzed"),
            ("failed", "Failed"),
        ],
        LibrarySelectKind::TranscriptSource => &[
            ("all", "All lyric types"),
            ("generated", "Generated"),
            ("lyrics", "AI aligned"),
            ("lrc", "LRC"),
            ("usdx", "UltraStar"),
        ],
    }
}

pub(crate) fn library_select_value(kind: LibrarySelectKind, session: &StudioSession) -> &str {
    match kind {
        LibrarySelectKind::Status => session.library_status.as_deref().unwrap_or("all"),
        LibrarySelectKind::TranscriptSource => session
            .library_transcript_source
            .as_deref()
            .unwrap_or("all"),
    }
}

pub(crate) fn library_select_label(kind: LibrarySelectKind, value: &str) -> &'static str {
    library_select_options(kind)
        .iter()
        .find_map(|(option, label)| (*option == value).then_some(*label))
        .unwrap_or("All")
}

pub(crate) fn spawn_library_filter_select(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    kind: LibrarySelectKind,
    session: &StudioSession,
) {
    let current = library_select_value(kind, session);
    let open = session.open_library_select == Some(kind);
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: px(132),
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
                    UiAction::OpenLibrarySelect(kind),
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
                    spawn_text(
                        button,
                        font.clone(),
                        library_select_label(kind, current),
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
                            left: px(0),
                            right: px(0),
                            top: px(38),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(4)),
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
                        for (value, label) in library_select_options(kind) {
                            let selected = *value == current;
                            menu.spawn((
                                Button,
                                UiAction::SelectLibraryValue(kind, (*value).to_string()),
                                Node {
                                    width: percent(100),
                                    min_height: px(29),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(px(8)),
                                    column_gap: px(6),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(if selected {
                                    theme.primary.with_alpha(0.12)
                                } else {
                                    Color::NONE
                                }),
                            ))
                            .with_children(|option| {
                                spawn_text(
                                    option,
                                    font.clone(),
                                    *label,
                                    9.0,
                                    if selected {
                                        theme.primary
                                    } else {
                                        theme.foreground
                                    },
                                );
                                option.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                if selected {
                                    spawn_icon(
                                        option,
                                        icons.clone(),
                                        UiIcon::Check,
                                        12.0,
                                        theme.primary,
                                    );
                                }
                            });
                        }
                    });
            }
        });
}

pub(crate) fn spawn_export_all_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    session: &StudioSession,
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
                    UiAction::ToggleExportAllMenu,
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
                                UiAction::ExportAllUtz,
                            ),
                            (
                                "UltraStar bundles",
                                "One .txt bundle per ready chart",
                                UiAction::ExportAllUltraStar,
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
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let artists = session.library_view == LibraryView::Artists;
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
                        UiAction::SetLibraryFacet(facet),
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

pub(crate) fn spawn_library(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSession,
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
                        padding: UiRect::axes(px(28), px(24)),
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
                                    UiAction::RescanLibrary,
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
                                    UiAction::AnalyzeAll,
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
                                    UiAction::SettingsTab(SettingsTab::Models),
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
                                UiAction::ToggleLibraryLayout,
                                false,
                            );
                        });
                });

            library
                .spawn((
                    LibrarySongList,
                    ScrollPosition(Vec2::new(0.0, session.library_scroll_offset)),
                    Node {
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: if session.config.song_list_view.as_deref() == Some("grid")
                        {
                            FlexDirection::Row
                        } else {
                            FlexDirection::Column
                        },
                        flex_wrap: if session.config.song_list_view.as_deref() == Some("grid") {
                            FlexWrap::Wrap
                        } else {
                            FlexWrap::NoWrap
                        },
                        align_content: AlignContent::FlexStart,
                        padding: if session.config.song_list_view.as_deref() == Some("grid") {
                            UiRect::all(px(22))
                        } else {
                            UiRect::ZERO
                        },
                        row_gap: px(14),
                        column_gap: px(14),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|list| {
                    let grid = session.config.song_list_view.as_deref() == Some("grid");
                    if session.library_view == LibraryView::Queue {
                        spawn_analysis_session_overview(list, font.clone(), session, theme);
                        spawn_analysis_history_list(list, font.clone(), session, theme);
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
                            UiAction::LoadMoreSongs,
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
        UiAction::DismissSongContext,
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
                UiAction::PlayLibrarySong(context.song.file_hash.clone()),
            );
            if context.song.editor_ready {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Edit chart",
                    11.0,
                    UiAction::OpenEditor(context.song.file_hash.clone()),
                );
            } else if !context.song.authoring_ready {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Analyze song",
                    11.0,
                    UiAction::AnalyzeSong(context.song.file_hash.clone()),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Open track page",
                11.0,
                UiAction::OpenSong(context.song.file_hash.clone()),
            );
            if context.song.authoring_ready {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Export Uta package (.utz)",
                    11.0,
                    UiAction::ExportUtz(context.song.file_hash.clone()),
                );
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Export UltraStar (.txt)",
                    11.0,
                    UiAction::ExportUltraStar(context.song.file_hash.clone()),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Open with default app",
                11.0,
                UiAction::OpenSource(context.song.path.clone()),
            );
            spawn_text_button(
                menu,
                font,
                theme,
                "Show in file manager",
                11.0,
                UiAction::RevealSource(context.song.path.clone()),
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
            move |mut event: On<Pointer<Click>>,
                  mut session: ResMut<StudioSession>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                open_song_from_click(&event, &context_song, &mut session, &mut invalidated);
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
            move |mut event: On<Pointer<Click>>,
                  mut session: ResMut<StudioSession>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                open_song_from_click(&event, &context_song, &mut session, &mut invalidated);
            },
        );
}

pub(crate) fn open_song_from_click(
    event: &Pointer<Click>,
    song: &Song,
    session: &mut StudioSession,
    invalidated: &mut UiInvalidated,
) {
    match event.button {
        PointerButton::Primary => {
            session.selected_song = Some(song.file_hash.clone());
            session.route = StudioRoute::SongDetail;
            session.song_context = None;
            session.notice = None;
        }
        PointerButton::Secondary => {
            session.song_context = Some(SongContextMenu {
                song: song.clone(),
                position: event.pointer_location.position,
            });
        }
        PointerButton::Middle => return,
    }
    invalidated.0 = true;
}

pub(crate) fn spawn_song_header(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(34),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(22)),
                ..default()
            },
            BackgroundColor(theme.muted.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                width: px(56),
                flex_shrink: 0.0,
                ..default()
            });
            spawn_text(row, font.clone(), "TRACK", 9.0, theme.muted_foreground);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            row.spawn(Node {
                width: px(150),
                ..default()
            })
            .with_children(|artist| {
                spawn_text(artist, font.clone(), "ARTIST", 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(180),
                ..default()
            })
            .with_children(|album| {
                spawn_text(album, font.clone(), "ALBUM", 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(64),
                ..default()
            })
            .with_children(|duration| {
                spawn_text(duration, font.clone(), "TIME", 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(150),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|status| {
                spawn_text(status, font, "STATUS", 9.0, theme.muted_foreground);
            });
        });
}

pub(crate) fn handle_library_search_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    inputs: Query<&EditableText, With<LibrarySearchInput>>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let command = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight);
    if command && keys.just_pressed(KeyCode::KeyK) && session.route != StudioRoute::Editor {
        session.search_open = true;
        session.activity_open = false;
        session.about_open = false;
        invalidated.0 = true;
        return;
    }
    if keys.just_pressed(KeyCode::Escape) && session.search_open {
        session.search_open = false;
        invalidated.0 = true;
        return;
    }
    if !session.search_open || !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(entity) = focus.get() else {
        return;
    };
    let Ok(input) = inputs.get(entity) else {
        return;
    };
    let value = input.value().to_string();
    let value = value.trim();
    session.library_search = (!value.is_empty()).then(|| value.to_string());
    session.route = StudioRoute::Library;
    session.library_view = LibraryView::All;
    session.library_facet = None;
    session.search_open = false;
    session.refresh_library();
    invalidated.0 = true;
}

pub(crate) fn start_export_job(
    file_hash: &str,
    extension: &'static str,
    export_directory: Option<PathBuf>,
    job: &mut NativeExportJob,
) -> String {
    if job.receiver.is_some() {
        return "An export is already in progress.".to_string();
    }
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = export_song(&file_hash, extension, export_directory.as_deref());
        let _ = sender.send(result);
    });
    job.receiver = Some(Mutex::new(receiver));
    format!(
        "Choose where to save the {} export…",
        if extension == "utz" {
            "UTZ"
        } else {
            "UltraStar"
        }
    )
}

pub(crate) fn start_export_all_job(
    extension: &'static str,
    export_directory: PathBuf,
    job: &mut NativeExportJob,
) -> String {
    if job.receiver.is_some() {
        return "An export is already in progress.".to_string();
    }
    if !export_directory.is_dir() {
        return format!(
            "The export folder is unavailable: {}. Choose it again in Settings > Storage.",
            export_directory.display()
        );
    }

    let songs = SongsStore::load_all()
        .processed
        .into_iter()
        .filter(|song| song.authoring_ready)
        .collect::<Vec<_>>();
    if songs.is_empty() {
        return "No chart is ready to export. Analyze or import a chart first.".to_string();
    }
    let total = songs.len();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = export_all_songs(&songs, extension, &export_directory);
        let _ = sender.send(result);
    });
    job.receiver = Some(Mutex::new(receiver));
    format!(
        "Exporting {total} ready chart{} as {}…",
        if total == 1 { "" } else { "s" },
        if extension == "utz" {
            "UTZ"
        } else {
            "UltraStar"
        }
    )
}

pub(crate) fn poll_export_job(
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = session.export_job.receiver.as_ref().and_then(|receiver| {
        receiver
            .lock()
            .ok()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some("Export worker exited unexpectedly.".to_string())
                }
            })
    });
    let Some(result) = result else {
        return;
    };
    session.export_job.receiver = None;
    session.notice = Some(result);
    invalidated.0 = true;
}

pub(crate) fn library_visible_position(playback: &LibraryPlayback) -> f64 {
    if playback.status.playing {
        (playback.status.position_secs + playback.last_audio_sync.elapsed().as_secs_f64()).min(
            playback
                .status
                .duration_secs
                .max(playback.status.position_secs),
        )
    } else {
        playback.visible_position
    }
}

pub(crate) fn play_library_song(
    audio: &uta_studio_audio::EditorAudioPlayer,
    file_hash: &str,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    let song = app_core::load_song_by_hash(file_hash)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Song not found: {file_hash}"))?;
    if !song.path.is_file() {
        return Err(format!(
            "Source audio is unavailable: {}",
            song.path.display()
        ));
    }
    audio.load_path(&song.path)?;
    audio.set_volume(playback.volume)?;
    let status = audio.play()?;
    if let Some(error) = status.error.as_ref() {
        return Err(format!("Could not play the original source: {error}"));
    }
    playback.file_hash = Some(file_hash.to_string());
    playback.queue_index = playback.queue.iter().position(|hash| hash == file_hash);
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

/// §7.6 "Play audio artifact": plays one artifact revision's file (a
/// vocal/instrumental stem at whichever revision the user picked) through
/// the same player `play_library_song` uses, but as a one-off preview
/// outside the library queue -- `playback.file_hash`/`queue`/`queue_index`
/// are cleared rather than repurposed, since this isn't "now playing this
/// song," it's "now previewing this artifact revision."
pub(crate) fn play_artifact_revision(
    audio: &uta_studio_audio::EditorAudioPlayer,
    path: &std::path::Path,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("Artifact file is unavailable: {}", path.display()));
    }
    audio.load_path(path)?;
    audio.set_volume(playback.volume)?;
    let status = audio.play()?;
    if let Some(error) = status.error.as_ref() {
        return Err(format!("Could not play this artifact: {error}"));
    }
    playback.file_hash = None;
    playback.queue.clear();
    playback.queue_index = None;
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

pub(crate) fn prepare_library_queue(
    songs: &[Song],
    file_hash: &str,
    playback: &mut LibraryPlayback,
) {
    playback.queue = songs
        .iter()
        .filter(|song| song.path.is_file())
        .map(|song| song.file_hash.clone())
        .collect();
    if !playback.queue.iter().any(|hash| hash == file_hash) {
        playback.queue.push(file_hash.to_string());
    }
    playback.queue_index = playback.queue.iter().position(|hash| hash == file_hash);
}

pub(crate) fn advance_library_queue(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
    direction: i8,
    wrap: bool,
) -> Result<(), String> {
    if playback.queue.is_empty() {
        return Err("The playback queue is empty.".to_string());
    }
    let current = playback
        .queue_index
        .or_else(|| {
            playback
                .file_hash
                .as_ref()
                .and_then(|hash| playback.queue.iter().position(|item| item == hash))
        })
        .unwrap_or(0);
    let len = playback.queue.len();
    let next = if playback.shuffle && len > 1 && direction > 0 {
        playback.shuffle_seed = playback
            .shuffle_seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let candidate = (playback.shuffle_seed as usize) % len;
        if candidate == current {
            (candidate + 1) % len
        } else {
            candidate
        }
    } else if direction < 0 {
        if current > 0 {
            current - 1
        } else if wrap {
            len - 1
        } else {
            return Err("This is the start of the queue.".to_string());
        }
    } else if current + 1 < len {
        current + 1
    } else if wrap {
        0
    } else {
        return Err("This is the end of the queue.".to_string());
    };
    let file_hash = playback.queue[next].clone();
    playback.queue_index = Some(next);
    play_library_song(audio, &file_hash, playback)
}

pub(crate) fn restart_library_song(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    audio.seek(0.0)?;
    let status = audio.play()?;
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

pub(crate) fn set_library_volume(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
    volume: f64,
) -> Result<(), String> {
    playback.volume = volume.clamp(0.0, 1.0);
    if playback.volume > 0.0 {
        playback.volume_before_mute = playback.volume;
    }
    if playback.status.loaded {
        let status = audio.set_volume(playback.volume)?;
        playback.visible_position = status.position_secs;
        playback.status = status;
        playback.last_audio_sync = Instant::now();
    }
    Ok(())
}

pub(crate) fn toggle_library_playback(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    if !playback.status.loaded {
        return Err("Choose a song before starting playback.".to_string());
    }
    let status = if playback.status.playing {
        audio.pause()?
    } else {
        if playback.status.ended {
            audio.seek(0.0)?;
        }
        audio.play()?
    };
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

pub(crate) fn seek_library_relative(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
    delta_secs: f64,
) -> Result<(), String> {
    if !playback.status.loaded {
        return Err("Choose a song before seeking.".to_string());
    }
    let was_playing = playback.status.playing;
    let target = (library_visible_position(playback) + delta_secs)
        .clamp(0.0, playback.status.duration_secs.max(0.0));
    let mut status = audio.seek(target)?;
    if was_playing {
        status = audio.play()?;
    }
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

pub(crate) fn handle_library_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut session: ResMut<StudioSession>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<LibrarySongList>>,
    graphs: Query<(&ComputedNode, &UiGlobalTransform), With<AnalysisGraphViewport>>,
) {
    if session.route != StudioRoute::Library {
        wheel.clear();
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if shift
        && let Ok(window) = windows.single()
        && let Some(pointer) = window.cursor_position()
        && graphs
            .iter()
            .any(|(computed, transform)| ui_node_contains_pointer(computed, transform, pointer))
    {
        wheel.clear();
        return;
    }
    let Ok((computed, mut position)) = lists.single_mut() else {
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
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
    session.library_scroll_offset = position.y;
}

pub(crate) fn sync_library_audio(
    time: Res<Time>,
    mut timer: ResMut<LibraryAudioSyncTimer>,
    audio: Res<NativeLibraryAudio>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if session.library_playback.file_hash.is_none() {
        return;
    }
    if timer.0.tick(time.delta()).just_finished() {
        let was_playing = session.library_playback.status.playing;
        let had_ended = session.library_playback.status.ended;
        match audio.0.status() {
            Ok(status) => {
                if let Some(error) = status.error.clone() {
                    session.notice = Some(format!("Library playback stopped: {error}"));
                    invalidated.0 = true;
                }
                session.library_playback.visible_position = status.position_secs;
                session.library_playback.status = status;
                session.library_playback.last_audio_sync = Instant::now();
                if session.library_playback.status.ended && !had_ended {
                    let repeat = session.library_playback.repeat;
                    let result = if repeat == LibraryRepeatMode::One {
                        restart_library_song(&audio.0, &mut session.library_playback)
                    } else {
                        advance_library_queue(
                            &audio.0,
                            &mut session.library_playback,
                            1,
                            repeat == LibraryRepeatMode::All,
                        )
                    };
                    if let Err(error) = result
                        && error != "This is the end of the queue."
                    {
                        session.notice = Some(error);
                    }
                    invalidated.0 = true;
                }
                if was_playing != session.library_playback.status.playing
                    || had_ended != session.library_playback.status.ended
                {
                    invalidated.0 = true;
                }
            }
            Err(error) => {
                session.notice = Some(error);
                invalidated.0 = true;
            }
        }
    } else if session.library_playback.status.playing {
        session.library_playback.visible_position =
            library_visible_position(&session.library_playback);
    }
}

pub(crate) fn update_library_player_ui(
    session: Res<StudioSession>,
    mut progress: Query<&mut Node, With<LibraryPlayerProgress>>,
    mut clocks: Query<&mut Text, (With<LibraryPlayerClockText>, Without<LibraryPlayerProgress>)>,
) {
    let playback = &session.library_playback;
    if !playback.status.loaded {
        return;
    }
    let position = library_visible_position(playback);
    let duration = playback.status.duration_secs.max(0.001);
    let width = ((position / duration) * 100.0).clamp(0.0, 100.0) as f32;
    for mut node in &mut progress {
        node.width = percent(width);
    }
    let label = format_editor_clock(position, playback.status.duration_secs);
    for mut text in &mut clocks {
        **text = label.clone();
    }
}

pub(crate) fn validate_source_path(
    path: &std::path::Path,
    config: &AppConfig,
) -> Result<PathBuf, String> {
    let requested = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let mut allowed_roots = config.library_paths();
    if let Some(export_path) = config.export_path.as_ref() {
        allowed_roots.push(export_path.clone());
    }
    let allowed = allowed_roots.iter().any(|root| {
        std::fs::canonicalize(root)
            .map(|root| requested.starts_with(root))
            .unwrap_or(false)
    });
    allowed
        .then_some(requested)
        .ok_or_else(|| "Path is outside configured library and output locations".to_string())
}

pub(crate) fn open_library_entry(path: &std::path::Path, config: &AppConfig) -> String {
    match validate_source_path(path, config) {
        Ok(path) => match open::that_detached(&path) {
            Ok(()) => format!("Opened {}", path.display()),
            Err(error) => format!("Could not open {}: {error}", path.display()),
        },
        Err(error) => format!("Could not open this library item: {error}"),
    }
}

pub(crate) fn reveal_library_entry(path: &std::path::Path, config: &AppConfig) -> String {
    match validate_source_path(path, config) {
        Ok(path) => {
            let target = if path.is_dir() {
                path.as_path()
            } else if let Some(parent) = path.parent() {
                parent
            } else {
                path.as_path()
            };
            match open::that_detached(target) {
                Ok(()) => format!("Revealed {}", path.display()),
                Err(error) => format!("Could not reveal {}: {error}", path.display()),
            }
        }
        Err(error) => format!("Could not reveal this library item: {error}"),
    }
}

/// Same authorization shape as `validate_source_path`, scoped to the app's
/// own generated-cache root instead of the user's configured library
/// folders -- artifact revisions live under the cache root, never a
/// library folder, so reusing `validate_source_path` would always reject
/// them.
pub(crate) fn validate_cache_path(
    path: &std::path::Path,
    cache_root: &std::path::Path,
) -> Result<PathBuf, String> {
    let requested = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let root = std::fs::canonicalize(cache_root).map_err(|error| error.to_string())?;
    requested
        .starts_with(&root)
        .then_some(requested)
        .ok_or_else(|| "This item is outside the app's cache directory".to_string())
}

/// §7.6/§6.3 "Open Artifact" -- the artifact-revision counterpart of
/// `open_library_entry`, scoped to the cache root instead of the user's
/// library folders via the same `validate_cache_path` check
/// `reveal_artifact_entry` already uses. Opens the artifact file itself
/// (whatever the OS's default handler for its extension is), not its
/// containing folder.
pub(crate) fn open_artifact_entry(path: &std::path::Path) -> String {
    let cache_root = app_core::CacheDir::new().path;
    match validate_cache_path(path, &cache_root) {
        Ok(path) => match open::that_detached(&path) {
            Ok(()) => format!("Opened {}", path.display()),
            Err(error) => format!("Could not open {}: {error}", path.display()),
        },
        Err(error) => format!("Could not open this artifact: {error}"),
    }
}

/// §7.6 "Preview": a bounded, in-app text preview for a JSON/text artifact
/// (transcript, pitch data, music analysis -- everything but the audio
/// stems, which already have "Play" for exactly this purpose). Reads at
/// most `PREVIEW_BYTE_LIMIT` bytes rather than the whole file -- some
/// artifacts (pitch tracks) can be large, and this is a quick look, not an
/// editor. Same `validate_cache_path` boundary as `open_artifact_entry`/
/// `reveal_artifact_entry`.
const PREVIEW_BYTE_LIMIT: usize = 4000;

pub(crate) fn preview_artifact_entry(path: &std::path::Path) -> String {
    let cache_root = app_core::CacheDir::new().path;
    let validated = match validate_cache_path(path, &cache_root) {
        Ok(path) => path,
        Err(error) => return format!("Could not preview this artifact: {error}"),
    };
    let bytes = match std::fs::read(&validated) {
        Ok(bytes) => bytes,
        Err(error) => return format!("Could not read {}: {error}", validated.display()),
    };
    format_artifact_preview(&validated, &bytes)
}

/// Testable core of `preview_artifact_entry`, separated from the real
/// `CacheDir`/filesystem read so the truncation/byte-count formatting can
/// be tested without a real cache root or on-disk fixture.
fn format_artifact_preview(path: &std::path::Path, bytes: &[u8]) -> String {
    let total_len = bytes.len();
    let truncated = total_len > PREVIEW_BYTE_LIMIT;
    let shown = &bytes[..total_len.min(PREVIEW_BYTE_LIMIT)];
    let text = String::from_utf8_lossy(shown);
    if truncated {
        format!(
            "{} ({total_len} bytes, showing first {PREVIEW_BYTE_LIMIT}):\n{text}…",
            path.display()
        )
    } else {
        format!("{} ({total_len} bytes):\n{text}", path.display())
    }
}

pub(crate) fn reveal_artifact_entry(path: &std::path::Path) -> String {
    let cache_root = app_core::CacheDir::new().path;
    match validate_cache_path(path, &cache_root) {
        Ok(path) => {
            let target = if path.is_dir() {
                path.as_path()
            } else if let Some(parent) = path.parent() {
                parent
            } else {
                path.as_path()
            };
            match open::that_detached(target) {
                Ok(()) => format!("Revealed {}", path.display()),
                Err(error) => format!("Could not reveal {}: {error}", path.display()),
            }
        }
        Err(error) => format!("Could not reveal this artifact: {error}"),
    }
}

pub(crate) fn export_song(
    file_hash: &str,
    extension: &str,
    export_directory: Option<&std::path::Path>,
) -> String {
    let song = match app_core::load_song_by_hash(file_hash) {
        Ok(Some(song)) => song,
        Ok(None) => return format!("Song not found: {file_hash}"),
        Err(error) => return error.to_string(),
    };
    let file_name = format!("{}.{}", safe_file_stem(&song.title), extension);
    let mut dialog = rfd::FileDialog::new().set_file_name(file_name);
    if let Some(path) = export_directory {
        dialog = dialog.set_directory(path);
    }
    dialog = if extension == "utz" {
        dialog.add_filter("Uta package", &["utz"])
    } else {
        dialog.add_filter("UltraStar chart", &["txt"])
    };
    let Some(output) = dialog.save_file() else {
        return "Export cancelled.".to_string();
    };
    let result = if extension == "utz" {
        app_core::export_utz(file_hash, &output)
    } else {
        app_core::export_ultrastar(file_hash, &output)
    };
    match result {
        Ok(path) => format!("Exported {}", path.display()),
        Err(error) => format!("Export failed: {error}"),
    }
}

pub(crate) fn export_all_songs(
    songs: &[Song],
    extension: &str,
    export_directory: &std::path::Path,
) -> String {
    let mut title_counts = HashMap::<String, usize>::new();
    for song in songs {
        *title_counts
            .entry(safe_file_stem(&song.title).to_lowercase())
            .or_default() += 1;
    }

    let mut used_stems = HashMap::<String, usize>::new();
    let mut exported = 0usize;
    let mut skipped = 0usize;
    let mut failures = Vec::new();
    for song in songs {
        let title = safe_file_stem(&song.title);
        let mut stem = if title_counts
            .get(&title.to_lowercase())
            .copied()
            .unwrap_or_default()
            > 1
        {
            format!("{} — {}", title, safe_file_stem(&song.artist))
        } else {
            title
        };
        let collision_key = stem.to_lowercase();
        let collision = used_stems.entry(collision_key).or_default();
        if *collision > 0 {
            let suffix = song.file_hash.chars().take(8).collect::<String>();
            stem = format!("{stem} — {suffix}");
        }
        *collision += 1;

        let output = export_directory.join(format!("{stem}.{extension}"));
        if output.exists() {
            skipped += 1;
            continue;
        }
        let result = match extension {
            "utz" => app_core::export_utz(&song.file_hash, &output),
            "txt" => app_core::export_ultrastar(&song.file_hash, &output),
            _ => unreachable!("batch export extensions are fixed by the UI"),
        };
        match result {
            Ok(_) => exported += 1,
            Err(error) => failures.push(format!("{}: {error}", song.title)),
        }
    }

    let mut summary = format!(
        "Export all finished · {exported} exported · {skipped} already existed · {} failed · {}",
        failures.len(),
        export_directory.display()
    );
    if !failures.is_empty() {
        summary.push_str(" · ");
        summary.push_str(
            &failures
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; "),
        );
        if failures.len() > 3 {
            summary.push_str(&format!("; and {} more", failures.len() - 3));
        }
    }
    summary
}

pub(crate) fn safe_file_stem(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => character,
        })
        .collect::<String>();
    let value = value.trim().trim_matches('.');
    if value.is_empty() {
        "Uta Studio Export".to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn refresh_library_while_scanning(
    time: Res<Time>,
    mut timer: ResMut<LibraryRefreshTimer>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !session.scanning || !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let previous = (
        session.meta.processed_count,
        session.meta.songs_count,
        session.meta.videos_count,
    );
    session.refresh_library();
    let current = (
        session.meta.processed_count,
        session.meta.songs_count,
        session.meta.videos_count,
    );
    if current != previous {
        invalidated.0 = true;
    }
    if session.meta.processed_count >= session.meta.count && session.meta.count > 0 {
        session.scanning = false;
        invalidated.0 = true;
    }
}

#[cfg(test)]
mod play_artifact_revision_tests {
    //! §7.6 "Play audio artifact". `play_artifact_revision` itself drives
    //! real playback hardware once past the existence check, which is out
    //! of scope for a unit test (see `native-audio/examples/
    //! playback_smoke_test.rs` for that level of verification) -- this just
    //! locks the one thing safe to assert without real audio output: a
    //! missing file is rejected before the player is ever touched.
    use super::{LibraryPlayback, play_artifact_revision};

    #[test]
    fn a_missing_artifact_file_is_rejected_without_touching_the_player() {
        let audio = uta_studio_audio::EditorAudioPlayer::new();
        let mut playback = LibraryPlayback::default();
        let missing =
            std::env::temp_dir().join("uta-studio-play-artifact-test-does-not-exist.flac");

        let result = play_artifact_revision(&audio, &missing, &mut playback);

        assert!(result.is_err());
        assert!(playback.file_hash.is_none());
    }
}

#[cfg(test)]
mod format_artifact_preview_tests {
    //! §7.6 "Preview".
    use super::{PREVIEW_BYTE_LIMIT, format_artifact_preview};
    use std::path::Path;

    #[test]
    fn a_short_file_is_shown_in_full_with_its_byte_count() {
        let copy = format_artifact_preview(Path::new("/cache/song_transcript.json"), b"{}");
        assert!(copy.contains("(2 bytes)"));
        assert!(copy.contains("{}"));
        assert!(!copy.contains("showing first"));
    }

    #[test]
    fn a_long_file_is_truncated_and_says_so() {
        let bytes = vec![b'x'; PREVIEW_BYTE_LIMIT + 500];
        let copy = format_artifact_preview(Path::new("/cache/song_pitch_track.json"), &bytes);
        assert!(copy.contains(&format!("({} bytes", PREVIEW_BYTE_LIMIT + 500)));
        assert!(copy.contains("showing first"));
        // The shown content itself must actually be truncated, not just the
        // label claiming it is -- count only the filler character, which
        // appears nowhere else in the surrounding label text.
        let shown_x_count = copy.matches('x').count();
        assert!(shown_x_count <= PREVIEW_BYTE_LIMIT);
        assert!(shown_x_count > 0);
    }
}

#[cfg(test)]
mod cache_path_tests {
    use super::validate_cache_path;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "uta-studio-cache-path-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn a_file_inside_the_cache_root_is_accepted() {
        let root = temp_dir("inside");
        let file = root.join("song_transcript.json");
        std::fs::write(&file, b"{}").unwrap();

        assert!(validate_cache_path(&file, &root).is_ok());
    }

    #[test]
    fn a_file_outside_the_cache_root_is_rejected() {
        let root = temp_dir("outside-root");
        let outsider_dir = temp_dir("outside-file");
        let file = outsider_dir.join("not_cache.json");
        std::fs::write(&file, b"{}").unwrap();

        assert!(validate_cache_path(&file, &root).is_err());
    }

    #[test]
    fn a_sibling_directory_that_shares_a_path_prefix_is_still_rejected() {
        // Regression guard for the classic `starts_with` string-prefix trap:
        // "/cache-evil" starts with the *string* "/cache" but is not really
        // inside it.
        let base = temp_dir("prefix-guard");
        let root = base.join("cache");
        let sibling = base.join("cache-evil");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let file = sibling.join("not_cache.json");
        std::fs::write(&file, b"{}").unwrap();

        assert!(validate_cache_path(&file, &root).is_err());
    }

    #[test]
    fn a_nonexistent_path_is_rejected_rather_than_panicking() {
        let root = temp_dir("nonexistent");
        let missing = root.join("does_not_exist.json");

        assert!(validate_cache_path(&missing, &root).is_err());
    }
}
