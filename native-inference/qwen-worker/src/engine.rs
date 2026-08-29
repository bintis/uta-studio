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
const ALIGN_CONTEXT_UNITS: usize = 3;
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
        });
    }
    Ok(plans)
}

fn compact_character_count(text: &str) -> usize {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .count()
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
    let overlap_ticks =
        ((previous_word.end - next_word.start) / ALIGN_TIMESTAMP_TICK_SECONDS).round() as i64;
    let split_ticks = overlap_ticks.max(0) / 2;
    let seam = next_word.start + split_ticks as f64 * ALIGN_TIMESTAMP_TICK_SECONDS;
    let window_lower = previous_plan
        .audio_start_seconds
        .max(next_plan.audio_start_seconds);
    let window_upper = previous_plan
        .audio_end_seconds
        .min(next_plan.audio_end_seconds);
    if seam - previous_word.start > EPSILON
        && next_word.end - seam > EPSILON
        && seam >= window_lower - EPSILON
        && seam <= window_upper + EPSILON
    {
        previous_word.end = seam;
        next_word.start = seam;
        return Ok(());
    }
    Err(
        "Qwen long-form alignment windows produced overlapping timing that could not be \
         reconciled at the window seam"
            .to_string(),
    )
}

fn execute_alignment_window(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    raw: &Path,
    text: &str,
    runtime_language: Option<&str>,
) -> Result<Vec<AlignmentWord>, String> {
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
    let raw_alignment: RawAlignment =
        serde_json::from_slice(&std::fs::read(raw).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Qwen alignment output is invalid: {error}"))?;
    let _ = std::fs::remove_file(raw);
    Ok(raw_alignment.words)
}

/// The pinned aligner is deterministic for a fixed audio/text/window-plan
/// input, so retrying an *unmodified* failing window plan against unchanged
/// audio bytes reproduces the identical failure (confirmed against a real
/// production song: identical window plan failed identically on every
/// retry). What genuinely varies run to run is the upstream GPU separation
/// stage: repeated real production runs of the same source measured ~0.14%
/// of PCM samples differing by a few least-significant bits (real GPU
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

/// Halved per retry attempt (110s -> 55s -> 27.5s), each roughly doubling the
/// window count so the seam under test is very unlikely to land on the exact
/// same text/audio boundary as the previous attempt.
fn align_window_target_seconds(attempt: u32) -> f64 {
    ALIGN_WINDOW_TARGET_SECONDS / 2f64.powi(attempt as i32)
}

fn run_align(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
    progress: &mut dyn FnMut(u64, u64, &'static str) -> Result<(), String>,
) -> Result<PathBuf, String> {
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
            align_window_target_seconds(attempt),
            &mut buffer_progress,
        ) {
            Ok(destination) => {
                for (completed, total, message) in buffered {
                    progress(completed, total, message)?;
                }
                return Ok(destination);
            }
            Err(error) if error.contains("could not be reconciled at the window seam") => {
                last_error = error;
                if attempt + 1 < ALIGN_SEAM_RETRY_ATTEMPTS {
                    continue;
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error)
}

fn run_align_once(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
    window_target_seconds: f64,
    progress: &mut dyn FnMut(u64, u64, &'static str) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let input = normalize_alignment_input(config)?;
    let destination = output_dir.join("qwen-alignment-evidence.json");
    let source_duration_seconds = audio::wav_duration_seconds(audio)?;
    let text_units = alignment_text_units(&input.transcript);
    let plans = plan_alignment_segments(
        source_duration_seconds,
        text_units.len(),
        window_target_seconds,
    )?;
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
        let mut normalized = normalize_alignment_words(target_words)?;
        if plan.index > 0
            && let Some(next_word) = normalized.first_mut()
            && let Some(previous_word) = words.last_mut()
        {
            reconcile_alignment_seam(&plans[plan.index - 1], plan, previous_word, next_word)?;
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
mod tests {
    use super::*;

    fn word(text: &str, start: f64, end: f64) -> AlignmentWord {
        AlignmentWord {
            word: text.to_string(),
            start,
            end,
        }
    }

    #[test]
    fn alignment_units_preserve_words_and_segment_unspaced_cjk() {
        assert_eq!(
            alignment_text_units("one two three"),
            vec!["one", "two", "three"]
        );
        assert_eq!(
            alignment_text_units("春天在哪里"),
            vec!["春", "天", "在", "哪", "里"]
        );
    }

    #[test]
    fn engine_output_reader_stops_at_the_combined_capture_limit() {
        let total = Arc::new(AtomicUsize::new(0));
        let oversized = Arc::new(AtomicBool::new(false));
        let bytes = read_bounded_engine_pipe(
            std::io::Cursor::new(vec![0_u8; MAX_ENGINE_OUTPUT_BYTES + 1]),
            Arc::clone(&total),
            Arc::clone(&oversized),
        );
        assert_eq!(bytes.len(), MAX_ENGINE_OUTPUT_BYTES);
        assert!(oversized.load(Ordering::SeqCst));
    }

    #[cfg(unix)]
    fn unix_process_is_running(pid: i32) -> bool {
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| stat.rsplit_once(") ").map(|(_, tail)| tail.to_string()))
            .and_then(|tail| tail.chars().next());
        if state == Some('Z') {
            return false;
        }
        // SAFETY: signal 0 only probes process existence/permission.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(unix)]
    #[test]
    fn run_engine_kills_descendants_that_outlive_the_direct_child() {
        let dir = std::env::temp_dir().join(format!("uta-qwen-engine-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pid_path = dir.join("descendant.pid");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("sleep 30 & echo $! > '{}'", pid_path.display()));
        let output = run_engine(&mut command).unwrap();
        assert!(output.status.success());
        let descendant_pid: i32 = std::fs::read_to_string(&pid_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while unix_process_is_running(descendant_pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !unix_process_is_running(descendant_pid),
            "a descendant left running by the pinned engine must not outlive it"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(windows)]
    fn windows_process_is_alive(pid: u32) -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: the queried handle is closed on every successful open and
        // the exit-code pointer refers to a live local `u32`.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let mut exit_code = 0_u32;
            let queried = GetExitCodeProcess(process, &mut exit_code) != 0;
            CloseHandle(process);
            queried && exit_code == STILL_ACTIVE as u32
        }
    }

    #[cfg(windows)]
    #[test]
    fn run_engine_kills_descendants_that_outlive_the_direct_child() {
        let dir = std::env::temp_dir().join(format!("uta-qwen-engine-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pid_path = dir.join("descendant.pid");
        let escaped_pid_path = pid_path.to_string_lossy().replace('\'', "''");
        let script = dir.join("spawn-descendant.ps1");
        std::fs::write(
            &script,
            format!(
                "$child = Start-Process -FilePath \"$env:SystemRoot\\System32\\ping.exe\" -ArgumentList \"-t\",\"127.0.0.1\" -PassThru -WindowStyle Hidden\nSet-Content -LiteralPath '{escaped_pid_path}' -Value $child.Id\n"
            ),
        )
        .unwrap();
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script);
        let output = run_engine(&mut command).unwrap();
        assert!(output.status.success());
        let descendant_pid: u32 = std::fs::read_to_string(&pid_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while windows_process_is_alive(descendant_pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !windows_process_is_alive(descendant_pid),
            "a descendant left running by the pinned engine must not outlive it"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn asr_runtime_arguments_preserve_detected_language_logging() {
        assert!(!ASR_RUNTIME_ARGS.contains(&"-q"));
        assert!(
            ASR_RUNTIME_ARGS
                .windows(2)
                .any(|pair| pair == ["--timestamps", "none"])
        );
    }

    #[test]
    fn asr_language_contract_rejects_explicit_hints() {
        assert!(validate_asr_language_policy(&serde_json::json!({})).is_ok());
        let error =
            validate_asr_language_policy(&serde_json::json!({"language": "ja"})).unwrap_err();
        assert!(error.contains("language contract v1"));
    }

    #[test]
    fn asr_evidence_uses_runtime_detected_language() {
        let (language, text) =
            parse_asr_result("<|ja|>歌詞です\n", b"", b"detected-language: ja\n").unwrap();
        assert_eq!(language, "ja");
        assert_eq!(text, "歌詞です");
        assert_eq!(
            language_from_log(b"Detected-Language : EN\n").unwrap(),
            Some("en".to_string())
        );
        assert!(parse_asr_result("歌詞です", b"", b"").is_err());
        assert!(parse_asr_result("<|ja|>歌詞です", b"detected-language: zh\n", b"").is_err());
        assert!(language_from_log(b"detected-language: en\ndetected language: ja\n").is_err());
    }

    #[test]
    fn asr_window_plan_is_bounded_contiguous_and_complete() {
        let plan = plan_asr_segments(305.813_333).unwrap();
        assert_eq!(plan.len(), 4);
        assert_eq!(plan[0], (0.0, 90.0));
        assert_eq!(plan[3], (270.0, 305.813_333));
        assert!(plan.windows(2).all(|pair| pair[0].1 == pair[1].0));
        assert!(
            plan.iter()
                .all(|(start, end)| end - start <= ASR_WINDOW_MAX_SECONDS)
        );
    }

    #[test]
    fn aligner_input_contract_normalizes_text_and_supported_language_codes() {
        let input = normalize_alignment_input(&serde_json::json!({
            "text": "  一行目\r\n二行目  ",
            "language": " JA "
        }))
        .unwrap();
        assert_eq!(
            input,
            NormalizedAlignmentInput {
                transcript: "一行目\n二行目".to_string(),
                language: Some("ja"),
                runtime_language: Some("japanese"),
            }
        );
        assert!(normalize_alignment_input(&serde_json::json!({"text": "  "})).is_err());
        assert!(
            normalize_alignment_input(&serde_json::json!({"text": "words", "language": "nl"}))
                .is_err()
        );
    }

    #[test]
    fn aligner_input_contract_preserves_inner_unicode_and_allows_no_language() {
        let input = normalize_alignment_input(&serde_json::json!({
            "text": "Ａ Ｂ。é"
        }))
        .unwrap();
        assert_eq!(input.transcript, "Ａ Ｂ。é");
        assert_eq!(input.language, None);
        assert_eq!(input.runtime_language, None);
    }

    #[test]
    fn zero_duration_unicode_pieces_join_measured_segments_without_new_timing() {
        let normalized = normalize_alignment_words(vec![
            word("土", 0.0, 1.28),
            word("地", 1.28, 1.28),
            word("の", 1.28, 1.44),
            word("そ", 1.44, 1.44),
            word("の", 1.44, 1.44),
            word("歌", 1.68, 1.92),
        ])
        .unwrap();
        assert_eq!(
            normalized,
            [
                word("土地", 0.0, 1.28),
                word("のその", 1.28, 1.44),
                word("歌", 1.68, 1.92)
            ]
        );
    }

    #[test]
    fn leading_zero_piece_joins_the_next_measured_segment() {
        let normalized =
            normalize_alignment_words(vec![word("前", 0.0, 0.0), word("語", 0.1, 0.5)]).unwrap();
        assert_eq!(normalized, [word("前語", 0.1, 0.5)]);
    }

    #[test]
    fn all_zero_or_overlapping_output_fails_closed() {
        assert!(normalize_alignment_words(vec![word("x", 0.0, 0.0)]).is_err());
        assert!(normalize_alignment_words(vec![word("a", 0.0, 1.0), word("b", 0.5, 1.5)]).is_err());
    }

    #[test]
    fn long_form_plan_is_bounded_complete_and_has_context() {
        let plan = plan_alignment_segments(305.813_375, 26, ALIGN_WINDOW_TARGET_SECONDS).unwrap();
        assert_eq!(plan.len(), 3);
        assert_eq!(
            plan.iter()
                .map(|segment| (segment.target_unit_start, segment.target_unit_end))
                .collect::<Vec<_>>(),
            [(0, 9), (9, 18), (18, 26)]
        );
        assert_eq!(plan[1].context_unit_start, 6);
        assert_eq!(plan[2].context_unit_start, 15);
        assert!(plan.iter().all(|segment| {
            segment.audio_end_seconds - segment.audio_start_seconds
                <= ALIGN_WINDOW_MAX_SECONDS + 0.001
        }));
        assert!(
            (plan[2].audio_start_seconds / ALIGN_TIMESTAMP_TICK_SECONDS - 2072.0).abs() < 0.001
        );
    }

    #[test]
    fn window_context_is_removed_without_dropping_target_text() {
        let selected = target_words_from_context(
            vec![
                word("anchor", 0.0, 0.4),
                word("歌", 0.5, 0.8),
                word("詞", 0.8, 1.0),
            ],
            "anchor 歌詞",
            "anchor".chars().count(),
            2,
        )
        .unwrap();
        assert_eq!(selected, [word("歌", 0.5, 0.8), word("詞", 0.8, 1.0)]);
        assert!(
            target_words_from_context(vec![word("anchortarget", 0.0, 1.0)], "anchor target", 6, 6,)
                .is_err()
        );
    }

    fn seam_plan(index: usize, audio_start: f64, audio_end: f64) -> AlignmentSegmentPlan {
        AlignmentSegmentPlan {
            index,
            audio_start_seconds: audio_start,
            audio_end_seconds: audio_end,
            context_unit_start: 0,
            target_unit_start: 0,
            target_unit_end: 1,
        }
    }

    #[test]
    fn seam_reconciliation_leaves_non_overlapping_windows_untouched() {
        let previous_plan = seam_plan(0, 0.0, 140.0);
        let next_plan = seam_plan(1, 60.0, 200.0);
        let mut previous = word("恋", 64.00, 65.20);
        let mut next = word("花", 65.50, 66.00);
        reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
        assert_eq!(previous, word("恋", 64.00, 65.20));
        assert_eq!(next, word("花", 65.50, 66.00));
    }

    #[test]
    fn seam_reconciliation_accepts_touching_boundary_unchanged() {
        let previous_plan = seam_plan(0, 0.0, 140.0);
        let next_plan = seam_plan(1, 60.0, 200.0);
        let mut previous = word("恋", 64.00, 65.20);
        let mut next = word("花", 65.20, 66.00);
        reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
        assert_eq!(previous.end, next.start);
        assert_eq!(previous.end, 65.20);
    }

    #[test]
    fn seam_reconciliation_splits_a_small_overlap_deterministically() {
        let previous_plan = seam_plan(0, 0.0, 140.0);
        let next_plan = seam_plan(1, 60.0, 200.0);
        let mut previous = word("恋", 64.00, 65.20);
        let mut next = word("花", 65.04, 66.00);
        reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
        // Overlap is [65.04, 65.20] = 2 ticks; split_ticks = 1 => seam = 65.04 + 0.08.
        assert!((previous.end - 65.12).abs() < 1e-9);
        assert_eq!(previous.end, next.start);
        assert!(previous.start < previous.end);
        assert!(next.start < next.end);
    }

    #[test]
    fn seam_reconciliation_resolves_a_sub_tick_overlap() {
        let previous_plan = seam_plan(0, 0.0, 140.0);
        let next_plan = seam_plan(1, 60.0, 200.0);
        let mut previous = word("恋", 64.00, 65.20);
        let mut next = word("花", 65.18, 66.00);
        reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
        assert_eq!(previous.end, next.start);
        assert!(previous.start < previous.end);
        assert!(next.start < next.end);
    }

    #[test]
    fn seam_reconciliation_resolves_a_larger_overlap_within_bounds() {
        let previous_plan = seam_plan(0, 0.0, 140.0);
        let next_plan = seam_plan(1, 60.0, 200.0);
        // Both words carry ample internal margin, so a 10-tick (0.8s) overlap
        // still has a valid deterministic seam strictly inside both words.
        let mut previous = word("恋", 60.00, 66.00);
        let mut next = word("花", 65.20, 70.00);
        reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
        // Overlap is [65.20, 66.00] = 10 ticks; split_ticks = 5 => seam = 65.20 + 0.40.
        assert!((previous.end - 65.60).abs() < 1e-9);
        assert_eq!(previous.end, next.start);
        assert!(previous.start < previous.end);
        assert!(next.start < next.end);
    }

    #[test]
    fn seam_reconciliation_fails_closed_when_one_word_would_collapse() {
        let previous_plan = seam_plan(0, 0.0, 140.0);
        let next_plan = seam_plan(1, 60.0, 200.0);
        // previous is exactly one tick wide, and the overlap consumes the
        // entire word: no seam can keep previous.start < previous.end.
        let mut previous = word("恋", 65.12, 65.20);
        let mut next = word("花", 65.04, 65.28);
        let previous_before = previous.clone();
        let next_before = next.clone();
        let error = reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next)
            .unwrap_err();
        assert!(error.contains("could not be reconciled"));
        // Fail-closed must never partially mutate either word.
        assert_eq!(previous, previous_before);
        assert_eq!(next, next_before);
    }

    #[test]
    fn seam_reconciliation_fails_closed_outside_shared_window_audio() {
        // The two windows share no audio (window ranges do not overlap), so
        // no candidate seam can be grounded in evidence either window
        // actually measured.
        let previous_plan = seam_plan(0, 0.0, 65.0);
        let next_plan = seam_plan(1, 65.5, 200.0);
        let mut previous = word("恋", 64.00, 66.00);
        let mut next = word("花", 65.00, 67.00);
        let previous_before = previous.clone();
        let next_before = next.clone();
        let error = reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next)
            .unwrap_err();
        assert!(error.contains("could not be reconciled"));
        assert_eq!(previous, previous_before);
        assert_eq!(next, next_before);
    }

    #[test]
    fn seam_reconciliation_is_deterministic_across_repeated_calls() {
        let previous_plan = seam_plan(0, 0.0, 140.0);
        let next_plan = seam_plan(1, 60.0, 200.0);
        let run = || {
            let mut previous = word("恋", 64.00, 65.20);
            let mut next = word("花", 65.04, 66.00);
            reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
            (previous, next)
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn asr_truncation_marker_matches_the_captured_production_failure() {
        let real = "pinned Qwen engine failed with exit status: 1: [debug] ggml_vulkan: \
            Found 1 Vulkan devices:\n[warn] qwen3_asr run: output truncated at 1024 tokens \
            \u{2014} decode reached the generation budget before end-of-stream; the \
            transcript may be incomplete.\n[info] timings: load=1825.62 ms";
        assert!(is_generation_budget_truncation(real));
        assert!(!is_generation_budget_truncation(
            "pinned Qwen engine failed with exit status: 1: some unrelated Vulkan error"
        ));
        assert!(!is_generation_budget_truncation(
            "Qwen ASR returned an empty transcript"
        ));
        assert!(!is_generation_budget_truncation(
            "could not start pinned Qwen engine: No such file or directory"
        ));
    }

    #[test]
    fn asr_split_midpoint_halves_until_the_floor_then_stops() {
        assert_eq!(asr_split_midpoint(0.0, 90.0, 0), Some(45.0));
        assert_eq!(asr_split_midpoint(0.0, 45.0, 1), Some(22.5));
        assert_eq!(asr_split_midpoint(0.0, 22.5, 2), Some(11.25));
        // Half of 11.25s is 5.625s, below the 10s floor.
        assert_eq!(asr_split_midpoint(0.0, 11.25, 3), None);
    }

    #[test]
    fn asr_split_midpoint_respects_the_max_depth_even_with_room_to_spare() {
        assert_eq!(asr_split_midpoint(0.0, 1000.0, ASR_MAX_SPLIT_DEPTH), None);
        assert!(asr_split_midpoint(0.0, 1000.0, ASR_MAX_SPLIT_DEPTH - 1).is_some());
    }

    // ---- End-to-end fixtures: a fake pinned-engine executable stands in for
    // the real Vulkan binary so `run_align`/`run_asr` exercise their real
    // window-stitching logic without GPU/model dependencies. The fake engine
    // only understands "-o <path>", copying the next canned response file
    // from its own control directory (one response per call, in order).
    //
    // The fake engine is a `#!/bin/sh` script, so everything from
    // here to the end of this module is Unix-only; the pure-function
    // seam/split-budget tests above already cover the same orchestration
    // logic (including the platform-portable `ProcessTreeGuard` machinery in
    // `run_engine`) without depending on a shell.

    #[cfg(unix)]
    fn synthetic_silent_wav(duration_seconds: f64) -> Vec<u8> {
        let byte_rate: u32 = 32_000;
        let data_bytes = (duration_seconds * f64::from(byte_rate)).round() as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&16_000_u32.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_bytes.to_le_bytes());
        wav.resize(wav.len() + data_bytes as usize, 0);
        wav
    }

    /// Write a fake engine whose control directory is baked into the script
    /// itself (never a shared process-wide env var), so concurrently running
    /// tests never interfere with each other's call counters.
    #[cfg(unix)]
    fn write_fake_engine(script_path: &Path, control: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let script = format!(
            "#!/bin/sh\nset -euo pipefail\ncontrol={control:?}\nout=\"\"\nprev=\"\"\nfor arg in \"$@\"; do\n  if [ \"$prev\" = \"-o\" ]; then out=\"$arg\"; fi\n  prev=\"$arg\"\ndone\ncount_file=\"$control/count\"\nn=0\nif [ -f \"$count_file\" ]; then n=$(cat \"$count_file\"); fi\necho $((n+1)) > \"$count_file\"\nif [ -f \"$control/truncate-$n\" ]; then\n  echo '[warn] qwen3_asr run: output truncated at 1024 tokens \u{2014} decode reached the generation budget before end-of-stream; the transcript may be incomplete.' >&2\n  exit 1\nfi\ncp \"$control/response-$n\" \"$out\"\n",
        );
        std::fs::write(script_path, script).unwrap();
        let mut permissions = std::fs::metadata(script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(script_path, permissions).unwrap();
    }

    #[cfg(unix)]
    fn reset_fake_engine_calls(control: &Path) {
        let _ = std::fs::remove_file(control.join("count"));
    }

    #[cfg(unix)]
    fn fixture_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uta-qwen-e2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("control")).unwrap();
        dir
    }

    #[cfg(unix)]
    fn assert_words_are_ordered_and_non_overlapping(words: &[serde_json::Value]) {
        let mut previous_end = 0.0_f64;
        for word in words {
            let start = word["start"].as_f64().unwrap();
            let end = word["end"].as_f64().unwrap();
            assert!(start < end, "word {word:?} is not positive-duration");
            assert!(
                start >= previous_end - 1e-9,
                "word {word:?} overlaps the previous word (previous_end={previous_end})"
            );
            previous_end = end;
        }
    }

    #[cfg(unix)]
    #[test]
    fn run_align_reconciles_a_real_seam_overlap_for_dense_cjk_lyrics() {
        let test_dir = fixture_dir("cjk");
        let control = test_dir.join("control");
        let transcript = "风吹沙蝶恋花千古佳话";
        let audio_path = test_dir.join("source.wav");
        std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

        // Window 0 owns chars[0..5]; context == target (no prefix).
        std::fs::write(
            control.join("response-0"),
            serde_json::to_vec(&serde_json::json!({"words": [
                {"word": "风", "start": 0.00, "end": 0.08},
                {"word": "吹", "start": 0.08, "end": 0.16},
                {"word": "沙", "start": 0.16, "end": 0.24},
                {"word": "蝶", "start": 0.24, "end": 0.32},
                {"word": "恋", "start": 64.00, "end": 65.20}
            ]}))
            .unwrap(),
        )
        .unwrap();
        // Window 1 context is chars[2..10]; chars[2..5] are discarded prefix,
        // chars[5..10] are the owned target. "花" is deliberately timed to
        // overlap the previous window's "恋" by 2 ticks after offsetting.
        std::fs::write(
            control.join("response-1"),
            serde_json::to_vec(&serde_json::json!({"words": [
                {"word": "沙", "start": 0.00, "end": 0.08},
                {"word": "蝶", "start": 0.08, "end": 0.16},
                {"word": "恋", "start": 0.16, "end": 0.24},
                {"word": "花", "start": 5.04, "end": 6.00},
                {"word": "千", "start": 6.00, "end": 6.08},
                {"word": "古", "start": 6.08, "end": 6.16},
                {"word": "佳", "start": 6.16, "end": 6.24},
                {"word": "话", "start": 6.24, "end": 6.32}
            ]}))
            .unwrap(),
        )
        .unwrap();
        let script_path = test_dir.join("engine.sh");
        write_fake_engine(&script_path, &control);
        let runtime = crate::runtime::ValidatedRuntime {
            engine: script_path,
            manifest_sha256: "0".repeat(64),
        };
        let config = serde_json::json!({"text": transcript});
        let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
        let destination = run_align(
            &runtime,
            Path::new("/fake-model.gguf"),
            &audio_path,
            &test_dir,
            &config,
            &mut progress,
        )
        .unwrap();
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
        let words = evidence["words"].as_array().unwrap();
        assert_eq!(words.len(), 10);
        let recovered: String = words
            .iter()
            .map(|word| word["word"].as_str().unwrap())
            .collect();
        assert_eq!(recovered, transcript);
        assert_words_are_ordered_and_non_overlapping(words);
        // The deliberate 2-tick seam overlap between "恋" and "花" reconciles
        // to the deterministic midpoint tick, 65.12s.
        assert!((words[4]["end"].as_f64().unwrap() - 65.12).abs() < 1e-9);
        assert!((words[5]["start"].as_f64().unwrap() - 65.12).abs() < 1e-9);

        std::fs::remove_dir_all(&test_dir).unwrap();
    }

    /// Builds a valid raw-engine response covering exactly one plan segment's
    /// context range, with tick-spaced sequential local timestamps starting
    /// at zero. Safe/non-conflicting by construction: real window seams are
    /// only ever tested by deliberately overriding specific entries.
    #[cfg(unix)]
    fn sequential_context_response(text_units: &[String], plan: &AlignmentSegmentPlan) -> Vec<u8> {
        let mut t = 0.0_f64;
        let words: Vec<serde_json::Value> = text_units
            [plan.context_unit_start..plan.target_unit_end]
            .iter()
            .map(|unit| {
                let entry = serde_json::json!({"word": unit, "start": t, "end": t + 0.08});
                t += 0.08;
                entry
            })
            .collect();
        serde_json::to_vec(&serde_json::json!({"words": words})).unwrap()
    }

    #[test]
    fn align_window_target_shrinks_each_attempt_so_retries_replan_windows() {
        assert_eq!(align_window_target_seconds(0), ALIGN_WINDOW_TARGET_SECONDS);
        assert_eq!(
            align_window_target_seconds(1),
            ALIGN_WINDOW_TARGET_SECONDS / 2.0
        );
        assert_eq!(
            align_window_target_seconds(2),
            ALIGN_WINDOW_TARGET_SECONDS / 4.0
        );
        // A shorter target plans strictly more windows for the same audio,
        // so a retry genuinely changes what the model is asked to align
        // rather than replaying an identical, deterministic computation.
        let attempt0 = plan_alignment_segments(200.0, 10, align_window_target_seconds(0)).unwrap();
        let attempt1 = plan_alignment_segments(200.0, 10, align_window_target_seconds(1)).unwrap();
        assert!(attempt1.len() > attempt0.len());
    }

    #[cfg(unix)]
    #[test]
    fn run_align_retries_a_real_measurement_after_a_transient_unresolvable_seam() {
        let test_dir = fixture_dir("retry-success");
        let control = test_dir.join("control");
        let transcript = "风吹沙蝶恋花千古佳话";
        let text_units = alignment_text_units(transcript);
        let audio_path = test_dir.join("source.wav");
        std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

        // Attempt 0 (2 windows, the default target): identical to the
        // deterministic-seam fixture's data, but with "花" placed so far
        // from "恋" that no seam can satisfy previous.start < seam.
        let attempt0 = plan_alignment_segments(200.0, 10, align_window_target_seconds(0)).unwrap();
        std::fs::write(
            control.join("response-0"),
            serde_json::to_vec(&serde_json::json!({"words": [
                {"word": "风", "start": 0.00, "end": 0.08},
                {"word": "吹", "start": 0.08, "end": 0.16},
                {"word": "沙", "start": 0.16, "end": 0.24},
                {"word": "蝶", "start": 0.24, "end": 0.32},
                {"word": "恋", "start": 64.00, "end": 65.20}
            ]}))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            control.join("response-1"),
            serde_json::to_vec(&serde_json::json!({"words": [
                {"word": "沙", "start": 0.00, "end": 0.08},
                {"word": "蝶", "start": 0.08, "end": 0.16},
                {"word": "恋", "start": 0.16, "end": 0.24},
                {"word": "花", "start": 0.08, "end": 0.90},
                {"word": "千", "start": 0.90, "end": 1.00},
                {"word": "古", "start": 1.00, "end": 1.10},
                {"word": "佳", "start": 1.10, "end": 1.20},
                {"word": "话", "start": 1.20, "end": 1.30}
            ]}))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            attempt0.len(),
            2,
            "test fixture assumes 2 windows at the default target"
        );

        // Attempt 1 (retry, a shorter target -> more/different windows):
        // every window is measured with simple sequential, non-conflicting
        // timestamps, so this attempt succeeds cleanly on its own merits.
        let attempt1 = plan_alignment_segments(200.0, 10, align_window_target_seconds(1)).unwrap();
        assert!(
            attempt1.len() > attempt0.len(),
            "the retry must actually replan with different windows"
        );
        for plan in &attempt1 {
            std::fs::write(
                control.join(format!("response-{}", attempt0.len() + plan.index)),
                sequential_context_response(&text_units, plan),
            )
            .unwrap();
        }

        let script_path = test_dir.join("engine.sh");
        write_fake_engine(&script_path, &control);
        let runtime = crate::runtime::ValidatedRuntime {
            engine: script_path,
            manifest_sha256: "0".repeat(64),
        };
        let config = serde_json::json!({"text": transcript});
        let mut progress_calls = Vec::new();
        let mut progress = |completed: u64, total: u64, _message: &'static str| {
            progress_calls.push((completed, total));
            Ok(())
        };
        let destination = run_align(
            &runtime,
            Path::new("/fake-model.gguf"),
            &audio_path,
            &test_dir,
            &config,
            &mut progress,
        )
        .unwrap();
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
        let words = evidence["words"].as_array().unwrap();
        assert_eq!(words.len(), 10);
        assert_words_are_ordered_and_non_overlapping(words);
        let recovered: String = words
            .iter()
            .map(|word| word["word"].as_str().unwrap())
            .collect();
        assert_eq!(recovered, transcript);
        // The failed attempt's progress is never surfaced: the caller only
        // ever sees the winning attempt's own monotonic, non-regressing,
        // complete (final == total) sequence.
        assert_eq!(progress_calls.len(), attempt1.len());
        assert!(progress_calls.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(
            progress_calls.last().unwrap().0,
            progress_calls.last().unwrap().1
        );
        // Exactly 2 attempts were made (attempt 0's 2 windows + attempt 1's
        // windows): bounded, not endless.
        assert_eq!(
            std::fs::read_to_string(control.join("count"))
                .unwrap()
                .trim(),
            (attempt0.len() + attempt1.len()).to_string()
        );

        std::fs::remove_dir_all(&test_dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn run_align_fails_closed_after_exhausting_seam_retries() {
        let test_dir = fixture_dir("retry-exhausted");
        let control = test_dir.join("control");
        let transcript = "风吹沙蝶恋花千古佳话";
        let text_units = alignment_text_units(transcript);
        let audio_path = test_dir.join("source.wav");
        std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

        // `run_align_once` bails out at the first unresolvable seam, so only
        // each attempt's first two windows are ever measured regardless of
        // how many windows that attempt's (shrinking-target) plan has. Window
        // 0 always starts at local/global time 0, so pinning its last target
        // word's *local* time near the window's own 140s ceiling pushes its
        // *global* end far past any later window's naturally small audio
        // start + local offset, reproducing an unresolvable gap under every
        // attempt's own text/audio boundaries without depending on exactly
        // where window 1 happens to start.
        let mut response_index = 0_usize;
        let mut total_windows = 0_usize;
        for attempt in 0..ALIGN_SEAM_RETRY_ATTEMPTS {
            let plan =
                plan_alignment_segments(200.0, 10, align_window_target_seconds(attempt)).unwrap();
            total_windows += plan.len();
            for (position, segment) in plan.iter().take(2).enumerate() {
                let mut response = sequential_context_response(&text_units, segment);
                if position == 0 {
                    // Force window 0's last (target) word far into its own
                    // window, well past where window 1's small, unmodified
                    // sequential timestamps can possibly reach it.
                    let mut value: serde_json::Value = serde_json::from_slice(&response).unwrap();
                    let words = value["words"].as_array_mut().unwrap();
                    let last = words.len() - 1;
                    words[last]["start"] = serde_json::json!(ALIGN_WINDOW_MAX_SECONDS - 0.5);
                    words[last]["end"] = serde_json::json!(ALIGN_WINDOW_MAX_SECONDS - 0.42);
                    response = serde_json::to_vec(&value).unwrap();
                }
                std::fs::write(control.join(format!("response-{response_index}")), response)
                    .unwrap();
                response_index += 1;
            }
        }
        let script_path = test_dir.join("engine.sh");
        write_fake_engine(&script_path, &control);
        let runtime = crate::runtime::ValidatedRuntime {
            engine: script_path,
            manifest_sha256: "0".repeat(64),
        };
        let config = serde_json::json!({"text": transcript});
        let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
        let error = run_align(
            &runtime,
            Path::new("/fake-model.gguf"),
            &audio_path,
            &test_dir,
            &config,
            &mut progress,
        )
        .unwrap_err();
        assert!(error.contains("could not be reconciled at the window seam"));
        assert!(
            total_windows > response_index,
            "the fixture must exercise fewer real calls than total planned \
             windows, proving the bail-out-at-first-seam behavior"
        );
        // Exactly ALIGN_SEAM_RETRY_ATTEMPTS attempts were made (2 real calls
        // each): bounded, not endless, and not silently retried forever.
        assert_eq!(
            std::fs::read_to_string(control.join("count"))
                .unwrap()
                .trim(),
            response_index.to_string()
        );

        std::fs::remove_dir_all(&test_dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn run_align_reconciles_a_seam_overlap_for_whitespace_lyrics_and_is_deterministic() {
        let test_dir = fixture_dir("latin");
        let control = test_dir.join("control");
        let transcript = "one two three four five six seven eight nine ten";
        let audio_path = test_dir.join("source.wav");
        std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

        std::fs::write(
            control.join("response-0"),
            serde_json::to_vec(&serde_json::json!({"words": [
                {"word": "one", "start": 0.00, "end": 0.32},
                {"word": "two", "start": 0.32, "end": 0.64},
                {"word": "three", "start": 0.64, "end": 0.96},
                {"word": "four", "start": 0.96, "end": 1.28},
                {"word": "five", "start": 64.00, "end": 65.20}
            ]}))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            control.join("response-1"),
            serde_json::to_vec(&serde_json::json!({"words": [
                {"word": "three", "start": 0.00, "end": 0.32},
                {"word": "four", "start": 0.32, "end": 0.64},
                {"word": "five", "start": 0.64, "end": 0.96},
                {"word": "six", "start": 5.04, "end": 6.00},
                {"word": "seven", "start": 6.00, "end": 6.32},
                {"word": "eight", "start": 6.32, "end": 6.64},
                {"word": "nine", "start": 6.64, "end": 6.96},
                {"word": "ten", "start": 6.96, "end": 7.28}
            ]}))
            .unwrap(),
        )
        .unwrap();
        let script_path = test_dir.join("engine.sh");
        write_fake_engine(&script_path, &control);
        let runtime = crate::runtime::ValidatedRuntime {
            engine: script_path,
            manifest_sha256: "0".repeat(64),
        };
        let config = serde_json::json!({"text": transcript});

        let run = || {
            reset_fake_engine_calls(&control);
            let mut progress_calls = Vec::new();
            let mut progress = |completed: u64, total: u64, _message: &'static str| {
                progress_calls.push((completed, total));
                Ok(())
            };
            let destination = run_align(
                &runtime,
                Path::new("/fake-model.gguf"),
                &audio_path,
                &test_dir,
                &config,
                &mut progress,
            )
            .unwrap();
            (std::fs::read(&destination).unwrap(), progress_calls)
        };

        let (first_bytes, first_progress) = run();
        let (second_bytes, second_progress) = run();
        assert_eq!(
            first_bytes, second_bytes,
            "repeated runs must be deterministic"
        );
        assert_eq!(first_progress, second_progress);
        assert_eq!(first_progress, vec![(1, 2), (2, 2)]);

        let evidence: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
        let words = evidence["words"].as_array().unwrap();
        assert_eq!(words.len(), 10);
        let recovered: String = words
            .iter()
            .map(|word| word["word"].as_str().unwrap())
            .collect();
        assert_eq!(
            recovered,
            transcript
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
        );
        assert_words_are_ordered_and_non_overlapping(words);
        assert!((words[4]["end"].as_f64().unwrap() - 65.12).abs() < 1e-9);
        assert!((words[5]["start"].as_f64().unwrap() - 65.12).abs() < 1e-9);

        std::fs::remove_dir_all(&test_dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn run_asr_recovers_from_truncation_by_splitting_the_offending_window() {
        let test_dir = fixture_dir("asr-retry");
        let control = test_dir.join("control");
        let audio_path = test_dir.join("source.wav");
        // 60s fits in a single top-level 90s-max plan window.
        std::fs::write(&audio_path, synthetic_silent_wav(60.0)).unwrap();
        // Call 0: the whole-file attempt truncates.
        std::fs::write(control.join("truncate-0"), "").unwrap();
        // Call 1: the [0,30) half succeeds.
        std::fs::write(control.join("response-1"), "<|zh|>chunk-a").unwrap();
        // Call 2: the [30,60) half succeeds.
        std::fs::write(control.join("response-2"), "<|zh|>chunk-b").unwrap();
        let script_path = test_dir.join("engine.sh");
        write_fake_engine(&script_path, &control);
        let runtime = crate::runtime::ValidatedRuntime {
            engine: script_path,
            manifest_sha256: "0".repeat(64),
        };
        let mut progress_calls = Vec::new();
        let mut progress = |completed: u64, total: u64, _message: &'static str| {
            progress_calls.push((completed, total));
            Ok(())
        };
        let destination = run_asr(
            &runtime,
            Path::new("/fake-model.gguf"),
            &audio_path,
            &test_dir,
            &serde_json::json!({}),
            &mut progress,
        )
        .unwrap();
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
        assert_eq!(evidence["text"], "chunk-a chunk-b");
        let segments = evidence["long_input"]["segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0]["audio_start_seconds"], 0.0);
        assert_eq!(segments[0]["audio_end_seconds"], 30.0);
        assert_eq!(segments[1]["audio_start_seconds"], 30.0);
        assert_eq!(segments[1]["audio_end_seconds"], 60.0);
        // Progress reports real audio-time coverage (ms), monotonically
        // reaching the true total despite the retry/split, never a guessed
        // window-count percentage.
        assert_eq!(progress_calls, vec![(30_000, 60_000), (60_000, 60_000)]);

        std::fs::remove_dir_all(&test_dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn run_asr_fails_closed_when_truncation_cannot_be_resolved_within_policy_bounds() {
        let test_dir = fixture_dir("asr-retry-limit");
        let control = test_dir.join("control");
        let audio_path = test_dir.join("source.wav");
        std::fs::write(&audio_path, synthetic_silent_wav(20.0)).unwrap();
        // Call 0: the whole-file attempt on [0,20) truncates, so it splits
        // to [0,10) and [10,20). Call 1: [0,10) truncates too; half of that
        // is 5s, below the 10s floor, so it cannot split again and must fail
        // closed immediately rather than retrying forever or ever reaching
        // the untried [10,20) half.
        std::fs::write(control.join("truncate-0"), "").unwrap();
        std::fs::write(control.join("truncate-1"), "").unwrap();
        let script_path = test_dir.join("engine.sh");
        write_fake_engine(&script_path, &control);
        let runtime = crate::runtime::ValidatedRuntime {
            engine: script_path,
            manifest_sha256: "0".repeat(64),
        };
        let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
        let error = run_asr(
            &runtime,
            Path::new("/fake-model.gguf"),
            &audio_path,
            &test_dir,
            &serde_json::json!({}),
            &mut progress,
        )
        .unwrap_err();
        assert!(error.contains("could not be split further within policy bounds"));
        // Exactly 2 calls were made (the untried [10,20) half is never
        // attempted once its sibling fails closed): no unbounded retry loop.
        assert_eq!(
            std::fs::read_to_string(control.join("count"))
                .unwrap()
                .trim(),
            "2"
        );

        std::fs::remove_dir_all(&test_dir).unwrap();
    }
}
