use super::*;
use crate::studio::*;

pub(crate) fn analysis_graph_phase_accent(kind: LayoutLaneKind, theme: &StudioTheme) -> Color {
    match (theme.dark, kind) {
        (true, LayoutLaneKind::Preparation) => Color::srgb(0.38, 0.66, 1.0),
        (true, LayoutLaneKind::Music) => Color::srgb(0.25, 0.84, 0.80),
        (true, LayoutLaneKind::VocalsAndPitch) => Color::srgb(0.78, 0.50, 0.93),
        (true, LayoutLaneKind::LyricsAndTiming) => Color::srgb(0.96, 0.68, 0.18),
        (true, LayoutLaneKind::AuthoringAndOutput) => Color::srgb(0.46, 0.86, 0.40),
        (false, LayoutLaneKind::Preparation) => Color::srgb(0.16, 0.42, 0.76),
        (false, LayoutLaneKind::Music) => Color::srgb(0.05, 0.52, 0.50),
        (false, LayoutLaneKind::VocalsAndPitch) => Color::srgb(0.55, 0.25, 0.70),
        (false, LayoutLaneKind::LyricsAndTiming) => Color::srgb(0.72, 0.42, 0.02),
        (false, LayoutLaneKind::AuthoringAndOutput) => Color::srgb(0.22, 0.58, 0.18),
    }
}

pub(crate) fn analysis_graph_node_phase(node_id: &str) -> LayoutLaneKind {
    if node_id == "preflight" {
        LayoutLaneKind::Preparation
    } else if node_id.starts_with("music.") || node_id == "artifact.music_analysis" {
        LayoutLaneKind::Music
    } else if node_id.starts_with("lyrics.")
        || matches!(
            node_id,
            "artifact.lyrics"
                | "artifact.lyrics_input"
                | "artifact.recognized_text"
                | "artifact.timed_lyrics"
        )
    {
        LayoutLaneKind::LyricsAndTiming
    } else if node_id == "chart.build_candidate"
        || node_id == "artifact.chart"
        || node_id.starts_with("export.")
    {
        LayoutLaneKind::AuthoringAndOutput
    } else {
        LayoutLaneKind::VocalsAndPitch
    }
}

pub(crate) fn analysis_graph_node_accent(node_id: &str, theme: &StudioTheme) -> Color {
    analysis_graph_phase_accent(analysis_graph_node_phase(node_id), theme)
}

/// Short overview copy. Full backend/model diagnostics belong in Inspect;
/// rendering them inside a fitted card makes the DAG impossible to scan.
pub(crate) fn analysis_graph_node_detail<'a>(node_id: &str, fallback: &'a str) -> &'a str {
    match node_id {
        "preflight" => "Source check",
        "music.analysis" => "Key · BPM",
        "stems.separate" | "stems.vocals" => "Vocal separation",
        "vocals.denoise" => "Noise reduction",
        "vocals.dereverb" => "Reverb reduction",
        "stems.instrumental" => "BGM separation",
        "instrumental.denoise" => "BGM noise reduction",
        "instrumental.dereverb" => "BGM reverb reduction",
        "stems.bind_analysis_outputs" => "Analysis-ready audio",
        "pitch.extract" => "Pitch contour",
        "lyrics.preprocess" => "Audio preparation",
        "lyrics.transcribe" => "Speech to text",
        "lyrics.align" => "Timing alignment",
        "lyrics.import_timed" => "Text and timing",
        "chart.build_candidate" => "Build Chart",
        "artifact.music_analysis" => "Key · BPM",
        "artifact.raw_vocal" => "Extracted vocal",
        "artifact.denoised_vocal" => "Denoised vocal",
        "artifact.dereverbed_vocal" => "Dereverbed vocal",
        "artifact.raw_instrumental" => "Separated BGM",
        "artifact.denoised_instrumental" => "Denoised BGM",
        "artifact.dereverbed_instrumental" => "Dereverbed BGM",
        "artifact.vocal_stem" => "Analysis vocal",
        "artifact.note_guide" => "Pitch contour",
        "artifact.lyrics_input" => "Source lyrics",
        "artifact.lyrics" => "Normalized text",
        "artifact.timed_lyrics" => "Text and timing",
        "artifact.chart" => "Editable chart",
        _ => fallback,
    }
}

fn analysis_graph_model_label(node_id: &str, model_id: &str, config: &AppConfig) -> Option<String> {
    let model_id = model_id.trim();
    if model_id.is_empty() || model_id == "default" {
        return None;
    }
    let kind = match node_id {
        "stems.vocals" => SettingsSelectKind::AudioVocalModel,
        "stems.instrumental" => SettingsSelectKind::AudioAccompanimentModel,
        "stems.karaoke" => SettingsSelectKind::AudioKaraokeModel,
        "vocals.denoise" => SettingsSelectKind::AudioVocalPostprocess1,
        "vocals.dereverb" => SettingsSelectKind::AudioVocalPostprocess2,
        "instrumental.denoise" => SettingsSelectKind::AudioBgmPostprocess1,
        "instrumental.dereverb" => SettingsSelectKind::AudioBgmPostprocess2,
        "pitch.extract" => SettingsSelectKind::PitchModel,
        "lyrics.transcribe" => SettingsSelectKind::WhisperModel,
        "lyrics.align" => SettingsSelectKind::AlignBackend,
        _ => return Some(model_id.to_string()),
    };
    let label = settings_select_label(kind, model_id);
    if label == "Off" {
        return None;
    }
    let _ = config;
    Some(compact_analysis_model_label(label).to_string())
}

