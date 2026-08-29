//! Editable chart boundary for Uta! Studio.
//!
//! Analyzer output is an import source. The editor loads and saves the
//! authoritative UTZ 0.2 vocal chart; target formats such as UltraStar are
//! produced from it at export time.

use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use ts_rs::TS;
use utz::VocalChartV1;

use crate::{
    analysis_artifact::{
        ArtifactRevision, ArtifactStore, load_active_artifact, record_artifact_revision,
        set_active_artifact_revision,
    },
    analysis_graph::{AnalysisNodeId, ArtifactKind},
    artifact_workbench::ArtifactRef,
    audio_format::{browser_can_decode, export_extension, transcode_audio},
    authoring::{get_audio_paths, resolve_original_media},
    cache::{CacheDir, normalize_tempo},
    error::UtaStudioError,
    library_db,
    vocal_chart::{load_saved_or_candidate_chart, migrate_analyzer_chart},
};

fn playable_audio(
    cache: &CacheDir,
    file_hash: &str,
    source_name: &str,
    source_path: &str,
) -> Result<String, UtaStudioError> {
    let source = Path::new(source_path);
    if !source.is_file() {
        return Err(UtaStudioError::Other(format!(
            "audio source is missing: {}",
            source.display()
        )));
    }
    if browser_can_decode(source) {
        return Ok(source.to_string_lossy().into_owned());
    }
    let extension = export_extension(source);
    let output = cache.editor_preview_path(file_hash, source_name, extension);
    let refresh = std::fs::metadata(source)
        .and_then(|source_meta| {
            let source_modified = source_meta.modified()?;
            let output_modified = std::fs::metadata(&output)?.modified()?;
            Ok(source_modified > output_modified)
        })
        .unwrap_or(true);
    if refresh {
        let filename = output
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("preview");
        let temporary = output.with_file_name(format!(".{filename}.tmp.{extension}"));
        let result = transcode_audio(source, &temporary);
        if let Err(error) = result {
            let _ = std::fs::remove_file(temporary);
            return Err(error);
        }
        if output.is_file() {
            std::fs::remove_file(&output)?;
        }
        std::fs::rename(temporary, &output)?;
    }
    Ok(output.to_string_lossy().into_owned())
}

