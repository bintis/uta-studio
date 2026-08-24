use super::*;
use crate::{
    analysis_artifact::{load_analysis_artifacts, load_artifact_revisions},
    analysis_graph::baseline_graph_spec,
};

pub(crate) fn kind_string(kind: ArtifactKind) -> String {
    serde_json::to_string(&kind).unwrap_or_else(|_| format!("{kind:?}"))
}

pub(crate) fn parse_kind(value: &str) -> Option<ArtifactKind> {
    serde_json::from_str(value).ok()
}

pub(crate) fn media_type(kind: ArtifactKind) -> ArtifactMediaType {
    match kind {
        ArtifactKind::SourceMedia => ArtifactMediaType::SourceMedia,
        ArtifactKind::VocalStem
        | ArtifactKind::InstrumentalStem
        | ArtifactKind::RawVocalStem
        | ArtifactKind::DenoisedVocalStem
        | ArtifactKind::DereverbedVocalStem
        | ArtifactKind::AnalysisVocalStem
        | ArtifactKind::HighQualityInstrumentalStem
        | ArtifactKind::DenoisedInstrumentalStem
        | ArtifactKind::DereverbedInstrumentalStem
        | ArtifactKind::KaraokeInstrumentalStem
        | ArtifactKind::DrumStem
        | ArtifactKind::BassStem
        | ArtifactKind::GuitarStem
        | ArtifactKind::PianoStem
        | ArtifactKind::OtherStem
        | ArtifactKind::AudioStem
        | ArtifactKind::PreprocessedAudio => ArtifactMediaType::Audio,
        ArtifactKind::LyricsInput => ArtifactMediaType::Text,
        ArtifactKind::RecognizedText => ArtifactMediaType::Json,
        ArtifactKind::CandidateChart | ArtifactKind::AuthoredChart => ArtifactMediaType::Chart,
        ArtifactKind::MusicAnalysis
        | ArtifactKind::KeyAnalysis
        | ArtifactKind::RhythmAnalysis
        | ArtifactKind::AudioDescriptors
        | ArtifactKind::PitchTrack
        | ArtifactKind::PitchNoteCandidates
        | ArtifactKind::PitchEvidence
        | ArtifactKind::BoundaryEvidence
        | ArtifactKind::TechniqueEvidence
        | ArtifactKind::AcousticEvidence
        | ArtifactKind::CanonicalLyrics
        | ArtifactKind::TranscriptEvidence
        | ArtifactKind::AlignmentEvidence
        | ArtifactKind::AsrSegments
        | ArtifactKind::TimedTranscript
        | ArtifactKind::EvidenceBundle
        | ArtifactKind::CandidateGraph
        | ArtifactKind::CanonicalSingingTrack
        | ArtifactKind::HumanCorrectionSet => ArtifactMediaType::Json,
    }
}

pub(crate) fn revision_ref(revision: &ArtifactRevision) -> ArtifactRef {
    ArtifactRef {
        file_hash: revision.file_hash.clone(),
        kind: revision.kind,
        revision_id: revision.id.clone(),
    }
}

pub(crate) fn revision_by_id(file_hash: &str, revision_id: &str) -> Option<ArtifactRevision> {
    load_analysis_artifacts(file_hash)
        .into_iter()
        .find(|revision| revision.id == revision_id)
}

pub(crate) fn best_revision(file_hash: &str, kind: ArtifactKind) -> Option<ArtifactRevision> {
    load_active_artifact(file_hash, kind).or_else(|| {
        load_artifact_revisions(file_hash, kind)
            .into_iter()
            .find(|revision| !revision.invalidated)
    })
}

/// Resolves only the concrete output bound to a historical run. It never
/// substitutes today's Active revision when exact history is absent.
pub fn resolve_artifact_for_run(
    file_hash: &str,
    run_id: i64,
    kind: ArtifactKind,
) -> Option<ArtifactRevision> {
    let graph = baseline_graph_spec();
    graph
        .nodes
        .iter()
        .filter(|node| node.outputs.contains(&kind))
        .find_map(|node| {
            let rows = library_db::analysis_node_artifacts_load(run_id, node.id.as_str()).ok()?;
            let revision_id = rows
                .iter()
                .find(|row| {
                    row.direction == "output" && parse_kind(&row.artifact_kind) == Some(kind)
                })?
                .revision_id
                .as_deref()?;
            revision_by_id(file_hash, revision_id)
        })
}

