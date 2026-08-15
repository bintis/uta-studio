//! The editor action registry.
//!
//! One table names every editor command once. The keyboard map, the toolbar
//! and inspector buttons, the undo history label, and the diagnostics
//! enumeration all read from it, so a command cannot exist with a shortcut but
//! no name, or be renamed in one place and not the other.
//!
//! The table is deliberately UI-framework free: keys are spelled with the
//! Bevy `KeyCode` variant names, and the desktop shell resolves them once,
//! under a test that proves every chord in the table resolves.

/// Where an action belongs in the editor's menus and help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EditorActionGroup {
    Document,
    Transport,
    View,
    Selection,
    Notes,
    Lyrics,
}

impl EditorActionGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Document => "Chart",
            Self::Transport => "Transport",
            Self::View => "View",
            Self::Selection => "Selection",
            Self::Notes => "Notes",
            Self::Lyrics => "Lyrics",
        }
    }
}

/// The API access class of an action, matching [`crate::ApiCapability`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorActionAccess {
    /// Changes only what is on screen.
    Read,
    /// Changes the chart under edit.
    Mutation,
}

impl EditorActionAccess {
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Mutation => "mutation",
        }
    }
}

/// A key chord, spelled with Bevy `KeyCode` variant names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    pub key: &'static str,
    pub ctrl: bool,
    pub shift: bool,
}

impl KeyChord {
    const fn plain(key: &'static str) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
        }
    }

    const fn ctrl(key: &'static str) -> Self {
        Self {
            key,
            ctrl: true,
            shift: false,
        }
    }

    const fn shift(key: &'static str) -> Self {
        Self {
            key,
            ctrl: false,
            shift: true,
        }
    }

    const fn ctrl_shift(key: &'static str) -> Self {
        Self {
            key,
            ctrl: true,
            shift: true,
        }
    }

    /// Human-readable chord, for menus and the shortcut list.
    pub fn describe(&self) -> String {
        let mut text = String::new();
        if self.ctrl {
            text.push_str("Ctrl+");
        }
        if self.shift {
            text.push_str("Shift+");
        }
        text.push_str(match self.key {
            "ArrowLeft" => "Left",
            "ArrowRight" => "Right",
            "ArrowUp" => "Up",
            "ArrowDown" => "Down",
            other => other.strip_prefix("Key").unwrap_or(other),
        });
        text
    }
}

/// One editor command.
#[derive(Debug, Clone, Copy)]
pub struct EditorActionDef {
    /// Stable identifier. Also the name diagnostics and the API contract use.
    pub command: &'static str,
    /// What the action is called in menus and in the undo history.
    pub label: &'static str,
    pub group: EditorActionGroup,
    pub access: EditorActionAccess,
    /// Every chord that runs this action. The first one is the canonical hint.
    pub shortcuts: &'static [KeyChord],
}

macro_rules! action {
    ($command:literal, $label:literal, $group:ident, $access:ident, [$($chord:expr),* $(,)?]) => {
        EditorActionDef {
            command: $command,
            label: $label,
            group: EditorActionGroup::$group,
            access: EditorActionAccess::$access,
            shortcuts: &[$($chord),*],
        }
    };
}

