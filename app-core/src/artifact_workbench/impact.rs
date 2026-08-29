use super::*;

/// Returns the Studio-owned outer Workflow graph for artifact lineage and
/// authoring-impact presentation. Execution readiness and scheduling remain
/// exclusively authoritative in the Analysis Engine Plan.
pub(crate) fn current_workflow_graph(
    file_hash: &str,
) -> Result<crate::analysis_graph::AnalysisGraphSpec, String> {
    let definition = crate::workflow::load_song_workflow(file_hash)
        .map(|stored| stored.definition)
        .unwrap_or_else(|_| crate::workflow::default_workflow(file_hash));
    crate::workflow::compile_workflow(&definition)
        .map(|snapshot| snapshot.graph)
        .map_err(|error| error.to_string())
}

fn output_producer_for_kind(
    graph: &crate::analysis_graph::AnalysisGraphSpec,
    kind: ArtifactKind,
) -> Option<AnalysisNodeId> {
    graph
        .nodes
        .iter()
        .find(|node| node.outputs.contains(&kind))
        .map(|node| node.id.clone())
}

fn workflow_impact(
    file_hash: &str,
    focus: AnalysisNodeId,
    graph: &crate::analysis_graph::AnalysisGraphSpec,
) -> DownstreamImpact {
    let affected_nodes = graph.dependents_of(&focus).into_iter().collect::<Vec<_>>();
    let mut affected_with_focus = affected_nodes.clone();
    if !affected_with_focus.contains(&focus) {
        affected_with_focus.insert(0, focus.clone());
    }
    let export_may_need_regeneration = affected_with_focus.iter().any(|node_id| {
        graph
            .node(node_id)
            .is_some_and(|node| node.outputs.contains(&ArtifactKind::CandidateChart))
    });
    DownstreamImpact {
        file_hash: file_hash.to_string(),
        node_id: focus,
        affected_nodes,
        authored_chart_preserved: true,
        export_may_need_regeneration,
    }
}

pub(crate) fn preview_kind_downstream_impact(
    file_hash: &str,
    kind: ArtifactKind,
) -> Result<DownstreamImpact, String> {
    let graph = current_workflow_graph(file_hash)?;
    let focus = output_producer_for_kind(&graph, kind)
        .ok_or_else(|| format!("{kind:?} has no producer in the current Workflow"))?;
    Ok(workflow_impact(file_hash, focus, &graph))
}

pub fn preview_artifact_edit_impact(draft: &ArtifactEditDraft) -> Result<DownstreamImpact, String> {
    preview_kind_downstream_impact(&draft.source.file_hash, draft.output_kind)
}

pub fn resolve_graph_edge_binding(
    file_hash: &str,
    run_id: Option<i64>,
    producer_node: &str,
    kind: ArtifactKind,
) -> ArtifactBinding {
    match inspect_analysis_node_io(file_hash, producer_node, run_id) {
        Ok(inspection) => inspection
            .resolved_outputs
            .into_iter()
            .find(|binding| binding.kind == kind)
            .unwrap_or_else(|| {
                missing_binding(ArtifactDirection::Output, "output:edge".to_string(), kind)
            }),
        Err(_) => missing_binding(ArtifactDirection::Output, "output:edge".to_string(), kind),
    }
}

pub(crate) fn recursive_json_changes(
    a: &serde_json::Value,
    b: &serde_json::Value,
    path: &str,
    changes: &mut Vec<String>,
) {
    const MAX_CHANGES: usize = 200;
    if changes.len() >= MAX_CHANGES || a == b {
        return;
    }
    match (a, b) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            let keys = a.keys().chain(b.keys()).cloned().collect::<BTreeSet<_>>();
            for key in keys {
                let next = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match (a.get(&key), b.get(&key)) {
                    (Some(a), Some(b)) => recursive_json_changes(a, b, &next, changes),
                    (Some(_), None) => changes.push(format!("Removed {next}")),
                    (None, Some(_)) => changes.push(format!("Added {next}")),
                    (None, None) => {}
                }
                if changes.len() >= MAX_CHANGES {
                    break;
                }
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            for index in 0..a.len().max(b.len()) {
                let next = format!("{path}[{index}]");
                match (a.get(index), b.get(index)) {
                    (Some(a), Some(b)) => recursive_json_changes(a, b, &next, changes),
                    (Some(_), None) => changes.push(format!("Removed {next}")),
                    (None, Some(_)) => changes.push(format!("Added {next}")),
                    (None, None) => {}
                }
                if changes.len() >= MAX_CHANGES {
                    break;
                }
            }
        }
        _ => changes.push(format!(
            "Changed {}",
            if path.is_empty() { "value" } else { path }
        )),
    }
}

