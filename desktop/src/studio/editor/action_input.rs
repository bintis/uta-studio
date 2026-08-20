//! Bevy input and UI routing for the editor command registry.

use std::time::{Duration, Instant};

use app_core::editor_action_for_chord;
use bevy::{
    ecs::system::SystemParam,
    input_focus::InputFocus,
    prelude::{ButtonInput, KeyCode, Query, Res, ResMut, With},
    text::EditableText,
};

use crate::studio::{
    commands::{EditorCommand, UiAction, UiCommand},
    session::{NativeAudio, NativePitchAudition, StudioRoute},
    state::{DialogState, EditorUiState, ShellState},
    ui_invalidation::{UiDirtyRegion, UiInvalidated},
};

use super::{
    actions::{EditorAction, EditorActionContext, move_selection_to_track, run_editor_action},
    audition::select_editor_audio_source,
    state::{AuditionMode, EditorDockSelectKind, WordSelection},
};

/// Handles the editor's share of [`UiAction`]. Returns whether the action
/// belonged to the editor.
pub(crate) fn handle_editor_ui_action(
    action: &UiAction,
    keys: &ButtonInput<KeyCode>,
    ctx: &mut EditorActionContext,
) -> bool {
    match &action.0 {
        UiCommand::Editor(EditorCommand::Editor(action)) => run_editor_action(*action, ctx),
        UiCommand::Editor(EditorCommand::FocusChartProblem(track, millis)) => {
            let target = *millis as f64 / 1000.0;
            if let Some(editor) = ctx.editor.editor.as_mut() {
                if editor.document.set_active_track(*track) {
                    editor.clear_selection();
                }
                editor.viewport_start = (target - editor.viewport_duration / 2.0).max(0.0);
                editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
            } else {
                return true;
            }
            ctx.seek(target);
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        UiCommand::Editor(EditorCommand::OpenEditorSelect(kind)) => {
            ctx.dialogs.open_editor_select = if ctx.dialogs.open_editor_select == Some(*kind) {
                None
            } else {
                Some(*kind)
            };
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        UiCommand::Editor(EditorCommand::SelectEditorValue(kind, value)) => {
            ctx.dialogs.open_editor_select = None;
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return true;
            };
            match kind {
                EditorDockSelectKind::AudioSource => {
                    ctx.shell.notice = select_editor_audio_source(ctx.audio, editor, value).err();
                }
                EditorDockSelectKind::AuditionMode => {
                    editor.audition_mode = AuditionMode::from_label(value);
                    ctx.shell.notice = None;
                }
                EditorDockSelectKind::SnapGrid => {
                    const GRIDS: [f64; 6] = [0.0, 0.01, 0.025, 0.05, 0.1, 0.25];
                    match value.parse::<f64>() {
                        Ok(value) if GRIDS.contains(&value) => {
                            editor.snap_seconds = value;
                            ctx.shell.notice = None;
                        }
                        _ => {
                            ctx.shell.notice =
                                Some("That timing grid is not supported.".to_string())
                        }
                    }
                }
            }
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        UiCommand::Editor(EditorCommand::SelectEditorTrack(index)) => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return true;
            };
            if editor.document.set_active_track(*index) {
                editor.clear_selection();
                ctx.shell.notice = Some(format!("Editing track {}.", index + 1));
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
            }
        }
        UiCommand::Editor(EditorCommand::MoveSelectionToTrack(index)) => {
            move_selection_to_track(EditorAction::MoveSelectionToNextTrack, *index, ctx);
        }
        UiCommand::Editor(EditorCommand::SetNoteKind(kind)) => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return true;
            };
            let selected = editor.selected_note_indices();
            if selected.is_empty() {
                ctx.shell.notice = Some("Select a note to change its type.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return true;
            }
            editor.checkpoint("Change note type");
            let changed = editor.document.set_note_kind(&selected, *kind);
            if changed > 0 {
                editor.dirty = true;
                ctx.shell.notice = Some(format!("Set to {}.", kind.label().replace('_', " ")));
            } else {
                editor.undo.pop();
            }
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        UiCommand::Editor(EditorCommand::SelectEditorWord(segment, word, position_ms)) => {
            let selection = WordSelection {
                segment: *segment,
                word: *word,
            };
            let additive = keys.any_pressed([
                KeyCode::ShiftLeft,
                KeyCode::ShiftRight,
                KeyCode::ControlLeft,
                KeyCode::ControlRight,
            ]);
            ctx.seek(*position_ms as f64 / 1000.0);
            if let Some(editor) = ctx.editor.editor.as_mut() {
                if additive {
                    if editor.selected_words.remove(&selection) {
                        editor.selected_word = editor.selected_words.iter().next().copied();
                    } else {
                        editor.selected_words.insert(selection);
                        editor.selected_word = Some(selection);
                    }
                    editor.word_edit_focus = None;
                    editor.selected_note = None;
                    editor.selected_notes.clear();
                } else if editor.word_edit_focus == Some(selection) {
                    editor.select_only_word(selection);
                    editor.word_edit_focus = Some(selection);
                } else if editor.selected_words.len() > 1
                    && editor.selected_words.contains(&selection)
                {
                    editor.selected_word = Some(selection);
                    editor.selected_note = None;
                    editor.selected_notes.clear();
                } else {
                    editor.select_only_word(selection);
                }
                editor.inspector_open = true;
            }
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        _ => return false,
    }
    true
}

fn chord_key_name(key: KeyCode) -> Option<&'static str> {
    Some(match key {
        KeyCode::KeyA => "KeyA",
        KeyCode::KeyC => "KeyC",
        KeyCode::KeyD => "KeyD",
        KeyCode::KeyF => "KeyF",
        KeyCode::KeyH => "KeyH",
        KeyCode::KeyL => "KeyL",
        KeyCode::KeyM => "KeyM",
        KeyCode::KeyQ => "KeyQ",
        KeyCode::KeyS => "KeyS",
        KeyCode::KeyT => "KeyT",
        KeyCode::KeyV => "KeyV",
        KeyCode::KeyX => "KeyX",
        KeyCode::KeyY => "KeyY",
        KeyCode::KeyZ => "KeyZ",
        KeyCode::Space => "Space",
        KeyCode::Tab => "Tab",
        KeyCode::Escape => "Escape",
        KeyCode::Delete => "Delete",
        KeyCode::Backspace => "Backspace",
        KeyCode::Home => "Home",
        KeyCode::End => "End",
        KeyCode::ArrowLeft => "ArrowLeft",
        KeyCode::ArrowRight => "ArrowRight",
        KeyCode::ArrowUp => "ArrowUp",
        KeyCode::ArrowDown => "ArrowDown",
        _ => return None,
    })
}

