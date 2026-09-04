pub mod audio;
pub mod engine;
pub mod error;
pub mod gguf;
pub mod layers;
pub mod mel16;
pub mod protocol;
pub mod rmvpe;
pub mod rosvot_host;
pub mod singing_frontend;

use std::io::BufRead;
use std::path::{Path, PathBuf};

use error::{Error, Result};
use protocol::{PROTOCOL_VERSION, WorkerCommand, WorkerFrame, emit};

pub const COMPONENT_NAME: &str = "uta-rosvot-worker";
pub const RUNTIME_RECIPE_DIGEST: &str = "rosvot-native-recipe-v1";

#[derive(serde::Deserialize)]
struct RosvotConfigWord {
    id: String,
    text: String,
    start: u64,
    duration: u64,
}

/// Flat top-level shape mirroring
/// `native-inference/openvino-worker/src/advanced_notes.rs::TaskConfig`
/// (minus its OpenVINO-only `device` field), matching
/// `native-inference/stars`'s own worker config shape.
#[derive(serde::Deserialize)]
struct RosvotTaskConfig {
    model_path: PathBuf,
    rmvpe_model_path: PathBuf,
    model_generation: String,
    source_start: u64,
    #[serde(default)]
    #[allow(dead_code)]
    source_duration: u64,
    timed_transcript_generation: String,
    words: Vec<RosvotConfigWord>,
}

pub fn run_task(
    task_id: &str,
    _model_id: &str,
    input_artifacts: &[PathBuf],
    output_dir: &Path,
    config: &serde_json::Value,
) -> Result<()> {
    if input_artifacts.is_empty() {
        return Err(Error::message(
            "ROSVOT requires at least one input audio artifact",
        ));
    }
    if !output_dir.is_dir() {
        return Err(Error::message("Output directory does not exist"));
    }
    let task_config: RosvotTaskConfig = serde_json::from_value(config.clone())
        .map_err(|e| Error::message(format!("ROSVOT task config is invalid: {e}")))?;
    if task_config.words.is_empty() {
        return Err(Error::message(
            "ROSVOT requires a non-empty timed-transcript word list",
        ));
    }
    let words = task_config
        .words
        .iter()
        .map(|word| engine::ConfigWord {
            id: word.id.clone(),
            text: word.text.clone(),
            start: word.start,
            duration: word.duration,
        })
        .collect::<Vec<_>>();

    emit(&WorkerFrame::Progress {
        task_id,
        fraction: 0.01,
        message: "Decoding audio (24 kHz mono)",
        work_units_completed: None,
        work_units_total: None,
    });
    let audio_24k = audio::decode_mono(
        &input_artifacts[0],
        output_dir,
        singing_frontend::SAMPLE_RATE,
    )
    .map_err(|e| Error::message(format!("could not decode audio at 24 kHz: {e}")))?;
    let audio_16k = audio::decode_mono(&input_artifacts[0], output_dir, mel16::SAMPLE_RATE)
        .map_err(|e| Error::message(format!("could not decode audio at 16 kHz: {e}")))?;

    if !task_config.model_path.is_file() {
        return Err(Error::message("ROSVOT GGUF model path is unavailable"));
    }
    if !task_config.rmvpe_model_path.is_file() {
        return Err(Error::message("RMVPE GGUF model path is unavailable"));
    }

    let output = engine::infer(
        &audio_24k,
        &audio_16k,
        &words,
        task_config.source_start,
        &task_config.timed_transcript_generation,
        &task_config.model_generation,
        &task_config.model_path,
        &task_config.rmvpe_model_path,
        output_dir,
        |fraction, message, work_units| {
            emit(&WorkerFrame::Progress {
                task_id,
                fraction: 0.05 + fraction * 0.92,
                message,
                work_units_completed: work_units.map(|(c, _)| c),
                work_units_total: work_units.map(|(_, t)| t),
            });
        },
    )
    .map_err(|e| Error::message(e.to_string()))?;

    emit(&WorkerFrame::Output {
        task_id,
        artifact: "advanced_note_evidence",
        path: &output,
        media_type: "application/json",
    });

    emit(&WorkerFrame::Done {
        task_id,
        status: "ok",
    });

    Ok(())
}

pub fn run_stdio() -> std::process::ExitCode {
    emit(&WorkerFrame::Ready {
        protocol: PROTOCOL_VERSION,
        component: COMPONENT_NAME,
        runtime_recipe_digest: RUNTIME_RECIPE_DIGEST,
    });

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("stdin read error: {e}");
                break;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let cmd: WorkerCommand = match serde_json::from_str(trimmed) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to parse worker command: {e}; input={trimmed}");
                continue;
            }
        };

        match cmd {
            WorkerCommand::Quit { .. } => {
                break;
            }
            WorkerCommand::Cancel { task_id, .. } => {
                emit(&WorkerFrame::Done {
                    task_id: &task_id,
                    status: "cancelled",
                });
            }
            WorkerCommand::Run {
                task_id,
                model_id,
                input_artifacts,
                output_dir,
                config,
                ..
            } => {
                if let Err(err) =
                    run_task(&task_id, &model_id, &input_artifacts, &output_dir, &config)
                {
                    emit(&WorkerFrame::Error {
                        task_id: Some(&task_id),
                        code: "rosvot_execution_error",
                        message: &err.to_string(),
                        retryable: false,
                    });
                }
            }
        }
    }
    std::process::ExitCode::SUCCESS
}
