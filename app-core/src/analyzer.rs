use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use ts_rs::TS;

use crate::cache::{CacheDir, models_dir};
use crate::config::AppConfig;
use crate::error::UtaStudioError;
use crate::library_db;
use crate::library_model::LibraryMenuFilters;
use crate::lyrics::{fetch_lrclib_lyrics, write_lyrics_file};
use crate::song::{Song, TranscriptSource, read_transcript_meta};

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
    /// docs/analysis-dag-redesign.md). `None` for events the Python
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
    /// from the analyzer process itself (`server.py::_progress_payload`),
    /// not something Rust infers from socket receive time. `started_at_ms`
    /// is set once, the first time this route appears; `finished_at_ms`
    /// only by a terminal event (`node_completed`/`node_failed`/
    /// `artifact_reused`). `#[serde(default)]` so a `snapshot_json` row
    /// written before this field existed keeps deserializing.
    #[serde(default)]
    pub started_at_ms: Option<i64>,
    #[serde(default)]
    pub finished_at_ms: Option<i64>,
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

fn node_attempt_changed_fields(a: &NodeAttempt, b: &NodeAttempt) -> Vec<&'static str> {
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
fn compare_analysis_runs_from(
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
    library_db::analysis_history_clear().map_err(|error| error.to_string())
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
fn authoring_state_from_signals(
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
    let live = LIVE_ANALYSIS.lock().unwrap().clone();
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
use crate::vendor::{analyzer_dir, ffmpeg_path, python_path, silent_command};

// ─── Server process ──────────────────────────────────────────────────

static SERVER_PID: AtomicU32 = AtomicU32::new(0);

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);

struct ServerProcess {
    child: Child,
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let pid = self.child.id();
        info!("[analyzer] Killing server process (pid={pid})");
        SERVER_PID.store(0, Ordering::SeqCst);
        if let Ok(stream) = self.writer.get_ref().try_clone() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static ANALYZER_SERVER: LazyLock<Mutex<Option<ServerProcess>>> = LazyLock::new(|| Mutex::new(None));

#[derive(Debug, Deserialize)]
struct ReadyHandshake {
    port: u16,
    token: String,
    #[serde(default)]
    device: Option<String>,
}

fn drain_lines_to_log<R: BufRead + Send + 'static>(mut reader: R, label: &'static str) {
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    let trimmed = line.trim_end();
                    if !trimmed.is_empty() {
                        info!("[analyzer {label}] {trimmed}");
                    }
                }
            }
        }
    });
}

fn drain_lines_to_log_and_capture<R: BufRead + Send + 'static>(
    mut reader: R,
    label: &'static str,
    captured: Arc<Mutex<VecDeque<String>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    let trimmed = line.trim_end();
                    if trimmed.is_empty() {
                        continue;
                    }
                    info!("[analyzer {label}] {trimmed}");
                    if let Ok(mut lines) = captured.lock() {
                        if lines.len() == 24 {
                            lines.pop_front();
                        }
                        lines.push_back(trimmed.to_string());
                    }
                }
            }
        }
    })
}

fn analyzer_startup_error(
    error: UtaStudioError,
    captured: &Arc<Mutex<VecDeque<String>>>,
) -> UtaStudioError {
    let details = captured
        .lock()
        .ok()
        .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    if details.is_empty() {
        error
    } else {
        UtaStudioError::Other(format!("{error}\nAnalyzer startup stderr:\n{details}"))
    }
}

fn read_ready_handshake<R: BufRead>(reader: &mut R) -> Result<ReadyHandshake, UtaStudioError> {
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Err(UtaStudioError::Other(
                "Analyzer server exited before handshake".into(),
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(value) if value.get("event").and_then(|v| v.as_str()) == Some("ready") => {
                return serde_json::from_value::<ReadyHandshake>(value)
                    .map_err(|e| UtaStudioError::Other(format!("Malformed ready handshake: {e}")));
            }
            _ => {
                info!("[analyzer stdout] {trimmed}");
            }
        }
    }
}

fn connect_and_authenticate(
    port: u16,
    token: &str,
) -> Result<(BufReader<TcpStream>, BufWriter<TcpStream>), UtaStudioError> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let stream = TcpStream::connect_timeout(&addr, HANDSHAKE_TIMEOUT)
        .map_err(|e| UtaStudioError::Other(format!("Failed to connect to analyzer server: {e}")))?;
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;

    let writer_stream = stream
        .try_clone()
        .map_err(|e| UtaStudioError::Other(format!("Failed to clone analyzer socket: {e}")))?;
    let mut reader = BufReader::new(stream);
    let mut writer = BufWriter::new(writer_stream);

    let hello = serde_json::json!({"type": "hello", "token": token});
    writer.write_all(serde_json::to_string(&hello).unwrap().as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    let mut line = String::new();
    let bytes = reader.read_line(&mut line)?;
    if bytes == 0 {
        return Err(UtaStudioError::Other(
            "Analyzer server closed connection during handshake".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_str(line.trim())?;
    if value.get("type").and_then(|v| v.as_str()) != Some("hello_ack") {
        return Err(UtaStudioError::Other(format!(
            "Analyzer auth failed: {}",
            line.trim()
        )));
    }

    reader.get_ref().set_read_timeout(None)?;
    reader.get_ref().set_write_timeout(None)?;

    Ok((reader, writer))
}

fn spawn_server() -> Result<ServerProcess, UtaStudioError> {
    let python = python_path();
    let script = analyzer_dir().join("server.py");
    let models = models_dir();
    let compute_backend = AppConfig::load()
        .compute_backend
        .unwrap_or_else(|| "cpu".to_string());
    let ffmpeg = ffmpeg_path();
    let ffmpeg_dir = ffmpeg.parent().unwrap_or(std::path::Path::new("."));
    let path_env = if let Some(existing) = std::env::var_os("PATH") {
        let mut paths = std::env::split_paths(&existing).collect::<Vec<_>>();
        paths.insert(0, ffmpeg_dir.to_path_buf());
        std::env::join_paths(paths).unwrap_or(existing)
    } else {
        ffmpeg_dir.as_os_str().to_os_string()
    };

    let mut cmd = silent_command(&python);
    cmd.env("PATH", &path_env)
        .env("TORCH_HOME", models.join("torch"))
        .env("HF_HOME", models.join("huggingface"))
        .env("PITCH_MODEL_DIR", models.join("pitch").join("rmvpe"))
        .env("FFMPEG_PATH", &ffmpeg)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONWARNINGS", "ignore")
        .env("PYTORCH_ENABLE_MPS_FALLBACK", "1")
        .env("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")
        .env("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
        .env("NLTK_DATA", models.join("nltk_data"))
        .env("NEMO_CACHE_DIR", models.join("nemo"))
        .env("ONNX_ASR_CACHE_DIR", models.join("onnx_asr"))
        .env(
            "OPENVINO_WHISPER_MODEL_DIR",
            models.join("whisper").join("openvino-large-v3-turbo"),
        )
        .env("OPENVINO_SEPARATOR_MODEL_DIR", models.join("separation"))
        .env("UTA_STUDIO_COMPUTE_BACKEND", compute_backend)
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| UtaStudioError::Other(format!("Failed to start analyzer server: {e}")))?;
    let pid = child.id();
    SERVER_PID.store(pid, Ordering::SeqCst);
    info!("[analyzer] Server process spawned (pid={pid})");

    let startup_stderr = Arc::new(Mutex::new(VecDeque::new()));
    let stderr_drain = match child.stderr.take() {
        Some(stderr) => drain_lines_to_log_and_capture(
            BufReader::new(stderr),
            "stderr",
            Arc::clone(&startup_stderr),
        ),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            SERVER_PID.store(0, Ordering::SeqCst);
            return Err(UtaStudioError::Other(
                "Failed to capture server stderr".into(),
            ));
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            SERVER_PID.store(0, Ordering::SeqCst);
            return Err(UtaStudioError::Other(
                "Failed to capture server stdout".into(),
            ));
        }
    };
    let mut stdout_reader = BufReader::new(stdout);

    let handshake = match read_ready_handshake(&mut stdout_reader) {
        Ok(h) => h,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_drain.join();
            SERVER_PID.store(0, Ordering::SeqCst);
            return Err(analyzer_startup_error(e, &startup_stderr));
        }
    };
    if let Some(device) = handshake.device.as_deref() {
        info!(
            "[analyzer] Handshake ok: device={device} port={}",
            handshake.port
        );
    } else {
        info!("[analyzer] Handshake ok: port={}", handshake.port);
    }

    let (reader, writer) = match connect_and_authenticate(handshake.port, &handshake.token) {
        Ok(pair) => pair,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_drain.join();
            SERVER_PID.store(0, Ordering::SeqCst);
            return Err(analyzer_startup_error(e, &startup_stderr));
        }
    };

    drain_lines_to_log(stdout_reader, "stdout");
    drop(stderr_drain);

    Ok(ServerProcess {
        child,
        reader,
        writer,
    })
}

fn ensure_server(
    guard: &mut std::sync::MutexGuard<Option<ServerProcess>>,
) -> Result<(), UtaStudioError> {
    if guard.is_some() {
        return Ok(());
    }
    let server = spawn_server()?;
    **guard = Some(server);
    Ok(())
}

// ─── Queue state ─────────────────────────────────────────────────────

struct AnalyzerState {
    queue: VecDeque<String>,
    active_hash: Option<String>,
    worker_running: bool,
}

static ANALYZER: LazyLock<Mutex<AnalyzerState>> = LazyLock::new(|| {
    Mutex::new(AnalyzerState {
        queue: VecDeque::new(),
        active_hash: None,
        worker_running: false,
    })
});

static LIVE_ANALYSIS: LazyLock<Mutex<HashMap<String, AnalysisProgressSnapshot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ANALYSIS_STARTED: LazyLock<Mutex<HashMap<String, i64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

/// Maps a route's last recorded structured event kind
/// (`AnalysisStageRoute.node_event`) to an `analysis_node_attempts.status`
/// value, independent of the enclosing run's overall status -- a node that
/// finished successfully earlier in a run that later failed at a different
/// node must not be reported as failed too. Unset/unrecognized events (a
/// node that was targeted but never reached a terminal event before the
/// run itself ended, e.g. the run failed at an earlier node first) map to
/// "incomplete".
fn node_attempt_status(node_event: Option<&str>) -> &'static str {
    match node_event {
        Some("node_completed") => "succeeded",
        Some("node_failed") => "failed",
        Some("artifact_reused") => "reused",
        _ => "incomplete",
    }
}

/// Persists one `analysis_node_attempts` row per route that carries a real
/// node id (routes from a pre-Phase-3 call site, `node_id: None`, produce
/// no row -- the same Legacy Adapter boundary Phase 3 draws elsewhere).
/// Reuses `stage_routes`, already accumulated for the run's history
/// snapshot, rather than intercepting individual progress events -- no new
/// event-handling path, so nothing about how a run actually executes
/// changes; this only adds a durable, queryable record of what
/// `stage_routes` already captured in memory for the run's lifetime.
fn record_node_attempts(run_id: i64, file_hash: &str, snapshot: &AnalysisProgressSnapshot) {
    let attempts: Vec<library_db::NewAnalysisNodeAttempt> = snapshot
        .stage_routes
        .iter()
        .filter_map(|route| {
            let node_id = route.node_id.as_deref()?;
            Some(library_db::NewAnalysisNodeAttempt {
                node_id,
                status: node_attempt_status(route.node_event.as_deref()),
                progress: route.stage_progress as i64,
                operation: &route.operation,
                implementation: &route.implementation,
                model: &route.model,
                requested_device: &route.requested_device,
                actual_device: &route.actual_device,
                fallback_from: route.fallback_from.as_deref(),
                fallback_reason: route.fallback_reason.as_deref(),
                backend_fallback_from: route.backend_fallback_from.as_deref(),
                backend_fallback_reason: route.backend_fallback_reason.as_deref(),
                started_at_ms: route.started_at_ms,
                finished_at_ms: route.finished_at_ms,
            })
        })
        .collect();
    let _ = library_db::analysis_node_attempts_insert_batch(run_id, file_hash, &attempts);
}

fn finish_analysis_history(file_hash: &str, status: &str, error_message: Option<&str>) {
    let Some(started_at_ms) = ANALYSIS_STARTED.lock().unwrap().remove(file_hash) else {
        return;
    };
    let Some(song) = library_db::load_song_by_hash(file_hash).ok().flatten() else {
        return;
    };
    let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get(file_hash).cloned() else {
        return;
    };
    let Ok(snapshot_json) = serde_json::to_string(&snapshot) else {
        return;
    };
    let Ok(run_id) = library_db::analysis_history_insert(
        file_hash,
        &song.title,
        &song.artist,
        status,
        started_at_ms,
        unix_time_ms(),
        &snapshot_json,
        error_message,
    ) else {
        return;
    };
    record_node_attempts(run_id, file_hash, &snapshot);
}

fn update_live_analysis(file_hash: &str, snapshot: AnalysisProgressSnapshot) {
    LIVE_ANALYSIS
        .lock()
        .unwrap()
        .insert(file_hash.to_string(), snapshot);
}

/// Per-hash node targeting for the *next* queued run, replacing the old
/// `FORCE_TRANSCRIBE`/`STEMS_ONLY`/`PITCH_ONLY` `HashSet<String>` trio
/// (analysis DAG redesign Phase 4, phase plan §4). `targets`/`disabled_nodes`
/// are resolved into `skip_transcription`/`skip_separation`/`skip_pitch`
/// booleans through `analysis_plan::build_plan`
/// (`pipeline_flags_for_request` below) instead of three independent
/// boolean special cases -- the Python wire protocol is unchanged; only how
/// Rust decides those booleans changed.
#[derive(Debug, Clone, Default)]
struct PendingNodeIntent {
    targets: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    /// Distinct from `targets`: this is an input-source override ("don't
    /// try LRCLIB, go straight to ASR"), not a node-skip decision, so it
    /// doesn't belong in the Planner's targeting closure.
    force_transcribe: bool,
    /// (original_path, backup_path) pairs from a reset that renamed old
    /// output aside instead of deleting it outright, resolved by
    /// `restore_or_commit_backup` once `process_song` learns how the
    /// triggered run finished (docs/analysis-dag-redesign.md Phase 5, phase
    /// plan §9.2 "失败时保留旧 Pitch" -- deleting eagerly, before the rerun
    /// is even queued, meant a failed/crashed/OOM-killed rerun destroyed
    /// the previous good output for nothing).
    backup_paths: Vec<(PathBuf, PathBuf)>,
    /// Phase 4's generic executor gap closer: nodes to actually turn off for
    /// this one run, set by `run_analysis_plan` (and, through it,
    /// `disable_analysis_node_for_run`). Empty for every legacy special-case
    /// function (`reanalyze_pitch`, `mark_stems_only`, `realign`, ...) --
    /// they only ever add to `targets`, never disable anything.
    disabled_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    /// Phase 4 §4.5's Freeze consumer: artifact kinds whose *current*
    /// on-disk output must be force-reused for this run even if the active
    /// config would otherwise consider them stale (different separator
    /// options, etc.) -- set only by `freeze_analysis_node_outputs_for_run`.
    /// Empty for every other caller, including every legacy special-case
    /// function. Distinct from ordinary cache-hit reuse (which already
    /// happens for free when nothing changed): Freeze exists specifically
    /// to keep old output even though current settings would produce
    /// something different.
    frozen_artifacts: BTreeSet<crate::analysis_graph::ArtifactKind>,
    /// Phase 4 §4.5's Bypass consumer: nodes to route around using their
    /// designated alternate input for this run (today: only
    /// `stems.separate`, bypassed with the Original Mix) -- set only by
    /// `bypass_analysis_node_with_original_mix_for_run`. Empty for every
    /// other caller.
    bypassed_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    /// Phase 8's Run tier: a one-run-only override for a single
    /// profile-controlled field, set only by `configure_analysis_node_for_run`.
    /// `None` for every legacy special-case caller. Drained the same
    /// one-shot way as every other field here -- real precisely because it
    /// only ever applies to the one run it was set for.
    run_override: Option<(crate::analysis_profile::ProfileField, String)>,
}

static PENDING_NODE_INTENTS: LazyLock<Mutex<HashMap<String, PendingNodeIntent>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Phase 4 §4.1 "Enqueue 时冻结配置": the config snapshot a queued job will
/// run with, captured the moment it actually joins the queue (not a fresh
/// `AppConfig::load()` at execution time). Without this, changing global
/// separator/model/device settings while a job sits in the queue -- not yet
/// started -- silently changed what that already-queued job would run
/// with, contradicting "全局设置在任务排队后变化，只影响之后新建的任务". Drained
/// (removed) by `process_song` when the job actually starts, so this map
/// only ever holds entries for jobs that are queued but not yet running.
static FROZEN_CONFIGS: LazyLock<Mutex<HashMap<String, AppConfig>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Resolves (and drains) the config a job actually runs with: the snapshot
/// frozen at enqueue time if one exists -- checked under both the current
/// and pre-rekey hash, since a remote song's hash can change between
/// enqueue and this point -- or `fallback` for a job with no frozen entry
/// (e.g. one enqueued by an older build mid-upgrade, so this can never
/// panic or block a run outright).
fn resolve_frozen_config(
    file_hash: &str,
    initial_hash: &str,
    fallback: impl FnOnce() -> AppConfig,
) -> AppConfig {
    let mut frozen = FROZEN_CONFIGS.lock().unwrap();
    frozen
        .remove(file_hash)
        .or_else(|| {
            if file_hash != initial_hash {
                frozen.remove(initial_hash)
            } else {
                None
            }
        })
        .unwrap_or_else(fallback)
}