pub(crate) fn chord_key_code(name: &str) -> Option<KeyCode> {
    CHORD_KEYS
        .iter()
        .copied()
        .find(|key| chord_key_name(*key) == Some(name))
}

const CHORD_KEYS: &[KeyCode] = &[
    KeyCode::KeyA,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyF,
    KeyCode::KeyH,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyQ,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyV,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
    KeyCode::Space,
    KeyCode::Tab,
    KeyCode::Escape,
    KeyCode::Delete,
    KeyCode::Backspace,
    KeyCode::Home,
    KeyCode::End,
    KeyCode::ArrowLeft,
    KeyCode::ArrowRight,
    KeyCode::ArrowUp,
    KeyCode::ArrowDown,
];

#[derive(SystemParam)]
pub(crate) struct EditorKeyboardContext<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    focus: Res<'w, InputFocus>,
    editable: Query<'w, 's, (), With<EditableText>>,
    focusable: Query<'w, 's, (), With<UiAction>>,
    audio: Res<'w, NativeAudio>,
    tones: Res<'w, NativePitchAudition>,
    shell: ResMut<'w, ShellState>,
    editor: ResMut<'w, EditorUiState>,
    dialogs: ResMut<'w, DialogState>,
    invalidated: ResMut<'w, UiInvalidated>,
}

pub(crate) fn handle_editor_keyboard(context: EditorKeyboardContext) {
    let EditorKeyboardContext {
        keys,
        focus,
        editable,
        focusable,
        audio,
        tones,
        mut shell,
        mut editor,
        mut dialogs,
        mut invalidated,
    } = context;
    if shell.route != StudioRoute::Editor {
        return;
    }
    if focus.get().is_some_and(|entity| editable.contains(entity)) {
        return;
    }
    let navigating = focus.get().is_some_and(|entity| focusable.contains(entity))
        && [
            KeyCode::Tab,
            KeyCode::Enter,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
        ]
        .iter()
        .any(|key| keys.just_pressed(*key));
    if navigating {
        return;
    }

    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let Some(action) = CHORD_KEYS
        .iter()
        .filter(|key| keys.just_pressed(**key))
        .filter_map(|key| chord_key_name(*key))
        .find_map(|key| editor_action_for_chord(key, ctrl, shift))
        .and_then(|def| EditorAction::from_command(def.command))
    else {
        return;
    };
    run_editor_action(
        action,
        &mut EditorActionContext {
            audio: &audio.0,
            tones: &tones.0,
            shell: &mut shell,
            editor: &mut editor,
            dialogs: &mut dialogs,
            invalidated: &mut invalidated,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_entry_has_a_dispatchable_action() {
        for def in app_core::editor_actions() {
            let action = EditorAction::from_command(def.command)
                .unwrap_or_else(|| panic!("`{}` has no dispatch variant", def.command));
            assert_eq!(action.command(), def.command);
            assert_eq!(action.label(), def.label);
        }
    }

    #[test]
    fn every_action_has_a_registry_entry() {
        assert_eq!(EditorAction::ALL.len(), app_core::editor_actions().len());
        for action in EditorAction::ALL {
            assert!(!action.label().is_empty());
        }
    }

    #[test]
    fn every_bound_chord_resolves_to_a_key_the_shell_reads() {
        let readable = CHORD_KEYS
            .iter()
            .filter_map(|key| chord_key_name(*key))
            .collect::<Vec<_>>();
        for def in app_core::editor_actions() {
            for chord in def.shortcuts {
                assert!(
                    readable.contains(&chord.key),
                    "`{}` binds {}, which the shell cannot read",
                    def.command,
                    chord.describe()
                );
            }
        }
    }

    #[test]
    fn undo_history_labels_come_from_the_registry() {
        assert_eq!(EditorAction::PasteNotes.label(), "Paste notes");
        assert_eq!(EditorAction::DeleteSelection.label(), "Delete selection");
        assert_eq!(EditorAction::Save.shortcut().as_deref(), Some("Ctrl+S"));
    }
}
