//! Song detail route: overview, lyrics editor, and authoring jobs.

use crate::studio::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LyricsInputMode {
    Plain,
    TimedLrc,
}

pub(crate) struct NativeLyricsEditor {
    pub(crate) file_hash: String,
    pub(crate) mode: LyricsInputMode,
    pub(crate) separate_stems: bool,
    pub(crate) initial_text: String,
    pub(crate) candidates: Vec<app_core::LrclibCandidate>,
    pub(crate) candidate_index: usize,
    pub(crate) searching: bool,
}

pub(crate) struct NativeLanguageEditor {
    pub(crate) file_hash: String,
    pub(crate) initial_language: String,
    pub(crate) force_transcribe: bool,
    pub(crate) picker_open: bool,
}

pub(crate) const ANALYSIS_LANGUAGE_OPTIONS: &[(&str, &str)] = &[
    ("auto", "Automatic detection"),
    ("ja", "Japanese"),
    ("en", "English"),
    ("zh", "Chinese"),
    ("ko", "Korean"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("id", "Indonesian"),
    ("vi", "Vietnamese"),
    ("th", "Thai"),
    ("tr", "Turkish"),
    ("pl", "Polish"),
    ("uk", "Ukrainian"),
    ("nl", "Dutch"),
    ("sv", "Swedish"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
];

pub(crate) fn canonical_analysis_language(language: &str) -> String {
    match language
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .as_str()
    {
        "jp" | "jpn" => "ja".into(),
        "eng" => "en".into(),
        "kor" => "ko".into(),
        "chi" | "zho" | "cn" | "zh-cn" | "zh-tw" => "zh".into(),
        language
            if ANALYSIS_LANGUAGE_OPTIONS
                .iter()
                .any(|(code, _)| *code == language) =>
        {
            language.to_string()
        }
        _ => "auto".into(),
    }
}

pub(crate) fn analysis_language_label(language: &str) -> &'static str {
    ANALYSIS_LANGUAGE_OPTIONS
        .iter()
        .find_map(|(code, label)| (*code == language).then_some(*label))
        .unwrap_or("Automatic detection")
}

#[derive(Resource, Default)]
pub(crate) struct NativeAuthoringJob {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<AuthoringEvent>>>,
}

#[derive(Default)]
pub(crate) struct NativeLyricsSearchJob {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<Vec<app_core::LrclibCandidate>>>>,
}

pub(crate) struct AuthoringEvent {
    pub(crate) result: Result<app_core::ShiftResult, String>,
    pub(crate) kind: &'static str,
}

#[derive(Component)]
pub(crate) struct SongDetailContent;

#[derive(Component)]
pub(crate) struct LyricsEditorInput;

#[derive(Component)]
pub(crate) struct LanguageEditorInput;

pub(crate) fn lyrics_text(file_hash: &str, mode: LyricsInputMode) -> String {
    if mode == LyricsInputMode::Plain
        && let Some(file) = app_core::load_lyrics_file(file_hash)
    {
        return file.lines.join("\n");
    }
    let Ok(chart) = app_core::load_chart(file_hash) else {
        return String::new();
    };
    let document = app_core::EditorDocument::new(chart.vocal_chart);
    (0..document.phrase_count())
        .filter_map(|phrase| {
            let text = document.phrase_text(phrase);
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            if mode == LyricsInputMode::TimedLrc {
                let start = document
                    .lyric(app_core::LyricAddress {
                        segment: phrase,
                        word: 0,
                    })
                    .map(|(_, start, _)| start)
                    .unwrap_or(0.0);
                Some(format!("[{}]{text}", format_lrc_timestamp(start)))
            } else {
                Some(text.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn format_lrc_timestamp(seconds: f64) -> String {
    let centiseconds = (seconds.max(0.0) * 100.0).round() as u64;
    format!(
        "{:02}:{:02}.{:02}",
        centiseconds / 6000,
        centiseconds / 100 % 60,
        centiseconds % 100
    )
}

pub(crate) fn spawn_song_detail(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let Some(song) = session.selected_song() else {
        parent
            .spawn(Node {
                min_height: px(0),
                flex_grow: 1.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                ..default()
            })
            .with_children(|empty| {
                spawn_text(
                    empty,
                    font.clone(),
                    "Choose a song first",
                    22.0,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    empty,
                    font.clone(),
                    "Open a track from the library to see its production page.",
                    11.0,
                    theme.muted_foreground,
                );
                spawn_action_button(empty, font, theme, "Back to library", UiAction::Home);
            });
        return;
    };

    let cover = album_art_handle(&song, asset_server, images, local_images);
    parent
        .spawn((
            SongDetailContent,
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
        .with_children(|detail| {
            detail
                .spawn((
                    Node {
                        width: percent(100),
                        min_height: px(120),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(24),
                        row_gap: px(10),
                        padding: UiRect::axes(px(32), px(14)),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.background),
                    BorderColor::all(theme.border.with_alpha(0.45)),
                ))
                .with_children(|header| {
                    header
                    .spawn(Node {
                        min_width: px(320),
                        flex_grow: 1.0,
                        align_items: AlignItems::Center,
                        column_gap: px(18),
                        ..default()
                    })
                    .with_children(|identity| {
                        identity.spawn((
                            Node {
                                width: px(92),
                                height: px(92),
                                flex_shrink: 0.0,
                                overflow: Overflow::clip(),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(6)),
                                ..default()
                            },
                            ImageNode::new(cover),
                            BorderColor::all(theme.border.with_alpha(0.9)),
                        ));
                        identity
                            .spawn(Node {
                                min_width: px(0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Center,
                                ..default()
                            })
                            .with_children(|copy| {
                                spawn_wrapped_text(
                                    copy,
                                    font.clone(),
                                    song.title.clone(),
                                    28.0,
                                    theme.foreground,
                                );
                                spawn_text(
                                    copy,
                                    font.clone(),
                                    format!(
                                        "{}{}",
                                        song.artist,
                                        if song.album.is_empty() {
                                            String::new()
                                        } else {
                                            format!(" · {}", song.album)
                                        }
                                    ),
                                    12.0,
                                    theme.muted_foreground,
                                );
                                copy.spawn(Node {
                                    align_items: AlignItems::Center,
                                    flex_wrap: FlexWrap::Wrap,
                                    column_gap: px(14),
                                    row_gap: px(3),
                                    margin: UiRect::top(px(5)),
                                    ..default()
                                })
                                .with_children(|metadata| {
                                    spawn_text(metadata, font.clone(), format_duration(song.duration_secs), 9.0, theme.muted_foreground);
                                    spawn_text(metadata, font.clone(), song.language.as_deref().unwrap_or("Language unknown"), 9.0, theme.muted_foreground);
                                    if let Some(key) = song.override_key.as_ref().or(song.key.as_ref()) {
                                        spawn_text(metadata, font.clone(), format!("Key {key}"), 9.0, theme.muted_foreground);
                                    }
                                    spawn_text(metadata, font.clone(), format!("{:.1}× tempo", song.tempo), 9.0, theme.muted_foreground);
                                });
                            });
                    });
                    header
                        .spawn(Node {
                            min_width: px(0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::FlexEnd,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(8),
                            row_gap: px(6),
                            ..default()
                        })
                        .with_children(|actions| {
                            let current = session.library_playback.file_hash.as_deref()
                                == Some(song.file_hash.as_str())
                                && session.library_playback.status.loaded;
                            spawn_compact_action_button(
                                actions,
                                font.clone(),
                                theme,
                                if current && session.library_playback.status.playing {
                                    "Pause"
                                } else if current {
                                    "Resume"
                                } else {
                                    "Play original"
                                },
                                if current {
                                    UiAction::ToggleLibraryPlayback
                                } else {
                                    UiAction::PlayLibrarySong(song.file_hash.clone())
                                },
                            );
                            spawn_song_primary_actions(actions, font.clone(), &song, session, theme);
                        });
                });

            detail
                .spawn(Node {
                    width: percent(100),
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(32), px(14)),
                    row_gap: px(16),
                    ..default()
                })
                .with_children(|body| {
                    body.spawn(Node {
                        width: percent(100),
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(18),
                        row_gap: px(18),
                        ..default()
                    })
                    .with_children(|columns| {
                        columns
                            .spawn((
                                Node {
                                    min_width: px(540),
                                    flex_basis: px(620),
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(6)),
                                    ..default()
                                },
                                BackgroundColor(theme.card.with_alpha(0.32)),
                                BorderColor::all(theme.border.with_alpha(0.55)),
                            ))
                            .with_children(|production| {
                                spawn_detail_heading(
                                    production,
                                    font.clone(),
                                    theme,
                                    "AUTHORING",
                                    "Production controls",
                                );
                                if song.is_analyzed
                                    && !matches!(
                                        song.transcript_source,
                                        Some(app_core::TranscriptSource::Usdx)
                                    )
                                {
                                    spawn_shift_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Key",
                                        song.key
                                            .as_ref()
                                            .map(|key| format!("Original key: {key}"))
                                            .unwrap_or_else(|| {
                                                "Analyze again to detect the key.".to_string()
                                            }),
                                        song.override_key
                                            .as_ref()
                                            .or(song.key.as_ref())
                                            .cloned()
                                            .unwrap_or_else(|| "—".to_string()),
                                        UiAction::ShiftSongKey(song.file_hash.clone(), -1),
                                        UiAction::ShiftSongKey(song.file_hash.clone(), 1),
                                    );
                                    spawn_shift_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Tempo",
                                        "Create an export-speed variant in 0.1× steps.",
                                        format!("{:.1}×", song.tempo),
                                        UiAction::ShiftSongTempo(song.file_hash.clone(), -1),
                                        UiAction::ShiftSongTempo(song.file_hash.clone(), 1),
                                    );
                                } else {
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Key & tempo",
                                        "Controls become available after compatible analysis.",
                                        None::<(&'static str, UiAction)>,
                                    );
                                }
                                spawn_setting_row(
                                    production,
                                    font.clone(),
                                    theme,
                                    "Lyrics",
                                    "Paste plain lyrics to realign, or provide timed LRC without replacing source media.",
                                    if matches!(
                                        song.transcript_source,
                                        Some(app_core::TranscriptSource::Usdx)
                                    ) {
                                        None
                                    } else {
                                        Some((
                                            "Edit lyrics…".to_string(),
                                            UiAction::OpenLyricsEditor(song.file_hash.clone()),
                                        ))
                                    },
                                );
                                if !matches!(
                                    song.transcript_source,
                                    Some(app_core::TranscriptSource::Usdx)
                                ) {
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Language",
                                        format!(
                                            "Current analysis language: {}. Choose whether to realign current lyrics or transcribe again.",
                                            song.language.as_deref().unwrap_or("automatic")
                                        ),
                                        Some((
                                            "Change language…",
                                            UiAction::OpenLanguageEditor(song.file_hash.clone()),
                                        )),
                                    );
                                }
                                spawn_setting_row(
                                    production,
                                    font.clone(),
                                    theme,
                                    "Analysis defaults",
                                    "Tune separator, transcription, alignment, pitch, batching, and sensitivity. Existing chart data changes only after re-analysis.",
                                    Some(("Open analysis settings", UiAction::SettingsTab(SettingsTab::Analysis))),
                                );
                                if song.is_analyzed
                                    && !matches!(
                                        song.transcript_source,
                                        Some(app_core::TranscriptSource::Usdx)
                                    )
                                {
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Word timing",
                                        "Rebuild timings from current lyrics using the selected alignment backend.",
                                        Some((
                                            "Realign",
                                            UiAction::RealignSong(song.file_hash.clone()),
                                        )),
                                    );
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Lyrics source",
                                        "Refetch lyrics and align, or force a fresh transcription from the vocals.",
                                        Some((
                                            "Refetch & align",
                                            UiAction::ReanalyzeTranscript(song.file_hash.clone()),
                                        )),
                                    );
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Transcription",
                                        "Ignore online lyrics and transcribe the vocals again.",
                                        Some((
                                            "Force transcribe",
                                            UiAction::ForceTranscribe(song.file_hash.clone()),
                                        )),
                                    );
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Frequency analysis",
                                        "Generate or repair the editable pitch guide.",
                                        Some((
                                            "Analyze pitch",
                                            UiAction::ReanalyzePitch(song.file_hash.clone()),
                                        )),
                                    );
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Full reanalysis",
                                        "Recreate stems, lyrics, timing, key, tempo, and pitch assets.",
                                        Some((
                                            "Reanalyze all",
                                            UiAction::ReanalyzeFull(song.file_hash.clone()),
                                        )),
                                    );
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Generated song data",
                                        "Delete generated cache for this song. Source media is never changed.",
                                        Some((
                                            "Delete cache…",
                                            UiAction::RequestDeleteSongCache(song.file_hash.clone()),
                                        )),
                                    );
                                }
                            });

                        columns
                            .spawn((
                                Node {
                                    width: px(360),
                                    min_width: px(360),
                                    flex_grow: 1.0,
                                    flex_shrink: 0.0,
                                    flex_direction: FlexDirection::Column,
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(6)),
                                    ..default()
                                },
                                BackgroundColor(theme.card.with_alpha(0.28)),
                                BorderColor::all(theme.border.with_alpha(0.55)),
                            ))
                            .with_children(|overview| {
                                spawn_detail_heading(
                                    overview,
                                    font.clone(),
                                    theme,
                                    "PRODUCTION OVERVIEW",
                                    "Track information",
                                );
                                for (label, value) in song_overview_rows(&song) {
                                    spawn_detail_value(
                                        overview,
                                        font.clone(),
                                        theme,
                                        label,
                                        value,
                                    );
                                }
                                spawn_source_file_row(
                                    overview,
                                    font.clone(),
                                    theme,
                                    &song.path,
                                );
                            });
                    });

                    if let Some(notice) = session.notice.as_deref() {
                        spawn_wrapped_text(
                            body,
                            font.clone(),
                            notice,
                            10.0,
                            theme.muted_foreground,
                        );
                    }
                });
            if let Some(editor) = session.lyrics_editor.as_ref() {
                spawn_lyrics_editor(
                    detail,
                    font.clone(),
                    theme,
                    editor,
                    session.notice.as_deref(),
                );
            }
            if let Some(file_hash) = session.pending_cache_delete.as_deref() {
                spawn_cache_delete_confirmation(detail, font.clone(), theme, file_hash);
            }
            if let Some(editor) = session.language_editor.as_ref() {
                spawn_language_editor(
                    detail,
                    font.clone(),
                    theme,
                    editor,
                    session.notice.as_deref(),
                );
            }
        });
}