/// Resolves which real pipeline booleans a set of Planner targets implies,
/// by asking the Phase 1 domain model what would actually run -- rather
/// than hand-maintaining the equivalence between "which special flag" and
/// "which boolean" the way the three old `HashSet`s required. The lyrics
/// route passed in is irrelevant whenever no lyrics node is targeted (the
/// common case for both former STEMS_ONLY and PITCH_ONLY intents), so a
/// fixed placeholder is safe here.
/// Shared plan-building step behind every `pipeline_flags_*` variant below.
/// Empty `targets` means "no special intent was stashed for this run" --
/// self-contained default of "run everything," so callers don't have to
/// duplicate that empty-check themselves.
fn build_execution_plan(
    targets: &BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    disabled_nodes: &BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    frozen_artifacts: &BTreeSet<crate::analysis_graph::ArtifactKind>,
    bypassed_nodes: &BTreeSet<crate::analysis_graph::AnalysisNodeId>,
) -> Result<crate::analysis_plan::AnalysisPlan, crate::analysis_plan::PlanError> {
    use crate::analysis_graph::{AnalysisNodeId, baseline_graph_spec};
    use crate::analysis_plan::{AnalysisRequest, LyricsRoute, build_plan};
    use crate::analysis_profile::AnalysisProfileSnapshot;

    let graph = baseline_graph_spec();
    let effective_targets = if targets.is_empty() {
        BTreeSet::from([AnalysisNodeId::new("chart.build_candidate")])
    } else {
        targets.clone()
    };
    let request = AnalysisRequest {
        file_hash: String::new(),
        targets: effective_targets,
        disabled_nodes: disabled_nodes.clone(),
        frozen_artifacts: frozen_artifacts.clone(),
        bypassed_nodes: bypassed_nodes.clone(),
        lyrics_route: LyricsRoute::WhisperAsr,
        model_availability: BTreeMap::new(),
        profile_snapshot: AnalysisProfileSnapshot::default(),
    };
    build_plan(&graph, &request)
}

/// Reads the three pipeline-honorable booleans off an already-built plan:
/// which of lyrics/stems/pitch actually needs to run. `run_pipeline` has no
/// finer-grained hook than these three today (docs/analysis-dag-redesign.md
/// Phase 4 status note) -- every other node (`music.*`, `preflight`,
/// `chart.build_candidate`) is computed unconditionally regardless of what
/// the plan says.
fn pipeline_flags_from_plan(
    plan: &crate::analysis_plan::AnalysisPlan,
) -> (bool, bool, bool, bool, bool, bool) {
    use crate::analysis_graph::AnalysisNodeId;
    use crate::analysis_plan::NodeState;

    let will_run = |id: &str| {
        plan.node(&AnalysisNodeId::new(id))
            .map(|n| n.will_run)
            .unwrap_or(false)
    };
    let node_state_is = |id: &str, state: NodeState| {
        plan.node(&AnalysisNodeId::new(id))
            .map(|n| n.state == state)
            .unwrap_or(false)
    };
    let lyrics_ran = will_run("lyrics.transcribe")
        || will_run("lyrics.align")
        || will_run("lyrics.import_timed");
    let freeze_separation = node_state_is("stems.separate", NodeState::Frozen);
    let freeze_pitch = node_state_is("pitch.extract", NodeState::Frozen);
    let bypass_separation = node_state_is("stems.separate", NodeState::Bypassed);
    // A Frozen node must still be "run" from the pipeline's point of view --
    // it needs to hand its cached output (the vocals path, for stems) to
    // whatever downstream node actually executes this run. A Bypassed node
    // is the opposite: it genuinely does not run (no separation call at
    // all) -- `pipeline.py` substitutes the Original Mix as the vocals path
    // instead, which is exactly what `skip_separation` already models
    // (`vocals_path` stays unset by the separation call itself), so no
    // extra exemption is needed here for it the way Frozen needs one.
    let skip_separation = !will_run("stems.separate") && !freeze_separation;
    let skip_pitch = !will_run("pitch.extract") && !freeze_pitch;
    (
        !lyrics_ran,
        skip_separation,
        skip_pitch,
        freeze_separation,
        freeze_pitch,
        bypass_separation,
    )
}

/// Resolves which real pipeline booleans a request implies, by asking the
/// Phase 1 domain model what would actually run -- rather than
/// hand-maintaining the equivalence between "which special flag" and "which
/// boolean" the way the three old `HashSet`s required. Also honors
/// `disabled_nodes` and surfaces the plan's own warnings *for the nodes the
/// caller actually tried to disable* (e.g. "can't disable an
/// `AlwaysRequired` node") instead of silently dropping them.
///
/// Deliberately does **not** reject on a warning for some other,
/// merely-downstream node that ends up `Blocked` as an expected
/// *consequence* of the disable (e.g. disabling `pitch.extract` blocks
/// `chart.build_candidate`, since nothing this run supplies its
/// `PitchNoteCandidates` input another way -- docs/analysis-dag-redesign.md
/// §6, `DisablePolicy::Optional`'s own doc comment: "downstream nodes
/// become Blocked unless a Freeze or Bypass supplies their input another
/// way"). That's the disable working as designed, not a failure -- and
/// `run_pipeline` doesn't actually gate its own final chart-writing step on
/// pitch data being present (a missing pitch guide falls back to real-time
/// pitchy detection), so nothing downstream is unsafe to still run. Every
/// legacy special-case caller (which never disables anything) just passes
/// an empty `disabled_nodes` set, so this is a no-op change for them.
fn pipeline_flags_for_request(
    targets: &BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    disabled_nodes: &BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    frozen_artifacts: &BTreeSet<crate::analysis_graph::ArtifactKind>,
    bypassed_nodes: &BTreeSet<crate::analysis_graph::AnalysisNodeId>,
) -> Result<(bool, bool, bool, bool, bool, bool), Vec<crate::analysis_plan::PlanWarning>> {
    let plan = build_execution_plan(targets, disabled_nodes, frozen_artifacts, bypassed_nodes)
        .map_err(|_| Vec::new())?;
    let rejected_disables: Vec<_> = plan
        .warnings
        .iter()
        .filter(|warning| disabled_nodes.contains(&warning.node))
        .cloned()
        .collect();
    if !rejected_disables.is_empty() {
        return Err(rejected_disables);
    }
    Ok(pipeline_flags_from_plan(&plan))
}

/// Node ids `run_pipeline` can currently honor an explicit disable request
/// for. Every other Optional node (`music.descriptors`) is computed
/// unconditionally inside `analyze_music`'s single atomic call alongside
/// `music.key`/`music.rhythm` -- accepting a disable request for it would
/// silently have no effect on what Python actually runs, so
/// `run_analysis_plan` rejects it up front instead.
fn pipeline_can_honor_disable(id: &crate::analysis_graph::AnalysisNodeId) -> bool {
    matches!(
        id.as_str(),
        "stems.separate"
            | "pitch.extract"
            | "lyrics.preprocess"
            | "lyrics.transcribe"
            | "lyrics.align"
            | "lyrics.import_timed"
    )
}

/// Node ids `run_pipeline` can currently honor an explicit Freeze request
/// for -- narrower than `pipeline_can_honor_disable`. Freezing means "force
/// this node's *current* on-disk output to survive this run even though the
/// active config might otherwise consider it stale," which only makes sense
/// for a node whose output is (a) a standalone cache file `run_pipeline` can
/// locate on its own and (b) subject to config-driven invalidation in the
/// first place. `stems.separate` qualifies on both counts (separator
/// options can invalidate its cache). `pitch.extract` qualifies on (a); it
/// has no config-driven invalidation today (its cache-hit check is pure
/// file-existence), so freezing it is currently equivalent to ordinary
/// cache reuse -- wired anyway for API/UI symmetry and so it stays correct
/// if pitch ever grows parameterized cache invalidation. The lyrics nodes
/// don't qualify: their output is merged into the single `transcript.json`
/// (Phase 4 §4.4 artifact splitting hasn't happened), so there is no
/// standalone file to freeze independently of the whole transcript.
fn pipeline_can_honor_freeze(id: &crate::analysis_graph::AnalysisNodeId) -> bool {
    matches!(id.as_str(), "stems.separate" | "pitch.extract")
}

/// Maps a freezable node id to the `ArtifactKind`s its output is made of,
/// for populating `AnalysisRequest.frozen_artifacts` (which the Phase 1
/// planner keys by kind, not node id).
fn frozen_artifact_kinds_for_node(
    id: &crate::analysis_graph::AnalysisNodeId,
) -> BTreeSet<crate::analysis_graph::ArtifactKind> {
    use crate::analysis_graph::ArtifactKind;
    match id.as_str() {
        "stems.separate" => {
            BTreeSet::from([ArtifactKind::VocalStem, ArtifactKind::InstrumentalStem])
        }
        "pitch.extract" => {
            BTreeSet::from([ArtifactKind::PitchTrack, ArtifactKind::PitchNoteCandidates])
        }
        _ => BTreeSet::new(),
    }
}

/// Whether `node_id`'s output currently exists on disk for `file_hash` --
/// the second half of "can this actually be frozen right now" alongside
/// `pipeline_can_honor_freeze`. A node with no output yet has nothing to
/// freeze: silently accepting the request would mean the next run either
/// crashes (pipeline.py finds nothing to reuse) or, worse, silently runs
/// the node anyway, which is exactly the "frozen but not really" failure
/// Freeze exists to prevent. Takes `&CacheDir` rather than resolving one
/// itself so tests can point it at a temp directory instead of the real
/// (and possibly absent) data directory.
fn node_output_exists_for_freeze(
    cache: &crate::cache::CacheDir,
    file_hash: &str,
    node_id: &crate::analysis_graph::AnalysisNodeId,
) -> bool {
    match node_id.as_str() {
        "stems.separate" => {
            cache.vocals_path(file_hash).is_file() && cache.instrumental_path(file_hash).is_file()
        }
        "pitch.extract" => {
            cache.pitch_track_path(file_hash).is_file()
                && cache.pitch_notes_path(file_hash).is_file()
        }
        _ => false,
    }
}

/// Phase 4's generic per-node execution entry point -- the previously
/// missing piece `AnalysisRequest.disabled_nodes` was modeled and
/// planner-tested for since Phase 1 but nothing at runtime ever populated or
/// consumed (docs/analysis-dag-redesign.md Phase 4 status note). Every
/// `disabled_nodes` entry is checked against `pipeline_can_honor_disable`
/// and the plan's own warnings first, so a caller never gets silent success
/// for a disable request the pipeline has no way to actually act on.
pub fn run_analysis_plan(
    file_hash: &str,
    targets: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    disabled_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }
    for id in &disabled_nodes {
        if !pipeline_can_honor_disable(id) {
            return Err(format!(
                "{id} cannot be disabled for a single run yet -- it is computed together with sibling nodes that always run"
            ));
        }
    }
    // Validates (and would reject an unhonorable disable) the same way
    // `process_song` will re-derive the real flags once this run actually
    // starts -- see that function's doc comment for why a downstream node
    // merely going `Blocked` as a consequence isn't itself a rejection.
    if let Err(warnings) = pipeline_flags_for_request(
        &targets,
        &disabled_nodes,
        &BTreeSet::new(),
        &BTreeSet::new(),
    ) {
        return Err(match warnings.first() {
            Some(warning) => format!("{}: {}", warning.node, warning.message),
            None => "invalid analysis request: unknown or not-applicable target node".to_string(),
        });
    }
    {
        let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
        let intent = intents.entry(file_hash.to_string()).or_default();
        intent.targets.extend(targets);
        intent.disabled_nodes.extend(disabled_nodes);
    }
    enqueue_one(file_hash);
    Ok(())
}

/// §7.5 "Run this node only": a single-node target run through the generic
/// executor, honoring the node's real upstream closure (Phase 1 planner)
/// rather than a special-cased flag.
pub fn run_analysis_node(file_hash: &str, node_id: &str) -> Result<(), String> {
    run_analysis_plan(
        file_hash,
        BTreeSet::from([crate::analysis_graph::AnalysisNodeId::new(node_id)]),
        BTreeSet::new(),
    )
}

/// Every node reachable by following outgoing edges from `id`, including
/// `id` itself -- the target set for §7.5 "Run this node and downstream".
fn downstream_closure(
    graph: &crate::analysis_graph::AnalysisGraphSpec,
    id: &crate::analysis_graph::AnalysisNodeId,
) -> BTreeSet<crate::analysis_graph::AnalysisNodeId> {
    let mut visited = BTreeSet::new();
    let mut stack = vec![id.clone()];
    while let Some(current) = stack.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        for edge in &graph.edges {
            if edge.from == current {
                stack.push(edge.to.clone());
            }
        }
    }
    visited
}

/// §7.5 "Run this node and downstream": targets `node_id` plus every node
/// that (transitively) consumes its output, through the same generic
/// per-node executor as `run_analysis_node`. No new pipeline-side mechanism
/// needed -- the Phase 1 planner already computes each target's own
/// required upstream closure, so targeting {node, ...its descendants} is
/// enough to make the planner require `node_id` itself (as an ancestor of
/// its own descendants) without also forcing unrelated ancestors like
/// `stems.separate` to do real work: `pipeline.py`'s existing cache-hit
/// check (`_cached_separator_matches`, the `music_analysis.json` version
/// check, ...) still short-circuits anything that hasn't actually changed,
/// the same way it already does for any other multi-target run today.
pub fn run_analysis_node_downstream(file_hash: &str, node_id: &str) -> Result<(), String> {
    let graph = crate::analysis_graph::baseline_graph_spec();
    let targets = downstream_closure(&graph, &crate::analysis_graph::AnalysisNodeId::new(node_id));
    run_analysis_plan(file_hash, targets, BTreeSet::new())
}

/// §7.5 "Disable for this run": the default full run
/// (`chart.build_candidate`, via `run_analysis_plan`'s empty-targets
/// default) with one node turned off.
pub fn disable_analysis_node_for_run(file_hash: &str, node_id: &str) -> Result<(), String> {
    run_analysis_plan(
        file_hash,
        BTreeSet::new(),
        BTreeSet::from([crate::analysis_graph::AnalysisNodeId::new(node_id)]),
    )
}

/// Desktop-facing predicate for whether "Disable for this run" would even
/// have a chance of succeeding for `node_id`, so the Node Context Menu can
/// omit or grey out the button instead of offering an action that
/// `run_analysis_plan` is guaranteed to reject.
pub fn node_can_be_disabled_for_run(node_id: &str) -> bool {
    pipeline_can_honor_disable(&crate::analysis_graph::AnalysisNodeId::new(node_id))
}

/// §7.5 "Freeze current outputs" -- Phase 4 §4.5's Freeze consumer. Unlike
/// `disable_analysis_node_for_run`, this never goes through
/// `run_analysis_plan`/`pipeline_flags_for_request`'s rejection path: a
/// freeze never disables anything, so it can never make the plan reject a
/// downstream node the way an unhonorable disable can. It has its own two
/// preconditions instead -- `pipeline_can_honor_freeze` (structural: is this
/// node's output even a freezable standalone artifact) and
/// `node_output_exists_for_freeze` (does it actually have an output yet) --
/// both re-checked here rather than trusted from the UI's own
/// `node_can_be_frozen_for_run` call, since a caller could race a "Freeze"
/// click against the very run that would produce the output being frozen.
pub fn freeze_analysis_node_outputs_for_run(file_hash: &str, node_id: &str) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }
    let id = crate::analysis_graph::AnalysisNodeId::new(node_id);
    if !pipeline_can_honor_freeze(&id) {
        return Err(format!(
            "{id} cannot be frozen for a single run -- it has no standalone cached output yet"
        ));
    }
    let Some(cache) = crate::cache::CacheDir::try_new() else {
        return Err("the analysis data directory is not available".to_string());
    };
    if !node_output_exists_for_freeze(&cache, file_hash, &id) {
        return Err(format!(
            "{id} has no output to freeze yet -- run it at least once first"
        ));
    }
    {
        let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
        let intent = intents.entry(file_hash.to_string()).or_default();
        intent
            .frozen_artifacts
            .extend(frozen_artifact_kinds_for_node(&id));
    }
    enqueue_one(file_hash);
    Ok(())
}

/// Desktop-facing predicate mirroring `node_can_be_disabled_for_run`: does
/// "Freeze current outputs" have a chance of succeeding for `node_id` on
/// this specific song right now (structurally freezable *and* it already
/// has output on disk), so the Node Context Menu can omit the button
/// instead of offering an action guaranteed to fail.
pub fn node_can_be_frozen_for_run(file_hash: &str, node_id: &str) -> bool {
    let id = crate::analysis_graph::AnalysisNodeId::new(node_id);
    let Some(cache) = crate::cache::CacheDir::try_new() else {
        return false;
    };
    pipeline_can_honor_freeze(&id) && node_output_exists_for_freeze(&cache, file_hash, &id)
}

/// Node ids `run_pipeline` can currently honor a Bypass request for.
/// `stems.separate` is the one real, concrete case the whole codebase
/// discusses (docs/analysis-dag-redesign.md §6: "routing stems.separate
/// around via Original Mix"): the source media itself is always a valid
/// substitute input for whatever `stems.separate` would have produced. No
/// other node has an alternate input concept today.
fn pipeline_can_honor_bypass(id: &crate::analysis_graph::AnalysisNodeId) -> bool {
    id.as_str() == "stems.separate"
}

/// §7.5 "Choose bypass" -- Phase 4 §4.5's Bypass consumer, the other half
/// of what this phase's Freeze work left undone. Unlike Freeze, there is no
/// on-disk existence precondition to check: the substitute input (the
/// song's own source media) is the thing that made this song loadable in
/// the first place, so a valid `file_hash` always has one. Like
/// `freeze_analysis_node_outputs_for_run`, this never goes through
/// `run_analysis_plan`'s rejection path -- a bypass never disables
/// anything, so it can't make the plan reject a downstream node.
pub fn bypass_analysis_node_with_original_mix_for_run(
    file_hash: &str,
    node_id: &str,
) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }
    let id = crate::analysis_graph::AnalysisNodeId::new(node_id);
    if !pipeline_can_honor_bypass(&id) {
        return Err(format!(
            "{id} cannot be bypassed for a single run -- it has no alternate input to route through yet"
        ));
    }
    {
        let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
        let intent = intents.entry(file_hash.to_string()).or_default();
        intent.bypassed_nodes.insert(id);
    }
    enqueue_one(file_hash);
    Ok(())
}

