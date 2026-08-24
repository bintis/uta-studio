use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use openvino::{CompiledModel, Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::rosvot_host;
use crate::singing_frontend::{self, AnnotationPitch};
use crate::stars_g2p::{self, ChineseG2pAsset};
use crate::stars_viterbi;

const FRAME_BUCKET: usize = 256;
const NOTE_BUCKET: usize = 32;
const HIDDEN: usize = 256;
const PITCH_CLASSES: usize = 89;
const RMVPE_CLASSES: usize = 360;
const RMVPE_OVERLAP: usize = 64;
const RMVPE_STRIDE: usize = FRAME_BUCKET - RMVPE_OVERLAP;
const SHARED_MANIFEST_SHA256: &str =
    "986327618f2055873a98fca481893db83ffff2e386b6c522532a5272a1597a2c";
const STARS_MANIFEST_SHA256: &str =
    "c279da93e84069cb2a5321aadda7d37e5ac2a263a8ff32084c3faab1422d83ea";
const ROSVOT_MANIFEST_SHA256: &str =
    "a84ef89fba4863a49198f83c232b3a8d14c1ec3b44ad58ef6407a7528e82e9e5";
const STARS_COMMIT: &str = "f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167";
const STARS_CHECKPOINT: &str = "9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c";
const STARS_CONFIG: &str = "01e8a495ba2e47b47b21fccda8db2605c85ec76cdaae258768d10a459e4e7e91";
const ROSVOT_COMMIT: &str = "3c8332bf43adae35f6e4d64971862f2f6139b310";
const ROSVOT_SOURCE_MANIFEST: &str =
    "5ee3fe4d8f166da11ab0f1fbbc67fbd37e4ab906544d504876c7ebb60b0b32c8";
const ROSVOT_CHECKPOINT: &str = "7501fb5f913d971c2f51bcb3063b930027b03206581820a4d2bfdc394c9c3fcb";
const ROSVOT_CONFIG: &str = "2ad2cb756623418c471b7dc2f56175cce88b69a70b4a2c354fa1a78525aa54e2";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskConfig {
    model_path: PathBuf,
    model_generation: String,
    source_start: u64,
    source_duration: u64,
    timed_transcript_generation: String,
    words: Vec<ConfigWord>,
    #[serde(default)]
    device: DiagnosticDevice,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigWord {
    id: String,
    text: String,
    start: u64,
    duration: u64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DiagnosticDevice {
    Cpu,
    #[default]
    Gpu,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelManifest {
    schema_version: u32,
    model_id: String,
    format: String,
    source_revision: String,
    #[serde(default)]
    source_manifest_sha256: Option<String>,
    checkpoint_sha256: String,
    config_sha256: String,
    frame_bucket: usize,
    note_bucket: usize,
    segmentation: Segmentation,
    shared_frontend: SharedFrontend,
    #[serde(default)]
    g2p_profile: Option<String>,
    #[serde(default)]
    g2p_asset_sha256: Option<String>,
    #[serde(default)]
    word_boundary_source: Option<String>,
    #[serde(default)]
    rwbd_included: Option<bool>,
    graphs: Vec<String>,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Segmentation {
    policy: String,
    frame_step_num: u32,
    frame_step_den: u32,
    unconditioned_frames: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedFrontend {
    profile: String,
    manifest: String,
    manifest_sha256: String,
    annotation_rmvpe_sha256: String,
}

struct ModelArtifact {
    directory: PathBuf,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
struct DependencyIdentity<'a> {
    kind: &'a str,
    generation: &'a str,
}

#[derive(Debug, Clone, Serialize)]
struct RawNote {
    start_frame: usize,
    end_frame: usize,
    pitch_logits: Vec<f32>,
    midi: Option<u8>,
}

#[derive(Serialize)]
struct Evidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    capability: &'a str,
    upstream_commit: &'a str,
    checkpoint_sha256: &'a str,
    config_sha256: &'a str,
    model_generation: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    shared_frontend_profile: &'a str,
    shared_frontend_generation: &'a str,
    annotation_rmvpe_sha256: &'a str,
    word_boundary_source: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    g2p_profile: Option<&'a str>,
    frame_step_num: u32,
    frame_step_den: u32,
    valid_frames: usize,
    note_boundary_logits: Vec<f32>,
    regulated_note_boundaries: Vec<usize>,
    notes: Vec<RawNote>,
    dependencies: Vec<DependencyIdentity<'a>>,
}

struct Segment {
    start: usize,
    timeline_start: u64,
    valid: usize,
    words: Vec<ConfigWord>,
}

struct SharedInputs {
    mel: Vec<f32>,
    frames: usize,
    pitch: AnnotationPitch,
}

type NoteInference = (Vec<f32>, Vec<usize>, Vec<RawNote>);

pub fn infer(
    model_id: &str,
    audio_24k: &[f32],
    audio_16k: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &str),
) -> Result<PathBuf, String> {
    let config: TaskConfig = serde_json::from_value(config.clone())
        .map_err(|error| format!("advanced note task config is invalid: {error}"))?;
    validate_task_config(&config, audio_24k)?;
    let model = model_artifact(model_id, &config.model_path)?;
    progress(0.02, "Validating source-built OpenVINO runtime");
    let runtime_manifest_sha256 = crate::runtime::validate_runtime()?;
    let mut core = configured_core(config.device)?;
    progress(0.05, "Computing the shared singing frontend");
    let shared = shared_inputs(&mut core, &model, audio_24k, audio_16k, config.device)?;
    let segments = conditioned_segments(&config.words, config.source_start, shared.frames)?;
    if segments.is_empty() {
        return Err("advanced note expert has no TimedTranscript-conditioned frames".to_string());
    }
    progress(0.35, "Running conditioned singing-note segments");
    let (boundary_logits, boundaries, notes) = if model_id == "stars" {
        run_stars(
            &mut core,
            &model,
            &shared,
            &segments,
            config.device,
            |completed| {
                progress(0.35 + 0.6 * completed, "Running STARS Stage A/B/C segments");
            },
        )?
    } else {
        run_rosvot(
            &mut core,
            &model,
            &shared,
            &segments,
            config.device,
            |completed| {
                progress(
                    0.35 + 0.6 * completed,
                    "Running ROSVOT frame/pitch segments",
                );
            },
        )?
    };
    let backend = match config.device {
        DiagnosticDevice::Cpu => "openvino_cpu",
        DiagnosticDevice::Gpu => "openvino_gpu",
    };
    let (commit, checkpoint, config_sha, capability, g2p) = if model_id == "stars" {
        (
            STARS_COMMIT,
            STARS_CHECKPOINT,
            STARS_CONFIG,
            "notes.stars",
            Some(stars_g2p::PROFILE),
        )
    } else {
        (
            ROSVOT_COMMIT,
            ROSVOT_CHECKPOINT,
            ROSVOT_CONFIG,
            "notes.rosvot",
            None,
        )
    };
    let mut dependencies = vec![
        DependencyIdentity {
            kind: "shared_frontend",
            generation: SHARED_MANIFEST_SHA256,
        },
        DependencyIdentity {
            kind: "annotation_rmvpe",
            generation: SHARED_MANIFEST_SHA256,
        },
        DependencyIdentity {
            kind: "timed_transcript",
            generation: &config.timed_transcript_generation,
        },
    ];
    if model_id == "stars" {
        dependencies.push(DependencyIdentity {
            kind: "chinese_g2p",
            generation: stars_g2p::ASSET_SHA256,
        });
    }
    let evidence = Evidence {
        schema_version: 1,
        model_id,
        capability,
        upstream_commit: commit,
        checkpoint_sha256: checkpoint,
        config_sha256: config_sha,
        model_generation: &config.model_generation,
        runtime_manifest_sha256: &runtime_manifest_sha256,
        backend,
        shared_frontend_profile: singing_frontend::PROFILE,
        shared_frontend_generation: SHARED_MANIFEST_SHA256,
        annotation_rmvpe_sha256: singing_frontend::ANNOTATION_RMVPE_SHA256,
        word_boundary_source: "timed_transcript",
        g2p_profile: g2p,
        frame_step_num: singing_frontend::HOP_SIZE as u32,
        frame_step_den: singing_frontend::SAMPLE_RATE as u32,
        valid_frames: shared.frames,
        note_boundary_logits: boundary_logits,
        regulated_note_boundaries: boundaries,
        notes,
        dependencies,
    };
    progress(0.97, "Publishing typed advanced-note evidence");
    atomic_json(output_dir, "advanced-note-evidence.json", &evidence)
}

fn validate_task_config(config: &TaskConfig, audio: &[f32]) -> Result<(), String> {
    let source_end = config
        .source_start
        .checked_add(config.source_duration)
        .ok_or_else(|| "advanced note source timeline overflows".to_string())?;
    if !is_sha256(&config.model_generation)
        || !valid_identity(&config.timed_transcript_generation)
        || config.source_duration == 0
        || config.words.is_empty()
        || audio.is_empty()
        || audio.iter().any(|value| !value.is_finite())
    {
        return Err("advanced note task identity or audio is invalid".to_string());
    }
    let mut previous_end = config.source_start;
    for word in &config.words {
        let end = word
            .start
            .checked_add(word.duration)
            .ok_or_else(|| "TimedTranscript word overflows".to_string())?;
        if !valid_identity(&word.id)
            || word.text.trim().is_empty()
            || word.duration == 0
            || word.start < previous_end
            || word.start < config.source_start
            || end > source_end
        {
            return Err("TimedTranscript words are invalid or out of order".to_string());
        }
        previous_end = end;
    }
    Ok(())
}

fn model_artifact(model_id: &str, configured: &Path) -> Result<ModelArtifact, String> {
    let expected_manifest = match model_id {
        "stars" => STARS_MANIFEST_SHA256,
        "rosvot" => ROSVOT_MANIFEST_SHA256,
        _ => return Err("advanced note worker rejects baseline substitution".to_string()),
    };
    let directory = if configured.is_dir() {
        configured.to_path_buf()
    } else {
        configured
            .parent()
            .ok_or_else(|| "advanced note model path has no parent".to_string())?
            .to_path_buf()
    };
    let manifest_path = directory.join("manifest.json");
    require_file_hash(&manifest_path, expected_manifest)?;
    let manifest: ModelManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("advanced note model manifest is invalid: {error}"))?;
    let common = manifest.schema_version == 1
        && manifest.model_id == model_id
        && manifest.format == "openvino_ir_v11_conditioned_segmented"
        && manifest.frame_bucket == FRAME_BUCKET
        && manifest.note_bucket == NOTE_BUCKET
        && manifest.segmentation.policy == "timed-transcript-fixed-256-v1"
        && manifest.segmentation.frame_step_num == 128
        && manifest.segmentation.frame_step_den == 24_000
        && manifest.segmentation.unconditioned_frames == "no_claim"
        && manifest.shared_frontend.profile == singing_frontend::PROFILE
        && manifest.shared_frontend.manifest == "shared/manifest.json"
        && manifest.shared_frontend.manifest_sha256 == SHARED_MANIFEST_SHA256
        && manifest.shared_frontend.annotation_rmvpe_sha256
            == singing_frontend::ANNOTATION_RMVPE_SHA256;
    let specific = if model_id == "stars" {
        manifest.source_revision == STARS_COMMIT
            && manifest.checkpoint_sha256 == STARS_CHECKPOINT
            && manifest.config_sha256 == STARS_CONFIG
            && manifest.source_manifest_sha256.is_none()
            && manifest.g2p_profile.as_deref() == Some(stars_g2p::PROFILE)
            && manifest.g2p_asset_sha256.as_deref() == Some(stars_g2p::ASSET_SHA256)
            && manifest.word_boundary_source.is_none()
            && manifest.rwbd_included.is_none()
            && manifest.graphs == ["stage-a", "stage-b", "stage-c"]
    } else {
        manifest.source_revision == ROSVOT_COMMIT
            && manifest.checkpoint_sha256 == ROSVOT_CHECKPOINT
            && manifest.config_sha256 == ROSVOT_CONFIG
            && manifest.source_manifest_sha256.as_deref() == Some(ROSVOT_SOURCE_MANIFEST)
            && manifest.g2p_profile.is_none()
            && manifest.g2p_asset_sha256.is_none()
            && manifest.word_boundary_source.as_deref() == Some("timed_transcript")
            && manifest.rwbd_included == Some(false)
            && manifest.graphs == ["frame", "pitch"]
    };
    let expected_files = expected_file_names(model_id);
    if !common
        || !specific
        || manifest.files.len() != expected_files.len()
        || expected_files
            .iter()
            .any(|name| !manifest.files.contains_key(*name))
    {
        return Err("advanced note model generation identity is incompatible".to_string());
    }
    for (relative, expected) in &manifest.files {
        if !safe_relative(relative) {
            return Err("advanced note manifest contains an unsafe path".to_string());
        }
        require_file_hash(&directory.join(relative), expected)?;
    }
    Ok(ModelArtifact {
        directory,
        files: manifest.files,
    })
}

fn expected_file_names(model_id: &str) -> BTreeSet<&'static str> {
    let mut names = BTreeSet::from([
        "shared/manifest.json",
        "shared/annotation-rmvpe-t256.onnx",
        "shared/annotation-rmvpe-t256.xml",
        "shared/annotation-rmvpe-t256.bin",
    ]);
    if model_id == "stars" {
        names.extend([
            "stars-stage-a-t256-n32.xml",
            "stars-stage-a-t256-n32.bin",
            "stars-stage-b-t256-n32.xml",
            "stars-stage-b-t256-n32.bin",
            "stars-stage-c-t256-n32.xml",
            "stars-stage-c-t256-n32.bin",
        ]);
    } else {
        names.extend([
            "rosvot-frame-t256-n32.xml",
            "rosvot-frame-t256-n32.bin",
            "rosvot-pitch-t256-n32.xml",
            "rosvot-pitch-t256-n32.bin",
        ]);
    }
    names
}

fn configured_core(device: DiagnosticDevice) -> Result<Core, String> {
    let mut core = Core::new().map_err(|error| format!("OpenVINO is unavailable: {error}"))?;
    let selected = match device {
        DiagnosticDevice::Cpu => DeviceType::CPU,
        DiagnosticDevice::Gpu => DeviceType::GPU,
    };
    let devices = core
        .available_devices()
        .map_err(|error| format!("could not enumerate OpenVINO devices: {error}"))?;
    if !devices.contains(&selected) {
        return Err("requested OpenVINO CPU/GPU device is unavailable".to_string());
    }
    let mut properties = vec![
        (RwPropertyKey::HintInferencePrecision, "f32"),
        (RwPropertyKey::HintExecutionMode, "ACCURACY"),
    ];
    if matches!(device, DiagnosticDevice::Gpu) {
        properties.push((
            RwPropertyKey::Other("GPU_ENABLE_LOOP_UNROLLING".into()),
            "NO",
        ));
    }
    core.set_properties(&selected, properties)
        .map_err(|error| format!("could not configure OpenVINO accuracy mode: {error}"))?;
    if matches!(device, DiagnosticDevice::Gpu) {
        crate::runtime::configure_low_impact_gpu_queue(&mut core)?;
    }
    Ok(core)
}

fn compile(
    core: &mut Core,
    model: &ModelArtifact,
    name: &str,
    device: DiagnosticDevice,
) -> Result<CompiledModel, String> {
    let xml = model.directory.join(format!("{name}.xml"));
    let bin = model.directory.join(format!("{name}.bin"));
    let xml_relative = xml
        .strip_prefix(&model.directory)
        .map_err(|_| "model graph escaped generation".to_string())?
        .to_string_lossy();
    let bin_relative = bin
        .strip_prefix(&model.directory)
        .map_err(|_| "model weights escaped generation".to_string())?
        .to_string_lossy();
    if !model.files.contains_key(xml_relative.as_ref())
        || !model.files.contains_key(bin_relative.as_ref())
    {
        return Err(format!("advanced note manifest omitted graph {name}"));
    }
    let graph = core
        .read_model_from_file(
            xml.to_str()
                .ok_or_else(|| "IR path is not UTF-8".to_string())?,
            bin.to_str()
                .ok_or_else(|| "IR path is not UTF-8".to_string())?,
        )
        .map_err(|error| format!("could not read {name} OpenVINO IR: {error}"))?;
    core.compile_model(
        &graph,
        match device {
            DiagnosticDevice::Cpu => DeviceType::CPU,
            DiagnosticDevice::Gpu => DeviceType::GPU,
        },
    )
    .map_err(|error| format!("could not compile {name} OpenVINO IR: {error}"))
}

fn shared_inputs(
    core: &mut Core,
    model: &ModelArtifact,
    audio_24k: &[f32],
    audio_16k: &[f32],
    device: DiagnosticDevice,
) -> Result<SharedInputs, String> {
    let (mel, frames) = singing_frontend::mel_80(audio_24k)?;
    let (rmvpe_mel, raw_frames) = crate::mel::log_mel_spectrogram(audio_16k, |_| {})?;
    let mut compiled = compile(core, model, "shared/annotation-rmvpe-t256", device)?;
    let raw_f0 = run_annotation_rmvpe(&mut compiled, &rmvpe_mel, raw_frames)?;
    let pitch = singing_frontend::annotation_pitch(&raw_f0, frames)?;
    Ok(SharedInputs { mel, frames, pitch })
}

fn run_annotation_rmvpe(
    compiled: &mut CompiledModel,
    mel: &[f32],
    frames: usize,
) -> Result<Vec<f32>, String> {
    let windows = if frames <= FRAME_BUCKET {
        1
    } else {
        (frames - FRAME_BUCKET).div_ceil(RMVPE_STRIDE) + 1
    };
    let mut raw = Vec::with_capacity(frames);
    let mut start = 0;
    for window in 0..windows {
        let remaining = frames.saturating_sub(start);
        let final_window = remaining <= FRAME_BUCKET;
        let values = crate::mel::to_channel_major_window(mel, frames, start, FRAME_BUCKET);
        let outputs = infer_tensors(
            compiled,
            vec![tensor_f32(&[1, 128, FRAME_BUCKET as i64], &values)?],
            1,
        )?;
        let salience = &outputs[0];
        if salience.len() != FRAME_BUCKET * RMVPE_CLASSES {
            return Err("annotation RMVPE output shape is invalid".to_string());
        }
        let keep_start = if window == 0 { 0 } else { RMVPE_OVERLAP / 2 };
        let keep_end = if final_window {
            remaining
        } else {
            FRAME_BUCKET - RMVPE_OVERLAP / 2
        };
        for frame in keep_start..keep_end {
            raw.push(decode_rmvpe_frame(
                &salience[frame * RMVPE_CLASSES..(frame + 1) * RMVPE_CLASSES],
            ));
        }
        if final_window {
            break;
        }
        start += RMVPE_STRIDE;
    }
    if raw.len() != frames {
        return Err("annotation RMVPE window stitching lost frames".to_string());
    }
    Ok(raw)
}

fn decode_rmvpe_frame(values: &[f32]) -> f32 {
    const CENTS_OFFSET: f32 = 1_997.379_4;
    let (center, confidence) = values
        .iter()
        .copied()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .unwrap_or((0, 0.0));
    if confidence < 0.03 {
        return 0.0;
    }
    let start = center.saturating_sub(4);
    let end = (center + 4).min(RMVPE_CLASSES - 1);
    let mut weighted = 0.0;
    let mut weight = 0.0;
    for (class, salience) in values.iter().copied().enumerate().take(end + 1).skip(start) {
        weighted += salience * (20.0 * class as f32 + CENTS_OFFSET);
        weight += salience;
    }
    let cents = if weight > f32::EPSILON {
        weighted / weight
    } else {
        20.0 * center as f32 + CENTS_OFFSET
    };
    10.0 * 2.0_f32.powf(cents / 1_200.0)
}

fn conditioned_segments(
    words: &[ConfigWord],
    source_start: u64,
    frames: usize,
) -> Result<Vec<Segment>, String> {
    let mut result = Vec::new();
    for start in (0..frames).step_by(FRAME_BUCKET) {
        let valid = (frames - start).min(FRAME_BUCKET);
        let segment_start = frame_to_canonical(start)?
            .checked_add(source_start)
            .ok_or_else(|| "segment timeline overflows".to_string())?;
        let segment_end = frame_to_canonical(start + valid)?
            .checked_add(source_start)
            .ok_or_else(|| "segment timeline overflows".to_string())?;
        let segment_words = words
            .iter()
            .filter(|word| {
                let end = word.start.saturating_add(word.duration);
                word.start < segment_end && end > segment_start
            })
            .cloned()
            .collect::<Vec<_>>();
        if !segment_words.is_empty() {
            result.push(Segment {
                start,
                timeline_start: segment_start,
                valid,
                words: segment_words,
            });
        }
    }
    Ok(result)
}

fn run_stars(
    core: &mut Core,
    model: &ModelArtifact,
    shared: &SharedInputs,
    segments: &[Segment],
    device: DiagnosticDevice,
    mut progress: impl FnMut(f32),
) -> Result<NoteInference, String> {
    let mut stage_a = compile(core, model, "stars-stage-a-t256-n32", device)?;
    let mut stage_b = compile(core, model, "stars-stage-b-t256-n32", device)?;
    let mut stage_c = compile(core, model, "stars-stage-c-t256-n32", device)?;
    let g2p = ChineseG2pAsset::load_embedded()?;
    let mut all_logits = vec![0.0; shared.frames];
    let mut all_boundaries = Vec::new();
    let mut all_notes = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let mel = padded_rows(
            &shared.mel,
            shared.frames,
            singing_frontend::MEL_BINS,
            segment.start,
        );
        let pitch = padded_i64(&shared.pitch.pitch_coarse, segment.start);
        let uv = padded_i64(&shared.pitch.uv, segment.start);
        let mut nonpadding = vec![0.0_f32; FRAME_BUCKET];
        nonpadding[..segment.valid].fill(1.0);
        let a = infer_tensors(
            &mut stage_a,
            vec![
                tensor_f32(&[1, FRAME_BUCKET as i64, 80], &mel)?,
                tensor_i64(&[1, FRAME_BUCKET as i64], &pitch)?,
                tensor_i64(&[1, FRAME_BUCKET as i64], &uv)?,
                tensor_f32(&[1, FRAME_BUCKET as i64], &nonpadding)?,
            ],
            6,
        )?;
        require_len(&a[0], FRAME_BUCKET * HIDDEN, "STARS mel embedding")?;
        require_len(&a[1], FRAME_BUCKET * HIDDEN, "STARS frame features")?;
        require_len(&a[2], FRAME_BUCKET, "STARS phoneme boundary")?;
        require_len(&a[3], FRAME_BUCKET * 61, "STARS phoneme logits")?;
        let phonemes = g2p.phonemize_words(
            &segment
                .words
                .iter()
                .map(|word| word.text.clone())
                .collect::<Vec<_>>(),
        )?;
        let alignment = stars_viterbi::align(
            &a[3][..segment.valid * 61],
            61,
            &a[2][..segment.valid],
            &phonemes.phone_ids,
            &phonemes.phone_to_word,
        )?;
        let mut mel2ph = alignment.mel_to_phoneme;
        let mut mel2word = alignment.mel_to_word;
        mel2ph.resize(FRAME_BUCKET, 0);
        mel2word.resize(FRAME_BUCKET, 0);
        let b = infer_tensors(
            &mut stage_b,
            vec![
                tensor_f32(&[1, FRAME_BUCKET as i64, HIDDEN as i64], &a[0])?,
                tensor_f32(&[1, FRAME_BUCKET as i64, HIDDEN as i64], &a[1])?,
                tensor_i64(&[1, FRAME_BUCKET as i64], &mel2ph)?,
                tensor_i64(&[1, FRAME_BUCKET as i64], &mel2word)?,
            ],
            3,
        )?;
        require_len(&b[0], FRAME_BUCKET * HIDDEN, "STARS enhanced features")?;
        require_len(&b[1], FRAME_BUCKET, "STARS note boundary")?;
        all_logits[segment.start..segment.start + segment.valid]
            .copy_from_slice(&b[1][..segment.valid]);
        let regulated = stars_viterbi::regulate_boundaries(&b[1], 0.8, 17, segment.valid)?;
        let local_boundaries = boundary_indices(&regulated, segment.valid);
        let ranges = note_ranges(&local_boundaries, segment.valid);
        if ranges.len() > NOTE_BUCKET {
            return Err("STARS segment exceeds the pinned note bucket".to_string());
        }
        let mel2note = mapping_from_boundaries(&regulated, segment.valid);
        let c = infer_tensors(
            &mut stage_c,
            vec![
                tensor_f32(&[1, FRAME_BUCKET as i64, HIDDEN as i64], &a[0])?,
                tensor_f32(&[1, FRAME_BUCKET as i64, HIDDEN as i64], &b[0])?,
                tensor_i64(&[1, FRAME_BUCKET as i64], &mel2note)?,
            ],
            4,
        )?;
        require_len(&c[3], NOTE_BUCKET * PITCH_CLASSES, "STARS note pitch")?;
        append_notes(
            &mut all_notes,
            &mut all_boundaries,
            segment.start,
            &ranges,
            &c[3],
        );
        progress((index + 1) as f32 / segments.len() as f32);
    }
    stitch_notes(&mut all_notes);
    Ok((all_logits, all_boundaries, all_notes))
}

fn run_rosvot(
    core: &mut Core,
    model: &ModelArtifact,
    shared: &SharedInputs,
    segments: &[Segment],
    device: DiagnosticDevice,
    mut progress: impl FnMut(f32),
) -> Result<NoteInference, String> {
    let mut frame_graph = compile(core, model, "rosvot-frame-t256-n32", device)?;
    let mut pitch_graph = compile(core, model, "rosvot-pitch-t256-n32", device)?;
    let mel_40 = singing_frontend::rosvot_mel_prefix(&shared.mel, shared.frames)?;
    let mut all_logits = vec![0.0; shared.frames];
    let mut all_boundaries = Vec::new();
    let mut all_notes = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let mel = padded_rows(&mel_40, shared.frames, 40, segment.start);
        let pitch = padded_i64(&shared.pitch.pitch_coarse, segment.start);
        let uv = padded_i64(&shared.pitch.uv, segment.start);
        let reference = segment_word_boundaries(segment)?;
        let output = infer_tensors(
            &mut frame_graph,
            vec![
                tensor_f32(&[1, FRAME_BUCKET as i64, 40], &mel)?,
                tensor_i64(&[1, FRAME_BUCKET as i64], &pitch)?,
                tensor_i64(&[1, FRAME_BUCKET as i64], &uv)?,
                tensor_i64(&[1, FRAME_BUCKET as i64], &reference)?,
            ],
            3,
        )?;
        require_len(&output[0], FRAME_BUCKET, "ROSVOT note boundary")?;
        require_len(&output[1], FRAME_BUCKET, "ROSVOT frame attention")?;
        require_len(
            &output[2],
            FRAME_BUCKET * HIDDEN,
            "ROSVOT weighted features",
        )?;
        all_logits[segment.start..segment.start + segment.valid]
            .copy_from_slice(&output[0][..segment.valid]);
        let regulated =
            rosvot_host::regulate_boundaries(&output[0], 0.85, 17, &reference, 8, segment.valid)?;
        let aggregated = rosvot_host::aggregate_notes(
            &output[2],
            &output[1],
            &regulated,
            HIDDEN,
            segment.valid,
        )?;
        if aggregated.count > NOTE_BUCKET {
            return Err("ROSVOT segment exceeds the pinned note bucket".to_string());
        }
        let mut note_features = vec![0.0_f32; NOTE_BUCKET * HIDDEN];
        note_features[..aggregated.features.len()].copy_from_slice(&aggregated.features);
        let pitch_output = infer_tensors(
            &mut pitch_graph,
            vec![tensor_f32(
                &[1, NOTE_BUCKET as i64, HIDDEN as i64],
                &note_features,
            )?],
            1,
        )?;
        require_len(
            &pitch_output[0],
            NOTE_BUCKET * PITCH_CLASSES,
            "ROSVOT note pitch",
        )?;
        let local_boundaries = boundary_indices(&regulated, segment.valid);
        let ranges = note_ranges(&local_boundaries, segment.valid);
        append_notes(
            &mut all_notes,
            &mut all_boundaries,
            segment.start,
            &ranges,
            &pitch_output[0],
        );
        progress((index + 1) as f32 / segments.len() as f32);
    }
    stitch_notes(&mut all_notes);
    Ok((all_logits, all_boundaries, all_notes))
}

