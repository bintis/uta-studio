use super::*;

pub(crate) fn request_fingerprint(request: &AnalysisRequest) -> String {
    serde_json::to_string(&(
        &request.file_hash,
        request
            .targets
            .iter()
            .map(|id| id.as_str().to_string())
            .collect::<Vec<_>>(),
        request
            .disabled_nodes
            .iter()
            .map(|id| id.as_str().to_string())
            .collect::<Vec<_>>(),
        request
            .frozen_artifacts
            .iter()
            .map(|kind| format!("{kind:?}"))
            .collect::<Vec<_>>(),
        request
            .bypassed_nodes
            .iter()
            .map(|id| id.as_str().to_string())
            .collect::<Vec<_>>(),
        format!("{:?}", request.lyrics_route),
        serde_json::to_value(&request.profile_snapshot).unwrap_or(serde_json::Value::Null),
    ))
    .unwrap_or_default()
}

pub fn queued_request_matches_preview(
    impact: &DownstreamImpact,
    request: &AnalysisRequest,
) -> bool {
    impact.request_fingerprint == request_fingerprint(request)
}

pub fn analysis_request_from_impact(file_hash: &str, impact: &DownstreamImpact) -> AnalysisRequest {
    crate::analyzer::analysis_request_snapshot(
        file_hash,
        impact.queued_targets.iter().cloned().collect(),
        impact.queued_disabled.iter().cloned().collect(),
        impact.queued_frozen.iter().copied().collect(),
        impact.queued_bypassed.iter().cloned().collect(),
    )
}

pub(crate) fn classify_plan_impact(
    file_hash: &str,
    focus: AnalysisNodeId,
    plan: &AnalysisPlan,
    request: &AnalysisRequest,
    authored_chart_preserved: bool,
) -> DownstreamImpact {
    let graph = baseline_graph_spec();
    let mut will_run = Vec::new();
    let mut will_reuse = Vec::new();
    let mut will_be_blocked = Vec::new();
    for node in &plan.nodes {
        match node.state {
            NodeState::Frozen => will_reuse.push(node.id.clone()),
            NodeState::Blocked => will_be_blocked.push(node.id.clone()),
            NodeState::Disabled => will_be_blocked.push(node.id.clone()),
            _ if node.will_run => will_run.push(node.id.clone()),
            _ => {}
        }
    }
    let affected_nodes = graph.dependents_of(&focus).into_iter().collect::<Vec<_>>();
    let will_become_stale = affected_nodes
        .iter()
        .filter(|node_id| {
            will_run.iter().any(|running| running == *node_id)
                && graph.node(node_id).is_some_and(|node| {
                    node.outputs
                        .iter()
                        .any(|kind| load_active_artifact(file_hash, *kind).is_some())
                })
        })
        .cloned()
        .collect();
    let mut will_remain_preserved = Vec::new();
    if authored_chart_preserved {
        will_remain_preserved.push("AuthoredChart".to_string());
    }
    for kind in [
        ArtifactKind::VocalStem,
        ArtifactKind::InstrumentalStem,
        ArtifactKind::TimedTranscript,
        ArtifactKind::PitchTrack,
        ArtifactKind::CandidateChart,
        ArtifactKind::AuthoredChart,
    ] {
        if let Some(revision) = load_active_artifact(file_hash, kind)
            && library_db::analysis_artifact_is_pinned(&revision.id).unwrap_or(false)
        {
            will_remain_preserved.push(format!("Pinned {kind:?}"));
        }
    }
    let export_may_need_regeneration = will_run
        .iter()
        .any(|node| node.as_str() == "chart.build_candidate")
        || focus.as_str() == "chart.build_candidate";
    DownstreamImpact {
        file_hash: file_hash.to_string(),
        node_id: focus,
        affected_nodes,
        authored_chart_preserved,
        export_may_need_regeneration,
        will_run,
        will_reuse,
        will_become_stale,
        will_be_blocked,
        will_remain_preserved,
        exports_needing_regeneration: if export_may_need_regeneration {
            vec!["UTZ".to_string(), "UltraStar".to_string()]
        } else {
            Vec::new()
        },
        queued_targets: request.targets.iter().cloned().collect(),
        queued_disabled: request.disabled_nodes.iter().cloned().collect(),
        queued_frozen: request.frozen_artifacts.iter().copied().collect(),
        queued_bypassed: request.bypassed_nodes.iter().cloned().collect(),
        request_fingerprint: request_fingerprint(request),
    }
}

