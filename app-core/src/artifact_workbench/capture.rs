use super::*;

pub(crate) fn committed_kind(value: &str) -> Option<ArtifactKind> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
}

pub(crate) fn relation(
    run_id: i64,
    attempt_id: Option<i64>,
    node_id: &str,
    direction: &str,
    slot: String,
    kind: ArtifactKind,
    revision_id: Option<String>,
    binding_kind: &str,
) -> Result<(), String> {
    library_db::analysis_node_artifact_upsert(&library_db::AnalysisNodeArtifactRow {
        run_id,
        attempt_id,
        node_id: node_id.to_string(),
        direction: direction.to_string(),
        slot,
        artifact_kind: kind_string(kind),
        revision_id,
        binding_kind: binding_kind.to_string(),
    })
    .map_err(|e| e.to_string())
}

/// Finalizes the exact output-boundary events captured while the run was
/// executing. This never scans canonical paths or uses mtimes to infer what
/// happened: an output without a structured commit is recorded as missing.
pub fn capture_analysis_run_artifacts(run_id: i64, file_hash: &str) -> Result<(), String> {
    capture_analysis_run_artifacts_in(&CacheDir::new(), run_id, file_hash)
}

pub(crate) fn capture_analysis_run_artifacts_in(
    cache: &CacheDir,
    run_id: i64,
    file_hash: &str,
) -> Result<(), String> {
    let history = library_db::analysis_history_load(500)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|row| row.id == run_id)
        .ok_or_else(|| format!("analysis run {run_id} not found"))?;
    if history.file_hash != file_hash {
        return Err("analysis run belongs to a different song".to_string());
    }
    let snapshot: crate::analyzer::AnalysisProgressSnapshot =
        serde_json::from_str(&history.snapshot_json)
            .map_err(|error| format!("analysis run snapshot is invalid: {error}"))?;

    let attempts = library_db::analysis_node_attempts_load(run_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|attempt| (attempt.node_id.clone(), attempt))
        .collect::<BTreeMap<_, _>>();
    migrate_artifact_revisions_to_store(cache, file_hash)?;
    let graph = baseline_graph_spec();
    let order = graph.topo_order().map_err(|e| e.to_string())?;
    let mut latest = load_analysis_artifacts(file_hash)
        .into_iter()
        .filter(|revision| !revision.invalidated)
        .fold(
            BTreeMap::<ArtifactKind, ArtifactRevision>::new(),
            |mut map, revision| {
                map.entry(revision.kind).or_insert(revision);
                map
            },
        );

    for node_id in order {
        let Some(spec) = graph.node(&node_id) else {
            continue;
        };
        let Some(attempt) = attempts.get(node_id.as_str()) else {
            continue;
        };
        let route = snapshot
            .stage_routes
            .iter()
            .find(|route| route.node_id.as_deref() == Some(node_id.as_str()));
        if attempt.status == "failed" || attempt.status == "cancelled" {
            continue;
        }

        let mut input_ids = Vec::new();
        for (index, kind) in spec.inputs.iter().copied().enumerate() {
            let slot = format!("input:{index}");
            if kind == ArtifactKind::SourceMedia {
                relation(
                    run_id,
                    Some(attempt.id),
                    node_id.as_str(),
                    "input",
                    slot,
                    kind,
                    None,
                    "source",
                )?;
            } else if kind == ArtifactKind::PreprocessedAudio {
                relation(
                    run_id,
                    Some(attempt.id),
                    node_id.as_str(),
                    "input",
                    slot,
                    kind,
                    None,
                    "ephemeral",
                )?;
            } else if let Some(revision_id) = route
                .and_then(|route| route.input_revision_ids.get(index))
                .and_then(Option::as_ref)
            {
                let revision = latest
                    .get(&kind)
                    .filter(|revision| revision.id == *revision_id)
                    .cloned()
                    .or_else(|| revision_by_id(file_hash, revision_id))
                    .ok_or_else(|| {
                        format!(
                            "{} input:{} references missing revision {}",
                            node_id.as_str(),
                            index,
                            revision_id
                        )
                    })?;
                input_ids.push(revision.id.clone());
                relation(
                    run_id,
                    Some(attempt.id),
                    node_id.as_str(),
                    "input",
                    slot,
                    kind,
                    Some(revision.id.clone()),
                    "revision",
                )?;
            } else if route.is_some_and(|route| route.input_revision_ids.len() > index) {
                relation(
                    run_id,
                    Some(attempt.id),
                    node_id.as_str(),
                    "input",
                    slot,
                    kind,
                    None,
                    "missing",
                )?;
            } else if let Some(revision) = latest.get(&kind) {
                // Compatibility adapter for history captured before exact
                // input selections were embedded in structured routes.
                input_ids.push(revision.id.clone());
                relation(
                    run_id,
                    Some(attempt.id),
                    node_id.as_str(),
                    "input",
                    slot,
                    kind,
                    Some(revision.id.clone()),
                    "legacy_untracked",
                )?;
            } else {
                relation(
                    run_id,
                    Some(attempt.id),
                    node_id.as_str(),
                    "input",
                    slot,
                    kind,
                    None,
                    "missing",
                )?;
            }
        }

        for (index, kind) in spec.outputs.iter().copied().enumerate() {
            let slot = format!("output:{index}");
            if kind == ArtifactKind::PreprocessedAudio {
                relation(
                    run_id,
                    Some(attempt.id),
                    node_id.as_str(),
                    "output",
                    slot,
                    kind,
                    None,
                    "ephemeral",
                )?;
                continue;
            };
            let committed = route.and_then(|route| {
                route.committed_outputs.iter().find(|output| {
                    output.slot == slot && committed_kind(&output.artifact_kind) == Some(kind)
                })
            });
            let Some(committed) = committed else {
                relation(
                    run_id,
                    Some(attempt.id),
                    node_id.as_str(),
                    "output",
                    slot,
                    kind,
                    None,
                    "missing",
                )?;
                continue;
            };
            if let Some(error) = committed.capture_error.as_deref() {
                return Err(format!(
                    "{} {} could not be captured at commit: {error}",
                    node_id.as_str(),
                    slot
                ));
            }
            let immutable_path = committed.immutable_path.clone().ok_or_else(|| {
                format!("{} {} has no immutable commit path", node_id.as_str(), slot)
            })?;
            let content_hash = committed.content_hash.clone().ok_or_else(|| {
                format!(
                    "{} {} has no committed content hash",
                    node_id.as_str(),
                    slot
                )
            })?;
            let byte_size = committed.byte_size.ok_or_else(|| {
                format!("{} {} has no committed byte size", node_id.as_str(), slot)
            })?;
            if hash_file_contents(&immutable_path).map_err(|error| error.to_string())?
                != content_hash
            {
                return Err(format!(
                    "{} {} immutable bytes failed verification",
                    node_id.as_str(),
                    slot
                ));
            }
            let existing = load_artifact_revisions(file_hash, kind)
                .into_iter()
                .find(|revision| revision.content_hash == content_hash);
            let produced = committed.binding_kind == "produced";

            let (revision, record_with_binding) = if produced {
                let id = format!("{file_hash}:{}:{content_hash}", kind_string(kind));
                let revision = ArtifactRevision {
                    id,
                    file_hash: file_hash.to_string(),
                    kind,
                    path: immutable_path,
                    content_hash,
                    producer_node: node_id.clone(),
                    input_revisions: input_ids.clone(),
                    config_hash: if committed.config_hash.is_empty() {
                        format!("run:{run_id}")
                    } else {
                        committed.config_hash.clone()
                    },
                    algorithm_version: if committed.algorithm_version.is_empty() {
                        spec.algorithm_version.clone()
                    } else {
                        committed.algorithm_version.clone()
                    },
                    created_at_ms: history.finished_at_ms,
                    byte_size,
                    active: load_active_artifact(file_hash, kind).is_none(),
                    legacy: false,
                    invalidated: false,
                };
                (revision, true)
            } else if let Some(existing) = existing {
                (existing, false)
            } else {
                // A reuse event can legitimately refer to cache bytes from
                // before inventory existed. Capture them, but do not claim
                // that this attempt produced them.
                let id = format!("{file_hash}:{}:{content_hash}", kind_string(kind));
                let revision = ArtifactRevision {
                    id,
                    file_hash: file_hash.to_string(),
                    kind,
                    path: immutable_path,
                    content_hash,
                    producer_node: AnalysisNodeId::new("legacy.import"),
                    input_revisions: Vec::new(),
                    config_hash: "legacy_unknown".to_string(),
                    algorithm_version: "legacy_unknown".to_string(),
                    created_at_ms: history.finished_at_ms,
                    byte_size,
                    active: load_active_artifact(file_hash, kind).is_none(),
                    legacy: true,
                    invalidated: false,
                };
                (revision, true)
            };

            let binding_kind = committed.binding_kind.as_str();
            let binding = library_db::AnalysisNodeArtifactRow {
                run_id,
                attempt_id: Some(attempt.id),
                node_id: node_id.as_str().to_string(),
                direction: "output".to_string(),
                slot,
                artifact_kind: kind_string(kind),
                revision_id: Some(revision.id.clone()),
                binding_kind: binding_kind.to_string(),
            };
            if record_with_binding {
                library_db::analysis_artifact_and_node_binding_upsert(
                    &crate::analysis_artifact::revision_to_row(&revision),
                    &binding,
                )
                .map_err(|error| error.to_string())?;
            } else {
                library_db::analysis_node_artifact_upsert(&binding)
                    .map_err(|error| error.to_string())?;
            }
            latest.insert(kind, revision);
        }
    }
    Ok(())
}

