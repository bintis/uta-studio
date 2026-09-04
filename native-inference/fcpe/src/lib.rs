pub mod audio;
pub mod engine;
pub mod error;
pub mod gguf;
pub mod mel;
pub mod protocol;

use std::io::BufRead;
use std::path::{Path, PathBuf};

use error::{Error, Result};
use protocol::{PROTOCOL_VERSION, WorkerCommand, WorkerFrame, emit};

pub const COMPONENT_NAME: &str = "uta-fcpe-worker";
pub const RUNTIME_RECIPE_DIGEST: &str = "fcpe-native-recipe-v1";

fn resolve_model_path(config: &serde_json::Value) -> Result<PathBuf> {
    if let Some(path_str) = config.get("model_path").and_then(serde_json::Value::as_str) {
        let path = PathBuf::from(path_str);
        if path.is_file() {
            return Ok(path);
        }
        if path.is_dir() {
            let candidate = path.join("fcpe-f32.gguf");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let candidate = home.join(".local/share/uta-studio/runtime/ggml-models/fcpe/fcpe-f32.gguf");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(Error::message(
        "FCPE GGUF model path not found in config or runtime store",
    ))
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
            "FCPE requires at least one input audio artifact",
        ));
    }
    if !output_dir.is_dir() {
        return Err(Error::message("Output directory does not exist"));
    }

    emit(&WorkerFrame::Progress {
        task_id,
        fraction: 0.02,
        message: "Decoding audio (16 kHz mono)",
        work_units_completed: None,
        work_units_total: None,
    });
    let audio = audio::decode_mono(&input_artifacts[0], output_dir, 16_000)
        .map_err(|e| Error::message(format!("could not decode audio: {e}")))?;

    let execution =
        uta_wgpu_runtime::ExecutionDevice::from_worker_config(config).map_err(Error::message)?;
    #[cfg(not(feature = "gpu"))]
    if matches!(execution, uta_wgpu_runtime::ExecutionDevice::Gpu(_)) {
        return Err(Error::message(
            "FCPE GPU execution requested but the worker was built without the `gpu` feature",
        ));
    }
    let model_path = resolve_model_path(config)?;

    let output = engine::infer(
        &audio,
        &model_path,
        output_dir,
        execution,
        |fraction, message, work_units| {
            emit(&WorkerFrame::Progress {
                task_id,
                fraction: 0.05 + fraction * 0.90,
                message,
                work_units_completed: work_units.map(|(c, _)| c),
                work_units_total: work_units.map(|(_, t)| t),
            });
        },
    )?;

    emit(&WorkerFrame::Output {
        task_id,
        artifact: "fcpe_pitch_evidence",
        path: &output,
        media_type: "application/vnd.uta.timed-pitch-evidence+json;version=3",
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
                        code: "fcpe_execution_error",
                        message: &err.to_string(),
                        retryable: false,
                    });
                }
            }
        }
    }
    std::process::ExitCode::SUCCESS
}