fn segment_word_boundaries(segment: &Segment) -> Result<Vec<i64>, String> {
    let mut boundaries = vec![0_i64; FRAME_BUCKET];
    for word in segment.words.iter().skip(1) {
        let local = word.start.saturating_sub(segment.timeline_start);
        let frame = canonical_to_frame(local)?.min(segment.valid.saturating_sub(1));
        if frame > 0 {
            boundaries[frame] = 1;
        }
    }
    Ok(boundaries)
}

fn append_notes(
    notes: &mut Vec<RawNote>,
    boundaries: &mut Vec<usize>,
    segment_start: usize,
    ranges: &[(usize, usize)],
    logits: &[f32],
) {
    for boundary in ranges.iter().skip(1).map(|range| segment_start + range.0) {
        boundaries.push(boundary);
    }
    for (index, (start, end)) in ranges.iter().copied().enumerate() {
        let row = logits[index * PITCH_CLASSES..(index + 1) * PITCH_CLASSES].to_vec();
        let midi = row
            .iter()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(right.1))
            .and_then(|(class, _)| (30..=85).contains(&class).then_some(class as u8));
        notes.push(RawNote {
            start_frame: segment_start + start,
            end_frame: segment_start + end,
            pitch_logits: row,
            midi,
        });
    }
}