/// Desktop-facing predicate mirroring `node_can_be_frozen_for_run`: does
/// "Choose bypass" have a chance of succeeding for `node_id` -- purely
/// structural (no per-song existence check, see
/// `bypass_analysis_node_with_original_mix_for_run`'s doc comment), so the
/// Node Context Menu can omit the button instead of offering an action
/// guaranteed to fail.
pub fn node_can_be_bypassed_for_run(node_id: &str) -> bool {
    pipeline_can_honor_bypass(&crate::analysis_graph::AnalysisNodeId::new(node_id))
}

/// The one profile-controlled parameter (if any) a node's output actually
/// depends on -- mirrors `desktop/src/studio/analysis.rs::selected_stage_parameter`'s
/// mapping exactly, so the Node Context Menu's "Configure for this
/// run"/"Save as song profile" buttons and the inspector's PARAMETER fact
/// row can never disagree about which nodes have a real, controllable knob.
fn node_config_field_for(node_id: &str) -> Option<crate::analysis_profile::ProfileField> {
    use crate::analysis_profile::ProfileField;
    match node_id {
        "stems.separate" => Some(ProfileField::Separator),
        "lyrics.transcribe" => Some(ProfileField::AsrEngine),
        "lyrics.align" => Some(ProfileField::AlignmentBackend),
        _ => None,
    }
}

/// Desktop-facing predicate for whether "Configure for this run"/"Save as
/// song profile" have a chance of applying to `node_id` at all, so the Node
/// Context Menu can omit both buttons for a node with no real
/// profile-controlled parameter instead of offering actions guaranteed to
/// have no effect.
pub fn node_can_be_configured_for_run(node_id: &str) -> bool {
    node_config_field_for(node_id).is_some()
}

/// Read-only peek at a pending Run override for the inspector's PARAMETER
/// SOURCE display -- deliberately does not drain `PENDING_NODE_INTENTS` the
/// way `process_song` does, since this is called on every render and must
/// not consume a real queued run's override out from under it.
pub fn pending_run_override_for(file_hash: &str, node_id: &str) -> Option<String> {
    let field = node_config_field_for(node_id)?;
    let intents = PENDING_NODE_INTENTS.lock().unwrap();
    intents.get(file_hash).and_then(|intent| {
        intent
            .run_override
            .as_ref()
            .filter(|(f, _)| *f == field)
            .map(|(_, value)| value.clone())
    })
}

/// §7.5 "Configure for this run" -- the Run tier (phase plan §8.4's
/// previously-unimplemented third tier) of the Global Defaults -> Song
/// Profile -> Run Override chain. Stores `value` alongside the node's
/// mapped `ProfileField` in `PENDING_NODE_INTENTS`, one-shot (drained by
/// `process_song` the same way `targets`/`disabled_nodes` already are), and
/// targets `node_id` the same way `run_analysis_node` does so the override
/// actually reaches a run that executes this node.
pub fn configure_analysis_node_for_run(
    file_hash: &str,
    node_id: &str,
    value: String,
) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }
    let Some(field) = node_config_field_for(node_id) else {
        return Err(format!(
            "{node_id} has no profile-controlled parameter to configure for a single run"
        ));
    };
    {
        let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
        let intent = intents.entry(file_hash.to_string()).or_default();
        intent
            .targets
            .insert(crate::analysis_graph::AnalysisNodeId::new(node_id));
        intent.run_override = Some((field, value));
    }
    enqueue_one(file_hash);
    Ok(())
}

/// §7.5 "Save as song profile" -- persists the node's *current effective*
/// value (whichever tier is winning right now, via `resolve_profile_field`)
/// as this song's saved profile override, without changing anything about
/// the run in progress. Starts from the song's existing saved profile (or
/// the real global defaults, via `AnalysisProfileSnapshot::from_app_config`,
/// if none exists yet) so saving one node's field never clobbers another
/// field a previous "Save as song profile" call already persisted.
pub fn save_node_config_as_song_profile(file_hash: &str, node_id: &str) -> Result<(), String> {
    use crate::analysis_profile::{
        AnalysisProfileSnapshot, ProfileField, get_song_analysis_profile, resolve_profile_field,
        set_song_analysis_profile,
    };
    let Some(field) = node_config_field_for(node_id) else {
        return Err(format!(
            "{node_id} has no profile-controlled parameter to save as a song profile"
        ));
    };
    let config = AppConfig::load();
    let global = AnalysisProfileSnapshot::from_app_config(&config, file_hash);
    let song = get_song_analysis_profile(file_hash);
    let run_override = pending_run_override_for(file_hash, node_id);
    let effective_value =
        resolve_profile_field(field, &global, song.as_ref(), run_override.as_deref()).value;

    let mut updated = song.unwrap_or(global);
    match field {
        ProfileField::Separator => updated.separator = effective_value,
        ProfileField::AsrEngine => updated.asr_engine = effective_value,
        ProfileField::AlignmentBackend => updated.alignment_backend = effective_value,
    }
    set_song_analysis_profile(file_hash, &updated)
}

/// Shared profile/model-availability resolution behind every
/// `preview_*_analysis_plan` variant -- one real resolution path, not
/// several copies that can drift (this codebase has hit that exact class of
/// bug before: the canvas/inspector percentage mismatch, the PARAMETER
/// SOURCE binary check). Only `disabled_nodes` varies by caller; target is
/// always the default full run (`chart.build_candidate`) and route is
/// always `WhisperAsr`, matching every other existing call site's
/// placeholder (`build_execution_plan`) -- no code path anywhere lets a
/// user pick a route today.
fn preview_analysis_request_for(
    file_hash: &str,
    disabled_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
) -> crate::analysis_plan::AnalysisRequest {
    use crate::analysis_graph::AnalysisNodeId;
    use crate::analysis_plan::{AnalysisRequest, LyricsRoute};

    let profile_snapshot = crate::analysis_profile::get_song_analysis_profile(file_hash)
        .unwrap_or_else(|| {
            crate::analysis_profile::AnalysisProfileSnapshot::from_app_config(
                &AppConfig::load(),
                file_hash,
            )
        });
    let availability_params =
        crate::vendor::model_availability_params_for_profile(&profile_snapshot);
    let model_availability = crate::vendor::node_model_availability_for(&availability_params);
    AnalysisRequest {
        file_hash: file_hash.to_string(),
        targets: BTreeSet::from([AnalysisNodeId::new("chart.build_candidate")]),
        disabled_nodes,
        frozen_artifacts: BTreeSet::new(),
        bypassed_nodes: BTreeSet::new(),
        lyrics_route: LyricsRoute::WhisperAsr,
        model_availability,
        profile_snapshot,
    }
}

/// Real Phase 1 plan preview for a song's default full run
/// (`chart.build_candidate`), grounded in the song's saved analysis
/// profile (falling back to the real global defaults when unset -- see
/// `preview_analysis_request_for`). Phase 7's Plan Preview panel and node
/// inspector both read from this rather than reconstructing an
/// `AnalysisRequest` themselves.
pub fn preview_full_analysis_plan(
    file_hash: &str,
) -> Result<crate::analysis_plan::AnalysisPlan, crate::analysis_plan::PlanError> {
    let request = preview_analysis_request_for(file_hash, BTreeSet::new());
    crate::analysis_plan::preview_analysis_plan(file_hash, request)
}

/// Phase 8's standalone Plan Preview panel: previews a *hypothetical*
/// disabled-node combination against the default full run, without
/// enqueueing anything -- lets a caller see the resulting Will-run/
/// Will-reuse/Blocked breakdown before committing to it via
/// `run_analysis_plan`. Target stays the default full run and route stays
/// fixed (see `preview_analysis_request_for`'s doc comment) -- only
/// `disabled_nodes` is a real staged variable today.
pub fn preview_analysis_plan_for_selection(
    file_hash: &str,
    disabled_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
) -> Result<crate::analysis_plan::AnalysisPlan, crate::analysis_plan::PlanError> {
    let request = preview_analysis_request_for(file_hash, disabled_nodes);
    crate::analysis_plan::preview_analysis_plan(file_hash, request)
}

/// Mark a hash so its next analysis pass separates stems (and, as an
/// unavoidable side effect of today's pipeline.py, regenerates pitch too --
/// see docs/analysis-dag-redesign.md Phase 4 status note) without
/// transcribing, preserving the transcript built from provided LRC.
pub fn mark_stems_only(file_hash: &str) {
    PENDING_NODE_INTENTS
        .lock()
        .unwrap()
        .entry(file_hash.to_string())
        .or_default()
        .targets
        .insert(crate::analysis_graph::AnalysisNodeId::new("pitch.extract"));
}

// ─── Helpers ─────────────────────────────────────────────────────────

fn update_queue_status(file_hash: &str, status: QueuedStatus) {
    let (st, pct, msg) = match &status {
        QueuedStatus::Queued => ("queued", None, None::<String>),
        QueuedStatus::Analyzing(p) => ("analyzing", Some(*p as i64), None::<String>),
        QueuedStatus::Failed(s) => ("failed", None, Some(s.clone())),
    };
    let _ = library_db::analysis_queue_upsert_row(file_hash, st, pct, msg.as_deref());
    if let QueuedStatus::Failed(message) = &status {
        finish_analysis_history(file_hash, "failed", Some(message));
    }
}

fn remove_from_queue(file_hash: &str) {
    let _ = library_db::analysis_queue_delete(file_hash);
}

pub(crate) fn update_song_analyzed(
    file_hash: &str,
    is_analyzed: bool,
    language: Option<String>,
    transcript_source: Option<TranscriptSource>,
    key: Option<String>,
    bpm: Option<f64>,
    tempo: Option<f64>,
) {
    let Some(mut song) = library_db::load_song_by_hash(file_hash).ok().flatten() else {
        return;
    };
    song.is_analyzed = is_analyzed;
    song.language = language;
    song.transcript_source = transcript_source;
    if is_analyzed {
        song.key = key;
        song.bpm = bpm;
        if let Some(value) = tempo {
            song.tempo = value;
        }
        // LRC-provided songs without stem separation are flagged in the
        // transcript; mirror that onto the song so authoring uses the original mix.
        song.no_stems = read_transcript_meta(&CacheDir::new(), file_hash).no_stems;
    } else {
        song.key = None;
        song.override_key = None;
        song.bpm = None;
        song.tempo = 1.0;
        song.key_offset = 0;
        song.no_stems = false;
    }
    let _ = library_db::update_song_fields(file_hash, &song);
}

fn ensure_worker_running(state: &mut AnalyzerState) {
    if !state.worker_running && !state.queue.is_empty() {
        state.worker_running = true;
        spawn_worker();
    }
}

// ─── Public API ──────────────────────────────────────────────────────

pub(crate) fn is_usdx_song(file_hash: &str) -> bool {
    library_db::load_song_by_hash(file_hash)
        .ok()
        .flatten()
        .map(|s| s.usdx.is_some())
        .unwrap_or(false)
}

pub fn enqueue_one(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }
    let mut state = ANALYZER.lock().unwrap();
    if state.active_hash.as_deref() == Some(file_hash) {
        return;
    }
    if !state.queue.iter().any(|h| h == file_hash) {
        state.queue.push_back(file_hash.to_string());
        update_queue_status(file_hash, QueuedStatus::Queued);
        FROZEN_CONFIGS
            .lock()
            .unwrap()
            .insert(file_hash.to_string(), AppConfig::load());
    }
    ensure_worker_running(&mut state);
}

fn queue_entry_blocks_enqueue(status: Option<&QueuedStatus>) -> bool {
    matches!(
        status,
        Some(QueuedStatus::Queued | QueuedStatus::Analyzing(_))
    )
}

/// Phase 6 `cancel_analysis_run`. Deliberately scoped to the *queued, not
/// yet started* case only: the single background worker thread
/// (`spawn_worker`) runs `process_song` synchronously against a live Python
/// subprocess with no interrupt hook in the wire protocol, so a genuinely
/// running analysis cannot be safely cancelled mid-node today (killing the
/// analyzer server outright would corrupt whatever node was mid-write and
/// take down every *other* queued song's server connection with it). This
/// is why "取消一个正在执行的节点需要节点函数是可中断的独立单元" in
/// docs/plan.md's Phase 4 §4.2 note remains a real, separate blocker for
/// the running case -- rejecting cleanly (not pretending to cancel, not
/// panicking) is safer than either.
pub fn cancel_analysis_run(file_hash: &str) -> Result<(), String> {
    let mut state = ANALYZER.lock().unwrap();
    if state.active_hash.as_deref() == Some(file_hash) {
        return Err(
            "this song is already being analyzed and cannot be cancelled mid-run yet".to_string(),
        );
    }
    let before = state.queue.len();
    state.queue.retain(|hash| hash != file_hash);
    if state.queue.len() == before {
        return Err(format!("{file_hash} is not currently queued"));
    }
    drop(state);
    remove_from_queue(file_hash);
    // The run never actually happened, so any per-run intent stashed for
    // it (targets/disabled/frozen/bypassed nodes, the enqueue-time config
    // snapshot) is moot -- left behind, it would silently apply to some
    // future unrelated enqueue of the same song.
    PENDING_NODE_INTENTS.lock().unwrap().remove(file_hash);
    FROZEN_CONFIGS.lock().unwrap().remove(file_hash);
    Ok(())
}

pub fn enqueue_all(filters: &LibraryMenuFilters) {
    let queue = AnalysisQueue::load();
    let mut state = ANALYZER.lock().unwrap();
    // One snapshot for the whole batch (all newly-queued jobs join the
    // queue in this same synchronous call, so there's no window for global
    // settings to change between them) rather than re-reading the config
    // file from disk once per song.
    let batch_config = AppConfig::load();

    let pending_hashes =
        library_db::iter_file_hashes_filtered_not_analyzed(filters).unwrap_or_default();

    let mut newly_queued = Vec::new();
    for file_hash in pending_hashes {
        // A failed row is history, not active work. "Analyze all" must be able
        // to retry it without asking the user to clear the activity log.
        let blocked_by_active_entry = queue_entry_blocks_enqueue(queue.entries.get(&file_hash));
        if !blocked_by_active_entry
            && state.active_hash.as_deref() != Some(&file_hash)
            && !state.queue.iter().any(|h| h == &file_hash)
        {
            state.queue.push_back(file_hash.clone());
            FROZEN_CONFIGS
                .lock()
                .unwrap()
                .insert(file_hash.clone(), batch_config.clone());
            newly_queued.push(file_hash);
        }
    }

    let should_start = !state.worker_running && !state.queue.is_empty();
    if should_start {
        state.worker_running = true;
    }
    drop(state);

    for hash in &newly_queued {
        let _ = library_db::analysis_queue_upsert_row(hash, "queued", None, None);
    }

    if should_start {
        spawn_worker();
    }
}

#[cfg(test)]
mod song_authoring_state_tests {
    use super::{QueuedStatus, SongAuthoringState, authoring_state_from_signals};

    #[test]
    fn failed_queue_entry_wins_over_everything_else() {
        let failed = QueuedStatus::Failed("boom".to_string());
        assert_eq!(
            authoring_state_from_signals(Some(&failed), true, true, true),
            SongAuthoringState::RetryFailedNode
        );
    }

    #[test]
    fn queued_or_analyzing_reports_in_progress() {
        assert_eq!(
            authoring_state_from_signals(Some(&QueuedStatus::Queued), true, true, true),
            SongAuthoringState::InProgress
        );
        assert_eq!(
            authoring_state_from_signals(Some(&QueuedStatus::Analyzing(42)), false, false, false),
            SongAuthoringState::InProgress
        );
    }

    #[test]
    fn never_analyzed_prompts_analyze_song() {
        assert_eq!(
            authoring_state_from_signals(None, false, false, false),
            SongAuthoringState::AnalyzeSong
        );
    }

    #[test]
    fn analyzed_without_a_chart_yet_prompts_open_editor() {
        assert_eq!(
            authoring_state_from_signals(None, true, false, false),
            SongAuthoringState::OpenEditor
        );
    }

    #[test]
    fn chart_present_but_editor_blocked_prompts_fix_chart_issues() {
        assert_eq!(
            authoring_state_from_signals(None, true, true, false),
            SongAuthoringState::FixChartIssues
        );
    }

    #[test]
    fn chart_present_and_editor_ready_prompts_edit_chart() {
        assert_eq!(
            authoring_state_from_signals(None, true, true, true),
            SongAuthoringState::EditChart
        );
    }
}

