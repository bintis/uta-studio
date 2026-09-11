//! GGML-only real-device fixture; shared transcript host code has no backend dependency.
use super::transcript::load_vocabulary;
use super::*;
use crate::{DeviceKind, GgmlRuntime};
use std::path::PathBuf;

fn path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {name}"))
}

#[test]
#[ignore = "requires an explicit packaged runtime, device, rewritten FireRed GGUF, WAV, CMVN, and token vocabulary"]
fn actual_firered_transcribes_a_reference_wav_end_to_end() {
    let runtime = GgmlRuntime::load(&path("UTA_TEST_GGML_RUNTIME_DIR")).unwrap();
    let expected_kind = match std::env::var("UTA_TEST_GGML_DEVICE_KIND")
        .expect("set UTA_TEST_GGML_DEVICE_KIND")
        .as_str()
    {
        "cpu" => DeviceKind::Cpu,
        "integrated_gpu" => DeviceKind::IntegratedGpu,
        "discrete_gpu" => DeviceKind::DiscreteGpu,
        other => panic!("unsupported test device kind: {other}"),
    };
    let description = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION").unwrap_or_default();
    let device = runtime
        .devices()
        .unwrap()
        .into_iter()
        .find(|device| device.kind == expected_kind && device.description.contains(&description))
        .expect("requested FireRed test device is unavailable");
    let model = FireRed::load(runtime, &device, &path("UTA_TEST_FIRERED_GGUF")).unwrap();
    let cmvn = std::fs::read(path("UTA_TEST_FIRERED_CMVN")).unwrap();
    let vocabulary_bytes = std::fs::read(path("UTA_TEST_FIRERED_TOKENS")).unwrap();
    let samples =
        crate::wav::read_f32_wav(&path("UTA_TEST_FIRERED_WAV"), SAMPLE_RATE as u32, 1).unwrap();
    let mut padded = vec![0.0_f32; samples.len().max(MIN_WINDOW_SAMPLES)];
    padded[..samples.len()].copy_from_slice(&samples);
    let (features, frames) = extract_features(&padded, &cmvn).unwrap();
    assert_eq!(frames, FEATURE_FRAMES);
    let encoded = model.encode(&features).unwrap();
    let generated = model.greedy_decode(&encoded).unwrap();
    let vocabulary = load_vocabulary(&vocabulary_bytes).unwrap();
    eprintln!(
        "FireRed whole-chain tokens: {generated:?} -> {:?}",
        generated
            .iter()
            .map(|token| vocabulary[*token as usize].clone())
            .collect::<Vec<_>>()
    );
    let transcription = model
        .transcribe_wav(
            &path("UTA_TEST_FIRERED_WAV"),
            &cmvn,
            &vocabulary_bytes,
            |_, _| {},
        )
        .unwrap();
    eprintln!(
        "FireRed transcript: {:?} tokens {:?}",
        transcription.text, transcription.token_ids
    );
    assert!(!transcription.text.is_empty());
}
