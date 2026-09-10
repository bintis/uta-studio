use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use uta_ggml_runtime::{DeviceDescriptor, DeviceKind, GgmlRuntime};
use uta_libtorch_runtime::{Backend, Library, Precision};

#[derive(Serialize)]
struct Report {
    backend: String,
    resource: String,
    scope: String,
    load_seconds: f64,
    execution_seconds: f64,
    status: String,
    detail: String,
}

fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    if arguments.len() != 5 {
        eprintln!("usage: amd_backend_bench <ggml|libtorch> <resource> <model.gguf> <canonical.wav>");
        std::process::exit(2);
    }
    let result = run(&arguments[1], &arguments[2], Path::new(&arguments[3]), Path::new(&arguments[4]));
    match result {
        Ok(report) => println!("{}", serde_json::to_string(&report).expect("serialize benchmark report")),
        Err(report) => {
            println!("{}", serde_json::to_string(&report).expect("serialize benchmark error"));
            std::process::exit(1);
        }
    }
}

fn run(backend: &str, resource: &str, model_path: &Path, input_path: &Path) -> Result<Report, Report> {
    let started = Instant::now();
    let loaded = match backend {
        "ggml" => load_ggml(resource, model_path).map(Loaded::Ggml),
        "libtorch" => load_libtorch(resource, model_path).map(Loaded::Libtorch),
        other => Err(format!("unknown benchmark backend: {other}")),
    };
    let loaded = match loaded {
        Ok(value) => value,
        Err(error) => return Err(failure(backend, resource, scope(resource), started.elapsed().as_secs_f64(), 0.0, error)),
    };
    let load_seconds = started.elapsed().as_secs_f64();
    let execution_started = Instant::now();
    let result = match loaded {
        Loaded::Ggml(value) => run_ggml(value, resource, input_path),
        Loaded::Libtorch(value) => run_libtorch(value, resource, input_path),
    };
    let execution_seconds = execution_started.elapsed().as_secs_f64();
    match result {
        Ok(detail) => Ok(Report {
            backend: backend.to_string(), resource: resource.to_string(), scope: scope(resource).to_string(),
            load_seconds, execution_seconds, status: "passed".to_string(), detail,
        }),
        Err(error) => Err(failure(backend, resource, scope(resource), load_seconds, execution_seconds, error)),
    }
}

fn failure(backend: &str, resource: &str, scope: &str, load_seconds: f64, execution_seconds: f64, detail: String) -> Report {
    Report {
        backend: backend.to_string(), resource: resource.to_string(), scope: scope.to_string(),
        load_seconds, execution_seconds, status: "failed".to_string(), detail,
    }
}

fn scope(resource: &str) -> &'static str {
    match resource {
        "qwen3_asr_1_7b" | "qwen3_forced_aligner_0_6b" => "canonical_30s_frontend_plus_audio_encoder",
        "firered_asr2_aed" => "canonical_30s_frontend_plus_all_encoder_windows",
        "jbm555_cectc_80" => "canonical_30s_frontend_plus_network_same_mix_and_vocal",
        "stars" => "canonical_30s_frontend_plus_transcript_conditioned_all_learned_stages",
        "rosvot" => "canonical_30s_frontend_plus_transcript_conditioned_frame_and_pitch_stages",
        _ => "canonical_30s_full_model_pipeline",
    }
}

enum Loaded {
    Ggml(GgmlLoaded),
    Libtorch(LibtorchLoaded),
}

struct GgmlLoaded {
    runtime: std::sync::Arc<GgmlRuntime>,
    device: DeviceDescriptor,
    model_path: PathBuf,
}

struct LibtorchLoaded {
    library: Library,
    model_path: PathBuf,
}

