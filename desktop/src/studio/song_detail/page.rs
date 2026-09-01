use super::*;
use crate::studio::*;

pub(crate) fn view_song_analysis_action(file_hash: &str) -> UiAction {
    UiAction::from(AnalysisCommand::OpenSongAnalysis(file_hash.to_string()))
}

fn analysis_profile_summary(
    global_quality: app_core::AnalysisQualityProfile,
    global_target: app_core::AnalysisDefaultTarget,
    song: Option<&app_core::AnalysisExperienceOverride>,
) -> String {
    let (quality, quality_source) = match song.and_then(|profile| profile.quality_profile) {
        Some(quality) => (quality, "SONG"),
        None => (global_quality, "GLOBAL"),
    };
    let quality = match quality {
        app_core::AnalysisQualityProfile::Fast => "Fast",
        app_core::AnalysisQualityProfile::Balanced => "Balanced",
        app_core::AnalysisQualityProfile::Maximum => "Maximum",
    };
    let (target, target_source) = match song.and_then(|profile| profile.default_target) {
        Some(target) => (target, "SONG"),
        None => (global_target, "GLOBAL"),
    };
    let target = match target {
        app_core::AnalysisDefaultTarget::FullCandidate => "Candidate chart",
        app_core::AnalysisDefaultTarget::Transcript => "Transcript",
        app_core::AnalysisDefaultTarget::Alignment => "Alignment",
        app_core::AnalysisDefaultTarget::PitchEvidence => "PitchEvidence",
        app_core::AnalysisDefaultTarget::Instrumental => "Instrumental",
    };
    format!(
        "Quality: {quality} · Source: {quality_source}. Target: {target} · Source: {target_source}. This row is read-only. Edit defaults in Settings > Analysis; use Plan Preview for one run. Existing chart data changes only after re-analysis."
    )
}

fn spawn_song_hero_fact(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
    value: impl Into<String>,
    accent: Color,
) {
    parent
        .spawn((
            Node {
                min_width: px(132),
                flex_basis: px(158),
                flex_grow: 1.0,
                min_height: px(54),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(12), px(8)),
                row_gap: px(3),
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                border_radius: studio_control_radius(),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.24)),
            BorderColor::all(theme.border.with_alpha(0.38)),
        ))
        .with_children(|fact| {
            spawn_text(fact, font.clone(), label, 7.0, theme.muted_foreground);
            spawn_bounded_wrapped_text(fact, font, value, 9.5, accent);
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_song_next_step(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    title: impl Into<String>,
    detail: impl Into<String>,
    action_label: impl Into<String>,
    action: UiAction,
    accent: Color,
) {
    let title = title.into();
    let detail = detail.into();
    let action_label = action_label.into();
    parent
        .spawn((
            Node {
                min_width: px(236),
                flex_basis: px(272),
                flex_grow: 0.55,
                min_height: px(160),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(14)),
                row_gap: px(9),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(accent.with_alpha(0.08)),
            BorderColor::all(accent.with_alpha(0.34)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn((
                            Node {
                                width: px(36),
                                height: px(36),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(9)),
                                ..default()
                            },
                            BackgroundColor(accent.with_alpha(0.12)),
                            BorderColor::all(accent.with_alpha(0.26)),
                        ))
                        .with_children(|slot| spawn_icon(slot, icons, icon, 17.0, accent));
                    header
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(copy, font.clone(), "NEXT STEP", 7.0, accent);
                            spawn_bounded_wrapped_text(
                                copy,
                                font.clone(),
                                title,
                                14.0,
                                theme.foreground,
                            );
                        });
                });
            spawn_wrapped_text(panel, font.clone(), detail, 8.8, theme.muted_foreground);
            panel.spawn(Node {
                min_height: px(2),
                flex_grow: 1.0,
                ..default()
            });
            panel
                .spawn(Node {
                    width: percent(100),
                    ..default()
                })
                .with_children(|actions| {
                    spawn_compact_primary_action_button(actions, font, theme, action_label, action);
                });
        });
}

