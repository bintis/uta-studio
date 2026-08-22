use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde::{Deserialize, Serialize};

use crate::WorkerKind;
use crate::runtime::{ValidatedRuntime, sha256};

const ASR_MODEL_SHA256: &str = "b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e";
const ALIGN_MODEL_SHA256: &str = "c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b";

#[derive(Serialize)]
struct TranscriptEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    model_sha256: &'a str,
    backend: &'a str,
    runtime_manifest_sha256: &'a str,
    language: Option<&'a str>,
    text: &'a str,
}

#[derive(Debug, Deserialize, Serialize)]
struct AlignmentWord {
    word: String,
    start: f64,
    end: f64,
}

#[derive(Deserialize)]
struct RawAlignment {
    words: Vec<AlignmentWord>,
}

#[derive(Serialize)]
struct AlignmentEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    model_sha256: &'a str,
    backend: &'a str,
    runtime_manifest_sha256: &'a str,
    transcript: &'a str,
    language: Option<&'a str>,
    words: Vec<AlignmentWord>,
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
    let expected = match kind {
        WorkerKind::Asr => ASR_MODEL_SHA256,
        WorkerKind::Align => ALIGN_MODEL_SHA256,
    };
    if sha256(&path)? != expected {
        return Err(format!("{} model hash mismatch", kind.model_id()));
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

fn run_asr(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
) -> Result<PathBuf, String> {
    let raw = output_dir.join("qwen-asr-transcript.txt");
    let destination = output_dir.join("qwen-asr-transcript-evidence.json");
    let language = config.get("language").and_then(|value| value.as_str());
    let mut command = Command::new(&runtime.engine);
    command
        .args(["-m"])
        .arg(model)
        .args([
            "--backend",
            "vulkan",
            "--device",
            "0",
            "--n-ctx",
            "0",
            "--timestamps",
            "none",
            "-q",
            "-o",
        ])
        .arg(&raw);
    if let Some(language) = language {
        command.args(["-l", language]);
    }
    command.arg(audio).env("GGML_VK_VISIBLE_DEVICES", "0");
    run_engine(&mut command)?;
    let text = std::fs::read_to_string(&raw).map_err(|error| error.to_string())?;
    let text = text.trim();
    let _ = std::fs::remove_file(&raw);
    if text.is_empty() {
        return Err("Qwen ASR returned an empty transcript".to_string());
    }
    atomic_json(
        &destination,
        &TranscriptEvidence {
            schema_version: 1,
            model_id: WorkerKind::Asr.model_id(),
            model_sha256: ASR_MODEL_SHA256,
            backend: "vulkan",
            runtime_manifest_sha256: &runtime.manifest_sha256,
            language,
            text,
        },
    )?;
    Ok(destination)
}

fn run_align(
    runtime: &ValidatedRuntime,
    model: &Path,
    audio: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
) -> Result<PathBuf, String> {
    let transcript = config
        .get("text")
        .and_then(|value| value.as_str())
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| "Qwen Forced Aligner requires config.text".to_string())?;
    let language = config.get("language").and_then(|value| value.as_str());
    let raw = output_dir.join("qwen-align-raw.json");
    let destination = output_dir.join("qwen-alignment-evidence.json");
    let mut command = Command::new(&runtime.engine);
    command
        .args(["-m"])
        .arg(model)
        .args(["-f"])
        .arg(audio)
        .args(["-o"])
        .arg(&raw)
        .args(["--align", "--text", transcript, "--no-timing"])
        .env("GGML_VK_VISIBLE_DEVICES", "0")
        .env("QWEN_USE_VRAM", "1")
        .env("QWEN_REQUIRE_GPU", "1");
    if let Some(language) = language {
        command.args(["-l", language]);
    }
    run_engine(&mut command)?;
    let raw_alignment: RawAlignment =
        serde_json::from_slice(&std::fs::read(&raw).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Qwen alignment output is invalid: {error}"))?;
    let _ = std::fs::remove_file(&raw);
    let mut previous = 0.0;
    for word in &raw_alignment.words {
        if word.word.is_empty()
            || !word.start.is_finite()
            || !word.end.is_finite()
            || word.start < previous
            || word.end < word.start
        {
            return Err("Qwen alignment output has invalid word timing".to_string());
        }
        previous = word.start;
    }
    if raw_alignment.words.is_empty() {
        return Err("Qwen Forced Aligner returned no boundaries".to_string());
    }
    atomic_json(
        &destination,
        &AlignmentEvidence {
            schema_version: 1,
            model_id: WorkerKind::Align.model_id(),
            model_sha256: ALIGN_MODEL_SHA256,
            backend: "vulkan",
            runtime_manifest_sha256: &runtime.manifest_sha256,
            transcript,
            language,
            words: raw_alignment.words,
        },
    )?;
    Ok(destination)
}
