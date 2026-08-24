use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static ANALYSIS_LOG_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(crate) struct AnalyzerState {
    pub(crate) queue: VecDeque<String>,
    pub(crate) active_hash: Option<String>,
    pub(crate) worker_running: bool,
}

pub(crate) static ANALYZER: LazyLock<Mutex<AnalyzerState>> = LazyLock::new(|| {
    Mutex::new(AnalyzerState {
        queue: VecDeque::new(),
        active_hash: None,
        worker_running: false,
    })
});

pub(crate) static LIVE_ANALYSIS: LazyLock<Mutex<HashMap<String, AnalysisProgressSnapshot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub(crate) static ANALYSIS_STARTED: LazyLock<Mutex<HashMap<String, i64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub(crate) static STOP_REQUESTED: LazyLock<Mutex<BTreeSet<String>>> =
    LazyLock::new(|| Mutex::new(BTreeSet::new()));
pub(crate) static RETRY_ATTEMPT_ROUTES: LazyLock<Mutex<HashMap<String, Vec<AnalysisStageRoute>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(crate) fn create_analysis_log(file_hash: &str, started_at_ms: i64) -> Option<PathBuf> {
    let root = crate::cache::uta_studio_dir().join("analysis-logs");
    std::fs::create_dir_all(&root).ok()?;
    let safe_hash: String = file_hash
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .take(48)
        .collect();
    let safe_hash = if safe_hash.is_empty() {
        "unknown"
    } else {
        &safe_hash
    };
    let sequence = ANALYSIS_LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let run_id = format!("{}-{sequence}", std::process::id());
    let path = root.join(format!("{started_at_ms}-{safe_hash}-{run_id}.jsonl"));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    use std::io::Write as _;
    let record = serde_json::json!({
        "timestamp_ms": unix_time_ms(),
        "record_type": "run_requested",
        "run_id": run_id,
        "file_hash": file_hash,
    });
    let _ = serde_json::to_writer(&mut file, &record);
    let _ = writeln!(file);
    Some(path)
}

pub(crate) fn append_analysis_log_path(path: Option<&Path>, message: &str) {
    let Some(path) = path else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
        use std::io::Write as _;
        let record = serde_json::json!({
            "timestamp_ms": unix_time_ms(),
            "record_type": "native_event",
            "message": message,
        });
        let _ = serde_json::to_writer(&mut file, &record);
        let _ = writeln!(file);
    }
}

pub(crate) fn append_analysis_log_node_event(
    path: Option<&Path>,
    node_id: &str,
    event: &str,
    progress: usize,
    message: &str,
) {
    let Some(path) = path else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
        use std::io::Write as _;
        let record = serde_json::json!({
            "timestamp_ms": unix_time_ms(),
            "record_type": "node_event",
            "node_id": node_id,
            "event": event,
            "stage_progress": progress.min(100),
            "msg": message,
            "implementation": "Uta Studio native preflight",
            "requested_device": "cpu",
            "actual_device": "cpu",
        });
        let _ = serde_json::to_writer(&mut file, &record);
        let _ = writeln!(file);
    }
}

pub(crate) fn append_analysis_log_attempt(
    path: Option<&Path>,
    attempt: usize,
    reason: Option<&str>,
) {
    let Some(path) = path else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
        use std::io::Write as _;
        let record = serde_json::json!({
            "timestamp_ms": unix_time_ms(),
            "record_type": "attempt_started",
            "attempt": attempt,
            "reason": reason,
        });
        let _ = serde_json::to_writer(&mut file, &record);
        let _ = writeln!(file);
    }
}

pub(crate) fn append_analysis_log_terminal(
    path: Option<&Path>,
    status: &str,
    message: Option<&str>,
) {
    let Some(path) = path else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
        use std::io::Write as _;
        let record = serde_json::json!({
            "timestamp_ms": unix_time_ms(),
            "record_type": "history_terminal",
            "status": status,
            "message": message,
        });
        let _ = serde_json::to_writer(&mut file, &record);
        let _ = writeln!(file);
    }
}

pub(crate) fn append_analysis_artifacts(path: Option<&Path>, routes: &[AnalysisStageRoute]) {
    let Some(path) = path else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) else {
        return;
    };
    use std::io::Write as _;
    for route in routes {
        for artifact in &route.committed_outputs {
            let record = serde_json::json!({
                "timestamp_ms": unix_time_ms(),
                "record_type": "artifact_committed",
                "node_id": route.node_id,
                "artifact_kind": artifact.artifact_kind,
                "binding_kind": artifact.binding_kind,
                "path": artifact.path,
                "immutable_path": artifact.immutable_path,
                "content_hash": artifact.content_hash,
                "byte_size": artifact.byte_size,
                "capture_error": artifact.capture_error,
            });
            let _ = serde_json::to_writer(&mut file, &record);
            let _ = writeln!(file);
        }
    }
}