pub(crate) fn binding_from_revision(
    direction: ArtifactDirection,
    slot: String,
    revision: ArtifactRevision,
    exact: bool,
    binding_kind: Option<&str>,
) -> ArtifactBinding {
    let pinned = library_db::analysis_artifact_is_pinned(&revision.id).unwrap_or(false);
    let state = if revision.invalidated {
        ArtifactBindingState::Invalidated
    } else {
        match binding_kind {
            Some("frozen") => ArtifactBindingState::FrozenReuse,
            Some("bypass") => ArtifactBindingState::Bypassed,
            _ if exact => ArtifactBindingState::Resolved,
            _ => ArtifactBindingState::LegacyUntracked,
        }
    };
    ArtifactBinding {
        direction,
        slot,
        kind: revision.kind,
        state,
        artifact_ref: Some(revision_ref(&revision)),
        display_name: revision
            .path
            .file_name()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_else(|| revision.id.clone()),
        path: Some(revision.path.clone()),
        media_type: media_type(revision.kind),
        byte_size: Some(revision.byte_size),
        content_hash: Some(revision.content_hash.clone()),
        producer_node: Some(revision.producer_node.clone()),
        active: revision.active,
        invalidated: revision.invalidated,
        legacy: revision.legacy,
        pinned,
        explanation: (!exact).then(|| {
            "No exact attempt-to-artifact binding was recorded for this run; showing the current best-known revision."
                .to_string()
        }),
    }
}

pub(crate) fn missing_binding(
    direction: ArtifactDirection,
    slot: String,
    kind: ArtifactKind,
) -> ArtifactBinding {
    let state = if kind == ArtifactKind::PreprocessedAudio {
        ArtifactBindingState::Ephemeral
    } else {
        ArtifactBindingState::Missing
    };
    ArtifactBinding {
        direction,
        slot,
        kind,
        state,
        artifact_ref: None,
        display_name: format!("{kind:?}"),
        path: None,
        media_type: if state == ArtifactBindingState::Ephemeral {
            ArtifactMediaType::Ephemeral
        } else {
            media_type(kind)
        },
        byte_size: None,
        content_hash: None,
        producer_node: None,
        active: false,
        invalidated: false,
        legacy: false,
        pinned: false,
        explanation: Some(if state == ArtifactBindingState::Ephemeral {
            "This node output is ephemeral by default and is not retained after the run."
                .to_string()
        } else {
            "No recorded revision is available for this artifact kind.".to_string()
        }),
    }
}

pub(crate) fn source_binding(
    file_hash: &str,
    direction: ArtifactDirection,
    slot: String,
) -> ArtifactBinding {
    let song = library_db::load_song_by_hash(file_hash).ok().flatten();
    let path = song.as_ref().map(|song| song.path.clone());
    let byte_size = path
        .as_ref()
        .and_then(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len());
    ArtifactBinding {
        direction,
        slot,
        kind: ArtifactKind::SourceMedia,
        state: if path.as_ref().is_some_and(|p| p.is_file()) {
            ArtifactBindingState::Source
        } else {
            ArtifactBindingState::Missing
        },
        artifact_ref: None,
        display_name: path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Source media".to_string()),
        path,
        media_type: ArtifactMediaType::SourceMedia,
        byte_size,
        content_hash: None,
        producer_node: None,
        active: true,
        invalidated: false,
        legacy: false,
        pinned: true,
        explanation: Some(
            "Authorized source media is read-only and never treated as a generated revision."
                .to_string(),
        ),
    }
}

