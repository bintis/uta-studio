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

pub(crate) fn append_analysis_lifecycle_log(
    path: Option<&Path>,
    event: &crate::backend_cli::AnalysisLifecycleFrameWireV1,
) {
    let Some(path) = path else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
        use std::io::Write as _;
        let mut record = serde_json::to_value(event).unwrap_or_default();
        record["record_type"] = serde_json::json!("engine_lifecycle");
        let _ = serde_json::to_writer(&mut file, &record);
        let _ = writeln!(file);
    }
}

pub(crate) fn append_analysis_log_path(path: Option<&Path>, message: &str) {
    let Some(path) = path else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
        use std::io::Write as _;
        let record = serde_json::json!({
            "timestamp_ms": unix_time_ms(),
            "record_type": "engine_event",
            "message": message,
        });
        let _ = serde_json::to_writer(&mut file, &record);
        let _ = writeln!(file);
    }
}

fn append_analysis_log_terminal(path: Option<&Path>, status: &str, message: Option<&str>) {
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

fn analysis_log_line_matches_node(line: &str, node_id: Option<&str>) -> bool {
    let Some(node_id) = node_id else {
        return true;
    };
    serde_json::from_str::<serde_json::Value>(line).map_or(true, |value| {
        value.get("node_id").and_then(|value| value.as_str()) == Some(node_id)
            || value
                .get("presentation_node_id")
                .and_then(|value| value.as_str())
                == Some(node_id)
            || (value.get("node_id").is_none() && value.get("presentation_node_id").is_none())
    })
}

fn node_attempt_status(route: &AnalysisStageRoute) -> &'static str {
    match route.binding_kind.as_deref() {
        Some("frozen") => "frozen",
        Some("bypassed") => "bypassed",
        _ => match route.node_event.as_deref() {
            Some("completed" | "node_completed") => "succeeded",
            Some("failed" | "node_failed") => "failed",
            Some("reused" | "artifact_reused") => "reused",
            Some("skipped" | "node_skipped") => "bypassed",
            Some("cancelled" | "node_cancelled") => "cancelled",
            _ => "incomplete",
        },
    }
}

fn record_node_attempts(run_id: i64, file_hash: &str, snapshot: &AnalysisProgressSnapshot) {
    let attempts = snapshot
        .stage_routes
        .iter()
        .filter_map(|route| {
            let node_id = route.node_id.as_deref()?;
            Some(library_db::NewAnalysisNodeAttempt {
                node_id,
                status: node_attempt_status(route),
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
        .collect::<Vec<_>>();
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
    if let Err(error) = crate::artifact_workbench::capture_analysis_run_artifacts(run_id, file_hash)
    {
        let message = format!("Artifact lineage recording failed after output commit: {error}");
        append_analysis_log_path(snapshot.analysis_log_path.as_deref(), &message);
        let _ = library_db::analysis_history_set_error(run_id, &message);
    }
}

pub(crate) fn update_queue_status(file_hash: &str, status: QueuedStatus) {
    let (state, progress, message) = match &status {
        QueuedStatus::Staged => ("staged", None, None::<String>),
        QueuedStatus::Queued => ("queued", None, None::<String>),
        QueuedStatus::Analyzing(progress) => ("analyzing", Some(*progress as i64), None),
        QueuedStatus::Failed(message) => ("failed", None, Some(message.clone())),
    };
    let _ = library_db::analysis_queue_upsert_row(file_hash, state, progress, message.as_deref());
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

pub(crate) fn is_usdx_song(file_hash: &str) -> bool {
    library_db::load_song_by_hash(file_hash)
        .ok()
        .flatten()
        .is_some_and(|song| song.usdx.is_some())
}

pub(crate) fn stage_engine_intent(
    intent: &crate::library_db::EngineQueueIntent,
) -> Result<(), String> {
    let state = ANALYZER.lock().unwrap();
    if state.active_hash.as_deref() == Some(&intent.file_hash)
        || state.queue.iter().any(|hash| hash == &intent.file_hash)
    {
        return Err("this song already has a queued or running analysis".to_string());
    }
    let persisted = crate::library_db::analysis_queue_stage_engine_intent(intent)
        .map_err(|error| error.to_string())?;
    if !persisted {
        return Err("this song already has a queued or running analysis".to_string());
    }
    Ok(())
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

pub fn start_queued_analysis(file_hash: &str) -> Result<(), String> {
    let status =
        crate::library_db::analysis_queue_status(file_hash).map_err(|error| error.to_string())?;
    if status.as_deref() != Some("staged") {
        return Err("this analysis is not waiting for a manual start".to_string());
    }
    if crate::library_db::analysis_queue_engine_intent(file_hash)
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Err("the queued analysis has no exact Engine request".to_string());
    }
    update_queue_status(file_hash, QueuedStatus::Queued);
    resume_engine_intent(file_hash);
    Ok(())
}

pub fn enqueue_one(file_hash: &str) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }
    crate::analysis_engine_adapter::preview_and_queue_engine_run(file_hash, None).map(|_| ())
}

fn queue_entry_blocks_enqueue(status: Option<&QueuedStatus>) -> bool {
    matches!(
        status,
        Some(QueuedStatus::Staged | QueuedStatus::Queued | QueuedStatus::Analyzing(_))
    )
}

pub fn cancel_analysis_run(file_hash: &str) -> Result<(), String> {
    let mut state = ANALYZER.lock().unwrap();
    if state.active_hash.as_deref() == Some(file_hash) {
        return Err("this song is already running; use stop_analysis_run".to_string());
    }
    let before = state.queue.len();
    state.queue.retain(|hash| hash != file_hash);
    if state.queue.len() == before {
        drop(state);
        if crate::library_db::analysis_queue_status(file_hash)
            .map_err(|error| error.to_string())?
            .as_deref()
            == Some("staged")
        {
            remove_from_queue(file_hash);
            return Ok(());
        }
        return Err(format!("{file_hash} is not currently queued"));
    }
    drop(state);
    remove_from_queue(file_hash);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::analysis_log_line_matches_node;

    #[test]
    fn split_provider_log_filter_accepts_presentation_node_identity() {
        let line = r#"{"node_id":"separator","presentation_node_id":"separator.vocals"}"#;
        assert!(analysis_log_line_matches_node(
            line,
            Some("separator.vocals")
        ));
        assert!(analysis_log_line_matches_node(line, Some("separator")));
        assert!(!analysis_log_line_matches_node(
            line,
            Some("separator.instrumental")
        ));
    }
}

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
    super::engine_run::cancel_active_engine(file_hash).unwrap_or_else(|| {
        Err("active queue entry has no cancellable exact Engine request".to_string())
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnalysisBatchQueueResult {
    pub queued: usize,
    pub blocked: usize,
}

pub fn enqueue_all(filters: &LibraryMenuFilters) -> AnalysisBatchQueueResult {
    let queue = AnalysisQueue::load();
    let pending_hashes =
        library_db::iter_file_hashes_filtered_not_analyzed(filters).unwrap_or_default();
    let mut result = AnalysisBatchQueueResult::default();
    for file_hash in pending_hashes {
        if queue_entry_blocks_enqueue(queue.entries.get(&file_hash)) {
            continue;
        }
        match crate::analysis_engine_adapter::preview_and_queue_engine_run(&file_hash, None) {
            Ok(_) => result.queued += 1,
            Err(error) => {
                result.blocked += 1;
                let message = format!("Exact Engine preview blocked: {error}");
                let _ = library_db::analysis_queue_upsert_row(
                    &file_hash,
                    "failed",
                    None,
                    Some(&message),
                );
            }
        }
    }
    result
}