pub fn analysis_log_path_for(run_id: Option<i64>, file_hash: &str) -> Option<PathBuf> {
    if let Some(run_id) = run_id {
        return load_analysis_history(500)
            .into_iter()
            .find(|run| run.id == run_id && run.file_hash == file_hash)
            .and_then(|run| run.log_path.or(run.snapshot.analysis_log_path));
    }
    LIVE_ANALYSIS
        .lock()
        .unwrap()
        .get(file_hash)
        .and_then(|snapshot| snapshot.analysis_log_path.clone())
}

pub fn analysis_log_lines(
    run_id: Option<i64>,
    file_hash: &str,
    node_id: Option<&str>,
    limit: usize,
) -> Vec<crate::applog::LogLine> {
    let Some(path) = analysis_log_path_for(run_id, file_hash) else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut lines: Vec<crate::applog::LogLine> = contents
        .lines()
        .filter(|line| analysis_log_line_matches_node(line, node_id))
        .map(|line| {
            let timestamp_ms = serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|value| value.get("timestamp_ms").and_then(|value| value.as_i64()))
                .or_else(|| {
                    line.split_once(' ')
                        .and_then(|(value, _)| value.parse().ok())
                })
                .unwrap_or(0);
            crate::applog::LogLine {
                timestamp_ms,
                text: line.to_string(),
            }
        })
        .collect();
    let keep = limit.clamp(1, 2_000);
    if lines.len() > keep {
        lines.drain(..lines.len() - keep);
    }
    lines
}

pub(crate) fn analysis_log_line_matches_node(line: &str, node_id: Option<&str>) -> bool {
    let Some(node_id) = node_id else {
        return true;
    };
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
        return value.get("node_id").and_then(|value| value.as_str()) == Some(node_id)
            || !matches!(
                value.get("record_type").and_then(|value| value.as_str()),
                Some("node_event" | "process_output")
            );
    }
    line.contains(&format!("node={node_id}")) || !line.contains("[progress]")
}

/// Maps a route's last recorded structured event kind
/// (`AnalysisStageRoute.node_event`) to an `analysis_node_attempts.status`
/// value, independent of the enclosing run's overall status -- a node that
/// finished successfully earlier in a run that later failed at a different
/// node must not be reported as failed too. Unset/unrecognized events (a
/// node that was targeted but never reached a terminal event before the
/// run itself ended, e.g. the run failed at an earlier node first) map to
/// "incomplete".
pub(crate) fn node_attempt_status(node_event: Option<&str>) -> &'static str {
    match node_event {
        Some("completed" | "node_completed") => "succeeded",
        Some("failed" | "node_failed") => "failed",
        Some("reused" | "artifact_reused") => "reused",
        Some("skipped" | "node_skipped") => "bypassed",
        Some("cancelled" | "node_cancelled") => "cancelled",
        _ => "incomplete",
    }
}

pub(crate) fn take_stop_requested(file_hash: &str) -> bool {
    STOP_REQUESTED.lock().unwrap().remove(file_hash)
}

pub fn analysis_stop_requested(file_hash: &str) -> bool {
    STOP_REQUESTED.lock().unwrap().contains(file_hash)
}