fn compact_analysis_model_label(label: &str) -> &str {
    match label {
        "BS-RoFormer Vocals EP317" => "BS-RoFormer · EP317",
        "Default karaoke (aufr33 + viperx)" => "MelBand Karaoke",
        "OpenVINO native worker" => "OpenVINO native worker",
        "MelBand-RoFormer Denoise" => "MelBand Denoise",
        "MelBand-RoFormer Dereverb" => "MelBand Dereverb",
        "MMS Karaoke (Japanese)" => "MMS Karaoke · JA",
        "CTC Forced Alignment" => "CTC Align",
        "Qwen Forced Alignment" => "Qwen Align",
        _ => label,
    }
}

pub(crate) fn analysis_graph_configured_model_tag(
    node_id: &str,
    config: &AppConfig,
) -> Option<String> {
    let audio = audio_settings(config);
    match node_id {
        "preflight" => Some("Source check".to_string()),
        "music.analysis" => Some("Essentia Key/BPM".to_string()),
        "stems.vocals" => {
            Some(compact_analysis_model_label(vocal_separation_label(config)).to_string())
        }
        "stems.karaoke" => audio
            .karaoke_model_id
            .as_deref()
            .and_then(|model| analysis_graph_model_label(node_id, model, config)),
        "stems.instrumental" => audio
            .accompaniment_model_id
            .as_deref()
            .and_then(|model| analysis_graph_model_label(node_id, model, config)),
        "vocals.denoise" => audio
            .vocal_cleanup_chain
            .iter()
            .find(|model| model.contains("denoise"))
            .and_then(|model| analysis_graph_model_label(node_id, model, config)),
        "vocals.dereverb" => audio
            .vocal_cleanup_chain
            .iter()
            .find(|model| model.contains("dereverb"))
            .and_then(|model| analysis_graph_model_label(node_id, model, config)),
        "instrumental.denoise" => audio
            .accompaniment_cleanup_chain
            .iter()
            .find(|model| model.contains("denoise"))
            .and_then(|model| analysis_graph_model_label(node_id, model, config)),
        "instrumental.dereverb" => audio
            .accompaniment_cleanup_chain
            .iter()
            .find(|model| model.contains("dereverb"))
            .and_then(|model| analysis_graph_model_label(node_id, model, config)),
        "pitch.extract" => Some(pitch_model_label(config.pitch_model()).to_string()),
        "lyrics.preprocess" => Some("Audio preparation".to_string()),
        "lyrics.transcribe" => Some(format!(
            "{} {}",
            asr_engine_label(config.asr_engine()),
            settings_select_label(SettingsSelectKind::WhisperModel, config.whisper_model())
        )),
        "lyrics.align" => Some(
            compact_analysis_model_label(align_backend_label(config.align_backend())).to_string(),
        ),
        "lyrics.import_timed" => Some("Text timing".to_string()),
        "chart.build_candidate" => Some("AutoChart v2".to_string()),
        _ => None,
    }
}

pub(crate) fn analysis_graph_node_model_tag(
    node_id: &str,
    runtime_route: Option<&str>,
    planned_detail: &str,
    config: &AppConfig,
) -> String {
    if matches!(
        node_id,
        "preflight" | "music.analysis" | "lyrics.preprocess" | "chart.build_candidate"
    ) {
        return analysis_graph_configured_model_tag(node_id, config)
            .unwrap_or_else(|| analysis_graph_node_detail(node_id, "Configured step").to_string());
    }
    if let Some(route) = runtime_route {
        let model = route
            .rsplit_once(" · ")
            .map(|(_, model)| model)
            .unwrap_or(route);
        if let Some(label) = analysis_graph_model_label(node_id, model, config) {
            return label;
        }
    }
    if !planned_detail.is_empty() {
        let model = planned_detail
            .split_once(" · ")
            .map(|(model, _)| model)
            .unwrap_or(planned_detail);
        if let Some(label) = analysis_graph_model_label(node_id, model, config) {
            return label;
        }
    }
    analysis_graph_configured_model_tag(node_id, config)
        .unwrap_or_else(|| analysis_graph_node_detail(node_id, "Configured step").to_string())
}