#[derive(Debug, Clone, Serialize)]
pub struct ChartAudio {
    pub instrumental: String,
    pub vocals: Option<String>,
    pub original: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ChartWaveform {
    pub peaks: Vec<(f32, f32)>,
    pub duration_secs: f64,
}

/// Decode a bounded, display-only waveform from an already-authorized chart
/// audio path. Callers run this while native playback is stopped so decoding
/// cannot contend with the GStreamer audition clock.
pub fn decode_chart_waveform(path: &Path) -> Result<ChartWaveform, UtaStudioError> {
    const SAMPLE_RATE: usize = 4_000;
    const PEAK_BUCKETS: usize = 6_000;

    if !path.is_file() {
        return Err(UtaStudioError::Other(format!(
            "waveform source is missing: {}",
            path.display()
        )));
    }
    let output = crate::vendor::silent_command(crate::vendor::ffmpeg_path())
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "4000", "-f", "f32le", "pipe:1"])
        .output()?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(UtaStudioError::Other(if detail.is_empty() {
            format!("ffmpeg could not decode waveform ({})", output.status)
        } else {
            format!("ffmpeg could not decode waveform: {detail}")
        }));
    }
    let samples = output
        .stdout
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .filter(|sample| sample.is_finite())
        .collect::<Vec<_>>();
    if samples.is_empty() {
        return Err(UtaStudioError::Other(
            "decoded waveform contains no audio samples".to_string(),
        ));
    }

    let bucket_count = samples.len().clamp(1, PEAK_BUCKETS);
    let mut peaks = Vec::with_capacity(bucket_count);
    for bucket in 0..bucket_count {
        let start = bucket * samples.len() / bucket_count;
        let end = ((bucket + 1) * samples.len() / bucket_count).max(start + 1);
        let mut minimum = 0.0f32;
        let mut maximum = 0.0f32;
        for sample in &samples[start..end.min(samples.len())] {
            minimum = minimum.min(*sample);
            maximum = maximum.max(*sample);
        }
        peaks.push((minimum.clamp(-1.0, 1.0), maximum.clamp(-1.0, 1.0)));
    }
    Ok(ChartWaveform {
        peaks,
        duration_secs: samples.len() as f64 / SAMPLE_RATE as f64,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ChartDocument {
    pub file_hash: String,
    /// The authoritative authoring document. The editor edits this directly.
    pub vocal_chart: VocalChartV1,
    /// Optional frame-level analyzer evidence, rendered behind the notes. It is
    /// never scoring data and is never written back into the chart.
    pub pitch_track: serde_json::Value,
    pub audio: ChartAudio,
    /// Safe, in-memory repairs applied while importing analyzer output.
    /// Saving the chart persists these normalized timings.
    pub repaired_issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ChartReadiness {
    pub ready: bool,
    pub authoring_ready: bool,
    pub missing: Vec<String>,
    pub blocked_reason: Option<String>,
    pub can_repair_pitch: bool,
}

pub fn chart_readiness(file_hash: &str) -> Result<ChartReadiness, UtaStudioError> {
    let song = library_db::load_song_by_hash(file_hash)
        .map_err(|error| UtaStudioError::Other(error.to_string()))?
        .ok_or_else(|| UtaStudioError::Other(format!("song not found: {file_hash}")))?;
    let only_pitch_missing = !song.authoring_missing.is_empty()
        && song
            .authoring_missing
            .iter()
            .all(|asset| matches!(asset.as_str(), "pitch_track" | "pitch_notes"));
    Ok(ChartReadiness {
        ready: song.editor_ready,
        authoring_ready: song.authoring_ready,
        missing: song.authoring_missing,
        blocked_reason: song.editor_blocked_reason,
        can_repair_pitch: only_pitch_missing
            && song.transcript_source != Some(crate::song::TranscriptSource::Usdx),
    })
}

pub fn load_chart(file_hash: &str) -> Result<ChartDocument, UtaStudioError> {
    let song = library_db::load_song_by_hash(file_hash)
        .map_err(|error| UtaStudioError::Other(error.to_string()))?
        .ok_or_else(|| UtaStudioError::Other(format!("song not found: {file_hash}")))?;

    if song.key_offset != 0 || normalize_tempo(song.tempo) != 1.0 {
        return Err(UtaStudioError::Other(
            "Reset key and tempo before editing the source chart".into(),
        ));
    }

    let cache = CacheDir::new();
    let pitch_track_path = cache.pitch_track_path(file_hash);
    let pitch_track = if pitch_track_path.is_file() {
        read_json(&pitch_track_path, "pitch track")?
    } else {
        serde_json::Value::Null
    };
    let mut repaired_issues = Vec::new();

    let vocal_chart = if let Some(chart) = load_saved_or_candidate_chart(file_hash)? {
        chart
    } else {
        // Legacy songs may still carry only transcript and pitch-note output.
        let mut transcript = read_json(
            &cache.resolve_timed_transcript_path(file_hash),
            "transcript",
        )?;
        let mut pitch_notes = read_json(&cache.pitch_notes_path(file_hash), "pitch notes")?;
        let repaired_timings = normalize_transcript_timings(&mut transcript);
        if repaired_timings > 0 {
            repaired_issues.push(format!(
                "Repaired {repaired_timings} analyzer lyric timing{}",
                if repaired_timings == 1 { "" } else { "s" }
            ));
        }
        let repaired_notes = normalize_pitch_note_timings(&mut pitch_notes);
        if repaired_notes > 0 {
            repaired_issues.push(format!(
                "Repaired {repaired_notes} analyzer pitch note{}",
                if repaired_notes == 1 { "" } else { "s" }
            ));
        }
        validate_transcript(&transcript)?;
        validate_pitch_notes(&pitch_notes)?;
        repaired_issues.push("Prepared the analyzer chart for UTZ 0.2 note-owned lyrics".into());
        migrate_analyzer_chart(&transcript, &pitch_notes)?
    };

    let audio = get_audio_paths(file_hash);
    let original_audio = resolve_original_media(&song, &cache);
    if !Path::new(&audio.instrumental).is_file() {
        return Err(UtaStudioError::Other(
            "instrumental or source audio is not ready".into(),
        ));
    }

    Ok(ChartDocument {
        file_hash: file_hash.to_owned(),
        vocal_chart,
        pitch_track,
        audio: ChartAudio {
            instrumental: playable_audio(&cache, file_hash, "instrumental", &audio.instrumental)?,
            vocals: audio
                .vocals
                .as_deref()
                .map(|path| playable_audio(&cache, file_hash, "vocals", path))
                .transpose()?,
            original: playable_audio(&cache, file_hash, "original", &original_audio)?,
        },
        repaired_issues,
    })
}

fn normalize_transcript_timings(value: &mut serde_json::Value) -> usize {
    let Some(segments) = value
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return 0;
    };
    let mut repaired = 0usize;
    let mut previous_segment_start = 0.0f64;

    for segment in segments {
        let original_start = segment
            .get("start")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(previous_segment_start);
        let segment_start = if original_start.is_finite() {
            original_start.max(0.0).max(previous_segment_start)
        } else {
            previous_segment_start
        };
        if (original_start - segment_start).abs() > f64::EPSILON {
            segment["start"] = serde_json::Value::from(segment_start);
            repaired += 1;
        }

        let original_end = segment
            .get("end")
            .and_then(serde_json::Value::as_f64)
            .filter(|end| end.is_finite())
            .unwrap_or(segment_start);
        let mut furthest_word_end = segment_start;
        if let Some(words) = segment
            .get_mut("words")
            .and_then(serde_json::Value::as_array_mut)
        {
            let original_starts = words
                .iter()
                .map(|word| word.get("start").and_then(serde_json::Value::as_f64))
                .collect::<Vec<_>>();
            let mut previous_word_start = segment_start;
            for (index, word) in words.iter_mut().enumerate() {
                let original_word_start = word
                    .get("start")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(previous_word_start);
                let word_start = if original_word_start.is_finite() {
                    original_word_start
                        .max(segment_start)
                        .max(previous_word_start)
                } else {
                    previous_word_start
                };
                if (original_word_start - word_start).abs() > f64::EPSILON {
                    word["start"] = serde_json::Value::from(word_start);
                    repaired += 1;
                }

                let original_word_end = word
                    .get("end")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(word_start);
                let word_end = if original_word_end.is_finite() && original_word_end > word_start {
                    original_word_end
                } else {
                    original_starts
                        .get(index + 1)
                        .copied()
                        .flatten()
                        .filter(|next| next.is_finite() && *next > word_start)
                        .or_else(|| (original_end > word_start).then_some(original_end))
                        .unwrap_or(word_start + 0.04)
                };
                if (original_word_end - word_end).abs() > f64::EPSILON {
                    word["end"] = serde_json::Value::from(word_end);
                    repaired += 1;
                }
                previous_word_start = word_start;
                furthest_word_end = furthest_word_end.max(word_end);
            }
        }

        let segment_end = original_end
            .max(furthest_word_end)
            .max(segment_start + 0.04);
        if (original_end - segment_end).abs() > f64::EPSILON {
            segment["end"] = serde_json::Value::from(segment_end);
            repaired += 1;
        }
        previous_segment_start = segment_start;
    }
    repaired
}

fn normalize_pitch_note_timings(value: &mut serde_json::Value) -> usize {
    let Some(notes) = value
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return 0;
    };
    let was_sorted = notes.windows(2).all(|pair| {
        pair[0]
            .get("start")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0)
            <= pair[1]
                .get("start")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0)
    });
    notes.sort_by(|left, right| {
        left.get("start")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0)
            .total_cmp(
                &right
                    .get("start")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0),
            )
    });
    let mut repaired = usize::from(!was_sorted);
    for note in notes {
        let original_start = note
            .get("start")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let start = if original_start.is_finite() {
            original_start.max(0.0)
        } else {
            0.0
        };
        let original_end = note
            .get("end")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(start);
        let end = if original_end.is_finite() && original_end > start {
            original_end
        } else {
            start + 0.03
        };
        let original_midi = note
            .get("midi")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(60.0);
        let midi = original_midi.clamp(0.0, 127.0).round();
        let original_confidence = note
            .get("confidence")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0);
        let confidence = original_confidence.clamp(0.0, 1.0);
        let changed = (original_start - start).abs() > f64::EPSILON
            || (original_end - end).abs() > f64::EPSILON
            || (original_midi - midi).abs() > f64::EPSILON
            || (original_confidence - confidence).abs() > f64::EPSILON;
        if changed {
            repaired += 1;
        }
        note["start"] = serde_json::Value::from(start);
        note["end"] = serde_json::Value::from(end);
        note["midi"] = serde_json::Value::from(midi);
        note["confidence"] = serde_json::Value::from(confidence);
    }
    repaired
}

pub fn save_vocal_chart(file_hash: &str, vocal_chart: VocalChartV1) -> Result<(), UtaStudioError> {
    save_vocal_chart_from_revision(file_hash, vocal_chart, None)
}

