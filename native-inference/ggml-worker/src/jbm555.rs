use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use uta_ggml_runtime::jbm555::{Jbm555, OFFSET_THRESHOLD, ONSET_THRESHOLD, SAMPLE_RATE};
use uta_ggml_runtime::{DeviceDescriptor, DeviceKind, GgmlRuntime};

#[derive(Serialize)]
struct Evidence<'a> {
    schema_version: u32,
    model_id: &'static str,
    upstream_revision: &'a str,
    checkpoint_identity: &'a str,
    config_identity: &'a str,
    conversion_identity: &'a str,
    model_generation: &'a str,
    runtime_identity: &'static str,
    backend: &'static str,
    source_start: u64,
    source_duration: u64,
    mix_audio_identity: &'a str,
    vocal_audio_identity: &'a str,
    separator_model_generation: &'a str,
    vocal_preparation_generation: &'a str,
    frontend_profile: &'static str,
    decode_profile: &'static str,
    onset_threshold: f32,
    offset_threshold: f32,
    notes: Vec<NoteEvidence>,
}

#[derive(Serialize)]
struct NoteEvidence {
    range: RangeEvidence,
    midi: u8,
    onset_score: f32,
    offset_score: Option<f32>,
    pitch_score: f32,
}

#[derive(Serialize)]
struct RangeEvidence {
    start: u64,
    end: u64,
}

pub fn infer(
    runtime: Arc<GgmlRuntime>,
    device: &DeviceDescriptor,
    model_path: &Path,
    mix_path: &Path,
    vocal_path: &Path,
    config: &Value,
    destination: &Path,
    report: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    if config.get("semantic_output").and_then(Value::as_str) != Some("note_candidate_evidence") {
        return Err("JBM555 artifact semantic output is invalid".to_string());
    }
    let model = Jbm555::load(runtime, device, model_path)?;
    let (notes, sample_count) = model.process_wavs(mix_path, vocal_path, report)?;
    let source_start = config
        .get("source_start")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let default_duration = u64::try_from(sample_count)
        .unwrap_or(u64::MAX)
        .saturating_mul(1_000_000)
        / SAMPLE_RATE as u64;
    let source_duration = config
        .get("source_duration")
        .and_then(Value::as_u64)
        .unwrap_or(default_duration);
    let source_end = source_start.saturating_add(source_duration);
    let notes = notes
        .into_iter()
        .filter_map(|note| {
            let start = note.range.start.saturating_add(source_start);
            let end = note.range.end.saturating_add(source_start).min(source_end);
            (start < end && start < source_end).then_some(NoteEvidence {
                range: RangeEvidence { start, end },
                midi: note.midi,
                onset_score: note.onset_score,
                offset_score: note.offset_score,
                pitch_score: note.pitch_score,
            })
        })
        .collect();
    let evidence = Evidence {
        schema_version: 1,
        model_id: "jbm555_cectc_80",
        upstream_revision: config_text(config, "upstream_revision", "jbm555-public"),
        checkpoint_identity: config_text(config, "checkpoint_identity", "jbm555_80"),
        config_identity: config_text(config, "config_identity", "cectc80-public"),
        conversion_identity: config_text(config, "conversion_identity", "gguf-f32-v1"),
        model_generation: config_text(config, "model_generation", "runtime-managed"),
        runtime_identity: "shared_ggml_vulkan",
        backend: match device.kind {
            DeviceKind::Cpu => "ggml_cpu",
            DeviceKind::DiscreteGpu | DeviceKind::IntegratedGpu => "ggml_vulkan",
        },
        source_start,
        source_duration,
        mix_audio_identity: config_text(config, "mix_audio_identity", "task-mix"),
        vocal_audio_identity: config_text(config, "vocal_audio_identity", "task-vocal"),
        separator_model_generation: config_text(config, "separator_model_generation", "leap-xe90"),
        vocal_preparation_generation: config_text(
            config,
            "vocal_preparation_generation",
            "native-44k1",
        ),
        frontend_profile: "jbm555-rust-logfft-44k1-hop1024-midi24-384x48-scales0.5-1-2-v1",
        decode_profile: "jbm555-cectc-onset0.32-offset0.70-v1",
        onset_threshold: ONSET_THRESHOLD,
        offset_threshold: OFFSET_THRESHOLD,
        notes,
    };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("could not create raw JBM555 evidence: {error}"))?;
    serde_json::to_writer(&mut file, &evidence)
        .map_err(|error| format!("could not encode raw JBM555 evidence: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("could not finish raw JBM555 evidence: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not sync raw JBM555 evidence: {error}"))
}

pub fn publish(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err("JBM555 evidence target already exists".to_string());
    }
    let metadata = source
        .metadata()
        .map_err(|error| format!("JBM555 engine evidence is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 64 * 1024 * 1024 {
        return Err("JBM555 engine evidence size is invalid".to_string());
    }
    let value: Value = serde_json::from_slice(
        &std::fs::read(source)
            .map_err(|error| format!("could not read JBM555 engine evidence: {error}"))?,
    )
    .map_err(|error| format!("JBM555 engine evidence JSON is invalid: {error}"))?;
    if value.get("schema_version").and_then(Value::as_u64) != Some(1)
        || value.get("model_id").and_then(Value::as_str) != Some("jbm555_cectc_80")
        || value.get("notes").and_then(Value::as_array).is_none()
    {
        return Err("JBM555 engine evidence identity is invalid".to_string());
    }
    std::fs::hard_link(source, destination)
        .map_err(|error| format!("could not atomically publish JBM555 evidence: {error}"))
}

fn config_text<'a>(config: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    config
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
}
