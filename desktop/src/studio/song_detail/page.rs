use super::*;
use crate::studio::*;

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
                                "Tune separator, transcription, alignment, pitch, batching, and sensitivity. These settings only affect the next analysis; existing chart data will not change immediately.",
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

fn spawn_timed_transcript_structure(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeLyricsEditor,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&editor.initial_text) else {
        return;
    };
    let Some(segments) = value.get("segments").and_then(serde_json::Value::as_array) else {
        return;
    };
    parent
        .spawn((
            Node {
                width: percent(100),
                max_height: px(220),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(5),
                overflow: Overflow::scroll_y(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.42)),
            BorderColor::all(theme.border.with_alpha(0.55)),
        ))
        .with_children(|list| {
            spawn_text(
                list,
                font.clone(),
                "SEGMENTS AND WORD/TOKEN TIMING",
                8.0,
                theme.primary,
            );
            if !editor.waveform.peaks.is_empty() {
                spawn_text(
                    list,
                    font.clone(),
                    format!("AUDIO WAVEFORM · {}", format_duration(editor.waveform.duration_secs)),
                    7.5,
                    theme.muted_foreground,
                );
                list.spawn(Node {
                    width: percent(100),
                    height: px(46),
                    align_items: AlignItems::Center,
                    column_gap: px(1),
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|waveform| {
                    let stride = (editor.waveform.peaks.len() / 160).max(1);
                    for (minimum, maximum) in
                        editor.waveform.peaks.iter().step_by(stride).take(160)
                    {
                        let amplitude = minimum.abs().max(maximum.abs()).clamp(0.02, 1.0);
                        waveform.spawn((
                            Node {
                                min_width: px(1),
                                flex_grow: 1.0,
                                height: px(2.0 + amplitude * 40.0),
                                ..default()
                            },
                            BackgroundColor(theme.primary.with_alpha(0.55)),
                        ));
                    }
                });
            } else {
                spawn_text(
                    list,
                    font.clone(),
                    "Loading transcript waveform…",
                    7.5,
                    theme.muted_foreground,
                );
            }
            let mut rendered_words = 0usize;
            for (segment_index, segment) in segments.iter().take(50).enumerate() {
                let start = segment.get("start").and_then(|value| value.as_f64()).unwrap_or(0.0);
                let end = segment.get("end").and_then(|value| value.as_f64()).unwrap_or(start);
                let text = segment.get("text").and_then(|value| value.as_str()).unwrap_or("");
                list.spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(5),
                    row_gap: px(3),
                    ..default()
                })
                .with_children(|row| {
                    spawn_text(row, font.clone(), format!("S{}", segment_index + 1), 8.0, theme.muted_foreground);
                    spawn_text_button(row, font.clone(), theme, &format!("▶ {start:.3}s"), 8.0, UiAction::PreviewTranscriptAt(editor.file_hash.clone(), (start * 1000.0).round() as i64));
                    for (label, edge, delta) in [
                        ("start −10", TranscriptBoundaryEdge::Start, -10),
                        ("start +10", TranscriptBoundaryEdge::Start, 10),
                        ("end −10", TranscriptBoundaryEdge::End, -10),
                        ("end +10", TranscriptBoundaryEdge::End, 10),
                    ] {
                        spawn_text_button(row, font.clone(), theme, label, 7.5, UiAction::AdjustTranscriptBoundary(TranscriptBoundaryTarget::Segment(segment_index), edge, delta));
                    }
                    spawn_transcript_drag_handle(row, font.clone(), theme, "drag start ↔", TranscriptBoundaryTarget::Segment(segment_index), TranscriptBoundaryEdge::Start);
                    spawn_transcript_drag_handle(row, font.clone(), theme, "drag end ↔", TranscriptBoundaryTarget::Segment(segment_index), TranscriptBoundaryEdge::End);
                    spawn_bounded_wrapped_text(row, font.clone(), format!("{start:.3}–{end:.3}  {text}"), 8.5, theme.foreground);
                });
                if let Some(words) = segment.get("words").and_then(serde_json::Value::as_array) {
                    for (word_index, word) in words.iter().enumerate() {
                        if rendered_words >= 200 { break; }
                        rendered_words += 1;
                        let start = word.get("start").and_then(|value| value.as_f64()).unwrap_or(0.0);
                        let end = word.get("end").and_then(|value| value.as_f64()).unwrap_or(start);
                        let text = word.get("word").or_else(|| word.get("text")).and_then(|value| value.as_str()).unwrap_or("");
                        list.spawn(Node {
                            width: percent(100), padding: UiRect::left(px(20)),
                            align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap,
                            column_gap: px(4), row_gap: px(3), ..default()
                        }).with_children(|row| {
                            spawn_text_button(row, font.clone(), theme, &format!("▶ {start:.3}s"), 7.5, UiAction::PreviewTranscriptAt(editor.file_hash.clone(), (start * 1000.0).round() as i64));
                            for (label, edge, delta) in [
                                ("S−", TranscriptBoundaryEdge::Start, -10),
                                ("S+", TranscriptBoundaryEdge::Start, 10),
                                ("E−", TranscriptBoundaryEdge::End, -10),
                                ("E+", TranscriptBoundaryEdge::End, 10),
                            ] {
                                spawn_text_button(row, font.clone(), theme, label, 7.0, UiAction::AdjustTranscriptBoundary(TranscriptBoundaryTarget::Word { segment: segment_index, word: word_index }, edge, delta));
                            }
                            spawn_transcript_drag_handle(row, font.clone(), theme, "drag S ↔", TranscriptBoundaryTarget::Word { segment: segment_index, word: word_index }, TranscriptBoundaryEdge::Start);
                            spawn_transcript_drag_handle(row, font.clone(), theme, "drag E ↔", TranscriptBoundaryTarget::Word { segment: segment_index, word: word_index }, TranscriptBoundaryEdge::End);
                            spawn_text(row, font.clone(), format!("{start:.3}–{end:.3}  {text}"), 8.0, theme.foreground);
                        });
                    }
                }
            }
            if segments.len() > 50 || rendered_words >= 200 {
                spawn_wrapped_text(list, font, "The structured list is bounded for responsiveness; the lossless JSON editor below retains every segment, word, and extension field.", 8.0, theme.muted_foreground);
            }
        });
}