pub(crate) fn spawn_lyrics_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeLyricsEditor,
    notice: Option<&str>,
) {
    parent
        .spawn((
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
            ZIndex(80),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: percent(72),
                        max_width: px(760),
                        height: percent(78),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(22)),
                        row_gap: px(10),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(dialog, font.clone(), "EDIT LYRICS", 8.0, theme.primary);
                    spawn_text(
                        dialog,
                        font.clone(),
                        if editor.mode == LyricsInputMode::TimedLrc {
                            "Timed LRC"
                        } else {
                            "Plain lyrics"
                        },
                        18.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        if editor.mode == LyricsInputMode::TimedLrc {
                            "Paste line-level or enhanced LRC. Existing analyzed songs keep their stems; new songs can author over the original mix or explicitly queue separation."
                        } else if app_core::AppConfig::load().align_backend() == "mms_karaoke" {
                            "Enter one lyric phrase per line. MMS Karaoke accepts optional pronunciation overrides such as {漢字|かな} or [display|romaji]. Saving queues alignment and never modifies the source song."
                        } else {
                            "Enter one lyric phrase per line. Saving queues alignment and never modifies the source song."
                        },
                        10.0,
                        theme.muted_foreground,
                    );
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(8),
                            row_gap: px(8),
                            ..default()
                        })
                        .with_children(|options| {
                            spawn_text_button(
                                options,
                                font.clone(),
                                theme,
                                if editor.mode == LyricsInputMode::TimedLrc {
                                    "Use plain lyrics"
                                } else {
                                    "Use timed LRC"
                                },
                                10.0,
                                UiAction::ToggleLyricsInputMode,
                            );
                            if editor.mode == LyricsInputMode::TimedLrc {
                                spawn_text_button(
                                    options,
                                    font.clone(),
                                    theme,
                                    if editor.separate_stems {
                                        "Separate stems: on"
                                    } else {
                                        "Author on original mix"
                                    },
                                    10.0,
                                    UiAction::ToggleLyricsSeparateStems,
                                );
                            }
                            spawn_text_button(
                                options,
                                font.clone(),
                                theme,
                                if editor.searching {
                                    "Searching LRCLIB…"
                                } else if editor.candidates.is_empty() {
                                    "Find on LRCLIB"
                                } else {
                                    "Search LRCLIB again"
                                },
                                10.0,
                                UiAction::SearchLrclibLyrics,
                            );
                        });
                    if let Some(candidate) = editor.candidates.get(editor.candidate_index) {
                        dialog
                            .spawn((
                                Node {
                                    width: percent(100),
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect::all(px(11)),
                                    row_gap: px(6),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(6)),
                                    ..default()
                                },
                                BackgroundColor(theme.primary.with_alpha(0.06)),
                                BorderColor::all(theme.primary.with_alpha(0.32)),
                            ))
                            .with_children(|match_card| {
                                match_card
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
                                            format!(
                                                "LRCLIB MATCH  {} / {}",
                                                editor.candidate_index + 1,
                                                editor.candidates.len()
                                            ),
                                            8.0,
                                            theme.primary,
                                        );
                                        header.spawn(Node {
                                            flex_grow: 1.0,
                                            ..default()
                                        });
                                        if editor.candidates.len() > 1 {
                                            spawn_text_button(
                                                header,
                                                font.clone(),
                                                theme,
                                                "Previous",
                                                9.0,
                                                UiAction::PreviousLrclibCandidate,
                                            );
                                            spawn_text_button(
                                                header,
                                                font.clone(),
                                                theme,
                                                "Next",
                                                9.0,
                                                UiAction::NextLrclibCandidate,
                                            );
                                        }
                                    });
                                spawn_text(
                                    match_card,
                                    font.clone(),
                                    candidate.track_name.clone(),
                                    11.0,
                                    theme.foreground,
                                );
                                spawn_wrapped_text(
                                    match_card,
                                    font.clone(),
                                    format!(
                                        "{}{} · {} lines · {}",
                                        candidate.artist_name,
                                        if candidate.album_name.trim().is_empty() {
                                            String::new()
                                        } else {
                                            format!(" · {}", candidate.album_name)
                                        },
                                        candidate.lines.len(),
                                        format_duration(candidate.duration_secs)
                                    ),
                                    9.0,
                                    theme.muted_foreground,
                                );
                                match_card
                                    .spawn(Node {
                                        width: percent(100),
                                        align_items: AlignItems::Center,
                                        column_gap: px(8),
                                        ..default()
                                    })
                                    .with_children(|actions| {
                                        if candidate.synced_lyrics.is_some() {
                                            spawn_action_button(
                                                actions,
                                                font.clone(),
                                                theme,
                                                "Use timed LRC",
                                                UiAction::UseLrclibTimed,
                                            );
                                        }
                                        if !candidate.lines.is_empty() {
                                            spawn_text_button(
                                                actions,
                                                font.clone(),
                                                theme,
                                                "Use as plain lyrics",
                                                9.0,
                                                UiAction::UseLrclibPlain,
                                            );
                                        }
                                    });
                            });
                    }
                    dialog.spawn((
                        LyricsEditorInput,
                        EditableText {
                            visible_lines: Some(16.0),
                            visible_width: Some(72.0),
                            allow_newlines: true,
                            max_characters: Some(100_000),
                            ..EditableText::new(&editor.initial_text)
                        },
                        Node {
                            width: percent(100),
                            min_height: px(0),
                            flex_grow: 1.0,
                            padding: UiRect::all(px(10)),
                            overflow: Overflow::scroll(),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        ui_text_font(font.clone(), 11.0),
                        TextColor(theme.foreground),
                        TextLayout {
                            linebreak: bevy::text::LineBreak::WordOrCharacter,
                            ..default()
                        },
                        TextCursorStyle {
                            color: theme.primary,
                            selected_text_color: Some(theme.primary_foreground),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.65)),
                        BorderColor::all(theme.border.with_alpha(0.72)),
                        TabIndex(0),
                        AutoFocus,
                    ));
                    if let Some(notice) = notice {
                        spawn_wrapped_text(
                            dialog,
                            font.clone(),
                            notice,
                            9.0,
                            theme.destructive,
                        );
                    }
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|actions| {
                            spawn_text_button(
                                actions,
                                font.clone(),
                                theme,
                                "Cancel",
                                10.0,
                                UiAction::CloseLyricsEditor,
                            );
                            spawn_action_button(
                                actions,
                                font,
                                theme,
                                "Save lyrics",
                                UiAction::SaveLyricsEditor,
                            );
                        });
                });
        });
}

