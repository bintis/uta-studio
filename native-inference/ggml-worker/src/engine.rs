use std::io::Write;
use std::path::{Path, PathBuf};

use uta_ggml_runtime::{DeviceDescriptor, DeviceKind, GgmlRuntime};

use crate::protocol::DeviceReport;
use crate::{audio, runtime};

pub struct PublishedOutput {
    pub artifact: &'static str,
    pub path: PathBuf,
    pub media_type: &'static str,
}

fn cleanup_inputs(primary: &Path, secondary: Option<&Path>) {
    let _ = std::fs::remove_file(primary);
    if let Some(secondary) = secondary {
        let _ = std::fs::remove_file(secondary);
    }
}

fn model_path(config: &serde_json::Value) -> Result<PathBuf, String> {
    config
        .get("model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "GGML task requires Runtime Manager-resolved config.model_path".to_string())
}

fn resolve_device_class(
    devices: &[DeviceDescriptor],
    device_class: &str,
) -> Result<DeviceDescriptor, String> {
    let expected = match device_class {
        "cpu" => DeviceKind::Cpu,
        "gpu" => DeviceKind::DiscreteGpu,
        "integrated_gpu" => DeviceKind::IntegratedGpu,
        _ => return Err(format!("unsupported GGML device class: {device_class}")),
    };
    devices
        .iter()
        .find(|device| device.kind == expected)
        .cloned()
        .ok_or_else(|| {
            let available = devices
                .iter()
                .map(|device| format!("{} ({:?})", device.description, device.kind))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "no GGML {device_class} device is available; found: {}",
                if available.is_empty() {
                    "none"
                } else {
                    &available
                }
            )
        })
}

fn same_device_name(left: &str, right: &str) -> bool {
    let tokens = |value: &str| {
        value
            .split(|character: char| !character.is_ascii_alphanumeric())
            .map(str::to_ascii_lowercase)
            .filter(|token| !token.is_empty() && token != "r" && token != "tm")
            .collect::<Vec<_>>()
    };
    let left = tokens(left);
    let right = tokens(right);
    !left.is_empty()
        && !right.is_empty()
        && (left.iter().all(|token| right.contains(token))
            || right.iter().all(|token| left.contains(token)))
}

/// Resolves the user preference without launching a helper. Explicit Vulkan
/// indices are checked by the read-only physical-device probe, then matched to
/// GGML's loaded Vulkan plugin descriptor. A logical device is created only
/// after this function returns.
/// Environment name the local GGML patch reads to decide whether an all-F32
/// matrix multiply may use the device's fast shader family. Absent is the
/// exact scalar path.
const F32_MATMUL_ENV: &str = "UTA_STUDIO_GGML_F32_MATMUL";

/// Models whose F32 x F32 matrix multiplies may be promoted to the device's
/// F16 shader family.
///
/// Promotion evaluates F32 operands after rounding them to F16. On the two
/// separators listed here the whole graph is a mask applied to a spectrogram,
/// and measurement against the CPU reference lane puts the promoted result at
/// 1.322e-3 relative RMS where the exact path is 2.256e-4 — the promoted
/// figure being what the scalar attention path shipped before the matrix
/// engine was reachable, or about 0.13% of signal, 58 dB down.
///
/// It is deliberately a list rather than a family test. FireRed decodes
/// greedily, so the same rounding does not move its output by a fraction of a
/// decibel, it changes which token is emitted; a model added to the catalog
/// later must be measured before it is added here.
fn f32_matmul_may_be_promoted(model_id: &str) -> bool {
    matches!(
        model_id,
        "bs_roformer_leap_xe90_vocals"
            | "bs_roformer_leap_xe90_instrumental"
            | "bs_polarformer_public_instrumental"
    )
}

/// Enumerates the machine's usable GGML devices through the packaged runtime.
///
/// Enumeration loads the shared libraries and reads backend metadata; it never
/// creates a logical device and never submits work, so it is safe to call on a
/// worker that has been given no task.
pub fn device_inventory() -> Result<Vec<DeviceReport>, String> {
    let validated = runtime::validate_runtime_libraries()?;
    let runtime = GgmlRuntime::load(&validated.library_dir)?;
    Ok(runtime
        .devices()?
        .into_iter()
        .map(|device| DeviceReport {
            ggml_index: device.ggml_index,
            name: device.name,
            description: device.description,
            kind: match device.kind {
                DeviceKind::Cpu => "cpu",
                DeviceKind::DiscreteGpu => "discrete_gpu",
                DeviceKind::IntegratedGpu => "integrated_gpu",
            }
            .to_string(),
        })
        .collect())
}

fn execution_device(
    config: &serde_json::Value,
    runtime: &GgmlRuntime,
) -> Result<DeviceDescriptor, String> {
    let devices = runtime.devices()?;
    if let Some(index) = config
        .get("vulkan_device")
        .and_then(serde_json::Value::as_u64)
    {
        let index = usize::try_from(index)
            .ok()
            .filter(|index| *index <= 255)
            .ok_or_else(|| "GGML Vulkan device index is invalid".to_string())?;
        let probe = uta_gpu_probes::probe_vulkan()?;
        let physical = probe
            .devices
            .get(index)
            .ok_or_else(|| "selected Vulkan physical device is unavailable".to_string())?;
        return devices
            .into_iter()
            .find(|device| {
                same_device_name(&device.name, &physical.name)
                    || same_device_name(&device.description, &physical.name)
            })
            .ok_or_else(|| {
                format!(
                    "selected Vulkan device {} is not exposed by packaged GGML",
                    physical.name
                )
            });
    }
    if let Some(class) = config
        .get("device_class")
        .and_then(serde_json::Value::as_str)
    {
        return resolve_device_class(&devices, class);
    }
    devices
        .into_iter()
        .find(|device| device.kind != DeviceKind::Cpu)
        .ok_or_else(|| "packaged GGML exposes no Vulkan GPU".to_string())
}