pub(crate) fn ordered_line_diff(a: &str, b: &str) -> (usize, usize, Vec<String>) {
    const MAX_LINES: usize = 1_000;
    let a = a.lines().take(MAX_LINES).collect::<Vec<_>>();
    let b = b.lines().take(MAX_LINES).collect::<Vec<_>>();
    let mut previous = vec![0usize; b.len() + 1];
    for left in &a {
        let mut current = vec![0usize; b.len() + 1];
        for (index, right) in b.iter().enumerate() {
            current[index + 1] = if left == right {
                previous[index] + 1
            } else {
                current[index].max(previous[index + 1])
            };
        }
        previous = current;
    }
    let common = previous[b.len()];
    let removed = a.len().saturating_sub(common);
    let added = b.len().saturating_sub(common);
    let mut examples = Vec::new();
    for index in 0..a.len().max(b.len()) {
        if a.get(index) != b.get(index) {
            if let Some(line) = a.get(index) {
                examples.push(format!("Line {} removed/changed: {}", index + 1, line));
            }
            if let Some(line) = b.get(index) {
                examples.push(format!("Line {} added/changed: {}", index + 1, line));
            }
        }
        if examples.len() >= 20 {
            break;
        }
    }
    (added, removed, examples)
}

pub(crate) fn json_array_len(value: &serde_json::Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len)
}

pub(crate) fn pitch_note_semantic_diff(
    a: &serde_json::Value,
    b: &serde_json::Value,
) -> (String, Vec<String>) {
    let a_notes = a
        .get("notes")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let b_notes = b
        .get("notes")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut moved = 0;
    let mut transposed = 0;
    let mut changes = Vec::new();
    for (index, (left, right)) in a_notes.iter().zip(b_notes).enumerate() {
        let left_start = left.get("start").and_then(serde_json::Value::as_f64);
        let right_start = right.get("start").and_then(serde_json::Value::as_f64);
        let left_end = left.get("end").and_then(serde_json::Value::as_f64);
        let right_end = right.get("end").and_then(serde_json::Value::as_f64);
        if left_start != right_start || left_end != right_end {
            moved += 1;
            if changes.len() < 40 {
                changes.push(format!("Note {} timing moved.", index + 1));
            }
        }
        let left_midi = left.get("midi").and_then(serde_json::Value::as_i64);
        let right_midi = right.get("midi").and_then(serde_json::Value::as_i64);
        if left_midi != right_midi {
            transposed += 1;
            if changes.len() < 40 {
                changes.push(format!(
                    "Note {} transposed from {} to {}.",
                    index + 1,
                    left_midi.map_or_else(|| "?".to_string(), |value| value.to_string()),
                    right_midi.map_or_else(|| "?".to_string(), |value| value.to_string())
                ));
            }
        }
    }
    let added = b_notes.len().saturating_sub(a_notes.len());
    let removed = a_notes.len().saturating_sub(b_notes.len());
    (
        format!(
            "Pitch notes differ: {added} added, {removed} removed, {moved} moved, {transposed} transposed."
        ),
        changes,
    )
}

pub(crate) fn chart_semantic_diff(
    a: &serde_json::Value,
    b: &serde_json::Value,
) -> (String, Vec<String>) {
    let a_tracks = json_array_len(a, "tracks");
    let b_tracks = json_array_len(b, "tracks");
    let a_phrases = json_array_len(a, "phrases");
    let b_phrases = json_array_len(b, "phrases");
    let a_segments = json_array_len(a, "segments");
    let b_segments = json_array_len(b, "segments");
    let mut changes = Vec::new();
    recursive_json_changes(a, b, "", &mut changes);
    (
        format!(
            "Chart differs: tracks {a_tracks}→{b_tracks}, phrases {a_phrases}→{b_phrases}, timed lyric segments {a_segments}→{b_segments}; {} changed semantic path(s).",
            changes.len()
        ),
        changes,
    )
}