/// Save a chart working copy derived from a specific immutable revision.
/// The source is recorded in provenance even when it is not the current
/// Active Candidate/Authored revision.
pub fn save_vocal_chart_from_revision(
    file_hash: &str,
    vocal_chart: VocalChartV1,
    source: Option<&ArtifactRef>,
) -> Result<(), UtaStudioError> {
    let song = library_db::load_song_by_hash(file_hash)
        .map_err(|error| UtaStudioError::Other(error.to_string()))?
        .ok_or_else(|| UtaStudioError::Other(format!("song not found: {file_hash}")))?;
    if song.key_offset != 0 || normalize_tempo(song.tempo) != 1.0 {
        return Err(UtaStudioError::Other(
            "Reset key and tempo before saving the source chart".into(),
        ));
    }
    vocal_chart
        .validate()
        .map_err(|error| UtaStudioError::Other(error.to_string()))?;

    // Build the revision away from the compatibility path. The immutable
    // bytes and DB row must exist before `set_active_artifact_revision`
    // atomically updates the canonical editor materialization; a failed
    // save therefore leaves the previous Active chart usable.
    let cache = CacheDir::new();
    let vocal_json = serde_json::to_value(&vocal_chart)?;
    let draft_path = cache.path.join(".artifact-drafts").join(format!(
        "chart-{}-{}.json",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    atomic_write_json(&draft_path, &vocal_json)?;
    let captured = ArtifactStore::new(&cache.path)
        .and_then(|store| store.capture(file_hash, ArtifactKind::AuthoredChart, &draft_path));
    let _ = std::fs::remove_file(&draft_path);
    let (path, content_hash, byte_size) = captured.map_err(UtaStudioError::Other)?;
    let mut input_revisions = source
        .map(|source| vec![source.revision_id.clone()])
        .unwrap_or_default();
    let active_inputs = [
        ArtifactKind::CandidateChart,
        ArtifactKind::TimedTranscript,
        ArtifactKind::PitchNoteCandidates,
    ]
    .into_iter()
    .filter_map(|kind| load_active_artifact(file_hash, kind).map(|revision| revision.id))
    .collect::<Vec<_>>();
    for revision_id in active_inputs {
        if !input_revisions.contains(&revision_id) {
            input_revisions.push(revision_id);
        }
    }
    let revision = ArtifactRevision {
        id: format!(
            "{file_hash}:{}:{content_hash}",
            serde_json::to_string(&ArtifactKind::AuthoredChart)
                .unwrap_or_else(|_| "AuthoredChart".to_string())
        ),
        file_hash: file_hash.to_string(),
        kind: ArtifactKind::AuthoredChart,
        path,
        content_hash,
        producer_node: AnalysisNodeId::new("user.chart_editor"),
        input_revisions,
        config_hash: "user-edit".to_string(),
        algorithm_version: format!("chart-editor-v1/app-{}", env!("CARGO_PKG_VERSION")),
        created_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
        byte_size,
        active: false,
        legacy: false,
        invalidated: false,
    };
    record_artifact_revision(&revision).map_err(UtaStudioError::Other)?;
    set_active_artifact_revision(
        &cache.path,
        file_hash,
        ArtifactKind::AuthoredChart,
        &revision.id,
    )
    .map_err(UtaStudioError::Other)?;
    Ok(())
}

/// Explicit, user-confirmed removal of the active Authored Chart selection.
/// Immutable authored revisions remain non-invalidated for explicit recovery
/// in Artifact Workbench; the next normal load therefore resolves the active
/// Candidate Chart instead of silently resurrecting that history. Reanalysis
/// paths never call this automatically. Callers must gate this behind a
/// confirmation UI; it performs no confirmation of its own.
pub fn delete_authored_chart(file_hash: &str) -> Result<(), UtaStudioError> {
    delete_authored_chart_from_cache(&CacheDir::new(), file_hash)
}

pub(crate) fn delete_authored_chart_from_cache(
    cache: &CacheDir,
    file_hash: &str,
) -> Result<(), UtaStudioError> {
    let chart_path = cache.vocal_chart_path(file_hash);
    if crate::artifact_workbench::authored_chart_is_pinned(file_hash)
        || crate::library_db::analysis_artifact_path_is_pinned(&chart_path).unwrap_or(false)
    {
        return Err(UtaStudioError::Other(
            "the authored chart is pinned; unpin its artifact revision before deleting it".into(),
        ));
    }
    let staged = chart_path.with_file_name(format!(
        ".delete-pending-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        chart_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("authored-chart.json")
    ));
    if chart_path.is_file() {
        std::fs::rename(&chart_path, &staged)?;
    }
    let recovery_source = staged
        .is_file()
        .then_some(staged.as_path())
        .unwrap_or(&chart_path);
    let recovery =
        crate::analysis_artifact::capture_compatibility_recovery_revision(
            cache,
            file_hash,
            ArtifactKind::AuthoredChart,
            recovery_source,
        )
        .map_err(|error| {
            if staged.is_file()
                && let Err(restore_error) = std::fs::rename(&staged, &chart_path)
            {
                return UtaStudioError::Other(format!(
                    "could not preserve the authored chart for recovery: {error}; restoring the chart also failed: {restore_error}"
                ));
            }
            UtaStudioError::Other(format!(
                "could not preserve the authored chart for recovery: {error}"
            ))
        })?;
    let recovery_row = recovery
        .as_ref()
        .map(crate::analysis_artifact::revision_to_row);
    let kind = serde_json::to_string(&ArtifactKind::AuthoredChart)?;
    let deactivated = crate::library_db::analysis_artifacts_deactivate_kind_with_recovery(
        file_hash,
        &kind,
        recovery_row.as_ref(),
    );
    match deactivated {
        Ok(true) => {}
        Ok(false) => {
            if staged.is_file()
                && let Err(error) = std::fs::rename(&staged, &chart_path)
            {
                return Err(UtaStudioError::Other(format!(
                    "the authored chart became pinned and restoring its compatibility file failed: {error}"
                )));
            }
            return Err(UtaStudioError::Other(
                "the authored chart is pinned; unpin its artifact revision before deleting it"
                    .into(),
            ));
        }
        Err(error) => {
            if staged.is_file()
                && let Err(restore_error) = std::fs::rename(&staged, &chart_path)
            {
                return Err(UtaStudioError::Other(format!(
                    "could not update authored chart state: {error}; restoring the compatibility file also failed: {restore_error}"
                )));
            }
            return Err(UtaStudioError::Other(error.to_string()));
        }
    }
    if staged.is_file()
        && let Err(error) = std::fs::remove_file(&staged)
    {
        // The semantic deletion has already committed. A hidden staging file is
        // non-authoritative and can be cleaned later; reporting failure here
        // would falsely imply that the chart deactivation rolled back.
        tracing::warn!(
            "[chart] Authored chart deactivated, but staged compatibility cleanup failed at {}: {error}",
            staged.display()
        );
    }
    Ok(())
}

/// Backwards-compatible name for the explicit authored-chart discard path.
pub fn replace_authored_chart_with_fresh_analysis(file_hash: &str) -> Result<(), UtaStudioError> {
    delete_authored_chart(file_hash)
}

/// Phase 5 §5.1: how a fresh analysis result should reconcile with an
/// existing Authored Chart. `CreateCandidate` (leave the Authored Chart on
/// disk untouched, let the user explicitly compare/replace through
/// `candidate_chart_status`/`replace_authored_chart_with_fresh_analysis`) is
/// the only policy any analysis path actually implements today --
/// `run_pipeline`/`process_song` never touch `vocal_chart.json` regardless
/// of what triggered the run, and nothing currently constructs the other
/// two variants. This enum exists to name that already-true default and the
/// two escape hatches (skip a rerun entirely; or explicitly discard edits
/// and replace) rather than to select between three different code paths
/// today -- see `the immutable artifact contract` §6.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub enum ChartUpdatePolicy {
    KeepAuthoredChart,
    #[default]
    CreateCandidate,
    ReplaceAfterConfirmation,
}

/// Phase 5 §5.4 "查看 Candidate 与 Authored Chart 的摘要差异": counts, not a
/// full field-by-field diff -- `VocalChartV1` carries no detected key/BPM of
/// its own (that lives in `music_analysis.json`, outside the chart), so a
/// deep semantic diff isn't meaningful at this layer. Phrase/note counts and
/// which analyzer inputs actually changed since the chart was last saved are
/// real, useful signals without pretending to be a merge tool.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CandidateChartSummary {
    pub authored_phrase_count: usize,
    pub authored_note_count: usize,
    pub candidate_phrase_count: usize,
    pub candidate_note_count: usize,
    /// `transcript.json` was rewritten after the Authored Chart was last
    /// saved (a transcription/alignment/timed-lyrics rerun happened since).
    pub lyrics_changed: bool,
    /// `pitch_track.json`/`pitch_notes.json` were rewritten after the
    /// Authored Chart was last saved (a pitch rerun happened since).
    pub pitch_evidence_changed: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(tag = "kind", content = "summary")]