#[cfg(test)]
mod chart_protection_tests {
    use super::{apply_pitch_reanalysis_reset, apply_realign_reset, apply_reanalyze_reset};
    use crate::cache::CacheDir;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_cache(label: &str) -> CacheDir {
        let nonce = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "uta-studio-chart-protection-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp cache dir");
        CacheDir { path }
    }

    fn touch(path: &std::path::Path) {
        std::fs::write(path, b"{}").expect("write fixture file");
    }

    #[test]
    fn pitch_reanalysis_reset_preserves_the_authored_chart() {
        let cache = temp_cache("pitch");
        let hash = "songPitch";
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.pitch_track_path(hash));
        touch(&cache.pitch_notes_path(hash));

        apply_pitch_reanalysis_reset(&cache, hash);

        assert!(
            cache.vocal_chart_path(hash).is_file(),
            "authored chart must survive a pitch-only rerun"
        );
        assert!(!cache.pitch_track_path(hash).is_file());
        assert!(!cache.pitch_notes_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn realign_reset_preserves_the_authored_chart() {
        let cache = temp_cache("realign");
        let hash = "songRealign";
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.transcript_path(hash));
        touch(&cache.variant_transcript_path(hash, 1.2));

        apply_realign_reset(&cache, hash);

        assert!(
            cache.vocal_chart_path(hash).is_file(),
            "authored chart must survive realign"
        );
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.variant_transcript_path(hash, 1.2).is_file());
        cache.clear_all();
    }

    #[test]
    fn transcript_only_reanalyze_reset_preserves_the_authored_chart() {
        let cache = temp_cache("reanalyze-transcript");
        let hash = "songReanalyzeTranscript";
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.transcript_path(hash));
        touch(&cache.lyrics_path(hash));

        apply_reanalyze_reset(&cache, hash, false);

        assert!(cache.vocal_chart_path(hash).is_file());
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.lyrics_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn full_reanalyze_reset_preserves_the_authored_chart_but_clears_everything_else() {
        // The highest-stakes case: "Reanalyze all" regenerates every
        // analysis artifact, yet must still default to keeping the chart
        // (phase plan Phase 9 test: "Full Reanalysis 默认保留 Authored Chart").
        let cache = temp_cache("reanalyze-full");
        let hash = "songReanalyzeFull";
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.transcript_path(hash));
        touch(&cache.pitch_track_path(hash));
        touch(&cache.pitch_notes_path(hash));
        touch(&cache.music_analysis_path(hash));
        touch(&cache.vocals_path(hash));
        touch(&cache.instrumental_path(hash));

        apply_reanalyze_reset(&cache, hash, true);

        assert!(
            cache.vocal_chart_path(hash).is_file(),
            "authored chart must survive a full reanalysis reset"
        );
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.pitch_track_path(hash).is_file());
        assert!(!cache.pitch_notes_path(hash).is_file());
        assert!(!cache.music_analysis_path(hash).is_file());
        assert!(!cache.vocals_path(hash).is_file());
        assert!(!cache.instrumental_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn explicit_delete_song_cache_still_removes_the_chart() {
        // The one place total deletion remains correct: the explicit,
        // user-confirmed "Delete cache" action (delete_cache ->
        // cache.delete_song_cache), unaffected by this phase's change.
        let cache = temp_cache("delete-cache");
        let hash = "songDeleteCache";
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.transcript_path(hash));

        cache.delete_song_cache(hash);

        assert!(!cache.vocal_chart_path(hash).is_file());
        assert!(!cache.transcript_path(hash).is_file());
        cache.clear_all();
    }
}

#[cfg(test)]
mod pipeline_flags_tests {
    use super::pipeline_flags_for_request;
    use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
    use std::collections::BTreeSet;

    fn targets(ids: &[&str]) -> BTreeSet<AnalysisNodeId> {
        ids.iter().map(|s| AnalysisNodeId::new(*s)).collect()
    }

    fn no_freeze() -> BTreeSet<ArtifactKind> {
        BTreeSet::new()
    }

    fn no_bypass() -> BTreeSet<AnalysisNodeId> {
        BTreeSet::new()
    }

    #[test]
    fn no_targets_means_run_everything() {
        let (
            skip_transcription,
            skip_separation,
            skip_pitch,
            freeze_separation,
            freeze_pitch,
            bypass_separation,
        ) = pipeline_flags_for_request(
            &BTreeSet::new(),
            &BTreeSet::new(),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(!skip_transcription);
        assert!(!skip_separation);
        assert!(!skip_pitch);
        assert!(!freeze_separation);
        assert!(!freeze_pitch);
        assert!(!bypass_separation);
    }

    #[test]
    fn pitch_only_target_skips_transcription_but_not_separation() {
        // Replaces the old PITCH_ONLY special case: pitch.extract requires
        // stems.separate transitively, so separation must still run, but no
        // lyrics node is targeted so transcription/alignment must not.
        let (skip_transcription, skip_separation, skip_pitch, ..) = pipeline_flags_for_request(
            &targets(&["pitch.extract"]),
            &BTreeSet::new(),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(skip_transcription);
        assert!(!skip_separation);
        assert!(!skip_pitch);
    }

    #[test]
    fn lyrics_target_never_skips_transcription() {
        let (skip_transcription, ..) = pipeline_flags_for_request(
            &targets(&["lyrics.align"]),
            &BTreeSet::new(),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(!skip_transcription);
    }

    #[test]
    fn full_candidate_chart_target_skips_neither() {
        let (skip_transcription, skip_separation, skip_pitch, ..) = pipeline_flags_for_request(
            &targets(&["chart.build_candidate"]),
            &BTreeSet::new(),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(!skip_transcription);
        assert!(!skip_separation);
        assert!(!skip_pitch);
    }

    #[test]
    fn disabling_pitch_extract_under_the_default_full_target_blocks_the_chart_but_is_not_rejected()
    {
        // pitch.extract feeds chart.build_candidate's PitchNoteCandidates
        // input directly, so disabling it under the default full-run target
        // makes the plan mark chart.build_candidate Blocked -- that's the
        // disable working as designed (docs/analysis-dag-redesign.md §6),
        // not a request the caller's own disable was refused for, so this
        // must still succeed and skip pitch.
        let (skip_transcription, skip_separation, skip_pitch, ..) = pipeline_flags_for_request(
            &BTreeSet::new(),
            &targets(&["pitch.extract"]),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(!skip_transcription);
        assert!(!skip_separation);
        assert!(skip_pitch);
    }

    #[test]
    fn disabling_pitch_extract_while_targeting_only_stems_has_no_downstream_to_block() {
        let (_skip_transcription, skip_separation, skip_pitch, ..) = pipeline_flags_for_request(
            &targets(&["stems.separate"]),
            &targets(&["pitch.extract"]),
            &no_freeze(),
            &no_bypass(),
        )
        .unwrap();
        assert!(!skip_separation);
        assert!(skip_pitch);
    }

    #[test]
    fn disabling_an_always_required_node_is_rejected_with_a_warning() {
        let result = pipeline_flags_for_request(
            &BTreeSet::new(),
            &targets(&["chart.build_candidate"]),
            &no_freeze(),
            &no_bypass(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn freezing_stems_does_not_skip_separation_but_sets_the_freeze_flag() {
        // A Frozen stems.separate must still be "run" (so pipeline.py calls
        // run_stem_separation and gets a vocals path to hand downstream) --
        // it must NOT collapse to skip_separation the way a Blocked/Disabled
        // stems.separate would, or pitch.extract/transcription would get a
        // `None` vocals path and crash instead of reusing the frozen file.
        let mut frozen = BTreeSet::new();
        frozen.insert(ArtifactKind::VocalStem);
        let (
            _skip_transcription,
            skip_separation,
            _skip_pitch,
            freeze_separation,
            freeze_pitch,
            bypass_separation,
        ) = pipeline_flags_for_request(
            &targets(&["pitch.extract"]),
            &BTreeSet::new(),
            &frozen,
            &no_bypass(),
        )
        .unwrap();
        assert!(!skip_separation, "a frozen node must not also be skipped");
        assert!(freeze_separation);
        assert!(!freeze_pitch);
        assert!(!bypass_separation);
    }

    #[test]
    fn freezing_pitch_sets_only_the_pitch_freeze_flag() {
        let mut frozen = BTreeSet::new();
        frozen.insert(ArtifactKind::PitchTrack);
        frozen.insert(ArtifactKind::PitchNoteCandidates);
        let (
            _skip_transcription,
            skip_separation,
            skip_pitch,
            freeze_separation,
            freeze_pitch,
            _bypass_separation,
        ) = pipeline_flags_for_request(
            &targets(&["chart.build_candidate"]),
            &BTreeSet::new(),
            &frozen,
            &no_bypass(),
        )
        .unwrap();
        assert!(!skip_separation);
        assert!(!freeze_separation);
        assert!(!skip_pitch, "a frozen node must not also be skipped");
        assert!(freeze_pitch);
    }

    #[test]
    fn bypassing_stems_skips_separation_and_sets_the_bypass_flag() {
        // Unlike Freeze, a Bypassed stems.separate genuinely does not run --
        // pipeline.py substitutes the Original Mix as the vocals path
        // itself, so skip_separation stays true (no real separation call),
        // with bypass_separation telling it to use the substitute rather
        // than leaving the vocals path unset.
        let mut bypassed = BTreeSet::new();
        bypassed.insert(AnalysisNodeId::new("stems.separate"));
        let (
            _skip_transcription,
            skip_separation,
            _skip_pitch,
            freeze_separation,
            _freeze_pitch,
            bypass_separation,
        ) = pipeline_flags_for_request(
            &targets(&["pitch.extract"]),
            &BTreeSet::new(),
            &no_freeze(),
            &bypassed,
        )
        .unwrap();
        assert!(skip_separation);
        assert!(!freeze_separation);
        assert!(bypass_separation);
    }
}

#[cfg(test)]
mod preview_full_analysis_plan_tests {
    use super::preview_full_analysis_plan;
    use crate::analysis_graph::AnalysisNodeId;
    use crate::analysis_profile::{AnalysisProfileSnapshot, set_song_analysis_profile};
    use std::collections::BTreeSet;

    #[test]
    fn targets_the_full_chart_build_and_lists_every_node() {
        let plan = preview_full_analysis_plan("preview-plan-test-song-a")
            .expect("baseline graph always plans");
        assert!(
            plan.target_nodes
                .contains(&AnalysisNodeId::new("chart.build_candidate"))
        );
        assert!(!plan.nodes.is_empty());
        assert!(plan.node(&AnalysisNodeId::new("music.analysis")).is_some());
    }

    #[test]
    fn falls_back_to_the_real_global_config_when_no_song_profile_is_saved() {
        // Phase 8: this used to fall back to `AnalysisProfileSnapshot::default()`'s
        // hardcoded stand-ins, which could silently disagree with the user's
        // actual global settings. Compares against the same
        // `from_app_config` resolution `process_song` now uses for real
        // execution, rather than a hardcoded literal, so this test doesn't
        // depend on what's in the real config file on the machine running
        // it (a real value each time, just not a fixed one).
        let hash = "preview-plan-test-song-b";
        let plan = preview_full_analysis_plan(hash).expect("baseline graph always plans");
        let expected =
            AnalysisProfileSnapshot::from_app_config(&crate::config::AppConfig::load(), hash);
        assert_eq!(plan.profile_snapshot, expected);
    }

    /// See `library_db::reconnect_for_test` -- shared crate-wide so
    /// isolation holds across every module's DB-touching tests, not just
    /// within this one.
    fn isolated_test_db(label: &str) -> std::sync::MutexGuard<'static, ()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "uta-studio-analyzer-plan-preview-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        crate::library_db::reconnect_for_test(&dir)
    }

    #[test]
    fn a_saved_song_profile_flows_into_the_previewed_plan() {
        let _guard = isolated_test_db("flows-in");
        let hash = "preview-plan-test-song-c";
        let saved = AnalysisProfileSnapshot {
            separator: "demucs".to_string(),
            alignment_backend: "mms_karaoke".to_string(),
            asr_engine: "parakeet".to_string(),
            requested_device: "cuda".to_string(),
            language_override: Some("ja".to_string()),
        };
        set_song_analysis_profile(hash, &saved).expect("save profile");

        let plan = preview_full_analysis_plan(hash).expect("baseline graph always plans");

        assert_eq!(plan.profile_snapshot, saved);
    }

    #[test]
    fn selection_preview_with_nothing_disabled_matches_the_full_preview() {
        // Same shared `preview_analysis_request_for` resolution path -- an
        // empty disabled set should produce an identical plan to
        // `preview_full_analysis_plan`, not a second, potentially-drifted
        // copy of the same logic.
        let hash = "preview-plan-test-song-selection-empty";
        let full = preview_full_analysis_plan(hash).expect("baseline graph always plans");
        let selection = super::preview_analysis_plan_for_selection(hash, BTreeSet::new())
            .expect("baseline graph always plans");
        assert_eq!(full.nodes, selection.nodes);
        assert_eq!(full.profile_snapshot, selection.profile_snapshot);
    }

    // `disabling_pitch_extract_blocks_chart_build_candidate` deliberately
    // calls `analysis_plan::build_plan` directly with an explicit
    // `AnalysisRequest` (empty `model_availability`, which the planner
    // defaults to "available" for every node -- see that field's own doc
    // comment) rather than going through
    // `preview_analysis_plan_for_selection`. That function does a *real*
    // vendor/disk model-availability lookup (Phase 8 §8.6, by design) --
    // in the `nix build` sandbox, no real models exist on disk, so
    // `pitch.extract`'s own parent (`stems.separate`) already comes back
    // `Blocked` for a missing model before the disable check even runs,
    // and `build_plan`'s "blocking parent" propagation
    // (`analysis_plan.rs`, checked *before* the explicit-disable branch)
    // marks `pitch.extract` `Blocked` too -- not because disabling it
    // didn't work, but because its environment-dependent parent state hid
    // it. This test is about disable/blocked precedence, not about which
    // models happen to be installed on the machine running it, so it
    // constructs a deterministic request instead of depending on real
    // disk state.
    #[test]
    fn disabling_pitch_extract_blocks_chart_build_candidate() {
        use crate::analysis_graph::baseline_graph_spec;
        use crate::analysis_plan::{AnalysisRequest, LyricsRoute, NodeState, build_plan};

        let request = AnalysisRequest {
            file_hash: "preview-plan-test-song-selection-disable-pitch".to_string(),
            targets: BTreeSet::from([AnalysisNodeId::new("chart.build_candidate")]),
            disabled_nodes: BTreeSet::from([AnalysisNodeId::new("pitch.extract")]),
            frozen_artifacts: BTreeSet::new(),
            bypassed_nodes: BTreeSet::new(),
            lyrics_route: LyricsRoute::WhisperAsr,
            model_availability: std::collections::BTreeMap::new(),
            profile_snapshot: AnalysisProfileSnapshot::default(),
        };
        let plan =
            build_plan(&baseline_graph_spec(), &request).expect("baseline graph always plans");

        assert_eq!(
            plan.node(&AnalysisNodeId::new("pitch.extract"))
                .unwrap()
                .state,
            NodeState::Disabled
        );
        assert_eq!(
            plan.node(&AnalysisNodeId::new("chart.build_candidate"))
                .unwrap()
                .state,
            NodeState::Blocked
        );
    }
}

#[cfg(test)]
mod frozen_config_tests {
    //! Phase 4 §4.1 "Enqueue 时冻结配置": a queued job must run with the
    //! config snapshot captured when it joined the queue, not whatever the
    //! user has changed global settings to by the time a worker thread
    //! actually picks it up.
    use super::{AppConfig, FROZEN_CONFIGS, resolve_frozen_config};
    use std::sync::Mutex;

    /// `FROZEN_CONFIGS` is a process-wide singleton; serialize tests that
    /// touch it, same reasoning as `pending_intent_tests`'s guard.
    static GUARD: Mutex<()> = Mutex::new(());

    fn config_with_model(model: &str) -> AppConfig {
        let mut config = AppConfig::default();
        config.whisper_model = Some(model.to_string());
        config
    }

    #[test]
    fn resolve_frozen_config_returns_and_drains_the_frozen_snapshot() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "frozen-config-test-song";
        FROZEN_CONFIGS
            .lock()
            .unwrap()
            .insert(hash.to_string(), config_with_model("frozen-model"));

        let resolved = resolve_frozen_config(hash, hash, || config_with_model("fallback-model"));
        assert_eq!(resolved.whisper_model.as_deref(), Some("frozen-model"));

        // Drained -- a second resolve for the same hash must not see the
        // same snapshot reused; it should fall back.
        let resolved_again =
            resolve_frozen_config(hash, hash, || config_with_model("fallback-model"));
        assert_eq!(
            resolved_again.whisper_model.as_deref(),
            Some("fallback-model")
        );
    }

    #[test]
    fn resolve_frozen_config_falls_back_when_nothing_was_frozen() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "frozen-config-test-song-missing";
        FROZEN_CONFIGS.lock().unwrap().remove(hash);

        let resolved = resolve_frozen_config(hash, hash, || config_with_model("fallback-model"));
        assert_eq!(resolved.whisper_model.as_deref(), Some("fallback-model"));
    }

    #[test]
    fn resolve_frozen_config_finds_a_snapshot_stored_under_the_pre_rekey_hash() {
        // A remote song's hash can change between enqueue (frozen under
        // the pre-rekey hash) and process_song reaching this point (now
        // using the real, rekeyed hash).
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let initial_hash = "frozen-config-test-song-initial";
        let real_hash = "frozen-config-test-song-real";
        FROZEN_CONFIGS.lock().unwrap().remove(real_hash);
        FROZEN_CONFIGS
            .lock()
            .unwrap()
            .insert(initial_hash.to_string(), config_with_model("frozen-model"));

        let resolved = resolve_frozen_config(real_hash, initial_hash, || {
            config_with_model("fallback-model")
        });
        assert_eq!(resolved.whisper_model.as_deref(), Some("frozen-model"));
    }

    #[test]
    fn resolve_frozen_config_prefers_the_current_hash_over_the_initial_one() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let initial_hash = "frozen-config-test-song-initial-2";
        let real_hash = "frozen-config-test-song-real-2";
        FROZEN_CONFIGS
            .lock()
            .unwrap()
            .insert(initial_hash.to_string(), config_with_model("initial-model"));
        FROZEN_CONFIGS
            .lock()
            .unwrap()
            .insert(real_hash.to_string(), config_with_model("real-model"));

        let resolved = resolve_frozen_config(real_hash, initial_hash, || {
            config_with_model("fallback-model")
        });
        assert_eq!(resolved.whisper_model.as_deref(), Some("real-model"));

        // The initial-hash entry was never touched by this resolve call
        // (current-hash entry took priority), so drain it manually to
        // avoid leaking state into other tests.
        FROZEN_CONFIGS.lock().unwrap().remove(initial_hash);
    }
}

#[cfg(test)]
mod pending_intent_tests {
    use super::{PENDING_NODE_INTENTS, mark_stems_only, pipeline_flags_for_request};
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    /// `PENDING_NODE_INTENTS` is a process-wide singleton; serialize tests
    /// that touch it so they can't interleave and observe each other's
    /// stashed intents.
    static GUARD: Mutex<()> = Mutex::new(());

    #[test]
    fn mark_stems_only_stashes_a_pitch_extract_target() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "pending-intent-test-song";
        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);

        mark_stems_only(hash);

        let intents = PENDING_NODE_INTENTS.lock().unwrap();
        let intent = intents.get(hash).expect("intent must be stashed");
        assert!(
            intent
                .targets
                .contains(&crate::analysis_graph::AnalysisNodeId::new("pitch.extract"))
        );
        assert!(!intent.force_transcribe);
        drop(intents);
        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);
    }

    #[test]
    fn stashed_pitch_extract_target_resolves_to_skip_transcription_only() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "pending-intent-resolve-test-song";
        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);
        mark_stems_only(hash);

