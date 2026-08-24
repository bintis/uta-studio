use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use openvino::{CompiledModel, Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use serde::{Deserialize, Serialize};

use crate::runtime;

const MANIFEST_SHA256: &str = "aa9f3a4c2d107527913ef3947f337b41bff7b6de39de6c91ce46b82ced15ac87";
const SOURCE_ASSET_SHA256: &str =
    "5b7a21e64c6310efac399f5d12838fffa70565be162436b5a4a65f290721e7d8";
const SOURCE_COMMIT: &str = "475a8ee781fe8cca980b3b12fbe6c80c768a813a";
const SAMPLE_RATE: usize = 44_100;
const CHUNK_SAMPLES: usize = 1_323_000;
const CHUNK_FRAMES: usize = 3_000;
const OVERLAP_SAMPLES: usize = 88_200;
const EMBEDDING_DIM: usize = 256;
const D3PM_STEPS: usize = 8;
const BOUNDARY_THRESHOLD: f32 = 0.2;
const BOUNDARY_RADIUS: i64 = 2;
const PRESENCE_THRESHOLD: f32 = 0.2;
const SEAM_MERGE_MAX_SEMITONES: f32 = 0.5;
const SEAM_BOUNDARY_TOLERANCE_SECONDS: f64 = 0.08;

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    model_id: String,
    variant: String,
    format: String,
    source_asset_sha256: String,
    model_license: String,
    runtime_recipe_sha256: String,
    sample_rate: usize,
    chunk_samples: usize,
    chunk_frames: usize,
    chunk_overlap_samples: usize,
    d3pm_steps: usize,
    estimator_note_buckets: Vec<usize>,
    files: BTreeMap<String, String>,
}

struct ModelFiles {
    directory: PathBuf,
    hashes: BTreeMap<String, String>,
    variant: String,
    estimator_note_buckets: Vec<usize>,
}

struct InferenceModels<'a> {
    core: &'a mut Core,
    files: &'a ModelFiles,
    encoder: &'a mut CompiledModel,
    segmenter: &'a mut CompiledModel,
    estimators: &'a mut BTreeMap<usize, CompiledModel>,
}

impl ModelFiles {
    fn verified(&self, name: &str) -> Result<PathBuf, String> {
        let expected = self
            .hashes
            .get(name)
            .ok_or_else(|| format!("GAME manifest is missing {name}"))?;
        let path = self.directory.join(name);
        if runtime::sha256(&path)? != *expected {
            return Err(format!("GAME IR hash mismatch: {name}"));
        }
        Ok(path)
    }
}

#[derive(Debug, Serialize)]
struct GameEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    variant: &'a str,
    source_asset_sha256: &'a str,
    source_commit: &'a str,
    model_manifest_sha256: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    sample_rate: usize,
    timestep_ms: u32,
    d3pm_steps: usize,
    estimator_note_buckets: &'a [usize],
    boundary_decision_threshold: f32,
    presence_decision_threshold: f32,
    notes: Vec<GameNote>,
}

#[derive(Debug, Serialize)]
struct GameNote {
    start: f64,
    duration: f64,
    midi: f32,
    voiced: bool,
}

fn append_stitched_note(
    notes: &mut Vec<GameNote>,
    note: GameNote,
    seam_time: Option<f64>,
) -> Result<(), String> {
    if let Some(previous) = notes.last_mut() {
        let previous_end = previous.start + previous.duration;
        if note.start < previous.start {
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
            let clipped_duration = note.start - previous.start;
            if clipped_duration <= 0.0 {
                return Err("GAME chunk stitching produced duplicate note starts".to_string());
            }
            previous.duration = clipped_duration;
        }
    }
    notes.push(note);
    Ok(())
}

fn tensor(element: ElementType, dimensions: &[i64]) -> Result<Tensor, String> {
    let shape = Shape::new(dimensions).map_err(|error| error.to_string())?;
    Tensor::new(element, &shape).map_err(|error| error.to_string())
}