pub(crate) fn analysis_graph_node_label<'a>(node_id: &str, fallback: &'a str) -> &'a str {
    match node_id {
        "stems.vocals" => "Extract",
        "vocals.denoise" => "Denoise",
        "vocals.dereverb" => "Dereverb",
        "stems.instrumental" => "BGM Extract",
        "instrumental.denoise" => "BGM Denoise",
        "instrumental.dereverb" => "BGM Dereverb",
        "pitch.extract" => "Pitch",
        "lyrics.preprocess" => "Prep",
        "lyrics.transcribe" => "Transcribe",
        "lyrics.align" => "Align",
        "lyrics.import_timed" => "Timing",
        "chart.build_candidate" => "Chart",
        "artifact.music_analysis" => "Music Data",
        "artifact.raw_vocal" | "artifact.raw_instrumental" => "Raw",
        "artifact.denoised_vocal" | "artifact.denoised_instrumental" => "Clean",
        "artifact.dereverbed_vocal" | "artifact.dereverbed_instrumental" => "Dry",
        "artifact.vocal_stem" => "vocals",
        _ => fallback,
    }
}

/// Keep fitted and overview-scale cards readable. These labels describe the
/// same nodes as the full titles; they only remove redundant words and raw
/// artifact filenames when a card has less than its normal visual width.
pub(crate) fn analysis_graph_node_display_label<'a>(
    node_id: &str,
    fallback: &'a str,
    zoom: f32,
) -> &'a str {
    if zoom >= 0.72 {
        return analysis_graph_node_label(node_id, fallback);
    }
    match node_id {
        "music.analysis" => "Music",
        "stems.vocals" => "Extract",
        "vocals.denoise" => "Denoise",
        "vocals.dereverb" => "Dereverb",
        "stems.instrumental" => "BGM",
        "instrumental.denoise" => "BGM Denoise",
        "instrumental.dereverb" => "BGM Dereverb",
        "stems.bind_analysis_outputs" => "Vocal",
        "pitch.extract" => "Pitch",
        "lyrics.preprocess" => "Prep",
        "lyrics.transcribe" => "Transcribe",
        "lyrics.align" => "Align",
        "lyrics.import_timed" => "Timed Lyrics",
        "chart.build_candidate" => "Chart",
        "artifact.music_analysis" => "Music Data",
        "artifact.raw_vocal" => "Raw Vocal",
        "artifact.denoised_vocal" => "Denoised",
        "artifact.dereverbed_vocal" => "Dry Vocal",
        "artifact.raw_instrumental" => "Raw BGM",
        "artifact.denoised_instrumental" => "Denoised BGM",
        "artifact.dereverbed_instrumental" => "Dry BGM",
        "artifact.vocal_stem" => "Vocals",
        "artifact.note_guide" => "Pitch Guide",
        "artifact.lyrics_input" => "Lyrics Input",
        "artifact.lyrics" => "Lyrics",
        "artifact.timed_lyrics" => "Timing",
        "artifact.chart" => "Chart",
        _ => analysis_graph_node_label(node_id, fallback),
    }
}

pub(crate) fn spawn_analysis_graph_status_pill(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    text: String,
    color: Color,
    zoom: f32,
) {
    spawn_analysis_graph_status_pill_at(parent, font, text, color, zoom, 7.0);
}

/// Keep controls readable while zooming out, but scale them together with
/// their node while zooming in. Capping at the 100% size made enlarged nodes
/// look empty and left their status/model rows behind at stale coordinates.
pub(crate) fn analysis_graph_scaled(base: f32, minimum: f32, zoom: f32) -> f32 {
    (base * zoom).max(minimum)
}

fn spawn_analysis_graph_status_pill_at(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    text: String,
    color: Color,
    zoom: f32,
    bottom: f32,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(analysis_graph_scaled(bottom, 4.5, zoom)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(
                        px(analysis_graph_scaled(6.0, 4.0, zoom)),
                        px(analysis_graph_scaled(2.5, 1.5, zoom)),
                    ),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(color.with_alpha(0.16)),
            ))
            .with_children(|pill| {
                spawn_text(
                    pill,
                    font,
                    text,
                    analysis_graph_scaled(7.5, 6.4, zoom),
                    color,
                );
            });
        });
}

fn spawn_analysis_graph_model_tag(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    route: &str,
    accent: Color,
    zoom: f32,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(analysis_graph_scaled(6.0, 4.0, zoom)),
                right: px(analysis_graph_scaled(6.0, 4.0, zoom)),
                bottom: px(analysis_graph_scaled(5.0, 3.0, zoom)),
                height: px(analysis_graph_scaled(21.0, 15.0, zoom)),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(analysis_graph_scaled(6.0, 4.0, zoom))),
                overflow: Overflow::clip(),
                border: UiRect::all(px(analysis_graph_scaled(1.0, 0.7, zoom))),
                border_radius: BorderRadius::all(px(analysis_graph_scaled(4.0, 3.0, zoom))),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.30)),
            BorderColor::all(accent.with_alpha(0.42)),
        ))
        .with_children(|tag| {
            tag.spawn((
                Text::new(route),
                ui_text_font(font, analysis_graph_scaled(7.2, 6.2, zoom)),
                TextColor(theme.muted_foreground),
                TextLayout::no_wrap(),
            ));
        });
}

