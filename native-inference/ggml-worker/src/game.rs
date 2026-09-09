use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use uta_ggml_runtime::game::{Game, GameInferParams};
use uta_ggml_runtime::{DeviceDescriptor, DeviceKind, GgmlRuntime};

const SOURCE_COMMIT: &str = "475a8ee781fe8cca980b3b12fbe6c80c768a813a";

#[derive(Debug, Serialize)]
struct GameEvidence {
    schema_version: u32,
    model_id: &'static str,
    variant: String,
    source_commit: &'static str,
    model_gguf_size_bytes: u64,
    runtime_manifest_sha256: String,
    backend: String,
    semantic_output: &'static str,
    sample_rate: usize,
    timestep_ms: u32,
    d3pm_steps: usize,
    boundary_decision_threshold: f32,
    presence_decision_threshold: f32,
    notes: Vec<GameNoteEvidence>,
}

#[derive(Debug, Serialize)]
struct GameNoteEvidence {
    start: f64,
    duration: f64,
    midi: f32,
    voiced: bool,
}

pub fn infer(
    runtime: Arc<GgmlRuntime>,
    device: &DeviceDescriptor,
    model_path: &Path,
    input_path: &Path,
    runtime_manifest_sha256: &str,
    artifact_identity: &Value,
    destination: &Path,
    report: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    validate_artifact_identity(artifact_identity)?;
    let model = Game::load(runtime, device, model_path)?;
    let params = infer_params(artifact_identity)?;
    let variant = format!("GAME-1.0.3-{}-onnx", model.config().variant);
    let model_gguf_size_bytes = model_path
        .metadata()
        .map_err(|error| format!("GAME GGUF metadata is unavailable: {error}"))?
        .len();
    let output = model.process_wav_with_progress(input_path, &params, report)?;
    let evidence = GameEvidence {
        schema_version: 1,
        model_id: "game",
        variant,
        source_commit: SOURCE_COMMIT,
        model_gguf_size_bytes,
        runtime_manifest_sha256: runtime_manifest_sha256.to_string(),
        backend: match device.kind {
            DeviceKind::Cpu => "ggml_cpu".to_string(),
            DeviceKind::DiscreteGpu | DeviceKind::IntegratedGpu => "ggml_vulkan".to_string(),
        },
        semantic_output: "note_candidate_evidence",
        sample_rate: 44_100,
        timestep_ms: 10,
        d3pm_steps: params.d3pm_steps,
        boundary_decision_threshold: params.boundary_threshold,
        presence_decision_threshold: params.note_threshold,
        notes: output
            .notes
            .into_iter()
            .map(|note| GameNoteEvidence {
                start: f64::from(note.offset_seconds),
                duration: f64::from(note.duration_seconds),
                midi: note.pitch_midi,
                voiced: note.voiced,
            })
            .collect(),
    };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("could not create raw GAME evidence: {error}"))?;
    serde_json::to_writer(&mut file, &evidence)
        .map_err(|error| format!("could not encode raw GAME evidence: {error}"))?;
    use std::io::Write;
    file.write_all(b"\n")
        .map_err(|error| format!("could not finish raw GAME evidence: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not sync raw GAME evidence: {error}"))
}

pub fn publish(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err("GAME evidence target already exists".to_string());
    }
    let metadata = source
        .metadata()
        .map_err(|error| format!("GAME engine evidence is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 64 * 1024 * 1024 {
        return Err("GAME engine evidence size is invalid".to_string());
    }
    let value: Value = serde_json::from_slice(
        &std::fs::read(source)
            .map_err(|error| format!("could not read GAME engine evidence: {error}"))?,
    )
    .map_err(|error| format!("GAME engine evidence JSON is invalid: {error}"))?;
    if value.get("schema_version").and_then(Value::as_u64) != Some(1)
        || value.get("model_id").and_then(Value::as_str) != Some("game")
        || value.get("semantic_output").and_then(Value::as_str) != Some("note_candidate_evidence")
        || value.get("notes").and_then(Value::as_array).is_none()
    {
        return Err("GAME engine evidence identity is invalid".to_string());
    }
    std::fs::hard_link(source, destination)
        .map_err(|error| format!("could not atomically publish GAME evidence: {error}"))
}

fn infer_params(config: &Value) -> Result<GameInferParams, String> {
    let language = match config
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "en" => 1,
        "ja" => 2,
        "yue" => 3,
        "zh" => 4,
        _ => 0,
    };
    let known_boundaries = match config.get("known_boundaries_us") {
        None => Vec::new(),
        Some(Value::Array(values)) => {
            let mut frames = Vec::with_capacity(values.len());
            let mut previous = None;
            for value in values {
                let microseconds = value.as_u64().ok_or_else(|| {
                    "GAME known boundary must be an integer microsecond".to_string()
                })?;
                let frame = usize::try_from(
                    microseconds
                        .checked_add(5_000)
                        .ok_or_else(|| "GAME known boundary overflows".to_string())?
                        / 10_000,
                )
                .map_err(|_| "GAME known boundary exceeds this platform".to_string())?;
                if frame == 0 || previous == Some(frame) {
                    continue;
                }
                if previous.is_some_and(|previous| frame < previous) {
                    return Err("GAME known boundaries must be increasing".to_string());
                }
                frames.push(frame);
                previous = Some(frame);
            }
            frames
        }
        Some(_) => return Err("GAME known_boundaries_us must be an array".to_string()),
    };
    Ok(GameInferParams {
        language,
        known_boundaries,
        ..GameInferParams::default()
    })
}

fn validate_artifact_identity(identity: &Value) -> Result<(), String> {
    if identity.get("semantic_output").and_then(Value::as_str) != Some("note_candidate_evidence") {
        return Err("GAME artifact semantic output is invalid".to_string());
    }
    Ok(())
}