pub fn compare_artifacts_typed(
    a: &ArtifactRef,
    b: &ArtifactRef,
) -> Result<ArtifactTypedDiff, String> {
    let chart_pair = matches!(
        (a.kind, b.kind),
        (ArtifactKind::CandidateChart, ArtifactKind::AuthoredChart)
            | (ArtifactKind::AuthoredChart, ArtifactKind::CandidateChart)
    );
    if a.file_hash != b.file_hash || (a.kind != b.kind && !chart_pair) {
        return Err(
            "typed artifact comparison requires the same song and compatible artifact kinds"
                .to_string(),
        );
    }
    let revision_a = revision_by_id(&a.file_hash, &a.revision_id)
        .ok_or_else(|| format!("artifact revision not found: {}", a.revision_id))?;
    let revision_b = revision_by_id(&b.file_hash, &b.revision_id)
        .ok_or_else(|| format!("artifact revision not found: {}", b.revision_id))?;
    if revision_a.content_hash == revision_b.content_hash {
        return Ok(ArtifactTypedDiff {
            revision_a: a.clone(),
            revision_b: b.clone(),
            same_content: true,
            summary: "Revisions are byte-identical.".to_string(),
            changed_fields: Vec::new(),
        });
    }

    let (summary, changed_fields) = match media_type(a.kind) {
        ArtifactMediaType::Json | ArtifactMediaType::Chart => {
            let va = preview_artifact(a)?;
            let vb = preview_artifact(b)?;
            match (va, vb) {
                (ArtifactPreview::Json(va), ArtifactPreview::Json(vb)) => {
                    if a.kind == ArtifactKind::PitchNoteCandidates {
                        return Ok({
                            let (summary, changed_fields) = pitch_note_semantic_diff(&va, &vb);
                            ArtifactTypedDiff {
                                revision_a: a.clone(),
                                revision_b: b.clone(),
                                same_content: false,
                                summary,
                                changed_fields,
                            }
                        });
                    }
                    if chart_pair
                        || matches!(
                            a.kind,
                            ArtifactKind::CandidateChart | ArtifactKind::AuthoredChart
                        )
                    {
                        let (summary, changed_fields) = chart_semantic_diff(&va, &vb);
                        return Ok(ArtifactTypedDiff {
                            revision_a: a.clone(),
                            revision_b: b.clone(),
                            same_content: false,
                            summary,
                            changed_fields,
                        });
                    }
                    let mut changes = Vec::new();
                    recursive_json_changes(&va, &vb, "", &mut changes);
                    let subject = match a.kind {
                        ArtifactKind::TimedTranscript => "Timed transcript",
                        ArtifactKind::PitchTrack => "Pitch curve",
                        ArtifactKind::PitchNoteCandidates => "Pitch note candidates",
                        ArtifactKind::CandidateChart | ArtifactKind::AuthoredChart => "Chart",
                        _ => "Structured artifact",
                    };
                    (
                        format!("{subject} differs at {} semantic path(s).", changes.len()),
                        changes,
                    )
                }
                _ => ("Structured preview unavailable.".to_string(), Vec::new()),
            }
        }
        ArtifactMediaType::Text => {
            let ta = match preview_artifact(a)? {
                ArtifactPreview::Text(value) => value,
                _ => String::new(),
            };
            let tb = match preview_artifact(b)? {
                ArtifactPreview::Text(value) => value,
                _ => String::new(),
            };
            let (added, removed, changes) = ordered_line_diff(&ta, &tb);
            (
                format!("Text differs: {added} added line(s), {removed} removed line(s)."),
                changes,
            )
        }
        ArtifactMediaType::Audio | ArtifactMediaType::SourceMedia => {
            let left = preview_artifact(a)?;
            let right = preview_artifact(b)?;
            match (left, right) {
                (
                    ArtifactPreview::AudioMetadata {
                        duration_ms: left_duration,
                        sample_rate: left_rate,
                        channels: left_channels,
                        ..
                    },
                    ArtifactPreview::AudioMetadata {
                        duration_ms: right_duration,
                        sample_rate: right_rate,
                        channels: right_channels,
                        ..
                    },
                ) => {
                    let mut changes = Vec::new();
                    if left_duration != right_duration {
                        changes.push(format!(
                            "Duration: {left_duration:?} → {right_duration:?} ms"
                        ));
                    }
                    if left_rate != right_rate {
                        changes.push(format!("Sample rate: {left_rate:?} → {right_rate:?} Hz"));
                    }
                    if left_channels != right_channels {
                        changes.push(format!("Channels: {left_channels:?} → {right_channels:?}"));
                    }
                    if changes.is_empty() {
                        changes.push(
                            "Encoded audio content differs; decoded metadata matches.".to_string(),
                        );
                    }
                    ("Audio revisions differ.".to_string(), changes)
                }
                _ => (
                    "Audio metadata preview unavailable.".to_string(),
                    Vec::new(),
                ),
            }
        }
        _ => (
            format!(
                "Binary content differs ({} bytes vs {} bytes).",
                revision_a.byte_size, revision_b.byte_size
            ),
            vec!["content".to_string()],
        ),
    };

    Ok(ArtifactTypedDiff {
        revision_a: a.clone(),
        revision_b: b.clone(),
        same_content: false,
        summary,
        changed_fields,
    })
}