        let targets = PENDING_NODE_INTENTS
            .lock()
            .unwrap()
            .remove(hash)
            .unwrap()
            .targets;
        let (skip_transcription, skip_separation, ..) = pipeline_flags_for_request(
            &targets,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .unwrap();
        assert!(skip_transcription);
        assert!(!skip_separation);
    }
}

#[cfg(test)]
mod compare_analysis_runs_tests {
    //! Phase 6 `compare_analysis_runs` / Phase 7 §7.5 "Compare with
    //! previous attempt". `compare_analysis_runs_from` is a pure function
    //! over already-loaded rows, so these build fixtures directly instead
    //! of needing a real DB.
    use super::{
        AnalysisRunHistory, NodeAttempt, compare_analysis_runs_from,
        compare_node_attempt_with_previous_run,
    };

    fn run(id: i64, file_hash: &str, finished_at_ms: i64) -> AnalysisRunHistory {
        AnalysisRunHistory {
            id,
            file_hash: file_hash.to_string(),
            title: "Test Song".to_string(),
            artist: "Test Artist".to_string(),
            status: "completed".to_string(),
            started_at_ms: finished_at_ms - 1000,
            finished_at_ms,
            error_message: None,
            snapshot: super::AnalysisProgressSnapshot {
                stage: "complete".to_string(),
                stage_progress: 100,
                operation: String::new(),
                detail: String::new(),
                implementation: String::new(),
                model: String::new(),
                device: String::new(),
                requested_device: String::new(),
                fallback_from: None,
                fallback_reason: None,
                backend_fallback_from: None,
                backend_fallback_reason: None,
                stage_routes: Vec::new(),
                node_id: None,
                node_event: None,
                artifact_reused_reason: None,
            },
        }
    }

