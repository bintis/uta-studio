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
                                    spawn_text(metadata, font.clone(), format!("{:.1}× playback speed", song.tempo), 9.0, theme.muted_foreground);
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
                            spawn_compact_action_button(
                                actions,
                                font.clone(),
                                theme,
                                "View in Analysis",
                                UiAction::SetLibraryView(LibraryView::Queue),
                            );
                            // Secondary actions (phase plan §8.1: "次级操作 Play
                            // original / Export / Song metadata / More").
                            // Export moved out of this row into the
                            // "Authoring & Export" section card (§8.2) --
                            // this row keeps Play/Settings only now.
                            spawn_compact_action_button(
                                actions,
                                font.clone(),
                                theme,
                                "Settings",
                                UiAction::OpenSongSettings(song.file_hash.clone()),
                            );
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
                        let analyzed_and_native = song.is_analyzed
                            && !matches!(
                                song.transcript_source,
                                Some(app_core::TranscriptSource::Usdx)
                            );
                        let native_source = !matches!(
                            song.transcript_source,
                            Some(app_core::TranscriptSource::Usdx)
                        );

                        // §8.2: 6 independent, named section cards
                        // (Overview/Analysis/Lyrics & Timing/Audio &
                        // Pitch/Authoring & Export/Artifacts & History)
                        // instead of one wide "Production controls" card
                        // with subheadings crammed into a single scrolling
                        // column -- same controls, same actions, same
                        // gating, just each with its own bordered card so
                        // the sections are independently legible and can
                        // reflow in the wrap layout.
                        spawn_song_detail_section_card(columns, theme, 360.0, |overview| {
                            spawn_detail_heading(
                                overview,
                                font.clone(),
                                theme,
                                "OVERVIEW",
                                "Track information",
                            );
                            for (label, value) in song_overview_rows(&song) {
                                spawn_detail_value(overview, font.clone(), theme, label, value);
                            }
                            spawn_source_file_row(overview, font.clone(), theme, &song.path);
                        });

                        spawn_song_detail_section_card(columns, theme, 420.0, |analysis| {
                            spawn_detail_heading(
                                analysis,
                                font.clone(),
                                theme,
                                "ANALYSIS",
                                "Analysis",
                            );
                            spawn_setting_row(
                                analysis,
                                font.clone(),
                                theme,
                                "Analysis defaults",
                                "Tune separator, transcription, alignment, pitch, batching, and sensitivity. Existing chart data changes only after re-analysis.",
                                Some(("Open analysis settings", UiAction::SettingsTab(SettingsTab::Analysis))),
                            );
                            if analyzed_and_native {
                                spawn_setting_row(
                                    analysis,
                                    font.clone(),
                                    theme,
                                    "Full reanalysis",
                                    "Recreate stems, lyrics, timing, detected key, musical BPM, and pitch assets.",
                                    Some((
                                        "Reanalyze all",
                                        UiAction::ReanalyzeFull(song.file_hash.clone()),
                                    )),
                                );
                                if matches!(
                                    app_core::candidate_chart_status(&song.file_hash),
                                    app_core::CandidateChartStatus::CandidateAvailable(_)
                                ) {
                                    spawn_setting_row(
                                        analysis,
                                        font.clone(),
                                        theme,
                                        "Candidate analysis",
                                        "A newer analysis result differs from your saved chart. Compare and choose whether to replace it.",
                                        Some((
                                            "Compare & replace…",
                                            UiAction::RequestReplaceAuthoredChart(
                                                song.file_hash.clone(),
                                            ),
                                        )),
                                    );
                                }
                            }
                        });

                        spawn_song_detail_section_card(columns, theme, 420.0, |lyrics| {
                            spawn_detail_heading(
                                lyrics,
                                font.clone(),
                                theme,
                                "LYRICS & TIMING",
                                "Lyrics & timing",
                            );
                            spawn_setting_row(
                                lyrics,
                                font.clone(),
                                theme,
                                "Lyrics",
                                "Paste plain lyrics to realign, or provide timed LRC without replacing source media.",
                                if native_source {
                                    Some((
                                        "Edit lyrics…".to_string(),
                                        UiAction::OpenLyricsEditor(song.file_hash.clone()),
                                    ))
                                } else {
                                    None
                                },
                            );
                            if native_source {
                                spawn_setting_row(
                                    lyrics,
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
                            if analyzed_and_native {
                                spawn_setting_row(
                                    lyrics,
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
                                    lyrics,
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
                                    lyrics,
                                    font.clone(),
                                    theme,
                                    "Transcription",
                                    "Ignore online lyrics and transcribe the vocals again.",
                                    Some((
                                        "Force transcribe",
                                        UiAction::ForceTranscribe(song.file_hash.clone()),
                                    )),
                                );
                            }
                        });

                        spawn_song_detail_section_card(columns, theme, 420.0, |audio| {
                            spawn_detail_heading(
                                audio,
                                font.clone(),
                                theme,
                                "AUDIO & PITCH",
                                "Audio & pitch",
                            );
                            if analyzed_and_native {
                                // Distinct labels per phase plan §8.5:
                                // "不得再让 BPM 和 1.0× Tempo 使用含混的同一文案"
                                // -- this row shifts the Detected Key by
                                // semitones (Key Transpose), never the
                                // detected musical BPM.
                                spawn_shift_setting_row(
                                    audio,
                                    font.clone(),
                                    theme,
                                    "Key transpose",
                                    song.key
                                        .as_ref()
                                        .map(|key| format!("Detected key: {key}"))
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
                                // This is the export-speed multiplier, not
                                // the detected Musical BPM (shown in Song
                                // Settings) -- kept explicitly out of
                                // "Tempo"/"BPM" territory.
                                spawn_shift_setting_row(
                                    audio,
                                    font.clone(),
                                    theme,
                                    "Playback / export speed",
                                    "Create an export-speed variant in 0.1× steps.",
                                    format!("{:.1}×", song.tempo),
                                    UiAction::ShiftSongTempo(song.file_hash.clone(), -1),
                                    UiAction::ShiftSongTempo(song.file_hash.clone(), 1),
                                );
                                spawn_setting_row(
                                    audio,
                                    font.clone(),
                                    theme,
                                    "Frequency analysis",
                                    "Generate or repair the editable pitch guide.",
                                    Some((
                                        "Analyze pitch",
                                        UiAction::ReanalyzePitch(song.file_hash.clone()),
                                    )),
                                );
                            } else {
                                spawn_setting_row(
                                    audio,
                                    font.clone(),
                                    theme,
                                    "Key transpose & playback speed",
                                    "Controls become available after compatible analysis.",
                                    None::<(&'static str, UiAction)>,
                                );
                            }
                        });

                        // Moved out of the page header's action row -- the
                        // header keeps the primary CTA/Play/Settings only,
                        // Export now lives with the rest of this song's
                        // authoring controls (phase plan §8.2's "Authoring &
                        // Export" section).
                        spawn_song_detail_section_card(columns, theme, 380.0, |authoring| {
                            spawn_detail_heading(
                                authoring,
                                font.clone(),
                                theme,
                                "AUTHORING",
                                "Authoring & export",
                            );
                            if song.authoring_ready {
                                spawn_setting_row(
                                    authoring,
                                    font.clone(),
                                    theme,
                                    "UTZ project",
                                    "Export the full editable project file for Uta Studio.",
                                    Some((
                                        "Export UTZ",
                                        UiAction::ExportUtz(song.file_hash.clone()),
                                    )),
                                );
                                spawn_setting_row(
                                    authoring,
                                    font.clone(),
                                    theme,
                                    "UltraStar chart",
                                    "Export a chart compatible with UltraStar-format karaoke games.",
                                    Some((
                                        "Export UltraStar",
                                        UiAction::ExportUltraStar(song.file_hash.clone()),
                                    )),
                                );
                            } else {
                                spawn_setting_row(
                                    authoring,
                                    font.clone(),
                                    theme,
                                    "Export",
                                    "Export becomes available once this song's chart is ready for authoring.",
                                    None::<(&'static str, UiAction)>,
                                );
                            }
                        });

                        spawn_song_detail_section_card(columns, theme, 380.0, |history| {
                            spawn_detail_heading(
                                history,
                                font.clone(),
                                theme,
                                "ARTIFACTS & HISTORY",
                                "Artifacts & history",
                            );
                            if analyzed_and_native {
                                spawn_setting_row(
                                    history,
                                    font.clone(),
                                    theme,
                                    "Generated song data",
                                    "Delete generated cache for this song. Source media is never changed.",
                                    Some((
                                        "Delete cache…",
                                        UiAction::RequestDeleteSongCache(song.file_hash.clone()),
                                    )),
                                );
                            } else {
                                spawn_setting_row(
                                    history,
                                    font.clone(),
                                    theme,
                                    "Generated song data",
                                    "Controls become available after compatible analysis.",
                                    None::<(&'static str, UiAction)>,
                                );
                            }
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
            if let Some(file_hash) = session.pending_chart_replace.as_deref() {
                spawn_chart_replace_confirmation(detail, font.clone(), theme, file_hash);
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

/// Phase 5 §5.4 "Compare / Merge / Replace" confirmation modal. Fetches a
/// fresh `candidate_chart_status` at render time (same pattern as
/// `spawn_cache_delete_confirmation` re-fetching the song title) rather than
/// threading the summary through `pending_chart_replace`, so the numbers
/// shown are never stale relative to whatever analysis has run since the
/// button was clicked.
pub(crate) fn spawn_chart_replace_confirmation(
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
    let body = match app_core::candidate_chart_status(file_hash) {
        app_core::CandidateChartStatus::CandidateAvailable(summary) => {
            let mut changed = Vec::new();
            if summary.lyrics_changed {
                changed.push("lyrics timing");
            }
            if summary.pitch_evidence_changed {
                changed.push("pitch evidence");
            }
            format!(
                "The current candidate analysis for “{title}” has {} note(s) across {} phrase(s), \
                 versus {} note(s) across {} phrase(s) in your saved chart. Updated: {}. \
                 Replacing discards your edits; the next time you open the editor it rebuilds \
                 from this candidate instead.",
                summary.candidate_note_count,
                summary.candidate_phrase_count,
                summary.authored_note_count,
                summary.authored_phrase_count,
                changed.join(" & "),
            )
        }
        _ => format!(
            "No candidate analysis is currently available for “{title}”. Replacing would discard \
             your saved chart and rebuild from whatever analysis output already exists on disk."
        ),
    };
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
                width: px(480),
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
                    Text::new("Replace authored chart with the candidate?"),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(body),
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
                            UiAction::CancelReplaceAuthoredChart,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Keep my chart"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmReplaceAuthoredChart,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Replace with candidate"),
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

pub(crate) fn spawn_artifact_delete_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    revision: &app_core::ArtifactRevision,
) {
    let file_name = revision
        .path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| revision.id.clone());
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
        // Above the activity center overlay (ZIndex 100) that this
        // confirmation is always triggered from.
        ZIndex(110),
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
                    Text::new("Delete this artifact revision?"),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "“{file_name}” will be removed from the cache and its revision history. This does not touch the source song."
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
                            UiAction::CancelDeleteArtifactRevision,
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
                            UiAction::ConfirmDeleteArtifactRevision,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Delete revision"),
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

/// Phase 6 `invalidate_artifact_revision` / Phase 7 §7.6 "Invalidate".
/// Same modal shape as `spawn_artifact_delete_confirmation`, but the copy
/// makes explicit that (unlike Delete) the file and its revision history
/// both survive -- only the "trustworthy/Active-eligible" status changes.
pub(crate) fn spawn_artifact_invalidate_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    revision: &app_core::ArtifactRevision,
) {
    let file_name = revision
        .path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| revision.id.clone());
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
        ZIndex(110),
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
                    Text::new("Invalidate this artifact revision?"),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "“{file_name}” will be marked stale/wrong and, if it's currently Active, stop being the one this song uses. The file and its revision history are kept -- this doesn't delete anything."
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
                            UiAction::CancelInvalidateArtifactRevision,
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
                            UiAction::ConfirmInvalidateArtifactRevision,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Invalidate revision"),
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

/// One highlighted primary CTA per song state (phase plan §8.1: "按歌曲状态只
/// 突出一个主 CTA"), driven by the real `SongAuthoringState` state machine
/// (Phase 8 backend, `analyzer.rs`) instead of the inline
/// analyzed/authoring_ready/editor_ready chain this replaces. That chain
/// used to show up to three buttons at once (export UTZ + export UltraStar
/// + edit chart) when a song was authoring-ready; those exports are now
/// secondary actions in the row this function is called from (see the
/// `song_detail.rs` call site), per §8.1's own primary/secondary split
/// ("次级操作: Play original / Export / Song metadata / More") -- moving
/// them out, not dropping them.
pub(crate) fn spawn_song_primary_actions(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    song: &Song,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let state = app_core::resolve_song_authoring_state(&song.file_hash)
        .unwrap_or(app_core::SongAuthoringState::AnalyzeSong);
    match state {
        app_core::SongAuthoringState::InProgress => {
            let label = session
                .analysis_tasks
                .iter()
                .find(|task| task.file_hash == song.file_hash)
                .map(|task| match task.status {
                    app_core::QueuedStatus::Queued => "Queued for analysis".to_string(),
                    app_core::QueuedStatus::Analyzing(progress) => {
                        format!("Analyzing · {progress}%")
                    }
                    app_core::QueuedStatus::Failed(_) => "Analyzing".to_string(),
                })
                .unwrap_or_else(|| "Analyzing".to_string());
            spawn_action_button(parent, font, theme, label, UiAction::ToggleActivity);
        }
        app_core::SongAuthoringState::RetryFailedNode => {
            spawn_action_button(
                parent,
                font,
                theme,
                "Retry failed analysis",
                UiAction::AnalyzeSong(song.file_hash.clone()),
            );
        }
        app_core::SongAuthoringState::AnalyzeSong => {
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
        }
        app_core::SongAuthoringState::OpenEditor => {
            spawn_action_button(
                parent,
                font,
                theme,
                "Open editor",
                UiAction::OpenEditor(song.file_hash.clone()),
            );
        }
        app_core::SongAuthoringState::FixChartIssues => {
            spawn_action_button(
                parent,
                font,
                theme,
                "Fix chart issues",
                UiAction::OpenEditor(song.file_hash.clone()),
            );
        }
        app_core::SongAuthoringState::EditChart => {
            spawn_action_button(
                parent,
                font,
                theme,
                "Edit chart",
                UiAction::OpenEditor(song.file_hash.clone()),
            );
        }
    }
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

/// One of the phase plan §8.2's 6 independent, named section cards
/// (Overview/Analysis/Lyrics & Timing/Audio & Pitch/Authoring &
/// Export/Artifacts & History) -- same bordered-card style every card on
/// this page already uses (`BackgroundColor(theme.card.with_alpha(0.32))` +
/// `BorderColor`), factored out since the page now has 6 of them instead of
/// the 2 (one wide "Production controls" card with subheadings crammed into
/// a single scrolling column, one "Overview" card) it used to. `min_width`
/// also doubles as `flex_basis` so cards keep a sensible starting width
/// before the row's `FlexWrap::Wrap` reflows them.
pub(crate) fn spawn_song_detail_section_card(
    columns: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    min_width: f32,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    columns
        .spawn((
            Node {
                min_width: px(min_width),
                flex_basis: px(min_width),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.32)),
            BorderColor::all(theme.border.with_alpha(0.55)),
        ))
        .with_children(build);
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

/// Overview section rows (phase plan §8.2's Overview list: authoring
/// readiness, detected key/confidence, musical BPM/confidence, beat count,
/// vocal/instrumental/pitch-evidence availability, active analysis
/// profile, timed-lyrics source, chart assets). Reads real on-disk/DB
/// state directly (`load_music_analysis`, `cached_artifact_presence_for_song`,
/// `get_song_analysis_profile`) rather than only `Song`'s cached summary
/// fields, same accepted pattern as this file's other direct app-core
/// reads during render. Deliberately **not** included: a chart issue
/// count -- that needs the full `ChartDocument`'s `ChartProblem` list,
/// which only exists once the chart is loaded into the editor, not
/// something to load and parse on every Song Detail render.
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
    let music_analysis = app_core::load_music_analysis(&app_core::CacheDir::new(), &song.file_hash);
    let presence = app_core::cached_artifact_presence_for_song(&song.file_hash);
    let has_artifact = |kind: app_core::ArtifactKind| app_core::artifact_present(&presence, kind);
    let profile_source = if app_core::get_song_analysis_profile(&song.file_hash).is_some() {
        "Song override"
    } else {
        "Global defaults"
    };

    let mut rows = vec![
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
        ("Active analysis profile", profile_source.to_string()),
        ("Lyrics source", transcript),
    ];

    // §8.2 Overview's "Last successful run" -- previously recorded as
    // blocked on "the Phase 3 history writer," which was stale: the
    // `analysis_history` table this reads has existed since before this
    // pass and already carries everything needed (`file_hash`, `status`,
    // `finished_at_ms`); the only actual gap was that nothing queried it
    // for this row yet.
    rows.push((
        "Last successful run",
        last_successful_run_copy(&app_core::load_analysis_history(200), &song.file_hash),
    ));

    // Phase 5 §5.5 "New candidate analysis is available" -- real, mtime-based
    // staleness comparison (`app_core::candidate_chart_status`), not a
    // placeholder. Omitted entirely (not just "N/A") for a song that has
    // never been authored yet: `chart_readiness`'s own missing-assets copy
    // already covers that case, and "candidate" isn't a meaningful concept
    // until there's an Authored Chart to compare it against.
    if let Some(copy) =
        candidate_availability_copy(&app_core::candidate_chart_status(&song.file_hash))
    {
        rows.push(("Candidate availability", copy));
    }

    // Phase 8 "Chart issue count" -- previously deferred as "needs a full
    // `ChartDocument` load, too expensive per render," which turned out to
    // be a false premise: `EditorDocument::new` only needs the chart's
    // structural data (lyrics/notes), not `ChartAudio`/`playable_audio`
    // resolution, so `app_core::chart_problem_count` is cheap enough to
    // call directly here.
    if let Some(copy) = chart_issue_count_copy(app_core::chart_problem_count(&song.file_hash)) {
        rows.push(("Chart issues", copy));
    }

    if let Some(analysis) = music_analysis.as_ref() {
        // §9.2 Music Analysis acceptance: "Unknown Key shows as Warning, not
        // Failure" -- this row has no error/failure styling to begin with
        // (plain informational text), so an undetected key already renders
        // as the same "Unknown" text it always has, never as a failure.
        rows.push(("Detected key", detected_key_copy(&analysis.key)));
        // "BPM-only fallback correctly displayed" -- `beats` is empty
        // whenever `analyze_rhythm` (rhythm.py) could only estimate a global
        // tempo via autocorrelation, without Essentia's full beat tracker
        // (see the doc comment on `MusicRhythmAnalysis::beats`). Previously
        // this rendered as "· 0 beats", indistinguishable from a bug; now
        // it's named explicitly.
        rows.push(("Musical BPM", musical_bpm_copy(&analysis.rhythm)));
        // "Descriptors unavailable shows Not Applicable" -- previously there
        // was no row for this at all, so the gap was that it was silently
        // absent rather than explicitly N/A.
        rows.push((
            "Extra descriptors",
            extra_descriptors_copy(analysis.descriptors.as_ref()),
        ));
    }

    rows.push((
        "Vocal / instrumental stems",
        match (
            has_artifact(app_core::ArtifactKind::VocalStem),
            has_artifact(app_core::ArtifactKind::InstrumentalStem),
        ) {
            (true, true) => "Both available".to_string(),
            (true, false) => "Vocal only".to_string(),
            (false, true) => "Instrumental only".to_string(),
            (false, false) if song.no_stems => "Original mix".to_string(),
            (false, false) => "Pending".to_string(),
        },
    ));
    rows.push((
        "Pitch evidence",
        if has_artifact(app_core::ArtifactKind::PitchTrack) {
            "Available"
        } else {
            "Pending"
        }
        .to_string(),
    ));
    rows.push((
        "Chart assets",
        if song.authoring_ready {
            "Complete".to_string()
        } else if song.authoring_missing.is_empty() {
            "Waiting for chart".to_string()
        } else {
            song.authoring_missing.join(" · ").replace('_', " ")
        },
    ));
    rows.push((
        "Export",
        if song.authoring_ready {
            "UTZ · UltraStar"
        } else {
            "Waiting for chart"
        }
        .to_string(),
    ));
    rows
}

/// Pure lookup behind `song_overview_rows`'s "Last successful run" row,
/// separated out so it's testable without a real DB fixture -- same pattern
/// as `resolve_song_authoring_state`/`overlay_failed_node_attempts`.
/// `history` is assumed newest-first (`analysis_history_load`'s real
/// ordering, `ORDER BY finished_at_ms DESC, id DESC`), so the first match
/// is genuinely the most recent completed run for this song, not merely *a*
/// completed run.
fn last_successful_run_copy(history: &[app_core::AnalysisRunHistory], file_hash: &str) -> String {
    history
        .iter()
        .find(|run| run.file_hash == file_hash && run.status == "completed")
        .map(|run| format_epoch_ms(run.finished_at_ms))
        .unwrap_or_else(|| "None yet".to_string())
}

/// Pure formatter behind the Overview panel's "Candidate availability" row
/// -- same "pure decision function separated from IO" pattern as
/// `last_successful_run_copy`. `None` means the row should be omitted
/// entirely (nothing authored yet, so "candidate" isn't a meaningful
/// concept for this song).
fn candidate_availability_copy(status: &app_core::CandidateChartStatus) -> Option<String> {
    match status {
        app_core::CandidateChartStatus::NotAuthoredYet => None,
        app_core::CandidateChartStatus::UpToDate => Some("Up to date".to_string()),
        app_core::CandidateChartStatus::CandidateAvailable(summary) => {
            let mut changed = Vec::new();
            if summary.lyrics_changed {
                changed.push("lyrics");
            }
            if summary.pitch_evidence_changed {
                changed.push("pitch");
            }
            Some(format!(
                "New candidate available ({} · {} notes vs {} authored)",
                changed.join(" & "),
                summary.candidate_note_count,
                summary.authored_note_count,
            ))
        }
    }
}

/// Pure formatter behind the Overview panel's "Chart issues" row -- same
/// "pure decision function separated from IO" pattern as
/// `candidate_availability_copy`. `None` means the row should be omitted
/// entirely (no transcript/pitch/authored chart data exists yet for this
/// song, so a problem count isn't a meaningful concept).
fn chart_issue_count_copy(count: Option<usize>) -> Option<String> {
    match count? {
        0 => Some("None".to_string()),
        1 => Some("1 issue".to_string()),
        n => Some(format!("{n} issues")),
    }
}

/// Pure formatter behind the Overview panel's "Detected key" row. §9.2
/// Music Analysis acceptance: "Unknown Key shows as Warning, not Failure."
fn detected_key_copy(key: &app_core::MusicKeyAnalysis) -> String {
    let key_name = key
        .tonic
        .as_deref()
        .map(|tonic| match key.scale.as_deref() {
            Some(scale) => format!("{tonic} {scale}"),
            None => tonic.to_string(),
        })
        .unwrap_or_else(|| "Unknown".to_string());
    format!("{key_name} (confidence {:.2})", key.confidence)
}

/// Pure formatter behind the Overview panel's "Musical BPM" row. §9.2 Music
/// Analysis acceptance: "BPM-only fallback correctly displayed" -- named
/// explicitly rather than rendering as an unexplained "0 beats".
fn musical_bpm_copy(rhythm: &app_core::MusicRhythmAnalysis) -> String {
    let Some(bpm) = rhythm.bpm else {
        return "Unavailable".to_string();
    };
    if rhythm.beats.is_empty() {
        format!(
            "{bpm:.1} (confidence {:.2}) · BPM-only, no beat grid",
            rhythm.confidence
        )
    } else {
        format!(
            "{bpm:.1} (confidence {:.2}) · {} beats",
            rhythm.confidence,
            rhythm.beats.len()
        )
    }
}

/// Pure formatter behind the Overview panel's "Extra descriptors" row. §9.2
/// Music Analysis acceptance: "Descriptors unavailable shows Not
/// Applicable" -- Essentia has no Windows wheel, so this is a real,
/// expected state, not an error.
fn extra_descriptors_copy(descriptors: Option<&app_core::MusicAnalysisDescriptors>) -> String {
    match descriptors {
        None => "Not Applicable".to_string(),
        Some(d) => format!(
            "Danceability {:.2} · Dynamic range {:.1} dB · Loudness {:.1} dB",
            d.danceability, d.dynamic_complexity_db, d.loudness_db
        ),
    }
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

/// Like `run_analysis_action`, for the Phase 4 executor's `Result`-returning
/// entry points (`run_analysis_node`/`disable_analysis_node_for_run`), which
/// can genuinely refuse a request (e.g. disabling an `AlwaysRequired` node)
/// instead of always succeeding the way every legacy special-case function
/// above does.
pub(crate) fn run_analysis_action_checked(
    file_hash: &str,
    action: impl FnOnce() -> Result<(), String>,
) -> String {
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
    match action() {
        Ok(()) => format!("Queued analysis for “{}”.", song.title),
        Err(error) => format!("Could not queue analysis: {error}"),
    }
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

#[cfg(test)]
mod last_successful_run_tests {
    //! §8.2 Overview's "Last successful run" row -- previously recorded as
    //! blocked on a not-yet-built Phase 3 history writer, which was stale:
    //! `analysis_history` already carried everything this needs.
    use super::last_successful_run_copy;
    use app_core::{AnalysisProgressSnapshot, AnalysisRunHistory};

    fn run(file_hash: &str, status: &str, finished_at_ms: i64) -> AnalysisRunHistory {
        AnalysisRunHistory {
            id: 1,
            file_hash: file_hash.to_string(),
            title: "Test Song".to_string(),
            artist: "Test Artist".to_string(),
            status: status.to_string(),
            started_at_ms: finished_at_ms - 1000,
            finished_at_ms,
            error_message: None,
            snapshot: AnalysisProgressSnapshot {
                stage: "complete".to_string(),
                stage_progress: 100,
                operation: String::new(),
                detail: String::new(),
                implementation: String::new(),
                model: String::new(),
                device: String::new(),
                requested_device: String::new(),
                fallback_from: None,
                fallback_reason: None,
                backend_fallback_from: None,
                backend_fallback_reason: None,
                stage_routes: Vec::new(),
                node_id: None,
                node_event: None,
                artifact_reused_reason: None,
            },
        }
    }

    #[test]
    fn finds_the_most_recent_completed_run_for_this_song() {
        let history = vec![
            run("songA", "completed", 2_000),
            run("songB", "completed", 5_000),
        ];
        assert_eq!(
            last_successful_run_copy(&history, "songA"),
            super::format_epoch_ms(2_000)
        );
    }

    #[test]
    fn ignores_a_failed_run_and_falls_back_to_an_earlier_success() {
        let history = vec![
            run("songA", "failed", 9_000),
            run("songA", "completed", 3_000),
        ];
        assert_eq!(
            last_successful_run_copy(&history, "songA"),
            super::format_epoch_ms(3_000)
        );
    }

    #[test]
    fn a_newest_first_ordered_list_returns_the_first_match_not_just_any_match() {
        let history = vec![
            run("songA", "completed", 9_000),
            run("songA", "completed", 1_000),
        ];
        assert_eq!(
            last_successful_run_copy(&history, "songA"),
            super::format_epoch_ms(9_000)
        );
    }

    #[test]
    fn a_song_with_no_completed_run_shows_none_yet() {
        let history = vec![run("songA", "failed", 1_000)];
        assert_eq!(last_successful_run_copy(&history, "songA"), "None yet");
    }

    #[test]
    fn a_different_songs_completed_run_is_not_matched() {
        let history = vec![run("songB", "completed", 1_000)];
        assert_eq!(last_successful_run_copy(&history, "songA"), "None yet");
    }
}

#[cfg(test)]
mod candidate_availability_copy_tests {
    //! Phase 5 §5.5 "New candidate analysis is available" -- the Overview
    //! panel's "Candidate availability" row.
    use super::candidate_availability_copy;
    use app_core::{CandidateChartStatus, CandidateChartSummary};

    #[test]
    fn not_authored_yet_omits_the_row_entirely() {
        assert_eq!(
            candidate_availability_copy(&CandidateChartStatus::NotAuthoredYet),
            None
        );
    }

    #[test]
    fn up_to_date_reports_up_to_date() {
        assert_eq!(
            candidate_availability_copy(&CandidateChartStatus::UpToDate),
            Some("Up to date".to_string())
        );
    }

    #[test]
    fn candidate_available_names_what_changed_and_the_note_counts() {
        let copy = candidate_availability_copy(&CandidateChartStatus::CandidateAvailable(
            CandidateChartSummary {
                authored_phrase_count: 2,
                authored_note_count: 10,
                candidate_phrase_count: 3,
                candidate_note_count: 14,
                lyrics_changed: true,
                pitch_evidence_changed: true,
            },
        ))
        .unwrap();
        assert!(copy.contains("lyrics"));
        assert!(copy.contains("pitch"));
        assert!(copy.contains("14 notes"));
        assert!(copy.contains("10 authored"));
    }

    #[test]
    fn candidate_available_only_names_the_input_that_actually_changed() {
        let copy = candidate_availability_copy(&CandidateChartStatus::CandidateAvailable(
            CandidateChartSummary {
                authored_phrase_count: 1,
                authored_note_count: 5,
                candidate_phrase_count: 1,
                candidate_note_count: 5,
                lyrics_changed: true,
                pitch_evidence_changed: false,
            },
        ))
        .unwrap();
        assert!(copy.contains("lyrics"));
        assert!(!copy.contains("pitch"));
    }
}

#[cfg(test)]
mod chart_issue_count_copy_tests {
    //! Phase 8 "Chart issue count" -- the Overview panel's "Chart issues"
    //! row.
    use super::chart_issue_count_copy;

    #[test]
    fn no_data_omits_the_row_entirely() {
        assert_eq!(chart_issue_count_copy(None), None);
    }

    #[test]
    fn zero_problems_reports_none() {
        assert_eq!(chart_issue_count_copy(Some(0)), Some("None".to_string()));
    }

    #[test]
    fn one_problem_uses_the_singular() {
        assert_eq!(chart_issue_count_copy(Some(1)), Some("1 issue".to_string()));
    }

    #[test]
    fn multiple_problems_use_the_plural() {
        assert_eq!(
            chart_issue_count_copy(Some(4)),
            Some("4 issues".to_string())
        );
    }
}

#[cfg(test)]
mod music_analysis_row_copy_tests {
    //! §9.2 Music Analysis acceptance: "Unknown Key shows as Warning, not
    //! Failure" / "BPM-only fallback correctly displayed" / "Descriptors
    //! unavailable shows Not Applicable" -- the Overview panel's "Detected
    //! key" / "Musical BPM" / "Extra descriptors" rows.
    use super::{detected_key_copy, extra_descriptors_copy, musical_bpm_copy};
    use app_core::{MusicAnalysisDescriptors, MusicKeyAnalysis, MusicRhythmAnalysis};

    #[test]
    fn unknown_key_reads_as_plain_unknown_never_a_failure() {
        let copy = detected_key_copy(&MusicKeyAnalysis {
            tonic: None,
            scale: None,
            confidence: 0.0,
        });
        assert!(copy.starts_with("Unknown"));
        assert!(!copy.to_lowercase().contains("fail"));
    }

    #[test]
    fn a_detected_key_names_tonic_and_scale() {
        let copy = detected_key_copy(&MusicKeyAnalysis {
            tonic: Some("F#".to_string()),
            scale: Some("minor".to_string()),
            confidence: 0.8,
        });
        assert_eq!(copy, "F# minor (confidence 0.80)");
    }

    #[test]
    fn no_bpm_is_unavailable() {
        let copy = musical_bpm_copy(&MusicRhythmAnalysis {
            bpm: None,
            confidence: 0.0,
            beats: vec![],
        });
        assert_eq!(copy, "Unavailable");
    }

    #[test]
    fn bpm_with_no_beats_is_named_as_the_fallback_explicitly() {
        let copy = musical_bpm_copy(&MusicRhythmAnalysis {
            bpm: Some(120.0),
            confidence: 0.5,
            beats: vec![],
        });
        assert!(copy.contains("BPM-only"));
        assert!(!copy.contains("0 beats"));
    }

    #[test]
    fn bpm_with_a_full_beat_grid_counts_the_beats() {
        let copy = musical_bpm_copy(&MusicRhythmAnalysis {
            bpm: Some(120.0),
            confidence: 0.9,
            beats: vec![0.5, 1.0, 1.5],
        });
        assert!(copy.contains("3 beats"));
        assert!(!copy.contains("BPM-only"));
    }

    #[test]
    fn missing_descriptors_is_not_applicable() {
        assert_eq!(extra_descriptors_copy(None), "Not Applicable");
    }

    #[test]
    fn present_descriptors_are_formatted() {
        let descriptors = MusicAnalysisDescriptors {
            danceability: 0.72,
            dynamic_complexity_db: 8.3,
            loudness_db: -12.4,
        };
        let copy = extra_descriptors_copy(Some(&descriptors));
        assert!(copy.contains("0.72"));
        assert!(copy.contains("8.3"));
        assert!(copy.contains("-12.4"));
    }
}