fn stitch_notes(notes: &mut Vec<RawNote>) {
    let mut stitched: Vec<RawNote> = Vec::with_capacity(notes.len());
    for note in notes.drain(..) {
        if let Some(previous) = stitched.last_mut()
            && previous.end_frame == note.start_frame
            && previous.midi.is_some()
            && previous.midi == note.midi
        {
            previous.end_frame = note.end_frame;
            for (left, right) in previous.pitch_logits.iter_mut().zip(note.pitch_logits) {
                *left = (*left + right) * 0.5;
            }
        } else {
            stitched.push(note);
        }
    }
    *notes = stitched;
}

fn note_ranges(boundaries: &[usize], valid: usize) -> Vec<(usize, usize)> {
    let mut starts = Vec::with_capacity(boundaries.len() + 1);
    starts.push(0);
    starts.extend(
        boundaries
            .iter()
            .copied()
            .filter(|value| *value > 0 && *value < valid),
    );
    starts.sort_unstable();
    starts.dedup();
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| (*start, starts.get(index + 1).copied().unwrap_or(valid)))
        .filter(|(start, end)| end > start)
        .collect()
}

fn boundary_indices(values: &[i64], valid: usize) -> Vec<usize> {
    values[..valid]
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (*value == 1).then_some(index))
        .collect()
}

