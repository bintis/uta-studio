//! Explicit GGML-device fixtures, separate from reusable host ASR algorithms.
use super::asr::DEFAULT_MAX_NEW_TOKENS;
use super::Qwen;
use crate::{DeviceKind, GgmlRuntime};
use std::path::PathBuf;

fn path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {name}"))
}

#[test]
#[ignore = "requires an explicit packaged runtime, device, Qwen ASR GGUF, and raw 16 kHz F32 speech"]
fn actual_asr_transcription_matches_historical_tokens() {
    let runtime = GgmlRuntime::load(&path("UTA_TEST_GGML_RUNTIME_DIR")).unwrap();
    let requested_kind =
        std::env::var("UTA_TEST_GGML_DEVICE_KIND").expect("set UTA_TEST_GGML_DEVICE_KIND");
    let expected_kind = match requested_kind.as_str() {
        "cpu" => DeviceKind::Cpu,
        "integrated_gpu" => DeviceKind::IntegratedGpu,
        other => panic!("unsupported test device kind: {other}"),
    };
    let description = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION")
        .expect("set UTA_TEST_GGML_DEVICE_DESCRIPTION");
    let device = runtime
        .devices()
        .unwrap()
        .into_iter()
        .find(|device| device.kind == expected_kind && device.description.contains(&description))
        .expect("requested Qwen ASR test device is unavailable");
    let model = Qwen::load(runtime, &device, &path("UTA_TEST_QWEN_GGUF")).unwrap();
    let bytes = std::fs::read(path("UTA_TEST_QWEN_ASR_F32")).unwrap();
    assert!(bytes.len().is_multiple_of(4));
    let samples = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect::<Vec<_>>();
    let transcription = model
        .transcribe(&samples, DEFAULT_MAX_NEW_TOKENS, None)
        .unwrap();
    assert_eq!(
        transcription.generated_tokens,
        [11_528, 6_364, 151_704, 2_403, 566, 1_101, 374, 369, 601, 13, 151_645]
    );
    assert_eq!(transcription.language_name.as_deref(), Some("English"));
    assert_eq!(transcription.text, "All he just is for us.");
    assert!(transcription.finished);
    assert_eq!(transcription.prompt_tokens, 171);
}
