use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::core::{Backend, InferParams, Model};

pub const SAMPLE_RATE: usize = 44_100;
pub const HOP_SIZE: usize = 441;
pub const CHUNK_SECONDS: usize = 30;
pub const CHUNK_OVERLAP_SECONDS: usize = 2;
pub const CHUNK_SAMPLES: usize = CHUNK_SECONDS * SAMPLE_RATE;
pub const OVERLAP_SAMPLES: usize = CHUNK_OVERLAP_SECONDS * SAMPLE_RATE;
pub const CHUNK_FRAMES: usize = CHUNK_SAMPLES / HOP_SIZE;
pub const D3PM_STEPS: usize = 8;
pub const BOUNDARY_THRESHOLD: f32 = 0.2;
pub const PRESENCE_THRESHOLD: f32 = 0.2;
pub const SEAM_MERGE_MAX_SEMITONES: f32 = 1.0;
pub const SEAM_BOUNDARY_TOLERANCE_SECONDS: f64 = 0.05;

pub const SOURCE_COMMIT: &str = "475a8ee781fe8cca980b3b12fbe6c80c768a813a";
pub const ESTIMATOR_NOTE_BUCKETS: [usize; 6] = [32, 64, 128, 256, 512, 1_024];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GameNote {
    pub start: f64,
    pub duration: f64,
    pub midi: f32,
    pub voiced: bool,
}

#[derive(Debug, Serialize)]
struct GameEvidence<'a> {
    schema_version: u32,
    model_id: &'static str,
    variant: &'static str,
    source_asset_sha256: &'a str,
    source_commit: &'static str,
    model_manifest_sha256: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    sample_rate: usize,
    timestep_ms: u32,
    d3pm_steps: usize,
    estimator_note_buckets: &'static [usize],
    boundary_decision_threshold: f32,
    presence_decision_threshold: f32,
    notes: Vec<GameNote>,
}

pub fn append_stitched_note(
    notes: &mut Vec<GameNote>,
    note: GameNote,
    seam_time: Option<f64>,
) -> Result<(), String> {
    if let Some(previous) = notes.last_mut() {
        let previous_end = previous.start + previous.duration;
        if note.start < previous.start {
            let note_end = note.start + note.duration;
            if let Some(seam) = seam_time
                && previous.start < seam
                && note_end > seam
            {
                let previous_owned_end = previous_end.min(seam);
                if previous_owned_end <= previous.start {
                    return Err(
                        "GAME chunk stitching produced an empty left seam interval".to_string(),
                    );
                }
                previous.duration = previous_owned_end - previous.start;
                let mut note = note;
                note.start = seam;
                note.duration = note_end - seam;
                notes.push(note);
                return Ok(());
            }
            return Err("GAME chunk stitching produced an unordered note".to_string());
        }
        if note.start < previous_end {
            let note_end = note.start + note.duration;
            let seam_continuation = seam_time.is_some_and(|seam| {
                previous_end >= seam - SEAM_BOUNDARY_TOLERANCE_SECONDS
                    && note.start <= seam + SEAM_BOUNDARY_TOLERANCE_SECONDS
                    && (previous.midi - note.midi).abs() <= SEAM_MERGE_MAX_SEMITONES
            });
            if seam_continuation {
                let total_weight = previous.duration + note.duration;
                if total_weight <= 0.0 || !total_weight.is_finite() {
                    return Err("GAME chunk stitching produced invalid seam weights".to_string());
                }
                previous.midi = ((f64::from(previous.midi) * previous.duration
                    + f64::from(note.midi) * note.duration)
                    / total_weight) as f32;
                previous.duration = previous_end.max(note_end) - previous.start;
                previous.voiced = previous.voiced && note.voiced;
                return Ok(());
            }
            if note.start == previous.start
                && let Some(seam) = seam_time
            {
                let split = previous_end.min(seam);
                if split > previous.start && note_end > split {
                    previous.duration = split - previous.start;
                    let mut note = note;
                    note.start = split;
                    note.duration = note_end - split;
                    notes.push(note);
                    return Ok(());
                }
            }
            let clipped_duration = note.start - previous.start;
            if clipped_duration <= 0.0 {
                return Err(
                    "GAME chunk stitching could not resolve a monophonic overlap".to_string(),
                );
            }
            previous.duration = clipped_duration;
        }
    }
    notes.push(note);
    Ok(())
}

fn known_boundary_frames(
    config: &serde_json::Value,
    total_frames: usize,
) -> Result<Vec<usize>, String> {
    let Some(values) = config.get("known_boundaries_us") else {
        return Ok(Vec::new());
    };
    let values = values
        .as_array()
        .ok_or_else(|| "GAME known_boundaries_us must be an array".to_string())?;
    let mut frames = Vec::with_capacity(values.len());
    let mut previous = None;
    for value in values {
        let microseconds = value
            .as_u64()
            .ok_or_else(|| "GAME known boundary must be an integer microsecond".to_string())?;
        let frame_u64 = microseconds
            .checked_add(5_000)
            .ok_or_else(|| "GAME known boundary overflows".to_string())?
            / 10_000;
        let frame = usize::try_from(frame_u64)
            .map_err(|_| "GAME known boundary exceeds this platform".to_string())?;
        if frame == 0 || frame >= total_frames {
            continue;
        }
        if previous.is_some_and(|previous| frame < previous) {
            return Err("GAME known boundaries must be increasing".to_string());
        }
        if previous == Some(frame) {
            continue;
        }
        frames.push(frame);
        previous = Some(frame);
    }
    Ok(frames)
}