pub(crate) fn spawn_cache_delete_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    file_hash: &str,
) {
    let title = app_core::load_song_by_hash(file_hash)
        .ok()
        .flatten()
        .map(|song| song.title)
        .unwrap_or_else(|| "this song".to_string());
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
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(90),
        children![(
            Node {
                width: px(460),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(11),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new("Delete generated song data?"),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "Generated stems, transcripts, pitch data, and derived variants for “{title}” will be removed. The source song remains untouched."
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
                            UiAction::CancelDeleteSongCache,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
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
                            UiAction::ConfirmDeleteSongCache,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Delete generated data"),
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

pub(crate) fn spawn_language_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeLanguageEditor,
    notice: Option<&str>,
) {
    parent
        .spawn((
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
            ZIndex(92),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(470),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(11),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(dialog, font.clone(), "LANGUAGE", 8.0, theme.primary);
                    spawn_text(
                        dialog,
                        font.clone(),
                        "Change analysis language",
                        17.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        "Choose a supported language or let the analyzer detect it. The chosen action runs only after saving.",
                        10.0,
                        theme.muted_foreground,
                    );
                    dialog
                        .spawn((
                            Button,
                            UiAction::ToggleLanguagePicker,
                            Node {
                                width: percent(100),
                                height: px(40),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(11)),
                                column_gap: px(8),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.65)),
                            BorderColor::all(if editor.picker_open {
                                theme.primary.with_alpha(0.64)
                            } else {
                                theme.border.with_alpha(0.72)
                            }),
                        ))
                        .with_children(|selector| {
                            spawn_text(
                                selector,
                                font.clone(),
                                analysis_language_label(&editor.initial_language),
                                11.0,
                                theme.foreground,
                            );
                            selector.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_text(
                                selector,
                                font.clone(),
                                editor.initial_language.to_ascii_uppercase(),
                                8.0,
                                theme.muted_foreground,
                            );
                            spawn_text(
                                selector,
                                font.clone(),
                                if editor.picker_open { "^" } else { "v" },
                                9.0,
                                theme.primary,
                            );
                        });
                    if editor.picker_open {
                        dialog
                            .spawn((
                                ScrollPosition::default(),
                                Node {
                                    width: percent(100),
                                    max_height: px(238),
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect::all(px(5)),
                                    row_gap: px(2),
                                    overflow: Overflow::scroll_y(),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(5)),
                                    ..default()
                                },
                                BackgroundColor(theme.background.with_alpha(0.82)),
                                BorderColor::all(theme.border.with_alpha(0.72)),
                            ))
                            .with_children(|options| {
                                for (code, label) in ANALYSIS_LANGUAGE_OPTIONS {
                                    let selected = editor.initial_language == *code;
                                    options
                                        .spawn((
                                            Button,
                                            UiAction::SelectAnalysisLanguage((*code).into()),
                                            Node {
                                                width: percent(100),
                                                min_height: px(30),
                                                align_items: AlignItems::Center,
                                                padding: UiRect::horizontal(px(9)),
                                                column_gap: px(8),
                                                border_radius: BorderRadius::all(px(4)),
                                                ..default()
                                            },
                                            BackgroundColor(if selected {
                                                theme.primary.with_alpha(0.13)
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
                                                    theme.foreground
                                                } else {
                                                    theme.muted_foreground
                                                },
                                            );
                                            option.spawn(Node {
                                                flex_grow: 1.0,
                                                ..default()
                                            });
                                            spawn_text(
                                                option,
                                                font.clone(),
                                                code.to_ascii_uppercase(),
                                                8.0,
                                                if selected {
                                                    theme.primary
                                                } else {
                                                    theme.muted_foreground
                                                },
                                            );
                                        });
                                }
                            });
                    }
                    spawn_text_button(
                        dialog,
                        font.clone(),
                        theme,
                        if editor.force_transcribe {
                            "Action: transcribe vocals again"
                        } else {
                            "Action: realign current lyrics"
                        },
                        10.0,
                        UiAction::ToggleLanguageReprocess,
                    );
                    if let Some(notice) = notice {
                        spawn_wrapped_text(
                            dialog,
                            font.clone(),
                            notice,
                            9.0,
                            theme.destructive,
                        );
                    }
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|actions| {
                            spawn_text_button(
                                actions,
                                font.clone(),
                                theme,
                                "Cancel",
                                10.0,
                                UiAction::CloseLanguageEditor,
                            );
                            spawn_action_button(
                                actions,
                                font,
                                theme,
                                "Save & reprocess",
                                UiAction::SaveLanguageEditor,
                            );
                        });
                });
        });
}