    fn attempt(run_id: i64, node_id: &str, status: &str, implementation: &str) -> NodeAttempt {
        NodeAttempt {
            id: 1,
            run_id,
            file_hash: "songA".to_string(),
            node_id: node_id.to_string(),
            status: status.to_string(),
            progress: 100,
            operation: String::new(),
            implementation: implementation.to_string(),
            model: String::new(),
            requested_device: String::new(),
            actual_device: String::new(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms: None,
            finished_at_ms: None,
        }
    }

    #[test]
    fn a_node_run_in_both_with_the_same_fields_has_no_changed_fields() {
        let history = vec![run(1, "songA", 1_000), run(2, "songA", 2_000)];
        let result = compare_analysis_runs_from(
            &history,
            1,
            vec![attempt(1, "pitch.extract", "succeeded", "RMVPE")],
            2,
            vec![attempt(2, "pitch.extract", "succeeded", "RMVPE")],
        )
        .unwrap();
        let diff = result
            .node_differences
            .iter()
            .find(|d| d.node_id == "pitch.extract")
            .unwrap();
        assert!(diff.changed_fields.is_empty());
        assert!(diff.attempt_a.is_some());
        assert!(diff.attempt_b.is_some());
    }

    #[test]
    fn a_changed_implementation_is_reported() {
        let history = vec![run(1, "songA", 1_000), run(2, "songA", 2_000)];
        let result = compare_analysis_runs_from(
            &history,
            1,
            vec![attempt(1, "stems.separate", "succeeded", "Demucs")],
            2,
            vec![attempt(2, "stems.separate", "succeeded", "UVR")],
        )
        .unwrap();
        let diff = result
            .node_differences
            .iter()
            .find(|d| d.node_id == "stems.separate")
            .unwrap();
        assert_eq!(diff.changed_fields, vec!["implementation"]);
    }

    #[test]
    fn a_node_only_attempted_in_one_run_has_no_changed_fields_but_a_missing_side() {
        let history = vec![run(1, "songA", 1_000), run(2, "songA", 2_000)];
        let result = compare_analysis_runs_from(
            &history,
            1,
            vec![attempt(1, "pitch.extract", "succeeded", "RMVPE")],
            2,
            vec![],
        )
        .unwrap();
        let diff = result
            .node_differences
            .iter()
            .find(|d| d.node_id == "pitch.extract")
            .unwrap();
        assert!(diff.attempt_a.is_some());
        assert!(diff.attempt_b.is_none());
        assert!(diff.changed_fields.is_empty());
    }

    #[test]
    fn comparing_runs_from_different_songs_is_rejected() {
        let history = vec![run(1, "songA", 1_000), run(2, "songB", 2_000)];
        let result = compare_analysis_runs_from(&history, 1, vec![], 2, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn an_unknown_run_id_is_rejected() {
        let history = vec![run(1, "songA", 1_000)];
        let result = compare_analysis_runs_from(&history, 1, vec![], 999, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn compare_with_previous_run_needs_a_real_history_lookup() {
        // compare_node_attempt_with_previous_run calls load_analysis_history
        // itself (real DB), so without a matching real run id this must
        // fail cleanly rather than panic -- the actual "found a previous
        // run" path is covered indirectly via compare_analysis_runs_from
        // above, same DB-avoidance reasoning as cancel_analysis_run_tests.
        let result = compare_node_attempt_with_previous_run(
            "compare-test-song-never-analyzed-xyz",
            "pitch.extract",
            999_999_999,
        );
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod cancel_analysis_run_tests {
    //! Deliberately does not cover the success path (actually removing a
    //! queued hash): `ANALYZER` is a real process-wide singleton with no
    //! test-injection seam, so mutating `state.queue` from a test risks
    //! interleaving with any other test that touches it -- same caution
    //! `run_analysis_plan_tests` below already documents for
    //! `enqueue_one`. The rejection path needs no such mutation.
    use super::cancel_analysis_run;

    #[test]
    fn cancelling_a_hash_that_was_never_queued_is_rejected() {
        let error = cancel_analysis_run("cancel-test-hash-never-queued-xyz")
            .expect_err("a hash that was never queued cannot be cancelled");
        assert!(error.contains("not currently queued"));
    }
}

#[cfg(test)]
mod run_analysis_plan_tests {
    // Deliberately does not cover `run_analysis_plan`'s success path: that
    // path calls `enqueue_one`, which spawns a real background worker
    // thread and touches the process-wide `ANALYZER`/library_db state --
    // out of scope for a unit test (`pipeline_flags_for_request`'s own
    // tests above already cover the flag-derivation logic this success
    // path relies on). Every case here is a rejection, which returns
    // before either of those side effects happen.
    use super::{node_can_be_disabled_for_run, run_analysis_plan};
    use std::collections::BTreeSet;

    #[test]
    fn rejects_disabling_a_node_the_pipeline_cannot_honor() {
        let result = run_analysis_plan(
            "run-analysis-plan-test-song",
            BTreeSet::new(),
            BTreeSet::from([crate::analysis_graph::AnalysisNodeId::new(
                "music.descriptors",
            )]),
        );
        let error = result.expect_err("music.descriptors cannot be gated by run_pipeline yet");
        assert!(error.contains("music.descriptors"));
        assert!(!node_can_be_disabled_for_run("music.descriptors"));
    }

    #[test]
    fn rejects_disabling_an_always_required_node() {
        let result = run_analysis_plan(
            "run-analysis-plan-test-song-2",
            BTreeSet::new(),
            BTreeSet::from([crate::analysis_graph::AnalysisNodeId::new(
                "chart.build_candidate",
            )]),
        );
        assert!(result.is_err());
    }

    #[test]
    fn every_pipeline_honorable_node_reports_itself_as_disableable() {
        for node_id in [
            "stems.separate",
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.align",
            "lyrics.import_timed",
        ] {
            assert!(
                node_can_be_disabled_for_run(node_id),
                "{node_id} should be disableable"
            );
        }
        for node_id in [
            "music.key",
            "music.rhythm",
            "preflight",
            "chart.build_candidate",
        ] {
            assert!(
                !node_can_be_disabled_for_run(node_id),
                "{node_id} should not be disableable"
            );
        }
    }
}

#[cfg(test)]
mod downstream_closure_tests {
    //! §7.5 "Run this node and downstream". `downstream_closure` is pure
    //! graph traversal over the real `baseline_graph_spec` edges, so these
    //! lock its shape directly against the graph rather than against
    //! `run_analysis_node_downstream`'s side-effecting success path (which
    //! -- like `run_analysis_plan`'s own success path -- calls
    //! `enqueue_one` and touches process-wide state, out of scope here).
    use super::downstream_closure;
    use crate::analysis_graph::{AnalysisNodeId, baseline_graph_spec};

    fn ids(values: &[&str]) -> std::collections::BTreeSet<AnalysisNodeId> {
        values.iter().map(|s| AnalysisNodeId::new(*s)).collect()
    }

    #[test]
    fn a_leaf_nodes_downstream_closure_is_only_itself() {
        let graph = baseline_graph_spec();
        assert_eq!(
            downstream_closure(&graph, &AnalysisNodeId::new("chart.build_candidate")),
            ids(&["chart.build_candidate"])
        );
    }

    #[test]
    fn stems_downstream_includes_pitch_and_the_lyrics_route_but_not_import_timed() {
        // lyrics.import_timed is fed directly by preflight (Timed LRC
        // doesn't need a vocal stem), so it must not appear here.
        let graph = baseline_graph_spec();
        assert_eq!(
            downstream_closure(&graph, &AnalysisNodeId::new("stems.separate")),
            ids(&[
                "stems.separate",
                "pitch.extract",
                "lyrics.preprocess",
                "lyrics.transcribe",
                "lyrics.align",
                "chart.build_candidate",
            ])
        );
    }

    #[test]
    fn preflights_downstream_closure_is_the_entire_graph() {
        let graph = baseline_graph_spec();
        let closure = downstream_closure(&graph, &AnalysisNodeId::new("preflight"));
        for node in &graph.nodes {
            assert!(
                closure.contains(&node.id),
                "{} missing from closure",
                node.id
            );
        }
    }

    #[test]
    fn pitch_downstream_never_pulls_in_its_own_ancestor_stems_separate() {
        let graph = baseline_graph_spec();
        let closure = downstream_closure(&graph, &AnalysisNodeId::new("pitch.extract"));
        assert!(!closure.contains(&AnalysisNodeId::new("stems.separate")));
        assert!(closure.contains(&AnalysisNodeId::new("chart.build_candidate")));
    }
}

#[cfg(test)]
mod freeze_analysis_node_tests {
    //! Phase 4 §4.5 Freeze consumer. `node_can_be_frozen_for_run` and
    //! `freeze_analysis_node_outputs_for_run` both check the same two
    //! preconditions (`pipeline_can_honor_freeze` + on-disk output
    //! existence); these tests exercise the pieces that don't need the real
    //! global data directory (`pipeline_can_honor_freeze`,
    //! `frozen_artifact_kinds_for_node`, `node_output_exists_for_freeze`
    //! against a temp `CacheDir`) directly, the same way
    //! `reanalysis_backup_tests` below tests cache-path logic without
    //! touching a real song.
    use super::{
        frozen_artifact_kinds_for_node, node_output_exists_for_freeze, pipeline_can_honor_freeze,
    };
    use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
    use crate::cache::CacheDir;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_cache_dir(label: &str) -> CacheDir {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-freeze-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp cache dir");
        CacheDir { path }
    }

    #[test]
    fn only_stems_and_pitch_are_freezable() {
        for id in ["stems.separate", "pitch.extract"] {
            assert!(
                pipeline_can_honor_freeze(&AnalysisNodeId::new(id)),
                "{id} should be freezable"
            );
        }
        // Lyrics nodes share one merged transcript.json -- no standalone
        // file to freeze independently until Phase 4 §4.4 artifact
        // splitting exists.
        for id in [
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.align",
            "lyrics.import_timed",
            "music.analysis",
            "preflight",
            "chart.build_candidate",
        ] {
            assert!(
                !pipeline_can_honor_freeze(&AnalysisNodeId::new(id)),
                "{id} should not be freezable yet"
            );
        }
    }

    #[test]
    fn frozen_artifact_kinds_map_to_the_nodes_real_outputs() {
        assert_eq!(
            frozen_artifact_kinds_for_node(&AnalysisNodeId::new("stems.separate")),
            std::collections::BTreeSet::from([
                ArtifactKind::VocalStem,
                ArtifactKind::InstrumentalStem
            ]),
        );
        assert_eq!(
            frozen_artifact_kinds_for_node(&AnalysisNodeId::new("pitch.extract")),
            std::collections::BTreeSet::from([
                ArtifactKind::PitchTrack,
                ArtifactKind::PitchNoteCandidates
            ]),
        );
        assert!(frozen_artifact_kinds_for_node(&AnalysisNodeId::new("music.analysis")).is_empty());
    }

    #[test]
    fn stems_output_missing_is_not_freezable() {
        let cache = temp_cache_dir("stems-missing");
        assert!(!node_output_exists_for_freeze(
            &cache,
            "songA",
            &AnalysisNodeId::new("stems.separate")
        ));
        cache.clear_all();
    }

    #[test]
    fn stems_output_requires_both_vocal_and_instrumental_files() {
        let cache = temp_cache_dir("stems-partial");
        std::fs::write(cache.vocals_path("songA"), b"fake-audio").unwrap();
        // Instrumental missing -- must not report freezable on vocals alone.
        assert!(!node_output_exists_for_freeze(
            &cache,
            "songA",
            &AnalysisNodeId::new("stems.separate")
        ));
        std::fs::write(cache.instrumental_path("songA"), b"fake-audio").unwrap();
        assert!(node_output_exists_for_freeze(
            &cache,
            "songA",
            &AnalysisNodeId::new("stems.separate")
        ));
        cache.clear_all();
    }

    #[test]
    fn pitch_output_requires_both_track_and_notes_files() {
        let cache = temp_cache_dir("pitch-partial");
        std::fs::write(cache.pitch_track_path("songA"), b"{}").unwrap();
        assert!(!node_output_exists_for_freeze(
            &cache,
            "songA",
            &AnalysisNodeId::new("pitch.extract")
        ));
        std::fs::write(cache.pitch_notes_path("songA"), b"{}").unwrap();
        assert!(node_output_exists_for_freeze(
            &cache,
            "songA",
            &AnalysisNodeId::new("pitch.extract")
        ));
        cache.clear_all();
    }

    #[test]
    fn a_different_song_hash_never_sees_another_songs_frozen_output() {
        let cache = temp_cache_dir("cross-song");
        std::fs::write(cache.vocals_path("songA"), b"fake-audio").unwrap();
        std::fs::write(cache.instrumental_path("songA"), b"fake-audio").unwrap();
        assert!(!node_output_exists_for_freeze(
            &cache,
            "songB",
            &AnalysisNodeId::new("stems.separate")
        ));
        cache.clear_all();
    }
}

#[cfg(test)]
mod bypass_analysis_node_tests {
    //! Phase 4 §4.5 Bypass consumer.
    use super::{node_can_be_bypassed_for_run, pipeline_can_honor_bypass};
    use crate::analysis_graph::AnalysisNodeId;

    #[test]
    fn only_stems_separate_can_be_bypassed() {
        assert!(pipeline_can_honor_bypass(&AnalysisNodeId::new(
            "stems.separate"
        )));
        for id in [
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.align",
            "lyrics.import_timed",
            "music.analysis",
            "preflight",
            "chart.build_candidate",
        ] {
            assert!(
                !pipeline_can_honor_bypass(&AnalysisNodeId::new(id)),
                "{id} should not be bypassable yet"
            );
        }
    }

    #[test]
    fn node_can_be_bypassed_for_run_has_no_per_song_existence_check() {
        // Unlike Freeze, Bypass's substitute input is the song's own source
        // media -- always present for a real song, so this is purely
        // structural and doesn't need a real file_hash to answer correctly.
        assert!(node_can_be_bypassed_for_run("stems.separate"));
        assert!(!node_can_be_bypassed_for_run("pitch.extract"));
    }
}

#[cfg(test)]
mod configure_node_tests {
    //! Phase 8's Run tier (§8.4's previously-missing third tier).
    //! Deliberately does not cover `configure_analysis_node_for_run`'s
    //! success path (calls `enqueue_one`, same real-side-effect concern
    //! `run_analysis_plan_tests` already documents) -- only its rejection
    //! path, plus the pure mapping and the `PENDING_NODE_INTENTS`
    //! read-through, which don't touch the real analyzer process.
    use super::{
        PENDING_NODE_INTENTS, configure_analysis_node_for_run, node_can_be_configured_for_run,
        pending_run_override_for, save_node_config_as_song_profile,
    };
    use crate::analysis_profile::{
        AnalysisProfileSnapshot, ProfileField, get_song_analysis_profile, set_song_analysis_profile,
    };
    use crate::config::AppConfig;
    use std::sync::Mutex;

    /// `PENDING_NODE_INTENTS` is a process-wide singleton; serialize tests
    /// that touch it, same reasoning as `pending_intent_tests`'s guard.
    static GUARD: Mutex<()> = Mutex::new(());

    fn isolated_test_db(label: &str) -> std::sync::MutexGuard<'static, ()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "uta-studio-configure-node-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        crate::library_db::reconnect_for_test(&dir)
    }

    #[test]
    fn only_the_three_profile_controlled_nodes_are_configurable() {
        for id in ["stems.separate", "lyrics.transcribe", "lyrics.align"] {
            assert!(
                node_can_be_configured_for_run(id),
                "{id} should be configurable"
            );
        }
        for id in [
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.import_timed",
            "music.analysis",
            "preflight",
            "chart.build_candidate",
        ] {
            assert!(
                !node_can_be_configured_for_run(id),
                "{id} should not be configurable"
            );
        }
    }

    #[test]
    fn configure_and_save_both_reject_a_node_with_no_controllable_field() {
        let error = configure_analysis_node_for_run("some-song", "music.analysis", "x".into())
            .expect_err("music.analysis has no profile-controlled parameter");
        assert!(error.contains("music.analysis"));

        let error = save_node_config_as_song_profile("some-song", "music.analysis")
            .expect_err("music.analysis has no profile-controlled parameter");
        assert!(error.contains("music.analysis"));
    }

    #[test]
    fn pending_run_override_only_surfaces_for_the_field_the_node_maps_to() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "configure-node-test-pending-override";
        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);
        {
            let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
            intents.entry(hash.to_string()).or_default().run_override =
                Some((ProfileField::Separator, "demucs".to_string()));
        }

        assert_eq!(
            pending_run_override_for(hash, "stems.separate"),
            Some("demucs".to_string())
        );
        // lyrics.transcribe maps to AsrEngine, a different field -- the
        // Separator override stashed above must not leak into it.
        assert_eq!(pending_run_override_for(hash, "lyrics.transcribe"), None);

        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);
    }

    #[test]
    fn pending_run_override_is_none_when_nothing_is_queued() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let hash = "configure-node-test-no-pending-override";
        PENDING_NODE_INTENTS.lock().unwrap().remove(hash);
        assert_eq!(pending_run_override_for(hash, "stems.separate"), None);
    }

    #[test]
    fn save_as_song_profile_preserves_other_fields_when_saving_just_one() {
        let _db_guard = isolated_test_db("preserve-others");
        let hash = "configure-node-test-song-preserve";
        let seeded = AnalysisProfileSnapshot {
            alignment_backend: "mms_karaoke".to_string(),
            ..AnalysisProfileSnapshot::from_app_config(&AppConfig::load(), hash)
        };
        set_song_analysis_profile(hash, &seeded).unwrap();

        save_node_config_as_song_profile(hash, "stems.separate").unwrap();

        let saved = get_song_analysis_profile(hash).unwrap();
        assert_eq!(saved.alignment_backend, "mms_karaoke");
        assert_eq!(saved.separator, seeded.separator);
    }

    #[test]
    fn save_as_song_profile_seeds_a_fresh_profile_from_real_global_defaults() {
        let _db_guard = isolated_test_db("fresh-profile");
        let hash = "configure-node-test-song-fresh";
        assert!(get_song_analysis_profile(hash).is_none());

        save_node_config_as_song_profile(hash, "lyrics.transcribe").unwrap();

        let saved = get_song_analysis_profile(hash).expect("a profile now exists");
        let expected_global = AnalysisProfileSnapshot::from_app_config(&AppConfig::load(), hash);
        assert_eq!(saved.asr_engine, expected_global.asr_engine);
        // Untouched fields are also seeded from the real global config, not
        // left as `AnalysisProfileSnapshot::default()`'s hardcoded values.
        assert_eq!(saved.separator, expected_global.separator);
        assert_eq!(saved.alignment_backend, expected_global.alignment_backend);
    }
}

#[cfg(test)]
mod reanalysis_backup_tests {
    //! Phase 5 fix (docs/analysis-dag-redesign.md, phase plan §9.2 "失败时
    //! 保留旧 Pitch"): `reanalyze_pitch` used to delete old pitch data
    //! *before* the rerun was even queued, so a failed/crashed/OOM-killed
    //! rerun permanently destroyed the previous good output. These tests
    //! lock the rename-instead-of-delete + existence-based
    //! restore-or-commit behavior that replaced it.
    use super::{apply_pitch_reanalysis_reset, back_up_before_reset, restore_or_commit_backup};
    use crate::cache::CacheDir;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_cache_dir(label: &str) -> CacheDir {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-reanalysis-backup-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp cache dir");
        CacheDir { path }
    }

    #[test]
    fn back_up_before_reset_renames_an_existing_file_and_returns_the_pair() {
        let cache = temp_cache_dir("rename");
        let original = cache.path.join("pitch.json");
        std::fs::write(&original, b"old pitch data").unwrap();

        let (returned_original, backup) = back_up_before_reset(&original).unwrap();

        assert_eq!(returned_original, original);
        assert!(!original.is_file(), "original must be moved, not copied");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old pitch data");
        cache.clear_all();
    }

    #[test]
    fn back_up_before_reset_returns_none_when_nothing_exists_to_back_up() {
        let cache = temp_cache_dir("missing");
        let original = cache.path.join("does-not-exist.json");
        assert!(back_up_before_reset(&original).is_none());
        cache.clear_all();
    }

    #[test]
    fn back_up_before_reset_clears_a_stale_leftover_backup_first() {
        // A .bak from some earlier, never-resolved run must not silently
        // become "the" backup for this run's original content.
        let cache = temp_cache_dir("stale");
        let original = cache.path.join("pitch.json");
        let mut backup_name = original.as_os_str().to_os_string();
        backup_name.push(".bak");
        let stale_backup = std::path::PathBuf::from(&backup_name);
        std::fs::write(&stale_backup, b"stale leftover").unwrap();
        std::fs::write(&original, b"current data").unwrap();

        back_up_before_reset(&original).unwrap();

        assert_eq!(std::fs::read(&stale_backup).unwrap(), b"current data");
        cache.clear_all();
    }

    #[test]
    fn restore_or_commit_backup_deletes_the_backup_when_a_fresh_file_was_written() {
        let cache = temp_cache_dir("commit");
        let original = cache.path.join("pitch.json");
        let backup = cache.path.join("pitch.json.bak");
        std::fs::write(&backup, b"old data").unwrap();
        std::fs::write(&original, b"freshly regenerated data").unwrap();

        restore_or_commit_backup(&original, &backup);

        assert!(!backup.is_file());
        assert_eq!(
            std::fs::read(&original).unwrap(),
            b"freshly regenerated data"
        );
        cache.clear_all();
    }

    #[test]
    fn restore_or_commit_backup_restores_the_old_file_when_the_run_produced_nothing() {
        // The exact bug being fixed: a failed/crashed/OOM-killed rerun (or
        // pipeline.py's analyze_pitch catching its own exception and
        // continuing without writing anything) must not leave the song
        // pitch-less.
        let cache = temp_cache_dir("restore");
        let original = cache.path.join("pitch.json");
        let backup = cache.path.join("pitch.json.bak");
        std::fs::write(&backup, b"old good pitch data").unwrap();

        restore_or_commit_backup(&original, &backup);

        assert!(!backup.is_file());
        assert_eq!(std::fs::read(&original).unwrap(), b"old good pitch data");
        cache.clear_all();
    }

    #[test]
    fn apply_pitch_reanalysis_reset_backs_up_both_pitch_files_and_leaves_neither_at_its_original_path()
     {
        let cache = temp_cache_dir("apply-reset");
        let hash = "songPitchReset";
        std::fs::write(cache.pitch_track_path(hash), b"track data").unwrap();
        std::fs::write(cache.pitch_notes_path(hash), b"notes data").unwrap();

        let backups = apply_pitch_reanalysis_reset(&cache, hash);

        assert_eq!(backups.len(), 2);
        assert!(!cache.pitch_track_path(hash).is_file());
        assert!(!cache.pitch_notes_path(hash).is_file());
        for (original, backup) in &backups {
            assert!(!original.is_file());
            assert!(backup.is_file());
        }
        cache.clear_all();
    }

    #[test]
    fn apply_pitch_reanalysis_reset_is_a_noop_when_no_prior_pitch_data_exists() {
        // A song being analyzed for the first time (or one whose pitch
        // extraction already failed and left nothing behind) must not
        // error or fabricate a backup out of nothing.
        let cache = temp_cache_dir("apply-reset-empty");
        let hash = "songNeverAnalyzed";

        let backups = apply_pitch_reanalysis_reset(&cache, hash);

        assert!(backups.is_empty());
        cache.clear_all();
    }

    // The realign/reanalyze extension of the same fix (docs/plan.md §2 item
    // 5, "realign/reanalyze_full 的同款急切删除问题"): identical
    // trigger-time eager-delete bug over a larger, directory-scanned file
    // set, now made safe the same way instead of left as a known gap.
    use super::{apply_realign_reset, apply_reanalyze_reset};

    #[test]
    fn apply_realign_reset_backs_up_the_transcript_and_every_variant() {
        let cache = temp_cache_dir("realign-reset");
        let hash = "songRealign";
        std::fs::write(cache.transcript_path(hash), b"base transcript").unwrap();
        std::fs::write(
            cache.variant_transcript_path(hash, 1.25),
            b"variant transcript",
        )
        .unwrap();

        let backups = apply_realign_reset(&cache, hash);

        assert_eq!(backups.len(), 2);
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.variant_transcript_path(hash, 1.25).is_file());
        for (original, backup) in &backups {
            assert!(!original.is_file());
            assert!(backup.is_file());
        }
        cache.clear_all();
    }

    #[test]
    fn apply_realign_reset_also_backs_up_the_split_transcript_artifacts_when_present() {
        // §4.4: a realign must not leave stale recognized_text/asr_segments
        // from the previous run behind once transcript.json/timed_transcript.json
        // regenerate fresh.
        let cache = temp_cache_dir("realign-reset-split");
        let hash = "songRealignSplit";
        std::fs::write(cache.transcript_path(hash), b"base transcript").unwrap();
        std::fs::write(cache.recognized_text_path(hash), b"recognized").unwrap();
        std::fs::write(cache.asr_segments_path(hash), b"segments").unwrap();
        std::fs::write(cache.timed_transcript_path(hash), b"timed").unwrap();

        let backups = apply_realign_reset(&cache, hash);

        assert_eq!(backups.len(), 4);
        assert!(!cache.recognized_text_path(hash).is_file());
        assert!(!cache.asr_segments_path(hash).is_file());
        assert!(!cache.timed_transcript_path(hash).is_file());
        for (original, backup) in &backups {
            assert!(!original.is_file());
            assert!(backup.is_file());
        }
        cache.clear_all();
    }

    #[test]
    fn apply_realign_reset_leaves_the_authored_chart_alone() {
        let cache = temp_cache_dir("realign-reset-chart");
        let hash = "songRealignChart";
        std::fs::write(cache.transcript_path(hash), b"base transcript").unwrap();
        std::fs::write(cache.vocal_chart_path(hash), b"authored chart").unwrap();

        apply_realign_reset(&cache, hash);

        assert!(cache.vocal_chart_path(hash).is_file());
        assert_eq!(
            std::fs::read(cache.vocal_chart_path(hash)).unwrap(),
            b"authored chart"
        );
        cache.clear_all();
    }

    #[test]
    fn apply_reanalyze_reset_transcript_only_backs_up_transcript_lyrics_and_variants_but_not_pitch()
    {
        let cache = temp_cache_dir("reanalyze-transcript-reset");
        let hash = "songReanalyzeTranscript";
        std::fs::write(cache.transcript_path(hash), b"transcript").unwrap();
        std::fs::write(cache.lyrics_path(hash), b"lyrics").unwrap();
        std::fs::write(cache.variant_transcript_path(hash, 0.8), b"variant").unwrap();
        std::fs::write(cache.pitch_track_path(hash), b"pitch track").unwrap();

        let backups = apply_reanalyze_reset(&cache, hash, false);

        assert_eq!(backups.len(), 3);
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.lyrics_path(hash).is_file());
        assert!(!cache.variant_transcript_path(hash, 0.8).is_file());
        // Transcript-only reanalysis must not touch pitch data at all --
        // neither delete it nor back it up.
        assert!(cache.pitch_track_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn apply_reanalyze_reset_transcript_only_also_backs_up_the_split_transcript_artifacts() {
        let cache = temp_cache_dir("reanalyze-transcript-reset-split");
        let hash = "songReanalyzeTranscriptSplit";
        std::fs::write(cache.transcript_path(hash), b"transcript").unwrap();
        std::fs::write(cache.recognized_text_path(hash), b"recognized").unwrap();
        std::fs::write(cache.asr_segments_path(hash), b"segments").unwrap();
        std::fs::write(cache.timed_transcript_path(hash), b"timed").unwrap();
        std::fs::write(cache.pitch_track_path(hash), b"pitch track").unwrap();

        let backups = apply_reanalyze_reset(&cache, hash, false);

        assert_eq!(backups.len(), 4);
        assert!(!cache.recognized_text_path(hash).is_file());
        assert!(!cache.asr_segments_path(hash).is_file());
        assert!(!cache.timed_transcript_path(hash).is_file());
        // Transcript-only reanalysis must not touch pitch data.
        assert!(cache.pitch_track_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn apply_reanalyze_reset_full_backs_up_every_analysis_output_but_not_the_authored_chart() {
        let cache = temp_cache_dir("reanalyze-full-reset");
        let hash = "songReanalyzeFull";
        std::fs::write(cache.transcript_path(hash), b"transcript").unwrap();
        std::fs::write(cache.pitch_track_path(hash), b"pitch track").unwrap();
        std::fs::write(cache.pitch_notes_path(hash), b"pitch notes").unwrap();
        std::fs::write(cache.music_analysis_path(hash), b"music analysis").unwrap();
        std::fs::write(cache.vocal_chart_path(hash), b"authored chart").unwrap();

        let backups = apply_reanalyze_reset(&cache, hash, true);

        assert_eq!(backups.len(), 4);
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.pitch_track_path(hash).is_file());
        assert!(!cache.pitch_notes_path(hash).is_file());
        assert!(!cache.music_analysis_path(hash).is_file());
        for (original, backup) in &backups {
            assert!(!original.is_file());
            assert!(backup.is_file());
        }
        assert!(
            cache.vocal_chart_path(hash).is_file(),
            "full reanalysis must still preserve the Authored Chart by default"
        );
        cache.clear_all();
    }

    #[test]
    fn apply_reanalyze_reset_is_a_noop_when_nothing_exists_yet() {
        let cache = temp_cache_dir("reanalyze-reset-empty");
        let hash = "songReanalyzeNeverRun";

        assert!(apply_reanalyze_reset(&cache, hash, true).is_empty());
        assert!(apply_reanalyze_reset(&cache, hash, false).is_empty());
        cache.clear_all();
    }
}

#[cfg(test)]
mod node_attempt_tests {
    use super::{
        AnalysisProgressSnapshot, AnalysisStageRoute, node_attempt_status, record_node_attempts,
    };
    use crate::library_db;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-node-attempt-status-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp db root");
        path
    }

    #[test]
    fn node_attempt_status_maps_every_real_event_kind() {
        assert_eq!(node_attempt_status(Some("node_completed")), "succeeded");
        assert_eq!(node_attempt_status(Some("node_failed")), "failed");
        assert_eq!(node_attempt_status(Some("artifact_reused")), "reused");
    }

    #[test]
    fn node_attempt_status_treats_unterminated_or_unknown_events_as_incomplete() {
        // node_started/node_progress mean the node was reached but the run
        // ended (or moved on) before a terminal event -- not success, not
        // failure, and not silently dropped either.
        assert_eq!(node_attempt_status(Some("node_started")), "incomplete");
        assert_eq!(node_attempt_status(Some("node_progress")), "incomplete");
        assert_eq!(
            node_attempt_status(Some("something_unrecognized")),
            "incomplete"
        );
        assert_eq!(node_attempt_status(None), "incomplete");
    }

    fn route(node_id: Option<&str>, node_event: Option<&str>) -> AnalysisStageRoute {
        AnalysisStageRoute {
            stage: "pitch".to_string(),
            node_id: node_id.map(str::to_string),
            node_event: node_event.map(str::to_string),
            operation: "Reference pitch extraction".to_string(),
            implementation: "RMVPE".to_string(),
            model: "RMVPE singing pitch model".to_string(),
            stage_progress: 100,
            requested_device: "cpu".to_string(),
            actual_device: "cpu".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms: None,
            finished_at_ms: None,
        }
    }

    #[test]
    fn record_node_attempts_skips_routes_without_a_real_node_id() {
        let root = temp_root("skip-legacy");
        let _guard = library_db::reconnect_for_test(&root);
        let run_id = library_db::analysis_history_insert(
            "songE",
            "Title",
            "Artist",
            "completed",
            1_000,
            2_000,
            "{}",
            None,
        )
        .expect("insert run");

        let snapshot = AnalysisProgressSnapshot {
            stage: "complete".into(),
            stage_progress: 100,
            operation: "Analysis complete".into(),
            detail: String::new(),
            implementation: String::new(),
            model: String::new(),
            device: String::new(),
            requested_device: String::new(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            stage_routes: vec![
                route(Some("pitch.extract"), Some("node_completed")),
                route(None, None),
            ],
            node_id: None,
            node_event: None,
            artifact_reused_reason: None,
        };
        record_node_attempts(run_id, "songE", &snapshot);

        let attempts = library_db::analysis_node_attempts_load(run_id).expect("load attempts");
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].node_id, "pitch.extract");
        assert_eq!(attempts[0].status, "succeeded");

        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod enqueue_tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{QueuedStatus, queue_entry_blocks_enqueue, validate_analysis_source};

    #[test]
    fn analyze_all_retries_failed_entries_but_not_active_work() {
        assert!(!queue_entry_blocks_enqueue(None));
        assert!(!queue_entry_blocks_enqueue(Some(&QueuedStatus::Failed(
            "previous failure".into()
        ))));
        assert!(queue_entry_blocks_enqueue(Some(&QueuedStatus::Queued)));
        assert!(queue_entry_blocks_enqueue(Some(&QueuedStatus::Analyzing(
            42
        ))));
    }

    #[test]
    fn empty_analysis_source_is_rejected_before_server_start() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-empty-analysis-source-{}-{nonce}.flac",
            std::process::id()
        ));
        std::fs::File::create(&path).expect("create empty source fixture");
        let error = validate_analysis_source(&path).expect_err("empty source must be rejected");
        let _ = std::fs::remove_file(&path);
        assert!(error.to_string().contains("source media is empty"));
    }
}

