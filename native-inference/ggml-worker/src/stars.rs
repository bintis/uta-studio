use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uta_ggml_runtime::stars::{
    GlobalStyle, PITCH_CLASSES, RawNote, RawTechnique, Stars, TECHNIQUE_TAXONOMY, TranscriptWord,
};
use uta_ggml_runtime::{DeviceDescriptor, GgmlRuntime};

use crate::pitch_input::read_shared_rmvpe;
use crate::stars_g2p::{ChineseG2pAsset, PROFILE};

const UPSTREAM_COMMIT: &str = "f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167";
const FRONTEND_PROFILE: &str = "shared-singing-frontend-24k-v1";

#[derive(Debug, Deserialize)]
struct Request {
    timed_transcript: Vec<TranscriptWordConfig>,
    #[serde(default)]
    source_start_micros: u64,
    #[serde(default = "default_true")]
    include_notes: bool,
    #[serde(default = "default_true")]
    include_technique: bool,
    model_content_digest: String,
    model_generation: String,
    rmvpe_model_content_digest: String,
    rmvpe_generation: String,
    transcript_generation: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranscriptWordConfig {
    id: String,
    text: String,
    start_micros: u64,
    duration_micros: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    schema_version: u32,
    model_id: String,
    capability: Option<String>,
    capabilities: Vec<String>,
    upstream_commit: String,
    checkpoint_sha256: String,
    config_sha256: String,
    model_generation: String,
    runtime_manifest_sha256: String,
    backend: String,
    shared_frontend_profile: String,
    shared_frontend_generation: String,
    annotation_rmvpe_sha256: String,
    word_boundary_source: String,
    g2p_profile: Option<String>,
    frame_step_num: u32,
    frame_step_den: u32,
    valid_frames: usize,
    note_boundary_logits: Vec<f32>,
    regulated_note_boundaries: Vec<usize>,
    notes: Vec<NoteEvidence>,
    technique_taxonomy: Option<Vec<String>>,
    technique_calibration: Option<String>,
    techniques: Option<Vec<TechniqueEvidence>>,
    style_scope: Option<String>,
    styles: Option<Vec<StyleEvidence>>,
    dependencies: Vec<DependencyIdentity>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NoteEvidence {
    start_frame: usize,
    end_frame: usize,
    pitch_logits: Vec<f32>,
    midi: Option<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TechniqueEvidence {
    start_frame: usize,
    end_frame: usize,
    phoneme_id: i64,
    raw_logits: Vec<f32>,
    source_local_scores: Vec<f32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StyleEvidence {
    start_frame: usize,
    end_frame: usize,
    heads: BTreeMap<String, StyleHead>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StyleHead {
    taxonomy: Vec<String>,
    raw_logits: Vec<f32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DependencyIdentity {
    kind: String,
    generation: String,
}

#[allow(clippy::too_many_arguments)]
pub fn infer(
    runtime: Arc<GgmlRuntime>,
    device: &DeviceDescriptor,
    stars_model: &Path,
    stars_wav: &Path,
    rmvpe_evidence: &Path,
    runtime_manifest_digest: &str,
    backend: &str,
    config: &serde_json::Value,
    destination: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let request: Request = serde_json::from_value(config.clone())
        .map_err(|error| format!("STARS request is invalid: {error}"))?;
    validate_request(&request)?;
    let words = request
        .timed_transcript
        .iter()
        .map(|word| TranscriptWord {
            id: word.id.clone(),
            text: word.text.clone(),
            start_micros: word.start_micros,
            duration_micros: word.duration_micros,
        })
        .collect::<Vec<_>>();
    let raw_f0 = read_shared_rmvpe(rmvpe_evidence)?;
    progress(200, 1000);
    let shared = uta_ggml_runtime::stars::prepare_wav_inputs(stars_wav, &raw_f0)?;
    let g2p = ChineseG2pAsset::load_embedded()?;
    let stars = Stars::load(runtime, device, stars_model)?;
    progress(250, 1000);
    let result = stars.infer_transcript(
        &shared,
        &words,
        request.source_start_micros,
        request.include_technique,
        |texts| g2p.phonemize_words(texts),
        |_, _| {},
    )?;
    progress(950, 1000);
    let frontend_generation = format!("rmvpe:{}", request.rmvpe_generation);
    let mut capabilities = Vec::new();
    if request.include_notes {
        capabilities.push("notes.stars".to_string());
    }
    if request.include_technique {
        capabilities.push("technique.analyze".to_string());
    }
    let (technique_taxonomy, technique_calibration, techniques, style_scope, styles) =
        if request.include_technique {
            (
                Some(TECHNIQUE_TAXONOMY.iter().map(ToString::to_string).collect()),
                Some("source_local_sigmoid_uncalibrated".to_string()),
                Some(
                    result
                        .techniques
                        .ok_or("STARS omitted requested technique evidence")?
                        .into_iter()
                        .map(technique_evidence)
                        .collect(),
                ),
                Some("segment_global".to_string()),
                Some(
                    result
                        .styles
                        .ok_or("STARS omitted requested style evidence")?
                        .into_iter()
                        .map(style_evidence)
                        .collect(),
                ),
            )
        } else {
            (None, None, None, None, None)
        };
    let evidence = Evidence {
        schema_version: 1,
        model_id: "stars".to_string(),
        capability: None,
        capabilities,
        upstream_commit: UPSTREAM_COMMIT.to_string(),
        checkpoint_sha256: request.model_content_digest,
        config_sha256: "stars-native-ggml-config-v1".to_string(),
        model_generation: request.model_generation,
        runtime_manifest_sha256: runtime_manifest_digest.to_string(),
        backend: backend.to_string(),
        shared_frontend_profile: FRONTEND_PROFILE.to_string(),
        shared_frontend_generation: frontend_generation.clone(),
        annotation_rmvpe_sha256: request.rmvpe_model_content_digest,
        word_boundary_source: "timed_transcript".to_string(),
        g2p_profile: Some(PROFILE.to_string()),
        frame_step_num: 128,
        frame_step_den: 24_000,
        valid_frames: result.valid_frames,
        note_boundary_logits: result.note_boundary_logits,
        regulated_note_boundaries: result.regulated_note_boundaries,
        notes: result.notes.into_iter().map(note_evidence).collect(),
        technique_taxonomy,
        technique_calibration,
        techniques,
        style_scope,
        styles,
        dependencies: vec![
            DependencyIdentity {
                kind: "shared_frontend".to_string(),
                generation: frontend_generation.clone(),
            },
            DependencyIdentity {
                kind: "annotation_rmvpe".to_string(),
                generation: frontend_generation,
            },
            DependencyIdentity {
                kind: "timed_transcript".to_string(),
                generation: request.transcript_generation,
            },
            DependencyIdentity {
                kind: "chinese_g2p".to_string(),
                generation: PROFILE.to_string(),
            },
        ],
    };
    validate_evidence(&evidence)?;
    write_evidence(destination, &evidence)?;
    progress(1000, 1000);
    Ok(())
}

pub fn publish(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err("STARS evidence destination already exists".to_string());
    }
    let bytes = std::fs::read(source)
        .map_err(|error| format!("could not read raw STARS evidence: {error}"))?;
    let evidence: Evidence = serde_json::from_slice(&bytes)
        .map_err(|error| format!("raw STARS evidence is invalid: {error}"))?;
    validate_evidence(&evidence)?;
    let (temporary, mut file) = create_publication_temporary(destination)?;
    if let Err(error) = encode_evidence(&mut file, &evidence) {
        drop(file);
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    let publish = std::fs::hard_link(&temporary, destination)
        .map_err(|error| format!("could not publish STARS evidence without overwrite: {error}"));
    let cleanup = std::fs::remove_file(&temporary)
        .map_err(|error| format!("could not remove STARS temporary evidence: {error}"));
    publish.and(cleanup)
}

fn validate_request(request: &Request) -> Result<(), String> {
    if request.timed_transcript.is_empty()
        || (!request.include_notes && !request.include_technique)
        || [
            request.model_content_digest.as_str(),
            request.model_generation.as_str(),
            request.rmvpe_model_content_digest.as_str(),
            request.rmvpe_generation.as_str(),
            request.transcript_generation.as_str(),
        ]
        .iter()
        .any(|value| value.trim().is_empty())
        || request.timed_transcript.iter().any(|word| {
            word.id.trim().is_empty()
                || word.text.trim().is_empty()
                || word.duration_micros == 0
                || word
                    .start_micros
                    .checked_add(word.duration_micros)
                    .is_none()
        })
    {
        return Err("STARS request contract is invalid".to_string());
    }
    Ok(())
}

fn validate_evidence(evidence: &Evidence) -> Result<(), String> {
    let capabilities_valid = matches!(
        evidence.capabilities.as_slice(),
        [notes] if notes == "notes.stars"
    ) || matches!(
        evidence.capabilities.as_slice(),
        [technique] if technique == "technique.analyze"
    ) || matches!(
        evidence.capabilities.as_slice(),
        [notes, technique] if notes == "notes.stars" && technique == "technique.analyze"
    );
    if evidence.schema_version != 1
        || evidence.model_id != "stars"
        || evidence.capability.is_some()
        || !capabilities_valid
        || evidence.upstream_commit != UPSTREAM_COMMIT
        || !matches!(evidence.backend.as_str(), "ggml_cpu" | "ggml_vulkan")
        || evidence.shared_frontend_profile != FRONTEND_PROFILE
        || evidence.g2p_profile.as_deref() != Some(PROFILE)
        || evidence.frame_step_num != 128
        || evidence.frame_step_den != 24_000
        || evidence.valid_frames == 0
        || evidence.note_boundary_logits.len() != evidence.valid_frames
        || evidence.notes.is_empty()
        || evidence.dependencies.len() != 4
        || evidence.notes.iter().any(|note| {
            note.start_frame >= note.end_frame
                || note.end_frame > evidence.valid_frames
                || note.pitch_logits.len() != PITCH_CLASSES
                || note.pitch_logits.iter().any(|value| !value.is_finite())
        })
    {
        return Err("raw STARS evidence identity or shape is invalid".to_string());
    }
    let techniques_requested = evidence
        .capabilities
        .iter()
        .any(|capability| capability == "technique.analyze");
    if techniques_requested
        != (evidence.technique_taxonomy.is_some()
            && evidence.technique_calibration.is_some()
            && evidence.techniques.is_some()
            && evidence.style_scope.is_some()
            && evidence.styles.is_some())
    {
        return Err("raw STARS technique presence is invalid".to_string());
    }
    Ok(())
}

fn create_publication_temporary(
    destination: &Path,
) -> Result<(std::path::PathBuf, std::fs::File), String> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..16 {
        let path = destination
            .with_extension(format!("json.{}.{nonce}.{attempt}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "could not create STARS publication temporary: {error}"
                ));
            }
        }
    }
    Err("could not allocate a unique STARS publication temporary".to_string())
}

fn write_evidence(path: &Path, evidence: &Evidence) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("could not create STARS evidence: {error}"))?;
    encode_evidence(&mut file, evidence)
}

fn encode_evidence(file: &mut std::fs::File, evidence: &Evidence) -> Result<(), String> {
    serde_json::to_writer(&mut *file, evidence)
        .map_err(|error| format!("could not encode STARS evidence: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("could not finish STARS evidence: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not sync STARS evidence: {error}"))
}

fn note_evidence(note: RawNote) -> NoteEvidence {
    NoteEvidence {
        start_frame: note.start_frame,
        end_frame: note.end_frame,
        pitch_logits: note.pitch_logits,
        midi: note.midi,
    }
}

fn technique_evidence(value: RawTechnique) -> TechniqueEvidence {
    TechniqueEvidence {
        start_frame: value.start_frame,
        end_frame: value.end_frame,
        phoneme_id: value.phoneme_id,
        raw_logits: value.raw_logits,
        source_local_scores: value.source_local_scores,
    }
}

fn style_evidence(value: GlobalStyle) -> StyleEvidence {
    let heads = [
        (
            "technique_group",
            &[
                "control",
                "mixed",
                "falsetto",
                "pharyngeal",
                "glissando",
                "vibrato",
                "breathy",
                "weak",
                "strong",
                "bubble",
            ][..],
            value.logits.technique_group,
        ),
        (
            "language",
            &[
                "Chinese", "English", "Italian", "French", "Japanese", "Spanish", "German",
                "Korean", "Russian",
            ][..],
            value.logits.language,
        ),
        ("gender", &["female", "male"][..], value.logits.gender),
        (
            "emotion",
            &["neutral", "happy", "sad", "angry"][..],
            value.logits.emotion,
        ),
        ("method", &["pop", "bel_canto"][..], value.logits.method),
        ("pace", &["slow", "moderate", "fast"][..], value.logits.pace),
        ("range", &["low", "medium", "high"][..], value.logits.range),
    ]
    .into_iter()
    .map(|(name, taxonomy, raw_logits)| {
        (
            name.to_string(),
            StyleHead {
                taxonomy: taxonomy.iter().map(ToString::to_string).collect(),
                raw_logits,
            },
        )
    })
    .collect();
    StyleEvidence {
        start_frame: value.start_frame,
        end_frame: value.end_frame,
        heads,
    }
}

fn default_true() -> bool {
    true
}