pub(crate) fn spawn_song_primary_actions(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    song: &Song,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    if let Some(task) = session
        .analysis_tasks
        .iter()
        .find(|task| task.file_hash == song.file_hash)
        && matches!(
            task.status,
            app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
        )
    {
        let label = match task.status {
            app_core::QueuedStatus::Queued => "Queued for analysis".to_string(),
            app_core::QueuedStatus::Analyzing(progress) => {
                format!("Analyzing · {progress}%")
            }
            app_core::QueuedStatus::Failed(_) => unreachable!(),
        };
        spawn_action_button(parent, font, theme, label, UiAction::ToggleActivity);
        return;
    }

    if !song.is_analyzed {
        if app_core::analysis_runtime_status().ready {
            spawn_action_button(
                parent,
                font,
                theme,
                "Analyze song",
                UiAction::AnalyzeSong(song.file_hash.clone()),
            );
        } else {
            spawn_action_button(
                parent,
                font,
                theme,
                "Set up analysis",
                UiAction::SettingsTab(SettingsTab::Models),
            );
        }
        return;
    }

    if song.authoring_ready {
        spawn_action_button(
            parent,
            font.clone(),
            theme,
            "Export UTZ",
            UiAction::ExportUtz(song.file_hash.clone()),
        );
        spawn_action_button(
            parent,
            font.clone(),
            theme,
            "Export UltraStar",
            UiAction::ExportUltraStar(song.file_hash.clone()),
        );
    }
    spawn_action_button(
        parent,
        font,
        theme,
        if song.editor_ready {
            "Edit chart"
        } else {
            "Prepare & edit"
        },
        UiAction::OpenEditor(song.file_hash.clone()),
    );
}