fn dual_separation_filenames(model_id: &str) -> Option<(&'static str, &'static str)> {
    match model_id {
        "bs_roformer_leap_xe90_vocals" | "bs_polarformer_public_instrumental" => {
            Some(("guide-vocals.flac", "instrumental.flac"))
        }
        "bs_roformer_leap_xe90_instrumental" => Some(("instrumental.flac", "guide-vocals.flac")),
        _ => None,
    }
}

#[allow(dead_code)]
fn dual_separation_model(model_id: &str) -> bool {
    dual_separation_filenames(model_id).is_some()
}

fn direct_instrumental_model(model_id: &str) -> bool {
    model_id == "bs_roformer_leap_xe90_instrumental"
}

fn is_game_model(model_id: &str) -> bool {
    runtime::game_variant(model_id).is_some()
}

fn backend_for_device(device: &DeviceDescriptor) -> &'static str {
    match device.kind {
        DeviceKind::Cpu => "ggml_cpu",
        DeviceKind::DiscreteGpu | DeviceKind::IntegratedGpu => "ggml_vulkan",
    }
}

fn validate_semantics(model_id: &str, config: &serde_json::Value) -> Result<(), String> {
    let backend = config
        .get("backend")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "GGML worker requires an explicit backend".to_string())?;
    let device_class = config
        .get("device_class")
        .and_then(serde_json::Value::as_str);
    if backend == "ggml_cpu" && config.get("vulkan_device").is_some() {
        return Err("experimental GGML CPU execution cannot select a Vulkan index".to_string());
    }
    match (backend, device_class) {
        ("ggml_cpu", Some("cpu")) => {}
        ("ggml_vulkan", Some("cpu")) => {
            return Err("GGML Vulkan requests cannot select the CPU device".to_string());
        }
        ("ggml_vulkan", _) => {}
        ("ggml_cpu", _) => {
            return Err("experimental GGML CPU execution must be selected explicitly".to_string());
        }
        _ => return Err(format!("unsupported GGML execution backend: {backend}")),
    }
    let semantic = config
        .get("semantic_output")
        .and_then(serde_json::Value::as_str);
    let expected = match model_id {
        "rmvpe" | "fcpe" => "pitch",
        "basic_pitch" => "note+onset+contour_activation",
        model if is_game_model(model) => "note_candidate_evidence",
        "jbm555_cectc_80" => "note_candidate_evidence",
        "stars" => "note+technique_evidence",
        "rosvot" => "note_candidate_evidence",
        "qwen3_forced_aligner_0_6b" => "alignment_evidence",
        "qwen3_asr_1_7b" | "firered_asr2_aed" => "transcript_evidence",
        "bs_roformer_leap_xe90_vocals" | "bs_polarformer_public_instrumental" => {
            "vocal+instrumental_residual"
        }
        "bs_roformer_leap_xe90_instrumental" => "instrumental+vocal_residual",
        "melband_roformer_denoise_aufr33" => "dry",
        "melband_roformer_dereverb_anvuew" => "noreverb",
        "melband_roformer_harmony" => "lead_vocal+backing_vocal_residual",
        _ => return Err(format!("model {model_id} has no GGML Vulkan executor")),
    };
    if semantic != Some(expected) {
        return Err(format!(
            "GGML {model_id} task requires semantic_output={expected}"
        ));
    }
    if model_id == "melband_roformer_harmony"
        && config
            .get("input_semantics")
            .and_then(serde_json::Value::as_str)
            != Some("all_vocals")
    {
        return Err("GGML Harmony requires explicit all_vocals input semantics".to_string());
    }
    Ok(())
}

fn output_name(model_id: &str) -> &'static str {
    match model_id {
        "melband_roformer_denoise_aufr33" => "clean-lead-vocal.flac",
        "melband_roformer_dereverb_anvuew" => "noreverb-vocal.flac",
        "melband_roformer_harmony" => "lead-vocal.flac",
        _ => unreachable!("dual separation and pitch outputs use dedicated publication paths"),
    }
}

fn artifact_name(model_id: &str) -> &'static str {
    match model_id {
        "melband_roformer_denoise_aufr33" => "clean_lead_vocal",
        "melband_roformer_dereverb_anvuew" => "dereverbed_vocal",
        "melband_roformer_harmony" => "lead_vocal",
        _ => unreachable!("dual separation and pitch outputs use dedicated publication paths"),
    }
}