pub fn shutdown_server() {
    let pid = SERVER_PID.swap(0, Ordering::SeqCst);
    if pid != 0 {
        info!("[analyzer] Graceful shutdown of server (pid={pid})");
        // A process killed here must not remain in the singleton.  Otherwise
        // `ensure_server` sees `Some` and the next analysis attempts to reuse
        // a dead connection (or, during setup, an old Python environment).
        if let Ok(mut guard) = ANALYZER_SERVER.try_lock() {
            if let Some(server) = guard.as_mut() {
                let _ = server.writer.write_all(b"{\"type\":\"quit\"}\n");
                let _ = server.writer.flush();
            }
            *guard = None;
            return;
        }
        std::thread::spawn(move || {
            let _ = Command::new("kill").args([&pid.to_string()]).status();
            std::thread::sleep(std::time::Duration::from_secs(3));
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
        });
    }
}

pub fn delete_cache(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }
    let cache = CacheDir::new();
    cache.delete_song_cache(file_hash);
    update_song_analyzed(file_hash, false, None, None, None, None, None);
}

pub fn reanalyze_transcript(file_hash: &str, language: Option<String>) {
    if is_usdx_song(file_hash) {
        return;
    }

    if let Some(lang) = language
        && !lang.is_empty()
    {
        let mut config = AppConfig::load();
        config.set_language_override(file_hash.to_string(), lang);
        if let Err(error) = config.save() {
            tracing::error!("Could not save language override: {error}");
            return;
        }
    }
    reanalyze(file_hash, false);
}

pub fn reanalyze_full(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }

    reanalyze(file_hash, true);
}

/// Clears cached pitch evidence so `pitch.extract` regenerates it.
/// Deliberately does **not** touch the Authored Chart
/// (docs/analysis-dag-redesign.md §6/Phase 5): a pitch-only rerun must
/// leave the user's edited chart in place, with new evidence available to
/// compare/merge rather than silently replacing it.
/// Renames `path` to a sibling `.bak` path if a file currently exists
/// there, returning `(original, backup)` for `restore_or_commit_backup` to
/// resolve once the triggered run finishes. Used instead of deleting
/// outright so a run that fails, crashes, or gets OOM-killed doesn't
/// destroy the previous good output for nothing.
fn back_up_before_reset(path: &Path) -> Option<(PathBuf, PathBuf)> {
    if !path.is_file() {
        return None;
    }
    let mut backup_name = path.as_os_str().to_os_string();
    backup_name.push(".bak");
    let backup = PathBuf::from(backup_name);
    // Clear out a stale backup left behind by some earlier run that never
    // got resolved (shouldn't happen -- restore_or_commit_backup always
    // consumes it -- but a leftover .bak must never silently become "the"
    // backup for two different runs).
    let _ = std::fs::remove_file(&backup);
    std::fs::rename(path, &backup)
        .ok()
        .map(|()| (path.to_path_buf(), backup))
}

/// Resolves one backed-up file once its triggering run has finished,
/// regardless of *how* it finished: if a fresh file now exists at the
/// original path, the run produced new output and the backup is no longer
/// needed. Otherwise (failure, crash, or a node that quietly skipped itself
/// -- e.g. `analyze_pitch`'s own exception handler in pipeline.py logs and
/// continues rather than failing the whole run) the old data is restored.
/// Deliberately existence-based rather than gated on `SongResult`, since a
/// single failed node doesn't necessarily surface as an overall run
/// failure.
fn restore_or_commit_backup(original: &Path, backup: &Path) {
    if original.is_file() {
        let _ = std::fs::remove_file(backup);
    } else {
        let _ = std::fs::rename(backup, original);
    }
}

fn apply_pitch_reanalysis_reset(cache: &CacheDir, file_hash: &str) -> Vec<(PathBuf, PathBuf)> {
    [
        cache.pitch_track_path(file_hash),
        cache.pitch_notes_path(file_hash),
    ]
    .into_iter()
    .filter_map(|path| back_up_before_reset(&path))
    .collect()
}

pub fn reanalyze_pitch(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }

    let cache = CacheDir::new();
    let backups = apply_pitch_reanalysis_reset(&cache, file_hash);
    let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
    let intent = intents.entry(file_hash.to_string()).or_default();
    intent
        .targets
        .insert(crate::analysis_graph::AnalysisNodeId::new("pitch.extract"));
    intent.backup_paths.extend(backups);
    drop(intents);
    enqueue_one(file_hash);
}

/// Drops the transcript so `lyrics.align` regenerates it from the (possibly
/// re-fetched) lyrics source. Preserves the Authored Chart -- realigning
/// must not throw away chart edits just because word timing is about to be
/// recomputed (docs/analysis-dag-redesign.md §6/Phase 5). Same
/// backup-before-delete pattern as `apply_pitch_reanalysis_reset` (Phase 5
/// "失败时保留旧 Pitch"): renames the transcript and each variant aside
/// instead of deleting outright, so a crashed/cancelled realign doesn't
/// destroy the previous good transcript for nothing.
fn apply_realign_reset(cache: &CacheDir, file_hash: &str) -> Vec<(PathBuf, PathBuf)> {
    [
        cache.transcript_path(file_hash),
        cache.recognized_text_path(file_hash),
        cache.asr_segments_path(file_hash),
        cache.timed_transcript_path(file_hash),
    ]
    .into_iter()
    .chain(cache.transcript_variant_paths(file_hash))
    .filter_map(|path| back_up_before_reset(&path))
    .collect()
}

pub fn realign(file_hash: &str, language: Option<String>) {
    if is_usdx_song(file_hash) {
        return;
    }

    if let Some(lang) = language.as_ref().filter(|lang| !lang.is_empty()) {
        let mut config = AppConfig::load();
        config.set_language_override(file_hash.to_string(), lang.clone());
        if let Err(error) = config.save() {
            tracing::error!("Could not save language override: {error}");
            return;
        }
    }

    let cache = CacheDir::new();
    let previous_language = library_db::load_song_by_hash(file_hash)
        .ok()
        .flatten()
        .and_then(|song| song.language);
    materialize_lyrics_from_transcript(&cache, file_hash);
    let backups = apply_realign_reset(&cache, file_hash);
    PENDING_NODE_INTENTS
        .lock()
        .unwrap()
        .entry(file_hash.to_string())
        .or_default()
        .backup_paths
        .extend(backups);
    update_song_analyzed(
        file_hash,
        false,
        language.or(previous_language),
        None,
        None,
        None,
        None,
    );
    enqueue_one(file_hash);
}

pub fn reanalyze_force_transcribe(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }

    PENDING_NODE_INTENTS
        .lock()
        .unwrap()
        .entry(file_hash.to_string())
        .or_default()
        .force_transcribe = true;

    reanalyze(file_hash, false);
}

/// Full or transcript-only reanalysis reset. Neither branch touches the
/// Authored Chart: "Reanalyze all" regenerating every analysis artifact
/// still must not discard the user's chart edits by default
/// (docs/analysis-dag-redesign.md §6/Phase 5; phase plan Phase 9 test
/// "Full Reanalysis 默认保留 Authored Chart"). Same backup-before-delete
/// pattern as `apply_pitch_reanalysis_reset`/`apply_realign_reset`, made
/// possible for the `full` branch's larger, directory-scanned file set by
/// `CacheDir::analysis_output_paths_keep_chart` -- a real enumeration of
/// what `delete_analysis_outputs_keep_chart` would remove, not a
/// hand-maintained duplicate list that could drift from it.
fn apply_reanalyze_reset(cache: &CacheDir, file_hash: &str, full: bool) -> Vec<(PathBuf, PathBuf)> {
    let paths: Vec<PathBuf> = if full {
        cache.analysis_output_paths_keep_chart(file_hash)
    } else {
        [
            cache.transcript_path(file_hash),
            cache.recognized_text_path(file_hash),
            cache.asr_segments_path(file_hash),
            cache.timed_transcript_path(file_hash),
            cache.lyrics_path(file_hash),
        ]
        .into_iter()
        .chain(cache.transcript_variant_paths(file_hash))
        .collect()
    };
    paths
        .into_iter()
        .filter_map(|path| back_up_before_reset(&path))
        .collect()
}

fn reanalyze(file_hash: &str, full: bool) {
    let cache = CacheDir::new();
    let backups = apply_reanalyze_reset(&cache, file_hash, full);
    PENDING_NODE_INTENTS
        .lock()
        .unwrap()
        .entry(file_hash.to_string())
        .or_default()
        .backup_paths
        .extend(backups);
    update_song_analyzed(file_hash, false, None, None, None, None, None);
    enqueue_one(file_hash);
}

fn materialize_lyrics_from_transcript(cache: &CacheDir, file_hash: &str) {
    if cache.lyrics_path(file_hash).is_file() {
        return;
    }

    let transcript_path = cache.transcript_path(file_hash);
    let Ok(data) = std::fs::read_to_string(&transcript_path) else {
        return;
    };

    #[derive(Deserialize)]
    struct Segment {
        #[serde(default)]
        text: String,
    }

    #[derive(Deserialize)]
    struct TranscriptShape {
        #[serde(default)]
        segments: Vec<Segment>,
    }

    let Ok(parsed) = serde_json::from_str::<TranscriptShape>(&data) else {
        return;
    };

    let lines: Vec<String> = parsed
        .segments
        .into_iter()
        .map(|s| s.text.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return;
    }

    if let Err(e) = write_lyrics_file(cache, file_hash, &lines) {
        warn!("[analyzer] Failed to materialize lyrics from transcript for {file_hash}: {e}");
    }
}

fn normalize_analysis_language(language: &str) -> String {
    let normalized = language.trim().to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "jp" | "jpn" => "ja".into(),
        "eng" => "en".into(),
        "kor" => "ko".into(),
        "chi" | "zho" | "cn" | "zh-cn" | "zh-tw" => "zh".into(),
        _ => normalized,
    }
}

// ─── Worker ──────────────────────────────────────────────────────────

fn spawn_worker() {
    std::thread::spawn(|| {
        let cache = CacheDir::new();

        loop {
            let file_hash = {
                let mut state = ANALYZER.lock().unwrap();
                match state.queue.pop_front() {
                    Some(hash) => {
                        state.active_hash = Some(hash.clone());
                        hash
                    }
                    None => {
                        state.worker_running = false;
                        state.active_hash = None;
                        return;
                    }
                }
            };

            process_song(&file_hash, &cache);

            let mut state = ANALYZER.lock().unwrap();
            state.active_hash = None;
        }
    });
}

