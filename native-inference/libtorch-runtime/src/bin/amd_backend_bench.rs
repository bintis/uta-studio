//! Diagnostic benchmark: one explicitly selected AMD backend and one resident
//! model per process. Runtime/weight loading, cold inference and warm inference
//! are separate measurements. Speech rows are encoder workloads, not ASR speed.
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use uta_ggml_runtime as ggml;
use uta_libtorch_runtime as torch;

type Runner = Box<dyn FnMut(&Path) -> Result<String, String>>;
struct Plan {
    runner: Runner,
    runtime_seconds: f64,
    model_seconds: f64,
    device: String,
    precision: String,
}

fn emit(value: &Value) {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value).expect("write benchmark record");
    writeln!(stdout).expect("finish benchmark record");
    stdout.flush().expect("flush benchmark record");
}

fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    if arguments.len() != 5 {
        eprintln!(
            "usage: amd_backend_bench <ggml|libtorch> <resource> <model.gguf> <canonical.wav>"
        );
        std::process::exit(2);
    }
    let backend = &arguments[1];
    let resource = &arguments[2];
    let input = Path::new(&arguments[4]);
    let mut report = json!({
        "event": "result", "backend": backend, "resource": resource,
        "scope": scope(resource), "status": "failed",
        "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "input": input, "model_path": arguments[3],
        "inference_excludes": ["runtime_load", "weight_load", "model_destruction", "source_resampling"],
        "output_parity_qualified": false,
    });
    let result = run(
        backend,
        resource,
        Path::new(&arguments[3]),
        input,
        &mut report,
    );
    match result {
        Ok(()) => report["status"] = json!("passed"),
        Err(error) => report["error"] = json!(error),
    }
    emit(&report);
    if result_is_failure(&report) {
        std::process::exit(1);
    }
}

fn result_is_failure(report: &Value) -> bool {
    report["status"] != "passed"
}

fn run(
    backend: &str,
    resource: &str,
    model_path: &Path,
    input: &Path,
    report: &mut Value,
) -> Result<(), String> {
    let reader = hound::WavReader::open(input).map_err(|error| error.to_string())?;
    let spec = reader.spec();
    let samples = reader.duration();
    report["input_samples_per_channel"] = json!(samples);
    report["input_sample_rate"] = json!(spec.sample_rate);
    report["input_channels"] = json!(spec.channels);
    report["input_seconds"] = json!(f64::from(samples) / f64::from(spec.sample_rate));
    drop(reader);
    let warm_runs = std::env::var("UTA_STUDIO_BENCH_WARM_RUNS")
        .unwrap_or_else(|_| "1".into())
        .parse::<usize>()
        .map_err(|_| "UTA_STUDIO_BENCH_WARM_RUNS must be an unsigned integer".to_string())?;
    emit(
        &json!({"event":"begin", "backend":backend, "resource":resource, "input_seconds":report["input_seconds"], "warm_runs":warm_runs}),
    );
    let mut plan = match backend {
        "ggml" => load_ggml(resource, model_path)?,
        "libtorch" => load_libtorch(resource, model_path)?,
        _ => return Err(format!("unknown explicit benchmark backend: {backend}")),
    };
    report["runtime_load_seconds"] = json!(plan.runtime_seconds);
    report["model_load_seconds"] = json!(plan.model_seconds);
    report["device"] = json!(plan.device);
    report["precision"] = json!(plan.precision);
    emit(
        &json!({"event":"loaded", "runtime_load_seconds":plan.runtime_seconds, "model_load_seconds":plan.model_seconds,
        "device":plan.device, "precision":plan.precision}),
    );
    let mut warm = Vec::new();
    let mut details = Vec::new();
    for iteration in 0..=warm_runs {
        emit(&json!({"event":"inference_begin", "iteration":iteration}));
        let started = Instant::now();
        let outcome = (plan.runner)(input);
        let seconds = started.elapsed().as_secs_f64();
        let detail = match outcome {
            Ok(detail) => detail,
            Err(error) => {
                report["failed_iteration"] = json!(iteration);
                report["failed_elapsed_seconds"] = json!(seconds);
                return Err(error);
            }
        };
        if iteration == 0 {
            report["cold_execution_seconds"] = json!(seconds);
        } else {
            warm.push(seconds);
        }
        emit(
            &json!({"event":"inference_complete", "iteration":iteration, "seconds":seconds, "detail":detail}),
        );
        details.push(detail);
    }
    report["warm_execution_seconds"] = json!(warm);
    report["warm_median_seconds"] = json!(median(&warm));
    report["details"] = json!(details);
    // Explicitly drop only after every measured inference has completed.
    drop(plan);
    Ok(())
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    Some(if sorted.len() % 2 == 0 {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    })
}