fn load_ggml(_resource: &str, model_path: &Path) -> Result<GgmlLoaded, String> {
    let library_dir = std::env::var("UTA_BENCH_GGML_LIB").map_err(|_| "UTA_BENCH_GGML_LIB is required".to_string())?;
    let runtime = GgmlRuntime::load(Path::new(&library_dir))?;
    let device = runtime.devices()?.into_iter().find(|device| {
        device.kind == DeviceKind::IntegratedGpu && device.description.contains("AMD Radeon 780M")
    }).ok_or_else(|| "AMD Radeon 780M integrated GGML device is unavailable".to_string())?;
    Ok(GgmlLoaded { runtime, device, model_path: model_path.to_path_buf() })
}

fn load_libtorch(resource: &str, model_path: &Path) -> Result<LibtorchLoaded, String> {
    let library_path = std::env::var("UTA_BENCH_LIBTORCH_LIB").map_err(|_| "UTA_BENCH_LIBTORCH_LIB is required".to_string())?;
    let library = Library::load(Path::new(&library_path))?;
    let precision = match std::env::var("UTA_BENCH_LIBTORCH_PRECISION").as_deref() {
        Ok("mixed_attention") => Precision::MixedAttention,
        Ok("strict") | Err(_) => Precision::Strict,
        Ok(other) => return Err(format!("unsupported UTA_BENCH_LIBTORCH_PRECISION: {other}")),
    };
    let _ = library.open(resource, model_path, Backend::LibtorchRocm, 0, precision)?;
    Ok(LibtorchLoaded { library, model_path: model_path.to_path_buf() })
}

fn lib_model(loaded: &LibtorchLoaded, resource: &str) -> Result<uta_libtorch_runtime::Model, String> {
    let precision = if matches!(resource,
        "bs_roformer_leap_xe90_vocals" | "bs_roformer_leap_xe90_instrumental" |
        "bs_polarformer_public_instrumental" | "melband_roformer_harmony" |
        "melband_roformer_denoise_aufr33" | "melband_roformer_dereverb_anvuew" |
        "game_1_0_3_small" | "game_1_0_3_medium" | "game_1_0_3_large") {
        Precision::MixedAttention
    } else { Precision::Strict };
    loaded.library.open(resource, &loaded.model_path, Backend::LibtorchRocm, 0, precision)
}

fn run_ggml(loaded: GgmlLoaded, resource: &str, input: &Path) -> Result<String, String> {
    match resource {
        "bs_roformer_leap_xe90_vocals" | "bs_roformer_leap_xe90_instrumental" |
        "bs_polarformer_public_instrumental" | "melband_roformer_harmony" |
        "melband_roformer_denoise_aufr33" | "melband_roformer_dereverb_anvuew" => {
            let mut model = uta_ggml_runtime::roformer::Roformer::load(loaded.runtime, &loaded.device, &loaded.model_path)?;
            let output = temporary_wav(resource, "ggml");
            model.process_wav(input, &output, &mut |_, _| {})?;
            let _ = std::fs::remove_file(output);
            Ok("complete overlap-add route".to_string())
        }
        "rmvpe" => {
            let model = uta_ggml_runtime::rmvpe::Rmvpe::load(loaded.runtime, &loaded.device, &loaded.model_path)?;
            let frames = model.process_wav(input, |_, _| {})?;
            Ok(format!("{} pitch frames", frames.len()))
        }
        "fcpe" => {
            let model = uta_ggml_runtime::fcpe::Fcpe::load(loaded.runtime, &loaded.device, &loaded.model_path)?;
            let frames = model.process_wav(input, |_, _| {})?;
            Ok(format!("{} pitch frames", frames.len()))
        }
        "basic_pitch" => {
            let model = uta_ggml_runtime::basic_pitch::BasicPitch::load(loaded.runtime, &loaded.device, &loaded.model_path)?;
            let frames = model.process_wav(input, |_, _| {})?;
            Ok(format!("{} activation frames", frames.len()))
        }
        "game_1_0_3_small" | "game_1_0_3_medium" | "game_1_0_3_large" => {
            let model = uta_ggml_runtime::game::Game::load(loaded.runtime, &loaded.device, &loaded.model_path)?;
            let output = model.process_wav(input, &uta_ggml_runtime::game::GameInferParams::default())?;
            Ok(format!("{} frames, {} notes", output.num_frames, output.notes.len()))
        }
        "jbm555_cectc_80" => {
            let model = uta_ggml_runtime::jbm555::Jbm555::load(loaded.runtime, &loaded.device, &loaded.model_path)?;
            let (notes, samples) = model.process_wavs(input, input, |_, _| {})?;
            Ok(format!("{samples} samples, {} notes", notes.len()))
        }
        "qwen3_asr_1_7b" | "qwen3_forced_aligner_0_6b" => {
            let samples = read_mono_f32(input, 16_000)?;
            let qwen = uta_ggml_runtime::qwen::Qwen::load(loaded.runtime, &loaded.device, &loaded.model_path)?;
            let mel = uta_ggml_runtime::qwen::frontend::Frontend::whisper(128).compute(&samples);
            let audio = qwen.encode_audio(&mel)?;
            Ok(format!("{} encoder rows x {}", audio.rows, audio.width))
        }
        "firered_asr2_aed" => run_ggml_firered(loaded, input),
        "stars" => run_ggml_stars(loaded, input),
        "rosvot" => run_ggml_rosvot(loaded, input),
        _ => Err(format!("unsupported benchmark resource: {resource}")),
    }
}