fn mapping_from_boundaries(values: &[i64], valid: usize) -> Vec<i64> {
    let mut mapping = vec![0_i64; FRAME_BUCKET];
    let mut note = 0_i64;
    for frame in 0..valid {
        note += values[frame];
        mapping[frame] = note.min((NOTE_BUCKET - 1) as i64);
    }
    mapping[valid..].fill(note.min((NOTE_BUCKET - 1) as i64));
    mapping
}

fn padded_rows(values: &[f32], frames: usize, width: usize, start: usize) -> Vec<f32> {
    let mut result = vec![0.0; FRAME_BUCKET * width];
    let count = frames.saturating_sub(start).min(FRAME_BUCKET);
    result[..count * width].copy_from_slice(&values[start * width..(start + count) * width]);
    result
}

fn padded_i64(values: &[i64], start: usize) -> Vec<i64> {
    let mut result = vec![0_i64; FRAME_BUCKET];
    let count = values.len().saturating_sub(start).min(FRAME_BUCKET);
    result[..count].copy_from_slice(&values[start..start + count]);
    result
}

fn infer_tensors(
    compiled: &mut CompiledModel,
    inputs: Vec<Tensor>,
    output_count: usize,
) -> Result<Vec<Vec<f32>>, String> {
    let mut request = compiled
        .create_infer_request()
        .map_err(|error| format!("could not create advanced note request: {error}"))?;
    for (index, tensor) in inputs.iter().enumerate() {
        request
            .set_input_tensor_by_index(index, tensor)
            .map_err(|error| format!("could not bind advanced note input {index}: {error}"))?;
    }
    request
        .infer()
        .map_err(|error| format!("advanced note OpenVINO inference failed: {error}"))?;
    (0..output_count)
        .map(|index| {
            request
                .get_output_tensor_by_index(index)
                .map_err(|error| format!("could not read advanced note output {index}: {error}"))?
                .get_data::<f32>()
                .map(|data| data.to_vec())
                .map_err(|error| format!("advanced note output {index} is not float32: {error}"))
        })
        .collect()
}

