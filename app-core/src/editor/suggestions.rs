use serde::{Deserialize, Serialize};

use crate::artifact_workbench::ArtifactRef;

use super::{EditorDocument, LyricAddress, TrackRole};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditorSuggestionKind {
    ChangePitch {
        note_index: usize,
        midi: f64,
    },
    MoveBoundary {
        note_index: usize,
        start: f64,
        end: f64,
    },
    BindLyric {
        lyric: LyricAddress,
        note_index: usize,
    },
    ChangeTrackRole {
        track_index: usize,
        role: TrackRole,
    },
    InspectEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorSuggestion {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub confidence: f32,
    pub suggestion: EditorSuggestionKind,
    #[serde(default)]
    pub evidence_refs: Vec<ArtifactRef>,
}

/// Applies only an explicitly accepted suggestion. Desktop code checkpoints
/// the document first, so this ordinary document mutation participates in the
/// existing undo/redo history rather than creating a model-owned history.
pub fn apply_editor_suggestion(
    document: &mut EditorDocument,
    suggestion: &EditorSuggestion,
) -> Result<bool, String> {
    let changed = match &suggestion.suggestion {
        EditorSuggestionKind::ChangePitch { note_index, midi } => {
            let note = document
                .notes()
                .get(*note_index)
                .cloned()
                .ok_or_else(|| "suggestion note no longer exists".to_string())?;
            document.move_note(*note_index, note.start, note.end, *midi)
        }
        EditorSuggestionKind::MoveBoundary {
            note_index,
            start,
            end,
        } => document.resize_note(*note_index, *start, *end),
        EditorSuggestionKind::BindLyric { lyric, note_index } => {
            document.bind_lyric_to_note(*lyric, *note_index).is_some()
        }
        EditorSuggestionKind::ChangeTrackRole { track_index, role } => {
            document.set_track_role(*track_index, *role)
        }
        EditorSuggestionKind::InspectEvidence => false,
    };
    Ok(changed)
}
