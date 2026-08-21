//! The track strip.
//!
//! UTZ 0.2 charts hold several vocal tracks — a duet partner, a harmony, an
//! ad-lib line — and the format forbids two notes overlapping inside one
//! track. The strip is where a second voice gets a place to live: it switches
//! the track under edit, names its singer, and takes a selection that would
//! otherwise overlap and moves it somewhere legal.

use app_core::TrackSummary;
use bevy::{
    color::Alpha,
    ecs::change_detection::DetectChanges,
    input_focus::tab_navigation::TabIndex,
    prelude::{
        AlignItems, BackgroundColor, BorderColor, BorderRadius, Button, ChildSpawnerCommands,
        FlexDirection, Font, Handle, Image, JustifyContent, Node, Overflow, Pickable, Query, Ref,
        ResMut, Text, TextColor, TextLayout, UiRect, children, default, percent, px,
    },
    text::{EditableText, TextCursorStyle},
};

use crate::{
    studio::{
        commands::{EditorCommand, UiAction},
        state::EditorUiState,
        widgets::{UiIcon, spawn_icon, spawn_icon_button, spawn_text, ui_text_font},
    },
    theme::StudioTheme,
};

use super::{
    actions::EditorAction,
    state::{EditorSingerInput, NativeEditor},
};

/// Height of the strip. Tall enough for a role, a singer, and a coverage bar.
const TRACK_CARD_WIDTH: f32 = 186.0;