pub(crate) fn spawn_analysis_graph_lane_band(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    band: LayoutLaneBand,
    zoom: f32,
) {
    let bounds = zoomed_box(band.rect, zoom);
    let accent = analysis_graph_phase_accent(band.kind, theme);
    let border_width = analysis_graph_scaled(1.0, 0.6, zoom);
    let header_height = 42.0 * zoom;
    let icon = match band.kind {
        LayoutLaneKind::Preparation => "↗",
        LayoutLaneKind::Music => "♪",
        LayoutLaneKind::VocalsAndPitch => "●",
        LayoutLaneKind::LyricsAndTiming => "A",
        LayoutLaneKind::AuthoringAndOutput => "+",
    };
    let label = if zoom < 0.60 {
        match band.kind {
            LayoutLaneKind::Preparation => "PREP",
            LayoutLaneKind::Music => "MUSIC",
            LayoutLaneKind::VocalsAndPitch => "STEMS",
            LayoutLaneKind::LyricsAndTiming => "LYRICS",
            LayoutLaneKind::AuthoringAndOutput => "OUTPUT",
        }
    } else {
        band.kind.label()
    };
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(bounds.x),
                top: px(bounds.y),
                width: px(bounds.width),
                height: px(bounds.height),
                min_width: px(0),
                min_height: px(0),
                padding: UiRect::axes(px(13.0 * zoom), px(10.0 * zoom)),
                border: UiRect::all(px(border_width)),
                border_radius: BorderRadius::all(px(analysis_graph_scaled(7.0, 5.0, zoom))),
                ..default()
            },
            BackgroundColor(accent.with_alpha(if theme.dark { 0.035 } else { 0.045 })),
            BorderColor::all(accent.with_alpha(0.24)),
            ZIndex(-1),
            Pickable::IGNORE,
        ))
        .with_children(|lane| {
            lane.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(header_height),
                    height: px(border_width),
                    ..default()
                },
                BackgroundColor(accent.with_alpha(0.30)),
                Pickable::IGNORE,
            ));
            lane.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(0),
                    height: px(header_height),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|header| {
                spawn_text(
                    header,
                    font,
                    format!("{icon}  {label}"),
                    analysis_graph_scaled(11.5, 8.5, zoom),
                    accent.with_alpha(0.98),
                );
            });
        });
}

fn spawn_analysis_graph_legend_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    detail: &str,
    accent: Color,
    round: bool,
) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(7),
            ..default()
        })
        .with_children(|item| {
            item.spawn((
                Node {
                    width: px(16),
                    height: px(16),
                    flex_shrink: 0.0,
                    border: UiRect::all(px(1)),
                    border_radius: if round {
                        BorderRadius::MAX
                    } else {
                        BorderRadius::all(px(4))
                    },
                    ..default()
                },
                BackgroundColor(accent.with_alpha(0.14)),
                BorderColor::all(accent.with_alpha(0.82)),
            ));
            item.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(1),
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 7.0, theme.foreground);
                spawn_text(copy, font, detail, 5.8, theme.muted_foreground);
            });
        });
}

pub(crate) fn spawn_analysis_graph_legend(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(48),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: px(22),
                flex_wrap: FlexWrap::Wrap,
                row_gap: px(7),
                padding: UiRect::axes(px(12), px(7)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.30)),
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|legend| {
            spawn_text(legend, font.clone(), "STATUS", 6.5, theme.muted_foreground);
            spawn_analysis_graph_legend_item(
                legend,
                font.clone(),
                theme,
                "Complete",
                "Finished successfully",
                analysis_graph_phase_accent(LayoutLaneKind::AuthoringAndOutput, theme),
                true,
            );
            spawn_analysis_graph_legend_item(
                legend,
                font.clone(),
                theme,
                "Waiting",
                "Pending execution",
                theme.muted_foreground,
                true,
            );
            spawn_text(
                legend,
                font.clone(),
                "NODE TYPES",
                6.5,
                theme.muted_foreground,
            );
            spawn_analysis_graph_legend_item(
                legend,
                font.clone(),
                theme,
                "Processing step",
                "Computational task",
                analysis_graph_phase_accent(LayoutLaneKind::VocalsAndPitch, theme),
                false,
            );
            spawn_analysis_graph_legend_item(
                legend,
                font.clone(),
                theme,
                "Artifact",
                "Intermediate data",
                theme.muted_foreground,
                false,
            );
            spawn_analysis_graph_legend_item(
                legend,
                font.clone(),
                theme,
                "Authoring step",
                "Chart creation",
                analysis_graph_phase_accent(LayoutLaneKind::AuthoringAndOutput, theme),
                false,
            );
            spawn_analysis_graph_legend_item(
                legend,
                font,
                theme,
                "Final output",
                "Deliverable",
                analysis_graph_phase_accent(LayoutLaneKind::AuthoringAndOutput, theme),
                false,
            );
        });
}