fn run_libtorch(loaded: LibtorchLoaded, resource: &str, input: &Path) -> Result<String, String> {
    let model = lib_model(&loaded, resource)?;
    match resource {
        "bs_roformer_leap_xe90_vocals" | "bs_roformer_leap_xe90_instrumental" |
        "bs_polarformer_public_instrumental" | "melband_roformer_harmony" |
        "melband_roformer_denoise_aufr33" | "melband_roformer_dereverb_anvuew" => {
            let mut route = uta_libtorch_runtime::roformer::Roformer::from_model(model)?;
            let output = temporary_wav(resource, "libtorch");
            route.process_wav(input, &output, &mut |_, _| {})?;
            let _ = std::fs::remove_file(output);
            Ok("complete overlap-add route".to_string())
        }
        "rmvpe" => {
            let route = uta_libtorch_runtime::rmvpe::Rmvpe::from_model(model);
            let frames = route.process_wav(input, |_, _| {})?;
            Ok(format!("{} pitch frames", frames.len()))
        }
        "fcpe" => {
            let route = uta_libtorch_runtime::fcpe::Fcpe::from_model(model)?;
            let frames = route.process_wav(input, |_, _| {})?;
            Ok(format!("{} pitch frames", frames.len()))
        }
        "basic_pitch" => {
            let route = uta_libtorch_runtime::basic_pitch::BasicPitch::from_model(model);
            let frames = route.process_wav(input, |_, _| {})?;
            Ok(format!("{} activation frames", frames.len()))
        }
        "game_1_0_3_small" | "game_1_0_3_medium" | "game_1_0_3_large" => {
            let route = uta_libtorch_runtime::game::Game::from_model(model)?;
            let output = route.process_wav(input, &uta_libtorch_runtime::game::GameInferParams::default())?;
            Ok(format!("{} frames, {} notes", output.num_frames, output.notes.len()))
        }
        "jbm555_cectc_80" => {
            let route = uta_libtorch_runtime::jbm555::Jbm555::from_model(model);
            let (notes, samples) = route.process_wavs(input, input, |_, _| {})?;
            Ok(format!("{samples} samples, {} notes", notes.len()))
        }
        "qwen3_asr_1_7b" | "qwen3_forced_aligner_0_6b" => run_libtorch_qwen(model, input),
        "firered_asr2_aed" => run_libtorch_firered(model, input),
        "stars" => run_libtorch_stars(model, input),
        "rosvot" => run_libtorch_rosvot(model, input),
        _ => Err(format!("unsupported benchmark resource: {resource}")),
    }
}

