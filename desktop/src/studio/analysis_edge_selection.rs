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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectedGraphEdge {
    pub(crate) from: String,
    pub(crate) from_port: String,
    pub(crate) to: String,
    pub(crate) to_port: String,
    pub(crate) semantic_type: String,
    pub(crate) audio_role: Option<String>,
    pub(crate) role: RenderEdgeRole,
}

pub(crate) fn selected_graph_edge_from_render(edge: &RenderEdge) -> SelectedGraphEdge {
    SelectedGraphEdge {
        from: edge.from.to_string(),
        from_port: edge.from_port.clone(),
        to: edge.to.to_string(),
        to_port: edge.to_port.clone(),
        semantic_type: edge.semantic_type.clone(),
        audio_role: edge.audio_role.clone(),
        role: edge.role,
    }
}

impl SelectedGraphEdge {
    pub(crate) fn matches_render(&self, edge: &RenderEdge) -> bool {
        self.from == edge.from.as_str()
            && self.from_port == edge.from_port
            && self.to == edge.to.as_str()
            && self.to_port == edge.to_port
            && self.semantic_type == edge.semantic_type
            && self.audio_role == edge.audio_role
            && self.role == edge.role
    }
}

pub(crate) fn edge_binding_style_copy(role: RenderEdgeRole) -> &'static str {
    match role {
        RenderEdgeRole::ComputeDependency => "Active binding",
        RenderEdgeRole::AnalyzerAttachment => "Analyzer attachment",
        RenderEdgeRole::InactiveBinding => "Inactive binding",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(from_port: &str) -> RenderEdge {
        RenderEdge {
            from: app_core::AnalysisNodeId::new("workflow.source"),
            from_port: from_port.to_string(),
            to: app_core::AnalysisNodeId::new("workflow.consumer"),
            to_port: "audio".to_string(),
            semantic_type: "audio.stem".to_string(),
            audio_role: Some("lead".to_string()),
            role: RenderEdgeRole::ComputeDependency,
        }
    }

    #[test]
    fn parallel_bindings_are_selected_by_ports_not_only_endpoints() {
        let selected = selected_graph_edge_from_render(&edge("lead"));
        assert!(selected.matches_render(&edge("lead")));
        assert!(!selected.matches_render(&edge("residual")));
    }
}
