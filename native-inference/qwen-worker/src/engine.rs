use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::WorkerKind;
use crate::audio;
use crate::runtime::ValidatedRuntime;

const ASR_MODEL_SHA256: &str = "b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e";
const ALIGN_MODEL_SHA256: &str = "c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b";
const ASR_LANGUAGE_CONTRACT_VERSION: u32 = 1;
const ASR_EXPLICIT_LANGUAGE_HINT_POLICY: &str = "reject";
const ASR_LANGUAGE_EVIDENCE_SOURCE: &str = "runtime_detected";
const ASR_LONG_INPUT_POLICY: &str = "qwen-asr-windowed-90s-v1";
const ASR_WINDOW_MAX_SECONDS: f64 = 90.0;
// The pinned qwen3_asr decode budget is a fixed per-call generation cap, not a
// caller-tunable flag (`transcribe-cli --help` exposes no max-tokens option).
// A dense-text window (e.g. compact CJK lyrics) can exhaust that budget before
// end-of-stream. Recover deterministically by halving the offending window and
// retrying, bounded so a pathological window cannot retry forever.
const ASR_WINDOW_MIN_SECONDS: f64 = 10.0;
const ASR_MAX_SPLIT_DEPTH: u32 = 4;
const ALIGN_TEXT_NORMALIZATION_PROFILE: &str = "qwen-align-text-preserve-v1";
const ALIGN_LANGUAGE_NORMALIZATION_PROFILE: &str = "qwen-align-language-v1";
const ALIGN_SEMANTICS_PROFILE: &str = "qwen-align-token-word-80ms-v1";
const ALIGN_LONG_INPUT_POLICY: &str = "qwen-align-windowed-v1";
const ALIGN_WINDOW_TARGET_SECONDS: f64 = 110.0;
const ALIGN_WINDOW_MAX_SECONDS: f64 = 140.0;
const ALIGN_TIMESTAMP_TICK_SECONDS: f64 = 0.08;
// A real Asphodelos alignment preserved all transcript characters and kept
// timestamps ordered, yet collapsed more than ten lyric lines into one
// 0.16-second boundary. Such a boundary cannot provide note-level lyric
// assignment: it puts a paragraph on one note while leaving the surrounding
// audio empty. Treat this specific loss-of-resolution shape as a bad window
// measurement so the existing shorter-window retry can measure it again.
const ALIGN_COLLAPSED_WORD_MIN_CHARACTERS: usize = 12;
const ALIGN_COLLAPSED_WORD_MAX_TICKS: f64 = 2.0;
const ALIGN_CONTEXT_UNITS: usize = 3;
/// Base search margin before each anchored window's real line span, and
/// after the final window where there is no following lyric boundary to
/// cross. Real singing commonly starts a beat before a crowd-sourced LRC
/// line's own stamped time. Non-final windows end at their last line's known
/// end: a right margin let a real line's final word drift 5+ seconds across
/// the next line's LRC start, making the two independently measured windows
/// impossible to reconcile. Retries reduce this margin; the final line-sized
/// recovery uses the exact caller ranges with no margin, so independently
/// measured lines cannot select audio across a known lyric boundary.
const ALIGN_ANCHOR_MARGIN_SECONDS: f64 = 6.0;
/// A single LRC line's own claimed [start, end) span is capped here before
/// the margin above is added. Timed LRC only stamps line *starts*; `end` is
/// synthesized from the *next* line's start (see `lrc::parse_lrc`), so one
/// mistimed neighbor can make an individual line's claimed span balloon to
/// tens of seconds -- confirmed against a real song where a stale/duplicate
/// timestamp left one line claiming a 34-second span. Capping keeps that
/// anchor's own search window bounded without discarding the anchor.
const ALIGN_ANCHOR_MAX_SPAN_SECONDS: f64 = 25.0;
const MAX_ENGINE_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const ENGINE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const ASR_RUNTIME_ARGS: &[&str] = &[
    "--backend",
    "vulkan",
    "--device",
    "0",
    "--n-ctx",
    "0",
    "--timestamps",
    "none",
    "-o",
];

#[derive(Serialize)]
struct LanguageContractEvidence<'a> {
    version: u32,
    explicit_hint_policy: &'a str,
    evidence_source: &'a str,
}

#[derive(Serialize)]
struct TranscriptEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    model_sha256: &'a str,
    backend: &'a str,
    runtime_manifest_sha256: &'a str,
    language_contract: LanguageContractEvidence<'a>,
    language: &'a str,
    text: &'a str,
    long_input: AsrLongInputEvidence<'a>,
}

#[derive(Serialize)]
struct AsrSegmentEvidence {
    index: usize,
    audio_start_seconds: f64,
    audio_end_seconds: f64,
    detected_language: String,
    text_characters: usize,
}

#[derive(Serialize)]
struct AsrLongInputEvidence<'a> {
    policy: &'a str,
    max_window_seconds: f64,
    source_duration_seconds: f64,
    segments: &'a [AsrSegmentEvidence],
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
struct AlignmentWord {
    word: String,
    start: f64,
    end: f64,
}

#[derive(Deserialize)]
struct RawAlignment {
    words: Vec<AlignmentWord>,
}

/// The pinned aligner may emit zero-duration Unicode pieces at the exact end
/// of a measured segment. Preserve every piece by joining it to the measured
/// adjacent segment; never invent or interpolate timestamps.
fn normalize_alignment_words(words: Vec<AlignmentWord>) -> Result<Vec<AlignmentWord>, String> {
    let mut normalized: Vec<AlignmentWord> = Vec::new();
    let mut pending = String::new();
    let mut previous_raw_start = 0.0;
    for mut word in words {
        if word.word.trim().is_empty()
            || !word.start.is_finite()
            || !word.end.is_finite()
            || word.start < 0.0
            || word.end < word.start
            || word.start < previous_raw_start
        {
            return Err("Qwen alignment output has invalid word timing".to_string());
        }
        previous_raw_start = word.start;
        if word.end == word.start {
            if let Some(previous) = normalized
                .last_mut()
                .filter(|previous| word.start <= previous.end + f64::EPSILON)
            {
                previous.word.push_str(&word.word);
            } else {
                pending.push_str(&word.word);
            }
            continue;
        }
        if !pending.is_empty() {
            word.word.insert_str(0, &pending);
            pending.clear();
        }
        if normalized
            .last()
            .is_some_and(|previous| word.start < previous.end)
        {
            return Err("Qwen alignment output has overlapping word timing".to_string());
        }
        normalized.push(word);
    }
    if !pending.is_empty() {
        normalized
            .last_mut()
            .ok_or_else(|| "Qwen Forced Aligner returned no measured boundaries".to_string())?
            .word
            .push_str(&pending);
    }
    if normalized.is_empty() {
        return Err("Qwen Forced Aligner returned no measured boundaries".to_string());
    }
    Ok(normalized)
}

#[derive(Debug, Clone, Serialize)]
struct AlignmentSegmentEvidence {
    index: usize,
    audio_start_seconds: f64,
    audio_end_seconds: f64,
    context_unit_start: usize,
    target_unit_start: usize,
    target_unit_end: usize,
    measured_units: usize,
}

#[derive(Serialize)]
struct AlignmentLongInputEvidence<'a> {
    policy: &'a str,
    max_window_seconds: f64,
    source_duration_seconds: f64,
    text_unit_count: usize,
    segments: &'a [AlignmentSegmentEvidence],
}

#[derive(Serialize)]
struct AlignmentEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    model_sha256: &'a str,
    backend: &'a str,
    runtime_manifest_sha256: &'a str,
    text_normalization_profile: &'a str,
    language_normalization_profile: &'a str,
    alignment_semantics_profile: &'a str,
    transcript: &'a str,
    language: Option<&'a str>,
    runtime_language: Option<&'a str>,
    long_input: AlignmentLongInputEvidence<'a>,
    words: Vec<AlignmentWord>,
}

