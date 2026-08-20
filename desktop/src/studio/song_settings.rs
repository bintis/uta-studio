//! Song settings: composer, country, BPM, and a background video, plus a
//! read-only look at Essentia's descriptors when analysis produced them.
//!
//! One panel, two entry points: the library detail page and the editor's own
//! settings menu both just fire `UiAction::OpenSongSettings(file_hash)`, and
//! `open_song_settings` reloads the same `Song` row either way, so the two
//! surfaces can never drift out of sync with each other.

use crate::studio::*;

#[derive(Component)]
pub(crate) struct SongSettingsComposerInput;

#[derive(Component)]
pub(crate) struct SongSettingsCountryInput;

#[derive(Component)]
pub(crate) struct SongSettingsBpmInput;

pub(crate) struct NativeSongSettings {
    pub(crate) file_hash: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) key: Option<String>,
    pub(crate) initial_composer: String,
    pub(crate) initial_country: String,
    pub(crate) initial_bpm: String,
    pub(crate) background_video_path: Option<PathBuf>,
    pub(crate) music_analysis: Option<app_core::MusicAnalysis>,
}

/// Loads a song's current settings fresh from the library row, for whichever
/// entry point opened the panel.
pub(crate) fn open_song_settings(file_hash: &str) -> Option<NativeSongSettings> {
    let song = app_core::load_song_by_hash(file_hash).ok().flatten()?;
    let music_analysis = app_core::load_music_analysis(&app_core::CacheDir::new(), file_hash);
    Some(NativeSongSettings {
        file_hash: file_hash.to_string(),
        title: song.title,
        artist: song.artist,
        key: song.override_key.or(song.key),
        initial_composer: song.composer.unwrap_or_default(),
        initial_country: song.country.unwrap_or_default(),
        initial_bpm: song
            .override_bpm
            .or(song.bpm)
            .map(|bpm| format!("{bpm:.2}"))
            .unwrap_or_default(),
        background_video_path: song.background_video_path,
        music_analysis,
    })
}

pub(crate) fn spawn_song_settings_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    panel: &NativeSongSettings,
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
            BackgroundColor(theme.background.with_alpha(0.8)),
            ZIndex(120),
        ))
        .with_children(|backdrop| {
            backdrop
                .spawn((
                    Node {
                        width: px(440),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(10),
                        padding: UiRect::all(px(20)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(
                        dialog,
                        font.clone(),
                        format!("Song settings · {}", panel.title),
                        15.0,
                        theme.foreground,
                    );
                    spawn_text(
                        dialog,
                        font.clone(),
                        panel.artist.clone(),
                        10.0,
                        theme.muted_foreground,
                    );

                    spawn_song_settings_field(
                        dialog,
                        font.clone(),
                        theme,
                        "Composer",
                        SongSettingsComposerInput,
                        &panel.initial_composer,
                    );
                    spawn_song_settings_field(
                        dialog,
                        font.clone(),
                        theme,
                        "Country",
                        SongSettingsCountryInput,
                        &panel.initial_country,
                    );
                    spawn_song_settings_field(
                        dialog,
                        font.clone(),
                        theme,
                        "Musical BPM",
                        SongSettingsBpmInput,
                        &panel.initial_bpm,
                    );

                    spawn_text(
                        dialog,
                        font.clone(),
                        format!(
                            "Detected key: {} — use the Key transpose control on this song to change it.",
                            panel.key.as_deref().unwrap_or("Unknown")
                        ),
                        9.0,
                        theme.muted_foreground,
                    );

                    dialog
                        .spawn(Node {
                            align_items: AlignItems::Center,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|row| {
                            spawn_text(
                                row,
                                font.clone(),
                                "Background video",
                                9.0,
                                theme.muted_foreground,
                            );
                            let label = panel
                                .background_video_path
                                .as_ref()
                                .and_then(|path| path.file_name())
                                .and_then(|name| name.to_str())
                                .map(str::to_string)
                                .unwrap_or_else(|| "None set".to_string());
                            spawn_text(row, font.clone(), label, 9.0, theme.foreground);
                        });
                    dialog
                        .spawn(Node {
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|row| {
                            spawn_text_button(
                                row,
                                font.clone(),
                                theme,
                                "Choose…",
                                9.0,
                                UiAction::from(EditorCommand::ChooseBackgroundVideo),
                            );
                            if panel.background_video_path.is_some() {
                                spawn_text_button(
                                    row,
                                    font.clone(),
                                    theme,
                                    "Clear",
                                    9.0,
                                    UiAction::from(EditorCommand::ClearBackgroundVideo),
                                );
                            }
                        });

                    if let Some(analysis) = &panel.music_analysis {
                        dialog.spawn(Node {
                            height: px(1),
                            margin: UiRect::vertical(px(2)),
                            ..default()
                        });
                        spawn_text(
                            dialog,
                            font.clone(),
                            "Music analysis",
                            9.0,
                            theme.muted_foreground,
                        );
                        let mut summary = format!(
                            "Key confidence {:.2}",
                            analysis.key.confidence
                        );
                        if let Some(bpm) = analysis.rhythm.bpm {
                            summary.push_str(&format!(
                                " · Musical BPM {bpm:.1} (confidence {:.2}) · {} beats detected",
                                analysis.rhythm.confidence,
                                analysis.rhythm.beats.len()
                            ));
                        } else {
                            summary.push_str(" · Musical BPM unavailable");
                        }
                        if let Some(descriptors) = &analysis.descriptors {
                            summary.push_str(&format!(
                                " · Danceability {:.2} · Dynamic range {:.1} dB · Loudness {:.1} dB",
                                descriptors.danceability,
                                descriptors.dynamic_complexity_db,
                                descriptors.loudness_db,
                            ));
                        }
                        spawn_wrapped_text(dialog, font.clone(), summary, 9.0, theme.foreground);
                    }

                    dialog
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            margin: UiRect::top(px(4)),
                            ..default()
                        })
                        .with_children(|row| {
                            spawn_text_button(
                                row,
                                font.clone(),
                                theme,
                                "Cancel",
                                10.0,
                                UiAction::from(EditorCommand::CloseSongSettings),
                            );
                            spawn_text_button(
                                row,
                                font.clone(),
                                theme,
                                "Save",
                                10.0,
                                UiAction::from(EditorCommand::SaveSongSettings),
                            );
                        });
                });
        });
}

fn spawn_song_settings_field(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    marker: impl Component,
    initial: &str,
) {
    spawn_text(parent, font.clone(), label, 9.0, theme.muted_foreground);
    parent.spawn((
        marker,
        EditableText {
            visible_width: Some(44.0),
            max_characters: Some(200),
            ..EditableText::new(initial)
        },
        Node {
            width: percent(100),
            height: px(26),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        ui_text_font(font, 10.0),
        TextColor(theme.foreground),
        TextCursorStyle {
            color: theme.primary,
            selected_text_color: Some(theme.primary_foreground),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.65)),
        BorderColor::all(theme.border.with_alpha(0.72)),
        TabIndex(0),
    ));
}
