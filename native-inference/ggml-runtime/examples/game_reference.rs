//! Explicit CPU real-audio diagnostic; never a production fallback.
//! Usage: game_reference RUNTIME_DIR MODEL_GGUF FLOAT_WAV OUTPUT_DIR LANGUAGE_ID
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use uta_ggml_runtime::game::{
    Game, GameInferParams, MelConfig, MelExtractor, decode_soft_boundaries,
};
use uta_ggml_runtime::{DeviceKind, GgmlRuntime};

fn write_floats(path: &Path, values: &[f32]) -> Result<(), Box<dyn Error>> {
    let mut output = OpenOptions::new().create_new(true).write(true).open(path)?;
    for value in values {
        output.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 5 {
        return Err("expected RUNTIME_DIR MODEL_GGUF FLOAT_WAV OUTPUT_DIR LANGUAGE_ID".into());
    }
    let output = Path::new(&arguments[3]);
    fs::create_dir(output)?;
    let mut reader = hound::WavReader::open(&arguments[2])?;
    let spec = reader.spec();
    if spec.sample_rate != 44_100
        || spec.channels != 1
        || spec.sample_format != hound::SampleFormat::Float
    {
        return Err("diagnostic input must be mono float WAV at 44100 Hz".into());
    }
    let audio = reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?;
    let frontend = MelExtractor::new(MelConfig::default())?;
    let frames = frontend.frame_count(audio.len());
    let mel = frontend.extract(&audio)?;
    let runtime = GgmlRuntime::load(Path::new(&arguments[0]))?;
    let device = runtime
        .devices()?
        .into_iter()
        .find(|device| device.kind == DeviceKind::Cpu)
        .ok_or("explicit CPU runtime unavailable")?;
    let model = Game::load(runtime, &device, Path::new(&arguments[1]))?;
    let parameters = GameInferParams {
        language: arguments[4].parse()?,
        ..Default::default()
    };
    let encoded = model.encode_mel(&mel, frames)?;
    write_floats(&output.join("mel.f32"), &mel)?;
    write_floats(
        &output.join("segmenter-embeddings.f32"),
        &encoded.segmenter_embeddings,
    )?;
    write_floats(
        &output.join("estimator-embeddings.f32"),
        &encoded.estimator_embeddings,
    )?;
    let logits = model.segmenter_logits(
        &encoded.segmenter_embeddings,
        &vec![0; frames],
        0.0,
        parameters.language,
    )?;
    write_floats(&output.join("initial-segmenter-logits.f32"), &logits)?;
    let probabilities = logits
        .iter()
        .map(|value| 1.0 / (1.0 + (-value).exp()))
        .collect::<Vec<_>>();
    let initial = decode_soft_boundaries(
        &probabilities,
        Some(&vec![0; frames]),
        Some(&vec![1; frames]),
        parameters.boundary_threshold,
        parameters.boundary_radius,
    )?;
    let prediction = model.infer_mel(&mel, frames, &parameters)?;
    let value = serde_json::json!({
        "scope": "Explicit CPU native diagnostic with source audio only; no reference notes or lyrics supplied",
        "runtime_directory": arguments[0], "model": arguments[1], "audio": arguments[2],
        "device": device.description, "frames": frames, "samples": audio.len(), "embedding_dimensions": 256,
        "language": parameters.language, "sampling_steps": parameters.d3pm_steps,
        "boundary_threshold": parameters.boundary_threshold, "boundary_radius": parameters.boundary_radius,
        "note_threshold": parameters.note_threshold, "initial_boundaries": initial,
        "final_boundaries": prediction.boundaries,
        "notes": prediction.notes.iter().map(|note| serde_json::json!({
            "start": note.offset_micros, "end": note.offset_micros + note.duration_micros,
            "midi": note.pitch_midi, "voiced": note.voiced
        })).collect::<Vec<_>>()
    });
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output.join("native.json"))?;
    serde_json::to_writer_pretty(&mut file, &value)?;
    file.write_all(b"\n")?;
    println!("{}", value);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("GAME CPU diagnostic failed: {error}");
        std::process::exit(1);
    }
}
