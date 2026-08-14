use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

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
}

pub fn load_analysis_tasks() -> Vec<AnalysisTask> {
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

static FORCE_TRANSCRIBE: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Hashes whose queued job should only run stem separation (key detect +
/// separation) and keep the already-written LRC-provided transcript.
static STEMS_ONLY: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Hashes queued to regenerate only cached pitch evidence and editable notes.
static PITCH_ONLY: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Mark a hash so its next analysis pass separates stems without transcribing,
/// preserving the transcript built from provided LRC.
pub fn mark_stems_only(file_hash: &str) {
    STEMS_ONLY.lock().unwrap().insert(file_hash.to_string());
}

// ─── Helpers ─────────────────────────────────────────────────────────

fn update_queue_status(file_hash: &str, status: QueuedStatus) {
    let (st, pct, msg) = match &status {
        QueuedStatus::Queued => ("queued", None, None::<String>),
        QueuedStatus::Analyzing(p) => ("analyzing", Some(*p as i64), None::<String>),
        QueuedStatus::Failed(s) => ("failed", None, Some(s.clone())),
    };
    let _ = library_db::analysis_queue_upsert_row(file_hash, st, pct, msg.as_deref());
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
        if let Some(value) = tempo {
            song.tempo = value;
        }
        // LRC-provided songs without stem separation are flagged in the
        // transcript; mirror that onto the song so authoring uses the original mix.
        song.no_stems = read_transcript_meta(&CacheDir::new(), file_hash).no_stems;
    } else {
        song.key = None;
        song.override_key = None;
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
    }
    ensure_worker_running(&mut state);
}

fn queue_entry_blocks_enqueue(status: Option<&QueuedStatus>) -> bool {
    matches!(
        status,
        Some(QueuedStatus::Queued | QueuedStatus::Analyzing(_))
    )
}

pub fn enqueue_all(filters: &LibraryMenuFilters) {
    let queue = AnalysisQueue::load();
    let mut state = ANALYZER.lock().unwrap();

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
    update_song_analyzed(file_hash, false, None, None, None, None);
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

pub fn reanalyze_pitch(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }

    let cache = CacheDir::new();
    let _ = std::fs::remove_file(cache.pitch_track_path(file_hash));
    let _ = std::fs::remove_file(cache.pitch_notes_path(file_hash));
    PITCH_ONLY.lock().unwrap().insert(file_hash.to_string());
    enqueue_one(file_hash);
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
    let _ = std::fs::remove_file(cache.transcript_path(file_hash));
    cache.delete_transcript_variants(file_hash);
    update_song_analyzed(
        file_hash,
        false,
        language.or(previous_language),
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

    FORCE_TRANSCRIBE
        .lock()
        .unwrap()
        .insert(file_hash.to_string());

    reanalyze(file_hash, false);
}

fn reanalyze(file_hash: &str, full: bool) {
    let cache = CacheDir::new();
    if full {
        cache.delete_song_cache(file_hash);
    } else {
        let _ = std::fs::remove_file(cache.transcript_path(file_hash));
        cache.delete_transcript_variants(file_hash);
        let _ = std::fs::remove_file(cache.lyrics_path(file_hash));
    }
    update_song_analyzed(file_hash, false, None, None, None, None);
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

    info!(
        "[analyzer] Starting analysis: {} (hash={})",
        local_path.display(),
        file_hash
    );

    update_queue_status(file_hash, QueuedStatus::Analyzing(0));

    // Stems-only: keep the LRC-provided transcript and just separate stems.
    // The intent may have been keyed by the pre-rekey hash for remote songs.
    let stems_only = {
        let mut set = STEMS_ONLY.lock().unwrap();
        set.remove(file_hash) || set.remove(initial_hash)
    };
    let pitch_only = {
        let mut set = PITCH_ONLY.lock().unwrap();
        set.remove(file_hash) || set.remove(initial_hash)
    };
    if stems_only && file_hash != initial_hash {
        // Move the pre-written transcript to the rekeyed hash so the pass can
        // patch it in place.
        let _ = std::fs::rename(
            cache.transcript_path(initial_hash),
            cache.transcript_path(file_hash),
        );
    }

    let config = AppConfig::load();
    let skip_lrclib =
        stems_only || pitch_only || FORCE_TRANSCRIBE.lock().unwrap().remove(file_hash);
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
        "separator": config.separator(),
        "engine": config.asr_engine(),
        "align_backend": config.align_backend(),
        "vocal_detection_threshold_pct": config.vocal_detection_threshold_pct(),
    });

    if stems_only || pitch_only {
        cmd_json["skip_transcription"] = serde_json::json!(true);
    }

    if let Some(ref lp) = lyrics_path {
        cmd_json["lyrics"] = serde_json::json!(lp.to_string_lossy());
    }
    let language_hint = config
        .language_override(file_hash)
        .map(str::to_string)
        .or_else(|| lyrics_path.as_ref().and_then(|_| song.language.clone()))
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
            return;
        }

        let server = guard.as_mut().unwrap();
        match send_and_monitor(server, &json_str, Some(file_hash)) {
            Ok(SongResult::Done) => {
                finalize_song(file_hash, cache);
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
                return;
            }
            Ok(SongResult::Error(msg)) => {
                update_queue_status(file_hash, QueuedStatus::Failed(msg));
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
                return;
            }
        }
    }
}

fn finalize_song(file_hash: &str, cache: &CacheDir) {
    if cache.transcript_exists(file_hash) {
        let meta = read_transcript_meta(cache, file_hash);
        remove_from_queue(file_hash);
        update_song_analyzed(
            file_hash,
            true,
            meta.language,
            Some(meta.source),
            meta.key,
            Some(meta.tempo),
        );
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
    song.tempo = 1.0;
    song.key_offset = 0;
    song.no_stems = true;
    library_db::update_song_fields(&real_hash, &song)
        .map_err(|e| UtaStudioError::Other(e.to_string()))?;
    // Detect the key off-queue in the background; patch it onto the row once it
    // lands so key/tempo export variants unlock without blocking authoring.
    std::thread::spawn(move || {
        let cache = CacheDir::new();
        if let Err(e) = run_key_pass(&cache, &local_path, &real_hash) {
            warn!("[analyzer] LRC key detection failed for {real_hash}: {e}");
            return;
        }
        let meta = read_transcript_meta(&cache, &real_hash);
        if let Some(mut updated) = library_db::load_song_by_hash(&real_hash).ok().flatten() {
            updated.key = meta.key;
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
            ServerEvent::Progress { pct, msg } => {
                if !msg.is_empty() {
                    info!("[analyzer] progress {pct}% {msg}");
                }
                if let Some(hash) = progress_hash {
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