fn model_files(config: &serde_json::Value) -> Result<ModelFiles, String> {
    let configured = config
        .get("model_path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "GAME task does not contain a Runtime Manager model path".to_string())?;
    let configured = PathBuf::from(configured);
    let directory = if configured.is_dir() {
        configured
    } else {
        configured
            .parent()
            .ok_or_else(|| "GAME model path has no generation directory".to_string())?
            .to_path_buf()
    };
    let manifest_path = directory.join("manifest.json");
    if runtime::sha256(&manifest_path)? != MANIFEST_SHA256 {
        return Err("GAME IR manifest identity mismatch".to_string());
    }
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("GAME IR manifest is invalid: {error}"))?;
    if manifest.schema_version != 2
        || manifest.model_id != "game"
        || manifest.format != "openvino_ir_v11_static_chunked_estimator_buckets"
        || manifest.source_asset_sha256 != SOURCE_ASSET_SHA256
        || manifest.model_license != "CC-BY-NC-SA-4.0"
        || manifest.runtime_recipe_sha256 != crate::protocol::COMPONENT_RECIPE
        || manifest.sample_rate != SAMPLE_RATE
        || manifest.chunk_samples != CHUNK_SAMPLES
        || manifest.chunk_frames != CHUNK_FRAMES
        || manifest.chunk_overlap_samples != OVERLAP_SAMPLES
        || manifest.d3pm_steps != D3PM_STEPS
        || manifest.estimator_note_buckets != [32, 64, 128, 256, 512, 1_024]
    {
        return Err("GAME IR manifest contract is incompatible".to_string());
    }
    Ok(ModelFiles {
        directory,
        hashes: manifest.files,
        variant: manifest.variant,
        estimator_note_buckets: manifest.estimator_note_buckets,
    })
}