pub(crate) fn analysis_graph_route_summary(
    task: &app_core::AnalysisTask,
    node_id: &str,
    stage_id: &str,
    completed: bool,
) -> (String, bool) {
    let route = task
        .live
        .as_ref()
        .and_then(|live| find_matching_route(&live.stage_routes, node_id, stage_id));
    let Some(route) = route else {
        return (
            if completed {
                "Complete · no runtime trace".to_string()
            } else {
                "Awaiting connected inputs".to_string()
            },
            false,
        );
    };
    let warning = route.fallback_from.is_some() || route.backend_fallback_from.is_some();
    let implementation = route
        .backend_fallback_from
        .as_ref()
        .map(|from| {
            format!(
                "{} > {}",
                from.to_ascii_uppercase(),
                route.implementation.to_ascii_uppercase()
            )
        })
        .unwrap_or_else(|| route.implementation.clone());
    let model = if !route.model.trim().is_empty() {
        route.model.as_str()
    } else {
        "default"
    };
    (format!("{implementation} · {model}"), warning)
}

pub(crate) struct AnalysisStageNodeSpec<'a> {
    pub(crate) bounds: AnalysisGraphBox,
    pub(crate) stage_id: &'a str,
    pub(crate) node_id: &'a str,
    pub(crate) file_hash: &'a str,
    pub(crate) label: &'a str,
    pub(crate) state: AnalysisGraphStageState,
    pub(crate) selected: bool,
    pub(crate) route: &'a str,
    pub(crate) warning: bool,
    pub(crate) dimmed: bool,
    pub(crate) zoom: f32,
}