pub enum CandidateChartStatus {
    /// No `vocal_chart.json` exists yet -- there is nothing "authored" to
    /// compare a candidate against. `chart_readiness` already covers
    /// first-time editing; this isn't a staleness signal.
    NotAuthoredYet,
    /// An Authored Chart exists and no analyzer output has changed since it
    /// was last saved.
    UpToDate,
    /// An Authored Chart exists, and at least one analyzer output
    /// (transcript or pitch evidence) was rewritten after it was last
    /// saved -- §5.5's "New candidate analysis is available".
    CandidateAvailable(CandidateChartSummary),
}

fn vocal_chart_counts(chart: &VocalChartV1) -> (usize, usize) {
    let phrase_count = chart.tracks.iter().map(|track| track.phrases.len()).sum();
    let note_count = chart
        .tracks
        .iter()
        .flat_map(|track| track.phrases.iter())
        .map(|phrase| phrase.notes.len())
        .sum();
    (phrase_count, note_count)
}

#[cfg(test)]
fn modified_after(path: &Path, reference: std::time::SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map(|mtime| mtime > reference)
        .unwrap_or(false)
}

/// Phase 5 §5.5 "Stale Evidence" staleness check: compares the Authored
/// Chart's own mtime against the analyzer output files it could be rebuilt
/// from. Deliberately mtime-based rather than a versioned `candidate_chart`
/// artifact table (Phase 2's `ArtifactRevision` model doesn't cover
/// `vocal_chart.json` -- it's authored, not analyzer-produced) -- simple,
/// real, and matches what the Authored Chart protection guarantee actually
/// is: "did analysis write something new since I last saved my edits."
pub fn candidate_chart_status(file_hash: &str) -> CandidateChartStatus {
    // Only the explicitly active authored revision is current. Historical
    // non-invalidated revisions remain recoverable through Artifact Workbench
    // after Delete Chart clears the active selection.
    let authored = load_active_artifact(file_hash, ArtifactKind::AuthoredChart);
    let Some(authored) = authored else {
        let compatibility = CacheDir::new().vocal_chart_path(file_hash);
        return if compatibility.is_file()
            && crate::vocal_chart::validate_candidate_chart_path(&compatibility).is_ok()
        {
            CandidateChartStatus::UpToDate
        } else {
            CandidateChartStatus::NotAuthoredYet
        };
    };
    let candidate =
        crate::analysis_artifact::load_artifact_revisions(file_hash, ArtifactKind::CandidateChart)
            .into_iter()
            .filter(|revision| !revision.invalidated)
            .max_by_key(|revision| revision.created_at_ms);
    let Some(candidate) = candidate else {
        return CandidateChartStatus::UpToDate;
    };
    if authored.input_revisions.contains(&candidate.id) {
        return CandidateChartStatus::UpToDate;
    }

    let read_counts = |path: &Path| {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<VocalChartV1>(&bytes).ok())
            .as_ref()
            .map(vocal_chart_counts)
            .unwrap_or((0, 0))
    };
    let (authored_phrase_count, authored_note_count) = read_counts(&authored.path);
    let (candidate_phrase_count, candidate_note_count) = read_counts(&candidate.path);
    let candidate_inputs = candidate
        .input_revisions
        .iter()
        .collect::<std::collections::HashSet<_>>();
    let authored_inputs = authored
        .input_revisions
        .iter()
        .collect::<std::collections::HashSet<_>>();
    let changed = candidate_inputs != authored_inputs;
    CandidateChartStatus::CandidateAvailable(CandidateChartSummary {
        authored_phrase_count,
        authored_note_count,
        candidate_phrase_count,
        candidate_note_count,
        lyrics_changed: changed,
        pitch_evidence_changed: changed,
    })
}

/// Phase 8 §8.2 "Chart 问题计数行" -- deliberately *not* built on
/// `load_chart`: that function resolves `ChartAudio` too
/// (`playable_audio`, which can transcode a non-browser-native source),
/// which is what made a per-render problem count look expensive. Counting
/// problems only ever needs the chart's own structure --
/// `EditorDocument::problems()` takes a bare `VocalChartV1`, nothing about
/// audio -- so this skips that resolution entirely and is cheap enough to
/// call on every render, same as `candidate_chart_status`/
/// `cached_artifact_presence_for_song`. `None` means there is no chart data
/// to count problems in yet (nothing analyzed, or the pieces needed to
/// synthesize a candidate are missing) -- not zero problems.
pub fn chart_problem_count(file_hash: &str) -> Option<usize> {
    chart_problem_count_for(&CacheDir::new(), file_hash)
}

fn chart_problem_count_for(cache: &CacheDir, file_hash: &str) -> Option<usize> {
    let vocal_chart_path = cache.vocal_chart_path(file_hash);
    let vocal_chart: VocalChartV1 = if vocal_chart_path.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&vocal_chart_path).ok()?).ok()?
    } else {
        let mut transcript = read_json(
            &cache.resolve_timed_transcript_path(file_hash),
            "transcript",
        )
        .ok()?;
        let mut pitch_notes = read_json(&cache.pitch_notes_path(file_hash), "pitch notes").ok()?;
        normalize_transcript_timings(&mut transcript);
        normalize_pitch_note_timings(&mut pitch_notes);
        migrate_analyzer_chart(&transcript, &pitch_notes).ok()?
    };
    Some(
        crate::editor::EditorDocument::new(vocal_chart)
            .problems()
            .total(),
    )
}