fn tensor_f32(shape: &[i64], values: &[f32]) -> Result<Tensor, String> {
    let shape = Shape::new(shape).map_err(|error| error.to_string())?;
    let mut tensor = Tensor::new(ElementType::F32, &shape).map_err(|error| error.to_string())?;
    let target = tensor
        .get_data_mut::<f32>()
        .map_err(|error| error.to_string())?;
    if target.len() != values.len() {
        return Err("float tensor shape does not match its values".to_string());
    }
    target.copy_from_slice(values);
    Ok(tensor)
}

fn tensor_i64(shape: &[i64], values: &[i64]) -> Result<Tensor, String> {
    let shape = Shape::new(shape).map_err(|error| error.to_string())?;
    let mut tensor = Tensor::new(ElementType::I64, &shape).map_err(|error| error.to_string())?;
    let target = tensor
        .get_data_mut::<i64>()
        .map_err(|error| error.to_string())?;
    if target.len() != values.len() {
        return Err("integer tensor shape does not match its values".to_string());
    }
    target.copy_from_slice(values);
    Ok(tensor)
}

fn require_len(values: &[f32], expected: usize, label: &str) -> Result<(), String> {
    if values.len() != expected || values.iter().any(|value| !value.is_finite()) {
        Err(format!("{label} output shape or values are invalid"))
    } else {
        Ok(())
    }
}