pub(crate) fn spawn_detail_heading(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    eyebrow: &'static str,
    title: &'static str,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                padding: UiRect::axes(px(16), px(13)),
                flex_direction: FlexDirection::Column,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.5)),
        ))
        .with_children(|header| {
            spawn_text(header, font.clone(), eyebrow, 8.0, theme.primary);
            spawn_text(header, font, title, 13.0, theme.foreground);
        });
}

pub(crate) fn spawn_detail_value(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
    value: String,
) {
    parent
        .spawn((
            Node {
                min_height: px(48),
                padding: UiRect::axes(px(14), px(10)),
                flex_direction: FlexDirection::Column,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.3)),
        ))
        .with_children(|row| {
            spawn_text(row, font.clone(), label, 9.0, theme.muted_foreground);
            spawn_wrapped_text(row, font, value, 11.0, theme.foreground);
        });
}

pub(crate) fn song_overview_rows(song: &Song) -> Vec<(&'static str, String)> {
    let media = song
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("media")
        .to_ascii_uppercase();
    let transcript = song
        .transcript_source
        .as_ref()
        .map(|source| format!("{source:?}"))
        .unwrap_or_else(|| "Not generated".to_string());
    vec![
        (
            "Media",
            format!(
                "{media} · {}",
                if song.is_video { "Video" } else { "Audio" }
            ),
        ),
        (
            "Analysis",
            if song.is_analyzed {
                "Analyzed"
            } else {
                "Not analyzed"
            }
            .to_string(),
        ),
        ("Lyrics source", transcript),
        (
            "Stems",
            if song.no_stems {
                "Original mix"
            } else if song.is_analyzed {
                "Separated"
            } else {
                "Pending"
            }
            .to_string(),
        ),
        (
            "Chart assets",
            if song.authoring_ready {
                "Complete".to_string()
            } else if song.authoring_missing.is_empty() {
                "Waiting for chart".to_string()
            } else {
                song.authoring_missing.join(" · ").replace('_', " ")
            },
        ),
        (
            "Export",
            if song.authoring_ready {
                "UTZ · UltraStar"
            } else {
                "Waiting for chart"
            }
            .to_string(),
        ),
    ]
}