pub(crate) fn spawn_analysis_stage_node(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    spec: AnalysisStageNodeSpec,
) {
    let AnalysisStageNodeSpec {
        bounds,
        stage_id,
        node_id,
        file_hash,
        label,
        state,
        selected,
        route,
        warning,
        dimmed,
        zoom,
    } = spec;
    let accent = analysis_graph_node_accent(node_id, theme);
    let padding = analysis_graph_scaled(8.0, 5.5, zoom);
    let gap = analysis_graph_scaled(4.0, 2.5, zoom);
    let title_size = analysis_graph_scaled(10.5, 8.0, zoom);
    let meta_size = analysis_graph_scaled(7.5, 6.4, zoom);
    let glyph = match node_id {
        "preflight" => "↗",
        value if value.starts_with("music.") => "♫",
        "pitch.extract" => "∿",
        value if value.starts_with("lyrics.") => "A",
        "chart.build_candidate" => "+",
        _ => "~",
    };
    let complete_color = analysis_graph_phase_accent(LayoutLaneKind::AuthoringAndOutput, theme);
    let (status, progress, status_color, status_glyph) = match state {
        AnalysisGraphStageState::Waiting => ("WAITING", 0, theme.muted_foreground, "○"),
        AnalysisGraphStageState::Running(progress) => ("RUNNING", progress, accent, "●"),
        AnalysisGraphStageState::Complete => ("COMPLETE", 100, complete_color, "✓"),
    };
    let running = matches!(state, AnalysisGraphStageState::Running(_));
    let complete = matches!(state, AnalysisGraphStageState::Complete);
    let context_node_id = node_id.to_string();
    let context_stage_id = stage_id.to_string();
    let context_file_hash = file_hash.to_string();
    let context_label = label.to_string();
    parent
        .spawn((
            Button,
            UiPointerApi(&[
                "ui.pointer.analysis_node.primary",
                "ui.pointer.analysis_node.secondary",
            ]),
            Node {
                position_type: PositionType::Absolute,
                left: px(bounds.x),
                top: px(bounds.y),
                width: px(bounds.width),
                height: px(bounds.height),
                flex_direction: FlexDirection::Column,
                padding: UiRect {
                    left: px(padding),
                    right: px(padding),
                    top: px(padding),
                    bottom: px(analysis_graph_scaled(28.0, 19.0, zoom)),
                },
                row_gap: px(gap),
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(analysis_graph_scaled(8.0, 5.0, zoom))),
                ..default()
            },
            BackgroundColor(if running {
                accent.with_alpha(if dimmed { 0.06 } else { 0.15 })
            } else if selected {
                theme.card.with_alpha(if dimmed { 0.35 } else { 0.96 })
            } else {
                theme.card.with_alpha(if dimmed { 0.22 } else { 0.90 })
            }),
            BorderColor::all(if selected {
                accent.with_alpha(0.96)
            } else if running {
                accent.with_alpha(if dimmed { 0.22 } else { 0.72 })
            } else if complete {
                accent.with_alpha(if dimmed { 0.16 } else { 0.70 })
            } else {
                accent.with_alpha(if dimmed { 0.18 } else { 0.46 })
            }),
            BoxShadow::new(
                accent.with_alpha(if dimmed {
                    0.0
                } else if running {
                    0.52
                } else if selected {
                    0.20
                } else {
                    0.045
                }),
                px(0),
                px(0),
                px(if running {
                    analysis_graph_scaled(20.0, 12.0, zoom)
                } else {
                    analysis_graph_scaled(8.0, 5.0, zoom)
                }),
                px(if running { (2.0 * zoom).max(1.0) } else { 0.0 }),
            ),
            ZIndex(2),
        ))
        .with_children(|node| {
            spawn_analysis_graph_ports(node, theme, complete || running, zoom);
            node.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(analysis_graph_scaled(7.0, 4.0, zoom)),
                    right: px(analysis_graph_scaled(7.0, 4.0, zoom)),
                    top: px(0),
                    height: px((1.0 * zoom).max(0.7)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(accent.with_alpha(if running {
                    0.92
                } else if complete {
                    0.46
                } else {
                    0.24
                })),
                Pickable::IGNORE,
            ));
            if selected {
                node.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(analysis_graph_scaled(9.0, 5.0, zoom)),
                        bottom: px(analysis_graph_scaled(9.0, 5.0, zoom)),
                        width: px(analysis_graph_scaled(2.0, 1.0, zoom)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(accent),
                    Pickable::IGNORE,
                ));
            }
            node.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(analysis_graph_scaled(7.0, 3.0, zoom)),
                ..default()
            })
            .with_children(|heading| {
                if zoom >= 0.60 {
                    heading
                        .spawn((
                            Node {
                                width: px(analysis_graph_scaled(20.0, 14.0, zoom)),
                                height: px(analysis_graph_scaled(20.0, 14.0, zoom)),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border: UiRect::all(px((1.0 * zoom).max(0.7))),
                                border_radius: BorderRadius::all(px(analysis_graph_scaled(
                                    5.0, 3.0, zoom,
                                ))),
                                ..default()
                            },
                            BackgroundColor(accent.with_alpha(if running || complete {
                                0.20
                            } else {
                                0.11
                            })),
                            BorderColor::all(accent.with_alpha(if running || complete {
                                0.42
                            } else {
                                0.24
                            })),
                        ))
                        .with_children(|badge| {
                            spawn_text(badge, font.clone(), glyph, meta_size, accent);
                        });
                }
                heading
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_bounded_wrapped_text(
                            copy,
                            font.clone(),
                            label,
                            title_size,
                            theme.foreground,
                        );
                    });
            });
            let copy = if zoom < 0.60 {
                if running {
                    format!("{progress}%")
                } else {
                    status_glyph.to_string()
                }
            } else if running {
                format!("{status_glyph} {status} {progress}%")
            } else {
                format!("{status_glyph} {status}")
            };
            if zoom >= 0.60 {
                spawn_analysis_graph_status_pill_at(
                    node,
                    font.clone(),
                    copy,
                    status_color,
                    zoom,
                    31.0,
                );
                spawn_analysis_graph_model_tag(
                    node,
                    font,
                    theme,
                    route,
                    if warning {
                        theme.editor_warning
                    } else {
                        accent
                    },
                    zoom,
                );
            } else {
                spawn_analysis_graph_status_pill(node, font, copy, status_color, zoom);
            }
        })
        .observe(
            move |mut event: On<Pointer<Press>>,
                  mut analysis: ResMut<AnalysisUiState>,
                  mut dialogs: ResMut<DialogState>,
                  mut invalidated: ResMut<UiInvalidated>,
                  windows: Query<&Window, With<PrimaryWindow>>| {
                event.propagate(false);
                let viewport_size = windows
                    .single()
                    .map(|window| Vec2::new(window.width(), window.height()))
                    .unwrap_or(Vec2::new(1280.0, 720.0));
                open_analysis_node_from_pointer(
                    event.button,
                    event.pointer_location.position,
                    viewport_size,
                    AnalysisNodeClickTarget {
                        node_id: &context_node_id,
                        label: &context_label,
                        file_hash: &context_file_hash,
                        stage_id: &context_stage_id,
                    },
                    &mut analysis,
                    &mut dialogs,
                    &mut invalidated,
                );
            },
        );
}

/// The pointer position `open_analysis_node_from_pointer` needs, converted from
/// raw window pixels into `LibrarySongList`'s own local space -- the
/// analysis node context menu is spawned as a direct absolute-positioned
/// child of that same list (`spawn_analysis_node_context_menu`), so that is
/// the coordinate space its `left`/`top` need. Falls back to the raw window
/// position if the list isn't found (defensive only -- every caller of this
/// only runs from inside that list's own subtree).
pub(crate) fn analysis_context_menu_position(
    window_position: Vec2,
    scroll_offset: f32,
    lists: &Query<(&ComputedNode, &UiGlobalTransform), With<LibrarySongList>>,
) -> Vec2 {
    let Ok((computed, transform)) = lists.single() else {
        return window_position;
    };
    // `UiGlobalTransform` maps into space centered on the node (matching
    // `ui_node_contains_pointer`'s own use of it), not space anchored at its
    // top-left corner -- which is what `Node::left`/`Node::top` on the
    // absolute-positioned menu actually need. Add back half the list's own
    // size to shift the origin from its center to its corner.
    let center_relative = transform
        .affine()
        .inverse()
        .transform_point2(window_position);
    let half_size = computed.size() * computed.inverse_scale_factor() / 2.0;
    let mut local = center_relative + half_size;
    // Bevy's UI layout subtracts a node's *parent's* scroll position from
    // its rendered spot regardless of position type -- an absolute child of
    // a scrolling node still moves with that scroll (unlike CSS, where
    // `position: absolute` normally opts out of that). The menu is a direct
    // child of `LibrarySongList`, so without adding the list's current
    // scroll back in here, the menu would render offset from the node by
    // however far the list had been scrolled at click time. The list only
    // scrolls vertically (`ScrollPosition(Vec2::new(0.0, ...))`), so only Y
    // needs it.
    local.y += scroll_offset;
    local
}