pub(crate) fn validate_timed_transcript(value: &serde_json::Value) -> ArtifactHealth {
    let Some(segments) = value.get("segments").and_then(serde_json::Value::as_array) else {
        return ArtifactHealth {
            status: ArtifactHealthStatus::Invalid,
            messages: vec!["Timed transcript must contain a segments array.".to_string()],
        };
    };
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut previous_start = None::<f64>;
    let mut previous_end = None::<f64>;
    for (segment_index, segment) in segments.iter().enumerate() {
        let start = segment.get("start").and_then(serde_json::Value::as_f64);
        let end = segment.get("end").and_then(serde_json::Value::as_f64);
        let (Some(start), Some(end)) = (start, end) else {
            errors.push(format!(
                "Segment {} requires numeric start and end times.",
                segment_index + 1
            ));
            continue;
        };
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end < 0.0 {
            errors.push(format!(
                "Segment {} has a negative or non-finite time.",
                segment_index + 1
            ));
        }
        if end < start {
            errors.push(format!(
                "Segment {} ends before it starts.",
                segment_index + 1
            ));
        }
        if previous_start.is_some_and(|previous| start < previous) {
            errors.push(format!(
                "Segment {} is not in chronological order.",
                segment_index + 1
            ));
        }
        if previous_end.is_some_and(|previous| start < previous) {
            warnings.push(format!(
                "Segment {} overlaps the preceding segment.",
                segment_index + 1
            ));
        }
        previous_start = Some(start);
        previous_end = Some(end);

        let words = segment
            .get("words")
            .or_else(|| segment.get("tokens"))
            .and_then(serde_json::Value::as_array);
        if let Some(words) = words {
            let mut previous_word_start = None::<f64>;
            for (word_index, word) in words.iter().enumerate() {
                let word_start = word.get("start").and_then(serde_json::Value::as_f64);
                let word_end = word.get("end").and_then(serde_json::Value::as_f64);
                let (Some(word_start), Some(word_end)) = (word_start, word_end) else {
                    errors.push(format!(
                        "Segment {}, word {} requires numeric start and end times.",
                        segment_index + 1,
                        word_index + 1
                    ));
                    continue;
                };
                if word_start < start || word_end > end || word_end < word_start {
                    errors.push(format!(
                        "Segment {}, word {} falls outside its segment.",
                        segment_index + 1,
                        word_index + 1
                    ));
                }
                if previous_word_start.is_some_and(|previous| word_start < previous) {
                    errors.push(format!(
                        "Segment {}, word {} is not in chronological order.",
                        segment_index + 1,
                        word_index + 1
                    ));
                }
                previous_word_start = Some(word_start);
            }
        }
    }
    if !errors.is_empty() {
        errors.extend(warnings);
        ArtifactHealth {
            status: ArtifactHealthStatus::Invalid,
            messages: errors,
        }
    } else if !warnings.is_empty() {
        ArtifactHealth {
            status: ArtifactHealthStatus::Warning,
            messages: warnings,
        }
    } else {
        ArtifactHealth {
            status: ArtifactHealthStatus::Valid,
            messages: Vec::new(),
        }
    }
}