pub(crate) fn album_art_handle(
    song: &Song,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
) -> Handle<Image> {
    let Some(path) = song.album_art_path.as_ref() else {
        return asset_server.load(LOGO_PATH);
    };
    if let Some(handle) = local_images.covers.get(path) {
        return handle.clone();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return asset_server.load(LOGO_PATH);
    };
    let extension = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else {
        "jpg"
    };
    let Ok(decoded) = Image::from_buffer(
        &bytes,
        ImageType::Extension(extension),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::default(),
    ) else {
        return asset_server.load(LOGO_PATH);
    };
    let Ok(dynamic) = decoded.try_into_dynamic() else {
        return asset_server.load(LOGO_PATH);
    };
    // Library artwork can be several thousand pixels wide while its largest
    // presentation in the desktop UI is a small cover. Bounding retained
    // textures prevents a route change from uploading another full-resolution
    // image while the analyzer has recently held several gigabytes of models.
    let bounded = dynamic.thumbnail(512, 512);
    let image = Image::from_dynamic(bounded, true, RenderAssetUsages::default());
    let handle = images.add(image);
    local_images.covers.insert(path.clone(), handle.clone());
    handle
}

pub(crate) fn start_key_shift(
    file_hash: &str,
    delta: i8,
    job: &mut NativeAuthoringJob,
    busy: &mut bool,
) -> String {
    if *busy || job.receiver.is_some() {
        return "A key or tempo render is already running.".to_string();
    }
    let Some(song) = app_core::load_song_by_hash(file_hash).ok().flatten() else {
        return format!("Song not found: {file_hash}");
    };
    let Some(original_key) = song.key.as_deref() else {
        return "Analyze the song again to detect its original key.".to_string();
    };
    let offset = (song.key_offset + i32::from(delta)).clamp(-5, 5);
    if offset == song.key_offset {
        return "Key shift is limited to five semitones in either direction.".to_string();
    }
    let (key, pitch_ratio) = calculate_key_shift(original_key, offset);
    let notice_key = key.clone();
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = app_core::shift_key(&file_hash, &key, pitch_ratio, offset)
            .map_err(|error| error.to_string());
        let _ = sender.send(AuthoringEvent {
            result,
            kind: "key",
        });
    });
    job.receiver = Some(Mutex::new(receiver));
    *busy = true;
    format!("Rendering key variant {notice_key}…")
}

