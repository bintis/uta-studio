//! Explicit real-model comparison for backend-only attention experiments.
//! The graph, chunking and reconstruction are the production Roformer methods.

use std::path::PathBuf;
use std::time::Instant;

use crate::roformer::Roformer;
use crate::wav::read_f32_wav;
use crate::{DeviceKind, GgmlRuntime};

fn configured_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name} explicitly")))
}

#[test]
#[ignore = "explicit B580 and installed-model authorization; creates new diagnostic outputs"]
fn b580_roformer_model_comparison() {
    let libraries = configured_path("UTA_TEST_GGML_RUNTIME_DIR");
    let model_path = configured_path("UTA_TEST_ROFORMER_MODEL");
    let input_path = configured_path("UTA_TEST_ROFORMER_INPUT");
    let output_directory = configured_path("UTA_TEST_ROFORMER_OUTPUT");
    let input = read_f32_wav(&input_path, 44_100, 2).expect("read-only input fixture");
    std::fs::create_dir(&output_directory).expect("use a new diagnostic output directory");
    let runtime = GgmlRuntime::load(&libraries).expect("load explicitly selected libraries");
    let device = runtime
        .devices()
        .expect("enumerate GGML devices")
        .into_iter()
        .find(|device| {
            device.kind == DeviceKind::DiscreteGpu && device.description.contains("B580")
        })
        .expect("B580 unavailable; no fallback");
    eprintln!("model comparison device: {device:?}; libraries: {libraries:?}");
    let start = Instant::now();
    let mut model =
        Roformer::load(runtime, &device, &model_path).expect("load real Roformer graph");
    let load_seconds = start.elapsed().as_secs_f64();
    let mut previous: Option<Vec<f32>> = None;
    for pass in 0..2 {
        let output = output_directory.join(format!("estimate-{pass}.wav"));
        let start = Instant::now();
        model
            .process_wav(&input_path, &output, |_, _| {})
            .expect("real model execution");
        let process_seconds = start.elapsed().as_secs_f64();
        let estimate = read_f32_wav(&output, 44_100, 2).expect("read diagnostic output");
        assert_eq!(estimate.len(), input.len(), "output timeline mismatch");
        assert!(estimate.iter().all(|sample| sample.is_finite()));
        let energy: f64 = estimate
            .iter()
            .map(|&sample| f64::from(sample).powi(2))
            .sum();
        assert!(energy > 0.0, "unexpected silent fixture output");
        let repeat_max_abs = previous.as_ref().map(|old| {
            old.iter()
                .zip(&estimate)
                .map(|(&a, &b)| (f64::from(a) - f64::from(b)).abs())
                .fold(0.0_f64, f64::max)
        });
        eprintln!(
            "ATTENTION_MODEL_RESULT {}",
            serde_json::json!({
                "pass": pass, "model": model_path, "input": input_path, "output": output,
                "libraries": libraries, "load_seconds": load_seconds,
                "process_seconds": process_seconds, "samples": estimate.len(),
                "sample_rate": 44_100, "channels": 2,
                "output_rms": (energy / estimate.len() as f64).sqrt(),
                "repeat_max_abs": repeat_max_abs,
                "scope": "real production Roformer graph including WAV frontend and synthesis; excludes worker codec/publication"
            })
        );
        previous = Some(estimate);
    }
}