pub(crate) fn node_attempt_status_for_route(route: &AnalysisStageRoute) -> &'static str {
    match route.binding_kind.as_deref() {
        Some("frozen") => "frozen",
        Some("bypassed") => "bypassed",
        _ => node_attempt_status(route.node_event.as_deref()),
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
pub(crate) fn record_node_attempts(
    run_id: i64,
    file_hash: &str,
    snapshot: &AnalysisProgressSnapshot,
) {
    let previous_routes = RETRY_ATTEMPT_ROUTES
        .lock()
        .unwrap()
        .remove(file_hash)
        .unwrap_or_default();
    let attempts: Vec<library_db::NewAnalysisNodeAttempt> = previous_routes
        .iter()
        .chain(snapshot.stage_routes.iter())
        .filter_map(|route| {
            let node_id = route.node_id.as_deref()?;
            Some(library_db::NewAnalysisNodeAttempt {
                node_id,
                status: node_attempt_status_for_route(route),
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

pub(crate) fn finish_analysis_history(file_hash: &str, status: &str, error_message: Option<&str>) {
    let Some(started_at_ms) = ANALYSIS_STARTED.lock().unwrap().remove(file_hash) else {
        return;
    };
    let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get(file_hash).cloned() else {
        return;
    };
    let song = library_db::load_song_by_hash(file_hash).ok().flatten();
    let title = song
        .as_ref()
        .map(|song| song.title.as_str())
        .unwrap_or(file_hash);
    let artist = song
        .as_ref()
        .map(|song| song.artist.as_str())
        .unwrap_or("Unknown artist");
    append_analysis_log_terminal(snapshot.analysis_log_path.as_deref(), status, error_message);
    let Ok(snapshot_json) = serde_json::to_string(&snapshot) else {
        return;
    };
    let Ok(run_id) = library_db::analysis_history_insert(&library_db::NewAnalysisHistory {
        file_hash,
        title,
        artist,
        status,
        started_at_ms,
        finished_at_ms: unix_time_ms(),
        snapshot_json: &snapshot_json,
        error_message,
        log_path: snapshot.analysis_log_path.as_deref(),
    }) else {
        return;
    };
    record_node_attempts(run_id, file_hash, &snapshot);
    match crate::artifact_workbench::capture_analysis_run_artifacts(run_id, file_hash) {
        Ok(()) => {
            let captured_preprocessed_audio = snapshot.stage_routes.iter().any(|route| {
                route.committed_outputs.iter().any(|output| {
                    output.artifact_kind == "PreprocessedAudio"
                        && output.content_hash.is_some()
                        && output.capture_error.is_none()
                })
            });
            if captured_preprocessed_audio
                && let Some(request) = ACTIVE_CAPTURE_REQUESTS.lock().unwrap().remove(file_hash)
                && !request.persistent
            {
                let mut disabled = request;
                disabled.enabled = false;
                let _ = crate::artifact_workbench::set_intermediate_capture_request(&disabled);
            }
        }
        Err(error) => {
            let message = format!("Artifact lineage recording failed after output commit: {error}");
            append_analysis_log_path(snapshot.analysis_log_path.as_deref(), &message);
            let _ = library_db::analysis_history_set_error(run_id, &message);
        }
    }
}

pub(crate) fn update_live_analysis(file_hash: &str, snapshot: AnalysisProgressSnapshot) {
    let mut live = LIVE_ANALYSIS.lock().unwrap();
    let mut snapshot = snapshot;
    // A progress message is never the durable success boundary. Only
    // `finalize_song`, after committed outputs have been checked, may set
    // the run to 100%.
    snapshot.overall_progress = snapshot.overall_progress.min(99);
    if let Some(previous) = live.get(file_hash) {
        snapshot.overall_progress = snapshot
            .overall_progress
            .max(previous.overall_progress.min(99));
        if snapshot.analysis_log_path.is_none() {
            snapshot.analysis_log_path = previous.analysis_log_path.clone();
        }
        let current_nodes: BTreeSet<_> = snapshot
            .stage_routes
            .iter()
            .filter_map(|route| route.node_id.clone())
            .collect();
        let mut retained: Vec<_> = previous
            .stage_routes
            .iter()
            .filter(|route| {
                route
                    .node_id
                    .as_deref()
                    .is_none_or(|node_id| !current_nodes.contains(node_id))
            })
            .cloned()
            .collect();
        retained.extend(snapshot.stage_routes);
        snapshot.stage_routes = retained;
    }
    live.insert(file_hash.to_string(), snapshot);
}

pub(crate) fn preserve_retry_attempt(file_hash: &str) {
    let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get(file_hash).cloned() else {
        return;
    };
    RETRY_ATTEMPT_ROUTES
        .lock()
        .unwrap()
        .entry(file_hash.to_string())
        .or_default()
        .extend(snapshot.stage_routes);
}

pub(crate) fn capture_committed_outputs(file_hash: &str, routes: &mut [AnalysisStageRoute]) {
    let cache = CacheDir::new();
    capture_committed_outputs_in(&cache, file_hash, routes);
}

pub(crate) fn capture_committed_outputs_in(
    cache: &CacheDir,
    file_hash: &str,
    routes: &mut [AnalysisStageRoute],
) {
    let store = match crate::analysis_artifact::ArtifactStore::new(&cache.path) {
        Ok(store) => store,
        Err(error) => {
            for output in routes
                .iter_mut()
                .flat_map(|route| route.committed_outputs.iter_mut())
            {
                output.capture_error.get_or_insert_with(|| error.clone());
            }
            return;
        }
    };
    let graph = crate::analysis_graph::baseline_graph_spec();
    let mut latest = BTreeMap::<crate::analysis_graph::ArtifactKind, String>::new();
    for revision in crate::analysis_artifact::load_analysis_artifacts(file_hash)
        .into_iter()
        .filter(|revision| !revision.invalidated)
    {
        if revision.active {
            latest.insert(revision.kind, revision.id);
        } else {
            latest.entry(revision.kind).or_insert(revision.id);
        }
    }
    for route in routes {
        if route.input_revision_ids.is_empty()
            && let Some(node_id) = route.node_id.as_deref()
            && let Some(spec) = graph.node(&crate::analysis_graph::AnalysisNodeId::new(node_id))
        {
            route.input_revision_ids = spec
                .inputs
                .iter()
                .map(|kind| latest.get(kind).cloned())
                .collect();
        }
        for output in &mut route.committed_outputs {
            let kind = serde_json::from_value::<crate::analysis_graph::ArtifactKind>(
                serde_json::Value::String(output.artifact_kind.clone()),
            );
            let Ok(kind) = kind else {
                output.capture_error = Some("unknown artifact kind in commit event".to_string());
                continue;
            };
            if kind == crate::analysis_graph::ArtifactKind::CandidateChart
                && output.content_hash.is_none()
                && output.capture_error.is_none()
            {
                match crate::chart::materialize_candidate_chart(cache, file_hash, &output.path) {
                    Ok(path) => output.path = path,
                    Err(error) => {
                        output.capture_error =
                            Some(format!("candidate chart materialization failed: {error}"));
                    }
                }
            }
            if output.content_hash.is_none() && output.capture_error.is_none() {
                match store.capture(file_hash, kind, &output.path) {
                    Ok((path, hash, byte_size)) => {
                        output.immutable_path = Some(path);
                        output.content_hash = Some(hash);
                        output.byte_size = Some(byte_size);
                        if kind == crate::analysis_graph::ArtifactKind::PreprocessedAudio {
                            let _ = std::fs::remove_file(&output.path);
                        }
                    }
                    Err(error) => output.capture_error = Some(error),
                }
            }
            if let Some(hash) = output.content_hash.as_deref() {
                let kind_name =
                    serde_json::to_string(&kind).unwrap_or_else(|_| format!("{kind:?}"));
                latest.insert(kind, format!("{file_hash}:{kind_name}:{hash}"));
            }
        }
    }
}

/// Per-hash node targeting for the *next* queued run, replacing the old
/// `FORCE_TRANSCRIBE`/`STEMS_ONLY`/`PITCH_ONLY` `HashSet<String>` trio
/// (analysis DAG redesign Phase 4, phase plan §4). `targets`/`disabled_nodes`
/// are resolved into `skip_transcription`/`skip_separation`/`skip_pitch`
/// booleans through `analysis_plan::build_plan`
/// (`pipeline_flags_for_request` below) instead of three independent
/// boolean special cases -- the native worker protocol is unchanged; only how
/// Rust decides those booleans changed.
#[derive(Debug, Clone, Default)]
pub(crate) struct PendingNodeIntent {
    pub(crate) targets: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    /// Distinct from `targets`: this is an input-source override ("don't
    /// try LRCLIB, go straight to ASR"), not a node-skip decision, so it
    /// doesn't belong in the Planner's targeting closure.
    pub(crate) force_transcribe: bool,
    /// (original_path, backup_path) pairs from a reset that renamed old
    /// output aside instead of deleting it outright, resolved by
    /// `restore_or_commit_backup` once `process_song` learns how the
    /// triggered run finished (docs/analysis-dag-redesign.md Phase 5, phase
    /// plan §9.2 "失败时保留旧 Pitch" -- deleting eagerly, before the rerun
    /// is even queued, meant a failed/crashed/OOM-killed rerun destroyed
    /// the previous good output for nothing).
    pub(crate) backup_paths: Vec<(PathBuf, PathBuf)>,
    /// Phase 4's generic executor gap closer: nodes to actually turn off for
    /// this one run, set by `run_analysis_plan` (and, through it,
    /// `disable_analysis_node_for_run`). Empty for every legacy special-case
    /// function (`reanalyze_pitch`, `mark_stems_only`, `realign`, ...) --
    /// they only ever add to `targets`, never disable anything.
    pub(crate) disabled_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    /// Phase 4 §4.5's Freeze consumer: artifact kinds whose *current*
    /// on-disk output must be force-reused for this run even if the active
    /// config would otherwise consider them stale (different separator
    /// options, etc.) -- set only by `freeze_analysis_node_outputs_for_run`.
    /// Empty for every other caller, including every legacy special-case
    /// function. Distinct from ordinary cache-hit reuse (which already
    /// happens for free when nothing changed): Freeze exists specifically
    /// to keep old output even though current settings would produce
    /// something different.
    pub(crate) frozen_artifacts: BTreeSet<crate::analysis_graph::ArtifactKind>,
    /// Phase 4 §4.5's Bypass consumer: nodes to route around using their
    /// designated alternate input for this run (today: only
    /// `stems.separate`, bypassed with the Original Mix) -- set only by
    /// `bypass_analysis_node_with_original_mix_for_run`. Empty for every
    /// other caller.
    pub(crate) bypassed_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    /// Phase 8's Run tier: a one-run-only override for a single
    /// profile-controlled field, set only by `configure_analysis_node_for_run`.
    /// `None` for every legacy special-case caller. Drained the same
    /// one-shot way as every other field here -- real precisely because it
    /// only ever applies to the one run it was set for.
    pub(crate) run_override: Option<(crate::analysis_profile::ProfileField, String)>,
    /// Explicit retention request frozen when the job joins the queue.
    /// `None` keeps preprocessing ephemeral and preserves ordinary-run
    /// storage behavior.
    pub(crate) capture_intermediate: Option<crate::artifact_workbench::CaptureIntermediateRequest>,
    pub(crate) workflow_execution: Option<crate::workflow::WorkflowExecutionSnapshot>,
}

pub(crate) static PENDING_NODE_INTENTS: LazyLock<Mutex<HashMap<String, PendingNodeIntent>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Snapshot of intents already staged for the next queued run. Impact preview
/// and confirmation read this instead of inventing a second plan source.
#[derive(Debug, Clone, Default)]
pub struct PendingAnalysisIntent {
    pub targets: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    pub disabled_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    pub frozen_artifacts: BTreeSet<crate::analysis_graph::ArtifactKind>,
    pub bypassed_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
}

pub fn pending_analysis_intent(file_hash: &str) -> PendingAnalysisIntent {
    PENDING_NODE_INTENTS
        .lock()
        .unwrap()
        .get(file_hash)
        .map(|intent| PendingAnalysisIntent {
            targets: intent.targets.clone(),
            disabled_nodes: intent.disabled_nodes.clone(),
            frozen_artifacts: intent.frozen_artifacts.clone(),
            bypassed_nodes: intent.bypassed_nodes.clone(),
        })
        .unwrap_or_default()
}

pub fn frozen_artifact_kinds_for_node_id(
    node_id: &str,
) -> BTreeSet<crate::analysis_graph::ArtifactKind> {
    frozen_artifact_kinds_for_node(&crate::analysis_graph::AnalysisNodeId::new(node_id))
}

pub fn downstream_node_ids(node_id: &str) -> BTreeSet<crate::analysis_graph::AnalysisNodeId> {
    downstream_closure(
        &crate::analysis_graph::baseline_graph_spec(),
        &crate::analysis_graph::AnalysisNodeId::new(node_id),
    )
}

/// Phase 4 §4.1 "Enqueue 时冻结配置": the config snapshot a queued job will
/// run with, captured the moment it actually joins the queue (not a fresh
/// `AppConfig::load()` at execution time). Without this, changing global
/// separator/model/device settings while a job sits in the queue -- not yet
/// started -- silently changed what that already-queued job would run
/// with, contradicting "全局设置在任务排队后变化，只影响之后新建的任务". Drained
/// (removed) by `process_song` when the job actually starts, so this map
/// only ever holds entries for jobs that are queued but not yet running.
pub(crate) static FROZEN_CONFIGS: LazyLock<Mutex<HashMap<String, AppConfig>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) static ACTIVE_CAPTURE_REQUESTS: LazyLock<
    Mutex<HashMap<String, crate::artifact_workbench::CaptureIntermediateRequest>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn freeze_intermediate_capture_request(file_hash: &str) {
    let Ok(request) = crate::artifact_workbench::intermediate_capture_request(file_hash) else {
        return;
    };
    if let Some(request) = request {
        PENDING_NODE_INTENTS
            .lock()
            .unwrap()
            .entry(file_hash.to_string())
            .or_default()
            .capture_intermediate = Some(request);
    }
}

/// Resolves (and drains) the config a job actually runs with: the snapshot
/// frozen at enqueue time if one exists -- checked under both the current
/// and pre-rekey hash, since a remote song's hash can change between
/// enqueue and this point -- or `fallback` for a job with no frozen entry
/// (e.g. one enqueued by an older build mid-upgrade, so this can never
/// panic or block a run outright).
pub(crate) fn resolve_frozen_config(
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
fn configured_audio_processing() -> (
    crate::audio_processing::AudioProcessingPlanSnapshot,
    BTreeSet<crate::analysis_graph::AnalysisNodeId>,
) {
    use crate::audio_processing::AudioProcessingSettings;

    let config = crate::config::AppConfig::load();
    let settings = config
        .audio_processing
        .clone()
        .unwrap_or_else(|| AudioProcessingSettings::from_legacy_separator(config.separator()));
    let active = crate::analysis_graph::active_stem_nodes_from_settings(&settings);
    (
        crate::audio_processing::AudioProcessingPlanSnapshot::from_settings(&settings),
        active,
    )
}

pub(crate) fn build_execution_plan(
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
    let (audio_processing, active_stem_nodes) = configured_audio_processing();
    let request = AnalysisRequest {
        file_hash: String::new(),
        targets: effective_targets,
        disabled_nodes: disabled_nodes.clone(),
        frozen_artifacts: frozen_artifacts.clone(),
        bypassed_nodes: bypassed_nodes.clone(),
        lyrics_route: LyricsRoute::GeneratedLyrics,
        model_availability: BTreeMap::new(),
        profile_snapshot: AnalysisProfileSnapshot::default(),
        active_stem_nodes,
        audio_processing: Some(audio_processing),
        workflow_execution: None,
    };
    build_plan(&graph, &request)
}

/// Reads the three pipeline-honorable booleans off an already-built plan:
/// which of lyrics/stems/pitch actually needs to run. `run_pipeline` has no
/// finer-grained hook than these three today (docs/analysis-dag-redesign.md
/// Phase 4 status note) -- every other node (`music.*`, `preflight`,
/// `chart.build_candidate`) is computed unconditionally regardless of what
/// the plan says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PipelineFlags {
    pub(crate) skip_transcription: bool,
    pub(crate) skip_separation: bool,
    pub(crate) skip_pitch: bool,
    pub(crate) freeze_separation: bool,
    pub(crate) freeze_pitch: bool,
    pub(crate) bypass_separation: bool,
}

pub(crate) fn pipeline_flags_from_plan(plan: &crate::analysis_plan::AnalysisPlan) -> PipelineFlags {
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
    let freeze_separation = node_state_is("stems.separate", NodeState::Frozen)
        || node_state_is("stems.bind_analysis_outputs", NodeState::Frozen);
    let freeze_pitch = node_state_is("pitch.extract", NodeState::Frozen);
    let bypass_separation = node_state_is("stems.separate", NodeState::Bypassed)
        || node_state_is("stems.bind_analysis_outputs", NodeState::Bypassed);
    // A Frozen node must still be "run" from the pipeline's point of view --
    // it needs to hand its cached output (the vocals path, for stems) to
    // whatever downstream node actually executes this run. A Bypassed node
    // is the opposite: it genuinely does not run (no separation call at
    // all) -- `pipeline.py` substitutes the Original Mix as the vocals path
    // instead, which is exactly what `skip_separation` already models
    // (`vocals_path` stays unset by the separation call itself), so no
    // extra exemption is needed here for it the way Frozen needs one.
    let separation_will_run = will_run("stems.separate")
        || will_run("stems.vocals")
        || will_run("stems.bind_analysis_outputs")
        || will_run("stems.instrumental")
        || will_run("instrumental.denoise")
        || will_run("instrumental.dereverb")
        || will_run("stems.multistem");
    let skip_separation = !separation_will_run && !freeze_separation;
    let skip_pitch = !will_run("pitch.extract") && !freeze_pitch;
    PipelineFlags {
        skip_transcription: !lyrics_ran,
        skip_separation,
        skip_pitch,
        freeze_separation,
        freeze_pitch,
        bypass_separation,
    }
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
pub(crate) fn pipeline_flags_for_request(
    targets: &BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    disabled_nodes: &BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    frozen_artifacts: &BTreeSet<crate::analysis_graph::ArtifactKind>,
    bypassed_nodes: &BTreeSet<crate::analysis_graph::AnalysisNodeId>,
) -> Result<PipelineFlags, Vec<crate::analysis_plan::PlanWarning>> {
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
/// silently have no effect on what native worker actually runs, so
/// `run_analysis_plan` rejects it up front instead.
pub(crate) fn pipeline_can_honor_disable(id: &crate::analysis_graph::AnalysisNodeId) -> bool {
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
/// if pitch ever grows parameterized cache invalidation. Lyrics outputs are
/// now split into typed artifacts, but the native pipeline still has no
/// independent freeze control for those stages, so this predicate remains
/// deliberately narrower than the artifact schema.
pub(crate) fn pipeline_can_honor_freeze(id: &crate::analysis_graph::AnalysisNodeId) -> bool {
    matches!(id.as_str(), "stems.separate" | "pitch.extract")
}

/// Maps a freezable node id to the `ArtifactKind`s its output is made of,
/// for populating `AnalysisRequest.frozen_artifacts` (which the Phase 1
/// planner keys by kind, not node id).
pub(crate) fn frozen_artifact_kinds_for_node(
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
pub(crate) fn node_output_exists_for_freeze(
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
    let mut request = preview_analysis_request_for(file_hash, disabled_nodes);
    if !targets.is_empty() {
        request.targets = targets;
    }
    run_analysis_request(request)
}

/// Queue the exact request an impact preview already classified. Confirmation
/// uses this instead of reconstructing targets from a node id.
pub fn run_analysis_request(request: crate::analysis_plan::AnalysisRequest) -> Result<(), String> {
    if is_usdx_song(&request.file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }
    for id in &request.disabled_nodes {
        if !pipeline_can_honor_disable(id) {
            return Err(format!(
                "{id} cannot be disabled for a single run yet -- it is computed together with sibling nodes that always run"
            ));
        }
    }
    let execution_graph = request
        .workflow_execution
        .as_ref()
        .map(|snapshot| snapshot.graph.clone())
        .unwrap_or_else(crate::analysis_graph::baseline_graph_spec);
    let plan = crate::analysis_plan::build_plan(&execution_graph, &request)
        .map_err(|error| error.to_string())?;
    if let Some(warning) = plan.warnings.first() {
        return Err(format!("{}: {}", warning.node, warning.message));
    }
    if request.workflow_execution.is_none()
        && let Err(warnings) = pipeline_flags_for_request(
            &request.targets,
            &request.disabled_nodes,
            &request.frozen_artifacts,
            &request.bypassed_nodes,
        )
    {
        return Err(match warnings.first() {
            Some(warning) => format!("{}: {}", warning.node, warning.message),
            None => "invalid analysis request: unknown or not-applicable target node".to_string(),
        });
    }
    {
        let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
        let intent = intents.entry(request.file_hash.clone()).or_default();
        intent.targets.extend(request.targets);
        intent.disabled_nodes.extend(request.disabled_nodes);
        intent.frozen_artifacts.extend(request.frozen_artifacts);
        intent.bypassed_nodes.extend(request.bypassed_nodes);
        intent.workflow_execution = request.workflow_execution;
    }
    enqueue_one(&request.file_hash);
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
pub(crate) fn downstream_closure(
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
        // stems.separate is the MINI/compat shell; its real charting
        // output is bind. Walk from bind so "run this node and downstream"
        // still covers pitch and lyrics.
        if current.as_str() == "stems.separate" {
            stack.push(crate::analysis_graph::AnalysisNodeId::new(
                "stems.bind_analysis_outputs",
            ));
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
pub(crate) fn pipeline_can_honor_bypass(id: &crate::analysis_graph::AnalysisNodeId) -> bool {
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
pub(crate) fn node_config_field_for(
    node_id: &str,
) -> Option<crate::analysis_profile::ProfileField> {
    use crate::analysis_profile::ProfileField;
    match node_id {
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
/// always `GeneratedLyrics`, matching every other existing call site's
/// placeholder (`build_execution_plan`) -- no code path anywhere lets a
/// user pick a route today.
pub(crate) fn preview_analysis_request_for(
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
    let mut model_availability = crate::vendor::node_model_availability_for(&availability_params);
    let pending = pending_analysis_intent(file_hash);
    let config = AppConfig::load();
    let audio_settings = config.audio_processing.clone().unwrap_or_else(|| {
        crate::audio_processing::AudioProcessingSettings::from_legacy_separator(
            &profile_snapshot.separator,
        )
    });
    let audio_processing =
        crate::audio_processing::AudioProcessingPlanSnapshot::from_settings(&audio_settings);
    let mut all_audio_models_ready = true;
    for step in &audio_processing.steps {
        let ready = crate::audio_processing::audio_model_is_usable(&step.model_id).unwrap_or(false);
        all_audio_models_ready &= ready;
        model_availability.insert(
            crate::analysis_graph::analysis_node_for_audio_step(&step.step_id),
            ready,
        );
    }
    model_availability.insert(
        crate::analysis_graph::AnalysisNodeId::new("stems.separate"),
        all_audio_models_ready,
    );
    let active_stem_nodes = crate::analysis_graph::active_stem_nodes_from_settings(&audio_settings);
    let mut disabled_nodes = disabled_nodes;
    disabled_nodes.extend(pending.disabled_nodes);
    AnalysisRequest {
        file_hash: file_hash.to_string(),
        targets: BTreeSet::from([AnalysisNodeId::new("chart.build_candidate")]),
        disabled_nodes,
        frozen_artifacts: pending.frozen_artifacts,
        bypassed_nodes: pending.bypassed_nodes,
        lyrics_route: LyricsRoute::GeneratedLyrics,
        model_availability,
        profile_snapshot,
        active_stem_nodes,
        audio_processing: Some(audio_processing),
        workflow_execution: None,
    }
}

pub(crate) fn analysis_request_snapshot(
    file_hash: &str,
    targets: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    disabled_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
    frozen_artifacts: BTreeSet<crate::analysis_graph::ArtifactKind>,
    bypassed_nodes: BTreeSet<crate::analysis_graph::AnalysisNodeId>,
) -> crate::analysis_plan::AnalysisRequest {
    let mut request = preview_analysis_request_for(file_hash, disabled_nodes);
    if !targets.is_empty() {
        request.targets = targets;
    }
    request.frozen_artifacts.extend(frozen_artifacts);
    request.bypassed_nodes.extend(bypassed_nodes);
    request
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

pub(crate) fn update_queue_status(file_hash: &str, status: QueuedStatus) {
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

pub(crate) fn remove_from_queue(file_hash: &str) {
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

pub(crate) fn ensure_worker_running(state: &mut AnalyzerState) {
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

pub(crate) fn enqueue_engine_intent(
    intent: &crate::library_db::EngineQueueIntent,
) -> Result<(), String> {
    let mut state = ANALYZER.lock().unwrap();
    if state.active_hash.as_deref() == Some(&intent.file_hash)
        || state.queue.iter().any(|hash| hash == &intent.file_hash)
    {
        return Err("this song already has a queued or running analysis".to_string());
    }
    let persisted = crate::library_db::analysis_queue_set_engine_intent(intent)
        .map_err(|error| error.to_string())?;
    if !persisted {
        return Err("this song already has a queued or running analysis".to_string());
    }
    state.queue.push_back(intent.file_hash.clone());
    ensure_worker_running(&mut state);
    Ok(())
}

pub(crate) fn resume_engine_intent(file_hash: &str) {
    let mut state = ANALYZER.lock().unwrap();
    if state.active_hash.as_deref() == Some(file_hash)
        || state.queue.iter().any(|hash| hash == file_hash)
    {
        return;
    }
    state.queue.push_back(file_hash.to_string());
    update_queue_status(file_hash, QueuedStatus::Queued);
    ensure_worker_running(&mut state);
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
        freeze_intermediate_capture_request(file_hash);
        state.queue.push_back(file_hash.to_string());
        update_queue_status(file_hash, QueuedStatus::Queued);
        FROZEN_CONFIGS
            .lock()
            .unwrap()
            .insert(file_hash.to_string(), AppConfig::load());
    }
    ensure_worker_running(&mut state);
}

pub(crate) fn queue_entry_blocks_enqueue(status: Option<&QueuedStatus>) -> bool {
    matches!(
        status,
        Some(QueuedStatus::Queued | QueuedStatus::Analyzing(_))
    )
}

/// Phase 6 `cancel_analysis_run`. Deliberately scoped to the *queued, not
/// yet started* case only: the single background worker thread
/// (`spawn_worker`) runs `process_song` synchronously against a live native worker
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

/// Stops either a queued or active run. Active work is terminated through
/// the child handle rather than the server mutex, which is intentionally
/// held by the monitoring worker for the duration of a model call.
pub fn stop_analysis_run(file_hash: &str) -> Result<(), String> {
    let state = ANALYZER.lock().unwrap();
    let active = state.active_hash.as_deref() == Some(file_hash);
    let queued = state.queue.iter().any(|hash| hash == file_hash);
    drop(state);
    if !active {
        return if queued {
            cancel_analysis_run(file_hash)
        } else {
            Err(format!("{file_hash} is not currently queued or running"))
        };
    }
    if let Some(result) = super::engine_run::cancel_active_engine(file_hash) {
        return result;
    }
    STOP_REQUESTED.lock().unwrap().insert(file_hash.to_string());
    let result = ACTIVE_SERVER_CHILD
        .lock()
        .unwrap()
        .as_ref()
        .cloned()
        .ok_or_else(|| "the analyzer process is not available".to_string())
        .and_then(|child| {
            child
                .lock()
                .map_err(|_| "the analyzer process lock is unavailable".to_string())?
                .kill()
                .map_err(|error| format!("could not stop analyzer process: {error}"))
        });
    if result.is_err() {
        STOP_REQUESTED.lock().unwrap().remove(file_hash);
    }
    result
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
            freeze_intermediate_capture_request(&file_hash);
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
