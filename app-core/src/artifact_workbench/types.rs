//! Typed inspection, lineage, impact, pinning and post-run artifact capture.
//!
//! This module is intentionally framework-independent. The desktop shell can
//! render these values without reading cache files or inventing lineage.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::analysis_artifact::{ArtifactRevision, load_active_artifact};
use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
use crate::library_db;

use super::inspect::{bounded_read, inspect_artifact, revision_by_id};

fn workbench_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub file_hash: String,
    pub kind: ArtifactKind,
    pub revision_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactBindingState {
    Resolved,
    Source,
    Ephemeral,
    FrozenReuse,
    Bypassed,
    Missing,
    LegacyUntracked,
    Invalidated,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactMediaType {
    SourceMedia,
    Audio,
    Json,
    Text,
    Chart,
    Binary,
    Ephemeral,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactBinding {
    pub direction: ArtifactDirection,
    pub slot: String,
    pub kind: ArtifactKind,
    pub state: ArtifactBindingState,
    pub artifact_ref: Option<ArtifactRef>,
    pub display_name: String,
    pub path: Option<PathBuf>,
    pub media_type: ArtifactMediaType,
    pub byte_size: Option<u64>,
    pub content_hash: Option<String>,
    pub producer_node: Option<AnalysisNodeId>,
    pub active: bool,
    pub invalidated: bool,
    pub legacy: bool,
    pub pinned: bool,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeIoInspection {
    pub file_hash: String,
    pub run_id: Option<i64>,
    pub node_id: AnalysisNodeId,
    pub label: String,
    pub expected_inputs: Vec<ArtifactKind>,
    pub expected_outputs: Vec<ArtifactKind>,
    pub resolved_inputs: Vec<ArtifactBinding>,
    pub resolved_outputs: Vec<ArtifactBinding>,
    pub exact_run_bindings: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactHealthStatus {
    Valid,
    Warning,
    Invalid,
    NotChecked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactHealth {
    pub status: ArtifactHealthStatus,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ArtifactCapability {
    PreviewText,
    PreviewJson,
    PreviewAudio,
    PreviewMetadata,
    OpenLyricsEditor,
    OpenChartEditor,
    Compare,
    SetActive,
    Reveal,
    Pin,
    Invalidate,
    Delete,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactInspection {
    pub artifact: ArtifactRevision,
    pub pinned: bool,
    pub media_type: ArtifactMediaType,
    pub capabilities: Vec<ArtifactCapability>,
    pub health: ArtifactHealth,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum ArtifactPreview {
    Text(String),
    Json(serde_json::Value),
    AudioMetadata {
        file_name: String,
        byte_size: u64,
        duration_ms: Option<u64>,
        sample_rate: Option<u32>,
        channels: Option<u8>,
    },
    BinaryMetadata {
        file_name: String,
        byte_size: u64,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactLineageNode {
    pub artifact: ArtifactRevision,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactLineage {
    pub root: ArtifactRef,
    pub nodes: Vec<ArtifactLineageNode>,
    pub missing_revision_ids: Vec<String>,
    pub downstream_consumers: Vec<AnalysisNodeId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownstreamImpact {
    pub file_hash: String,
    pub node_id: AnalysisNodeId,
    pub affected_nodes: Vec<AnalysisNodeId>,
    pub authored_chart_preserved: bool,
    pub export_may_need_regeneration: bool,
    pub will_run: Vec<AnalysisNodeId>,
    pub will_reuse: Vec<AnalysisNodeId>,
    pub will_become_stale: Vec<AnalysisNodeId>,
    pub will_be_blocked: Vec<AnalysisNodeId>,
    pub will_remain_preserved: Vec<String>,
    pub exports_needing_regeneration: Vec<String>,
    pub queued_targets: Vec<AnalysisNodeId>,
    pub queued_disabled: Vec<AnalysisNodeId>,
    pub queued_frozen: Vec<ArtifactKind>,
    pub queued_bypassed: Vec<AnalysisNodeId>,
    pub request_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactTrigger {
    RunNode,
    RunDownstream,
    SaveAndRunDownstream,
    SetActive,
    Invalidate,
    Delete,
    Freeze,
    Bypass,
    Disable,
    CandidateReplace,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactTypedDiff {
    pub revision_a: ArtifactRef,
    pub revision_b: ArtifactRef,
    pub same_content: bool,
    pub summary: String,
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartRevisionMergeMode {
    ReplaceAll,
    ReplacePhrase { track: usize, phrase: usize },
    ReplaceNoteRange { track: usize, start: u64, end: u64 },
    TakeCandidateLyricsTiming,
    TakeCandidatePitch,
}

/// Load the selected immutable revision into an already-opened chart document.
/// Pitch evidence stays read-only. Candidate/authored bytes replace only the
/// vocal chart. Pitch-note candidates are imported through the existing
/// transcript+notes migration and never write back to the evidence file.
pub fn apply_artifact_revision_to_chart(
    chart: &mut crate::chart::ChartDocument,
    reference: &ArtifactRef,
) -> Result<(), String> {
    if chart.file_hash != reference.file_hash {
        return Err("the selected revision belongs to a different song".into());
    }
    let inspection = inspect_artifact(reference)?;
    let bytes = bounded_read(&inspection.artifact.path, 16 * 1024 * 1024)?;
    match reference.kind {
        ArtifactKind::CandidateChart | ArtifactKind::AuthoredChart => {
            let selected: crate::VocalChartV1 =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            selected.validate().map_err(|error| error.to_string())?;
            chart.vocal_chart = selected;
            Ok(())
        }
        ArtifactKind::PitchTrack => {
            chart.pitch_track =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            Ok(())
        }
        ArtifactKind::PitchNoteCandidates => {
            let selected_notes: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            let transcript = load_active_artifact(
                &reference.file_hash,
                ArtifactKind::TimedTranscript,
            )
            .ok_or_else(|| {
                "Importing pitch-note candidates requires an Active TimedTranscript revision"
                    .to_string()
            })?;
            let transcript_bytes = bounded_read(&transcript.path, 16 * 1024 * 1024)?;
            let transcript: serde_json::Value =
                serde_json::from_slice(&transcript_bytes).map_err(|error| error.to_string())?;
            chart.vocal_chart = crate::migrate_analyzer_chart(&transcript, &selected_notes)
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        _ => Err(format!(
            "{:?} does not open as chart-editor evidence",
            reference.kind
        )),
    }
}

pub fn authored_chart_is_pinned(file_hash: &str) -> bool {
    load_active_artifact(file_hash, ArtifactKind::AuthoredChart)
        .and_then(|revision| library_db::analysis_artifact_is_pinned(&revision.id).ok())
        .unwrap_or(false)
}

/// Create an in-memory authored working copy from exact immutable revisions.
/// Neither source revision nor the canonical chart is modified; persistence
/// still goes through `save_vocal_chart_from_revision` after user review.
pub fn merge_chart_revisions(
    candidate: &ArtifactRef,
    authored: &ArtifactRef,
    mode: ChartRevisionMergeMode,
) -> Result<utz::VocalChartV1, String> {
    if candidate.file_hash != authored.file_hash
        || candidate.kind != ArtifactKind::CandidateChart
        || authored.kind != ArtifactKind::AuthoredChart
    {
        return Err(
            "chart merge requires CandidateChart and AuthoredChart revisions from the same song"
                .into(),
        );
    }
    let load = |reference: &ArtifactRef| -> Result<utz::VocalChartV1, String> {
        let revision = revision_by_id(&reference.file_hash, &reference.revision_id)
            .ok_or_else(|| format!("artifact revision not found: {}", reference.revision_id))?;
        let bytes = bounded_read(&revision.path, 16 * 1024 * 1024)?;
        let chart: utz::VocalChartV1 =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        chart.validate().map_err(|error| error.to_string())?;
        Ok(chart)
    };
    let candidate_chart = load(candidate)?;
    let mut merged = load(authored)?;
    match mode {
        ChartRevisionMergeMode::ReplaceAll => {
            let mut replacement = candidate_chart;
            for (index, track) in replacement.tracks.iter_mut().enumerate() {
                if let Some(metadata) = merged.tracks.get(index) {
                    track.id = metadata.id.clone();
                    track.role = metadata.role;
                    track.part = metadata.part;
                    track.singer = metadata.singer.clone();
                    track.scoring_enabled = metadata.scoring_enabled;
                }
            }
            merged = replacement;
        }
        ChartRevisionMergeMode::ReplacePhrase { track, phrase } => {
            let replacement = candidate_chart
                .tracks
                .get(track)
                .and_then(|track| track.phrases.get(phrase))
                .cloned()
                .ok_or_else(|| "candidate phrase selection is out of range".to_string())?;
            let target = merged
                .tracks
                .get_mut(track)
                .and_then(|track| track.phrases.get_mut(phrase))
                .ok_or_else(|| "authored phrase selection is out of range".to_string())?;
            *target = replacement;
        }
        ChartRevisionMergeMode::ReplaceNoteRange { track, start, end } => {
            if end <= start {
                return Err("note merge range must have start < end".into());
            }
            let source = candidate_chart
                .tracks
                .get(track)
                .ok_or_else(|| "candidate track selection is out of range".to_string())?;
            let target = merged
                .tracks
                .get_mut(track)
                .ok_or_else(|| "authored track selection is out of range".to_string())?;
            let replacement = source
                .phrases
                .iter()
                .flat_map(|phrase| phrase.notes.iter())
                .filter(|note| note.start < end && note.start.saturating_add(note.duration) > start)
                .cloned()
                .collect::<Vec<_>>();
            for phrase in &mut target.phrases {
                phrase.notes.retain(|note| {
                    note.start >= end || note.start.saturating_add(note.duration) <= start
                });
            }
            let phrase_index = target
                .phrases
                .iter()
                .position(|phrase| {
                    phrase.notes.first().is_some_and(|note| note.start <= start)
                        && phrase.notes.last().is_some_and(|note| note.start < end)
                })
                .or((!target.phrases.is_empty()).then_some(0))
                .ok_or_else(|| {
                    "authored track has no phrase to receive the note range".to_string()
                })?;
            let phrase = &mut target.phrases[phrase_index];
            phrase.notes.extend(replacement);
            phrase.notes.sort_by_key(|note| note.start);
            target.phrases.retain(|phrase| !phrase.notes.is_empty());
        }
        ChartRevisionMergeMode::TakeCandidateLyricsTiming
        | ChartRevisionMergeMode::TakeCandidatePitch => {
            let mut source_notes = candidate_chart
                .tracks
                .iter()
                .flat_map(|track| track.phrases.iter())
                .flat_map(|phrase| phrase.notes.iter());
            for target in merged
                .tracks
                .iter_mut()
                .flat_map(|track| track.phrases.iter_mut())
                .flat_map(|phrase| phrase.notes.iter_mut())
            {
                let Some(source) = source_notes.next() else {
                    break;
                };
                if mode == ChartRevisionMergeMode::TakeCandidateLyricsTiming {
                    target.start = source.start;
                    target.duration = source.duration;
                    target.lyrics = source.lyrics.clone();
                } else {
                    target.pitch = source.pitch;
                    target.vocal_mode = source.vocal_mode;
                }
            }
        }
    }
    merged.validate().map_err(|error| error.to_string())?;
    Ok(merged)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureIntermediateRequest {
    pub file_hash: String,
    pub node_id: AnalysisNodeId,
    pub kind: ArtifactKind,
    pub enabled: bool,
    #[serde(default)]
    pub persistent: bool,
}

/// Persist an explicit opt-in for retaining a normally ephemeral boundary.
/// Only the real `lyrics.preprocess -> PreprocessedAudio` boundary is
/// supported; accepting arbitrary pairs would create a misleading setting
/// the analyzer cannot honor.
pub fn set_intermediate_capture_request(
    request: &CaptureIntermediateRequest,
) -> Result<(), String> {
    if request.file_hash.trim().is_empty() {
        return Err("capture request requires a song hash".to_string());
    }
    if request.node_id.as_str() != "lyrics.preprocess"
        || request.kind != ArtifactKind::PreprocessedAudio
    {
        return Err(
            "only lyrics.preprocess PreprocessedAudio can be captured currently".to_string(),
        );
    }
    let kind = serde_json::to_value(request.kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or_else(|| "could not encode capture artifact kind".to_string())?;
    if request.enabled {
        library_db::analysis_capture_request_upsert(&library_db::AnalysisCaptureRequestRow {
            file_hash: request.file_hash.clone(),
            node_id: request.node_id.as_str().to_string(),
            artifact_kind: kind,
            persistent: request.persistent,
            created_at_ms: workbench_now_ms(),
        })
        .map_err(|error| error.to_string())
    } else {
        library_db::analysis_capture_request_delete(
            &request.file_hash,
            request.node_id.as_str(),
            &kind,
        )
        .map_err(|error| error.to_string())
    }
}

pub fn intermediate_capture_request(
    file_hash: &str,
) -> Result<Option<CaptureIntermediateRequest>, String> {
    let kind = serde_json::to_value(ArtifactKind::PreprocessedAudio)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or_else(|| "could not encode capture artifact kind".to_string())?;
    library_db::analysis_capture_request_get(file_hash, "lyrics.preprocess", &kind)
        .map(|row| {
            row.map(|row| CaptureIntermediateRequest {
                file_hash: row.file_hash,
                node_id: AnalysisNodeId::new(row.node_id),
                kind: ArtifactKind::PreprocessedAudio,
                enabled: true,
                persistent: row.persistent,
            })
        })
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactDraftKind {
    Lyrics,
    TimedTranscript,
    StructuredJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "content")]
pub enum ArtifactDraftContent {
    Text(String),
    Json(serde_json::Value),
}

/// An isolated working copy. The source revision and Active selection are
/// retained so saving can detect concurrent changes instead of silently
/// rebasing a user's edits onto different evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactEditDraft {
    pub source: ArtifactRef,
    pub draft_kind: ArtifactDraftKind,
    pub output_kind: ArtifactKind,
    pub original_content_hash: String,
    pub original_active_revision_id: Option<String>,
    pub working_copy: ArtifactDraftContent,
    pub dirty: bool,
    pub validation: ArtifactHealth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactSaveMode {
    SaveOnly,
    SaveAndRunDownstream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSaveOptions {
    pub mode: ArtifactSaveMode,
    pub set_active: bool,
    pub fork_from_old_revision: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactDraftCommit {
    pub revision: ArtifactRevision,
    pub downstream_impact: Option<DownstreamImpact>,
    /// The core never queues work as a side effect of saving. The desktop
    /// confirms this exact preview before submitting a run request.
    pub requires_downstream_confirmation: bool,
}