fn frame_to_canonical(frame: usize) -> Result<u64, String> {
    u64::try_from(
        (frame as u128) * singing_frontend::HOP_SIZE as u128 * 1_000_000
            / singing_frontend::SAMPLE_RATE as u128,
    )
    .map_err(|_| "advanced note frame timeline overflows".to_string())
}

fn canonical_to_frame(value: u64) -> Result<usize, String> {
    usize::try_from(
        (u128::from(value) * singing_frontend::SAMPLE_RATE as u128
            + singing_frontend::HOP_SIZE as u128 * 500_000)
            / (singing_frontend::HOP_SIZE as u128 * 1_000_000),
    )
    .map_err(|_| "TimedTranscript frame projection overflows".to_string())
}

fn atomic_json(
    output_dir: &Path,
    filename: &str,
    value: &impl Serialize,
) -> Result<PathBuf, String> {
    let destination = output_dir.join(filename);
    if destination.exists() {
        return Err("advanced note evidence output already exists".to_string());
    }
    let temporary = output_dir.join(format!(".{filename}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        serde_json::to_writer(&mut file, value).map_err(|error| error.to_string())?;
        file.write_all(b"\n")
            .and_then(|_| file.sync_all())
            .map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &destination)
            .map_err(|error| format!("could not atomically publish advanced notes: {error}"))?;
        Ok(destination.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

fn require_file_hash(path: &Path, expected: &str) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("advanced note generation file is unavailable: {error}"))?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || sha256(path)? != expected
    {
        return Err(format!(
            "advanced note generation hash mismatch: {}",
            path.display()
        ));
    }
    Ok(())
}

fn sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    std::io::copy(&mut file, &mut digest).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", digest.finalize()))
}

