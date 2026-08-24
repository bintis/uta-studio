use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use openvino::{Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use serde::{Deserialize, Serialize};

use crate::{kaldi_fbank, runtime};

const MANIFEST_SHA256: &str = "093335b6a113e5eead88bb011a7870d61f18319e8d0204523c3ce9d82e6c8c35";
const MIN_WINDOW_SAMPLES: usize = 37_040;
const MAX_WINDOW_SAMPLES: usize = 37_199;
const FEATURE_FRAMES: usize = 230;
const ENCODER_FRAMES: usize = 58;
const D_MODEL: usize = 1_280;
const VOCAB_SIZE: usize = 8_667;
const DECODER_LAYERS: usize = 16;
const SOS: i64 = 3;
const EOS: i64 = 4;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    model_id: String,
    format: String,
    source_revision: String,
    source_hashes: BTreeMap<String, String>,
    fixture_contract: FixtureContract,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureContract {
    feature_frames: usize,
    encoder_frames: usize,
    decoder_cache_max: usize,
}

#[derive(Serialize)]
struct TranscriptEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    selected_source_revision: &'a str,
    source_graph_sha256: &'a BTreeMap<String, String>,
    model_manifest_sha256: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    contract_scope: &'a str,
    input_samples: usize,
    window_samples: usize,
    window_count: usize,
    feature_frames: usize,
    encoder_frames: usize,
    decoder_cache_max: usize,
    text: String,
    token_ids: Vec<i64>,
    ctc_frames: usize,
    windows: Vec<WindowEvidence>,
}

#[derive(Serialize)]
struct WindowEvidence {
    index: usize,
    start_sample: usize,
    end_sample: usize,
    text: String,
    token_ids: Vec<i64>,
}

struct WindowResult {
    text: String,
    token_ids: Vec<i64>,
}

struct ModelFiles {
    directory: PathBuf,
    source_revision: String,
    source_hashes: BTreeMap<String, String>,
    hashes: BTreeMap<String, String>,
}

impl ModelFiles {
    fn verified(&self, name: &str) -> Result<PathBuf, String> {
        if !self.hashes.contains_key(name) {
            return Err(format!("FireRed manifest is missing {name}"));
        }
        Ok(self.directory.join(name))
    }
}