/// Testable core of `candidate_chart_status`, taking `&CacheDir` so tests
/// can point it at a temp directory instead of the real (and possibly
/// absent) data directory.
#[cfg(test)]
fn candidate_chart_status_for(cache: &CacheDir, file_hash: &str) -> CandidateChartStatus {
    let authored_path = cache.vocal_chart_path(file_hash);
    let Ok(authored_mtime) = std::fs::metadata(&authored_path).and_then(|meta| meta.modified())
    else {
        return CandidateChartStatus::NotAuthoredYet;
    };

    let transcript_path = cache.resolve_timed_transcript_path(file_hash);
    let pitch_notes_path = cache.pitch_notes_path(file_hash);
    let pitch_track_path = cache.pitch_track_path(file_hash);

    let lyrics_changed = modified_after(&transcript_path, authored_mtime);
    let pitch_evidence_changed = modified_after(&pitch_notes_path, authored_mtime)
        || modified_after(&pitch_track_path, authored_mtime);

    if !lyrics_changed && !pitch_evidence_changed {
        return CandidateChartStatus::UpToDate;
    }

    let (candidate_phrase_count, candidate_note_count) = (|| -> Option<(usize, usize)> {
        let mut transcript = read_json(&transcript_path, "transcript").ok()?;
        let mut pitch_notes = read_json(&pitch_notes_path, "pitch notes").ok()?;
        normalize_transcript_timings(&mut transcript);
        normalize_pitch_note_timings(&mut pitch_notes);
        let candidate = migrate_analyzer_chart(&transcript, &pitch_notes).ok()?;
        Some(vocal_chart_counts(&candidate))
    })()
    .unwrap_or((0, 0));

    let (authored_phrase_count, authored_note_count) = std::fs::read_to_string(&authored_path)
        .ok()
        .and_then(|text| serde_json::from_str::<VocalChartV1>(&text).ok())
        .as_ref()
        .map(vocal_chart_counts)
        .unwrap_or((0, 0));

    CandidateChartStatus::CandidateAvailable(CandidateChartSummary {
        authored_phrase_count,
        authored_note_count,
        candidate_phrase_count,
        candidate_note_count,
        lyrics_changed,
        pitch_evidence_changed,
    })
}

fn read_json(path: &Path, label: &str) -> Result<serde_json::Value, UtaStudioError> {
    if !path.is_file() {
        return Err(UtaStudioError::Other(format!("{label} is not ready")));
    }
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn atomic_write_json(destination: &Path, value: &serde_json::Value) -> Result<(), UtaStudioError> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("chart.json");
    let temporary: PathBuf =
        destination.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| -> Result<(), UtaStudioError> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)?;
        let bytes = serde_json::to_vec_pretty(value)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        std::fs::rename(&temporary, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Build the versioned analyzer chart proposal at the Rust schema boundary.
/// The native worker produces the exact timed transcript; Rust combines it with the
/// pitch-note evidence into the same validated UTZ chart type used by the
/// editor. This prevents transcript JSON from masquerading as a chart.
#[cfg(test)]
pub(crate) fn materialize_candidate_chart(
    cache: &CacheDir,
    file_hash: &str,
    transcript_path: &Path,
) -> Result<PathBuf, UtaStudioError> {
    let mut transcript = read_json(transcript_path, "timed transcript")?;
    let mut pitch_notes = read_json(&cache.pitch_notes_path(file_hash), "pitch notes")?;
    normalize_transcript_timings(&mut transcript);
    normalize_pitch_note_timings(&mut pitch_notes);
    let candidate = migrate_analyzer_chart(&transcript, &pitch_notes)?;
    candidate
        .validate()
        .map_err(|error| UtaStudioError::Other(error.to_string()))?;
    let destination = cache.candidate_chart_path(file_hash);
    atomic_write_json(&destination, &serde_json::to_value(candidate)?)?;
    Ok(destination)
}

fn validate_transcript(value: &serde_json::Value) -> Result<(), UtaStudioError> {
    let segments = value
        .get("segments")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| UtaStudioError::Other("transcript.segments must be an array".into()))?;

    let mut previous_segment_start = 0.0;
    for (segment_index, segment) in segments.iter().enumerate() {
        let start = finite_number(segment, "start", "segment", segment_index)?;
        let end = finite_number(segment, "end", "segment", segment_index)?;
        validate_range(start, end, "segment", segment_index)?;
        if segment_index > 0 && start < previous_segment_start {
            return Err(UtaStudioError::Other(format!(
                "segment {segment_index} starts before the preceding segment"
            )));
        }
        previous_segment_start = start;

        let words = segment
            .get("words")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                UtaStudioError::Other(format!("segment {segment_index}.words must be an array"))
            })?;
        let mut previous_word_start = start;
        for (word_index, word) in words.iter().enumerate() {
            let word_start = finite_number(word, "start", "word", word_index)?;
            let word_end = finite_number(word, "end", "word", word_index)?;
            validate_range(word_start, word_end, "word", word_index)?;
            if word_index > 0 && word_start < previous_word_start {
                return Err(UtaStudioError::Other(format!(
                    "segment {segment_index}, word {word_index} starts before the preceding word"
                )));
            }
            if word_start < start - 0.001 || word_end > end + 0.001 {
                return Err(UtaStudioError::Other(format!(
                    "segment {segment_index}, word {word_index} lies outside its segment"
                )));
            }
            if word
                .get("word")
                .and_then(serde_json::Value::as_str)
                .is_none()
            {
                return Err(UtaStudioError::Other(format!(
                    "segment {segment_index}, word {word_index} has no text"
                )));
            }
            previous_word_start = word_start;
        }
    }
    Ok(())
}

fn validate_pitch_notes(value: &serde_json::Value) -> Result<(), UtaStudioError> {
    let notes = value
        .get("notes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| UtaStudioError::Other("pitch_notes.notes must be an array".into()))?;
    let mut previous_start = 0.0;
    for (index, note) in notes.iter().enumerate() {
        let start = finite_number(note, "start", "note", index)?;
        let end = finite_number(note, "end", "note", index)?;
        validate_range(start, end, "note", index)?;
        if index > 0 && start < previous_start {
            return Err(UtaStudioError::Other(format!(
                "note {index} starts before the preceding note"
            )));
        }
        previous_start = start;
        let midi = finite_number(note, "midi", "note", index)?;
        if !(0.0..=127.0).contains(&midi) || midi.fract().abs() > f64::EPSILON {
            return Err(UtaStudioError::Other(format!(
                "note {index} MIDI must be an integer between 0 and 127"
            )));
        }
        let confidence = finite_number(note, "confidence", "note", index)?;
        if !(0.0..=1.0).contains(&confidence) {
            return Err(UtaStudioError::Other(format!(
                "note {index} confidence must be between 0 and 1"
            )));
        }
        if let Some(kind) = note.get("kind") {
            let kind = kind.as_str().ok_or_else(|| {
                UtaStudioError::Other(format!("note {index}.kind must be a string"))
            })?;
            if !matches!(
                kind,
                "normal" | "golden" | "freestyle" | "rap" | "golden_rap"
            ) {
                return Err(UtaStudioError::Other(format!(
                    "note {index}.kind is not a supported note type"
                )));
            }
        }
    }
    Ok(())
}

fn finite_number(
    value: &serde_json::Value,
    field: &str,
    label: &str,
    index: usize,
) -> Result<f64, UtaStudioError> {
    let number = value
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| {
            UtaStudioError::Other(format!("{label} {index}.{field} must be a number"))
        })?;
    if !number.is_finite() {
        return Err(UtaStudioError::Other(format!(
            "{label} {index}.{field} must be finite"
        )));
    }
    Ok(number)
}

