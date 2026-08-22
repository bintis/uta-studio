use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use openvino::{Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use serde::{Deserialize, Serialize};

use crate::{kaldi_fbank, runtime};

const MANIFEST_SHA256: &str = "093335b6a113e5eead88bb011a7870d61f18319e8d0204523c3ce9d82e6c8c35";
const FEATURE_FRAMES: usize = 230;
const ENCODER_FRAMES: usize = 58;
const D_MODEL: usize = 1_280;
const VOCAB_SIZE: usize = 8_667;
const DECODER_LAYERS: usize = 16;
const SOS: i64 = 3;
const EOS: i64 = 4;

#[derive(Deserialize)]
struct Manifest {
    files: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct TranscriptEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    model_manifest_sha256: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    text: String,
    token_ids: Vec<i64>,
    ctc_frames: usize,
}

struct ModelFiles {
    directory: PathBuf,
    hashes: BTreeMap<String, String>,
}

impl ModelFiles {
    fn verified(&self, name: &str) -> Result<PathBuf, String> {
        let path = self.directory.join(name);
        let expected = self
            .hashes
            .get(name)
            .ok_or_else(|| format!("FireRed manifest is missing {name}"))?;
        if runtime::sha256(&path)? != *expected {
            return Err(format!("FireRed IR hash mismatch: {name}"));
        }
        Ok(path)
    }
}

fn model_files() -> Result<ModelFiles, String> {
    let root = std::env::var_os("UTA_STUDIO_MODELS_PATH")
        .map(PathBuf::from)
        .ok_or_else(|| "UTA_STUDIO_MODELS_PATH is not configured".to_string())?;
    let directory = root.join("firered-asr2-aed/openvino-ir-2026.3.0-smoke");
    let manifest_path = directory.join("manifest.json");
    if runtime::sha256(&manifest_path)? != MANIFEST_SHA256 {
        return Err("FireRed smoke IR manifest identity mismatch".to_string());
    }
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    Ok(ModelFiles {
        directory,
        hashes: manifest.files,
    })
}

fn tensor(element: ElementType, dimensions: &[i64]) -> Result<Tensor, String> {
    let shape = Shape::new(dimensions).map_err(|error| error.to_string())?;
    Tensor::new(element, &shape).map_err(|error| error.to_string())
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

fn read_graph(
    core: &mut Core,
    files: &ModelFiles,
    name: &str,
    bin: &str,
) -> Result<openvino::Model, String> {
    let xml = files.verified(name)?;
    let bin = files.verified(bin)?;
    core.read_model_from_file(
        xml.to_string_lossy().as_ref(),
        bin.to_string_lossy().as_ref(),
    )
    .map_err(|error| format!("could not load FireRed IR {name}: {error}"))
}

fn tokenizer(files: &ModelFiles) -> Result<BTreeMap<i64, String>, String> {
    let path = files.verified("tokens.txt")?;
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    text.lines()
        .map(|line| {
            let (token, id) = line
                .rsplit_once(' ')
                .ok_or_else(|| "FireRed token vocabulary is malformed".to_string())?;
            let id = id.parse::<i64>().map_err(|error| error.to_string())?;
            Ok((id, token.to_string()))
        })
        .collect()
}

pub fn infer(audio: &[f32], output_dir: &Path) -> Result<PathBuf, String> {
    let runtime_manifest = runtime::validate_runtime()?;
    let files = model_files()?;
    let cmvn = std::fs::read(files.verified("cmvn.ark")?).map_err(|error| error.to_string())?;
    let (features, feature_frames) = kaldi_fbank::extract(audio, &cmvn)?;
    if feature_frames != FEATURE_FRAMES {
        return Err(format!(
            "FireRed smoke IR requires {FEATURE_FRAMES} feature frames, got {feature_frames}"
        ));
    }
    let mut core = core()?;
    let encoder = read_graph(&mut core, &files, "encoder.xml", "encoder.bin")?;
    let mut encoder = core
        .compile_model(&encoder, DeviceType::GPU)
        .map_err(|error| format!("could not compile FireRed encoder: {error}"))?;
    let mut request = encoder
        .create_infer_request()
        .map_err(|error| error.to_string())?;
    let mut feature_tensor = tensor(ElementType::F32, &[1, FEATURE_FRAMES as i64, 80])?;
    feature_tensor
        .get_data_mut::<f32>()
        .map_err(|error| error.to_string())?
        .copy_from_slice(&features);
    let mut length_tensor = tensor(ElementType::I64, &[1])?;
    length_tensor
        .get_data_mut::<i64>()
        .map_err(|error| error.to_string())?[0] = FEATURE_FRAMES as i64;
    request
        .set_input_tensor_by_index(0, &feature_tensor)
        .map_err(|error| error.to_string())?;
    request
        .set_input_tensor_by_index(1, &length_tensor)
        .map_err(|error| error.to_string())?;
    request
        .infer()
        .map_err(|error| format!("FireRed encoder GPU inference failed: {error}"))?;
    let encoder_output = request
        .get_output_tensor_by_index(0)
        .map_err(|error| error.to_string())?
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    let mask = request
        .get_output_tensor_by_index(2)
        .map_err(|error| error.to_string())?
        .get_data::<bool>()
        .map_err(|error| error.to_string())?
        .to_vec();
    if encoder_output.len() != ENCODER_FRAMES * D_MODEL || mask.len() != ENCODER_FRAMES {
        return Err("FireRed encoder output contract mismatch".to_string());
    }
    drop(request);
    drop(encoder);

    let ctc = read_graph(&mut core, &files, "ctc.xml", "ctc.bin")?;
    let mut ctc = core
        .compile_model(&ctc, DeviceType::GPU)
        .map_err(|error| format!("could not compile FireRed CTC: {error}"))?;
    let mut ctc_request = ctc
        .create_infer_request()
        .map_err(|error| error.to_string())?;
    let mut ctc_input = tensor(
        ElementType::F32,
        &[1, ENCODER_FRAMES as i64, D_MODEL as i64],
    )?;
    ctc_input
        .get_data_mut::<f32>()
        .map_err(|error| error.to_string())?
        .copy_from_slice(&encoder_output);
    ctc_request
        .set_input_tensor(&ctc_input)
        .map_err(|error| error.to_string())?;
    ctc_request
        .infer()
        .map_err(|error| format!("FireRed CTC GPU inference failed: {error}"))?;
    let ctc_output = ctc_request
        .get_output_tensor()
        .map_err(|error| error.to_string())?;
    let ctc_data = ctc_output
        .get_data::<f32>()
        .map_err(|error| error.to_string())?;
    if ctc_data.len() != ENCODER_FRAMES * VOCAB_SIZE
        || !ctc_data.iter().all(|value| value.is_finite())
    {
        return Err("FireRed CTC output contract mismatch".to_string());
    }
    drop(ctc_request);
    drop(ctc);

    let mut tokens = vec![SOS];
    let mut caches = vec![Vec::<f32>::new(); DECODER_LAYERS];
    for step in 0..=10 {
        let name = format!("decoder-{step:02}.xml");
        let decoder = read_graph(&mut core, &files, &name, "decoder.bin")?;
        let mut decoder = core
            .compile_model(&decoder, DeviceType::GPU)
            .map_err(|error| format!("could not compile FireRed decoder step {step}: {error}"))?;
        let mut request = decoder
            .create_infer_request()
            .map_err(|error| error.to_string())?;
        let mut ys = tensor(ElementType::I64, &[1, tokens.len() as i64])?;
        ys.get_data_mut::<i64>()
            .map_err(|error| error.to_string())?
            .copy_from_slice(&tokens);
        let mut enc = tensor(
            ElementType::F32,
            &[1, ENCODER_FRAMES as i64, D_MODEL as i64],
        )?;
        enc.get_data_mut::<f32>()
            .map_err(|error| error.to_string())?
            .copy_from_slice(&encoder_output);
        let mut mask_tensor = tensor(ElementType::Boolean, &[1, 1, ENCODER_FRAMES as i64])?;
        mask_tensor
            .get_data_mut::<bool>()
            .map_err(|error| error.to_string())?
            .copy_from_slice(&mask);
        request
            .set_input_tensor_by_index(0, &ys)
            .map_err(|error| error.to_string())?;
        request
            .set_input_tensor_by_index(1, &enc)
            .map_err(|error| error.to_string())?;
        request
            .set_input_tensor_by_index(2, &mask_tensor)
            .map_err(|error| error.to_string())?;
        for (layer, cache) in caches.iter().enumerate() {
            let mut cache_tensor = tensor(ElementType::F32, &[1, step as i64, D_MODEL as i64])?;
            cache_tensor
                .get_data_mut::<f32>()
                .map_err(|error| error.to_string())?
                .copy_from_slice(cache);
            request
                .set_input_tensor_by_index(3 + layer, &cache_tensor)
                .map_err(|error| error.to_string())?;
        }
        request
            .infer()
            .map_err(|error| format!("FireRed decoder GPU inference failed: {error}"))?;
        let logits = request
            .get_output_tensor_by_index(0)
            .map_err(|error| error.to_string())?;
        let logits = logits
            .get_data::<f32>()
            .map_err(|error| error.to_string())?;
        let next = logits
            .iter()
            .copied()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index as i64)
            .ok_or_else(|| "FireRed decoder returned no logits".to_string())?;
        let mut next_caches = Vec::with_capacity(DECODER_LAYERS);
        for layer in 0..DECODER_LAYERS {
            next_caches.push(
                request
                    .get_output_tensor_by_index(1 + layer)
                    .map_err(|error| error.to_string())?
                    .get_data::<f32>()
                    .map_err(|error| error.to_string())?
                    .to_vec(),
            );
        }
        caches = next_caches;
        tokens.push(next);
        if next == EOS {
            break;
        }
    }
    let output_tokens = tokens
        .iter()
        .copied()
        .skip(1)
        .take_while(|token| *token != EOS)
        .collect::<Vec<_>>();
    let vocabulary = tokenizer(&files)?;
    let text = output_tokens
        .iter()
        .filter_map(|token| vocabulary.get(token))
        .map(|token| token.replace('▁', " "))
        .collect::<String>()
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("FireRed decoder returned an empty transcript".to_string());
    }
    let destination = output_dir.join("firered-transcript-evidence.json");
    let temporary = output_dir.join("firered-transcript-evidence.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    serde_json::to_writer(
        &mut file,
        &TranscriptEvidence {
            schema_version: 1,
            model_id: "firered_asr2_aed",
            model_manifest_sha256: MANIFEST_SHA256,
            runtime_manifest_sha256: &runtime_manifest,
            backend: "openvino_gpu",
            text,
            token_ids: output_tokens,
            ctc_frames: ENCODER_FRAMES,
        },
    )
    .map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    Ok(destination)
}
