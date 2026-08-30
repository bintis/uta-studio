//! Typed selection helpers for compiled bindings and chart revision merges.

use crate::studio::*;

pub(crate) fn artifact_ref_from_revision(
    revision: &app_core::ArtifactRevision,
) -> app_core::ArtifactRef {
    app_core::ArtifactRef {
        file_hash: revision.file_hash.clone(),
        kind: revision.kind,
        revision_id: revision.id.clone(),
    }
}

pub(crate) fn merge_mode_from_editor_selection(
    editor: Option<&NativeEditor>,
    phrase: bool,
) -> Result<app_core::ChartRevisionMergeMode, String> {
    let Some(editor) = editor else {
        return Err(if phrase {
            "Select a phrase in the chart editor first.".to_string()
        } else {
            "Select notes in the chart editor first.".to_string()
        });
    };
    if editor.dirty {
        return Err(
            "Save the authored chart before merging a candidate into the current selection.".into(),
        );
    }
    let indices = editor.selected_note_indices();
    if indices.is_empty() {
        return Err(if phrase {
            "Select a phrase in the chart editor first.".to_string()
        } else {
            "Select notes in the chart editor first.".to_string()
        });
    }
    let track = editor.document.active_track_index();
    if phrase {
        let phrase = indices
            .iter()
            .next()
            .and_then(|index| editor.document.phrase_index_for_note(*index))
            .ok_or_else(|| "Select a phrase in the chart editor first.".to_string())?;
        Ok(app_core::ChartRevisionMergeMode::ReplacePhrase { track, phrase })
    } else {
        let (start, end) = editor
            .document
            .note_range_units(&indices)
            .ok_or_else(|| "Select notes in the chart editor first.".to_string())?;
        Ok(app_core::ChartRevisionMergeMode::ReplaceNoteRange { track, start, end })
    }
}