fn run_libtorch_qwen(model: uta_libtorch_runtime::Model, input: &Path) -> Result<String, String> {
    let samples = read_mono_f32(input, 16_000)?;
    let mel = uta_ggml_runtime::qwen::frontend::Frontend::whisper(128).compute(&samples);
    let shape = [mel.bins as i64, mel.frames as i64];
    let output = model.forward("encode_outputs", &[uta_libtorch_runtime::Input::f32("mel", &shape, &mel.data)])?;
    let audio = output.get("audio")?;
    Ok(format!("{} encoder rows x {}", audio.shape[0], audio.shape[1]))
}

fn run_ggml_firered(loaded: GgmlLoaded, input: &Path) -> Result<String, String> {
    let samples = read_mono_f32(input, 16_000)?;
    let cmvn = std::fs::read(firered_cmvn()).map_err(|error| format!("read FireRed CMVN: {error}"))?;
    let model = uta_ggml_runtime::firered::FireRed::load(loaded.runtime, &loaded.device, &loaded.model_path)?;
    let mut windows = 0usize;
    for chunk in samples.chunks(uta_ggml_runtime::firered::MAX_WINDOW_SAMPLES) {
        if chunk.len() < uta_ggml_runtime::firered::MIN_WINDOW_SAMPLES { break; }
        let (features, _) = uta_ggml_runtime::firered::extract_features(chunk, &cmvn)?;
        let _ = model.encode(&features)?;
        windows += 1;
    }
    Ok(format!("{windows} encoder windows"))
}

fn run_libtorch_firered(model: uta_libtorch_runtime::Model, input: &Path) -> Result<String, String> {
    let samples = read_mono_f32(input, 16_000)?;
    let cmvn = std::fs::read(firered_cmvn()).map_err(|error| format!("read FireRed CMVN: {error}"))?;
    let mut windows = 0usize;
    for chunk in samples.chunks(uta_ggml_runtime::firered::MAX_WINDOW_SAMPLES) {
        if chunk.len() < uta_ggml_runtime::firered::MIN_WINDOW_SAMPLES { break; }
        let (features, _) = uta_ggml_runtime::firered::extract_features(chunk, &cmvn)?;
        let shape = [uta_ggml_runtime::firered::FEATURE_FRAMES as i64, uta_ggml_runtime::firered::MEL_BINS as i64];
        let _ = model.forward("encode_outputs", &[uta_libtorch_runtime::Input::f32("features", &shape, &features)])?;
        windows += 1;
    }
    Ok(format!("{windows} encoder windows"))
}

fn run_ggml_stars(loaded: GgmlLoaded, input: &Path) -> Result<String, String> {
    let shared = uta_ggml_runtime::stars::prepare_wav_inputs(input, &constant_f0())?;
    let words = stars_words_ggml();
    let model = uta_ggml_runtime::stars::Stars::load(loaded.runtime, &loaded.device, &loaded.model_path)?;
    let output = model.infer_transcript(&shared, &words, 0, true, simple_phonemes_ggml, |_, _| {})?;
    Ok(format!("{} valid frames, {} notes", output.valid_frames, output.notes.len()))
}

fn run_libtorch_stars(model: uta_libtorch_runtime::Model, input: &Path) -> Result<String, String> {
    let shared = uta_libtorch_runtime::stars::prepare_wav_inputs(input, &constant_f0())?;
    let words = stars_words_libtorch();
    let route = uta_libtorch_runtime::stars::Stars::from_model(model);
    let output = route.infer_transcript(&shared, &words, 0, true, simple_phonemes_libtorch, |_, _| {})?;
    Ok(format!("{} valid frames, {} notes", output.valid_frames, output.notes.len()))
}

fn run_ggml_rosvot(loaded: GgmlLoaded, input: &Path) -> Result<String, String> {
    let shared = uta_ggml_runtime::rosvot::prepare_wav_inputs(input, &constant_f0())?;
    let words = rosvot_words_ggml();
    let model = uta_ggml_runtime::rosvot::Rosvot::load(loaded.runtime, &loaded.device, &loaded.model_path)?;
    let output = model.infer_transcript(&shared, &words, 0, |_, _| {})?;
    Ok(format!("{} valid frames, {} notes", output.valid_frames, output.notes.len()))
}