pub fn inspect_analysis_node_io(
    file_hash: &str,
    node_id: &str,
    run_id: Option<i64>,
) -> Result<NodeIoInspection, String> {
    let graph = baseline_graph_spec();
    let id = AnalysisNodeId::new(node_id);
    let node = graph
        .node(&id)
        .ok_or_else(|| format!("unknown analysis node: {node_id}"))?;

    let rows = run_id
        .map(|run_id| library_db::analysis_node_artifacts_load(run_id, node_id).unwrap_or_default())
        .unwrap_or_default();
    let exact = !rows.is_empty();

    let resolve_direction = |direction: ArtifactDirection, expected: &[ArtifactKind]| {
        expected
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                let direction_name = match direction {
                    ArtifactDirection::Input => "input",
                    ArtifactDirection::Output => "output",
                };
                let slot = format!("{direction_name}:{index}");
                if *kind == ArtifactKind::SourceMedia {
                    return source_binding(file_hash, direction, slot);
                }
                let row = rows.iter().find(|row| {
                    row.direction == direction_name
                        && row.slot == slot
                        && parse_kind(&row.artifact_kind) == Some(*kind)
                });
                if let Some(row) = row {
                    if row.binding_kind == "ephemeral" {
                        return missing_binding(direction, slot, *kind);
                    }
                    if let Some(revision_id) = row.revision_id.as_deref()
                        && let Some(revision) = revision_by_id(file_hash, revision_id)
                    {
                        return binding_from_revision(
                            direction,
                            slot,
                            revision,
                            true,
                            Some(&row.binding_kind),
                        );
                    }
                }
                best_revision(file_hash, *kind)
                    .map(|revision| {
                        binding_from_revision(direction, slot.clone(), revision, false, None)
                    })
                    .unwrap_or_else(|| missing_binding(direction, slot, *kind))
            })
            .collect::<Vec<_>>()
    };

    Ok(NodeIoInspection {
        file_hash: file_hash.to_string(),
        run_id,
        node_id: id,
        label: node.label.clone(),
        expected_inputs: node.inputs.clone(),
        expected_outputs: node.outputs.clone(),
        resolved_inputs: resolve_direction(ArtifactDirection::Input, &node.inputs),
        resolved_outputs: resolve_direction(ArtifactDirection::Output, &node.outputs),
        exact_run_bindings: exact,
    })
}

pub fn artifact_capabilities(revision: &ArtifactRevision) -> Vec<ArtifactCapability> {
    use ArtifactCapability::*;
    let mut values = vec![Compare, Reveal, Pin];
    match revision.kind {
        ArtifactKind::VocalStem
        | ArtifactKind::InstrumentalStem
        | ArtifactKind::RawVocalStem
        | ArtifactKind::DenoisedVocalStem
        | ArtifactKind::DereverbedVocalStem
        | ArtifactKind::AnalysisVocalStem
        | ArtifactKind::HighQualityInstrumentalStem
        | ArtifactKind::DenoisedInstrumentalStem
        | ArtifactKind::DereverbedInstrumentalStem
        | ArtifactKind::KaraokeInstrumentalStem
        | ArtifactKind::DrumStem
        | ArtifactKind::BassStem
        | ArtifactKind::GuitarStem
        | ArtifactKind::PianoStem
        | ArtifactKind::OtherStem
        | ArtifactKind::AudioStem
        | ArtifactKind::PreprocessedAudio => values.push(PreviewAudio),
        ArtifactKind::LyricsInput => {
            values.push(PreviewText);
            values.push(OpenLyricsEditor);
        }
        ArtifactKind::RecognizedText => {
            values.push(PreviewJson);
            values.push(OpenLyricsEditor);
        }
        ArtifactKind::TimedTranscript | ArtifactKind::AsrSegments => {
            values.push(PreviewJson);
            values.push(OpenLyricsEditor);
        }
        ArtifactKind::PitchTrack
        | ArtifactKind::PitchNoteCandidates
        | ArtifactKind::PitchEvidence
        | ArtifactKind::BoundaryEvidence
        | ArtifactKind::TechniqueEvidence
        | ArtifactKind::AcousticEvidence
        | ArtifactKind::EvidenceBundle
        | ArtifactKind::CandidateGraph
        | ArtifactKind::CanonicalSingingTrack => {
            values.push(PreviewJson);
            values.push(OpenChartEditor);
        }
        ArtifactKind::AuthoredChart | ArtifactKind::CandidateChart => {
            values.push(PreviewJson);
            values.push(OpenChartEditor);
        }
        ArtifactKind::MusicAnalysis
        | ArtifactKind::KeyAnalysis
        | ArtifactKind::RhythmAnalysis
        | ArtifactKind::AudioDescriptors
        | ArtifactKind::CanonicalLyrics
        | ArtifactKind::TranscriptEvidence
        | ArtifactKind::AlignmentEvidence
        | ArtifactKind::HumanCorrectionSet => values.push(PreviewJson),
        ArtifactKind::SourceMedia => values.push(PreviewMetadata),
    }
    if !revision.active && !revision.invalidated {
        values.push(SetActive);
    }
    if !revision.invalidated {
        values.push(Invalidate);
    }
    if !library_db::analysis_artifact_is_pinned(&revision.id).unwrap_or(false) {
        values.push(Delete);
    }
    values
}