const MAX_PITCH_EVIDENCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PITCH_FRAMES: usize = 4 * 60 * 60 * 100;

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawRmvpeEvidence {
    frames: Vec<RawRmvpeFrame>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawRmvpeFrame {
    time: f64,
    hz: f32,
    confidence: f32,
    voiced: bool,
}

fn write_raw_rmvpe_evidence(
    frames: Vec<uta_ggml_runtime::rmvpe::PitchFrame>,
    destination: &Path,
) -> Result<(), String> {
    let frames = frames
        .into_iter()
        .map(|frame| RawRmvpeFrame {
            time: frame.time,
            hz: frame.hz,
            confidence: frame.confidence,
            voiced: frame.voiced,
        })
        .collect();
    let evidence = RawRmvpeEvidence { frames };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("could not create raw RMVPE evidence: {error}"))?;
    serde_json::to_writer(&mut file, &evidence)
        .map_err(|error| format!("could not encode raw RMVPE evidence: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("could not finish raw RMVPE evidence: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not sync raw RMVPE evidence: {error}"))
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawFcpeEvidence {
    frames: Vec<RawFcpeFrame>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawFcpeFrame {
    time: f64,
    hz: Option<f32>,
}

fn write_raw_fcpe_evidence(
    frames: Vec<uta_ggml_runtime::fcpe::PitchFrame>,
    destination: &Path,
) -> Result<(), String> {
    let evidence = RawFcpeEvidence {
        frames: frames
            .into_iter()
            .map(|frame| RawFcpeFrame {
                time: frame.time,
                hz: frame.hz,
            })
            .collect(),
    };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("could not create raw FCPE evidence: {error}"))?;
    serde_json::to_writer(&mut file, &evidence)
        .map_err(|error| format!("could not encode raw FCPE evidence: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("could not finish raw FCPE evidence: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not sync raw FCPE evidence: {error}"))
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawBasicPitchEvidence {
    frames: Vec<RawBasicPitchFrame>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawBasicPitchFrame {
    time: f64,
    note_max: f32,
    onset_max: f32,
    contour_class: usize,
    contour_score: f32,
}

fn write_raw_basic_pitch_evidence(
    frames: Vec<uta_ggml_runtime::basic_pitch::ActivationFrame>,
    destination: &Path,
) -> Result<(), String> {
    let evidence = RawBasicPitchEvidence {
        frames: frames
            .into_iter()
            .map(|frame| RawBasicPitchFrame {
                time: frame.time,
                note_max: frame.note_max,
                onset_max: frame.onset_max,
                contour_class: frame.contour_class,
                contour_score: frame.contour_score,
            })
            .collect(),
    };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("could not create raw Basic Pitch evidence: {error}"))?;
    serde_json::to_writer(&mut file, &evidence)
        .map_err(|error| format!("could not encode raw Basic Pitch evidence: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("could not finish raw Basic Pitch evidence: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not sync raw Basic Pitch evidence: {error}"))
}

#[derive(serde::Serialize)]
struct RmvpeEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    source_model_sha256: &'a str,
    model_gguf_sha256: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    timeline_step_ms: u32,
    sample_rate: u32,
    frames: Vec<RawRmvpeFrame>,
}

fn publish_rmvpe_evidence(
    engine_output: &Path,
    destination: &Path,
    runtime_manifest_digest: &str,
    backend: &str,
) -> Result<(), String> {
    if destination.exists() {
        return Err("RMVPE evidence target already exists".to_string());
    }
    let metadata = engine_output
        .metadata()
        .map_err(|error| format!("RMVPE engine evidence is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_PITCH_EVIDENCE_BYTES {
        return Err("RMVPE engine evidence size is invalid".to_string());
    }
    let raw: RawRmvpeEvidence = serde_json::from_slice(
        &std::fs::read(engine_output)
            .map_err(|error| format!("could not read RMVPE engine evidence: {error}"))?,
    )
    .map_err(|error| format!("RMVPE engine evidence is invalid: {error}"))?;
    if raw.frames.is_empty() || raw.frames.len() > MAX_PITCH_FRAMES {
        return Err("RMVPE engine frame count is invalid".to_string());
    }
    for (index, frame) in raw.frames.iter().enumerate() {
        let expected_time = index as f64 * 0.01;
        if !frame.time.is_finite()
            || (frame.time - expected_time).abs() > 1.0e-6
            || !frame.hz.is_finite()
            || frame.hz <= 0.0
            || !frame.confidence.is_finite()
            || !(0.0..=1.0).contains(&frame.confidence)
            || frame.voiced != (frame.confidence >= 0.03)
        {
            return Err("RMVPE engine frames are invalid or off the 10 ms grid".to_string());
        }
    }
    let evidence = RmvpeEvidence {
        schema_version: 1,
        model_id: "rmvpe",
        source_model_sha256: runtime::RMVPE_SOURCE_SHA256,
        model_gguf_sha256: runtime::RMVPE_GGUF_SHA256,
        runtime_manifest_sha256: runtime_manifest_digest,
        backend,
        timeline_step_ms: 10,
        sample_rate: 16_000,
        frames: raw.frames,
    };
    let temporary = destination.with_extension("json.tmp");
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("could not create RMVPE evidence: {error}"))?;
        serde_json::to_writer(&mut file, &evidence)
            .map_err(|error| format!("could not encode RMVPE evidence: {error}"))?;
        file.write_all(b"\n")
            .map_err(|error| format!("could not finish RMVPE evidence: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("could not sync RMVPE evidence: {error}"))?;
        drop(file);
        std::fs::hard_link(&temporary, destination).map_err(|error| {
            format!("could not atomically publish RMVPE evidence without overwrite: {error}")
        })?;
        std::fs::remove_file(&temporary)
            .map_err(|error| format!("could not remove RMVPE temporary evidence: {error}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[derive(serde::Serialize)]
struct FcpeEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    model_gguf_size_bytes: u64,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    timeline_step_ms: u32,
    sample_rate: u32,
    window_samples: u32,
    window_hop_samples: u32,
    frames: Vec<RawFcpeFrame>,
}

fn publish_fcpe_evidence(
    engine_output: &Path,
    destination: &Path,
    model_size: u64,
    runtime_manifest_digest: &str,
    backend: &str,
) -> Result<(), String> {
    if destination.exists() {
        return Err("FCPE evidence target already exists".to_string());
    }
    let metadata = engine_output
        .metadata()
        .map_err(|error| format!("FCPE engine evidence is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_PITCH_EVIDENCE_BYTES {
        return Err("FCPE engine evidence size is invalid".to_string());
    }
    let raw: RawFcpeEvidence = serde_json::from_slice(
        &std::fs::read(engine_output)
            .map_err(|error| format!("could not read FCPE engine evidence: {error}"))?,
    )
    .map_err(|error| format!("FCPE engine evidence is invalid: {error}"))?;
    if raw.frames.is_empty() || raw.frames.len() > MAX_PITCH_FRAMES {
        return Err("FCPE engine frame count is invalid".to_string());
    }
    for (index, frame) in raw.frames.iter().enumerate() {
        let expected_time = index as f64 * 0.01;
        if !frame.time.is_finite()
            || (frame.time - expected_time).abs() > 1.0e-6
            || frame.hz.is_some_and(|hz| !hz.is_finite() || hz <= 0.0)
        {
            return Err("FCPE engine frames are invalid or off the 10 ms grid".to_string());
        }
    }
    let evidence = FcpeEvidence {
        schema_version: 1,
        model_id: "fcpe",
        model_gguf_size_bytes: model_size,
        runtime_manifest_sha256: runtime_manifest_digest,
        backend,
        timeline_step_ms: 10,
        sample_rate: 16_000,
        window_samples: 32_000,
        window_hop_samples: 32_000,
        frames: raw.frames,
    };
    let temporary = destination.with_extension("json.tmp");
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("could not create FCPE evidence: {error}"))?;
        serde_json::to_writer(&mut file, &evidence)
            .map_err(|error| format!("could not encode FCPE evidence: {error}"))?;
        file.write_all(b"\n")
            .map_err(|error| format!("could not finish FCPE evidence: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("could not sync FCPE evidence: {error}"))?;
        drop(file);
        std::fs::hard_link(&temporary, destination).map_err(|error| {
            format!("could not atomically publish FCPE evidence without overwrite: {error}")
        })?;
        std::fs::remove_file(&temporary)
            .map_err(|error| format!("could not remove FCPE temporary evidence: {error}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[derive(serde::Serialize)]
struct BasicPitchEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    model_gguf_size_bytes: u64,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    sample_rate: u32,
    window_samples: u32,
    window_hop_samples: u32,
    fft_hop_samples: u32,
    overlap_frames: u32,
    padding_samples: u32,
    frames_per_window: u32,
    owned_frames_per_window: u32,
    frames: Vec<RawBasicPitchFrame>,
}

fn publish_basic_pitch_evidence(
    engine_output: &Path,
    destination: &Path,
    model_size: u64,
    runtime_manifest_digest: &str,
    backend: &str,
) -> Result<(), String> {
    if destination.exists() {
        return Err("Basic Pitch evidence target already exists".to_string());
    }
    if !matches!(backend, "ggml_cpu" | "ggml_vulkan") {
        return Err("Basic Pitch evidence backend is invalid".to_string());
    }
    let metadata = engine_output
        .metadata()
        .map_err(|error| format!("Basic Pitch engine evidence is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_PITCH_EVIDENCE_BYTES {
        return Err("Basic Pitch engine evidence size is invalid".to_string());
    }
    let raw: RawBasicPitchEvidence = serde_json::from_slice(
        &std::fs::read(engine_output)
            .map_err(|error| format!("could not read Basic Pitch engine evidence: {error}"))?,
    )
    .map_err(|error| format!("Basic Pitch engine evidence is invalid: {error}"))?;
    if raw.frames.is_empty() || raw.frames.len() > MAX_PITCH_FRAMES {
        return Err("Basic Pitch engine frame count is invalid".to_string());
    }
    for (index, frame) in raw.frames.iter().enumerate() {
        let expected_time = index as f64 * 256.0 / 22_050.0;
        if !frame.time.is_finite()
            || (frame.time - expected_time).abs() > 1.0e-6
            || !frame.note_max.is_finite()
            || !(0.0..=1.0).contains(&frame.note_max)
            || !frame.onset_max.is_finite()
            || !(0.0..=1.0).contains(&frame.onset_max)
            || frame.contour_class >= 264
            || !frame.contour_score.is_finite()
            || !(0.0..=1.0).contains(&frame.contour_score)
        {
            return Err(
                "Basic Pitch engine frames are invalid or off the 256-sample grid".to_string(),
            );
        }
    }
    let evidence = BasicPitchEvidence {
        schema_version: 1,
        model_id: "basic_pitch",
        model_gguf_size_bytes: model_size,
        runtime_manifest_sha256: runtime_manifest_digest,
        backend,
        sample_rate: 22_050,
        window_samples: 43_844,
        window_hop_samples: 36_164,
        fft_hop_samples: 256,
        overlap_frames: 30,
        padding_samples: 3_840,
        frames_per_window: 172,
        owned_frames_per_window: 142,
        frames: raw.frames,
    };
    let temporary = destination.with_extension("json.tmp");
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("could not create Basic Pitch evidence: {error}"))?;
        serde_json::to_writer(&mut file, &evidence)
            .map_err(|error| format!("could not encode Basic Pitch evidence: {error}"))?;
        file.write_all(b"\n")
            .map_err(|error| format!("could not finish Basic Pitch evidence: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("could not sync Basic Pitch evidence: {error}"))?;
        drop(file);
        std::fs::hard_link(&temporary, destination).map_err(|error| {
            format!("could not atomically publish Basic Pitch evidence without overwrite: {error}")
        })?;
        std::fs::remove_file(&temporary)
            .map_err(|error| format!("could not remove Basic Pitch temporary evidence: {error}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub fn run(
    task_id: &str,
    model_id: &str,
    source: &Path,
    secondary_source: Option<&Path>,
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &'static str, Option<(u64, u64)>),
) -> Result<Vec<PublishedOutput>, String> {
    validate_semantics(model_id, config)?;
    progress(0.02, "Validating pinned GGML Vulkan runtime", None);
    let validated_runtime = runtime::validate_runtime(model_id)?;
    progress(0.05, "Validating GGUF model structure", None);
    let model = runtime::validate_model(model_id, &model_path(config)?, config)?;
    let model_size = model
        .metadata()
        .map_err(|error| format!("GGUF model metadata is unavailable: {error}"))?
        .len();
    let pitch_mode = matches!(model_id, "rmvpe" | "fcpe");
    let basic_pitch_mode = model_id == "basic_pitch";
    let game_mode = is_game_model(model_id);
    let jbm555_mode = model_id == "jbm555_cectc_80";
    let stars_mode = model_id == "stars";
    let rosvot_mode = model_id == "rosvot";
    let qwen_aligner_mode = model_id == "qwen3_forced_aligner_0_6b";
    let qwen_asr_mode = model_id == "qwen3_asr_1_7b";
    let firered_mode = model_id == "firered_asr2_aed";
    let json_evidence_mode = pitch_mode
        || basic_pitch_mode
        || game_mode
        || jbm555_mode
        || stars_mode
        || rosvot_mode
        || qwen_aligner_mode
        || qwen_asr_mode
        || firered_mode;
    let (input, secondary_input) = if jbm555_mode {
        let vocal = secondary_source.ok_or_else(|| {
            "JBM555 requires exactly two input artifacts: original mix and prepared vocal"
                .to_string()
        })?;
        let [mix_input, vocal_input] =
            audio::decode_jbm555_wavs(source, vocal, output_dir, task_id)?;
        (mix_input, Some(vocal_input))
    } else if stars_mode || rosvot_mode {
        secondary_source.ok_or_else(|| {
            "STARS and ROSVOT require shared RMVPE evidence as their second input".to_string()
        })?;
        (audio::decode_stars_wav(source, output_dir, task_id)?, None)
    } else {
        let input = if basic_pitch_mode {
            audio::decode_basic_pitch_wav(source, output_dir, task_id)?
        } else if game_mode {
            audio::decode_game_wav(source, output_dir, task_id)?
        } else if pitch_mode || qwen_aligner_mode || qwen_asr_mode || firered_mode {
            audio::decode_mono_wav(source, output_dir, task_id)?
        } else {
            audio::decode_stereo_wav(source, output_dir, task_id)?
        };
        (input, None)
    };
    let engine_output = output_dir.join(format!(
        "{task_id}-ggml-engine.{}",
        if json_evidence_mode { "json" } else { "wav" }
    ));
    if engine_output.exists() {
        cleanup_inputs(&input, secondary_input.as_deref());
        return Err("GGML engine output target already exists".to_string());
    }
    progress(0.1, "Loading GGML shared libraries from Rust", None);
    // The packaged GGML reads this once, when it builds the device's shader
    // pipelines, which happens inside the backend creation below. A worker
    // process runs one model, so the choice is per model.
    //
    // SAFETY: this process is single-threaded until the first GGML call, which
    // is the load immediately below, and nothing else reads the environment
    // before then.
    unsafe {
        if f32_matmul_may_be_promoted(model_id) {
            std::env::set_var(F32_MATMUL_ENV, "promote");
        } else {
            std::env::remove_var(F32_MATMUL_ENV);
        }
    }
    let ggml_runtime = match GgmlRuntime::load(&validated_runtime.library_dir) {
        Ok(runtime) => runtime,
        Err(error) => {
            cleanup_inputs(&input, secondary_input.as_deref());
            return Err(error);
        }
    };
    let device = match execution_device(config, &ggml_runtime) {
        Ok(device) => device,
        Err(error) => {
            cleanup_inputs(&input, secondary_input.as_deref());
            return Err(error);
        }
    };
    let mut last_units = None;
    let mut work_error = None;
    let mut report_units = |completed: u64, total: u64| {
        if total == 0
            || completed == 0
            || completed > total
            || last_units.is_some_and(|(previous, previous_total)| {
                total != previous_total || completed <= previous
            })
        {
            work_error = Some("GGML work units changed identity or regressed".to_string());
            return;
        }
        last_units = Some((completed, total));
        progress(
            0.1 + completed as f32 / total as f32 * 0.8,
            "Running measured Rust-to-GGML work unit",
            Some((completed, total)),
        );
    };
    let inference_result = if model_id == "rmvpe" {
        let rmvpe = uta_ggml_runtime::rmvpe::Rmvpe::load(ggml_runtime, &device, &model);
        rmvpe.and_then(|rmvpe| {
            let frames = rmvpe.process_wav(&input, &mut report_units)?;
            write_raw_rmvpe_evidence(frames, &engine_output)
        })
    } else if model_id == "fcpe" {
        let fcpe = uta_ggml_runtime::fcpe::Fcpe::load(ggml_runtime, &device, &model);
        fcpe.and_then(|fcpe| {
            let frames = fcpe.process_wav(&input, &mut report_units)?;
            write_raw_fcpe_evidence(frames, &engine_output)
        })
    } else if model_id == "basic_pitch" {
        let basic_pitch =
            uta_ggml_runtime::basic_pitch::BasicPitch::load(ggml_runtime, &device, &model);
        basic_pitch.and_then(|basic_pitch| {
            let frames = basic_pitch.process_wav(&input, &mut report_units)?;
            write_raw_basic_pitch_evidence(frames, &engine_output)
        })
    } else if game_mode {
        crate::game::infer(
            ggml_runtime,
            &device,
            &model,
            &input,
            &validated_runtime.manifest_content_digest,
            config,
            &engine_output,
            &mut report_units,
        )
    } else if jbm555_mode {
        crate::jbm555::infer(
            ggml_runtime,
            &device,
            &model,
            &input,
            secondary_input
                .as_deref()
                .expect("JBM555 secondary input was validated before execution"),
            config,
            &engine_output,
            &mut report_units,
        )
    } else if model_id == "stars" {
        crate::stars::infer(
            ggml_runtime,
            &device,
            &model,
            &input,
            secondary_source.expect("STARS shared RMVPE input was validated before execution"),
            &validated_runtime.manifest_content_digest,
            backend_for_device(&device),
            config,
            &engine_output,
            &mut report_units,
        )
    } else if model_id == "rosvot" {
        crate::rosvot::infer(
            ggml_runtime,
            &device,
            &model,
            &input,
            secondary_source.expect("ROSVOT shared RMVPE input was validated before execution"),
            &validated_runtime.manifest_content_digest,
            backend_for_device(&device),
            config,
            &engine_output,
            &mut report_units,
        )
    } else if model_id == "qwen3_forced_aligner_0_6b" {
        let backend = backend_for_device(&device);
        crate::qwen::infer(
            ggml_runtime,
            &device,
            &model,
            &input,
            &validated_runtime.manifest_content_digest,
            backend,
            config,
            &engine_output,
            &mut report_units,
        )
    } else if model_id == "qwen3_asr_1_7b" {
        let backend = backend_for_device(&device);
        crate::qwen_asr::infer(
            ggml_runtime,
            &device,
            &model,
            &input,
            &validated_runtime.manifest_content_digest,
            backend,
            config,
            &engine_output,
            &mut report_units,
        )
    } else if firered_mode {
        let backend = backend_for_device(&device);
        crate::firered::infer(
            ggml_runtime,
            &device,
            &model,
            &input,
            &validated_runtime.manifest_content_digest,
            backend,
            config,
            &engine_output,
            &mut report_units,
        )
    } else {
        let roformer = uta_ggml_runtime::roformer::Roformer::load(ggml_runtime, &device, &model);
        roformer.and_then(|mut roformer| {
            roformer.process_wav(&input, &engine_output, &mut report_units)
        })
    };
    drop(report_units);
    if let Err(error) = inference_result {
        cleanup_inputs(&input, secondary_input.as_deref());
        let _ = std::fs::remove_file(&engine_output);
        return Err(error);
    }
    if let Some(error) = work_error {
        cleanup_inputs(&input, secondary_input.as_deref());
        let _ = std::fs::remove_file(&engine_output);
        return Err(error);
    }
    if !engine_output.is_file() || last_units.is_none_or(|(completed, total)| completed != total) {
        cleanup_inputs(&input, secondary_input.as_deref());
        let _ = std::fs::remove_file(&engine_output);
        return Err("Rust GGML execution did not complete its measured route".to_string());
    }

    if json_evidence_mode {
        let backend = match device.kind {
            DeviceKind::Cpu => "ggml_cpu",
            DeviceKind::DiscreteGpu | DeviceKind::IntegratedGpu => "ggml_vulkan",
        };
        let (destination, artifact, result) = if model_id == "rmvpe" {
            progress(0.92, "Validating and publishing RMVPE pitch evidence", None);
            let destination = output_dir.join("rmvpe-pitch-evidence.json");
            let result = publish_rmvpe_evidence(
                &engine_output,
                &destination,
                &validated_runtime.manifest_content_digest,
                backend,
            );
            (destination, "pitch_evidence", result)
        } else if model_id == "fcpe" {
            progress(0.92, "Validating and publishing FCPE pitch evidence", None);
            let destination = output_dir.join("fcpe-pitch-evidence.json");
            let result = publish_fcpe_evidence(
                &engine_output,
                &destination,
                model_size,
                &validated_runtime.manifest_content_digest,
                backend,
            );
            (destination, "pitch_evidence", result)
        } else if model_id == "basic_pitch" {
            progress(
                0.92,
                "Validating and publishing Basic Pitch activation evidence",
                None,
            );
            let destination = output_dir.join("basic-pitch-activation-evidence.json");
            let result = publish_basic_pitch_evidence(
                &engine_output,
                &destination,
                model_size,
                &validated_runtime.manifest_content_digest,
                backend,
            );
            (destination, "basic_pitch_evidence", result)
        } else if game_mode {
            progress(0.92, "Validating and publishing GAME note evidence", None);
            let destination = output_dir.join("game-note-evidence.json");
            let result = crate::game::publish(&engine_output, &destination);
            (destination, "game_evidence", result)
        } else if jbm555_mode {
            progress(0.92, "Validating and publishing JBM555 note evidence", None);
            let destination = output_dir.join("jbm555-note-evidence.json");
            let result = crate::jbm555::publish(&engine_output, &destination);
            (destination, "jbm555_evidence", result)
        } else if model_id == "stars" {
            progress(0.92, "Validating and publishing STARS note evidence", None);
            let destination = output_dir.join("stars-note-evidence.json");
            let result = crate::stars::publish(&engine_output, &destination);
            (destination, "stars_evidence", result)
        } else if model_id == "rosvot" {
            progress(0.92, "Validating and publishing ROSVOT note evidence", None);
            let destination = output_dir.join("rosvot-note-evidence.json");
            let result = crate::rosvot::publish(&engine_output, &destination);
            (destination, "rosvot_evidence", result)
        } else if qwen_aligner_mode {
            progress(
                0.92,
                "Validating and publishing Qwen alignment evidence",
                None,
            );
            let destination = output_dir.join("qwen-alignment-evidence.json");
            let result = crate::qwen::publish(&engine_output, &destination);
            (destination, "alignment_evidence", result)
        } else if qwen_asr_mode {
            progress(
                0.92,
                "Validating and publishing Qwen transcript evidence",
                None,
            );
            let destination = output_dir.join("qwen-transcript-evidence.json");
            let result = crate::qwen_asr::publish(&engine_output, &destination);
            (destination, "transcript_evidence", result)
        } else {
            progress(
                0.92,
                "Validating and publishing FireRed transcript evidence",
                None,
            );
            let destination = output_dir.join("firered-transcript-evidence.json");
            let result = crate::firered::publish(&engine_output, &destination);
            (destination, "transcript_evidence", result)
        };
        let result = result.map(|()| {
            vec![PublishedOutput {
                artifact,
                path: destination.clone(),
                media_type: "application/json",
            }]
        });
        cleanup_inputs(&input, secondary_input.as_deref());
        let _ = std::fs::remove_file(&engine_output);
        // Each publisher owns and cleans only its unique temporary. A failed
        // no-replace publication must never remove a destination created by
        // another task between validation and publication.
        progress(1.0, "GGML inference complete", None);
        return result;
    }

    progress(0.92, "Atomically encoding lossless GGML output", None);
    let dual_filenames = dual_separation_filenames(model_id);
    let destination = output_dir.join(
        dual_filenames
            .map(|(direct, _)| direct)
            .unwrap_or_else(|| output_name(model_id)),
    );
    let result = (|| {
        let mut published = if let Some((_, residual_filename)) = dual_filenames {
            let residual = output_dir.join(residual_filename);
            audio::encode_flac(&engine_output, &destination)?;
            audio::encode_residual_flac(
                &input,
                &engine_output,
                &residual,
                if direct_instrumental_model(model_id) {
                    "GGML vocal residual"
                } else {
                    "GGML instrumental residual"
                },
            )?;
            let (guide_vocals, instrumental) = if direct_instrumental_model(model_id) {
                (residual, destination.clone())
            } else {
                (destination.clone(), residual)
            };
            vec![
                PublishedOutput {
                    artifact: "guide_vocals",
                    path: guide_vocals,
                    media_type: "audio/flac",
                },
                PublishedOutput {
                    artifact: "instrumental",
                    path: instrumental,
                    media_type: "audio/flac",
                },
            ]
        } else {
            audio::encode_flac(&engine_output, &destination)?;
            vec![PublishedOutput {
                artifact: artifact_name(model_id),
                path: destination.clone(),
                media_type: "audio/flac",
            }]
        };
        if model_id == "melband_roformer_harmony" {
            let residual = output_dir.join("vocal-residual.flac");
            audio::encode_vocal_residual_flac(&input, &engine_output, &residual)?;
            published.push(PublishedOutput {
                artifact: "vocal_residual",
                path: residual,
                media_type: "audio/flac",
            });
        }
        Ok(published)
    })();
    cleanup_inputs(&input, secondary_input.as_deref());
    let _ = std::fs::remove_file(&engine_output);
    if result.is_err() {
        let _ = std::fs::remove_file(&destination);
        let _ = std::fs::remove_file(output_dir.join("vocal-residual.flac"));
        let _ = std::fs::remove_file(output_dir.join("guide-vocals.flac"));
        let _ = std::fs::remove_file(output_dir.join("instrumental.flac"));
    }
    progress(1.0, "GGML inference complete", None);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(index: usize, name: &str, kind: DeviceKind) -> DeviceDescriptor {
        DeviceDescriptor {
            ggml_index: index,
            name: name.to_string(),
            description: name.to_string(),
            kind,
        }
    }

    #[test]
    fn resolve_device_class_picks_the_first_matching_ggml_device() {
        let devices = [
            descriptor(0, "CPU", DeviceKind::Cpu),
            descriptor(1, "Intel Arc B580", DeviceKind::DiscreteGpu),
            descriptor(2, "AMD Radeon 780M", DeviceKind::IntegratedGpu),
        ];
        assert_eq!(resolve_device_class(&devices, "cpu").unwrap().ggml_index, 0);
        assert_eq!(resolve_device_class(&devices, "gpu").unwrap().ggml_index, 1);
        assert_eq!(
            resolve_device_class(&devices, "integrated_gpu")
                .unwrap()
                .ggml_index,
            2
        );
        assert!(resolve_device_class(&[], "gpu").is_err());
    }

    #[test]
    fn physical_and_ggml_device_names_match_without_brand_punctuation() {
        assert!(same_device_name(
            "Intel(R) Arc(tm) B580 Graphics",
            "Intel Arc B580 Graphics"
        ));
        assert!(!same_device_name("Intel Arc B580", "AMD Radeon 780M"));
    }

    #[test]
    fn rmvpe_evidence_publication_is_typed_atomic_and_no_overwrite() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "uta-ggml-rmvpe-evidence-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let raw = root.join("engine.json");
        let published = root.join("rmvpe-pitch-evidence.json");
        std::fs::write(
            &raw,
            br#"{"frames":[{"time":0.0,"hz":220.0,"confidence":0.8,"voiced":true},{"time":0.01,"hz":120.0,"confidence":0.01,"voiced":false}]}"#,
        )
        .unwrap();
        let runtime_digest = "d".repeat(64);
        publish_rmvpe_evidence(&raw, &published, &runtime_digest, "ggml_vulkan").unwrap();
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&published).unwrap()).unwrap();
        assert_eq!(evidence["schema_version"], 1);
        assert_eq!(evidence["model_id"], "rmvpe");
        assert_eq!(evidence["backend"], "ggml_vulkan");
        assert_eq!(evidence["model_gguf_sha256"], runtime::RMVPE_GGUF_SHA256);
        assert_eq!(evidence["runtime_manifest_sha256"], runtime_digest);
        assert!(!published.with_extension("json.tmp").exists());
        assert!(publish_rmvpe_evidence(&raw, &published, "replacement", "ggml_cpu").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fcpe_evidence_publication_is_typed_atomic_and_no_overwrite() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "uta-ggml-fcpe-evidence-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let raw = root.join("engine.json");
        let published = root.join("fcpe-pitch-evidence.json");
        std::fs::write(
            &raw,
            br#"{"frames":[{"time":0.0,"hz":220.0},{"time":0.01,"hz":null}]}"#,
        )
        .unwrap();
        let runtime_digest = "d".repeat(64);
        publish_fcpe_evidence(&raw, &published, 43_309_760, &runtime_digest, "ggml_cpu").unwrap();
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&published).unwrap()).unwrap();
        assert_eq!(evidence["schema_version"], 1);
        assert_eq!(evidence["model_id"], "fcpe");
        assert_eq!(evidence["backend"], "ggml_cpu");
        assert_eq!(evidence["model_gguf_size_bytes"], 43_309_760);
        assert_eq!(evidence["runtime_manifest_sha256"], runtime_digest);
        assert!(!published.with_extension("json.tmp").exists());
        assert!(
            publish_fcpe_evidence(&raw, &published, 43_309_760, "replacement", "ggml_cpu").is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn basic_pitch_evidence_publication_is_typed_atomic_and_no_overwrite() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "uta-ggml-basic-pitch-evidence-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let raw = root.join("engine.json");
        let published = root.join("basic-pitch-activation-evidence.json");
        std::fs::write(
            &raw,
            br#"{"frames":[{"time":0.0,"note_max":0.7,"onset_max":0.6,"contour_class":42,"contour_score":0.8},{"time":0.011609977324263039,"note_max":0.2,"onset_max":0.1,"contour_class":263,"contour_score":0.3}]}"#,
        )
        .unwrap();
        let runtime_digest = "d".repeat(64);
        publish_basic_pitch_evidence(&raw, &published, 144_512, &runtime_digest, "ggml_vulkan")
            .unwrap();
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&published).unwrap()).unwrap();
        assert_eq!(evidence["schema_version"], 1);
        assert_eq!(evidence["model_id"], "basic_pitch");
        assert_eq!(evidence["backend"], "ggml_vulkan");
        assert_eq!(evidence["model_gguf_size_bytes"], 144_512);
        assert_eq!(evidence["runtime_manifest_sha256"], runtime_digest);
        assert_eq!(evidence["sample_rate"], 22_050);
        assert_eq!(evidence["window_samples"], 43_844);
        assert_eq!(evidence["window_hop_samples"], 36_164);
        assert_eq!(evidence["fft_hop_samples"], 256);
        assert_eq!(evidence["overlap_frames"], 30);
        assert_eq!(evidence["padding_samples"], 3_840);
        assert_eq!(evidence["frames_per_window"], 172);
        assert_eq!(evidence["owned_frames_per_window"], 142);
        assert!(!published.with_extension("json.tmp").exists());
        assert!(
            publish_basic_pitch_evidence(&raw, &published, 144_512, "replacement", "ggml_cpu")
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn basic_pitch_evidence_rejects_invalid_frames_and_backends() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "uta-ggml-basic-pitch-invalid-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let raw = root.join("engine.json");
        let published = root.join("evidence.json");
        std::fs::write(
            &raw,
            br#"{"frames":[{"time":0.01,"note_max":0.7,"onset_max":0.6,"contour_class":264,"contour_score":0.8}]}"#,
        )
        .unwrap();
        assert!(
            publish_basic_pitch_evidence(&raw, &published, 144_512, "runtime", "ggml_vulkan")
                .is_err()
        );
        assert!(
            publish_basic_pitch_evidence(&raw, &published, 144_512, "runtime", "wgpu_vulkan")
                .is_err()
        );
        assert!(!published.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn instrumental_target_publication_maps_direct_and_residual_roles_correctly() {
        assert_eq!(
            dual_separation_filenames("bs_roformer_leap_xe90_vocals"),
            Some(("guide-vocals.flac", "instrumental.flac"))
        );
        assert_eq!(
            dual_separation_filenames("bs_roformer_leap_xe90_instrumental"),
            Some(("instrumental.flac", "guide-vocals.flac"))
        );
    }

    #[test]
    fn semantic_routes_are_explicit_and_non_substituting() {
        assert!(
            validate_semantics(
                "bs_roformer_leap_xe90_vocals",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"vocal+instrumental_residual"
                })
            )
            .is_ok()
        );
        assert!(
            validate_semantics(
                "bs_roformer_leap_xe90_instrumental",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"instrumental+vocal_residual"
                })
            )
            .is_ok()
        );
        assert!(
            validate_semantics(
                "bs_roformer_leap_xe90_instrumental",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"vocal+instrumental_residual"
                })
            )
            .is_err()
        );
        assert!(
            validate_semantics(
                "bs_roformer_leap_xe90_vocals",
                &serde_json::json!({
                    "backend":"openvino_gpu",
                    "semantic_output":"vocal+instrumental_residual"
                })
            )
            .is_err()
        );
        assert!(
            validate_semantics(
                "bs_roformer_leap_xe90_vocals",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"instrumental"
                })
            )
            .is_err()
        );
        assert!(
            validate_semantics(
                "rmvpe",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"pitch"
                })
            )
            .is_ok()
        );
        assert!(
            validate_semantics(
                "rmvpe",
                &serde_json::json!({
                    "backend":"ggml_cpu",
                    "device_class":"cpu",
                    "semantic_output":"pitch"
                })
            )
            .is_ok()
        );
        for invalid_cpu in [
            serde_json::json!({"backend":"ggml_cpu", "semantic_output":"pitch"}),
            serde_json::json!({"backend":"ggml_vulkan", "device_class":"cpu", "semantic_output":"pitch"}),
            serde_json::json!({"backend":"ggml_cpu", "device_class":"cpu", "vulkan_device":0, "semantic_output":"pitch"}),
        ] {
            assert!(validate_semantics("rmvpe", &invalid_cpu).is_err());
        }
        assert!(
            validate_semantics(
                "rmvpe",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"guide_vocals"
                })
            )
            .is_err()
        );
        assert!(
            validate_semantics(
                "fcpe",
                &serde_json::json!({
                    "backend":"ggml_cpu",
                    "device_class":"cpu",
                    "semantic_output":"pitch"
                })
            )
            .is_ok()
        );
        assert!(
            validate_semantics(
                "basic_pitch",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "device_class":"integrated_gpu",
                    "semantic_output":"note+onset+contour_activation"
                })
            )
            .is_ok()
        );
        assert!(
            validate_semantics(
                "basic_pitch",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "device_class":"integrated_gpu",
                    "semantic_output":"pitch"
                })
            )
            .is_err()
        );
        assert!(
            validate_semantics(
                "melband_roformer_harmony",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "input_semantics":"all_vocals",
                    "semantic_output":"lead_vocal+backing_vocal_residual"
                })
            )
            .is_ok()
        );
        assert!(
            validate_semantics(
                "melband_roformer_harmony",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "input_semantics":"all_vocals",
                    "semantic_output":"lead_vocal+backing_vocal"
                })
            )
            .is_err()
        );
        assert!(
            validate_semantics(
                "bs_polarformer_public_instrumental",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"vocal+instrumental_residual"
                })
            )
            .is_ok()
        );
        for single_output in ["guide_vocals", "instrumental"] {
            assert!(
                validate_semantics(
                    "bs_polarformer_public_instrumental",
                    &serde_json::json!({
                        "backend":"ggml_vulkan",
                        "semantic_output":single_output
                    })
                )
                .is_err()
            );
        }
        assert!(
            validate_semantics(
                "bs_polarformer_public_instrumental",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"dry"
                })
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod f32_matmul_tests {
    use super::f32_matmul_may_be_promoted;

    #[test]
    fn only_measured_separators_may_promote_their_f32_matmul() {
        assert!(f32_matmul_may_be_promoted("bs_roformer_leap_xe90_vocals"));
        assert!(f32_matmul_may_be_promoted(
            "bs_roformer_leap_xe90_instrumental"
        ));
        assert!(f32_matmul_may_be_promoted(
            "bs_polarformer_public_instrumental"
        ));
    }

    #[test]
    fn a_greedy_decoder_never_promotes_its_f32_matmul() {
        for model in [
            "firered_asr2_aed",
            "qwen3_asr_1_7b",
            "qwen3_forced_aligner_0_6b",
        ] {
            assert!(
                !f32_matmul_may_be_promoted(model),
                "{model} must keep the exact F32 matmul"
            );
        }
    }

    #[test]
    fn an_unmeasured_model_keeps_the_exact_path() {
        assert!(!f32_matmul_may_be_promoted(
            "melband_roformer_denoise_aufr33"
        ));
        assert!(!f32_matmul_may_be_promoted("rmvpe"));
        assert!(!f32_matmul_may_be_promoted("some_model_added_later"));
    }
}
