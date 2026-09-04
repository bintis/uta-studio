use super::*;
use crate::studio::*;

#[expect(
    clippy::too_many_arguments,
    reason = "this declarative player renderer receives the shared UI asset set"
)]
pub(crate) fn spawn_library_player(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSessionView<'_>,
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
    let audio_sources = library_audio_sources(&song);
    let selected_audio_source = audio_sources
        .iter()
        .find(|source| source.id == session.library_playback.audio_source_id)
        .unwrap_or(&audio_sources[0])
        .clone();
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
                library_visible_position(session.library_playback)
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
                                UiAction::from(LibraryCommand::ToggleLibraryShuffle),
                                session.library_playback.shuffle,
                                false,
                                30.0,
                            );
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Previous,
                                UiAction::from(LibraryCommand::PreviousLibrarySong),
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
                                    UiAction::from(LibraryCommand::SeekLibraryRelative(-10)),
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
                                    UiAction::from(LibraryCommand::ToggleLibraryPlayback)
                                } else {
                                    UiAction::from(LibraryCommand::PlayLibrarySong(
                                        song.file_hash.clone(),
                                    ))
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
                                    UiAction::from(LibraryCommand::SeekLibraryRelative(10)),
                                );
                            }
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Next,
                                UiAction::from(LibraryCommand::NextLibrarySong),
                                false,
                                false,
                                30.0,
                            );
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Repeat,
                                UiAction::from(LibraryCommand::CycleLibraryRepeat),
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
            player
                .spawn(Node {
                    position_type: PositionType::Relative,
                    width: px(270),
                    min_width: px(190),
                    flex_shrink: 1.0,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    row_gap: px(5),
                    ..default()
                })
                .with_children(|source| {
                    let mut quality = source.spawn((
                        Node {
                            width: percent(100),
                            min_height: px(22),
                            justify_content: JustifyContent::FlexEnd,
                            align_items: AlignItems::Center,
                            column_gap: px(5),
                            padding: UiRect::horizontal(px(4)),
                            border_radius: BorderRadius::all(px(4)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ));
                    if audio_sources.len() > 1 {
                        quality.insert((
                            Button,
                            UiAction::from(LibraryCommand::ToggleLibraryAudioSourceMenu),
                        ));
                    } else {
                        quality.insert(Pickable::IGNORE);
                    }
                    quality.with_children(|quality| {
                        spawn_text(
                            quality,
                            font.clone(),
                            selected_audio_source.label.to_ascii_uppercase(),
                            7.0,
                            theme.muted_foreground.with_alpha(0.72),
                        );
                        spawn_text(
                            quality,
                            font.clone(),
                            selected_audio_source.format.clone(),
                            9.0,
                            theme.muted_foreground,
                        );
                        if audio_sources.len() > 1 {
                            spawn_icon(
                                quality,
                                icons.clone(),
                                UiIcon::ChevronDown,
                                12.0,
                                theme.muted_foreground,
                            );
                        }
                    });
                    if session.library_playback.audio_source_menu_open && audio_sources.len() > 1 {
                        source
                            .spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    right: px(0),
                                    bottom: px(62),
                                    width: px(250),
                                    max_height: px(320),
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect::all(px(7)),
                                    row_gap: px(3),
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
                                    px(12),
                                    px(28),
                                    px(-8),
                                ),
                                ZIndex(86),
                            ))
                            .with_children(|menu| {
                                spawn_text(menu, font.clone(), "AUDIO SOURCE", 7.0, theme.primary);
                                for candidate in &audio_sources {
                                    let selected = candidate.id == selected_audio_source.id;
                                    menu.spawn((
                                        Button,
                                        UiAction::from(LibraryCommand::SelectLibraryAudioSource(
                                            candidate.id.clone(),
                                        )),
                                        Node {
                                            width: percent(100),
                                            min_height: px(34),
                                            align_items: AlignItems::Center,
                                            padding: UiRect::horizontal(px(9)),
                                            column_gap: px(8),
                                            border_radius: BorderRadius::all(px(5)),
                                            ..default()
                                        },
                                        BackgroundColor(if selected {
                                            theme.primary.with_alpha(0.12)
                                        } else {
                                            Color::NONE
                                        }),
                                    ))
                                    .with_children(|row| {
                                        spawn_text(
                                            row,
                                            font.clone(),
                                            if selected { "✓" } else { "" },
                                            8.0,
                                            theme.primary,
                                        );
                                        spawn_text(
                                            row,
                                            font.clone(),
                                            candidate.label.clone(),
                                            9.0,
                                            if selected {
                                                theme.foreground
                                            } else {
                                                theme.muted_foreground
                                            },
                                        );
                                        row.spawn(Node {
                                            flex_grow: 1.0,
                                            ..default()
                                        });
                                        spawn_text(
                                            row,
                                            font.clone(),
                                            candidate.format.clone(),
                                            8.0,
                                            theme.muted_foreground,
                                        );
                                    });
                                }
                            });
                    }
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
                                UiAction::from(LibraryCommand::ToggleLibraryMute),
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
                                UiAction::from(LibraryCommand::AdjustLibraryVolume(-5)),
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
                                UiAction::from(LibraryCommand::AdjustLibraryVolume(5)),
                            );
                            spawn_icon_button(
                                output,
                                icons.clone(),
                                theme,
                                UiIcon::Queue,
                                UiAction::from(LibraryCommand::ToggleLibraryQueue),
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
    session: &StudioSessionView<'_>,
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
                        UiAction::from(LibraryCommand::ToggleLibraryQueue),
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
                        UiAction::from(LibraryCommand::PlayLibrarySong(file_hash.clone())),
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
                                "Pick a local folder. Uta! Studio will scan it, generate stems and charts with AI, then let you correct every word and note before exporting.",
                            ),
                            ui_text_font(font.clone(), 13.0),
                            TextColor(theme.muted_foreground),
                            TextLayout::justify(Justify::Center),
                        )],
                    ));
                    card.spawn((
                        Button,
                        UiAction::from(LibraryCommand::ChooseFolder),
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
