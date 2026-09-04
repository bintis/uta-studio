pub mod audio;
pub mod core;
pub mod engine;
pub mod protocol;

pub use core::profiler;
pub use core::*;
pub use engine::{HOP_SIZE, SAMPLE_RATE, infer_game_gguf};
pub use protocol::{PROTOCOL_VERSION, WorkerCommand, WorkerFrame, emit};

use std::io::BufRead;
use std::path::PathBuf;

pub const COMPONENT: &str = "uta-game-worker";
pub const RECIPE_DIGEST: &str = "game-native-recipe-v1";

pub fn run_stdio() {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|arg| arg == "--stdio-json") {
        eprintln!("{COMPONENT} requires --stdio-json");
        std::process::exit(1);
    }

    emit(&WorkerFrame::Ready {
        protocol: protocol::PROTOCOL_VERSION,
        component: COMPONENT,
        runtime_recipe_digest: RECIPE_DIGEST,
    });

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let command: WorkerCommand = match serde_json::from_str(trimmed) {
            Ok(cmd) => cmd,
            Err(e) => {
                emit(&WorkerFrame::Error {
                    task_id: None,
                    code: "invalid_command",
                    message: &format!("invalid JSON-lines worker command: {e}"),
                    retryable: false,
                });
                continue;
            }
        };

        match command {
            WorkerCommand::Run {
                protocol: _,
                task_id,
                node_id: _,
                model_id,
                input_artifacts,
                output_dir,
                config,
            } => {
                if model_id != "game" {
                    emit(&WorkerFrame::Error {
                        task_id: Some(&task_id),
                        code: "invalid_model",
                        message: &format!("{COMPONENT} does not support model {model_id}"),
                        retryable: false,
                    });
                    continue;
                }
                if input_artifacts.is_empty() {
                    emit(&WorkerFrame::Error {
                        task_id: Some(&task_id),
                        code: "missing_input",
                        message: "GAME worker requires at least one audio input",
                        retryable: false,
                    });
                    continue;
                }

                let input_path = &input_artifacts[0];
                let model_path = match config.get("model_path").and_then(|v| v.as_str()) {
                    Some(p) => PathBuf::from(p),
                    None => {
                        emit(&WorkerFrame::Error {
                            task_id: Some(&task_id),
                            code: "missing_model_path",
                            message: "GAME task requires config.model_path",
                            retryable: false,
                        });
                        continue;
                    }
                };

                let model_file = if model_path.is_file() {
                    model_path
                } else if model_path.join("game-medium-f32.gguf").is_file() {
                    model_path.join("game-medium-f32.gguf")
                } else {
                    model_path
                };

                emit(&WorkerFrame::Progress {
                    task_id: &task_id,
                    fraction: 0.01,
                    message: "Decoding source audio",
                    work_units_completed: None,
                    work_units_total: None,
                });

                let audio_samples =
                    match audio::decode_mono(input_path, &output_dir, engine::SAMPLE_RATE) {
                        Ok(samples) => samples,
                        Err(e) => {
                            emit(&WorkerFrame::Error {
                                task_id: Some(&task_id),
                                code: "decode_failed",
                                message: &format!("failed to decode input audio: {e}"),
                                retryable: false,
                            });
                            continue;
                        }
                    };

                let task_id_clone = task_id.clone();
                let result = engine::infer_game_gguf(
                    &audio_samples,
                    &model_file,
                    &output_dir,
                    &config,
                    |fraction, message, units| {
                        emit(&WorkerFrame::Progress {
                            task_id: &task_id_clone,
                            fraction,
                            message,
                            work_units_completed: units.map(|u| u.0),
                            work_units_total: units.map(|u| u.1),
                        });
                    },
                );

                match result {
                    Ok(output_path) => {
                        emit(&WorkerFrame::Output {
                            task_id: &task_id,
                            artifact: "note_candidate_evidence",
                            path: &output_path,
                            media_type: "application/json",
                        });
                        emit(&WorkerFrame::Done {
                            task_id: &task_id,
                            status: "ok",
                        });
                    }
                    Err(e) => {
                        emit(&WorkerFrame::Error {
                            task_id: Some(&task_id),
                            code: "inference_failed",
                            message: &format!("GAME native inference failed: {e}"),
                            retryable: false,
                        });
                    }
                }
            }
            WorkerCommand::Cancel {
                protocol: _,
                task_id,
            } => {
                emit(&WorkerFrame::Done {
                    task_id: &task_id,
                    status: "cancelled",
                });
            }
            WorkerCommand::Quit { protocol: _ } => {
                break;
            }
        }
    }
}