fn safe_relative(value: &str) -> bool {
    let path = Path::new(value);
    !value.contains('\\')
        && !value.contains(':')
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_segmentation_preserves_full_timeline_and_skips_only_unconditioned_chunks() {
        let words = vec![
            ConfigWord {
                id: "a".into(),
                text: "你".into(),
                start: 0,
                duration: 500_000,
            },
            ConfigWord {
                id: "b".into(),
                text: "好".into(),
                start: 2_000_000,
                duration: 1_500_000,
            },
        ];
        let segments = conditioned_segments(&words, 0, 600).unwrap();
        assert_eq!(
            segments.iter().map(|value| value.start).collect::<Vec<_>>(),
            [0, 256, 512]
        );
        assert_eq!(segments.last().unwrap().valid, 88);
    }

    #[test]
    fn seam_stitching_merges_only_identical_adjacent_pitch_claims() {
        let mut notes = vec![
            RawNote {
                start_frame: 0,
                end_frame: 256,
                pitch_logits: vec![1.0; PITCH_CLASSES],
                midi: Some(60),
            },
            RawNote {
                start_frame: 256,
                end_frame: 300,
                pitch_logits: vec![3.0; PITCH_CLASSES],
                midi: Some(60),
            },
            RawNote {
                start_frame: 300,
                end_frame: 320,
                pitch_logits: vec![0.0; PITCH_CLASSES],
                midi: Some(61),
            },
        ];
        stitch_notes(&mut notes);
        assert_eq!(notes.len(), 2);
        assert_eq!((notes[0].start_frame, notes[0].end_frame), (0, 300));
        assert_eq!(notes[0].pitch_logits[0], 2.0);
    }

    #[test]
    fn rmvpe_decoder_never_invents_pitch_below_the_upstream_threshold() {
        let mut values = vec![0.0; RMVPE_CLASSES];
        values[100] = 0.029;
        assert_eq!(decode_rmvpe_frame(&values), 0.0);
        values[100] = 0.5;
        assert!(decode_rmvpe_frame(&values) > 0.0);
    }
}