pub(crate) fn start_tempo_shift(
    file_hash: &str,
    delta: i8,
    job: &mut NativeAuthoringJob,
    busy: &mut bool,
) -> String {
    if *busy || job.receiver.is_some() {
        return "A key or tempo render is already running.".to_string();
    }
    let Some(song) = app_core::load_song_by_hash(file_hash).ok().flatten() else {
        return format!("Song not found: {file_hash}");
    };
    let tempo = ((song.tempo + f64::from(delta) * 0.1) * 10.0).round() / 10.0;
    let tempo = tempo.clamp(0.5, 2.0);
    if (tempo - song.tempo).abs() < f64::EPSILON {
        return "Tempo is limited to 0.5×–2.0×.".to_string();
    }
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = app_core::shift_tempo(&file_hash, tempo).map_err(|error| error.to_string());
        let _ = sender.send(AuthoringEvent {
            result,
            kind: "tempo",
        });
    });
    job.receiver = Some(Mutex::new(receiver));
    *busy = true;
    format!("Rendering {tempo:.1}× tempo variant…")
}

pub(crate) fn poll_authoring_job(
    mut job: ResMut<NativeAuthoringJob>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = job.receiver.as_ref().and_then(|receiver| {
        receiver
            .lock()
            .ok()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(event) => Some(Ok(event)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Key/tempo render worker exited unexpectedly.".to_string(),
                )),
            })
    });
    let Some(result) = result else {
        return;
    };
    job.receiver = None;
    session.authoring_busy = false;
    match result {
        Ok(event) => match event.result {
            Ok(rendered) => {
                session.notice = Some(format!(
                    "Song {} shifted successfully · key {} · {:.1}× tempo.",
                    event.kind, rendered.key, rendered.tempo
                ));
                session.refresh_library();
            }
            Err(error) => {
                session.notice = Some(format!("Could not render {} variant: {error}", event.kind))
            }
        },
        Err(error) => session.notice = Some(error),
    }
    invalidated.0 = true;
}