#[derive(Debug, PartialEq, Eq)]
struct NormalizedAlignmentInput {
    transcript: String,
    language: Option<&'static str>,
    runtime_language: Option<&'static str>,
}

fn normalize_alignment_input(
    config: &serde_json::Value,
) -> Result<NormalizedAlignmentInput, String> {
    let raw = config
        .get("text")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "Qwen Forced Aligner requires config.text".to_string())?;
    if raw.contains('\0') {
        return Err("Qwen Forced Aligner text contains a NUL character".to_string());
    }
    // Profile v1 canonicalizes line endings and outer whitespace only. Inner
    // Unicode scalar values and punctuation remain byte-for-byte unchanged.
    let line_normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let transcript = line_normalized.trim().to_string();
    if transcript.is_empty() {
        return Err("Qwen Forced Aligner requires non-empty normalized text".to_string());
    }
    let (language, runtime_language) = match config.get("language") {
        None | Some(serde_json::Value::Null) => (None, None),
        Some(value) => {
            let language = value.as_str().ok_or_else(|| {
                "Qwen Forced Aligner language must be a supported language code".to_string()
            })?;
            match language.trim().to_ascii_lowercase().as_str() {
                "zh" => (Some("zh"), Some("chinese")),
                "en" => (Some("en"), Some("english")),
                "yue" => (Some("yue"), Some("chinese")),
                "fr" => (Some("fr"), Some("french")),
                "de" => (Some("de"), Some("german")),
                "it" => (Some("it"), Some("italian")),
                "ja" => (Some("ja"), Some("japanese")),
                "ko" => (Some("ko"), Some("korean")),
                "pt" => (Some("pt"), Some("portuguese")),
                "ru" => (Some("ru"), Some("russian")),
                "es" => (Some("es"), Some("spanish")),
                _ => {
                    return Err(
                        "Qwen Forced Aligner language is not supported by qwen-align-language-v1"
                            .to_string(),
                    );
                }
            }
        }
    };
    Ok(NormalizedAlignmentInput {
        transcript,
        language,
        runtime_language,
    })
}

fn model_path(kind: WorkerKind, config: &serde_json::Value) -> Result<PathBuf, String> {
    let path = if let Some(path) = config.get("model_path").and_then(|value| value.as_str()) {
        PathBuf::from(path)
    } else {
        let root = std::env::var_os("UTA_STUDIO_MODELS_PATH")
            .map(PathBuf::from)
            .ok_or_else(|| "UTA_STUDIO_MODELS_PATH is not configured".to_string())?;
        root.join(kind.model_relative_path())
    };
    if !path.is_file() {
        return Err(format!(
            "{} is not installed; use Settings > Models & runtime",
            kind.model_id()
        ));
    }
    Ok(path)
}

fn read_bounded_engine_pipe(
    mut pipe: impl Read,
    total: Arc<AtomicUsize>,
    oversized: Arc<AtomicBool>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = match pipe.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        let previous = total.fetch_add(count, Ordering::SeqCst);
        let retained = MAX_ENGINE_OUTPUT_BYTES.saturating_sub(previous).min(count);
        bytes.extend_from_slice(&buffer[..retained]);
        if retained < count {
            oversized.store(true, Ordering::SeqCst);
            break;
        }
    }
    bytes
}

/// Owns the platform process-tree boundary for one pinned-engine invocation.
/// Unix keeps the fresh process-group identity the engine was spawned as
/// leader of; Windows owns a kill-on-close Job Object the engine was
/// assigned to before it ever ran user code. Either way, terminating this
/// guard closes the whole tree — including a descendant the engine spawned
/// and left running — not just the direct child.
struct ProcessTreeGuard {
    #[cfg(unix)]
    process_group: Option<i32>,
    #[cfg(windows)]
    job: Option<windows_sys::Win32::Foundation::HANDLE>,
}

impl ProcessTreeGuard {
    fn attach(child: &std::process::Child) -> Result<Self, String> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            };

            // SAFETY: all pointers refer to live local values for the
            // duration of each Win32 call; ownership of the returned handle
            // is retained by this guard and released exactly once in Drop.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err(format!(
                        "could not create pinned Qwen engine job object: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    std::ptr::addr_of!(limits).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) == 0
                {
                    let error = std::io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(format!(
                        "could not configure pinned Qwen engine job object: {error}"
                    ));
                }
                if AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0 {
                    let error = std::io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(format!(
                        "could not assign pinned Qwen engine to its job object: {error}"
                    ));
                }
                return Ok(Self { job: Some(job) });
            }
        }
        #[cfg(unix)]
        {
            Ok(Self {
                process_group: Some(child.id() as i32),
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self {})
        }
    }

    fn terminate(&mut self) {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group.take() {
            // SAFETY: the pinned runtime is spawned as leader of this fresh
            // process group.
            unsafe {
                libc::kill(-process_group, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = self.job.take() {
            // SAFETY: this guard uniquely owns the job object handle.
            // Closing it terminates every process assigned under
            // kill-on-close, including any descendant the engine spawned.
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(job);
            }
        }
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// The engine is created suspended on Windows so it cannot spawn an
/// uncontained descendant before job-object assignment. Resume every thread
/// belonging to the new process (normally just its primary thread) once the
/// guard is attached.
#[cfg(windows)]
fn resume_suspended_engine(child: &std::process::Child) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    // SAFETY: the snapshot handle and every thread handle opened from it are
    // closed on every exit path below.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(format!(
                "could not snapshot threads to resume the pinned Qwen engine: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..THREADENTRY32::default()
        };
        let mut found = false;
        let mut has_entry = Thread32First(snapshot, std::ptr::addr_of_mut!(entry)) != 0;
        while has_entry {
            if entry.th32OwnerProcessID == child.id() {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if thread.is_null() {
                    let error = std::io::Error::last_os_error();
                    CloseHandle(snapshot);
                    return Err(format!(
                        "could not open the pinned Qwen engine's suspended thread: {error}"
                    ));
                }
                let resumed = ResumeThread(thread);
                let resume_error = (resumed == u32::MAX).then(std::io::Error::last_os_error);
                CloseHandle(thread);
                if let Some(error) = resume_error {
                    CloseHandle(snapshot);
                    return Err(format!(
                        "could not resume the pinned Qwen engine's suspended thread: {error}"
                    ));
                }
                found = true;
            }
            has_entry = Thread32Next(snapshot, std::ptr::addr_of_mut!(entry)) != 0;
        }
        CloseHandle(snapshot);
        if !found {
            return Err("pinned Qwen engine's suspended primary thread was not found".to_string());
        }
    }
    Ok(())
}