fn scope(resource: &str) -> &'static str {
    match resource {
        "qwen3_asr_1_7b" | "qwen3_forced_aligner_0_6b" => {
            "audio_frontend_and_encoder_only_no_decoder"
        }
        "firered_asr2_aed" => {
            "audio_frontend_and_nonoverlapping_encoder_windows_tail_padded_no_decoder"
        }
        "jbm555_cectc_80" => "audio_pipeline_same_mix_and_vocal_input",
        "stars" => "audio_pipeline_all_stages_with_synthetic_pitch_transcript_and_phonemes",
        "rosvot" => "audio_pipeline_with_synthetic_pitch_and_transcript",
        _ => "complete_audio_model_pipeline",
    }
}

fn load_ggml(resource: &str, model_path: &Path) -> Result<Plan, String> {
    let started = Instant::now();
    let directory = std::env::var("UTA_BENCH_GGML_LIB")
        .map_err(|_| "UTA_BENCH_GGML_LIB is required".to_string())?;
    let runtime = ggml::GgmlRuntime::load(Path::new(&directory))?;
    let device = runtime
        .devices()?
        .into_iter()
        .find(|device| {
            device.kind == ggml::DeviceKind::IntegratedGpu
                && device.description.contains("AMD Radeon 780M")
        })
        .ok_or_else(|| "AMD Radeon 780M GGML device is unavailable; no fallback".to_string())?;
    let runtime_seconds = started.elapsed().as_secs_f64();
    let name = format!("{}: {}", device.name, device.description);
    emit(
        &json!({"event":"runtime_loaded", "backend":"ggml", "device":name, "seconds":runtime_seconds}),
    );
    let started = Instant::now();
    let runner: Runner = match resource {
        "bs_roformer_leap_xe90_vocals"
        | "bs_roformer_leap_xe90_instrumental"
        | "bs_polarformer_public_instrumental"
        | "melband_roformer_harmony"
        | "melband_roformer_denoise_aufr33"
        | "melband_roformer_dereverb_anvuew" => {
            let mut model = ggml::roformer::Roformer::load(runtime, &device, model_path)?;
            Box::new(move |input| {
                let output = temporary_wav();
                let mut chunks = 0;
                let result = model.process_wav(input, &output, &mut |completed, _| {
                    chunks = completed;
                });
                let _ = std::fs::remove_file(&output);
                result?;
                Ok(format!("complete overlap-add, {chunks} chunks"))
            })
        }
        "rmvpe" => {
            let model = ggml::rmvpe::Rmvpe::load(runtime, &device, model_path)?;
            Box::new(move |input| {
                Ok(format!(
                    "{} pitch frames",
                    model.process_wav(input, |_, _| {})?.len()
                ))
            })
        }
        "fcpe" => {
            let model = ggml::fcpe::Fcpe::load(runtime, &device, model_path)?;
            Box::new(move |input| {
                Ok(format!(
                    "{} pitch frames",
                    model.process_wav(input, |_, _| {})?.len()
                ))
            })
        }
        "basic_pitch" => {
            let model = ggml::basic_pitch::BasicPitch::load(runtime, &device, model_path)?;
            Box::new(move |input| {
                Ok(format!(
                    "{} activation frames",
                    model.process_wav(input, |_, _| {})?.len()
                ))
            })
        }
        "game_1_0_3_small" | "game_1_0_3_medium" | "game_1_0_3_large" => {
            let model = ggml::game::Game::load(runtime, &device, model_path)?;
            Box::new(move |input| {
                let output = model.process_wav(input, &ggml::game::GameInferParams::default())?;
                Ok(format!(
                    "{} frames, {} notes, eight diffusion steps",
                    output.num_frames,
                    output.notes.len()
                ))
            })
        }
        "jbm555_cectc_80" => {
            let model = ggml::jbm555::Jbm555::load(runtime, &device, model_path)?;
            Box::new(move |input| {
                let (notes, samples) = model.process_wavs(input, input, |_, _| {})?;
                Ok(format!("{samples} samples, {} notes", notes.len()))
            })
        }
        "qwen3_asr_1_7b" | "qwen3_forced_aligner_0_6b" => {
            let model = ggml::qwen::Qwen::load(runtime, &device, model_path)?;
            Box::new(move |input| {
                let samples = read_mono_f32(input, 16_000)?;
                let mel = ggml::qwen::frontend::Frontend::whisper(128).compute(&samples);
                let audio = model.encode_audio(&mel)?;
                check_finite(&audio.values)?;
                Ok(format!("{} encoder rows x {}", audio.rows, audio.width))
            })
        }
        "firered_asr2_aed" => {
            let model = ggml::firered::FireRed::load(runtime, &device, model_path)?;
            let cmvn = std::fs::read(firered_cmvn()?).map_err(|error| error.to_string())?;
            Box::new(move |input| {
                let samples = read_mono_f32(input, 16_000)?;
                let windows = fire_windows(&samples);
                for window in &windows {
                    let (features, _) = ggml::firered::extract_features(window, &cmvn)?;
                    check_finite(&model.encode(&features)?.values)?;
                }
                Ok(format!(
                    "{} source samples, {} encoder windows including padded tail",
                    samples.len(),
                    windows.len()
                ))
            })
        }
        "stars" => {
            let model = ggml::stars::Stars::load(runtime, &device, model_path)?;
            Box::new(move |input| {
                let shared = ggml::stars::prepare_wav_inputs(input, &synthetic_pitch())?;
                let words = (0..15)
                    .map(|index| ggml::stars::TranscriptWord {
                        id: format!("word-{index}"),
                        text: "你".into(),
                        start_micros: index * 2_000_000,
                        duration_micros: 2_000_000,
                    })
                    .collect::<Vec<_>>();
                let output = model.infer_transcript(
                    &shared,
                    &words,
                    0,
                    true,
                    |words| {
                        Ok(ggml::stars::PhonemeInput {
                            phone_ids: vec![2; words.len()],
                            phone_to_word: (0..words.len()).map(|index| index as i64).collect(),
                        })
                    },
                    |_, _| {},
                )?;
                check_finite(&output.note_boundary_logits)?;
                Ok(format!(
                    "{} valid frames, {} notes, {} techniques",
                    output.valid_frames,
                    output.notes.len(),
                    output.techniques.map_or(0, |items| items.len())
                ))
            })
        }
        "rosvot" => {
            let model = ggml::rosvot::Rosvot::load(runtime, &device, model_path)?;
            Box::new(move |input| {
                let shared = ggml::rosvot::prepare_wav_inputs(input, &synthetic_pitch())?;
                let words = (0..15)
                    .map(|index| ggml::rosvot::TranscriptWord {
                        id: format!("word-{index}"),
                        text: "你".into(),
                        start_micros: index * 2_000_000,
                        duration_micros: 2_000_000,
                    })
                    .collect::<Vec<_>>();
                let output = model.infer_transcript(&shared, &words, 0, |_, _| {})?;
                check_finite(&output.note_boundary_logits)?;
                Ok(format!(
                    "{} valid frames, {} notes",
                    output.valid_frames,
                    output.notes.len()
                ))
            })
        }
        _ => return Err(format!("unsupported benchmark resource: {resource}")),
    };
    Ok(Plan {
        runner,
        runtime_seconds,
        model_seconds: started.elapsed().as_secs_f64(),
        device: name,
        precision: "checkpoint_storage_with_current_ggml_vulkan_kernels".into(),
    })
}