pub const EDITOR_ACTIONS: &[EditorActionDef] = &[
    // -- chart ------------------------------------------------------------
    action!(
        "save",
        "Save chart",
        Document,
        Mutation,
        [KeyChord::ctrl("KeyS")]
    ),
    action!("undo", "Undo", Document, Mutation, [KeyChord::ctrl("KeyZ")]),
    action!(
        "redo",
        "Redo",
        Document,
        Mutation,
        [KeyChord::ctrl("KeyY"), KeyChord::ctrl_shift("KeyZ")]
    ),
    action!("repair_chart", "Repair chart", Document, Mutation, []),
    action!(
        "shift_chart_earlier",
        "Shift whole chart earlier",
        Document,
        Mutation,
        []
    ),
    action!(
        "shift_chart_later",
        "Shift whole chart later",
        Document,
        Mutation,
        []
    ),
    // -- transport --------------------------------------------------------
    action!(
        "toggle_playback",
        "Play or pause",
        Transport,
        Read,
        [KeyChord::plain("Space")]
    ),
    action!("seek_start", "Jump to the start", Transport, Read, []),
    // -- view -------------------------------------------------------------
    action!("toggle_lyrics", "Show or hide lyrics", View, Read, []),
    action!(
        "toggle_inspector",
        "Show or hide the inspector",
        View,
        Read,
        []
    ),
    action!(
        "close_inspector",
        "Close the inspector",
        View,
        Read,
        [KeyChord::plain("Escape")]
    ),
    action!("zoom_in_time", "Zoom in", View, Read, []),
    action!("zoom_out_time", "Zoom out", View, Read, []),
    action!("zoom_in_pitch", "Zoom in on pitch", View, Read, []),
    action!("zoom_out_pitch", "Zoom out on pitch", View, Read, []),
    action!("pan_pitch_up", "Pan up", View, Read, []),
    action!("pan_pitch_down", "Pan down", View, Read, []),
    // -- selection --------------------------------------------------------
    action!(
        "select_all",
        "Select all",
        Selection,
        Read,
        [KeyChord::ctrl("KeyA")]
    ),
    action!(
        "select_next_note",
        "Select the next note",
        Selection,
        Read,
        [KeyChord::plain("Tab")]
    ),
    action!(
        "select_previous_note",
        "Select the previous note",
        Selection,
        Read,
        [KeyChord::shift("Tab")]
    ),
    // -- notes ------------------------------------------------------------
    action!("add_note", "Add note", Notes, Mutation, []),
    action!(
        "delete_selection",
        "Delete selection",
        Notes,
        Mutation,
        [KeyChord::plain("Delete"), KeyChord::plain("Backspace")]
    ),
    action!(
        "split_selection",
        "Split selection",
        Notes,
        Mutation,
        [KeyChord::plain("KeyS")]
    ),
    action!(
        "merge_selection",
        "Merge selection",
        Notes,
        Mutation,
        [KeyChord::plain("KeyM")]
    ),
    action!(
        "quantize_notes",
        "Quantize notes",
        Notes,
        Mutation,
        [KeyChord::plain("KeyQ")]
    ),
    action!(
        "duplicate_notes",
        "Duplicate notes",
        Notes,
        Mutation,
        [KeyChord::ctrl("KeyD")]
    ),
    action!(
        "copy_notes",
        "Copy notes",
        Notes,
        Read,
        [KeyChord::ctrl("KeyC")]
    ),
    action!(
        "cut_notes",
        "Cut notes",
        Notes,
        Mutation,
        [KeyChord::ctrl("KeyX")]
    ),
    action!(
        "paste_notes",
        "Paste notes",
        Notes,
        Mutation,
        [KeyChord::ctrl("KeyV")]
    ),
    action!("cycle_note_kind", "Change note type", Notes, Mutation, []),
    action!(
        "nudge_earlier",
        "Nudge earlier",
        Notes,
        Mutation,
        [KeyChord::plain("ArrowLeft")]
    ),
    action!(
        "nudge_later",
        "Nudge later",
        Notes,
        Mutation,
        [KeyChord::plain("ArrowRight")]
    ),
    action!(
        "shorten_selection",
        "Shorten selection",
        Notes,
        Mutation,
        [KeyChord::shift("ArrowLeft")]
    ),
    action!(
        "lengthen_selection",
        "Lengthen selection",
        Notes,
        Mutation,
        [KeyChord::shift("ArrowRight")]
    ),
    action!(
        "raise_pitch",
        "Raise pitch",
        Notes,
        Mutation,
        [KeyChord::plain("ArrowUp")]
    ),
    action!(
        "lower_pitch",
        "Lower pitch",
        Notes,
        Mutation,
        [KeyChord::plain("ArrowDown")]
    ),
    action!(
        "raise_pitch_octave",
        "Raise pitch an octave",
        Notes,
        Mutation,
        [KeyChord::shift("ArrowUp")]
    ),
    action!(
        "lower_pitch_octave",
        "Lower pitch an octave",
        Notes,
        Mutation,
        [KeyChord::shift("ArrowDown")]
    ),
    // -- lyrics -----------------------------------------------------------
    action!("add_lyric", "Add lyric word", Lyrics, Mutation, []),
    action!("delete_lyrics", "Delete lyric words", Lyrics, Mutation, []),
    action!("split_lyrics", "Split lyric words", Lyrics, Mutation, []),
    action!("merge_lyrics", "Merge lyric words", Lyrics, Mutation, []),
    action!(
        "shift_lyric_earlier",
        "Move lyric words earlier",
        Lyrics,
        Mutation,
        []
    ),
    action!(
        "shift_lyric_later",
        "Move lyric words later",
        Lyrics,
        Mutation,
        []
    ),
    action!(
        "lyric_start_earlier",
        "Start the lyric earlier",
        Lyrics,
        Mutation,
        []
    ),
    action!(
        "lyric_start_later",
        "Start the lyric later",
        Lyrics,
        Mutation,
        []
    ),
    action!(
        "lyric_end_earlier",
        "End the lyric earlier",
        Lyrics,
        Mutation,
        []
    ),
    action!(
        "lyric_end_later",
        "End the lyric later",
        Lyrics,
        Mutation,
        []
    ),
    action!("split_phrase", "Start a new phrase", Lyrics, Mutation, []),
    action!("merge_phrase", "Join the next phrase", Lyrics, Mutation, []),
];