pub(crate) fn bounded_read(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > max_bytes as u64 {
        return Err(format!(
            "artifact is {} bytes; in-app structured preview is limited to {} bytes",
            metadata.len(),
            max_bytes
        ));
    }
    std::fs::read(path).map_err(|error| error.to_string())
}

pub(crate) fn validate_pitch_track(value: &serde_json::Value) -> ArtifactHealth {
    let Some(frames) = value.get("frames").and_then(serde_json::Value::as_array) else {
        return ArtifactHealth {
            status: ArtifactHealthStatus::Invalid,
            messages: vec!["Pitch track must contain a frames array.".to_string()],
        };
    };
    let mut errors = Vec::new();
    let mut previous_time = None;
    for (index, frame) in frames.iter().enumerate() {
        let time = frame.get("time").and_then(serde_json::Value::as_f64);
        let confidence = frame.get("confidence").and_then(serde_json::Value::as_f64);
        if !time.is_some_and(|value| value.is_finite() && value >= 0.0) {
            errors.push(format!("Pitch frame {} has an invalid time.", index + 1));
        } else if previous_time.is_some_and(|previous| time.unwrap() < previous) {
            errors.push(format!("Pitch frame {} is out of order.", index + 1));
        }
        if !confidence.is_some_and(|value| value.is_finite() && (0.0..=1.0).contains(&value)) {
            errors.push(format!(
                "Pitch frame {} confidence is outside 0–1.",
                index + 1
            ));
        }
        if let Some(hz) = frame.get("hz").and_then(serde_json::Value::as_f64)
            && (!hz.is_finite() || !(0.0..=20_000.0).contains(&hz))
        {
            errors.push(format!("Pitch frame {} frequency is invalid.", index + 1));
        }
        previous_time = time;
        if errors.len() >= 20 {
            break;
        }
    }
    ArtifactHealth {
        status: if errors.is_empty() {
            ArtifactHealthStatus::Valid
        } else {
            ArtifactHealthStatus::Invalid
        },
        messages: errors,
    }
}

pub(crate) fn validate_pitch_notes(value: &serde_json::Value) -> ArtifactHealth {
    let Some(notes) = value.get("notes").and_then(serde_json::Value::as_array) else {
        return ArtifactHealth {
            status: ArtifactHealthStatus::Invalid,
            messages: vec!["Pitch note candidates must contain a notes array.".to_string()],
        };
    };
    let mut errors = Vec::new();
    let mut previous_start = None;
    for (index, note) in notes.iter().enumerate() {
        let start = note.get("start").and_then(serde_json::Value::as_f64);
        let end = note.get("end").and_then(serde_json::Value::as_f64);
        let midi = note.get("midi").and_then(serde_json::Value::as_f64);
        let confidence = note.get("confidence").and_then(serde_json::Value::as_f64);
        if !matches!((start, end), (Some(start), Some(end)) if start.is_finite() && end.is_finite() && start >= 0.0 && end > start)
        {
            errors.push(format!(
                "Pitch note {} has an invalid time range.",
                index + 1
            ));
        }
        if start.is_some_and(|value| previous_start.is_some_and(|previous| value < previous)) {
            errors.push(format!("Pitch note {} is out of order.", index + 1));
        }
        if !midi.is_some_and(|value| {
            value.is_finite() && (0.0..=127.0).contains(&value) && value.fract() == 0.0
        }) {
            errors.push(format!("Pitch note {} MIDI is outside 0–127.", index + 1));
        }
        if !confidence.is_some_and(|value| value.is_finite() && (0.0..=1.0).contains(&value)) {
            errors.push(format!(
                "Pitch note {} confidence is outside 0–1.",
                index + 1
            ));
        }
        previous_start = start;
        if errors.len() >= 20 {
            break;
        }
    }
    ArtifactHealth {
        status: if errors.is_empty() {
            ArtifactHealthStatus::Valid
        } else {
            ArtifactHealthStatus::Invalid
        },
        messages: errors,
    }
}

pub(crate) fn validate_authored_chart(value: &serde_json::Value) -> ArtifactHealth {
    match serde_json::from_value::<utz::VocalChartV1>(value.clone()) {
        Ok(chart) => match chart.validate() {
            Ok(()) => ArtifactHealth {
                status: ArtifactHealthStatus::Valid,
                messages: Vec::new(),
            },
            Err(error) => ArtifactHealth {
                status: ArtifactHealthStatus::Invalid,
                messages: vec![format!("Authored chart validation failed: {error}")],
            },
        },
        Err(error) => ArtifactHealth {
            status: ArtifactHealthStatus::Invalid,
            messages: vec![format!("Authored chart shape is invalid: {error}")],
        },
    }
}