pub(crate) fn spawn_editor_tracks(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    editor: &NativeEditor,
    theme: &StudioTheme,
) {
    let tracks = editor.document.tracks();
    let active = editor.document.active_track_index();
    let has_selection = !editor.selected_note_indices().is_empty();
    // Coverage is read against the longest track, so the bars compare.
    let duration = editor
        .audio_status
        .duration_secs
        .max(tracks.iter().map(|track| track.span.1).fold(0.0, f64::max))
        .max(1.0);

    parent
        .spawn((
            Node {
                width: percent(100),
                // Tall enough for the active card's grown, four-row layout
                // (see `spawn_track_card`) plus a little breathing room.
                height: px(72),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: px(8),
                padding: UiRect::horizontal(px(12)),
                border: UiRect::bottom(px(1)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.34)),
            BorderColor::all(theme.border.with_alpha(0.45)),
        ))
        .with_children(|strip| {
            spawn_icon(
                strip,
                icons.clone(),
                UiIcon::Duet,
                22.0,
                theme.muted_foreground,
            );
            for track in &tracks {
                spawn_track_card(
                    strip,
                    font.clone(),
                    track,
                    track.index == active,
                    has_selection,
                    tracks.len() > 1,
                    duration,
                    theme,
                );
            }
            spawn_icon_button(
                strip,
                icons,
                theme,
                UiIcon::Add,
                UiAction::from(EditorCommand::Editor(EditorAction::AddTrack)),
                false,
                false,
                32.0,
            );
            strip.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            spawn_text(
                strip,
                font,
                if tracks.len() > 1 {
                    "Only the active track is editable. Move a selection to another track to resolve an overlap."
                } else {
                    "Add a track for a duet, harmony, or ad-lib line."
                },
                8.0,
                theme.muted_foreground,
            );
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_track_card(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    track: &TrackSummary,
    active: bool,
    has_selection: bool,
    removable: bool,
    duration: f64,
    theme: &StudioTheme,
) {
    let coverage = (track.sung_seconds / duration).clamp(0.0, 1.0) as f32;
    parent
        .spawn((
            Button,
            UiAction::from(EditorCommand::SelectEditorTrack(track.index)),
            Node {
                width: px(TRACK_CARD_WIDTH),
                // The active card packs an extra row (the singer name field)
                // and swaps its labels for pill buttons, both taller than the
                // idle card's plain text. A fixed height squeezed everything
                // down until rows overlapped; let the card grow to fit
                // instead, with `min_height` keeping idle cards from looking
                // collapsed.
                min_height: px(46),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                row_gap: px(3),
                padding: UiRect::axes(px(9), px(5)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(if active {
                theme.primary.with_alpha(0.16)
            } else {
                theme.background.with_alpha(0.42)
            }),
            BorderColor::all(if active {
                theme.primary.with_alpha(0.72)
            } else {
                theme.border.with_alpha(0.5)
            }),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(6),
                ..default()
            })
            .with_children(|header| {
                // The role reads as a label until the track is active, then it
                // becomes the control that cycles it.
                if active {
                    header.spawn((
                        Button,
                        UiAction::from(EditorCommand::Editor(EditorAction::CycleTrackRole)),
                        Node {
                            height: px(15),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(5)),
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(theme.primary.with_alpha(0.2)),
                        children![(
                            Text::new(track.role.label().to_uppercase()),
                            ui_text_font(font.clone(), 8.0),
                            TextColor(theme.primary),
                            TextLayout::no_wrap(),
                        )],
                    ));
                } else {
                    spawn_text(
                        header,
                        font.clone(),
                        track.role.label().to_uppercase(),
                        8.0,
                        theme.muted_foreground,
                    );
                }
                header.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                if active {
                    header.spawn((
                        Button,
                        UiAction::from(EditorCommand::Editor(EditorAction::ToggleTrackScoring)),
                        Node {
                            height: px(15),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(5)),
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(if track.scoring_enabled {
                            theme.primary.with_alpha(0.2)
                        } else {
                            theme.muted.with_alpha(0.4)
                        }),
                        children![(
                            Text::new(if track.scoring_enabled {
                                "SCORED"
                            } else {
                                "SILENT"
                            }),
                            ui_text_font(font.clone(), 8.0),
                            TextColor(if track.scoring_enabled {
                                theme.primary
                            } else {
                                theme.muted_foreground
                            }),
                            TextLayout::no_wrap(),
                        )],
                    ));
                    if removable {
                        header.spawn((
                            Button,
                            UiAction::from(EditorCommand::Editor(EditorAction::RemoveTrack)),
                            Node {
                                height: px(15),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(5)),
                                border_radius: BorderRadius::all(px(3)),
                                ..default()
                            },
                            BackgroundColor(theme.muted.with_alpha(0.4)),
                            children![(
                                Text::new("REMOVE"),
                                ui_text_font(font.clone(), 8.0),
                                TextColor(theme.muted_foreground),
                                TextLayout::no_wrap(),
                            )],
                        ));
                    }
                } else if has_selection {
                    header.spawn((
                        Button,
                        UiAction::from(EditorCommand::MoveSelectionToTrack(track.index)),
                        Node {
                            height: px(15),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(5)),
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(theme.muted.with_alpha(0.44)),
                        children![(
                            Text::new("MOVE HERE"),
                            ui_text_font(font.clone(), 8.0),
                            TextColor(theme.foreground),
                            TextLayout::no_wrap(),
                        )],
                    ));
                }
            });

            if active {
                card.spawn((
                    EditorSingerInput(track.index),
                    EditableText {
                        max_characters: Some(60),
                        visible_width: Some(20.0),
                        ..EditableText::new(track.singer.as_deref().unwrap_or_default())
                    },
                    Node {
                        width: percent(100),
                        height: px(14),
                        min_width: px(0),
                        align_items: AlignItems::Center,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    ui_text_font(font.clone(), 9.0),
                    TextColor(theme.foreground),
                    TextCursorStyle {
                        color: theme.editor_selection,
                        selected_text_color: Some(theme.primary_foreground),
                        ..default()
                    },
                    TabIndex(0),
                ));
            } else {
                spawn_text(
                    card,
                    font.clone(),
                    track
                        .singer
                        .clone()
                        .unwrap_or_else(|| "Unnamed singer".to_string()),
                    9.0,
                    theme.muted_foreground,
                );
            }

            // Coverage bar: how much of the song this track actually sings.
            card.spawn((
                Node {
                    width: percent(100),
                    height: px(3),
                    border_radius: BorderRadius::all(px(2)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(theme.muted.with_alpha(0.4)),
                Pickable::IGNORE,
            ))
            .with_children(|bar| {
                bar.spawn((
                    Node {
                        width: percent(coverage * 100.0),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(if active {
                        theme.primary.with_alpha(0.86)
                    } else {
                        theme.foreground.with_alpha(0.34)
                    }),
                    Pickable::IGNORE,
                ));
            });

            spawn_text(
                card,
                font,
                if track.note_count == 0 {
                    "Empty — not saved until it has notes".to_string()
                } else {
                    format!(
                        "{} notes · {} phrases · {:.0}%",
                        track.note_count,
                        track.phrase_count,
                        coverage * 100.0
                    )
                },
                8.0,
                theme.muted_foreground,
            );
        });
}

/// Applies a typed singer name to the track it belongs to.
pub(crate) fn sync_editor_singer_input(
    inputs: Query<(Ref<EditableText>, &EditorSingerInput)>,
    mut state: ResMut<EditorUiState>,
) {
    let Some(editor) = state.editor.as_mut() else {
        return;
    };
    for (input, marker) in &inputs {
        // See `sync_editor_word_input`: `Changed<EditableText>` also fires on
        // spawn, which would wrongly treat this card respawning (e.g. the
        // active track changing) as the user having retyped the name.
        if input.is_added() || !input.is_changed() {
            continue;
        }
        let text = input.value().to_string().trim().to_string();
        let current = editor
            .document
            .tracks()
            .get(marker.0)
            .and_then(|track| track.singer.clone())
            .unwrap_or_default();
        if text == current {
            continue;
        }
        editor.checkpoint("Name singer");
        if editor
            .document
            .set_track_singer(marker.0, (!text.is_empty()).then_some(text))
        {
            editor.dirty = true;
        } else {
            editor.undo.pop();
        }
    }
}