pub fn editor_actions() -> &'static [EditorActionDef] {
    EDITOR_ACTIONS
}

/// Looks an action up by its stable command id.
pub fn editor_action(command: &str) -> Option<&'static EditorActionDef> {
    EDITOR_ACTIONS
        .iter()
        .find(|action| action.command == command)
}

/// Resolves a pressed chord to the action it runs.
pub fn editor_action_for_chord(
    key: &str,
    ctrl: bool,
    shift: bool,
) -> Option<&'static EditorActionDef> {
    EDITOR_ACTIONS.iter().find(|action| {
        action
            .shortcuts
            .iter()
            .any(|chord| chord.key == key && chord.ctrl == ctrl && chord.shift == shift)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn commands_and_labels_are_unique() {
        let commands = EDITOR_ACTIONS
            .iter()
            .map(|action| action.command)
            .collect::<BTreeSet<_>>();
        assert_eq!(commands.len(), EDITOR_ACTIONS.len());
        let labels = EDITOR_ACTIONS
            .iter()
            .map(|action| action.label)
            .collect::<BTreeSet<_>>();
        assert_eq!(labels.len(), EDITOR_ACTIONS.len());
    }

    #[test]
    fn no_chord_runs_two_actions() {
        let mut seen = BTreeSet::new();
        for action in EDITOR_ACTIONS {
            for chord in action.shortcuts {
                assert!(
                    seen.insert((chord.key, chord.ctrl, chord.shift)),
                    "{} reuses {}",
                    action.command,
                    chord.describe()
                );
            }
        }
    }

    #[test]
    fn chords_resolve_to_their_action() {
        let save = editor_action_for_chord("KeyS", true, false).expect("Ctrl+S");
        assert_eq!(save.command, "save");
        let split = editor_action_for_chord("KeyS", false, false).expect("S");
        assert_eq!(split.command, "split_selection");
        assert_eq!(
            editor_action_for_chord("KeyZ", true, true).map(|action| action.command),
            Some("redo")
        );
        assert!(editor_action_for_chord("KeyS", false, true).is_none());
    }

    #[test]
    fn chords_read_the_way_a_menu_prints_them() {
        assert_eq!(KeyChord::ctrl("KeyS").describe(), "Ctrl+S");
        assert_eq!(KeyChord::shift("ArrowLeft").describe(), "Shift+Left");
        assert_eq!(KeyChord::plain("Space").describe(), "Space");
    }
}