fn spawn_transcript_drag_handle(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
    target: TranscriptBoundaryTarget,
    edge: TranscriptBoundaryEdge,
) {
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(px(5), px(3)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(theme.primary.with_alpha(0.08)),
            BorderColor::all(theme.primary.with_alpha(0.35)),
            children![(
                Text::new(label),
                ui_text_font(font, 7.0),
                TextColor(theme.primary),
            )],
        ))
        .observe(
            move |mut drag: On<Pointer<Drag>>,
                  ui_scale: Res<UiScale>,
                  mut session: ResMut<StudioSession>,
                  mut inputs: Query<&mut EditableText, With<LyricsEditorInput>>| {
                if drag.button != PointerButton::Primary || drag.delta.x.abs() < f32::EPSILON {
                    return;
                }
                drag.propagate(false);
                let Ok(mut input) = inputs.single_mut() else {
                    return;
                };
                let current = input.value().to_string();
                let result = serde_json::from_str::<serde_json::Value>(&current)
                    .map_err(|error| format!("Invalid JSON: {error}"))
                    .and_then(|mut value| {
                        adjust_transcript_boundary_value(
                            &mut value,
                            target,
                            edge,
                            f64::from(drag.delta.x / ui_scale.0) * 0.005,
                        )?;
                        let rendered = serde_json::to_string_pretty(&value).unwrap_or_default();
                        let editor = session
                            .lyrics_editor
                            .as_mut()
                            .ok_or_else(|| "TimedTranscript editor is closed.".to_string())?;
                        let mut draft = editor
                            .artifact_draft
                            .clone()
                            .ok_or_else(|| "TimedTranscript draft is unavailable.".to_string())?;
                        draft.replace_json(value)?;
                        editor.initial_text = rendered.clone();
                        editor.artifact_draft = Some(draft);
                        input.editor_mut().set_text(&rendered);
                        Ok(())
                    });
                session.notice = result.err();
            },
        )
        .observe(
            |mut release: On<Pointer<DragEnd>>, mut invalidated: ResMut<UiInvalidated>| {
                release.propagate(false);
                invalidated.0 = true;
            },
        )
        .observe(
            |mut cancel: On<Pointer<Cancel>>, mut invalidated: ResMut<UiInvalidated>| {
                cancel.propagate(false);
                invalidated.0 = true;
            },
        );
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
                        if editor.mode == LyricsInputMode::StructuredTimedTranscript {
                            "TimedTranscript JSON"
                        } else if editor.mode == LyricsInputMode::TimedLrc {
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
                        if editor.mode == LyricsInputMode::StructuredTimedTranscript {
                            "Edit the lossless segment/word structure. Start/end values must be ordered and every word must remain inside its segment. Unknown extension fields are preserved."
                        } else if editor.mode == LyricsInputMode::TimedLrc {
                            "Paste line-level or enhanced LRC. Existing analyzed songs keep their stems; new songs can author over the original mix or explicitly queue separation."
                        } else if app_core::AppConfig::load().align_backend() == "mms_karaoke" {
                            "Enter one lyric phrase per line. MMS Karaoke accepts optional pronunciation overrides such as {漢字|かな} or [display|romaji]. Saving queues alignment and never modifies the source song."
                        } else {
                            "Enter one lyric phrase per line. Saving queues alignment and never modifies the source song."
                        },
                        10.0,
                        theme.muted_foreground,
                    );
                    if editor.artifact_draft.is_none() {
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
                    }
                    if editor.artifact_draft.is_none()
                        && let Some(candidate) = editor.candidates.get(editor.candidate_index) {
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
                    if editor.mode == LyricsInputMode::StructuredTimedTranscript {
                        spawn_timed_transcript_structure(dialog, font.clone(), theme, editor);
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
                    if let Some(draft) = editor.artifact_draft.as_ref()
                        && let Ok(impact) = app_core::preview_artifact_edit_impact(draft)
                    {
                        spawn_wrapped_text(
                            dialog,
                            font.clone(),
                            format!(
                                "Impact preview · {} downstream node(s) · Authored Chart preserved{}",
                                impact.affected_nodes.len(),
                                if impact.export_may_need_regeneration {
                                    " · exports may need regeneration"
                                } else {
                                    ""
                                }
                            ),
                            9.0,
                            theme.muted_foreground,
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
                            if editor.artifact_draft.is_some() {
                                spawn_text_button(
                                    actions,
                                    font.clone(),
                                    theme,
                                    "Save Only",
                                    10.0,
                                    UiAction::SaveLyricsEditor,
                                );
                                spawn_action_button(
                                    actions,
                                    font,
                                    theme,
                                    "Save and Run Downstream",
                                    UiAction::SaveLyricsEditorAndRunDownstream,
                                );
                            } else {
                                spawn_action_button(
                                    actions,
                                    font,
                                    theme,
                                    "Save lyrics",
                                    UiAction::SaveLyricsEditor,
                                );
                            }
                        });
                });
        });
}
