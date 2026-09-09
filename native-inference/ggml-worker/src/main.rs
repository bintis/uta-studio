#![allow(clippy::all)]

mod audio;
mod engine;
mod firered;
mod game;
mod jbm555;
mod pitch_input;
mod protocol;
mod qwen;
mod qwen_asr;
mod rosvot;
mod runtime;
mod stars;
mod stars_g2p;

use std::io::BufRead;

use protocol::{WorkerCommand, WorkerFrame, emit};

fn run_task(
    task_id: &str,
    model_id: &str,
    input_artifacts: &[std::path::PathBuf],
    output_dir: &std::path::Path,
    config: &serde_json::Value,
) -> Result<(), String> {
    if !output_dir.is_dir() {
        return Err("authorized GGML task output directory is unavailable".to_string());
    }
    let source = input_artifacts
        .first()
        .ok_or_else(|| "GGML task has no input audio artifact".to_string())?;
    let secondary_source = if matches!(model_id, "jbm555_cectc_80" | "stars" | "rosvot") {
        if input_artifacts.len() != 2 {
            let expected = if model_id == "jbm555_cectc_80" {
                "JBM555 requires exactly two input artifacts: original mix and prepared vocal"
            } else {
                "STARS and ROSVOT require exactly two input artifacts: prepared vocal and shared RMVPE evidence"
            };
            return Err(expected.to_string());
        }
        input_artifacts.get(1).map(std::path::PathBuf::as_path)
    } else {
        None
    };
    let outputs = engine::run(
        task_id,
        model_id,
        source,
        secondary_source,
        output_dir,
        config,
        |fraction, message, work_units| {
            if let Some((completed, total)) = work_units {
                let _ = emit(WorkerFrame::Progress {
                    task_id,
                    fraction,
                    message,
                    work_units_completed: Some(completed),
                    work_units_total: Some(total),
                });
            }
        },
    )?;
    for output in outputs {
        emit(WorkerFrame::Output {
            task_id,
            artifact: output.artifact,
            path: &output.path,
            media_type: output.media_type,
        })?;
    }
    emit(WorkerFrame::Done {
        task_id,
        status: "ok",
    })
}

fn main() {
    if !std::env::args().any(|argument| argument == "--stdio-json") {
        eprintln!("uta-ggml-worker requires --stdio-json");
        std::process::exit(2);
    }
    if emit(WorkerFrame::Ready {
        component: "uta-ggml-worker",
    })
    .is_err()
    {
        std::process::exit(3);
    }
    for line in std::io::stdin().lock().lines() {
        let line = match line {
            Ok(line) if !line.trim().is_empty() => line,
            Ok(_) => continue,
            Err(error) => {
                eprintln!("GGML worker stdin failed: {error}");
                break;
            }
        };
        let command = match serde_json::from_str::<WorkerCommand>(&line) {
            Ok(command) => command,
            Err(error) => {
                let message = error.to_string();
                let _ = emit(WorkerFrame::Error {
                    task_id: None,
                    code: "invalid_command",
                    message: &message,
                    retryable: false,
                });
                continue;
            }
        };
        match command {
            WorkerCommand::Quit => break,
            WorkerCommand::Devices => match engine::device_inventory() {
                Ok(devices) => {
                    let _ = emit(WorkerFrame::Devices { devices: &devices });
                }
                Err(message) => {
                    let _ = emit(WorkerFrame::Error {
                        task_id: None,
                        code: "device_enumeration_failed",
                        message: &message,
                        retryable: false,
                    });
                }
            },
            WorkerCommand::Cancel { task_id } => {
                let _ = emit(WorkerFrame::Error {
                    task_id: Some(&task_id),
                    code: "cancelled",
                    message: "task cancelled before execution",
                    retryable: false,
                });
            }
            WorkerCommand::Run {
                task_id,
                node_id,
                model_id,
                input_artifacts,
                output_dir,
                config,
                ..
            } => {
                eprintln!("[uta-ggml-worker] node={node_id} model={model_id}");
                if let Err(message) =
                    run_task(&task_id, &model_id, &input_artifacts, &output_dir, &config)
                {
                    let _ = emit(WorkerFrame::Error {
                        task_id: Some(&task_id),
                        code: "native_inference_failed",
                        message: &message,
                        retryable: false,
                    });
                }
            }
        }
    }
}