fn process_song(initial_hash: &str, cache: &CacheDir) {
    ANALYSIS_STARTED
        .lock()
        .unwrap()
        .insert(initial_hash.to_string(), unix_time_ms());
    update_queue_status(initial_hash, QueuedStatus::Analyzing(0));
    update_live_analysis(
        initial_hash,
        AnalysisProgressSnapshot {
            stage: "preparing".into(),
            stage_progress: 0,
            operation: "Validating source media".into(),
            detail: "Checking the source before the analysis runtime starts.".into(),
            implementation: "Uta Studio native preflight".into(),
            model: "Source validation".into(),
            device: "CPU".into(),
            requested_device: "CPU".into(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            stage_routes: Vec::new(),
            node_id: Some("preflight".to_string()),
            node_event: Some("node_started".to_string()),
            artifact_reused_reason: None,
        },
    );
    // Note: a `reanalyze_pitch`-style backup recorded into
    // `PENDING_NODE_INTENTS` (see `resolve_backups` further down) isn't
    // drained or resolved by either early return below -- the song record
    // vanishing from the DB, or the source file failing to prepare, between
    // enqueue and this point. Both require the song to already have had a
    // successful prior analysis (for there to be anything to back up) and
    // then fail in this specific narrow window, which is rare; the residual
    // risk is an orphaned `.bak` file next to the original cache entry, not
    // silent data loss -- strictly better than the pre-fix behavior, even
    // though it isn't auto-restored here.
    let Some(song) = library_db::load_song_by_hash(initial_hash).ok().flatten() else {
        warn!("[analyzer] Song with hash {initial_hash} not found in store, skipping");
        return;
    };

    let (song, local_path, file_hash_owned) = match prepare_audio_for_analysis(&song, cache) {
        Ok(out) => out,
        Err(e) => {
            warn!("[analyzer] Failed to prepare audio for analysis: {e}");
            update_queue_status(
                initial_hash,
                QueuedStatus::Failed(format!("audio prep failed: {e}")),
            );
            return;
        }
    };
    let file_hash = file_hash_owned.as_str();

    if file_hash != initial_hash {
        let snapshot = LIVE_ANALYSIS.lock().unwrap().remove(initial_hash);
        if let Some(snapshot) = snapshot {
            LIVE_ANALYSIS
                .lock()
                .unwrap()
                .insert(file_hash.to_string(), snapshot);
        }
        let started = ANALYSIS_STARTED.lock().unwrap().remove(initial_hash);
        if let Some(started) = started {
            ANALYSIS_STARTED
                .lock()
                .unwrap()
                .insert(file_hash.to_string(), started);
        }
    }

    info!(
        "[analyzer] Starting analysis: {} (hash={})",
        local_path.display(),
        file_hash
    );

    update_queue_status(file_hash, QueuedStatus::Analyzing(0));

    // Node targeting for this run. The intent may have been keyed by the
    // pre-rekey hash for remote songs, so both are drained and merged.
    let intent = {
        let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
        let current = intents.remove(file_hash);
        let initial = if file_hash != initial_hash {
            intents.remove(initial_hash)
        } else {
            None
        };
        match (current, initial) {
            (Some(mut a), Some(b)) => {
                a.targets.extend(b.targets);
                a.force_transcribe |= b.force_transcribe;
                a.backup_paths.extend(b.backup_paths);
                a.disabled_nodes.extend(b.disabled_nodes);
                a.frozen_artifacts.extend(b.frozen_artifacts);
                a.bypassed_nodes.extend(b.bypassed_nodes);
                a.run_override = a.run_override.or(b.run_override);
                Some(a)
            }
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    };
    let node_targets = intent
        .as_ref()
        .map(|i| i.targets.clone())
        .unwrap_or_default();
    let disabled_nodes = intent
        .as_ref()
        .map(|i| i.disabled_nodes.clone())
        .unwrap_or_default();
    let frozen_artifacts = intent
        .as_ref()
        .map(|i| i.frozen_artifacts.clone())
        .unwrap_or_default();
    let bypassed_nodes = intent
        .as_ref()
        .map(|i| i.bypassed_nodes.clone())
        .unwrap_or_default();
    let run_override = intent.as_ref().and_then(|i| i.run_override.clone());
    let force_transcribe = intent.as_ref().map(|i| i.force_transcribe).unwrap_or(false);
    // Resolved (committed or restored) at every exit point below,
    // regardless of outcome -- see `restore_or_commit_backup`.
    let backup_paths = intent
        .as_ref()
        .map(|i| i.backup_paths.clone())
        .unwrap_or_default();
    let resolve_backups = || {
        for (original, backup) in &backup_paths {
            restore_or_commit_backup(original, backup);
        }
    };

    if !node_targets.is_empty() && file_hash != initial_hash {
        // Move the pre-written transcript to the rekeyed hash so the pass can
        // patch it in place.
        let _ = std::fs::rename(
            cache.transcript_path(initial_hash),
            cache.transcript_path(file_hash),
        );
    }

    // Phase 4: real disabled_nodes are threaded through here now, not just
    // targets -- `run_analysis_plan`/`disable_analysis_node_for_run` are the
    // only callers that ever populate `disabled_nodes` for a legacy
    // special-case function (empty set, so this is behavior-preserving for
    // them). The `Err` fallback mirrors `pipeline_flags_for_targets`'s own
    // fail-open: `run_analysis_plan` already rejects an unhonorable disable
    // before it's ever queued, so this should be unreachable in practice.
    let (
        skip_transcription,
        skip_separation,
        skip_pitch,
        freeze_separation,
        freeze_pitch,
        bypass_separation,
    ) = pipeline_flags_for_request(
        &node_targets,
        &disabled_nodes,
        &frozen_artifacts,
        &bypassed_nodes,
    )
    .unwrap_or((false, false, false, false, false, false));

    // Phase 4 §4.1: the config this job actually runs with is the snapshot
    // frozen at enqueue time (`enqueue_one`/`enqueue_all`), not whatever
    // the user has changed global settings to since then.
    let config = resolve_frozen_config(file_hash, initial_hash, AppConfig::load);

    // Phase 8: the three profile-controlled knobs (separator/asr engine/
    // align backend) now actually resolve through the Global Defaults ->
    // Song Profile -> Run Override chain, instead of reading `config`
    // directly -- `get_song_analysis_profile`/`run_override` used to be
    // decorative (preview-only, see `preview_full_analysis_plan`); this is
    // the one place real execution honors them.
    let profile_global =
        crate::analysis_profile::AnalysisProfileSnapshot::from_app_config(&config, file_hash);
    let song_profile = crate::analysis_profile::get_song_analysis_profile(file_hash);
    let run_override_for = |field: crate::analysis_profile::ProfileField| {
        run_override
            .as_ref()
            .filter(|(f, _)| *f == field)
            .map(|(_, value)| value.as_str())
    };
    let effective_separator = crate::analysis_profile::resolve_profile_field(
        crate::analysis_profile::ProfileField::Separator,
        &profile_global,
        song_profile.as_ref(),
        run_override_for(crate::analysis_profile::ProfileField::Separator),
    )
    .value;
    let effective_asr_engine = crate::analysis_profile::resolve_profile_field(
        crate::analysis_profile::ProfileField::AsrEngine,
        &profile_global,
        song_profile.as_ref(),
        run_override_for(crate::analysis_profile::ProfileField::AsrEngine),
    )
    .value;
    let effective_align_backend = crate::analysis_profile::resolve_profile_field(
        crate::analysis_profile::ProfileField::AlignmentBackend,
        &profile_global,
        song_profile.as_ref(),
        run_override_for(crate::analysis_profile::ProfileField::AlignmentBackend),
    )
    .value;

    let skip_lrclib = skip_transcription || force_transcribe;
    let lyrics_path = if skip_lrclib {
        None
    } else {
        fetch_lrclib_lyrics(&song, cache)
    };

    let mut cmd_json = serde_json::json!({
        "type": "analyze",
        "audio_path": local_path.to_string_lossy(),
        "cache_path": cache.path.to_string_lossy(),
        "hash": file_hash,
        "model": config.whisper_model(),
        "beam_size": config.beam_size(),
        "batch_size": config.batch_size(),
        "separator": effective_separator,
        "separator_options": {
            "segment_size": config.separator_segment_size,
            "overlap": config.separator_overlap(),
            "batch_size": config.separator_batch_size(),
            "normalization_pct": config.separator_normalization_pct(),
            "demucs_shifts": config.demucs_shifts(),
            "demucs_overlap_pct": config.demucs_overlap_pct(),
        },
        "engine": effective_asr_engine,
        "align_backend": effective_align_backend,
        "vocal_detection_threshold_pct": config.vocal_detection_threshold_pct(),
    });

    if skip_transcription {
        cmd_json["skip_transcription"] = serde_json::json!(true);
    }
    if skip_separation {
        cmd_json["skip_separation"] = serde_json::json!(true);
    }
    if skip_pitch {
        cmd_json["skip_pitch"] = serde_json::json!(true);
    }
    if freeze_separation {
        cmd_json["freeze_separation"] = serde_json::json!(true);
    }
    if freeze_pitch {
        cmd_json["freeze_pitch"] = serde_json::json!(true);
    }
    if bypass_separation {
        cmd_json["bypass_separation_with_original_mix"] = serde_json::json!(true);
    }

    if let Some(ref lp) = lyrics_path {
        cmd_json["lyrics"] = serde_json::json!(lp.to_string_lossy());
    }
    let language_hint = config
        .language_override(file_hash)
        .map(str::to_string)
        .or_else(|| lyrics_path.as_ref().and_then(|_| song.language.clone()))
        .map(|language| normalize_analysis_language(&language))
        .filter(|lang| {
            // "unknown"/empty is not a real language: passing it as a forced
            // alignment language crashes whisperx, so let the worker detect it.
            let normalized = lang.trim().to_ascii_lowercase();
            !normalized.is_empty() && normalized != "unknown" && normalized != "und"
        });
    if let Some(lang) = language_hint {
        cmd_json["language"] = serde_json::json!(lang);
    }

    let json_str = serde_json::to_string(&cmd_json).unwrap();
    let mut retried = false;

    loop {
        let mut guard = ANALYZER_SERVER.lock().unwrap();

        if let Err(e) = ensure_server(&mut guard) {
            warn!("[analyzer] Failed to start server: {e}");
            update_queue_status(file_hash, QueuedStatus::Failed(e.to_string()));
            resolve_backups();
            return;
        }

        let server = guard.as_mut().unwrap();
        match send_and_monitor(server, &json_str, Some(file_hash)) {
            Ok(SongResult::Done) => {
                finalize_song(file_hash, cache);
                resolve_backups();
                return;
            }
            Ok(SongResult::Oom) => {
                warn!("[analyzer] CUDA OOM, killing server to free GPU memory");
                *guard = None;

                if !retried {
                    retried = true;
                    info!("[analyzer] Respawning server and retrying with clean GPU");
                    update_queue_status(file_hash, QueuedStatus::Analyzing(0));
                    continue;
                }
                update_queue_status(file_hash, QueuedStatus::Failed("CUDA out of memory".into()));
                resolve_backups();
                return;
            }
            Ok(SongResult::Error(msg)) => {
                update_queue_status(file_hash, QueuedStatus::Failed(msg));
                resolve_backups();
                return;
            }
            Err(e) => {
                warn!("[analyzer] Server crashed: {e}");
                *guard = None;

                if !retried {
                    retried = true;
                    info!("[analyzer] Respawning server and retrying");
                    update_queue_status(file_hash, QueuedStatus::Analyzing(0));
                    continue;
                }
                update_queue_status(
                    file_hash,
                    QueuedStatus::Failed(format!("Server crashed: {e}")),
                );
                resolve_backups();
                return;
            }
        }
    }
}

fn finalize_song(file_hash: &str, cache: &CacheDir) {
    if cache.transcript_exists(file_hash) {
        let meta = read_transcript_meta(cache, file_hash);
        update_song_analyzed(
            file_hash,
            true,
            meta.language,
            Some(meta.source),
            meta.key,
            meta.bpm,
            Some(meta.tempo),
        );
        if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(file_hash) {
            snapshot.stage = "complete".into();
            snapshot.stage_progress = 100;
            snapshot.operation = "Analysis complete".into();
            snapshot.detail = "All requested analysis stages completed successfully.".into();
            if let Some(route) = snapshot
                .stage_routes
                .iter_mut()
                .find(|route| route.stage == "finalizing")
            {
                route.stage_progress = 100;
                route.operation = "Analysis complete".into();
            }
        }
        finish_analysis_history(file_hash, "completed", None);
        remove_from_queue(file_hash);
        LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
        info!("[analyzer] Analysis complete for {file_hash}");
    } else {
        update_queue_status(
            file_hash,
            QueuedStatus::Failed("Transcript file not found after analysis".into()),
        );
    }
}

// ─── LRC (original-mix) preparation ─────────────────────────────────

/// Prepare an LRC-provided song authored over its original mix, without
/// routing it through the analysis status queue.
///
/// The analyzer-free work runs synchronously so the song is immediately
/// editable: resolve the local audio, ensure its content hash is current, and
/// mark the song ready (source=Lrc, no_stems). None of this touches the
/// analyzer server, so it never stalls behind a running analysis.
///
/// The musical key is then detected on a background thread (which contends on
/// the analyzer server) and patched in once it lands, so the key/tempo controls
/// unlock later without blocking authoring.
pub fn prepare_lrc_no_stems(file_hash: &str) -> Result<(), UtaStudioError> {
    let cache = CacheDir::new();
    let Some(song) = library_db::load_song_by_hash(file_hash).ok().flatten() else {
        return Err(UtaStudioError::Other("Song not found".into()));
    };

    // Resolve the local audio and rekey the row if its content hash changed so
    // all downstream cache files follow the usual layout.
    let (mut song, local_path, real_hash) = prepare_audio_for_analysis(&song, &cache)?;
    let real_hash = real_hash.to_string();

    // A rekey moves the row — carry the transcript we wrote under the original
    // hash across so the key pass can patch it in place.
    if real_hash != file_hash {
        let _ = std::fs::rename(
            cache.transcript_path(file_hash),
            cache.transcript_path(&real_hash),
        );
    }

    // Mark ready right away (key still unknown) so the original-mix chart is
    // available immediately, before key detection runs.
    song.is_analyzed = true;
    song.transcript_source = Some(TranscriptSource::Lrc);
    song.key = None;
    song.override_key = None;
    song.bpm = None;
    song.tempo = 1.0;
    song.key_offset = 0;
    song.no_stems = true;
    library_db::update_song_fields(&real_hash, &song)
        .map_err(|e| UtaStudioError::Other(e.to_string()))?;
    // Detect the key (and tempo) off-queue in the background; patch them onto
    // the row once they land so key/tempo export variants unlock without
    // blocking authoring.
    std::thread::spawn(move || {
        let cache = CacheDir::new();
        if let Err(e) = run_key_pass(&cache, &local_path, &real_hash) {
            warn!("[analyzer] LRC key detection failed for {real_hash}: {e}");
            return;
        }
        let meta = read_transcript_meta(&cache, &real_hash);
        if let Some(mut updated) = library_db::load_song_by_hash(&real_hash).ok().flatten() {
            updated.key = meta.key;
            updated.bpm = meta.bpm;
            let _ = library_db::update_song_fields(&real_hash, &updated);
        }
        info!("[analyzer] LRC key detection complete for {real_hash}");
    });
    Ok(())
}

/// Run a key-only analysis pass (no transcription, no stem separation) against
/// the running analyzer server, keeping it off the status queue. On success the
/// detected key is patched into the existing transcript by the pipeline.
fn run_key_pass(
    cache: &CacheDir,
    local_path: &Path,
    file_hash: &str,
) -> Result<(), UtaStudioError> {
    let config = AppConfig::load();
    let cmd_json = serde_json::json!({
        "type": "analyze",
        "audio_path": local_path.to_string_lossy(),
        "cache_path": cache.path.to_string_lossy(),
        "hash": file_hash,
        "model": config.whisper_model(),
        "beam_size": config.beam_size(),
        "batch_size": config.batch_size(),
        "separator": config.separator(),
        "separator_options": {
            "segment_size": config.separator_segment_size,
            "overlap": config.separator_overlap(),
            "batch_size": config.separator_batch_size(),
            "normalization_pct": config.separator_normalization_pct(),
            "demucs_shifts": config.demucs_shifts(),
            "demucs_overlap_pct": config.demucs_overlap_pct(),
        },
        "engine": config.asr_engine(),
        "align_backend": config.align_backend(),
        "vocal_detection_threshold_pct": config.vocal_detection_threshold_pct(),
        // Key only: keep the provided LRC transcript and the original mix.
        "skip_transcription": true,
        "skip_separation": true,
    });
    let json_str = serde_json::to_string(&cmd_json).unwrap();

    let mut retried = false;
    loop {
        let mut guard = ANALYZER_SERVER.lock().unwrap();
        ensure_server(&mut guard)?;
        let server = guard.as_mut().unwrap();
        // `None` progress hash keeps this off the status pipe (no queue rows).
        match send_and_monitor(server, &json_str, None) {
            Ok(SongResult::Done) => return Ok(()),
            Ok(SongResult::Oom) | Err(_) => {
                *guard = None;
                if !retried {
                    retried = true;
                    continue;
                }
                return Err(UtaStudioError::Other("key detection failed".into()));
            }
            Ok(SongResult::Error(msg)) => {
                return Err(UtaStudioError::Other(msg));
            }
        }
    }
}

// ─── Local audio preparation ─────────────────────────────────────────

fn validate_analysis_source(path: &Path) -> Result<(), UtaStudioError> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(UtaStudioError::Other(format!(
            "source media is not a file: {}",
            path.display()
        )));
    }
    if metadata.len() == 0 {
        return Err(UtaStudioError::Other(format!(
            "source media is empty: {}",
            path.display()
        )));
    }
    Ok(())
}

fn prepare_audio_for_analysis(
    song: &Song,
    _cache: &CacheDir,
) -> Result<(Song, PathBuf, String), UtaStudioError> {
    validate_analysis_source(&song.path)?;
    Ok((song.clone(), song.path.clone(), song.file_hash.clone()))
}

// ─── Server communication ────────────────────────────────────────────

enum SongResult {
    Done,
    Oom,
    Error(String),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerEvent {
    Progress {
        pct: u32,
        #[serde(default)]
        msg: String,
        #[serde(default)]
        stage: String,
        #[serde(default)]
        stage_progress: usize,
        #[serde(default)]
        operation: String,
        #[serde(default)]
        implementation: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        device: String,
        #[serde(default)]
        requested_device: String,
        #[serde(default)]
        fallback_from: Option<String>,
        #[serde(default)]
        fallback_reason: Option<String>,
        #[serde(default)]
        backend_fallback_from: Option<String>,
        #[serde(default)]
        backend_fallback_reason: Option<String>,
        #[serde(default)]
        stage_routes: Vec<AnalysisStageRoute>,
        #[serde(default)]
        node_id: Option<String>,
        #[serde(default)]
        event: Option<String>,
        #[serde(default)]
        artifact_reused_reason: Option<String>,
    },
    Done,
    Error {
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        msg: String,
    },
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod node_event_tests {
    use super::*;

    #[test]
    fn progress_event_without_node_fields_still_deserializes() {
        // Legacy Adapter contract (phase plan §3.3): an event from a
        // pipeline call site that hasn't migrated to progress_node must
        // still parse -- node_id/event/artifact_reused_reason all default
        // to None rather than failing the whole event.
        let json = r#"{"type":"progress","pct":4,"msg":"Inspecting source codec..."}"#;
        let event: ServerEvent = serde_json::from_str(json).expect("legacy event must parse");
        match event {
            ServerEvent::Progress {
                node_id,
                event,
                artifact_reused_reason,
                ..
            } => {
                assert_eq!(node_id, None);
                assert_eq!(event, None);
                assert_eq!(artifact_reused_reason, None);
            }
            _ => panic!("expected Progress event"),
        }
    }

    #[test]
    fn progress_event_with_node_fields_parses_them() {
        let json = r#"{"type":"progress","pct":52,"msg":"Extracting reference pitch...",
            "node_id":"pitch.extract","event":"node_started"}"#;
        let event: ServerEvent = serde_json::from_str(json).expect("structured event must parse");
        match event {
            ServerEvent::Progress { node_id, event, .. } => {
                assert_eq!(node_id.as_deref(), Some("pitch.extract"));
                assert_eq!(event.as_deref(), Some("node_started"));
            }
            _ => panic!("expected Progress event"),
        }
    }

    #[test]
    fn artifact_reused_event_carries_its_reason() {
        let json = r#"{"type":"progress","pct":50,"msg":"Stems already cached",
            "node_id":"stems.separate","event":"artifact_reused","artifact_reused_reason":"cache_hit"}"#;
        let event: ServerEvent = serde_json::from_str(json).expect("event must parse");
        match event {
            ServerEvent::Progress {
                artifact_reused_reason,
                ..
            } => {
                assert_eq!(artifact_reused_reason.as_deref(), Some("cache_hit"));
            }
            _ => panic!("expected Progress event"),
        }
    }

    #[test]
    fn old_history_snapshot_json_without_node_fields_still_deserializes() {
        // Simulates a snapshot_json blob written by a pre-Phase-3 build and
        // stored in analysis_history.snapshot_json. load_analysis_history
        // silently drops any row that fails to deserialize (`.ok()?`), so
        // this must keep working or old runs vanish from history.
        let old_snapshot_json = r#"{
            "stage": "pitch",
            "stage_progress": 40,
            "operation": "Reference pitch extraction",
            "detail": "Extracting reference pitch...",
            "implementation": "RMVPE",
            "model": "RMVPE singing pitch model",
            "device": "cuda",
            "requested_device": "cuda",
            "fallback_from": null,
            "fallback_reason": null,
            "backend_fallback_from": null,
            "backend_fallback_reason": null,
            "stage_routes": []
        }"#;
        let snapshot: AnalysisProgressSnapshot =
            serde_json::from_str(old_snapshot_json).expect("old snapshot json must still parse");
        assert_eq!(snapshot.node_id, None);
        assert_eq!(snapshot.node_event, None);
        assert_eq!(snapshot.artifact_reused_reason, None);
        assert_eq!(snapshot.stage, "pitch");
    }
}

fn send_and_monitor(
    server: &mut ServerProcess,
    json_cmd: &str,
    progress_hash: Option<&str>,
) -> Result<SongResult, UtaStudioError> {
    server.writer.write_all(json_cmd.as_bytes())?;
    server.writer.write_all(b"\n")?;
    server.writer.flush()?;

    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        let bytes = server.reader.read_line(&mut line_buf)?;

        if bytes == 0 {
            return Err("Server closed connection unexpectedly".into());
        }

        let line = line_buf.trim();
        if line.is_empty() {
            continue;
        }

        let event: ServerEvent = match serde_json::from_str(line) {
            Ok(ev) => ev,
            Err(e) => {
                warn!("[analyzer] Skipping unparseable event: {e}; line={line:?}");
                continue;
            }
        };

        match event {
            ServerEvent::Progress {
                pct,
                msg,
                stage,
                stage_progress,
                operation,
                implementation,
                model,
                device,
                requested_device,
                fallback_from,
                fallback_reason,
                backend_fallback_from,
                backend_fallback_reason,
                stage_routes,
                node_id,
                event,
                artifact_reused_reason,
            } => {
                if !msg.is_empty() {
                    info!("[analyzer] progress {pct}% {msg}");
                }
                if let Some(hash) = progress_hash {
                    update_live_analysis(
                        hash,
                        AnalysisProgressSnapshot {
                            stage,
                            stage_progress,
                            operation,
                            detail: msg,
                            implementation,
                            model,
                            device,
                            requested_device,
                            fallback_from,
                            fallback_reason,
                            backend_fallback_from,
                            backend_fallback_reason,
                            stage_routes,
                            node_id,
                            node_event: event,
                            artifact_reused_reason,
                        },
                    );
                    update_queue_status(hash, QueuedStatus::Analyzing(pct as usize));
                }
            }
            ServerEvent::Done => return Ok(SongResult::Done),
            ServerEvent::Error { kind, msg } => {
                let kind_s = kind.as_deref().unwrap_or("generic");
                if kind_s == "oom" {
                    return Ok(SongResult::Oom);
                }
                let msg = if msg.is_empty() {
                    "Unknown error".to_string()
                } else {
                    msg
                };
                return Ok(SongResult::Error(msg));
            }
            ServerEvent::Unknown => {
                warn!("[analyzer] Ignoring unknown event: {line}");
            }
        }
    }
}