fn load_libtorch(resource: &str, model_path: &Path) -> Result<Plan, String> {
    let started = Instant::now();
    let path = std::env::var("UTA_BENCH_LIBTORCH_LIB")
        .map_err(|_| "UTA_BENCH_LIBTORCH_LIB is required".to_string())?;
    let library = torch::Library::load(Path::new(&path))?;
    let precision = torch::Precision::parse(
        &std::env::var("UTA_BENCH_LIBTORCH_PRECISION").unwrap_or_else(|_| "strict".into()),
    )?;
    let runtime_seconds = started.elapsed().as_secs_f64();
    emit(
        &json!({"event":"runtime_loaded", "backend":"libtorch", "build":library.build_info(), "seconds":runtime_seconds}),
    );
    let started = Instant::now();
    // This is the sole model-open call. The runner retains this owner's weights.
    let model = library.open(
        resource,
        model_path,
        torch::Backend::LibtorchRocm,
        0,
        precision,
    )?;
    let runner: Runner = match resource {
        "bs_roformer_leap_xe90_vocals"
        | "bs_roformer_leap_xe90_instrumental"
        | "bs_polarformer_public_instrumental"
        | "melband_roformer_harmony"
        | "melband_roformer_denoise_aufr33"
        | "melband_roformer_dereverb_anvuew" => {
            let mut route = torch::roformer::Roformer::from_model(model)?;
            Box::new(move |input| {
                let output = temporary_wav();
                let mut chunks = 0;
                let result = route.process_wav(input, &output, &mut |completed, _| {
                    chunks = completed;
                });
                let _ = std::fs::remove_file(&output);
                result?;
                Ok(format!("complete overlap-add, {chunks} chunks"))
            })
        }
        "rmvpe" => {
            let route = torch::rmvpe::Rmvpe::from_model(model);
            Box::new(move |input| {
                Ok(format!(
                    "{} pitch frames",
                    route.process_wav(input, |_, _| {})?.len()
                ))
            })
        }
        "fcpe" => {
            let route = torch::fcpe::Fcpe::from_model(model)?;
            Box::new(move |input| {
                Ok(format!(
                    "{} pitch frames",
                    route.process_wav(input, |_, _| {})?.len()
                ))
            })
        }
        "basic_pitch" => {
            let route = torch::basic_pitch::BasicPitch::from_model(model);
            Box::new(move |input| {
                Ok(format!(
                    "{} activation frames",
                    route.process_wav(input, |_, _| {})?.len()
                ))
            })
        }
        "game_1_0_3_small" | "game_1_0_3_medium" | "game_1_0_3_large" => {
            let route = torch::game::Game::from_model(model)?;
            Box::new(move |input| {
                let output = route.process_wav(input, &torch::game::GameInferParams::default())?;
                Ok(format!(
                    "{} frames, {} notes, eight diffusion steps",
                    output.num_frames,
                    output.notes.len()
                ))
            })
        }
        "jbm555_cectc_80" => {
            let route = torch::jbm555::Jbm555::from_model(model);
            Box::new(move |input| {
                let (notes, samples) = route.process_wavs(input, input, |_, _| {})?;
                Ok(format!("{samples} samples, {} notes", notes.len()))
            })
        }
        "qwen3_asr_1_7b" | "qwen3_forced_aligner_0_6b" => Box::new(move |input| {
            let samples = read_mono_f32(input, 16_000)?;
            let mel = ggml::qwen::frontend::Frontend::whisper(128).compute(&samples);
            let output = model.forward(
                "encode_outputs",
                &[torch::Input::f32(
                    "mel",
                    &[mel.bins as i64, mel.frames as i64],
                    &mel.data,
                )],
            )?;
            let audio = output.get("audio")?;
            check_finite(audio.f32()?)?;
            Ok(format!(
                "{} encoder rows x {}",
                audio.shape[0], audio.shape[1]
            ))
        }),
        "firered_asr2_aed" => {
            let cmvn = std::fs::read(firered_cmvn()?).map_err(|error| error.to_string())?;
            Box::new(move |input| {
                let samples = read_mono_f32(input, 16_000)?;
                let windows = fire_windows(&samples);
                for window in &windows {
                    let (features, frames) = ggml::firered::extract_features(window, &cmvn)?;
                    let output = model.forward(
                        "encode_outputs",
                        &[torch::Input::f32(
                            "features",
                            &[frames as i64, ggml::firered::MEL_BINS as i64],
                            &features,
                        )],
                    )?;
                    check_finite(output.get("audio")?.f32()?)?;
                }
                Ok(format!(
                    "{} source samples, {} encoder windows including padded tail",
                    samples.len(),
                    windows.len()
                ))
            })
        }
        "stars" => {
            let route = torch::stars::Stars::from_model(model);
            Box::new(move |input| {
                let shared = torch::stars::prepare_wav_inputs(input, &synthetic_pitch())?;
                let words = (0..15)
                    .map(|index| torch::stars::TranscriptWord {
                        id: format!("word-{index}"),
                        text: "你".into(),
                        start_micros: index * 2_000_000,
                        duration_micros: 2_000_000,
                    })
                    .collect::<Vec<_>>();
                let output = route.infer_transcript(
                    &shared,
                    &words,
                    0,
                    true,
                    |words| {
                        Ok(torch::stars::PhonemeInput {
                            phone_ids: vec![2; words.len()],
                            phone_to_word: (0..words.len()).map(|index| index as i64).collect(),
                        })
                    },
                    |_, _| {},
                )?;
                check_finite(&output.note_boundary_logits)?;
                Ok(format!(
                    "{} valid frames, {} notes, {} techniques",
                    output.valid_frames,
                    output.notes.len(),
                    output.techniques.map_or(0, |items| items.len())
                ))
            })
        }
        "rosvot" => {
            let route = torch::rosvot::Rosvot::from_model(model);
            Box::new(move |input| {
                let shared = torch::rosvot::prepare_wav_inputs(input, &synthetic_pitch())?;
                let words = (0..15)
                    .map(|index| torch::rosvot::TranscriptWord {
                        id: format!("word-{index}"),
                        text: "你".into(),
                        start_micros: index * 2_000_000,
                        duration_micros: 2_000_000,
                    })
                    .collect::<Vec<_>>();
                let output = route.infer_transcript(&shared, &words, 0, |_, _| {})?;
                check_finite(&output.note_boundary_logits)?;
                Ok(format!(
                    "{} valid frames, {} notes",
                    output.valid_frames,
                    output.notes.len()
                ))
            })
        }
        _ => return Err(format!("unsupported benchmark resource: {resource}")),
    };
    Ok(Plan {
        runner,
        runtime_seconds,
        model_seconds: started.elapsed().as_secs_f64(),
        device: "ROCm device 0".into(),
        precision: precision.name().into(),
    })
}