pub(crate) fn spawn_analysis_graph_ports(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    ready: bool,
    zoom: f32,
) {
    let size = analysis_graph_scaled(10.0, 7.0, zoom);
    for (left, right) in [(Some(px(-5)), None), (None, Some(px(-5)))] {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: left.map(|_| px(-size * 0.5)).unwrap_or_default(),
                right: right.map(|_| px(-size * 0.5)).unwrap_or_default(),
                top: percent(50),
                width: px(size),
                height: px(size),
                border: UiRect::all(px(analysis_graph_scaled(1.0, 0.7, zoom))),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            UiTransform::from_xy(px(0), px(-size * 0.5)),
            BackgroundColor(if ready {
                theme.pitch_contour
            } else {
                theme.muted
            }),
            BorderColor::all(theme.background.with_alpha(0.9)),
            Pickable::IGNORE,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_analysis_graph_binding_path(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    points: &[Vec2],
    edge: &RenderEdge,
    binding: Option<&app_core::ArtifactBinding>,
    selected: bool,
    dimmed: bool,
    show_label: bool,
    zoom: f32,
) {
    let state = binding
        .map(|binding| binding.state)
        .unwrap_or(app_core::ArtifactBindingState::NotApplicable);
    let mut color = match edge.role {
        RenderEdgeRole::ComputeDependency | RenderEdgeRole::ExportTarget => theme.foreground,
        RenderEdgeRole::ArtifactOutput => {
            if matches!(
                edge.to.as_str(),
                "artifact.raw_vocal" | "artifact.denoised_vocal" | "artifact.dereverbed_vocal"
            ) {
                theme.muted_foreground
            } else {
                analysis_graph_node_accent(edge.from.as_str(), theme)
            }
        }
    };
    let alpha = if dimmed {
        0.18
    } else if selected {
        0.95
    } else {
        0.72
    };
    color = color.with_alpha(alpha);
    let under_row_guide =
        edge.from.as_str() == "lyrics.preprocess" && edge.to.as_str() == "lyrics.align";
    let base_thickness = if selected {
        3.5
    } else if under_row_guide {
        1.0
    } else if matches!(edge.role, RenderEdgeRole::ExportTarget) {
        2.0
    } else {
        2.25
    };
    let thickness = analysis_graph_scaled(base_thickness, 0.8, zoom);
    if under_row_guide
        || matches!(
            edge.to.as_str(),
            "artifact.raw_vocal" | "artifact.denoised_vocal" | "artifact.dereverbed_vocal"
        )
    {
        spawn_analysis_graph_dashed_segments(parent, points, color, thickness, zoom);
    } else {
        spawn_analysis_graph_segments(parent, points, color, thickness, true);
    }

    let selected_edge = selected_graph_edge_from_binding(
        &edge.from,
        &edge.to,
        &edge.producer_node,
        edge.artifact_kind,
        binding,
    );
    let click_edge = selected_edge.clone();
    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        let hit_inset = analysis_graph_scaled(6.0, 6.0, zoom);
        let hit_left = first.x.min(last.x) - hit_inset;
        let hit_top = first.y.min(last.y) - hit_inset;
        let hit_width = (first.x - last.x).abs().max(16.0) + hit_inset * 2.0;
        let hit_height = (first.y - last.y).abs().max(16.0) + hit_inset * 2.0;
        parent
            .spawn((
                Button,
                UiPointerApi(&["ui.pointer.analysis_edge.primary"]),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(hit_left),
                    top: px(hit_top),
                    width: px(hit_width),
                    height: px(hit_height),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                ZIndex(1),
            ))
            .observe(
                move |mut event: On<Pointer<Click>>,
                      mut shell: ResMut<ShellState>,
                      library: Res<LibraryState>,
                      mut analysis: ResMut<AnalysisUiState>,
                      mut dialogs: ResMut<DialogState>,
                      mut invalidated: ResMut<UiInvalidated>| {
                    event.propagate(false);
                    if event.button != PointerButton::Primary {
                        return;
                    }
                    let short = click_edge
                        .revision_id
                        .as_deref()
                        .map(|id| {
                            id.chars()
                                .rev()
                                .take(10)
                                .collect::<String>()
                                .chars()
                                .rev()
                                .collect::<String>()
                        })
                        .unwrap_or_else(|| "no revision".to_string());
                    let kind = click_edge
                        .kind
                        .map(|kind| format!("{kind:?}"))
                        .unwrap_or_else(|| "compute".to_string());
                    shell.notice = Some(format!(
                        "{kind} · {} · {short}",
                        edge_binding_style_copy(click_edge.state)
                    ));
                    analysis.selected_graph_edge = Some(click_edge.clone());
                    if let Some(revision_id) = click_edge.revision_id.as_ref()
                        && let Some(kind) = click_edge.kind
                    {
                        let reference = app_core::ArtifactRef {
                            file_hash: analysis
                                .selected_analysis_history
                                .and_then(|id| {
                                    analysis
                                        .analysis_history
                                        .iter()
                                        .find(|history| history.id == id)
                                        .map(|history| history.file_hash.clone())
                                })
                                .or_else(|| library.selected_song.clone())
                                .unwrap_or_default(),
                            kind,
                            revision_id: revision_id.clone(),
                        };
                        if analysis.analysis_lineage_mode
                            && let Ok(lineage) = app_core::artifact_lineage(&reference)
                        {
                            dialogs.artifact_lineage = Some(ArtifactLineagePanel {
                                lineage,
                                scope: analysis.analysis_lineage_scope,
                                selected: reference,
                            });
                        }
                    }
                    invalidated.invalidate(UiDirtyRegion::Analysis);
                },
            );
    }

    if (show_label || selected)
        && let Some(mid) = points.get(points.len() / 2)
    {
        let kind = edge
            .artifact_kind
            .map(|kind| format!("{kind:?}"))
            .unwrap_or_else(|| "edge".to_string());
        let short = binding
            .and_then(|item| item.artifact_ref.as_ref())
            .map(|reference| {
                reference
                    .revision_id
                    .chars()
                    .rev()
                    .take(8)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            })
            .unwrap_or_default();
        let label = if short.is_empty() {
            format!("{kind} · {}", edge_binding_style_copy(state))
        } else {
            format!("{kind} · {short}")
        };
        parent
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(mid.x - 46.0 * zoom),
                    top: px(mid.y - 10.0 * zoom),
                    max_width: px(120.0 * zoom),
                    padding: UiRect::axes(px(4.0 * zoom), px(2.0 * zoom)),
                    border_radius: BorderRadius::all(px(3.0 * zoom)),
                    ..default()
                },
                BackgroundColor(theme.card.with_alpha(0.92)),
                ZIndex(3),
                Pickable::IGNORE,
            ))
            .with_children(|chip| {
                spawn_text(
                    chip,
                    font,
                    label,
                    analysis_graph_scaled(6.5, 6.0, zoom),
                    theme.muted_foreground,
                );
            });
    }
}