fn spawn_song_workspace_intro(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(58),
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(12),
                row_gap: px(8),
                padding: UiRect::axes(px(4), px(8)),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.34)),
        ))
        .with_children(|intro| {
            intro
                .spawn(Node {
                    min_width: px(260),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    ..default()
                })
                .with_children(|copy| {
                    spawn_text(copy, font.clone(), "SONG WORKSPACE", 7.5, theme.primary);
                    spawn_text(
                        copy,
                        font.clone(),
                        "Production tools",
                        16.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        copy,
                        font.clone(),
                        "Analysis, lyrics, pitch, chart authoring and exports are grouped by task.",
                        8.5,
                        theme.muted_foreground,
                    );
                });
            spawn_status_pill(
                intro,
                font,
                "Source media is read-only",
                theme.muted_foreground,
            );
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_song_detail_hero(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
    song: &Song,
) {
    let cover = album_art_handle(song, asset_server, images, local_images);
    let current = session.library_playback.file_hash.as_deref() == Some(song.file_hash.as_str())
        && session.library_playback.status.loaded;
    let playing = current && session.library_playback.status.playing;
    let play_action = if current {
        UiAction::from(LibraryCommand::ToggleLibraryPlayback)
    } else {
        UiAction::from(LibraryCommand::PlayLibrarySong(song.file_hash.clone()))
    };
    let (primary_label, primary_action) = song_analysis_action(song);
    let analysis_label = if song.is_analyzed {
        "Analysis complete"
    } else {
        "Analysis needed"
    };
    let analysis_color = if song.is_analyzed {
        theme.pitch_contour
    } else {
        theme.editor_warning
    };
    let chart_label = if song.authoring_ready {
        "Chart ready"
    } else if song.editor_ready {
        "Candidate ready"
    } else {
        "Chart incomplete"
    };
    let chart_color = if song.authoring_ready || song.editor_ready {
        theme.primary
    } else {
        theme.muted_foreground
    };
    let subtitle = if song.album.is_empty() {
        song.artist.clone()
    } else {
        format!("{} · {}", song.artist, song.album)
    };
    let transcript = song
        .transcript_source
        .as_ref()
        .map(|source| format!("{source:?}"))
        .unwrap_or_else(|| "Not available".to_string());
    let media_format = song
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("media")
        .to_ascii_uppercase();
    let media = format!(
        "{} · {media_format}",
        if song.is_video { "Video" } else { "Audio" }
    );
    let key = song
        .override_key
        .as_ref()
        .or(song.key.as_ref())
        .cloned()
        .unwrap_or_else(|| "Unknown key".to_string());
    let (next_title, next_detail, next_icon, next_accent) = if primary_label
        == "View processing queue"
    {
        (
            "Analysis is running",
            "Open the processing queue to review the active stage, progress and runtime details.",
            UiIcon::Sparkles,
            theme.primary,
        )
    } else if song.authoring_ready {
        (
            "Ready to edit & export",
            "The authored chart is ready for detailed editing, validation and project export.",
            UiIcon::Scissors,
            theme.pitch_contour,
        )
    } else if song.editor_ready {
        (
            "Review the candidate chart",
            "Open the editor to inspect timing, lyrics and notes before publishing an authored chart.",
            UiIcon::Scissors,
            theme.primary,
        )
    } else if song.is_analyzed {
        (
            "Continue the production path",
            "Analysis evidence exists, but the chart is not ready yet. Queue another run or inspect the workflow.",
            UiIcon::Sparkles,
            theme.editor_warning,
        )
    } else {
        (
            "Analyze this song",
            "Generate lyrics, timing, pitch and candidate-chart evidence while keeping the source file unchanged.",
            UiIcon::Sparkles,
            theme.primary,
        )
    };

    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(18)),
                row_gap: px(14),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.62)),
            BorderColor::all(theme.primary.with_alpha(0.24)),
            studio_card_shadow(theme),
        ))
        .with_children(|hero| {
            hero.spawn(Node {
                width: percent(100),
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Stretch,
                column_gap: px(18),
                row_gap: px(14),
                ..default()
            })
            .with_children(|top| {
                top.spawn((
                    Node {
                        width: px(158),
                        height: px(158),
                        flex_shrink: 0.0,
                        overflow: Overflow::clip(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(14)),
                        ..default()
                    },
                    ImageNode::new(cover),
                    BorderColor::all(theme.border.with_alpha(0.86)),
                    studio_card_shadow(theme),
                ));

                top.spawn(Node {
                    min_width: px(260),
                    flex_basis: px(346),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    row_gap: px(8),
                    ..default()
                })
                .with_children(|copy| {
                    copy.spawn(Node {
                        width: percent(100),
                        align_items: AlignItems::Center,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(7),
                        row_gap: px(6),
                        ..default()
                    })
                    .with_children(|status| {
                        spawn_text(status, font.clone(), "SONG", 7.5, theme.primary);
                        spawn_status_pill(status, font.clone(), analysis_label, analysis_color);
                        spawn_status_pill(status, font.clone(), chart_label, chart_color);
                    });
                    spawn_bounded_wrapped_text(
                        copy,
                        font.clone(),
                        song.title.clone(),
                        27.0,
                        theme.foreground,
                    );
                    spawn_bounded_wrapped_text(
                        copy,
                        font.clone(),
                        subtitle,
                        12.5,
                        theme.muted_foreground,
                    );
                    copy.spawn(Node {
                        width: percent(100),
                        align_items: AlignItems::Center,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(8),
                        row_gap: px(8),
                        ..default()
                    })
                    .with_children(|actions| {
                        spawn_toolbar_button(
                            actions,
                            font.clone(),
                            icons.clone(),
                            theme,
                            if playing { UiIcon::Pause } else { UiIcon::Play },
                            if playing { "Pause" } else { "Play" },
                            play_action,
                            false,
                        );
                        spawn_toolbar_button(
                            actions,
                            font.clone(),
                            icons.clone(),
                            theme,
                            UiIcon::Sparkles,
                            "Workflow",
                            UiAction::from(AnalysisCommand::OpenProcessingStudio(
                                song.file_hash.clone(),
                            )),
                            false,
                        );
                        spawn_toolbar_button(
                            actions,
                            font.clone(),
                            icons.clone(),
                            theme,
                            UiIcon::Settings,
                            "Settings",
                            UiAction::from(EditorCommand::OpenSongSettings(song.file_hash.clone())),
                            false,
                        );
                    });
                });

                spawn_song_next_step(
                    top,
                    font.clone(),
                    icons,
                    theme,
                    next_icon,
                    next_title,
                    next_detail,
                    primary_label,
                    primary_action,
                    next_accent,
                );
            });

            hero.spawn((
                Node {
                    width: percent(100),
                    flex_wrap: FlexWrap::Wrap,
                    padding: UiRect::top(px(14)),
                    column_gap: px(8),
                    row_gap: px(8),
                    border: UiRect::top(px(1)),
                    ..default()
                },
                BorderColor::all(theme.border.with_alpha(0.34)),
            ))
            .with_children(|facts| {
                spawn_song_hero_fact(
                    facts,
                    font.clone(),
                    theme,
                    "DURATION",
                    format_duration(song.duration_secs),
                    theme.foreground,
                );
                spawn_song_hero_fact(
                    facts,
                    font.clone(),
                    theme,
                    "SOURCE",
                    media,
                    theme.foreground,
                );
                spawn_song_hero_fact(
                    facts,
                    font.clone(),
                    theme,
                    "LYRICS",
                    transcript,
                    if song.transcript_source.is_some() {
                        theme.pitch_contour
                    } else {
                        theme.muted_foreground
                    },
                );
                spawn_song_hero_fact(
                    facts,
                    font,
                    theme,
                    "KEY / SPEED",
                    format!("{key} · {:.1}×", song.tempo),
                    theme.foreground,
                );
            });
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_song_detail(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSessionView<'_>,
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
                empty
                    .spawn((
                        Node {
                            width: px(68),
                            height: px(68),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::MAX,
                            margin: UiRect::bottom(px(4)),
                            ..default()
                        },
                        BackgroundColor(theme.primary.with_alpha(0.10)),
                        BorderColor::all(theme.primary.with_alpha(0.32)),
                    ))
                    .with_children(|icon| {
                        spawn_icon(icon, icons.clone(), UiIcon::Music, 28.0, theme.primary);
                    });
                spawn_text(
                    empty,
                    font.clone(),
                    "Choose a song to open its workspace",
                    22.0,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    empty,
                    font.clone(),
                    "Select a track from the library to review its analysis, edit lyrics and chart data, or export a project.",
                    11.0,
                    theme.muted_foreground,
                );
                spawn_compact_primary_action_button(
                    empty,
                    font,
                    theme,
                    "Browse library",
                    UiAction::from(AppCommand::Home),
                );
            });
        return;
    };

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
                .spawn(Node {
                    width: percent(100),
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(22), px(16)),
                    row_gap: px(16),
                    ..default()
                })
                .with_children(|body| {
                    spawn_song_detail_hero(
                        body,
                        font.clone(),
                        icons.clone(),
                        asset_server,
                        images,
                        local_images,
                        session,
                        theme,
                        &song,
                    );
                    spawn_song_workspace_intro(body, font.clone(), theme);
                    body.spawn(Node {
                        width: percent(100),
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        align_items: AlignItems::FlexStart,
                        column_gap: px(16),
                        row_gap: px(16),
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
                        spawn_song_detail_section_card(columns, theme, 400.0, |overview| {
                            spawn_detail_heading_with_action(
                                overview,
                                font.clone(),
                                theme,
                                "OVERVIEW",
                                "Technical details",
                                Some((
                                    "Settings",
                                    UiAction::from(EditorCommand::OpenSongSettings(
                                        song.file_hash.clone(),
                                    )),
                                )),
                            );
                            for (label, value) in song_overview_rows(&song)
                                .into_iter()
                                .filter(|(label, _)| {
                                    matches!(
                                        *label,
                                        "Language"
                                            | "Last successful run"
                                            | "Candidate availability"
                                            | "Chart issues"
                                            | "Detected key"
                                            | "Musical BPM"
                                            | "Extra descriptors"
                                            | "Vocal / instrumental stems"
                                            | "Pitch evidence"
                                    )
                                })
                            {
                                spawn_detail_value(overview, font.clone(), theme, label, value);
                            }
                            spawn_source_file_row(overview, font.clone(), theme, &song.path);
                        });

                        spawn_song_detail_section_card(columns, theme, 500.0, |analysis| {
                            spawn_detail_heading_with_action(
                                analysis,
                                font.clone(),
                                theme,
                                "ANALYSIS",
                                "Analysis",
                                (!song.is_analyzed).then(|| {
                                    (
                                        "Analyze now",
                                        UiAction::from(AnalysisCommand::AnalyzeNow(
                                            song.file_hash.clone(),
                                        )),
                                    )
                                }),
                            );
                            for (label, value) in song_analysis_summary_rows(&song) {
                                spawn_detail_value(analysis, font.clone(), theme, label, value);
                            }
                            spawn_song_detail_action_row(
                                analysis,
                                font.clone(),
                                theme,
                                "Workflow",
                                if song.is_analyzed {
                                    "Analyzed. Open the node-graph workflow to inspect or customize how this song's stages are configured."
                                } else {
                                    "Not analyzed yet. Open the node-graph workflow to customize stages before running, instead of the one-click default."
                                },
                                Some((
                                    "Open workflow",
                                    UiAction::from(AnalysisCommand::OpenProcessingStudio(
                                        song.file_hash.clone(),
                                    )),
                                )),
                            );
                            spawn_song_detail_action_row(
                                analysis,
                                font.clone(),
                                theme,
                                "Processing queue",
                                "Add this song to the processing queue using its configured analysis workflow.",
                                Some(song_analysis_action(&song)),
                            );
                            let song_profile =
                                app_core::get_song_analysis_profile(&song.file_hash);
                            spawn_song_detail_action_row(
                                analysis,
                                font.clone(),
                                theme,
                                "Analysis profile",
                                analysis_profile_summary(
                                    session.config.analysis_quality(),
                                    session.config.analysis_default_target(),
                                    song_profile
                                        .as_ref()
                                        .map(|profile| &profile.analysis_experience),
                                ),
                                Some((
                                    "Environment settings",
                                    UiAction::from(SettingsCommand::SettingsTab(
                                        SettingsTab::Analysis,
                                    )),
                                )),
                            );
                            if analyzed_and_native
                                && matches!(
                                    app_core::candidate_chart_status(&song.file_hash),
                                    app_core::CandidateChartStatus::CandidateAvailable(_)
                                )
                            {
                                spawn_song_detail_action_row(
                                    analysis,
                                    font.clone(),
                                    theme,
                                    "Candidate analysis",
                                    "A newer analysis result differs from your saved chart. Compare and choose whether to replace it.",
                                    Some((
                                        "Compare & replace…",
                                        UiAction::from(AnalysisCommand::RequestReplaceAuthoredChart(
                                            song.file_hash.clone(),
                                        )),
                                    )),
                                );
                            }
                        });

                        spawn_song_detail_section_card(columns, theme, 400.0, |lyrics| {
                            spawn_detail_heading(
                                lyrics,
                                font.clone(),
                                theme,
                                "LYRICS & TIMING",
                                "Lyrics & timing",
                            );
                            spawn_song_detail_action_row(
                                lyrics,
                                font.clone(),
                                theme,
                                "Lyrics",
                                "Edit plain lyrics or provide timed LRC without replacing source media or starting analysis.",
                                if native_source {
                                    Some((
                                        "Edit lyrics…".to_string(),
                                        UiAction::from(EditorCommand::OpenLyricsEditor(song.file_hash.clone())),
                                    ))
                                } else {
                                    None
                                },
                            );
                            if native_source {
                                spawn_song_detail_action_row(
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
                                        UiAction::from(EditorCommand::OpenLanguageEditor(song.file_hash.clone())),
                                    )),
                                );
                            }
                        });

                        spawn_song_detail_section_card(columns, theme, 400.0, |audio| {
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
                                spawn_song_detail_shift_row(
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
                                    UiAction::from(EditorCommand::ShiftSongKey(song.file_hash.clone(), -1)),
                                    UiAction::from(EditorCommand::ShiftSongKey(song.file_hash.clone(), 1)),
                                );
                                // This is the export-speed multiplier, not
                                // the detected Musical BPM (shown in Song
                                // Settings) -- kept explicitly out of
                                // "Tempo"/"BPM" territory.
                                spawn_song_detail_shift_row(
                                    audio,
                                    font.clone(),
                                    theme,
                                    "Playback / export speed",
                                    "Create an export-speed variant in 0.1× steps.",
                                    format!("{:.1}×", song.tempo),
                                    UiAction::from(EditorCommand::ShiftSongTempo(song.file_hash.clone(), -1)),
                                    UiAction::from(EditorCommand::ShiftSongTempo(song.file_hash.clone(), 1)),
                                );
                            } else {
                                spawn_song_detail_action_row(
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
                        spawn_song_detail_section_card(columns, theme, 500.0, |authoring| {
                            spawn_detail_heading(
                                authoring,
                                font.clone(),
                                theme,
                                "AUTHORING",
                                "Authoring & export",
                            );
                            spawn_song_detail_action_row(
                                authoring,
                                font.clone(),
                                theme,
                                "Chart editor",
                                if song.editor_ready {
                                    "Open the active authored or candidate chart for detailed note and lyric editing."
                                } else {
                                    song.editor_blocked_reason.as_deref().unwrap_or(
                                        "A complete chart and playable audio are required before editing.",
                                    )
                                },
                                Some((
                                    "Open editor",
                                    UiAction::from(LibraryCommand::OpenEditor(
                                        song.file_hash.clone(),
                                    )),
                                )),
                            );
                            if !matches!(
                                app_core::candidate_chart_status(&song.file_hash),
                                app_core::CandidateChartStatus::NotAuthoredYet
                            ) {
                                spawn_song_detail_action_row(
                                    authoring,
                                    font.clone(),
                                    theme,
                                    "Authored chart",
                                    "Delete only the authored chart. Source media, CandidateChart and analysis evidence are retained.",
                                    Some((
                                        "Delete chart…",
                                        UiAction::from(
                                            AnalysisCommand::RequestDeleteAuthoredChart(
                                                song.file_hash.clone(),
                                            ),
                                        ),
                                    )),
                                );
                            }
                            if song.authoring_ready {
                                spawn_song_detail_action_row(
                                    authoring,
                                    font.clone(),
                                    theme,
                                    "UTZ project",
                                    "Export the full editable project file for Uta! Studio.",
                                    Some((
                                        "Export UTZ",
                                        UiAction::from(LibraryCommand::ExportUtz(song.file_hash.clone())),
                                    )),
                                );
                                spawn_song_detail_action_row(
                                    authoring,
                                    font.clone(),
                                    theme,
                                    "UltraStar chart",
                                    "Export a chart compatible with UltraStar-format karaoke games.",
                                    Some((
                                        "Export UltraStar",
                                        UiAction::from(LibraryCommand::ExportUltraStar(song.file_hash.clone())),
                                    )),
                                );
                            } else {
                                spawn_song_detail_action_row(
                                    authoring,
                                    font.clone(),
                                    theme,
                                    "Export",
                                    "Export becomes available once this song's chart is ready for authoring.",
                                    None::<(&'static str, UiAction)>,
                                );
                            }
                        });

                        spawn_song_detail_section_card(columns, theme, 400.0, |history| {
                            spawn_detail_heading(
                                history,
                                font.clone(),
                                theme,
                                "ARTIFACTS & HISTORY",
                                "Artifacts & history",
                            );
                            if analyzed_and_native {
                                spawn_song_detail_action_row(
                                    history,
                                    font.clone(),
                                    theme,
                                    "Generated song data",
                                    "Delete generated cache for this song. Source media is never changed.",
                                    Some((
                                        "Delete cache…",
                                        UiAction::from(AnalysisCommand::RequestDeleteSongCache(song.file_hash.clone())),
                                    )),
                                );
                            } else {
                                spawn_song_detail_action_row(
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
                    session.config,
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
    config: &AppConfig,
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
                    localized_message(
                        config,
                        UiMessage::AudioWaveform,
                        &[(
                            "{duration}",
                            &format_duration(editor.waveform.duration_secs),
                        )],
                    ),
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
                    spawn_text_button(row, font.clone(), theme, format!("▶ {start:.3}s"), 8.0, UiAction::from(EditorCommand::PreviewTranscriptAt(editor.file_hash.clone(), (start * 1000.0).round() as i64)));
                    for (label, edge, delta) in [
                        ("start −10", TranscriptBoundaryEdge::Start, -10),
                        ("start +10", TranscriptBoundaryEdge::Start, 10),
                        ("end −10", TranscriptBoundaryEdge::End, -10),
                        ("end +10", TranscriptBoundaryEdge::End, 10),
                    ] {
                        spawn_text_button(row, font.clone(), theme, label, 7.5, UiAction::from(EditorCommand::AdjustTranscriptBoundary(TranscriptBoundaryTarget::Segment(segment_index), edge, delta)));
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
                            spawn_text_button(row, font.clone(), theme, format!("▶ {start:.3}s"), 7.5, UiAction::from(EditorCommand::PreviewTranscriptAt(editor.file_hash.clone(), (start * 1000.0).round() as i64)));
                            for (label, edge, delta) in [
                                ("S−", TranscriptBoundaryEdge::Start, -10),
                                ("S+", TranscriptBoundaryEdge::Start, 10),
                                ("E−", TranscriptBoundaryEdge::End, -10),
                                ("E+", TranscriptBoundaryEdge::End, 10),
                            ] {
                                spawn_text_button(row, font.clone(), theme, label, 7.0, UiAction::from(EditorCommand::AdjustTranscriptBoundary(TranscriptBoundaryTarget::Word { segment: segment_index, word: word_index }, edge, delta)));
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
            UiPointerApi(&["ui.pointer.transcript_boundary_drag"]),
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
                  mut shell: ResMut<ShellState>,
                  mut dialogs: ResMut<DialogState>,
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
                            &shell.config,
                        )?;
                        let rendered = serde_json::to_string_pretty(&value).unwrap_or_default();
                        let editor = dialogs
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
                shell.notice = result.err();
            },
        )
        .observe(
            |mut release: On<Pointer<DragEnd>>, mut invalidated: ResMut<UiInvalidated>| {
                release.propagate(false);
                invalidated.invalidate(UiDirtyRegion::Library);
            },
        )
        .observe(
            |mut cancel: On<Pointer<Cancel>>, mut invalidated: ResMut<UiInvalidated>| {
                cancel.propagate(false);
                invalidated.invalidate(UiDirtyRegion::Library);
            },
        );
}

pub(crate) fn spawn_lyrics_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    config: &AppConfig,
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
                            "Paste line-level or enhanced LRC. Its line timestamps are treated as authoritative. Already-analyzed songs keep their existing stems; new songs are authored directly over the original mix."
                        } else {
                            "Enter one lyric phrase per line. Saving stores the lyrics without starting analysis or modifying the source song."
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
                                UiAction::from(EditorCommand::ToggleLyricsInputMode),
                            );
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
                                UiAction::from(EditorCommand::SearchLrclibLyrics),
                            );
                            spawn_text_button(
                                options,
                                font.clone(),
                                theme,
                                "Extract lyrics",
                                10.0,
                                UiAction::from(EditorCommand::ExtractLyrics),
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
                                                UiAction::from(EditorCommand::PreviousLrclibCandidate),
                                            );
                                            spawn_text_button(
                                                header,
                                                font.clone(),
                                                theme,
                                                "Next",
                                                9.0,
                                                UiAction::from(EditorCommand::NextLrclibCandidate),
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
                                                UiAction::from(EditorCommand::UseLrclibTimed),
                                            );
                                        }
                                        if !candidate.lines.is_empty() {
                                            spawn_text_button(
                                                actions,
                                                font.clone(),
                                                theme,
                                                "Use as plain lyrics",
                                                9.0,
                                                UiAction::from(EditorCommand::UseLrclibPlain),
                                            );
                                        }
                                    });
                            });
                    }
                    if editor.mode == LyricsInputMode::StructuredTimedTranscript {
                        spawn_timed_transcript_structure(
                            dialog,
                            font.clone(),
                            config,
                            theme,
                            editor,
                        );
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
                                UiAction::from(EditorCommand::CloseLyricsEditor),
                            );
                            if editor.artifact_draft.is_some() {
                                spawn_action_button(
                                    actions,
                                    font,
                                    theme,
                                    "Save",
                                    UiAction::from(EditorCommand::SaveLyricsEditor),
                                );
                            } else {
                                spawn_action_button(
                                    actions,
                                    font,
                                    theme,
                                    "Save lyrics",
                                    UiAction::from(EditorCommand::SaveLyricsEditor),
                                );
                            }
                        });
                });
        });
}

#[cfg(test)]
mod analysis_profile_summary_tests {
    use super::analysis_profile_summary;

    #[test]
    fn song_detail_labels_global_and_song_sources_explicitly() {
        let global = analysis_profile_summary(
            app_core::AnalysisQualityProfile::Balanced,
            app_core::AnalysisDefaultTarget::FullCandidate,
            None,
        );
        assert!(global.contains("Balanced"));
        assert!(global.contains("Candidate chart"));
        assert_eq!(global.matches("Source: GLOBAL").count(), 2);
        assert!(global.contains("only after re-analysis"));

        let override_settings = app_core::AnalysisExperienceOverride {
            quality_profile: Some(app_core::AnalysisQualityProfile::Maximum),
            default_target: Some(app_core::AnalysisDefaultTarget::PitchEvidence),
            ..app_core::AnalysisExperienceOverride::default()
        };
        let song = analysis_profile_summary(
            app_core::AnalysisQualityProfile::Balanced,
            app_core::AnalysisDefaultTarget::FullCandidate,
            Some(&override_settings),
        );
        assert!(song.contains("Maximum"));
        assert!(song.contains("PitchEvidence"));
        assert_eq!(song.matches("Source: SONG").count(), 2);
    }
}