fn run_libtorch_rosvot(model: uta_libtorch_runtime::Model, input: &Path) -> Result<String, String> {
    let shared = uta_libtorch_runtime::rosvot::prepare_wav_inputs(input, &constant_f0())?;
    let words = rosvot_words_libtorch();
    let route = uta_libtorch_runtime::rosvot::Rosvot::from_model(model);
    let output = route.infer_transcript(&shared, &words, 0, |_, _| {})?;
    Ok(format!("{} valid frames, {} notes", output.valid_frames, output.notes.len()))
}

fn constant_f0() -> Vec<f32> { vec![220.0; 3_100] }

fn stars_words_ggml() -> Vec<uta_ggml_runtime::stars::TranscriptWord> {
    (0..15).map(|index| uta_ggml_runtime::stars::TranscriptWord {
        id: format!("word-{index}"), text: "你".to_string(),
        start_micros: index * 2_000_000, duration_micros: 1_800_000,
    }).collect()
}

fn stars_words_libtorch() -> Vec<uta_libtorch_runtime::stars::TranscriptWord> {
    (0..15).map(|index| uta_libtorch_runtime::stars::TranscriptWord {
        id: format!("word-{index}"), text: "你".to_string(),
        start_micros: index * 2_000_000, duration_micros: 1_800_000,
    }).collect()
}

fn rosvot_words_ggml() -> Vec<uta_ggml_runtime::rosvot::TranscriptWord> {
    (0..15).map(|index| uta_ggml_runtime::rosvot::TranscriptWord {
        id: format!("word-{index}"), text: "你".to_string(),
        start_micros: index * 2_000_000, duration_micros: 1_800_000,
    }).collect()
}

fn rosvot_words_libtorch() -> Vec<uta_libtorch_runtime::rosvot::TranscriptWord> {
    (0..15).map(|index| uta_libtorch_runtime::rosvot::TranscriptWord {
        id: format!("word-{index}"), text: "你".to_string(),
        start_micros: index * 2_000_000, duration_micros: 1_800_000,
    }).collect()
}

fn simple_phonemes_ggml(words: &[String]) -> Result<uta_ggml_runtime::stars::PhonemeInput, String> {
    Ok(uta_ggml_runtime::stars::PhonemeInput {
        phone_ids: vec![1; words.len()],
        phone_to_word: (0..words.len()).map(|index| index as i64).collect(),
    })
}

fn simple_phonemes_libtorch(words: &[String]) -> Result<uta_libtorch_runtime::stars::PhonemeInput, String> {
    Ok(uta_libtorch_runtime::stars::PhonemeInput {
        phone_ids: vec![1; words.len()],
        phone_to_word: (0..words.len()).map(|index| index as i64).collect(),
    })
}

fn firered_cmvn() -> PathBuf {
    PathBuf::from("/home/bintis/.local/share/uta-studio/runtime/models/firered_asr2_aed/generations/eb644a5299c5fe3159b9772ff2fb720fe0f863b8e0164297432dedd8c95b5f82/cmvn.ark")
}

fn temporary_wav(resource: &str, backend: &str) -> PathBuf {
    std::env::temp_dir().join(format!("uta-amd-bench-{backend}-{resource}-{}.wav", std::process::id()))
}

fn read_mono_f32(path: &Path, expected_rate: u32) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|error| format!("open benchmark wav: {error}"))?;
    let specification = reader.spec();
    if specification.channels != 1 || specification.sample_rate != expected_rate || specification.sample_format != hound::SampleFormat::Float || specification.bits_per_sample != 32 {
        return Err(format!("benchmark wav must be mono f32 {expected_rate} Hz"));
    }
    reader.samples::<f32>().collect::<Result<Vec<_>, _>>().map_err(|error| format!("read benchmark wav: {error}"))
}