pub fn preview_frozen_downstream_impact(
    file_hash: &str,
    trigger: ImpactTrigger,
    focus_node: Option<&str>,
) -> Result<DownstreamImpact, String> {
    let graph = baseline_graph_spec();
    let focus = focus_node
        .map(AnalysisNodeId::new)
        .unwrap_or_else(|| AnalysisNodeId::new("chart.build_candidate"));
    if focus_node.is_some() && graph.node(&focus).is_none() {
        return Err(format!("unknown analysis node: {}", focus.as_str()));
    }
    let mut targets = BTreeSet::new();
    let mut extra_disabled = BTreeSet::new();
    let mut extra_frozen = BTreeSet::new();
    let mut extra_bypassed = BTreeSet::new();
    match trigger {
        ImpactTrigger::RunNode => {
            targets.insert(focus.clone());
        }
        ImpactTrigger::RunDownstream | ImpactTrigger::SaveAndRunDownstream => {
            targets = crate::analyzer::downstream_node_ids(focus.as_str());
        }
        ImpactTrigger::Freeze => {
            extra_frozen.extend(crate::analyzer::frozen_artifact_kinds_for_node_id(
                focus.as_str(),
            ));
        }
        ImpactTrigger::Bypass => {
            extra_bypassed.insert(focus.clone());
        }
        ImpactTrigger::Disable => {
            extra_disabled.insert(focus.clone());
        }
        ImpactTrigger::SetActive | ImpactTrigger::Invalidate | ImpactTrigger::Delete => {
            targets = graph.dependents_of(&focus);
            if targets.is_empty() {
                targets.insert(AnalysisNodeId::new("chart.build_candidate"));
            }
        }
        ImpactTrigger::CandidateReplace => {
            targets.insert(AnalysisNodeId::new("chart.build_candidate"));
        }
    }
    let request = crate::analyzer::analysis_request_snapshot(
        file_hash,
        targets,
        extra_disabled,
        extra_frozen,
        extra_bypassed,
    );
    let plan = crate::analysis_plan::preview_analysis_plan(file_hash, request.clone())
        .map_err(|error| error.to_string())?;
    Ok(classify_plan_impact(
        file_hash,
        focus,
        &plan,
        &request,
        trigger != ImpactTrigger::CandidateReplace,
    ))
}

pub fn preview_node_downstream_impact(node_id: &str) -> Result<DownstreamImpact, String> {
    preview_frozen_downstream_impact("", ImpactTrigger::RunDownstream, Some(node_id))
}

pub fn preview_artifact_downstream_impact(
    reference: &ArtifactRef,
) -> Result<DownstreamImpact, String> {
    let revision = revision_by_id(&reference.file_hash, &reference.revision_id)
        .ok_or_else(|| format!("artifact revision not found: {}", reference.revision_id))?;
    preview_frozen_downstream_impact(
        &reference.file_hash,
        ImpactTrigger::Invalidate,
        Some(revision.producer_node.as_str()),
    )
}

pub(crate) fn preview_kind_downstream_impact(
    file_hash: &str,
    kind: ArtifactKind,
) -> Result<DownstreamImpact, String> {
    let graph = baseline_graph_spec();
    let first = graph
        .nodes
        .iter()
        .find(|node| node.inputs.contains(&kind))
        .map(|node| node.id.clone())
        .ok_or_else(|| format!("{kind:?} has no downstream consumer"))?;
    preview_frozen_downstream_impact(
        file_hash,
        ImpactTrigger::SaveAndRunDownstream,
        Some(first.as_str()),
    )
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