pub fn artifact_health(revision: &ArtifactRevision) -> ArtifactHealth {
    if !revision.path.is_file() {
        return ArtifactHealth {
            status: ArtifactHealthStatus::Invalid,
            messages: vec!["Backing file is missing.".to_string()],
        };
    }
    if revision.byte_size == 0 {
        return ArtifactHealth {
            status: ArtifactHealthStatus::Warning,
            messages: vec!["Backing file is empty.".to_string()],
        };
    }
    let actual_size = match revision.path.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return ArtifactHealth {
                status: ArtifactHealthStatus::Invalid,
                messages: vec![format!("Backing file metadata could not be read: {error}")],
            };
        }
    };
    if actual_size != revision.byte_size {
        return ArtifactHealth {
            status: ArtifactHealthStatus::Invalid,
            messages: vec![format!(
                "Byte size differs from the committed revision (expected {}, found {}).",
                revision.byte_size, actual_size
            )],
        };
    }
    match media_type(revision.kind) {
        ArtifactMediaType::Json | ArtifactMediaType::Chart => {
            match bounded_read(&revision.path, 8 * 1024 * 1024)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            {
                Some(value) => {
                    let mut messages = Vec::new();
                    let typed = match revision.kind {
                        ArtifactKind::TimedTranscript => validate_timed_transcript(&value),
                        ArtifactKind::PitchTrack => validate_pitch_track(&value),
                        ArtifactKind::PitchNoteCandidates => validate_pitch_notes(&value),
                        ArtifactKind::CandidateChart => validate_authored_chart(&value),
                        ArtifactKind::AuthoredChart => validate_authored_chart(&value),
                        ArtifactKind::RecognizedText | ArtifactKind::AsrSegments
                            if value
                                .get("segments")
                                .and_then(|value| value.as_array())
                                .is_none() =>
                        {
                            ArtifactHealth {
                                status: ArtifactHealthStatus::Invalid,
                                messages: vec!["Transcript has no segments array.".to_string()],
                            }
                        }
                        ArtifactKind::MusicAnalysis
                            if !value.is_object()
                                || value.get("key").is_none()
                                || value.get("rhythm").is_none() =>
                        {
                            ArtifactHealth {
                                status: ArtifactHealthStatus::Invalid,
                                messages: vec![
                                    "Music analysis requires key and rhythm objects.".to_string(),
                                ],
                            }
                        }
                        _ => ArtifactHealth {
                            status: ArtifactHealthStatus::Valid,
                            messages: Vec::new(),
                        },
                    };
                    messages.extend(typed.messages);
                    ArtifactHealth {
                        status: typed.status,
                        messages,
                    }
                }
                None => ArtifactHealth {
                    status: ArtifactHealthStatus::Invalid,
                    messages: vec!["JSON could not be parsed.".to_string()],
                },
            }
        }
        ArtifactMediaType::Text => match bounded_read(&revision.path, 8 * 1024 * 1024) {
            Ok(bytes) if std::str::from_utf8(&bytes).is_ok() => ArtifactHealth {
                status: ArtifactHealthStatus::Valid,
                messages: Vec::new(),
            },
            Ok(_) => ArtifactHealth {
                status: ArtifactHealthStatus::Invalid,
                messages: vec!["Text is not valid UTF-8.".to_string()],
            },
            Err(error) => ArtifactHealth {
                status: ArtifactHealthStatus::Invalid,
                messages: vec![error],
            },
        },
        ArtifactMediaType::Audio | ArtifactMediaType::SourceMedia => {
            match lofty::read_from_path(&revision.path) {
                Ok(file) => {
                    use lofty::file::AudioFile;
                    let properties = file.properties();
                    ArtifactHealth {
                        status: ArtifactHealthStatus::Valid,
                        messages: vec![format!(
                            "Audio metadata decoded: {:.2}s, {} Hz, {} channel(s).",
                            properties.duration().as_secs_f64(),
                            properties.sample_rate().unwrap_or(0),
                            properties.channels().unwrap_or(0)
                        )],
                    }
                }
                Err(error) => ArtifactHealth {
                    status: ArtifactHealthStatus::Invalid,
                    messages: vec![format!("Audio container could not be decoded: {error}")],
                },
            }
        }
        ArtifactMediaType::Binary | ArtifactMediaType::Ephemeral => ArtifactHealth {
            status: ArtifactHealthStatus::Valid,
            messages: vec!["Backing file and committed byte size are valid.".to_string()],
        },
    }
}

