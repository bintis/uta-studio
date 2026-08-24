use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::AtomicU32;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cache::CacheDir;
use crate::library_db;

// ─── Analysis queue (persisted to disk) ──────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum QueuedStatus {
    Queued,
    Analyzing(usize),
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export)]
pub struct AnalysisQueue {
    pub entries: HashMap<String, QueuedStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct AnalysisTask {
    pub file_hash: String,
    pub title: String,
    pub artist: String,
    pub status: QueuedStatus,
    pub live: Option<AnalysisProgressSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisProgressSnapshot {
    pub stage: String,
    /// Whole-run completion reported by the analyzer. Kept separately from
    /// `stage_progress` so the global rail and saved history do not mistake
    /// the current model step's percentage for the complete DAG's progress.
    #[serde(default)]
    pub overall_progress: usize,
    pub stage_progress: usize,
    pub operation: String,
    pub detail: String,
    pub implementation: String,
    pub model: String,
    pub device: String,
    pub requested_device: String,
    pub fallback_from: Option<String>,
    pub fallback_reason: Option<String>,
    pub backend_fallback_from: Option<String>,
    pub backend_fallback_reason: Option<String>,
    pub stage_routes: Vec<AnalysisStageRoute>,
    /// Structured Node event fields (analysis DAG redesign Phase 3,
    /// docs/analysis-dag-redesign.md). `None` for events the native worker
    /// pipeline hasn't migrated to `progress_node`/`artifact_reused` yet,
    /// and always `None` on history rows persisted before this field
    /// existed -- `#[serde(default)]` is required here, not optional
    /// polish: `load_analysis_history` silently drops a row that fails to
    /// deserialize (`.ok()?`), so an old snapshot_json blob without these
    /// keys must still parse.
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub node_event: Option<String>,
    #[serde(default)]
    pub artifact_reused_reason: Option<String>,
    /// One durable, run-scoped analyzer log. Old history snapshots do not
    /// contain this field and remain readable.
    #[serde(default)]
    pub analysis_log_path: Option<PathBuf>,
    /// Exact Engine process-boundary provenance. Legacy runs omit this field.
    #[serde(default)]
    pub engine: Option<EngineRunHistoryProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EngineRunHistoryProjection {
    pub request_id: String,
    pub request_json: String,
    pub request_digest: String,
    pub plan_json: String,
    #[serde(default)]
    pub result_json: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    pub source_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisStageRoute {
    pub stage: String,
    /// The real `AnalysisNodeId` this route was recorded for, when the
    /// emitting call site has migrated to `progress_node`/`artifact_reused`
    /// (`whisper_compat.py`). `None` for routes from a call site still on
    /// the pre-Phase-3 `progress()` path, and for any route recorded before
    /// this field existed (`#[serde(default)]` so old `snapshot_json` rows
    /// keep deserializing -- `load_analysis_history` drops a row that fails
    /// to parse). `stage` (the coarse 7-bucket text) stays authoritative
    /// for those legacy routes; `node_id`, when present, is authoritative
    /// and lets multiple real nodes sharing one bucket (e.g. a compound
    /// node's children) each keep their own route entry instead of
    /// overwriting each other.
    #[serde(default)]
    pub node_id: Option<String>,
    /// The last structured event kind recorded for this node (one of
    /// `node_started`/`node_progress`/`node_completed`/`node_failed`/
    /// `artifact_reused`), independent of the *run's* overall status --
    /// a node that finished successfully earlier in a run that later
    /// failed at a different node must not be reported as failed too.
    /// `None` under the same conditions as `node_id`. Feeds
    /// `analysis_node_attempts.status` (phase plan §2.3).
    #[serde(default)]
    pub node_event: Option<String>,
    /// Exact execution binding for reuse-like terminal events (`frozen`,
    /// `bypassed`, or `cache_hit`). Old snapshots omit it.
    #[serde(default)]
    pub binding_kind: Option<String>,
    /// Outputs announced only after the node atomically committed them.
    /// Rust copies each path into the immutable Artifact Store while the
    /// progress event is handled, before a later node can rewrite a
    /// compatibility materialization.
    #[serde(default)]
    pub committed_outputs: Vec<AnalysisArtifactCommit>,
    /// Revision selected for each declared input slot when this node first
    /// became observable. `None` represents source/ephemeral/missing input.
    #[serde(default)]
    pub input_revision_ids: Vec<Option<String>>,
    pub operation: String,
    pub implementation: String,
    pub model: String,
    pub stage_progress: usize,
    pub requested_device: String,
    pub actual_device: String,
    pub fallback_from: Option<String>,
    pub fallback_reason: Option<String>,
    pub backend_fallback_from: Option<String>,
    pub backend_fallback_reason: Option<String>,
    /// Phase 7 "Duration 检查器字段" gap, closed: real wall-clock timestamps
    /// from the native analyzer process itself (NDJSON progress frame),
    /// not something Rust infers from socket receive time. `started_at_ms`
    /// is set once, the first time this route appears; `finished_at_ms`
    /// only by a terminal event (`node_completed`/`node_failed`/
    /// `artifact_reused`). `#[serde(default)]` so a `snapshot_json` row
    /// written before this field existed keeps deserializing.
    #[serde(default)]
    pub started_at_ms: Option<i64>,
    #[serde(default)]
    pub finished_at_ms: Option<i64>,
    /// Timestamp of the most recently received lifecycle/progress event.
    #[serde(default)]
    pub event_at_ms: Option<i64>,
    /// Real chunk/batch accounting when the model exposes it.
    #[serde(default)]
    pub work_units_completed: Option<u64>,
    #[serde(default)]
    pub work_units_total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisArtifactCommit {
    pub slot: String,
    pub artifact_kind: String,
    pub path: PathBuf,
    #[serde(default)]
    pub binding_kind: String,
    #[serde(default)]
    pub config_hash: String,
    #[serde(default)]
    pub algorithm_version: String,
    #[serde(default)]
    pub immutable_path: Option<PathBuf>,
    #[serde(default)]
    pub content_hash: Option<String>,
    #[serde(default)]
    pub byte_size: Option<u64>,
    #[serde(default)]
    pub capture_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisRunHistory {
    pub id: i64,
    pub file_hash: String,
    pub title: String,
    pub artist: String,
    pub status: String,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub error_message: Option<String>,
    /// Durable run-owned JSONL path. Old rows legitimately have none.
    pub log_path: Option<PathBuf>,
    pub snapshot: AnalysisProgressSnapshot,
}

pub fn load_analysis_history(limit: usize) -> Vec<AnalysisRunHistory> {
    library_db::analysis_history_load(limit)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| {
            let snapshot = serde_json::from_str(&row.snapshot_json).ok()?;
            Some(AnalysisRunHistory {
                id: row.id,
                file_hash: row.file_hash,
                title: row.title,
                artist: row.artist,
                status: row.status,
                started_at_ms: row.started_at_ms,
                finished_at_ms: row.finished_at_ms,
                error_message: row.error_message,
                log_path: row.log_path,
                snapshot,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NodeAttempt {
    pub id: i64,
    pub run_id: i64,
    pub file_hash: String,
    pub node_id: String,
    pub status: String,
    pub progress: i64,
    pub operation: String,
    pub implementation: String,
    pub model: String,
    pub requested_device: String,
    pub actual_device: String,
    pub fallback_from: Option<String>,
    pub fallback_reason: Option<String>,
    pub backend_fallback_from: Option<String>,
    pub backend_fallback_reason: Option<String>,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
}

/// Real per-node attempt records for one run (phase plan §2.3), keyed by
/// `AnalysisRunHistory.id`. Empty for a run whose call sites never migrated
/// to `progress_node`/`artifact_reused` (routes without a `node_id`
/// produce no row -- see `record_node_attempts`), and for any run recorded
/// before this table existed.
pub fn load_analysis_node_attempts(run_id: i64) -> Vec<NodeAttempt> {
    library_db::analysis_node_attempts_load(run_id)
        .unwrap_or_default()
        .into_iter()
        .map(|row| NodeAttempt {
            id: row.id,
            run_id: row.run_id,
            file_hash: row.file_hash,
            node_id: row.node_id,
            status: row.status,
            progress: row.progress,
            operation: row.operation,
            implementation: row.implementation,
            model: row.model,
            requested_device: row.requested_device,
            actual_device: row.actual_device,
            fallback_from: row.fallback_from,
            fallback_reason: row.fallback_reason,
            backend_fallback_from: row.backend_fallback_from,
            backend_fallback_reason: row.backend_fallback_reason,
            started_at_ms: row.started_at_ms,
            finished_at_ms: row.finished_at_ms,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[cfg(test)]
pub(crate) struct HistoricalNodeWeight {
    node_id: String,
    implementation: String,
    actual_device: String,
    duration_ms: u64,
}

#[cfg(test)]
fn median_duration_ms(samples: &mut [u64]) -> u64 {
    samples.sort_unstable();
    let upper = samples.len() / 2;
    if samples.len() % 2 == 1 {
        samples[upper]
    } else {
        let lower_value = samples[upper - 1];
        lower_value + (samples[upper] - lower_value) / 2
    }
}

#[cfg(test)]
pub(crate) fn historical_progress_weights() -> Vec<HistoricalNodeWeight> {
    let mut durations = BTreeMap::<(String, String, String), Vec<u64>>::new();
    for run in load_analysis_history(100)
        .into_iter()
        .filter(|run| run.status == "completed")
    {
        for attempt in load_analysis_node_attempts(run.id)
            .into_iter()
            .filter(|attempt| attempt.status == "succeeded")
        {
            let Some(duration) = attempt
                .finished_at_ms
                .zip(attempt.started_at_ms)
                .and_then(|(finished, started)| finished.checked_sub(started))
                .filter(|duration| *duration > 0)
            else {
                continue;
            };
            durations
                .entry((
                    attempt.node_id,
                    attempt.implementation,
                    attempt.actual_device,
                ))
                .or_default()
                .push(duration as u64);
        }
    }
    durations
        .into_iter()
        .map(
            |((node_id, implementation, actual_device), mut samples)| HistoricalNodeWeight {
                node_id,
                implementation,
                actual_device,
                duration_ms: median_duration_ms(&mut samples).max(1),
            },
        )
        .collect()
}

#[cfg(test)]
mod progress_weight_tests {
    use super::median_duration_ms;

    #[test]
    fn duration_median_handles_odd_even_and_unsorted_samples() {
        assert_eq!(median_duration_ms(&mut [90, 10, 30]), 30);
        assert_eq!(median_duration_ms(&mut [100, 20, 80, 40]), 60);
    }
}

/// One node's attempt in each of two compared runs (Phase 6
/// `compare_analysis_runs`, Phase 7 §7.5 "Compare with previous attempt").
/// `None` on either side means the node wasn't attempted in that run (out
/// of scope for that run's targets, or the run predates the
/// `analysis_node_attempts` writer) -- itself a real, visible difference,
/// not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct NodeAttemptComparison {
    pub node_id: String,
    pub attempt_a: Option<NodeAttempt>,
    pub attempt_b: Option<NodeAttempt>,
    /// Field names that differ, computed only when both attempts are
    /// present -- an attempt existing on just one side is already its own
    /// signal via the `Option`s above, not a per-field diff.
    pub changed_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AnalysisRunComparison {
    pub run_a: AnalysisRunHistory,
    pub run_b: AnalysisRunHistory,
    pub node_differences: Vec<NodeAttemptComparison>,
}

pub(crate) fn node_attempt_changed_fields(a: &NodeAttempt, b: &NodeAttempt) -> Vec<&'static str> {
    let mut changed = Vec::new();
    if a.status != b.status {
        changed.push("status");
    }
    if a.implementation != b.implementation {
        changed.push("implementation");
    }
    if a.model != b.model {
        changed.push("model");
    }
    if a.requested_device != b.requested_device {
        changed.push("requested_device");
    }
    if a.actual_device != b.actual_device {
        changed.push("actual_device");
    }
    if a.fallback_from != b.fallback_from {
        changed.push("fallback_from");
    }
    if a.backend_fallback_from != b.backend_fallback_from {
        changed.push("backend_fallback_from");
    }
    changed
}

/// Testable core of `compare_analysis_runs`: takes already-loaded history
/// and attempt rows so tests can supply fixtures instead of a real DB.
pub(crate) fn compare_analysis_runs_from(
    history: &[AnalysisRunHistory],
    run_id_a: i64,
    attempts_a: Vec<NodeAttempt>,
    run_id_b: i64,
    attempts_b: Vec<NodeAttempt>,
) -> Result<AnalysisRunComparison, String> {
    let run_a = history
        .iter()
        .find(|run| run.id == run_id_a)
        .cloned()
        .ok_or_else(|| format!("analysis run {run_id_a} not found"))?;
    let run_b = history
        .iter()
        .find(|run| run.id == run_id_b)
        .cloned()
        .ok_or_else(|| format!("analysis run {run_id_b} not found"))?;
    if run_a.file_hash != run_b.file_hash {
        return Err("cannot compare analysis runs from two different songs".to_string());
    }

    let mut by_node_a: BTreeMap<String, NodeAttempt> = attempts_a
        .into_iter()
        .map(|attempt| (attempt.node_id.clone(), attempt))
        .collect();
    let mut by_node_b: BTreeMap<String, NodeAttempt> = attempts_b
        .into_iter()
        .map(|attempt| (attempt.node_id.clone(), attempt))
        .collect();
    let node_ids: BTreeSet<String> = by_node_a.keys().chain(by_node_b.keys()).cloned().collect();

    let node_differences = node_ids
        .into_iter()
        .map(|node_id| {
            let attempt_a = by_node_a.remove(&node_id);
            let attempt_b = by_node_b.remove(&node_id);
            let changed_fields = match (&attempt_a, &attempt_b) {
                (Some(a), Some(b)) => node_attempt_changed_fields(a, b),
                _ => Vec::new(),
            };
            NodeAttemptComparison {
                node_id,
                attempt_a,
                attempt_b,
                changed_fields,
            }
        })
        .collect();

    Ok(AnalysisRunComparison {
        run_a,
        run_b,
        node_differences,
    })
}

/// Phase 6 `compare_analysis_runs`: a per-node diff of two runs of the same
/// song, using the real `analysis_node_attempts` rows Phase 2/3 already
/// write. Rejects comparing runs from two different songs -- there is no
/// meaningful "config diff" between unrelated songs' runs.
pub fn compare_analysis_runs(
    run_id_a: i64,
    run_id_b: i64,
) -> Result<AnalysisRunComparison, String> {
    let history = load_analysis_history(500);
    let attempts_a = load_analysis_node_attempts(run_id_a);
    let attempts_b = load_analysis_node_attempts(run_id_b);
    compare_analysis_runs_from(&history, run_id_a, attempts_a, run_id_b, attempts_b)
}

/// §7.5 "Compare with previous attempt": finds the nearest earlier run of
/// the same song and diffs just `node_id`'s attempt between it and
/// `current_run_id`. Deliberately picks the nearest earlier run
/// unconditionally, not the nearest earlier run that happens to have
/// attempted this specific node -- searching arbitrarily far back in
/// history for a node-specific match trades a real cost (one DB round trip
/// per candidate run) for a benefit only a song with very sparse per-node
/// history would ever notice; `attempt_a`/`attempt_b` being `None` already
/// tells the caller a node wasn't attempted in a given run.
pub fn compare_node_attempt_with_previous_run(
    file_hash: &str,
    node_id: &str,
    current_run_id: i64,
) -> Result<NodeAttemptComparison, String> {
    let history = load_analysis_history(500);
    let current_index = history
        .iter()
        .position(|run| run.id == current_run_id)
        .ok_or_else(|| format!("analysis run {current_run_id} not found"))?;
    let previous_run_id = history[current_index + 1..]
        .iter()
        .find(|run| run.file_hash == file_hash)
        .map(|run| run.id)
        .ok_or_else(|| "no earlier analysis run exists for this song".to_string())?;
    let comparison = compare_analysis_runs(current_run_id, previous_run_id)?;
    comparison
        .node_differences
        .into_iter()
        .find(|diff| diff.node_id == node_id)
        .ok_or_else(|| format!("{node_id} has no recorded attempt in either run"))
}

pub fn clear_analysis_history() -> Result<(), String> {
    let referenced_logs =
        library_db::analysis_history_clear().map_err(|error| error.to_string())?;
    let root = crate::cache::uta_studio_dir().join("analysis-logs");
    delete_analysis_logs_in(&root, &referenced_logs)
}

pub(crate) fn delete_analysis_logs_in(
    root: &std::path::Path,
    referenced_logs: &[PathBuf],
) -> Result<(), String> {
    let Ok(canonical_root) = root.canonicalize() else {
        return Ok(());
    };
    let mut failures = Vec::new();
    for referenced in referenced_logs {
        if !referenced.exists() {
            continue;
        }
        let Ok(path) = referenced.canonicalize() else {
            failures.push(format!("{}: could not resolve path", referenced.display()));
            continue;
        };
        if path.parent() != Some(canonical_root.as_path()) || !path.is_file() {
            failures.push(format!(
                "{}: refused because it is outside the analysis-logs root",
                referenced.display()
            ));
            continue;
        }
        if let Err(error) = std::fs::remove_file(&path) {
            failures.push(format!("{}: {error}", path.display()));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "history was cleared, but some analysis logs could not be removed: {}",
            failures.join("; ")
        ))
    }
}

/// Which single primary action a song's detail page should surface, per
/// phase plan §8.1. Backend groundwork for the Phase 8 page restructure
/// (docs/analysis-dag-redesign.md) -- a pure, disk/queue-driven derivation
/// so the eventual UI has one real function to call instead of
/// re-deriving this from scattered booleans inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum SongAuthoringState {
    /// Never analyzed, or reset back to unanalyzed.
    AnalyzeSong,
    /// Currently queued or actively analyzing.
    InProgress,
    /// The last queued run for this song ended in `QueuedStatus::Failed`.
    RetryFailedNode,
    /// Analysis finished but no Authored Chart exists yet.
    OpenEditor,
    /// An Authored Chart exists and the editor can open it normally.
    EditChart,
    /// An Authored Chart exists but the song isn't at base key/tempo, so
    /// the editor is blocked until that's reset (`Song::editor_blocked_reason`).
    FixChartIssues,
}

/// Pure decision core for `resolve_song_authoring_state`, separated out so
/// it's testable without a real DB/cache fixture -- same pattern as
/// `pipeline_flags_for_targets` and the Phase 5 `apply_*_reset` helpers in
/// this file.
///
/// **Known gap** (phase plan §8.1's full state table has one more row this
/// doesn't cover): "有缺失或过期成果物 / Review analysis plan" needs a
/// staleness signal Phase 5 deliberately deferred (no `candidate_chart`
/// artifact or evidence-staleness tracking exists yet) -- see
/// docs/analysis-dag-redesign.md's Phase 5 status note.
pub(crate) fn authoring_state_from_signals(
    queue_status: Option<&QueuedStatus>,
    is_analyzed: bool,
    has_authored_chart: bool,
    editor_ready: bool,
) -> SongAuthoringState {
    if let Some(QueuedStatus::Failed(_)) = queue_status {
        return SongAuthoringState::RetryFailedNode;
    }
    if matches!(
        queue_status,
        Some(QueuedStatus::Queued | QueuedStatus::Analyzing(_))
    ) {
        return SongAuthoringState::InProgress;
    }
    if !is_analyzed {
        return SongAuthoringState::AnalyzeSong;
    }
    if !has_authored_chart {
        return SongAuthoringState::OpenEditor;
    }
    if !editor_ready {
        return SongAuthoringState::FixChartIssues;
    }
    SongAuthoringState::EditChart
}

/// Derives `SongAuthoringState` from real signals only: queue status
/// (`analysis_queue`), `Song.is_analyzed`/`editor_ready`, and whether an
/// Authored Chart file actually exists (`cached_artifact_presence`) --
/// never from a progress percentage or stage text. Returns `None` when the
/// song isn't in the library at all.
pub fn resolve_song_authoring_state(file_hash: &str) -> Option<SongAuthoringState> {
    let song = library_db::load_song_by_hash(file_hash).ok().flatten()?;
    let queue = AnalysisQueue::load();
    let presence = crate::analysis_artifact::cached_artifact_presence(&CacheDir::new(), file_hash);
    let has_chart = crate::analysis_artifact::artifact_present(
        &presence,
        crate::analysis_graph::ArtifactKind::AuthoredChart,
    );
    Some(authoring_state_from_signals(
        queue.entries.get(file_hash),
        song.is_analyzed,
        has_chart,
        song.editor_ready,
    ))
}

pub fn load_analysis_tasks() -> Vec<AnalysisTask> {
    let live = super::control::LIVE_ANALYSIS.lock().unwrap().clone();
    let mut tasks = AnalysisQueue::load()
        .entries
        .into_iter()
        .map(|(file_hash, status)| {
            let song = library_db::load_song_by_hash(&file_hash).ok().flatten();
            AnalysisTask {
                title: song
                    .as_ref()
                    .map(|song| song.title.clone())
                    .unwrap_or_else(|| "Unknown song".into()),
                artist: song
                    .as_ref()
                    .map(|song| song.artist.clone())
                    .unwrap_or_else(|| "Unknown artist".into()),
                live: live.get(&file_hash).cloned(),
                file_hash,
                status,
            }
        })
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| {
        let rank = |status: &QueuedStatus| match status {
            QueuedStatus::Analyzing(_) => 0,
            QueuedStatus::Queued => 1,
            QueuedStatus::Failed(_) => 2,
        };
        rank(&left.status)
            .cmp(&rank(&right.status))
            .then_with(|| left.artist.cmp(&right.artist))
            .then_with(|| left.title.cmp(&right.title))
    });
    tasks
}

impl AnalysisQueue {
    pub fn load() -> Self {
        let entries = library_db::analysis_queue_load_rows()
            .map(|rows| {
                rows.into_iter()
                    .map(|(h, st, pct, msg)| {
                        let status = match st.as_str() {
                            "queued" => QueuedStatus::Queued,
                            "analyzing" => QueuedStatus::Analyzing(pct.unwrap_or(0) as usize),
                            "failed" => QueuedStatus::Failed(msg.unwrap_or_default()),
                            _ => QueuedStatus::Queued,
                        };
                        (h, status)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { entries }
    }

    pub fn save(&self) {
        let rows: Vec<_> = self
            .entries
            .iter()
            .map(|(k, v)| match v {
                QueuedStatus::Queued => (k.clone(), "queued".to_string(), None, None),
                QueuedStatus::Analyzing(p) => {
                    (k.clone(), "analyzing".to_string(), Some(*p as i64), None)
                }
                QueuedStatus::Failed(s) => (k.clone(), "failed".to_string(), None, Some(s.clone())),
            })
            .collect();
        let _ = library_db::analysis_queue_save_rows(&rows);
    }

    pub fn clear() {
        let _ = library_db::analysis_queue_clear();
    }
}
// ─── Server process ──────────────────────────────────────────────────

#[cfg(test)]
pub(crate) static SERVER_PID: AtomicU32 = AtomicU32::new(0);