fn synthetic_pitch() -> Vec<f32> {
    vec![220.0; 3_100]
}
fn fire_windows(samples: &[f32]) -> Vec<Vec<f32>> {
    samples
        .chunks(ggml::firered::MAX_WINDOW_SAMPLES)
        .map(|chunk| {
            let mut window = chunk.to_vec();
            if window.len() < ggml::firered::MIN_WINDOW_SAMPLES {
                window.resize(ggml::firered::MIN_WINDOW_SAMPLES, 0.0);
            }
            window
        })
        .collect()
}
fn firered_cmvn() -> Result<PathBuf, String> {
    std::env::var_os("UTA_STUDIO_BENCH_FIRERED_CMVN")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "UTA_STUDIO_BENCH_FIRERED_CMVN must name the installed CMVN artifact".to_string()
        })
}
fn temporary_wav() -> PathBuf {
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "uta-studio-amd-bench-{}-{}.wav",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}
fn check_finite(values: &[f32]) -> Result<(), String> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        Err("empty or nonfinite model output".into())
    } else {
        Ok(())
    }
}
fn read_mono_f32(path: &Path, expected_rate: u32) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|error| error.to_string())?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != expected_rate
        || spec.sample_format != hound::SampleFormat::Float
        || spec.bits_per_sample != 32
    {
        return Err(format!(
            "benchmark WAV must be mono float32 {expected_rate} Hz"
        ));
    }
    let samples = reader
        .samples::<f32>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    check_finite(&samples)?;
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn statistics_do_not_fabricate_warm_measurements() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 2.0]), Some(3.0));
    }
    #[test]
    fn firered_tail_retains_every_source_sample() {
        let samples = vec![1.0; 480_000];
        let windows = fire_windows(&samples);
        assert_eq!(windows.len(), 13);
        assert_eq!(
            windows
                .iter()
                .flatten()
                .filter(|value| **value == 1.0)
                .count(),
            samples.len()
        );
        assert!(
            windows
                .iter()
                .all(|window| window.len() >= ggml::firered::MIN_WINDOW_SAMPLES
                    && window.len() <= ggml::firered::MAX_WINDOW_SAMPLES)
        );
        assert_eq!(windows.last().unwrap().last(), Some(&0.0));
    }
    #[test]
    fn empty_firered_source_is_not_an_encoder_window() {
        assert!(fire_windows(&[]).is_empty());
    }
    #[test]
    fn speech_and_synthetic_conditioning_are_explicit() {
        assert!(scope("qwen3_asr_1_7b").contains("no_decoder"));
        assert!(scope("firered_asr2_aed").contains("no_decoder"));
        assert!(scope("stars").contains("synthetic"));
    }
    #[test]
    fn invalid_output_is_not_a_successful_measurement() {
        assert!(check_finite(&[f32::NAN]).is_err());
        assert!(check_finite(&[]).is_err());
        assert!(check_finite(&[0.0, 1.0]).is_ok());
        assert!(result_is_failure(&json!({"status":"failed"})));
    }
}
