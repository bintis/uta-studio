use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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
const ALIGN_TEXT_NORMALIZATION_PROFILE: &str = "qwen-align-text-preserve-v1";
const ALIGN_LANGUAGE_NORMALIZATION_PROFILE: &str = "qwen-align-language-v1";
const ALIGN_SEMANTICS_PROFILE: &str = "qwen-align-token-word-80ms-v1";
const ALIGN_LONG_INPUT_POLICY: &str = "qwen-align-windowed-v1";
const ALIGN_WINDOW_TARGET_SECONDS: f64 = 110.0;
const ALIGN_WINDOW_MAX_SECONDS: f64 = 140.0;
const ALIGN_TIMESTAMP_TICK_SECONDS: f64 = 0.08;
const ALIGN_CONTEXT_UNITS: usize = 3;
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

fn run_engine(command: &mut Command) -> Result<Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: this closure runs in the child after fork and only calls the
    // async-signal-safe prctl syscall. It ensures supervisor cancellation or a
    // worker crash cannot orphan a GPU engine process.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = command
        .output()
        .map_err(|error| format!("could not start pinned Qwen engine: {error}"))?;
    if output.stdout.len() + output.stderr.len() > 16 * 1024 * 1024 {
        return Err("Qwen engine log exceeded the bounded capture limit".to_string());
    }
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
) -> Result<PathBuf, String> {
    let model = model_path(kind, config)?;
    match kind {
        WorkerKind::Asr => run_asr(runtime, &model, audio, output_dir, config),
        WorkerKind::Align => run_align(runtime, &model, audio, output_dir, config),
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

fn run_asr(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
) -> Result<PathBuf, String> {
    validate_asr_language_policy(config)?;
    let destination = output_dir.join("qwen-asr-transcript-evidence.json");
    let source_duration_seconds = audio::wav_duration_seconds(audio)?;
    let plans = plan_asr_segments(source_duration_seconds)?;
    let mut segments = Vec::with_capacity(plans.len());
    let mut texts = Vec::with_capacity(plans.len());
    let mut language_weights = std::collections::BTreeMap::<String, usize>::new();
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
        let (language, text) = result?;
        let text_characters = compact_character_count(&text);
        *language_weights.entry(language.clone()).or_default() += text_characters;
        texts.push(text);
        segments.push(AsrSegmentEvidence {
            index,
            audio_start_seconds: start,
            audio_end_seconds: end,
            detected_language: language,
            text_characters,
        });
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

fn alignment_text_units(transcript: &str) -> Vec<&str> {
    transcript.split_whitespace().collect()
}

fn tick_floor(seconds: f64) -> f64 {
    (seconds / ALIGN_TIMESTAMP_TICK_SECONDS).floor() * ALIGN_TIMESTAMP_TICK_SECONDS
}

fn plan_alignment_segments(
    source_duration_seconds: f64,
    text_unit_count: usize,
) -> Result<Vec<AlignmentSegmentPlan>, String> {
    if !source_duration_seconds.is_finite() || source_duration_seconds <= 0.0 {
        return Err("Qwen alignment source duration is invalid".to_string());
    }
    let segment_count = (source_duration_seconds / ALIGN_WINDOW_TARGET_SECONDS)
        .ceil()
        .max(1.0) as usize;
    if segment_count > 1 && text_unit_count < segment_count {
        return Err(format!(
            "Qwen long-form alignment requires at least {segment_count} whitespace-delimited lyric units"
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

fn run_align(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
) -> Result<PathBuf, String> {
    let input = normalize_alignment_input(config)?;
    let destination = output_dir.join("qwen-alignment-evidence.json");
    let source_duration_seconds = audio::wav_duration_seconds(audio)?;
    let text_units = alignment_text_units(&input.transcript);
    let plans = plan_alignment_segments(source_duration_seconds, text_units.len())?;
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
        let mut target_words = target_words_from_context(
            result?,
            &context_text,
            compact_character_count(&prefix_text),
            compact_character_count(&target_text),
        )?;
        for word in &mut target_words {
            word.start += plan.audio_start_seconds;
            word.end += plan.audio_start_seconds;
        }
        let normalized = normalize_alignment_words(target_words)?;
        if words
            .last()
            .zip(normalized.first())
            .is_some_and(|(previous, next)| next.start < previous.end)
        {
            return Err("Qwen long-form alignment windows produced overlapping timing".to_string());
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
        let plan = plan_alignment_segments(305.813_375, 26).unwrap();
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
}