fn chunk_known_frames(
    frames: &[usize],
    chunk_offset_frame: usize,
    valid_frames: usize,
) -> Vec<usize> {
    let chunk_end = chunk_offset_frame.saturating_add(valid_frames);
    frames
        .iter()
        .copied()
        .filter(|frame| *frame >= chunk_offset_frame && *frame < chunk_end)
        .map(|frame| frame - chunk_offset_frame)
        .collect()
}

fn language_id(config: &serde_json::Value) -> i32 {
    match config
        .get("language")
        .and_then(serde_json::Value::as_str)
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
    }
}

pub fn infer_game_gguf(
    audio: &[f32],
    model_path: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &'static str, Option<(u64, u64)>),
) -> Result<PathBuf, String> {
    if audio.is_empty() || audio.iter().any(|sample| !sample.is_finite()) {
        return Err("GAME input audio is empty or non-finite".to_string());
    }

    let backend = match config.get("device").and_then(|v| v.as_str()) {
        Some("gpu") => Backend::Gpu,
        Some("cpu") | None => Backend::Cpu,
        Some(other) => {
            return Err(format!(
                "GAME task config.device must be \"cpu\" or \"gpu\", got \"{other}\""
            ));
        }
    };

    progress(0.02, "loading GAME GGUF model", None);
    let model = Model::load(model_path, backend)
        .map_err(|e| format!("failed to load GAME GGUF model: {e}"))?;

    let language = language_id(config);
    let step = CHUNK_SAMPLES - OVERLAP_SAMPLES;
    let total_frames = audio.len().saturating_add(440) / 441;
    let known_boundary_frames = known_boundary_frames(config, total_frames)?;
    let chunk_count = audio.len().saturating_sub(1) / step + 1;

    let mut notes = Vec::new();
    for chunk_index in 0..chunk_count {
        let offset = chunk_index * step;
        let valid = (audio.len() - offset).min(CHUNK_SAMPLES);
        let mut samples = vec![0.0_f32; CHUNK_SAMPLES];
        samples[..valid].copy_from_slice(&audio[offset..offset + valid]);
        let valid_frames = ((valid + 220) / 441).clamp(1, CHUNK_FRAMES);

        let chunk_known = chunk_known_frames(&known_boundary_frames, offset / 441, valid_frames);

        let params = InferParams {
            language,
            d3pm_ts: Vec::new(),
            d3pm_t0: 0.0,
            d3pm_nsteps: D3PM_STEPS as i32,
            boundary_threshold: BOUNDARY_THRESHOLD,
            boundary_radius: 2,
            note_threshold: PRESENCE_THRESHOLD,
            seed: 0,
            known_boundaries: chunk_known,
        };

        let result = model
            .infer(&samples, &params)
            .map_err(|e| format!("GAME inference failed on chunk {chunk_index}: {e}"))?;

        let offset_seconds = offset as f64 / SAMPLE_RATE as f64;
        let left_cut = if chunk_index == 0 { 0.0 } else { 1.0 };
        let right_cut = if chunk_index + 1 == chunk_count {
            valid as f64 / SAMPLE_RATE as f64
        } else {
            valid as f64 / SAMPLE_RATE as f64 - 1.0
        };
        let seam_time = (chunk_index > 0).then_some(offset_seconds + 1.0);

        for core_note in result.notes {
            let start = core_note.offset_seconds as f64;
            let duration = core_note.duration_seconds as f64;
            let midpoint = start + duration / 2.0;
            if midpoint < left_cut || midpoint >= right_cut {
                continue;
            }
            let game_note = GameNote {
                start: start + offset_seconds,
                duration,
                midi: core_note.pitch_midi,
                voiced: core_note.voiced,
            };
            append_stitched_note(&mut notes, game_note, seam_time)?;
        }

        progress(
            0.1 + 0.89 * (chunk_index + 1) as f32 / chunk_count as f32,
            "running GAME note inference",
            Some(((chunk_index + 1) as u64, chunk_count as u64)),
        );
    }

    if notes.is_empty() {
        return Err("GAME produced no note evidence".to_string());
    }

    let destination = output_dir.join("game-note-evidence.json");
    let temporary = output_dir.join("game-note-evidence.json.tmp");
    if destination.exists() {
        let _ = std::fs::remove_file(&destination);
    }

    let model_bytes = std::fs::read(model_path).unwrap_or_default();
    let model_sha256 = format!("{:x}", Sha256::digest(&model_bytes));

    let evidence = GameEvidence {
        schema_version: 1,
        model_id: "game",
        variant: "GAME-1.0.3-medium-onnx",
        source_asset_sha256: &model_sha256,
        source_commit: SOURCE_COMMIT,
        model_manifest_sha256: &model_sha256,
        runtime_manifest_sha256: "uta-game-worker-native-v1",
        backend: "game_native",
        sample_rate: SAMPLE_RATE,
        timestep_ms: 10,
        d3pm_steps: D3PM_STEPS,
        estimator_note_buckets: &ESTIMATOR_NOTE_BUCKETS,
        boundary_decision_threshold: BOUNDARY_THRESHOLD,
        presence_decision_threshold: PRESENCE_THRESHOLD,
        notes,
    };

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, &evidence).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;

    Ok(destination)
}