fn validate_range(start: f64, end: f64, label: &str, index: usize) -> Result<(), UtaStudioError> {
    if start < 0.0 || end <= start {
        return Err(UtaStudioError::Other(format!(
            "{label} {index} must have 0 <= start < end"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod candidate_chart_status_tests {
    use super::{CandidateChartStatus, candidate_chart_status_for, materialize_candidate_chart};
    use crate::cache::CacheDir;
    use crate::vocal_chart::migrate_analyzer_chart;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn temp_cache_dir(label: &str) -> CacheDir {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-candidate-chart-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp cache dir");
        CacheDir { path }
    }

    fn sample_transcript() -> serde_json::Value {
        serde_json::json!({
            "language": "en",
            "segments": [{
                "text": "hello",
                "start": 1.0,
                "end": 1.5,
                "words": [{"word": "hello", "start": 1.0, "end": 1.5}]
            }]
        })
    }

    fn sample_pitch_notes() -> serde_json::Value {
        serde_json::json!({
            "format_version": 1,
            "notes": [{"start": 1.0, "end": 1.5, "midi": 60, "confidence": 0.9}]
        })
    }

    #[test]
    fn candidate_materialization_writes_a_valid_distinct_chart() {
        let cache = temp_cache_dir("materialize");
        let hash = "songCandidate";
        let transcript_path = cache.timed_transcript_path(hash);
        std::fs::write(
            &transcript_path,
            serde_json::to_vec(&sample_transcript()).unwrap(),
        )
        .unwrap();
        std::fs::write(
            cache.pitch_notes_path(hash),
            serde_json::to_vec(&sample_pitch_notes()).unwrap(),
        )
        .unwrap();

        let path = materialize_candidate_chart(&cache, hash, &transcript_path).unwrap();
        assert_eq!(path, cache.candidate_chart_path(hash));
        assert_ne!(path, transcript_path);
        let chart: utz::VocalChartV1 =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        chart.validate().unwrap();

        cache.clear_all();
    }

    /// Writes `value` as JSON and pins its mtime explicitly, so ordering
    /// between files doesn't depend on real wall-clock sleeps (flaky on
    /// coarse-resolution filesystems).
    fn write_json_at(path: &std::path::Path, value: &serde_json::Value, mtime: SystemTime) {
        std::fs::write(path, serde_json::to_string(value).unwrap()).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(mtime).unwrap();
    }

    #[test]
    fn no_authored_chart_is_not_authored_yet() {
        let cache = temp_cache_dir("no-authored");
        assert!(matches!(
            candidate_chart_status_for(&cache, "songA"),
            CandidateChartStatus::NotAuthoredYet
        ));
        cache.clear_all();
    }

    #[test]
    fn authored_chart_newer_than_everything_is_up_to_date() {
        let cache = temp_cache_dir("up-to-date");
        let base = SystemTime::now();
        write_json_at(&cache.transcript_path("songA"), &sample_transcript(), base);
        write_json_at(
            &cache.pitch_notes_path("songA"),
            &sample_pitch_notes(),
            base,
        );
        let chart = migrate_analyzer_chart(&sample_transcript(), &sample_pitch_notes()).unwrap();
        write_json_at(
            &cache.vocal_chart_path("songA"),
            &serde_json::to_value(&chart).unwrap(),
            base + Duration::from_secs(10),
        );

        assert!(matches!(
            candidate_chart_status_for(&cache, "songA"),
            CandidateChartStatus::UpToDate
        ));
        cache.clear_all();
    }

    #[test]
    fn transcript_rewritten_after_the_authored_chart_reports_a_candidate() {
        let cache = temp_cache_dir("stale-lyrics");
        let base = SystemTime::now();
        write_json_at(
            &cache.pitch_notes_path("songA"),
            &sample_pitch_notes(),
            base,
        );
        let chart = migrate_analyzer_chart(&sample_transcript(), &sample_pitch_notes()).unwrap();
        write_json_at(
            &cache.vocal_chart_path("songA"),
            &serde_json::to_value(&chart).unwrap(),
            base + Duration::from_secs(10),
        );
        write_json_at(
            &cache.transcript_path("songA"),
            &sample_transcript(),
            base + Duration::from_secs(20),
        );

        match candidate_chart_status_for(&cache, "songA") {
            CandidateChartStatus::CandidateAvailable(summary) => {
                assert!(summary.lyrics_changed);
                assert!(!summary.pitch_evidence_changed);
                assert_eq!(summary.authored_note_count, 1);
                assert_eq!(summary.candidate_note_count, 1);
            }
            other => panic!("expected CandidateAvailable, got {other:?}"),
        }
        cache.clear_all();
    }

    #[test]
    fn pitch_rewritten_after_the_authored_chart_reports_a_candidate() {
        let cache = temp_cache_dir("stale-pitch");
        let base = SystemTime::now();
        write_json_at(&cache.transcript_path("songA"), &sample_transcript(), base);
        let chart = migrate_analyzer_chart(&sample_transcript(), &sample_pitch_notes()).unwrap();
        write_json_at(
            &cache.vocal_chart_path("songA"),
            &serde_json::to_value(&chart).unwrap(),
            base + Duration::from_secs(10),
        );
        write_json_at(
            &cache.pitch_notes_path("songA"),
            &sample_pitch_notes(),
            base + Duration::from_secs(20),
        );

        match candidate_chart_status_for(&cache, "songA") {
            CandidateChartStatus::CandidateAvailable(summary) => {
                assert!(!summary.lyrics_changed);
                assert!(summary.pitch_evidence_changed);
            }
            other => panic!("expected CandidateAvailable, got {other:?}"),
        }
        cache.clear_all();
    }

    #[test]
    fn a_different_song_hash_is_unaffected() {
        let cache = temp_cache_dir("cross-song");
        let base = SystemTime::now();
        write_json_at(&cache.transcript_path("songA"), &sample_transcript(), base);
        write_json_at(
            &cache.pitch_notes_path("songA"),
            &sample_pitch_notes(),
            base,
        );
        let chart = migrate_analyzer_chart(&sample_transcript(), &sample_pitch_notes()).unwrap();
        write_json_at(
            &cache.vocal_chart_path("songA"),
            &serde_json::to_value(&chart).unwrap(),
            base + Duration::from_secs(10),
        );

        assert!(matches!(
            candidate_chart_status_for(&cache, "songB"),
            CandidateChartStatus::NotAuthoredYet
        ));
        cache.clear_all();
    }

    #[test]
    fn freshness_is_judged_against_the_dedicated_timed_transcript_file_when_present() {
        // §4.4: a stale compatibility `transcript.json` mtime must not mask
        // a fresh `timed_transcript.json` -- the dedicated file is the real
        // source of truth going forward.
        let cache = temp_cache_dir("dedicated-freshness");
        let base = SystemTime::now();
        // Old compatibility file, written long before the chart.
        write_json_at(&cache.transcript_path("songA"), &sample_transcript(), base);
        write_json_at(
            &cache.pitch_notes_path("songA"),
            &sample_pitch_notes(),
            base,
        );
        let chart = migrate_analyzer_chart(&sample_transcript(), &sample_pitch_notes()).unwrap();
        write_json_at(
            &cache.vocal_chart_path("songA"),
            &serde_json::to_value(&chart).unwrap(),
            base + Duration::from_secs(10),
        );
        // Dedicated file written fresh, after the chart -- must be what
        // freshness is actually judged against.
        write_json_at(
            &cache.timed_transcript_path("songA"),
            &sample_transcript(),
            base + Duration::from_secs(20),
        );

        match candidate_chart_status_for(&cache, "songA") {
            CandidateChartStatus::CandidateAvailable(summary) => {
                assert!(summary.lyrics_changed);
            }
            other => panic!("expected CandidateAvailable, got {other:?}"),
        }
        cache.clear_all();
    }
}

#[cfg(test)]
mod chart_problem_count_tests {
    use super::chart_problem_count_for;
    use crate::cache::CacheDir;
    use crate::vocal_chart::migrate_analyzer_chart;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_cache_dir(label: &str) -> CacheDir {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-chart-problem-count-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp cache dir");
        CacheDir { path }
    }

    fn sample_transcript() -> serde_json::Value {
        serde_json::json!({
            "language": "en",
            "segments": [{
                "text": "hello",
                "start": 1.0,
                "end": 1.5,
                "words": [{"word": "hello", "start": 1.0, "end": 1.5}]
            }]
        })
    }

    fn sample_pitch_notes() -> serde_json::Value {
        serde_json::json!({
            "format_version": 1,
            "notes": [{"start": 1.0, "end": 1.5, "midi": 60, "confidence": 0.9}]
        })
    }

    #[test]
    fn no_data_at_all_returns_none() {
        let cache = temp_cache_dir("no-data");
        assert_eq!(chart_problem_count_for(&cache, "songA"), None);
        cache.clear_all();
    }

    #[test]
    fn synthesizes_a_count_from_analyzer_output_when_not_yet_authored() {
        let cache = temp_cache_dir("synthesized");
        std::fs::write(
            cache.transcript_path("songA"),
            serde_json::to_string(&sample_transcript()).unwrap(),
        )
        .unwrap();
        std::fs::write(
            cache.pitch_notes_path("songA"),
            serde_json::to_string(&sample_pitch_notes()).unwrap(),
        )
        .unwrap();

        assert_eq!(chart_problem_count_for(&cache, "songA"), Some(0));
        cache.clear_all();
    }

    #[test]
    fn reads_the_authored_chart_directly_when_present() {
        let cache = temp_cache_dir("authored");
        let chart = migrate_analyzer_chart(&sample_transcript(), &sample_pitch_notes()).unwrap();
        std::fs::write(
            cache.vocal_chart_path("songA"),
            serde_json::to_string(&chart).unwrap(),
        )
        .unwrap();

        assert_eq!(chart_problem_count_for(&cache, "songA"), Some(0));
        cache.clear_all();
    }

    #[test]
    fn a_different_song_hash_is_isolated() {
        let cache = temp_cache_dir("isolated");
        std::fs::write(
            cache.transcript_path("songA"),
            serde_json::to_string(&sample_transcript()).unwrap(),
        )
        .unwrap();
        std::fs::write(
            cache.pitch_notes_path("songA"),
            serde_json::to_string(&sample_pitch_notes()).unwrap(),
        )
        .unwrap();

        assert_eq!(chart_problem_count_for(&cache, "songB"), None);
        cache.clear_all();
    }

    #[test]
    fn prefers_the_dedicated_timed_transcript_file_over_the_compatibility_one() {
        // §4.4: when both files exist, the dedicated one must win -- a
        // broken/empty compatibility file must not shadow good data in the
        // real source of truth going forward.
        let cache = temp_cache_dir("prefers-dedicated");
        std::fs::write(cache.transcript_path("songA"), b"not valid json").unwrap();
        std::fs::write(
            cache.timed_transcript_path("songA"),
            serde_json::to_string(&sample_transcript()).unwrap(),
        )
        .unwrap();
        std::fs::write(
            cache.pitch_notes_path("songA"),
            serde_json::to_string(&sample_pitch_notes()).unwrap(),
        )
        .unwrap();

        assert_eq!(chart_problem_count_for(&cache, "songA"), Some(0));
        cache.clear_all();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CandidateChartStatus, candidate_chart_status_for, delete_authored_chart_from_cache,
        normalize_pitch_note_timings, normalize_transcript_timings, playable_audio,
        validate_pitch_notes, validate_transcript,
    };
    use crate::{
        analysis_graph::ArtifactKind,
        cache::CacheDir,
        library_db::{
            AnalysisArtifactRow, analysis_active_artifact, analysis_artifact_set_pinned,
            analysis_artifacts_for_kind, analysis_artifacts_publish_batch,
        },
    };
    use std::{
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn failed_non_audio_preview_is_cleaned_without_touching_the_source() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let cache = CacheDir {
            path: std::env::temp_dir().join(format!(
                "uta-studio-editor-audio-source-test-{}-{nonce}",
                std::process::id()
            )),
        };
        std::fs::create_dir_all(&cache.path).unwrap();
        let source = cache.path.join("chart-without-audio.txt");
        std::fs::write(&source, b"not an audio stream").unwrap();

        let error =
            playable_audio(&cache, "isolated", "original", source.to_str().unwrap()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("ffmpeg could not create MP3 audio")
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"not an audio stream");
        assert!(
            !cache
                .editor_preview_path("isolated", "original", "mp3")
                .exists()
        );
        assert!(
            std::fs::read_dir(&cache.path)
                .unwrap()
                .flatten()
                .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp."))
        );
        cache.clear_all();
    }

    #[test]
    fn delete_chart_retires_only_authored_state_and_honors_pins() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "uta-studio-delete-authored-chart-test-{}-{nonce}",
            std::process::id()
        ));
        let cache = CacheDir {
            path: root.join("cache"),
        };
        std::fs::create_dir_all(&cache.path).unwrap();
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "delete-authored-song";
        let source = root.join("source.flac");
        let authored_revision = root.join("immutable-authored.json");
        let candidate_revision = root.join("immutable-candidate.json");
        let evidence_revision = root.join("immutable-evidence.json");
        for path in [
            &source,
            &authored_revision,
            &candidate_revision,
            &evidence_revision,
        ] {
            std::fs::write(path, path.to_string_lossy().as_bytes()).unwrap();
        }
        std::fs::write(cache.vocal_chart_path(file_hash), b"authored compatibility").unwrap();
        std::fs::write(
            cache.candidate_chart_path(file_hash),
            b"candidate compatibility",
        )
        .unwrap();

        let authored_kind = serde_json::to_string(&ArtifactKind::AuthoredChart).unwrap();
        let candidate_kind = serde_json::to_string(&ArtifactKind::CandidateChart).unwrap();
        let evidence_kind = serde_json::to_string(&ArtifactKind::EvidenceBundle).unwrap();
        let row = |id: &str, kind: &str, path: &Path| AnalysisArtifactRow {
            id: id.to_string(),
            file_hash: file_hash.to_string(),
            kind: kind.to_string(),
            path: path.to_string_lossy().into_owned(),
            content_hash: format!("content-{id}"),
            producer_node: "test".to_string(),
            input_revisions: "[]".to_string(),
            config_hash: "config".to_string(),
            algorithm_version: "1".to_string(),
            created_at_ms: 1,
            byte_size: 1,
            active: false,
            legacy: false,
            invalidated: false,
        };
        analysis_artifacts_publish_batch(
            &[
                row("authored", &authored_kind, &authored_revision),
                row("candidate", &candidate_kind, &candidate_revision),
                row("evidence", &evidence_kind, &evidence_revision),
            ],
            &[
                (
                    file_hash.to_string(),
                    authored_kind.clone(),
                    "authored".to_string(),
                ),
                (
                    file_hash.to_string(),
                    candidate_kind.clone(),
                    "candidate".to_string(),
                ),
                (
                    file_hash.to_string(),
                    evidence_kind.clone(),
                    "evidence".to_string(),
                ),
            ],
            &[],
        )
        .unwrap();

        analysis_artifact_set_pinned("authored", true).unwrap();
        assert!(delete_authored_chart_from_cache(&cache, file_hash).is_err());
        assert_eq!(
            std::fs::read(cache.vocal_chart_path(file_hash)).unwrap(),
            b"authored compatibility"
        );
        assert!(
            analysis_active_artifact(file_hash, &authored_kind)
                .unwrap()
                .is_some()
        );

        analysis_artifact_set_pinned("authored", false).unwrap();
        delete_authored_chart_from_cache(&cache, file_hash).unwrap();
        assert!(!cache.vocal_chart_path(file_hash).exists());
        assert!(cache.candidate_chart_path(file_hash).exists());
        assert!(source.exists());
        assert!(
            authored_revision.exists(),
            "immutable provenance is retained"
        );
        let retained_authored = analysis_artifacts_for_kind(file_hash, &authored_kind).unwrap();
        assert!(retained_authored.len() >= 2);
        assert!(
            retained_authored
                .iter()
                .all(|revision| !revision.active && !revision.invalidated)
        );
        assert!(retained_authored.iter().any(|revision| {
            std::fs::read(&revision.path)
                .ok()
                .as_deref()
                .is_some_and(|bytes| bytes == b"authored compatibility")
        }));
        assert!(candidate_revision.exists());
        assert!(evidence_revision.exists());
        assert!(
            analysis_active_artifact(file_hash, &authored_kind)
                .unwrap()
                .is_none()
        );
        assert!(
            analysis_active_artifact(file_hash, &candidate_kind)
                .unwrap()
                .is_some()
        );
        assert!(
            analysis_active_artifact(file_hash, &evidence_kind)
                .unwrap()
                .is_some()
        );

        let legacy_hash = "delete-legacy-authored-song";
        let legacy_chart = crate::vocal_chart::migrate_analyzer_chart(
            &serde_json::json!({
                "segments":[{"text":"legacy","start":1.0,"end":1.5,
                    "words":[{"word":"legacy","start":1.0,"end":1.5}]}]
            }),
            &serde_json::json!({
                "notes":[{"start":1.0,"end":1.5,"midi":60,"confidence":0.9}]
            }),
        )
        .unwrap();
        let legacy_bytes = serde_json::to_vec(&legacy_chart).unwrap();
        std::fs::write(cache.vocal_chart_path(legacy_hash), &legacy_bytes).unwrap();
        assert!(matches!(
            candidate_chart_status_for(&cache, legacy_hash),
            CandidateChartStatus::UpToDate
        ));
        delete_authored_chart_from_cache(&cache, legacy_hash).unwrap();
        assert!(!cache.vocal_chart_path(legacy_hash).exists());
        assert!(matches!(
            candidate_chart_status_for(&cache, legacy_hash),
            CandidateChartStatus::NotAuthoredYet
        ));
        let retained_legacy = analysis_artifacts_for_kind(legacy_hash, &authored_kind).unwrap();
        assert_eq!(retained_legacy.len(), 1);
        assert!(retained_legacy[0].legacy);
        assert!(!retained_legacy[0].active);
        assert!(!retained_legacy[0].invalidated);
        assert_eq!(
            std::fs::read(&retained_legacy[0].path).unwrap(),
            legacy_bytes
        );
        crate::analysis_artifact::set_active_artifact_revision(
            &cache.path,
            legacy_hash,
            ArtifactKind::AuthoredChart,
            &retained_legacy[0].id,
        )
        .unwrap();
        assert_eq!(
            std::fs::read(cache.vocal_chart_path(legacy_hash)).unwrap(),
            legacy_bytes
        );
        assert!(matches!(
            candidate_chart_status_for(&cache, legacy_hash),
            CandidateChartStatus::UpToDate
        ));
        cache.clear_all();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn validates_editor_documents() {
        let transcript = serde_json::json!({
            "language": "en",
            "segments": [{
                "text": "hello",
                "start": 1.0,
                "end": 1.5,
                "words": [{"word": "hello", "start": 1.0, "end": 1.5}]
            }]
        });
        let notes = serde_json::json!({
            "format_version": 1,
            "notes": [{"start": 1.0, "end": 1.5, "midi": 60, "confidence": 0.9}]
        });
        assert!(validate_transcript(&transcript).is_ok());
        assert!(validate_pitch_notes(&notes).is_ok());
    }

    #[test]
    fn normalizes_zero_length_timings_for_the_editor() {
        let mut transcript = serde_json::json!({
            "segments": [{
                "text": "hello world",
                "start": 1.0,
                "end": 1.0,
                "words": [
                    {"word": "hello", "start": 1.0, "end": 1.0},
                    {"word": "world", "start": 1.5, "end": 1.5}
                ]
            }]
        });
        let mut notes = serde_json::json!({
            "notes": [{"start": 2.0, "end": 2.0, "midi": 60.4, "confidence": 1.2}]
        });
        assert!(normalize_transcript_timings(&mut transcript) > 0);
        assert!(normalize_pitch_note_timings(&mut notes) > 0);
        validate_transcript(&transcript).unwrap();
        validate_pitch_notes(&notes).unwrap();
    }

    #[test]
    fn rejects_invalid_midi() {
        let notes = serde_json::json!({
            "notes": [{"start": 1.0, "end": 1.5, "midi": 128, "confidence": 0.9}]
        });
        assert!(validate_pitch_notes(&notes).is_err());
    }

    #[test]
    fn accepts_supported_note_kinds_and_rejects_unknown_ones() {
        let supported = serde_json::json!({
            "notes": [{"start": 1.0, "end": 1.5, "midi": 60, "confidence": 0.9, "kind": "golden"}]
        });
        let unsupported = serde_json::json!({
            "notes": [{"start": 1.0, "end": 1.5, "midi": 60, "confidence": 0.9, "kind": "spoken"}]
        });
        assert!(validate_pitch_notes(&supported).is_ok());
        assert!(validate_pitch_notes(&unsupported).is_err());
    }
}