pub(crate) fn poll_lyrics_search_job(
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = session
        .lyrics_search_job
        .receiver
        .as_ref()
        .and_then(|receiver| {
            receiver
                .lock()
                .ok()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(candidates) => Some(Ok(candidates)),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("LRCLIB search worker exited unexpectedly.".to_string()))
                    }
                })
        });
    let Some(result) = result else {
        return;
    };
    session.lyrics_search_job.receiver = None;
    match result {
        Ok(candidates) => {
            let count = candidates.len();
            if let Some(editor) = session.lyrics_editor.as_mut() {
                editor.searching = false;
                editor.candidates = candidates;
                editor.candidate_index = 0;
                session.notice = Some(if count == 0 {
                    "LRCLIB did not return a matching lyric.".to_string()
                } else {
                    format!("Found {count} LRCLIB lyric candidate(s). Review before applying.")
                });
            }
        }
        Err(error) => {
            if let Some(editor) = session.lyrics_editor.as_mut() {
                editor.searching = false;
            }
            session.notice = Some(error);
        }
    }
    invalidated.0 = true;
}

pub(crate) fn calculate_key_shift(original_key: &str, offset: i32) -> (String, f64) {
    const NOTES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let (note, quality) = original_key
        .strip_suffix('m')
        .map(|note| (note, "m"))
        .unwrap_or((original_key, ""));
    let key = NOTES
        .iter()
        .position(|candidate| *candidate == note)
        .map(|index| {
            let shifted = (index as i32 + offset).rem_euclid(NOTES.len() as i32) as usize;
            format!("{}{quality}", NOTES[shifted])
        })
        .unwrap_or_else(|| original_key.to_string());
    (key, 2f64.powf(f64::from(offset) / 12.0))
}

pub(crate) fn run_analysis_action(file_hash: &str, action: impl FnOnce()) -> String {
    if !app_core::analysis_runtime_status().ready {
        return "Analysis is disabled until setup is completed in Settings > Models & runtime."
            .to_string();
    }
    let Some(song) = app_core::load_song_by_hash(file_hash).ok().flatten() else {
        return format!("Song not found: {file_hash}");
    };
    if matches!(
        song.transcript_source,
        Some(app_core::TranscriptSource::Usdx)
    ) {
        return "This action is unavailable for imported USDX charts.".to_string();
    }
    action();
    format!("Queued analysis for “{}”.", song.title)
}

pub(crate) fn handle_song_detail_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    session: Res<StudioSession>,
    mut contents: Query<(&ComputedNode, &mut ScrollPosition), With<SongDetailContent>>,
) {
    if session.route != StudioRoute::SongDetail || session.lyrics_editor.is_some() {
        wheel.clear();
        return;
    }
    let Ok((computed, mut position)) = contents.single_mut() else {
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
}