pub(crate) fn validate_draft_content(
    kind: ArtifactDraftKind,
    content: &ArtifactDraftContent,
) -> ArtifactHealth {
    match (kind, content) {
        (ArtifactDraftKind::Lyrics, ArtifactDraftContent::Text(text)) => {
            if text.contains('\0') {
                ArtifactHealth {
                    status: ArtifactHealthStatus::Invalid,
                    messages: vec!["Lyrics contain a NUL character.".to_string()],
                }
            } else {
                ArtifactHealth {
                    status: if text.trim().is_empty() {
                        ArtifactHealthStatus::Warning
                    } else {
                        ArtifactHealthStatus::Valid
                    },
                    messages: if text.trim().is_empty() {
                        vec!["Lyrics are empty.".to_string()]
                    } else {
                        Vec::new()
                    },
                }
            }
        }
        (ArtifactDraftKind::TimedTranscript, ArtifactDraftContent::Json(value)) => {
            validate_timed_transcript(value)
        }
        (ArtifactDraftKind::StructuredJson, ArtifactDraftContent::Json(_)) => ArtifactHealth {
            status: ArtifactHealthStatus::Valid,
            messages: Vec::new(),
        },
        _ => ArtifactHealth {
            status: ArtifactHealthStatus::Invalid,
            messages: vec!["Draft content does not match its artifact type.".to_string()],
        },
    }
}