fn model_files(config: &serde_json::Value) -> Result<ModelFiles, String> {
    let directory = config
        .get("model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "FireRed requires Runtime Manager-resolved config.model_path".to_string())?;
    if !directory.is_dir() {
        return Err("resolved FireRed model generation is unavailable".to_string());
    }
    let manifest_path = directory.join("manifest.json");
    if runtime::sha256(&manifest_path)? != MANIFEST_SHA256 {
        return Err("FireRed smoke IR manifest identity mismatch".to_string());
    }
    let manifest =
        parse_manifest(&std::fs::read(manifest_path).map_err(|error| error.to_string())?)?;
    for (name, expected) in &manifest.files {
        if Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name.as_str()) {
            return Err("FireRed manifest contains an unsafe filename".to_string());
        }
        if runtime::sha256(&directory.join(name))? != *expected {
            return Err(format!("FireRed IR hash mismatch: {name}"));
        }
    }
    Ok(ModelFiles {
        directory,
        source_revision: manifest.source_revision,
        source_hashes: manifest.source_hashes,
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
    crate::runtime::configure_low_impact_gpu_queue(&mut core)?;
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

pub fn infer(
    audio: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
) -> Result<PathBuf, String> {
    if audio.is_empty() {
        return Err("FireRed input is empty".to_string());
    }
    let runtime_manifest = runtime::validate_runtime()?;
    let files = model_files(config)?;
    let cmvn = std::fs::read(files.verified("cmvn.ark")?).map_err(|error| error.to_string())?;
    let vocabulary = tokenizer(&files)?;
    let mut core = core()?;
    let ranges = window_ranges(audio.len());
    let mut windows = Vec::with_capacity(ranges.len());
    let mut token_ids = Vec::new();
    let mut texts = Vec::new();
    for (index, (start_sample, end_sample)) in ranges.into_iter().enumerate() {
        let result = infer_window(
            &audio[start_sample..end_sample],
            &files,
            &cmvn,
            &vocabulary,
            &mut core,
        )?;
        if !result.text.is_empty() {
            texts.push(result.text.clone());
        }
        token_ids.extend_from_slice(&result.token_ids);
        windows.push(WindowEvidence {
            index,
            start_sample,
            end_sample,
            text: result.text,
            token_ids: result.token_ids,
        });
    }
    let text = texts.join(" ");
    if text.is_empty() {
        return Err("FireRed decoder returned no transcript across all windows".to_string());
    }
    let destination = output_dir.join("firered-transcript-evidence.json");
    let temporary = output_dir.join("firered-transcript-evidence.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    serde_json::to_writer(
        &mut file,
        &TranscriptEvidence {
            schema_version: 3,
            model_id: "firered_asr2_aed",
            selected_source_revision: &files.source_revision,
            source_graph_sha256: &files.source_hashes,
            model_manifest_sha256: MANIFEST_SHA256,
            runtime_manifest_sha256: &runtime_manifest,
            backend: "openvino_gpu",
            contract_scope: "windowed_230_feature_frame_sequence",
            input_samples: audio.len(),
            window_samples: MAX_WINDOW_SAMPLES,
            window_count: windows.len(),
            feature_frames: FEATURE_FRAMES,
            encoder_frames: ENCODER_FRAMES,
            decoder_cache_max: 10,
            text,
            token_ids,
            ctc_frames: ENCODER_FRAMES,
            windows,
        },
    )
    .map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    Ok(destination)
}

fn window_ranges(samples: usize) -> Vec<(usize, usize)> {
    (0..samples)
        .step_by(MAX_WINDOW_SAMPLES)
        .map(|start| (start, (start + MAX_WINDOW_SAMPLES).min(samples)))
        .collect()
}

fn infer_window(
    audio: &[f32],
    files: &ModelFiles,
    cmvn: &[u8],
    vocabulary: &BTreeMap<i64, String>,
    core: &mut Core,
) -> Result<WindowResult, String> {
    if audio.is_empty() || audio.len() > MAX_WINDOW_SAMPLES {
        return Err("FireRed internal window shape is invalid".to_string());
    }
    let mut window = vec![0.0_f32; audio.len().max(MIN_WINDOW_SAMPLES)];
    window[..audio.len()].copy_from_slice(audio);
    let (features, feature_frames) = kaldi_fbank::extract(&window, cmvn)?;
    if feature_frames != FEATURE_FRAMES {
        return Err(format!(
            "FireRed smoke IR requires {FEATURE_FRAMES} feature frames, got {feature_frames}"
        ));
    }
    let encoder = read_graph(core, files, "encoder.xml", "encoder.bin")?;
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

    let ctc = read_graph(core, files, "ctc.xml", "ctc.bin")?;
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
        let decoder = read_graph(core, files, &name, "decoder.bin")?;
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
        .filter(|token| {
            vocabulary
                .get(token)
                .is_some_and(|value| is_lexical_token(value))
        })
        .collect::<Vec<_>>();
    let text = output_tokens
        .iter()
        .filter_map(|token| vocabulary.get(token))
        .map(|token| token.replace('▁', " "))
        .collect::<String>()
        .trim()
        .to_string();
    Ok(WindowResult {
        text,
        token_ids: output_tokens,
    })
}

fn is_lexical_token(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !(value.starts_with('<') && value.ends_with('>'))
}

fn parse_manifest(bytes: &[u8]) -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let expected_source_hashes = [
        (
            "encoder",
            "0fe4038f5e5cd340171535b7b5f2e184482e90e22aeb2ed0f7abe81af10783f9",
        ),
        (
            "decoder",
            "aeef22670d95aa90d78a1927242c2a6e4fbb8b44c1af8d3ae988c46fd67ae833",
        ),
        (
            "ctc",
            "8881d31c17bca30a7972299d5395daaa6424da6328a818ba496719c3118c32b4",
        ),
    ];
    if manifest.schema_version != 1
        || manifest.model_id != "firered_asr2_aed"
        || manifest.format != "openvino_ir_v11_smoke_buckets"
        || manifest.source_revision
            != "42ailab/FireRedASR2-AED-ONNX@13f950858934f7b6a0d3ce52bae65af0dc022258"
        || manifest.fixture_contract.feature_frames != FEATURE_FRAMES
        || manifest.fixture_contract.encoder_frames != ENCODER_FRAMES
        || manifest.fixture_contract.decoder_cache_max != 10
        || manifest.source_hashes.len() != expected_source_hashes.len()
        || expected_source_hashes.iter().any(|(name, digest)| {
            manifest.source_hashes.get(*name).map(String::as_str) != Some(*digest)
        })
    {
        return Err("FireRed smoke IR manifest contract is incompatible".to_string());
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_tokens_are_not_published_as_transcript_text() {
        assert!(!is_lexical_token("<sil>"));
        assert!(!is_lexical_token("<blank>"));
        assert!(!is_lexical_token("<unk>"));
        assert!(is_lexical_token("▁hello"));
        assert!(is_lexical_token("你"));
    }

    #[test]
    fn window_plan_is_deterministic_contiguous_and_complete() {
        let samples = MAX_WINDOW_SAMPLES * 3 + 17;
        let ranges = window_ranges(samples);
        assert_eq!(ranges.len(), 4);
        assert_eq!(ranges[0], (0, MAX_WINDOW_SAMPLES));
        assert_eq!(ranges[3], (MAX_WINDOW_SAMPLES * 3, samples));
        assert!(ranges.windows(2).all(|pair| pair[0].1 == pair[1].0));
        assert_eq!(
            ranges.iter().map(|(start, end)| end - start).sum::<usize>(),
            samples
        );
    }

    #[test]
    fn manifest_contract_is_typed_and_fixed_window() {
        let fixture = serde_json::json!({
            "schema_version": 1,
            "model_id": "firered_asr2_aed",
            "format": "openvino_ir_v11_smoke_buckets",
            "source_revision": "42ailab/FireRedASR2-AED-ONNX@13f950858934f7b6a0d3ce52bae65af0dc022258",
            "source_hashes": {
                "encoder": "0fe4038f5e5cd340171535b7b5f2e184482e90e22aeb2ed0f7abe81af10783f9",
                "decoder": "aeef22670d95aa90d78a1927242c2a6e4fbb8b44c1af8d3ae988c46fd67ae833",
                "ctc": "8881d31c17bca30a7972299d5395daaa6424da6328a818ba496719c3118c32b4"
            },
            "fixture_contract": {
                "feature_frames": 230,
                "encoder_frames": 58,
                "decoder_cache_max": 10
            },
            "files": {}
        });
        let manifest = parse_manifest(&serde_json::to_vec(&fixture).unwrap()).unwrap();
        assert_eq!(manifest.fixture_contract.feature_frames, 230);
        assert_eq!(manifest.fixture_contract.encoder_frames, 58);
        assert_eq!(manifest.fixture_contract.decoder_cache_max, 10);
    }
}