pub fn inspect_artifact(reference: &ArtifactRef) -> Result<ArtifactInspection, String> {
    let revision = revision_by_id(&reference.file_hash, &reference.revision_id)
        .ok_or_else(|| format!("artifact revision not found: {}", reference.revision_id))?;
    if revision.kind != reference.kind {
        return Err("artifact reference kind does not match stored revision".to_string());
    }
    Ok(ArtifactInspection {
        pinned: library_db::analysis_artifact_is_pinned(&revision.id).unwrap_or(false),
        media_type: media_type(revision.kind),
        capabilities: artifact_capabilities(&revision),
        health: artifact_health(&revision),
        artifact: revision,
    })
}

pub fn preview_artifact(reference: &ArtifactRef) -> Result<ArtifactPreview, String> {
    let inspection = inspect_artifact(reference)?;
    let revision = inspection.artifact;
    match inspection.media_type {
        ArtifactMediaType::Json | ArtifactMediaType::Chart => {
            let bytes = bounded_read(&revision.path, 256 * 1024)?;
            let value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            Ok(ArtifactPreview::Json(value))
        }
        ArtifactMediaType::Text => {
            let bytes = bounded_read(&revision.path, 256 * 1024)?;
            Ok(ArtifactPreview::Text(
                String::from_utf8_lossy(&bytes).into_owned(),
            ))
        }
        ArtifactMediaType::Audio | ArtifactMediaType::SourceMedia => {
            use lofty::file::AudioFile;
            let metadata = lofty::read_from_path(&revision.path).ok();
            let properties = metadata.as_ref().map(AudioFile::properties);
            Ok(ArtifactPreview::AudioMetadata {
                file_name: revision
                    .path
                    .file_name()
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or_else(|| revision.id.clone()),
                byte_size: revision.byte_size,
                duration_ms: properties.map(|value| value.duration().as_millis() as u64),
                sample_rate: properties.and_then(|value| value.sample_rate()),
                channels: properties.and_then(|value| value.channels()),
            })
        }
        _ => Ok(ArtifactPreview::BinaryMetadata {
            file_name: revision
                .path
                .file_name()
                .map(|x| x.to_string_lossy().into_owned())
                .unwrap_or_else(|| revision.id.clone()),
            byte_size: revision.byte_size,
        }),
    }
}

pub fn set_artifact_pinned(reference: &ArtifactRef, pinned: bool) -> Result<(), String> {
    let revision = revision_by_id(&reference.file_hash, &reference.revision_id)
        .ok_or_else(|| format!("artifact revision not found: {}", reference.revision_id))?;
    if revision.kind != reference.kind {
        return Err("artifact reference kind does not match stored revision".to_string());
    }
    library_db::analysis_artifact_set_pinned(&revision.id, pinned).map_err(|e| e.to_string())
}

pub fn artifact_lineage(reference: &ArtifactRef) -> Result<ArtifactLineage, String> {
    let root = revision_by_id(&reference.file_hash, &reference.revision_id)
        .ok_or_else(|| format!("artifact revision not found: {}", reference.revision_id))?;
    let all = load_analysis_artifacts(&reference.file_hash)
        .into_iter()
        .map(|revision| (revision.id.clone(), revision))
        .collect::<BTreeMap<_, _>>();
    let mut queue = VecDeque::from([(root.id.clone(), 0usize)]);
    let mut visited = BTreeSet::new();
    let mut nodes = Vec::new();
    let mut missing = BTreeSet::new();

    while let Some((id, depth)) = queue.pop_front() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(revision) = all.get(&id).cloned() else {
            missing.insert(id);
            continue;
        };
        for input in &revision.input_revisions {
            queue.push_back((input.clone(), depth + 1));
        }
        nodes.push(ArtifactLineageNode {
            artifact: revision,
            depth,
        });
    }

    let downstream_consumers =
        library_db::analysis_node_artifacts_for_revision(&reference.revision_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|binding| binding.direction == "input")
            .map(|binding| AnalysisNodeId::new(binding.node_id))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    Ok(ArtifactLineage {
        root: reference.clone(),
        nodes,
        missing_revision_ids: missing.into_iter().collect(),
        downstream_consumers,
    })
}