fn run_engine(command: &mut Command) -> Result<Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        // SAFETY: this closure runs in the child after fork and only calls
        // the async-signal-safe prctl syscall. It ensures supervisor
        // cancellation or a worker crash cannot orphan a GPU engine process.
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Job-object assignment happens after spawn but before this process
        // can run any of its own code, so a suspended start is required: an
        // engine that starts running (and possibly spawns a descendant)
        // before assignment could leave that descendant outside the job's
        // kill-on-close containment.
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start pinned Qwen engine: {error}"))?;
    let mut process_tree = match ProcessTreeGuard::attach(&child) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("could not supervise pinned Qwen engine: {error}"));
        }
    };
    #[cfg(windows)]
    if let Err(error) = resume_suspended_engine(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "pinned Qwen engine stdout unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "pinned Qwen engine stderr unavailable".to_string())?;
    let total = Arc::new(AtomicUsize::new(0));
    let oversized = Arc::new(AtomicBool::new(false));
    let stdout_total = Arc::clone(&total);
    let stdout_oversized = Arc::clone(&oversized);
    let stdout_reader = std::thread::spawn(move || {
        read_bounded_engine_pipe(stdout, stdout_total, stdout_oversized)
    });
    let stderr_total = Arc::clone(&total);
    let stderr_oversized = Arc::clone(&oversized);
    let stderr_reader = std::thread::spawn(move || {
        read_bounded_engine_pipe(stderr, stderr_total, stderr_oversized)
    });

    let status = loop {
        if oversized.load(Ordering::SeqCst) {
            process_tree.terminate();
            break child
                .wait()
                .map_err(|error| format!("pinned Qwen engine wait failed: {error}"))?;
        }
        match child
            .try_wait()
            .map_err(|error| format!("pinned Qwen engine wait failed: {error}"))?
        {
            Some(status) => {
                process_tree.terminate();
                break status;
            }
            None => std::thread::sleep(ENGINE_POLL_INTERVAL),
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "pinned Qwen engine stdout reader failed".to_string())?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "pinned Qwen engine stderr reader failed".to_string())?;
    if oversized.load(Ordering::SeqCst) {
        return Err("Qwen engine log exceeded the bounded capture limit".to_string());
    }
    let output = Output {
        status,
        stdout,
        stderr,
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "pinned Qwen engine failed with {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    if !output.stderr.is_empty() {
        eprintln!(
            "[uta-qwen-worker engine] {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output)
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

pub fn run(
    kind: WorkerKind,
    runtime: &ValidatedRuntime,
    audio: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
    progress: &mut dyn FnMut(u64, u64, &'static str) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let model = model_path(kind, config)?;
    match kind {
        WorkerKind::Asr => run_asr(runtime, &model, audio, output_dir, config, progress),
        WorkerKind::Align => run_align(runtime, &model, audio, output_dir, config, progress),
    }
}

fn validate_asr_language_policy(config: &serde_json::Value) -> Result<(), String> {
    if config.get("language").is_some() {
        return Err(format!(
            "Qwen ASR language contract v{ASR_LANGUAGE_CONTRACT_VERSION} rejects explicit language hints; the pinned runtime owns auto detection"
        ));
    }
    Ok(())
}

fn language_token(value: &str) -> Option<String> {
    let token = value
        .trim()
        .trim_matches(|character: char| {
            character.is_ascii_whitespace()
                || matches!(
                    character,
                    '[' | ']' | '(' | ')' | '<' | '>' | '|' | '\'' | '"'
                )
        })
        .split(|character: char| {
            character.is_ascii_whitespace() || character == ',' || character == ';'
        })
        .next()?
        .trim_end_matches(['.', ':']);
    (!token.is_empty()
        && token.len() <= 32
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'-' | b'_')))
    .then(|| token.to_ascii_lowercase())
}

fn language_from_log(log: &[u8]) -> Result<Option<String>, String> {
    let mut detected = std::collections::BTreeSet::new();
    for line in String::from_utf8_lossy(log).lines() {
        let Some((label, value)) = line.split_once(':').or_else(|| line.split_once('=')) else {
            continue;
        };
        let normalized_label = label
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|character| !character.is_ascii_whitespace())
            .collect::<String>();
        if matches!(
            normalized_label.as_str(),
            "detected-language" | "detectedlanguage" | "languagedetected"
        ) && let Some(language) = language_token(value)
        {
            detected.insert(language);
        }
    }
    if detected.len() > 1 {
        return Err("Qwen ASR returned conflicting detected-language logs".to_string());
    }
    Ok(detected.into_iter().next())
}

fn parse_asr_result(raw: &str, stdout: &[u8], stderr: &[u8]) -> Result<(String, String), String> {
    let mut text = raw.trim();
    let prefix_language = text
        .strip_prefix("<|")
        .and_then(|remainder| remainder.find("|>").map(|end| (&remainder[..end], end + 2)))
        .and_then(|(value, prefix_length)| {
            language_token(value).map(|language| (language, prefix_length + 2))
        });
    let stdout_language = language_from_log(stdout)?;
    let stderr_language = language_from_log(stderr)?;
    if matches!((&stdout_language, &stderr_language), (Some(stdout), Some(stderr)) if stdout != stderr)
    {
        return Err("Qwen ASR returned conflicting detected-language logs".to_string());
    }
    let detected_language = stdout_language.or(stderr_language);
    let language = match (prefix_language, detected_language) {
        (Some((prefix, _prefix_length)), Some(logged)) if prefix != logged => {
            return Err("Qwen ASR returned conflicting detected-language metadata".to_string());
        }
        (Some((prefix, prefix_length)), _) => {
            text = text[prefix_length..].trim();
            prefix
        }
        (None, Some(logged)) => logged,
        (None, None) => {
            return Err("Qwen ASR did not report a runtime-detected language".to_string());
        }
    };
    if text.is_empty() {
        return Err("Qwen ASR returned an empty transcript".to_string());
    }
    Ok((language, text.to_string()))
}

fn plan_asr_segments(source_duration_seconds: f64) -> Result<Vec<(f64, f64)>, String> {
    if !source_duration_seconds.is_finite() || source_duration_seconds <= 0.0 {
        return Err("Qwen ASR source duration is invalid".to_string());
    }
    let count = (source_duration_seconds / ASR_WINDOW_MAX_SECONDS)
        .ceil()
        .max(1.0) as usize;
    Ok((0..count)
        .map(|index| {
            let start = index as f64 * ASR_WINDOW_MAX_SECONDS;
            (
                start,
                (start + ASR_WINDOW_MAX_SECONDS).min(source_duration_seconds),
            )
        })
        .collect())
}

fn execute_asr_window(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    raw: &Path,
) -> Result<(String, String), String> {
    let mut command = Command::new(&runtime.engine);
    command
        .args(["-m"])
        .arg(model)
        // Quiet mode is intentionally absent: the pinned runtime's
        // detected-language line is required evidence, not cosmetic logging.
        .args(ASR_RUNTIME_ARGS)
        .arg(raw)
        .arg(audio)
        .env("GGML_VK_VISIBLE_DEVICES", "0");
    let result = (|| {
        let output = run_engine(&mut command)?;
        let raw_text = std::fs::read_to_string(raw).map_err(|error| error.to_string())?;
        parse_asr_result(&raw_text, &output.stdout, &output.stderr)
    })();
    let _ = std::fs::remove_file(raw);
    result
}

/// The pinned qwen3_asr decode loop reports this exact family of message when
/// it hits its fixed generation cap before end-of-stream (verified against a
/// captured production failure). This is a stable, specific two-phrase match,
/// not a guess: both phrases must appear together.
fn is_generation_budget_truncation(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("output truncated") && lower.contains("generation budget")
}

/// Deterministically halve `[start, end)` for a truncation retry, or return
/// `None` when policy bounds (minimum window width, maximum split depth)
/// forbid another split. Pure and independent of I/O so the bound is testable
/// without a real engine invocation.
fn asr_split_midpoint(start: f64, end: f64, depth: u32) -> Option<f64> {
    let half = (end - start) / 2.0;
    if depth >= ASR_MAX_SPLIT_DEPTH || half < ASR_WINDOW_MIN_SECONDS {
        return None;
    }
    Some(start + half)
}

/// Transcribe `[start, end)`, and on a detected generation-budget truncation,
/// deterministically split the window and retry each half. Every leaf window
/// that returns `Ok` is appended to `out` in left-to-right order, so the
/// concatenation of `out` covers `[start, end)` exactly once with no gaps,
/// overlaps, or duplication. `slice_index` is shared across the whole
/// recursion so every sliced file gets a distinct name.
#[allow(clippy::too_many_arguments)]
fn run_asr_window_recursive(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    output_dir: &Path,
    start: f64,
    end: f64,
    depth: u32,
    slice_index: &mut usize,
    out: &mut Vec<(f64, f64, String, String)>,
) -> Result<(), String> {
    let window = audio::slice_wav(audio, output_dir, *slice_index, start, end - start)?;
    let raw = output_dir.join(format!("qwen-asr-transcript-retry-{:03}.txt", *slice_index));
    *slice_index += 1;
    let result = execute_asr_window(runtime, model, &window, &raw);
    let _ = std::fs::remove_file(&window);
    match result {
        Ok((language, text)) => {
            out.push((start, end, language, text));
            Ok(())
        }
        Err(message) if is_generation_budget_truncation(&message) => {
            match asr_split_midpoint(start, end, depth) {
                Some(mid) => {
                    run_asr_window_recursive(
                        runtime,
                        model,
                        audio,
                        output_dir,
                        start,
                        mid,
                        depth + 1,
                        slice_index,
                        out,
                    )?;
                    run_asr_window_recursive(
                        runtime,
                        model,
                        audio,
                        output_dir,
                        mid,
                        end,
                        depth + 1,
                        slice_index,
                        out,
                    )
                }
                None => Err(format!(
                    "Qwen ASR window [{start:.3}s, {end:.3}s) exceeded the generation budget and \
                 could not be split further within policy bounds: {message}"
                )),
            }
        }
        Err(message) => Err(message),
    }
}

fn run_asr(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
    progress: &mut dyn FnMut(u64, u64, &'static str) -> Result<(), String>,
) -> Result<PathBuf, String> {
    validate_asr_language_policy(config)?;
    let destination = output_dir.join("qwen-asr-transcript-evidence.json");
    let source_duration_seconds = audio::wav_duration_seconds(audio)?;
    let plans = plan_asr_segments(source_duration_seconds)?;
    let mut segments = Vec::with_capacity(plans.len());
    let mut texts = Vec::with_capacity(plans.len());
    let mut language_weights = std::collections::BTreeMap::<String, usize>::new();
    // Progress is real audio-time coverage, not window count: a truncation
    // retry can split one planned window into several, so the eventual
    // window count is not known in advance and must never be guessed at.
    let total_progress_units = (source_duration_seconds * 1000.0).round().max(1.0) as u64;
    let mut slice_index = plans.len();
    for (index, (start, end)) in plans.iter().copied().enumerate() {
        let window = if plans.len() == 1 {
            audio.to_path_buf()
        } else {
            audio::slice_wav(audio, output_dir, index, start, end - start)?
        };
        let raw = output_dir.join(format!("qwen-asr-transcript-{index:03}.txt"));
        let result = execute_asr_window(runtime, model, &window, &raw);
        if plans.len() > 1 {
            let _ = std::fs::remove_file(&window);
        }
        let mut window_results = Vec::with_capacity(1);
        match result {
            Ok((language, text)) => window_results.push((start, end, language, text)),
            Err(message) if is_generation_budget_truncation(&message) => {
                // This exact window already truncated once; retrying it
                // unchanged against a deterministic engine would only
                // truncate again, so split immediately instead of repeating
                // the failed attempt.
                match asr_split_midpoint(start, end, 0) {
                    Some(mid) => {
                        run_asr_window_recursive(
                            runtime,
                            model,
                            audio,
                            output_dir,
                            start,
                            mid,
                            1,
                            &mut slice_index,
                            &mut window_results,
                        )?;
                        run_asr_window_recursive(
                            runtime,
                            model,
                            audio,
                            output_dir,
                            mid,
                            end,
                            1,
                            &mut slice_index,
                            &mut window_results,
                        )?;
                    }
                    None => {
                        return Err(format!(
                            "Qwen ASR window [{start:.3}s, {end:.3}s) exceeded the generation \
                             budget and could not be split further within policy bounds: \
                             {message}"
                        ));
                    }
                }
            }
            Err(message) => return Err(message),
        }
        for (window_start, window_end, language, text) in window_results {
            let completed_units = ((window_end.min(source_duration_seconds) * 1000.0).round()
                as u64)
                .min(total_progress_units);
            progress(
                completed_units,
                total_progress_units,
                "Running pinned Qwen ASR windows",
            )?;
            let text_characters = compact_character_count(&text);
            *language_weights.entry(language.clone()).or_default() += text_characters;
            texts.push(text);
            let segment_index = segments.len();
            segments.push(AsrSegmentEvidence {
                index: segment_index,
                audio_start_seconds: window_start,
                audio_end_seconds: window_end,
                detected_language: language,
                text_characters,
            });
        }
    }
    let text = texts.join(" ");
    let language = language_weights
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
        .map(|(language, _)| language)
        .ok_or_else(|| "Qwen ASR produced no window language evidence".to_string())?;
    atomic_json(
        &destination,
        &TranscriptEvidence {
            schema_version: 2,
            model_id: WorkerKind::Asr.model_id(),
            model_sha256: ASR_MODEL_SHA256,
            backend: "vulkan",
            runtime_manifest_sha256: &runtime.manifest_sha256,
            language_contract: LanguageContractEvidence {
                version: ASR_LANGUAGE_CONTRACT_VERSION,
                explicit_hint_policy: ASR_EXPLICIT_LANGUAGE_HINT_POLICY,
                evidence_source: ASR_LANGUAGE_EVIDENCE_SOURCE,
            },
            language: &language,
            text: &text,
            long_input: AsrLongInputEvidence {
                policy: ASR_LONG_INPUT_POLICY,
                max_window_seconds: ASR_WINDOW_MAX_SECONDS,
                source_duration_seconds,
                segments: &segments,
            },
        },
    )?;
    Ok(destination)
}

#[derive(Debug, Clone, PartialEq)]
struct AlignmentSegmentPlan {
    index: usize,
    audio_start_seconds: f64,
    audio_end_seconds: f64,
    context_unit_start: usize,
    target_unit_start: usize,
    target_unit_end: usize,
    /// This plan's own claimed line start (seconds, before margin/capping),
    /// for anchored plans only. The LRC's own stated line boundary is a more
    /// trustworthy seam point than a tick-split of two independently
    /// (and, for a short line given a wide search margin, sometimes badly)
    /// mismeasured windows -- confirmed against a real seam where a widened
    /// retry margin let the model attribute a single character a
    /// 17-second duration, drifting into the neighboring line's territory.
    anchor_start: Option<f64>,
}

fn alignment_text_units(transcript: &str) -> Vec<String> {
    let whitespace_units = transcript.split_whitespace().collect::<Vec<_>>();
    if whitespace_units.len() > 1 {
        return whitespace_units.into_iter().map(str::to_string).collect();
    }
    transcript
        .chars()
        .filter(|character| !character.is_whitespace())
        .map(|character| character.to_string())
        .collect()
}

fn tick_floor(seconds: f64) -> f64 {
    (seconds / ALIGN_TIMESTAMP_TICK_SECONDS).floor() * ALIGN_TIMESTAMP_TICK_SECONDS
}

fn tick_round(seconds: f64) -> f64 {
    (seconds / ALIGN_TIMESTAMP_TICK_SECONDS).round() * ALIGN_TIMESTAMP_TICK_SECONDS
}

fn plan_alignment_segments(
    source_duration_seconds: f64,
    text_unit_count: usize,
    window_target_seconds: f64,
) -> Result<Vec<AlignmentSegmentPlan>, String> {
    if !source_duration_seconds.is_finite() || source_duration_seconds <= 0.0 {
        return Err("Qwen alignment source duration is invalid".to_string());
    }
    if !window_target_seconds.is_finite() || window_target_seconds <= 0.0 {
        return Err("Qwen alignment window target is invalid".to_string());
    }
    let segment_count = (source_duration_seconds / window_target_seconds)
        .ceil()
        .max(1.0) as usize;
    if segment_count > 1 && text_unit_count < segment_count {
        return Err(format!(
            "Qwen long-form alignment requires at least {segment_count} lyric units"
        ));
    }
    let owner_seconds = source_duration_seconds / segment_count as f64;
    let mut plans = Vec::with_capacity(segment_count);
    for index in 0..segment_count {
        let target_unit_start = (index * text_unit_count).div_ceil(segment_count);
        let target_unit_end = ((index + 1) * text_unit_count).div_ceil(segment_count);
        let audio_start_seconds = if segment_count == 1 || index == 0 {
            0.0
        } else if index + 1 == segment_count {
            tick_floor((source_duration_seconds - ALIGN_WINDOW_MAX_SECONDS).max(0.0))
        } else {
            let owner_center = (index as f64 + 0.5) * owner_seconds;
            tick_floor((owner_center - ALIGN_WINDOW_MAX_SECONDS / 2.0).max(0.0))
        };
        let audio_end_seconds =
            (audio_start_seconds + ALIGN_WINDOW_MAX_SECONDS).min(source_duration_seconds);
        plans.push(AlignmentSegmentPlan {
            index,
            audio_start_seconds,
            audio_end_seconds,
            context_unit_start: target_unit_start.saturating_sub(ALIGN_CONTEXT_UNITS),
            target_unit_start,
            target_unit_end,
            anchor_start: None,
        });
    }
    Ok(plans)
}

/// One window per caller-supplied line, searched only near that line's own
/// given time range instead of a position blindly inferred from its index
/// among all lyric units. Blind planning (`plan_alignment_segments`) assumes
/// every unit occupies an equal share of the source duration; real lines
/// vary from a few seconds to tens of seconds, so once one line is unusually
/// long every later window's assumed position drifts from where that text
/// actually is -- confirmed against a real song where this produced a
/// window whose measurement collapsed 14 characters into 0.16 seconds and
/// left a 30+ second stretch of the song with no usable alignment at all.
///
/// Windows still cover *several* consecutive lines each, like blind
/// planning -- not one line per window. An earlier one-line-per-window
/// design measured every line boundary independently against windows that
/// deliberately overlap (each line's own margin), and on a real song that
/// made *most* line-to-line seams a genuine reconciliation gamble between
/// two independent measurements instead of the rare edge case blind
/// planning's seam logic was built for. Grouping lines the same way blind
/// planning does keeps that seam logic rare and well-exercised; only the
/// *position* of each group's window comes from real anchor times instead
/// of an even split, which is what actually fixes the original bug.
fn plan_alignment_segments_from_anchors(
    line_anchors: &[(f64, f64)],
    text_unit_count: usize,
    source_duration_seconds: f64,
    window_target_seconds: f64,
    margin_seconds: f64,
) -> Result<Vec<AlignmentSegmentPlan>, String> {
    if !source_duration_seconds.is_finite() || source_duration_seconds <= 0.0 {
        return Err("Qwen alignment source duration is invalid".to_string());
    }
    if !window_target_seconds.is_finite() || window_target_seconds <= 0.0 {
        return Err("Qwen alignment window target is invalid".to_string());
    }
    if !margin_seconds.is_finite() || margin_seconds < 0.0 {
        return Err("Qwen alignment anchor margin is invalid".to_string());
    }
    if line_anchors.is_empty() {
        return Err("Qwen alignment requires at least one anchored line".to_string());
    }
    // `alignment_text_units` splits the *whole* joined transcript on any
    // whitespace, including the `\n` between lines, so one caller line
    // becomes exactly one global unit -- but only when that holds; a line
    // containing its own internal whitespace (e.g. multi-word English)
    // would silently desync anchor index from unit index. Fail closed
    // instead of guessing: confirmed against a real bug where computing
    // each line's own unit count separately (falling back to a
    // per-*character* split for whitespace-free CJK lines) desynced from
    // the global line-level index, making a later window's "one line"
    // target swell to include a dozen unrelated lines.
    if text_unit_count != line_anchors.len() {
        return Err(format!(
            "Qwen alignment line_anchors count ({}) does not match transcript unit count ({}); anchored windowing requires exactly one lyric unit per anchored line",
            line_anchors.len(),
            text_unit_count
        ));
    }
    for (index, &(start, end)) in line_anchors.iter().enumerate() {
        if !start.is_finite() || !end.is_finite() || end <= start || start < 0.0 {
            return Err(format!(
                "Qwen alignment line_anchors[{index}] has an invalid time range"
            ));
        }
    }
    // One line's own claimed span is capped before it can grow a group --
    // otherwise the exact anomaly this function exists to contain (a single
    // mistimed line claiming tens of seconds) would just inflate whichever
    // group it lands in instead.
    let capped_ends = line_anchors
        .iter()
        .map(|&(start, end)| start + (end - start).min(ALIGN_ANCHOR_MAX_SPAN_SECONDS))
        .collect::<Vec<_>>();
    let mut groups = Vec::<(usize, usize)>::new();
    let mut group_first = 0_usize;
    // `index` is also used as a plain value (recorded into `groups`, and to
    // update `group_first`), not just to index `capped_ends`, so this isn't
    // a clean `enumerate()` rewrite.
    #[allow(clippy::needless_range_loop)]
    for index in 1..line_anchors.len() {
        if capped_ends[index] - line_anchors[group_first].0 > window_target_seconds {
            groups.push((group_first, index - 1));
            group_first = index;
        }
    }
    groups.push((group_first, line_anchors.len() - 1));

    let mut plans = Vec::with_capacity(groups.len());
    for (plan_index, &(first, last)) in groups.iter().enumerate() {
        let raw_start = line_anchors[first].0;
        let audio_start_seconds = tick_floor((raw_start - margin_seconds).max(0.0));
        let trailing_margin = if last + 1 == line_anchors.len() {
            margin_seconds
        } else {
            0.0
        };
        let audio_end_seconds = (capped_ends[last] + trailing_margin)
            .min(source_duration_seconds)
            .min(audio_start_seconds + ALIGN_WINDOW_MAX_SECONDS)
            .max(audio_start_seconds + ALIGN_TIMESTAMP_TICK_SECONDS);
        // A preceding line only belongs in the *searched text* when most of
        // its claimed content is still inside this window's sliced audio. A
        // fixed "3 units back", or merely checking that the line's final
        // instant is present, can otherwise supply a whole line for a tiny
        // audio remnant. Real Asphodelos repro: 0.10s of a 6.21s preceding
        // line remained, so the aligner crammed that whole line against the
        // window end and pushed the owned line across the next seam. Keep a
        // candidate when at least a quarter of its capped span remains; a line
        // starting slightly before the window is still useful when it is
        // substantially present. A second real boundary in the same song
        // retained 6.08s of a 12.28s preceding line; excluding that useful
        // context made the aligner pin the owned line to the window start
        // instead of finding its 6-second-later anchor.
        let mut context_unit_start = first;
        for back in 1..=ALIGN_CONTEXT_UNITS {
            let Some(candidate) = first.checked_sub(back) else {
                break;
            };
            let candidate_span = capped_ends[candidate] - line_anchors[candidate].0;
            let included_span =
                (capped_ends[candidate] - audio_start_seconds).clamp(0.0, candidate_span);
            if included_span * 4.0 < candidate_span {
                break;
            }
            context_unit_start = candidate;
        }
        plans.push(AlignmentSegmentPlan {
            index: plan_index,
            audio_start_seconds,
            audio_end_seconds,
            context_unit_start,
            target_unit_start: first,
            target_unit_end: last + 1,
            anchor_start: Some(raw_start),
        });
    }
    Ok(plans)
}

fn parsed_line_anchors(config: &serde_json::Value) -> Result<Option<Vec<(f64, f64)>>, String> {
    let Some(value) = config.get("line_anchors") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let array = value
        .as_array()
        .ok_or_else(|| "Qwen Forced Aligner line_anchors must be an array".to_string())?;
    if array.is_empty() {
        return Ok(None);
    }
    array
        .iter()
        .map(|entry| {
            let start = entry
                .get("start")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| {
                    "Qwen Forced Aligner line_anchors entry is missing start".to_string()
                })?;
            let end = entry
                .get("end")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| {
                    "Qwen Forced Aligner line_anchors entry is missing end".to_string()
                })?;
            Ok((start, end))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Some)
}

fn compact_character_count(text: &str) -> usize {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .count()
}

fn validate_alignment_measurement_resolution(words: &[AlignmentWord]) -> Result<(), String> {
    const EPSILON: f64 = 1e-6;
    let collapsed_duration =
        ALIGN_TIMESTAMP_TICK_SECONDS * ALIGN_COLLAPSED_WORD_MAX_TICKS + EPSILON;
    if let Some(word) = words.iter().find(|word| {
        compact_character_count(&word.word) >= ALIGN_COLLAPSED_WORD_MIN_CHARACTERS
            && word.end - word.start <= collapsed_duration
    }) {
        return Err(format!(
            "Qwen alignment output has invalid word timing: collapsed {} characters into {:.2} seconds",
            compact_character_count(&word.word),
            word.end - word.start
        ));
    }
    Ok(())
}

fn validate_alignment_unit_boundaries(
    words: &[AlignmentWord],
    target_units: &[String],
) -> Result<(), String> {
    let boundaries = target_units
        .iter()
        .take(target_units.len().saturating_sub(1))
        .scan(0_usize, |cursor, unit| {
            *cursor += compact_character_count(unit);
            Some(*cursor)
        })
        .collect::<Vec<_>>();
    let mut cursor = 0_usize;
    for word in words {
        let end = cursor
            .checked_add(compact_character_count(&word.word))
            .ok_or_else(|| "Qwen alignment word range overflow".to_string())?;
        if boundaries
            .iter()
            .any(|boundary| cursor < *boundary && *boundary < end)
        {
            return Err(
                "Qwen alignment output has invalid word timing: one measured boundary merged multiple lyric units"
                    .to_string(),
            );
        }
        cursor = end;
    }
    Ok(())
}

fn target_words_from_context(
    words: Vec<AlignmentWord>,
    context_text: &str,
    prefix_characters: usize,
    target_characters: usize,
) -> Result<Vec<AlignmentWord>, String> {
    if words
        .iter()
        .map(|word| word.word.as_str())
        .collect::<String>()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        != context_text
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    {
        return Err("Qwen alignment window did not preserve its complete text".to_string());
    }
    let target_end = prefix_characters
        .checked_add(target_characters)
        .ok_or_else(|| "Qwen alignment text range overflow".to_string())?;
    let mut cursor = 0_usize;
    let mut selected = Vec::new();
    for word in words {
        let count = compact_character_count(&word.word);
        let end = cursor
            .checked_add(count)
            .ok_or_else(|| "Qwen alignment word range overflow".to_string())?;
        if (cursor < prefix_characters && end > prefix_characters)
            || (cursor < target_end && end > target_end)
        {
            return Err("Qwen alignment context boundary split a runtime word".to_string());
        }
        if cursor >= prefix_characters && end <= target_end {
            selected.push(word);
        }
        cursor = end;
    }
    if cursor != compact_character_count(context_text) || selected.is_empty() {
        return Err("Qwen alignment window target text is incomplete".to_string());
    }
    Ok(selected)
}

/// The two windows either side of a long-form seam are independently measured
/// against overlapping audio, so their boundary words can disagree by a few
/// ticks. Reconcile only the exact pair of words touching the seam, and only
/// when a single deterministic tick-aligned point exists that keeps both
/// words positive-duration and falls inside audio both windows actually
/// measured. Otherwise leave the words untouched and fail closed.
fn reconcile_alignment_seam(
    previous_plan: &AlignmentSegmentPlan,
    next_plan: &AlignmentSegmentPlan,
    previous_word: &mut AlignmentWord,
    next_word: &mut AlignmentWord,
) -> Result<(), String> {
    const EPSILON: f64 = 1e-6;
    if next_word.start >= previous_word.end {
        return Ok(());
    }
    let window_lower = previous_plan
        .audio_start_seconds
        .max(next_plan.audio_start_seconds);
    let window_upper = previous_plan
        .audio_end_seconds
        .min(next_plan.audio_end_seconds);
    let tick_split_seam = {
        let overlap_ticks =
            ((previous_word.end - next_word.start) / ALIGN_TIMESTAMP_TICK_SECONDS).round() as i64;
        let split_ticks = overlap_ticks.max(0) / 2;
        next_word.start + split_ticks as f64 * ALIGN_TIMESTAMP_TICK_SECONDS
    };
    // Prefer the next line's own LRC-stamped start over a tick-split of the
    // two windows' independent measurements: it is the caller's ground truth
    // for where this lyrical line actually begins, not a guess derived from
    // two windows that -- for a short line given a wide search margin --
    // can both drift by many seconds (confirmed against a real seam where a
    // widened retry margin let the model attribute a single character a
    // 17-second duration). Only anchored plans carry this hint; blind
    // planning falls straight through to the tick-split candidate.
    // LRC starts are arbitrary centiseconds, while the worker's published
    // alignment contract requires every boundary to remain on the model's
    // 80 ms timestamp grid. Reconciliation happens after per-window timing
    // normalization, so inserting the raw LRC start here would otherwise
    // create an artifact that the Engine correctly rejects even though the
    // worker reports `done/ok` (real repro: 127.13s and 263.03s seams).
    let anchor_seam = next_plan.anchor_start.map(tick_round);
    for seam in [anchor_seam, Some(tick_split_seam)].into_iter().flatten() {
        if seam - previous_word.start > EPSILON
            && next_word.end - seam > EPSILON
            && seam >= window_lower - EPSILON
            && seam <= window_upper + EPSILON
        {
            previous_word.end = seam;
            next_word.start = seam;
            return Ok(());
        }
    }
    // Blind planning keeps the strict fail-closed contract above: a
    // continuous single measurement disagreeing with itself at an internal
    // seam is unexpected and worth surfacing loudly. Anchored planning
    // measures each line independently against windows that overlap by
    // design (every anchor's own margin), so two adjacent-but-different
    // lines' independent measurements disagreeing by a few seconds is the
    // expected cost of that independence, not a sign the whole run is
    // unusable. Fall back to a directional clamp instead of discarding the
    // run: trust whichever word's own boundary sits inside the other's
    // span, and only fail closed when the two truly invert (one line's
    // entire measured span precedes the other's start) -- confirmed
    // against real seams from both directions on a real song.
    if next_plan.anchor_start.is_some() {
        if next_word.start > previous_word.start && next_word.start < previous_word.end {
            previous_word.end = next_word.start;
            return Ok(());
        }
        if previous_word.end > next_word.start && previous_word.end < next_word.end {
            next_word.start = previous_word.end;
            return Ok(());
        }
    }
    Err(
        "Qwen long-form alignment windows produced overlapping timing that could not be \
         reconciled at the window seam"
            .to_string(),
    )
}

/// A parse failure here means the pinned aligner's JSON *write* was
/// corrupted, not that its measurement was wrong -- two different real
/// repros so far ("control character ... at line 4 column 0" and "key must
/// be a string at line 10 column 5") land at different byte offsets for
/// unchanged audio/text/window-plan input, which a genuine measurement
/// disagreement would not do (see the determinism note on
/// `is_retryable_window_measurement_error` below: unlike output corruption,
/// a real measurement problem reproduces identically on an unmodified
/// retry, which is exactly why that retry class re-plans with a different
/// window instead of just re-running). That points at a transient
/// write/flush race in the external engine process rather than content the
/// model actually got wrong, so a bounded retry of just this window's
/// run+parse step -- unmodified, unlike the seam/measurement retries -- is
/// the appropriate fix here. `is_retryable_window_measurement_error` does
/// not match this error's text, so this retry is the only thing that
/// covers it; the two mechanisms handle disjoint failure classes.
const ALIGNMENT_WINDOW_PARSE_ATTEMPTS: u32 = 3;

fn execute_alignment_window(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    raw: &Path,
    text: &str,
    runtime_language: Option<&str>,
) -> Result<Vec<AlignmentWord>, String> {
    let mut last_error = String::new();
    for attempt in 1..=ALIGNMENT_WINDOW_PARSE_ATTEMPTS {
        let mut command = Command::new(&runtime.engine);
        command
            .args(["-m"])
            .arg(model)
            .args(["-f"])
            .arg(audio)
            .args(["-o"])
            .arg(raw)
            .args(["--align", "--text", text, "--no-timing"])
            .env("GGML_VK_VISIBLE_DEVICES", "0")
            .env("QWEN_USE_VRAM", "1")
            .env("QWEN_REQUIRE_GPU", "1");
        if let Some(language) = runtime_language {
            command.args(["-l", language]);
        }
        run_engine(&mut command)?;
        let raw_bytes = std::fs::read(raw).map_err(|error| error.to_string())?;
        // The pinned aligner's own JSON writer has been observed emitting raw,
        // unescaped control bytes (e.g. a literal newline) inside string values
        // -- invalid per the JSON spec, and fatal to serde_json (real repro:
        // "control character ... found while parsing a string at line 4 column
        // 0"). Escape control bytes that fall inside string literals before
        // parsing; whitespace between tokens outside strings is left untouched
        // since it's valid JSON as-is.
        match serde_json::from_slice::<RawAlignment>(&sanitize_json_control_characters(&raw_bytes))
        {
            Ok(raw_alignment) => {
                let _ = std::fs::remove_file(raw);
                return Ok(raw_alignment.words);
            }
            Err(error) => {
                last_error = format!("Qwen alignment output is invalid: {error}");
                if attempt < ALIGNMENT_WINDOW_PARSE_ATTEMPTS {
                    eprintln!(
                        "[uta-qwen-worker engine] alignment output was corrupt on attempt {attempt}/{ALIGNMENT_WINDOW_PARSE_ATTEMPTS}, retrying: {last_error}"
                    );
                }
            }
        }
    }
    Err(last_error)
}

/// Escapes raw ASCII control bytes (0x00-0x1F) that occur inside JSON string
/// literals, leaving insignificant whitespace between tokens untouched.
fn sanitize_json_control_characters(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut in_string = false;
    let mut escaped = false;
    for &byte in raw {
        if !in_string {
            if byte == b'"' {
                in_string = true;
            }
            out.push(byte);
            continue;
        }
        if escaped {
            out.push(byte);
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => {
                escaped = true;
                out.push(byte);
            }
            b'"' => {
                in_string = false;
                out.push(byte);
            }
            0x00..=0x1F => match byte {
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b'\t' => out.extend_from_slice(b"\\t"),
                other => out.extend_from_slice(format!("\\u{other:04x}").as_bytes()),
            },
            _ => out.push(byte),
        }
    }
    out
}

/// The pinned aligner is deterministic for a fixed audio/text/window-plan
/// input, so retrying an *unmodified* failing window plan against unchanged
/// audio bytes reproduces the identical failure (confirmed against a real
/// production song: identical window plan failed identically on every
/// retry, on both the Vulkan and CPU backends -- ruling out a GPU race and
/// pointing at the model's own measurement for that exact window). What
/// genuinely varies run to run is the upstream GPU separation stage:
/// repeated real production runs of the same source measured ~0.14% of PCM
/// samples differing by a few least-significant bits (real GPU
/// floating-point non-associativity in the separation compute), and for at
/// least one real seam that sits on a genuine ambiguity (audio spanning a
/// verbatim-repeated lyric block), that tiny separation-stage difference was
/// enough to flip the aligner's measurement from clean to badly
/// inconsistent. A retry therefore only has a real chance of succeeding if
/// it changes what the aligner actually sees: each retry shrinks the
/// window-count target, so windows are re-planned with different text/audio
/// boundaries and the model gets genuinely different local context, not a
/// bit-identical replay. This still re-measures for real and never
/// fabricates a result; every invariant (text, ordering, non-overlap) is
/// enforced identically regardless of which window plan produced it.
const ALIGN_SEAM_RETRY_ATTEMPTS: u32 = 3;

/// Whether a `run_align_once` failure came from one window's own measurement
/// being unusable -- as opposed to a structural/config error (bad input,
/// missing runtime, invalid manifest) that an unmodified retry cannot fix --
/// and is therefore worth a re-planned retry under `ALIGN_SEAM_RETRY_ATTEMPTS`.
///
/// Confirmed against the real production song this was diagnosed from: the
/// same window (a verbatim-repeated chorus block, deep in a ~5-minute track)
/// deterministically returned every word pinned to a single timestamp --
/// `normalize_alignment_words`'s "no measured boundaries" fail-closed path --
/// on *both* the Vulkan and CPU backends, so this is the aligner's own
/// measurement degrading for that window's specific text/audio boundaries,
/// the same class of run-to-run-sensitive failure the seam case already
/// retries for.
///
/// `execute_alignment_window`'s own bounded retry (`ALIGNMENT_WINDOW_PARSE_ATTEMPTS`)
/// already re-runs an unmodified failing window up to 3x on an output-parse
/// failure, on the theory that the write corruption is a transient race.
/// Confirmed against a real second production repro that it is not always:
/// the identical "key must be a string at line 10 column 5" corruption
/// recurred, at the identical byte offset, on a completely separate run of
/// the same window's unchanged audio/text -- i.e. deterministic for that
/// window's content, exactly like a measurement disagreement, just at the
/// JSON-write layer instead of the model's own output. Retrying unmodified
/// cannot fix a deterministic failure; only changing what content the
/// window actually contains can, which is exactly what the re-planned
/// retry below already does for measurement errors.
fn is_retryable_window_measurement_error(error: &str) -> bool {
    // `starts_with`, not exact equality: callers append which window/plan
    // failed for diagnostics, and that suffix must not change retry
    // classification.
    error.contains("could not be reconciled at the window seam")
        || error.starts_with("Qwen Forced Aligner returned no measured boundaries")
        || error.starts_with("Qwen alignment output has invalid word timing")
        || error.starts_with("Qwen alignment output has overlapping word timing")
        || error.starts_with("Qwen alignment output is invalid:")
}

/// Halved per retry attempt (110s -> 55s -> 27.5s), each roughly doubling the
/// window count so the seam under test is very unlikely to land on the exact
/// same text/audio boundary as the previous attempt.
fn align_window_target_seconds(attempt: u32) -> f64 {
    ALIGN_WINDOW_TARGET_SECONDS / 2f64.powi(attempt as i32)
}

/// Anchored grouping gets its own (smaller) target instead of reusing blind
/// planning's 110s: confirmed against a real song with a verbatim-repeated
/// chorus that a 110s anchored window -- correctly positioned by real
/// anchor times, but still spanning *both* occurrences of the repeated
/// block -- let the model confuse which occurrence it was hearing and
/// collapse a ten-line span into one degenerate measurement. A window
/// short enough to rarely span two repeats of the same block measured the
/// same real audio far more completely. Shrink by four rather than two on
/// anchored retries (50s -> 12.5s -> 3.125s): the final attempt becomes
/// effectively one line per window, which is the deterministic recovery for
/// a grouped result that merged a measured boundary across lyric lines.
const ALIGN_ANCHOR_WINDOW_TARGET_SECONDS: f64 = 50.0;

fn anchor_window_target_seconds(attempt: u32) -> f64 {
    ALIGN_ANCHOR_WINDOW_TARGET_SECONDS / 4f64.powi(attempt as i32)
}

fn anchor_margin_seconds(attempt: u32) -> f64 {
    if attempt + 1 == ALIGN_SEAM_RETRY_ATTEMPTS {
        0.0
    } else {
        ALIGN_ANCHOR_MARGIN_SECONDS / 2f64.powi(attempt as i32)
    }
}

fn run_align(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
    progress: &mut dyn FnMut(u64, u64, &'static str) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let anchored = config
        .get("line_anchors")
        .is_some_and(|value| !value.is_null());
    let mut last_error = String::new();
    for attempt in 0..ALIGN_SEAM_RETRY_ATTEMPTS {
        // Each attempt's real per-window progress is buffered rather than
        // forwarded live: a discarded (failed) attempt must never surface
        // progress that a subsequent attempt would then regress behind, and
        // the caller's monotonic-progress contract forbids that. Once an
        // attempt succeeds, its buffered sequence — the exact real
        // completions that produced the result — is replayed in order.
        let mut buffered: Vec<(u64, u64, &'static str)> = Vec::new();
        let mut buffer_progress = |completed: u64, total: u64, message: &'static str| {
            buffered.push((completed, total, message));
            Ok(())
        };
        match run_align_once(
            runtime,
            model,
            audio,
            output_dir,
            config,
            if anchored {
                anchor_window_target_seconds(attempt)
            } else {
                align_window_target_seconds(attempt)
            },
            if anchored {
                anchor_margin_seconds(attempt)
            } else {
                ALIGN_ANCHOR_MARGIN_SECONDS
            },
            &mut buffer_progress,
        ) {
            Ok(destination) => {
                for (completed, total, message) in buffered {
                    progress(completed, total, message)?;
                }
                return Ok(destination);
            }
            Err(error) if is_retryable_window_measurement_error(&error) => {
                last_error = error;
                if attempt + 1 < ALIGN_SEAM_RETRY_ATTEMPTS {
                    continue;
                }
            }
            // A later attempt's halved window target can ask for more
            // windows than this transcript has lyric units to split across
            // (`plan_alignment_segments`'s own floor). That is a planning
            // artifact of the retry shrink, not a real measurement, and
            // every subsequent attempt only shrinks further -- it can never
            // become feasible. Prefer a real prior attempt's measurement
            // error, which is the more accurate explanation of the actual
            // failure; only surface the planning error itself when it is
            // the very first attempt (the default window was already too
            // fine for this transcript, so there is no better error to show).
            Err(error)
                if !last_error.is_empty()
                    && error.starts_with("Qwen long-form alignment requires at least ") =>
            {
                return Err(last_error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error)
}

#[allow(clippy::too_many_arguments)]
fn run_align_once(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
    window_target_seconds: f64,
    anchor_margin_seconds: f64,
    progress: &mut dyn FnMut(u64, u64, &'static str) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let input = normalize_alignment_input(config)?;
    let destination = output_dir.join("qwen-alignment-evidence.json");
    let source_duration_seconds = audio::wav_duration_seconds(audio)?;
    let text_units = alignment_text_units(&input.transcript);
    let line_anchors = parsed_line_anchors(config)?;
    let plans = if let Some(anchors) = line_anchors.as_ref() {
        plan_alignment_segments_from_anchors(
            anchors,
            text_units.len(),
            source_duration_seconds,
            window_target_seconds,
            anchor_margin_seconds,
        )?
    } else {
        plan_alignment_segments(
            source_duration_seconds,
            text_units.len(),
            window_target_seconds,
        )?
    };
    let mut words: Vec<AlignmentWord> = Vec::new();
    let mut segment_evidence = Vec::with_capacity(plans.len());
    for plan in &plans {
        let context_text = text_units[plan.context_unit_start..plan.target_unit_end].join(" ");
        let prefix_text = text_units[plan.context_unit_start..plan.target_unit_start].join(" ");
        let target_text = text_units[plan.target_unit_start..plan.target_unit_end].join(" ");
        let window = if plans.len() == 1 {
            audio.to_path_buf()
        } else {
            audio::slice_wav(
                audio,
                output_dir,
                plan.index,
                plan.audio_start_seconds,
                plan.audio_end_seconds - plan.audio_start_seconds,
            )?
        };
        let raw_path = output_dir.join(format!("qwen-align-raw-{:03}.json", plan.index));
        let result = execute_alignment_window(
            runtime,
            model,
            &window,
            &raw_path,
            &context_text,
            input.runtime_language,
        );
        if plans.len() > 1 {
            let _ = std::fs::remove_file(&window);
        }
        let result = result?;
        progress(
            (plan.index + 1) as u64,
            plans.len() as u64,
            "Running pinned Qwen alignment windows",
        )?;
        let mut target_words = target_words_from_context(
            result,
            &context_text,
            compact_character_count(&prefix_text),
            compact_character_count(&target_text),
        )?;
        for word in &mut target_words {
            word.start += plan.audio_start_seconds;
            word.end += plan.audio_start_seconds;
        }
        let mut normalized = normalize_alignment_words(target_words).map_err(|error| {
            format!(
                "{error} (window {}: audio=[{:.2}s, {:.2}s] target=\"{target_text}\")",
                plan.index, plan.audio_start_seconds, plan.audio_end_seconds
            )
        })?;
        validate_alignment_measurement_resolution(&normalized).map_err(|error| {
            format!(
                "{error} (window {}: audio=[{:.2}s, {:.2}s] target=\"{target_text}\")",
                plan.index, plan.audio_start_seconds, plan.audio_end_seconds
            )
        })?;
        // Anchored planning has already proved a one-to-one mapping between
        // text units and caller lyric lines. Blind English planning instead
        // uses whitespace-delimited words as units, where a runtime boundary
        // spanning two ordinary words is not necessarily a line-assignment
        // failure and must not be rejected by this line-specific check.
        if line_anchors.is_some() {
            validate_alignment_unit_boundaries(
                &normalized,
                &text_units[plan.target_unit_start..plan.target_unit_end],
            )
            .map_err(|error| {
                format!(
                    "{error} (window {}: audio=[{:.2}s, {:.2}s] target=\"{target_text}\")",
                    plan.index, plan.audio_start_seconds, plan.audio_end_seconds
                )
            })?;
        }
        if plan.index > 0
            && let Some(next_word) = normalized.first_mut()
            && let Some(previous_word) = words.last_mut()
        {
            if let Err(error) =
                reconcile_alignment_seam(&plans[plan.index - 1], plan, previous_word, next_word)
            {
                return Err(format!(
                    "{error} (seam {}->{}, previous=\"{}\" [{:.2}s, {:.2}s], next=\"{}\" [{:.2}s, {:.2}s], next_anchor={:?})",
                    plan.index - 1,
                    plan.index,
                    previous_word.word,
                    previous_word.start,
                    previous_word.end,
                    next_word.word,
                    next_word.start,
                    next_word.end,
                    plan.anchor_start,
                ));
            }
        }
        segment_evidence.push(AlignmentSegmentEvidence {
            index: plan.index,
            audio_start_seconds: plan.audio_start_seconds,
            audio_end_seconds: plan.audio_end_seconds,
            context_unit_start: plan.context_unit_start,
            target_unit_start: plan.target_unit_start,
            target_unit_end: plan.target_unit_end,
            measured_units: normalized.len(),
        });
        words.extend(normalized);
    }
    if words
        .iter()
        .map(|word| word.word.as_str())
        .collect::<String>()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        != input
            .transcript
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    {
        return Err("Qwen long-form alignment did not preserve complete lyrics".to_string());
    }
    atomic_json(
        &destination,
        &AlignmentEvidence {
            schema_version: 2,
            model_id: WorkerKind::Align.model_id(),
            model_sha256: ALIGN_MODEL_SHA256,
            backend: "vulkan",
            runtime_manifest_sha256: &runtime.manifest_sha256,
            text_normalization_profile: ALIGN_TEXT_NORMALIZATION_PROFILE,
            language_normalization_profile: ALIGN_LANGUAGE_NORMALIZATION_PROFILE,
            alignment_semantics_profile: ALIGN_SEMANTICS_PROFILE,
            transcript: &input.transcript,
            language: input.language,
            runtime_language: input.runtime_language,
            long_input: AlignmentLongInputEvidence {
                policy: ALIGN_LONG_INPUT_POLICY,
                max_window_seconds: ALIGN_WINDOW_MAX_SECONDS,
                source_duration_seconds,
                text_unit_count: text_units.len(),
                segments: &segment_evidence,
            },
            words,
        },
    )?;
    Ok(destination)
}

#[cfg(test)]
mod tests;