fn spawn_analysis_graph_segments(
    parent: &mut ChildSpawnerCommands,
    points: &[Vec2],
    color: Color,
    thickness: f32,
    pickable_segments: bool,
) {
    for pair in points.windows(2) {
        let from = pair[0];
        let to = pair[1];
        let horizontal = (from.y - to.y).abs() <= 0.5;
        let left = from.x.min(to.x);
        let top = from.y.min(to.y);
        let mut entity = parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(if horizontal {
                    (to.x - from.x).abs().max(thickness)
                } else {
                    thickness
                }),
                height: px(if horizontal {
                    thickness
                } else {
                    (to.y - from.y).abs().max(thickness)
                }),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(color),
            ZIndex(0),
        ));
        if !pickable_segments {
            entity.insert(Pickable::IGNORE);
        }
    }
}

fn spawn_analysis_graph_dashed_segments(
    parent: &mut ChildSpawnerCommands,
    points: &[Vec2],
    color: Color,
    thickness: f32,
    zoom: f32,
) {
    let dash = analysis_graph_scaled(4.0, 3.0, zoom);
    let gap = analysis_graph_scaled(4.0, 3.0, zoom);
    for pair in points.windows(2) {
        let from = pair[0];
        let to = pair[1];
        let horizontal = (from.y - to.y).abs() <= 0.5;
        let length = if horizontal {
            (to.x - from.x).abs()
        } else {
            (to.y - from.y).abs()
        };
        let direction = if horizontal {
            (to.x - from.x).signum()
        } else {
            (to.y - from.y).signum()
        };
        let mut cursor = 0.0;
        while cursor < length {
            let visible = dash.min(length - cursor).max(thickness);
            let x = if horizontal {
                from.x + direction * cursor
            } else {
                from.x
            };
            let y = if horizontal {
                from.y
            } else {
                from.y + direction * cursor
            };
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(if horizontal {
                        x.min(x + direction * visible)
                    } else {
                        x
                    }),
                    top: px(if horizontal {
                        y
                    } else {
                        y.min(y + direction * visible)
                    }),
                    width: px(if horizontal { visible } else { thickness }),
                    height: px(if horizontal { thickness } else { visible }),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(color),
                ZIndex(0),
                Pickable::IGNORE,
            ));
            cursor += dash + gap;
        }
    }
}