fn core() -> Result<Core, String> {
    let mut core = Core::new().map_err(|error| error.to_string())?;
    if !core
        .available_devices()
        .map_err(|error| error.to_string())?
        .contains(&DeviceType::GPU)
    {
        return Err("OpenVINO GPU is unavailable; CPU fallback is forbidden".to_string());
    }
    core.set_properties(
        &DeviceType::GPU,
        [
            (RwPropertyKey::HintInferencePrecision, "f32"),
            (RwPropertyKey::HintExecutionMode, "ACCURACY"),
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(core)
}

fn compile_files(
    core: &mut Core,
    files: &ModelFiles,
    label: &str,
    xml_name: &str,
    bin_name: &str,
) -> Result<CompiledModel, String> {
    let xml = files.verified(xml_name)?;
    let bin = files.verified(bin_name)?;
    let model = core
        .read_model_from_file(
            xml.to_string_lossy().as_ref(),
            bin.to_string_lossy().as_ref(),
        )
        .map_err(|error| format!("could not load GAME {label} IR: {error}"))?;
    core.compile_model(&model, DeviceType::GPU)
        .map_err(|error| format!("could not compile GAME {label} on GPU: {error}"))
}

fn compile(core: &mut Core, files: &ModelFiles, name: &str) -> Result<CompiledModel, String> {
    compile_files(
        core,
        files,
        name,
        &format!("{name}.xml"),
        &format!("{name}.bin"),
    )
}

fn compile_estimator(
    core: &mut Core,
    files: &ModelFiles,
    note_bucket: usize,
) -> Result<CompiledModel, String> {
    compile_files(
        core,
        files,
        &format!("estimator-{note_bucket:04}"),
        &format!("estimator-{note_bucket:04}.xml"),
        "estimator.bin",
    )
}

fn estimator_bucket(buckets: &[usize], note_count: usize) -> Result<usize, String> {
    buckets
        .iter()
        .copied()
        .find(|bucket| *bucket >= note_count)
        .ok_or_else(|| {
            format!(
                "GAME produced {note_count} note regions, exceeding the largest verified estimator bucket"
            )
        })
}

fn language_id(config: &serde_json::Value) -> i64 {
    match config
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
    {
        "en" => 1,
        "ja" => 2,
        "yue" => 3,
        "zh" => 4,
        _ => 0,
    }
}

fn infer_chunk(
    samples: &[f32],
    valid_samples: usize,
    language: i64,
    models: &mut InferenceModels<'_>,
) -> Result<Vec<GameNote>, String> {
    let valid_frames = ((valid_samples + 220) / 441).clamp(1, CHUNK_FRAMES);
    let mut encoder_request = models
        .encoder
        .create_infer_request()
        .map_err(|error| error.to_string())?;
    let mut waveform = tensor(ElementType::F32, &[1, CHUNK_SAMPLES as i64])?;
    waveform
        .get_data_mut::<f32>()
        .map_err(|error| error.to_string())?
        .copy_from_slice(samples);
    let mut duration = tensor(ElementType::F32, &[1])?;
    duration
        .get_data_mut::<f32>()
        .map_err(|error| error.to_string())?[0] = valid_samples as f32 / SAMPLE_RATE as f32;
    encoder_request
        .set_tensor("waveform", &waveform)
        .and_then(|_| encoder_request.set_tensor("duration", &duration))
        .map_err(|error| error.to_string())?;
    encoder_request
        .infer()
        .map_err(|error| format!("GAME encoder GPU inference failed: {error}"))?;
    let x_seg = encoder_request
        .get_tensor("x_seg")
        .map_err(|error| error.to_string())?
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    let x_est = encoder_request
        .get_tensor("x_est")
        .map_err(|error| error.to_string())?
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    let mask = encoder_request
        .get_tensor("maskT")
        .map_err(|error| error.to_string())?
        .get_data::<bool>()
        .map_err(|error| error.to_string())?
        .to_vec();
    if x_seg.len() != CHUNK_FRAMES * EMBEDDING_DIM
        || x_est.len() != CHUNK_FRAMES * EMBEDDING_DIM
        || mask.len() != CHUNK_FRAMES
        || x_seg.iter().chain(&x_est).any(|value| !value.is_finite())
    {
        return Err("GAME encoder output contract mismatch".to_string());
    }

    let known_boundaries = vec![false; CHUNK_FRAMES];
    let mut boundaries = known_boundaries.clone();
    for step in 0..D3PM_STEPS {
        let mut request = models
            .segmenter
            .create_infer_request()
            .map_err(|error| error.to_string())?;
        let mut x = tensor(
            ElementType::F32,
            &[1, CHUNK_FRAMES as i64, EMBEDDING_DIM as i64],
        )?;
        x.get_data_mut::<f32>()
            .map_err(|error| error.to_string())?
            .copy_from_slice(&x_seg);
        let mut language_tensor = tensor(ElementType::I64, &[1])?;
        language_tensor
            .get_data_mut::<i64>()
            .map_err(|error| error.to_string())?[0] = language;
        let mut known = tensor(ElementType::Boolean, &[1, CHUNK_FRAMES as i64])?;
        known
            .get_data_mut::<bool>()
            .map_err(|error| error.to_string())?
            .copy_from_slice(&known_boundaries);
        let mut previous = tensor(ElementType::Boolean, &[1, CHUNK_FRAMES as i64])?;
        previous
            .get_data_mut::<bool>()
            .map_err(|error| error.to_string())?
            .copy_from_slice(&boundaries);
        let mut time = tensor(ElementType::F32, &[1])?;
        time.get_data_mut::<f32>()
            .map_err(|error| error.to_string())?[0] = step as f32 / D3PM_STEPS as f32;
        let mut mask_tensor = tensor(ElementType::Boolean, &[1, CHUNK_FRAMES as i64])?;
        mask_tensor
            .get_data_mut::<bool>()
            .map_err(|error| error.to_string())?
            .copy_from_slice(&mask);
        let mut threshold = tensor(ElementType::F32, &[1])?;
        threshold
            .get_data_mut::<f32>()
            .map_err(|error| error.to_string())?[0] = BOUNDARY_THRESHOLD;
        let mut radius = tensor(ElementType::I64, &[1])?;
        radius
            .get_data_mut::<i64>()
            .map_err(|error| error.to_string())?[0] = BOUNDARY_RADIUS;
        for (name, input) in [
            ("x_seg", &x),
            ("language", &language_tensor),
            ("known_boundaries", &known),
            ("prev_boundaries", &previous),
            ("t", &time),
            ("maskT", &mask_tensor),
            ("threshold", &threshold),
            ("radius", &radius),
        ] {
            request
                .set_tensor(name, input)
                .map_err(|error| error.to_string())?;
        }
        request
            .infer()
            .map_err(|error| format!("GAME segmenter step {step} failed: {error}"))?;
        boundaries = request
            .get_tensor("boundaries")
            .map_err(|error| error.to_string())?
            .get_data::<bool>()
            .map_err(|error| error.to_string())?
            .to_vec();
        if boundaries.len() != CHUNK_FRAMES {
            return Err("GAME segmenter output contract mismatch".to_string());
        }
    }
    boundaries.truncate(valid_frames);
    let mut region = 0_usize;
    let mut durations = Vec::<usize>::new();
    for boundary in boundaries.iter().take(valid_frames) {
        if *boundary {
            region += 1;
        }
        if durations.len() <= region {
            durations.resize(region + 1, 0);
        }
        durations[region] += 1;
    }
    if durations.is_empty() {
        return Err("GAME produced no note regions".to_string());
    }
    let note_count = durations.len().min(CHUNK_FRAMES);
    let note_bucket = estimator_bucket(&models.files.estimator_note_buckets, note_count)?;
    if let std::collections::btree_map::Entry::Vacant(entry) = models.estimators.entry(note_bucket)
    {
        let compiled = compile_estimator(models.core, models.files, note_bucket)?;
        entry.insert(compiled);
    }
    let estimator = models
        .estimators
        .get_mut(&note_bucket)
        .ok_or_else(|| "GAME estimator cache is inconsistent".to_string())?;
    let mut estimator_request = estimator
        .create_infer_request()
        .map_err(|error| error.to_string())?;
    let mut x = tensor(
        ElementType::F32,
        &[1, CHUNK_FRAMES as i64, EMBEDDING_DIM as i64],
    )?;
    x.get_data_mut::<f32>()
        .map_err(|error| error.to_string())?
        .copy_from_slice(&x_est);
    let mut boundary_tensor = tensor(ElementType::Boolean, &[1, CHUNK_FRAMES as i64])?;
    let boundary_data = boundary_tensor
        .get_data_mut::<bool>()
        .map_err(|error| error.to_string())?;
    boundary_data.fill(false);
    boundary_data[..valid_frames].copy_from_slice(&boundaries);
    let mut mask_tensor = tensor(ElementType::Boolean, &[1, CHUNK_FRAMES as i64])?;
    mask_tensor
        .get_data_mut::<bool>()
        .map_err(|error| error.to_string())?
        .copy_from_slice(&mask);
    let mut note_mask = tensor(ElementType::Boolean, &[1, note_bucket as i64])?;
    let note_mask_data = note_mask
        .get_data_mut::<bool>()
        .map_err(|error| error.to_string())?;
    note_mask_data.fill(false);
    note_mask_data[..note_count].fill(true);
    let mut threshold = tensor(ElementType::F32, &[1])?;
    threshold
        .get_data_mut::<f32>()
        .map_err(|error| error.to_string())?[0] = PRESENCE_THRESHOLD;
    for (name, input) in [
        ("x_est", &x),
        ("boundaries", &boundary_tensor),
        ("maskT", &mask_tensor),
        ("maskN", &note_mask),
        ("threshold", &threshold),
    ] {
        estimator_request
            .set_tensor(name, input)
            .map_err(|error| error.to_string())?;
    }
    estimator_request
        .infer()
        .map_err(|error| format!("GAME estimator GPU inference failed: {error}"))?;
    let presence = estimator_request
        .get_tensor("presence")
        .map_err(|error| error.to_string())?
        .get_data::<bool>()
        .map_err(|error| error.to_string())?
        .to_vec();
    let scores = estimator_request
        .get_tensor("scores")
        .map_err(|error| error.to_string())?
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    if presence.len() != note_bucket || scores.len() != note_bucket {
        return Err(format!(
            "GAME estimator output contract mismatch: presence={}, scores={}",
            presence.len(),
            scores.len()
        ));
    }
    let mut notes = Vec::with_capacity(note_count);
    let mut start_frame = 0_usize;
    for index in 0..note_count {
        let duration_frames = durations[index];
        if duration_frames > 0 && presence[index] {
            let midi = scores[index];
            if !midi.is_finite() || !(0.0..=128.0).contains(&midi) {
                return Err(format!(
                    "GAME voiced note {index} has an invalid MIDI score"
                ));
            }
            notes.push(GameNote {
                start: start_frame as f64 * 0.01,
                duration: duration_frames as f64 * 0.01,
                midi,
                voiced: true,
            });
        }
        start_frame += duration_frames;
    }
    // A bounded chunk may legitimately contain only silence or unvoiced audio.
    // Preserve that as an empty contribution and let the whole-task aggregate
    // fail closed only when every chunk has no voiced note evidence.
    Ok(notes)
}

pub fn infer(
    audio: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &'static str),
) -> Result<PathBuf, String> {
    if audio.is_empty() || audio.iter().any(|sample| !sample.is_finite()) {
        return Err("GAME input audio is empty or non-finite".to_string());
    }
    let runtime_manifest = runtime::validate_runtime()?;
    let files = model_files(config)?;
    let mut core = core()?;
    progress(0.02, "compiling GAME encoder");
    let mut encoder = compile(&mut core, &files, "encoder")?;
    progress(0.05, "compiling GAME segmenter");
    let mut segmenter = compile(&mut core, &files, "segmenter")?;
    let mut estimators = BTreeMap::new();
    let mut models = InferenceModels {
        core: &mut core,
        files: &files,
        encoder: &mut encoder,
        segmenter: &mut segmenter,
        estimators: &mut estimators,
    };
    let language = language_id(config);
    let step = CHUNK_SAMPLES - OVERLAP_SAMPLES;
    let chunk_count = audio.len().saturating_sub(1) / step + 1;
    let mut notes = Vec::new();
    for chunk_index in 0..chunk_count {
        let offset = chunk_index * step;
        let valid = (audio.len() - offset).min(CHUNK_SAMPLES);
        let mut samples = vec![0.0_f32; CHUNK_SAMPLES];
        samples[..valid].copy_from_slice(&audio[offset..offset + valid]);
        let chunk_notes = infer_chunk(&samples, valid, language, &mut models)?;
        let offset_seconds = offset as f64 / SAMPLE_RATE as f64;
        let left_cut = if chunk_index == 0 { 0.0 } else { 1.0 };
        let right_cut = if chunk_index + 1 == chunk_count {
            valid as f64 / SAMPLE_RATE as f64
        } else {
            valid as f64 / SAMPLE_RATE as f64 - 1.0
        };
        let seam_time = (chunk_index > 0).then_some(offset_seconds + 1.0);
        for mut note in chunk_notes {
            let midpoint = note.start + note.duration / 2.0;
            if midpoint < left_cut || midpoint >= right_cut {
                continue;
            }
            note.start += offset_seconds;
            append_stitched_note(&mut notes, note, seam_time)?;
        }
        progress(
            0.1 + 0.89 * (chunk_index + 1) as f32 / chunk_count as f32,
            "running GAME note inference",
        );
    }
    if notes.is_empty() {
        return Err("GAME produced no note evidence".to_string());
    }
    let destination = output_dir.join("game-note-evidence.json");
    let temporary = output_dir.join("game-note-evidence.json.tmp");
    if destination.exists() {
        return Err("refusing to overwrite existing GAME evidence".to_string());
    }
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        serde_json::to_writer(
            &mut file,
            &GameEvidence {
                schema_version: 1,
                model_id: "game",
                variant: &files.variant,
                source_asset_sha256: SOURCE_ASSET_SHA256,
                source_commit: SOURCE_COMMIT,
                model_manifest_sha256: MANIFEST_SHA256,
                runtime_manifest_sha256: &runtime_manifest,
                backend: "openvino_gpu",
                sample_rate: SAMPLE_RATE,
                timestep_ms: 10,
                d3pm_steps: D3PM_STEPS,
                estimator_note_buckets: &files.estimator_note_buckets,
                boundary_decision_threshold: BOUNDARY_THRESHOLD,
                presence_decision_threshold: PRESENCE_THRESHOLD,
                notes,
            },
        )
        .map_err(|error| error.to_string())?;
        file.write_all(b"\n").map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map(|()| destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_mapping_is_explicit_and_never_guesses() {
        assert_eq!(language_id(&serde_json::json!({"language":"ja-JP"})), 2);
        assert_eq!(language_id(&serde_json::json!({"language":"yue"})), 3);
        assert_eq!(language_id(&serde_json::json!({"language":"ko"})), 0);
    }

    #[test]
    fn estimator_uses_the_smallest_verified_note_bucket() {
        let buckets = [32, 64, 128, 256, 512, 1_024];
        assert_eq!(estimator_bucket(&buckets, 26).unwrap(), 32);
        assert_eq!(estimator_bucket(&buckets, 33).unwrap(), 64);
        assert_eq!(estimator_bucket(&buckets, 1_024).unwrap(), 1_024);
        assert!(estimator_bucket(&buckets, 1_025).is_err());
    }

    #[test]
    fn chunk_stitching_clips_a_cross_seam_overlap() {
        let mut notes = vec![GameNote {
            start: 308.90,
            duration: 0.15,
            midi: 76.2,
            voiced: true,
        }];
        append_stitched_note(
            &mut notes,
            GameNote {
                start: 309.04,
                duration: 0.26,
                midi: 73.1,
                voiced: true,
            },
            Some(309.0),
        )
        .unwrap();

        assert!((notes[0].duration - 0.14).abs() < 1e-12);
        assert!(notes[0].start + notes[0].duration <= notes[1].start);
    }

    #[test]
    fn chunk_stitching_merges_a_same_pitch_seam_continuation() {
        let mut notes = vec![GameNote {
            start: 308.90,
            duration: 0.20,
            midi: 69.1,
            voiced: true,
        }];
        append_stitched_note(
            &mut notes,
            GameNote {
                start: 309.04,
                duration: 0.26,
                midi: 69.2,
                voiced: true,
            },
            Some(309.0),
        )
        .unwrap();

        assert_eq!(notes.len(), 1);
        assert!((notes[0].start - 308.90).abs() < 1e-12);
        assert!((notes[0].duration - 0.40).abs() < 1e-12);
        assert!((69.1..=69.2).contains(&notes[0].midi));
    }
}
