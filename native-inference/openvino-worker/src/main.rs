mod audio;
mod basic_pitch;
mod fcpe;
mod firered;
mod kaldi_fbank;
mod mel;
mod protocol;
mod rmvpe;
mod runtime;

use std::io::BufRead;
use std::path::Path;

use protocol::{COMPONENT_RECIPE, PROTOCOL_VERSION, WorkerCommand, WorkerFrame, emit};

fn run_task(
    task_id: &str,
    model_id: &str,
    input_artifacts: &[std::path::PathBuf],
    output_dir: &Path,
    config: &serde_json::Value,
) -> Result<(), String> {
    if !output_dir.is_dir() {
        return Err("authorized task output directory is unavailable".to_string());
    }
    let source = input_artifacts
        .first()
        .ok_or_else(|| "native OpenVINO task has no input audio artifact".to_string())?;
    emit(WorkerFrame::Progress {
        task_id,
        fraction: 0.01,
        message: "Decoding source audio to the model sample rate",
    })?;
    let sample_rate = if model_id == "basic_pitch" {
        22_050
    } else {
        16_000
    };
    let audio = audio::decode_mono(source, output_dir, sample_rate)?;
    let output = match model_id {
        "rmvpe" => rmvpe::infer(&audio, output_dir, config, |fraction, message| {
            let _ = emit(WorkerFrame::Progress {
                task_id,
                fraction: 0.02 + fraction * 0.97,
                message,
            });
        })?,
        "fcpe" => fcpe::infer(&audio, output_dir)?,
        "basic_pitch" => basic_pitch::infer(&audio, output_dir)?,
        "firered_asr2_aed" => firered::infer(&audio, output_dir)?,
        _ => {
            return Err(format!(
                "model {model_id} is not implemented by this OpenVINO worker"
            ));
        }
    };
    emit(WorkerFrame::Output {
        task_id,
        artifact: match model_id {
            "basic_pitch" => "onset_activation_evidence",
            "firered_asr2_aed" => "transcript_evidence",
            _ => "pitch_evidence",
        },
        path: &output,
        media_type: "application/json",
    })?;
    emit(WorkerFrame::Done {
        task_id,
        status: "ok",
    })
}

fn main() {
    runtime::configure_process_environment();
    if !std::env::args().any(|argument| argument == "--stdio-json") {
        eprintln!("uta-openvino-worker requires --stdio-json");
        std::process::exit(2);
    }
    if emit(WorkerFrame::Ready {
        protocol: PROTOCOL_VERSION,
        component: "uta-openvino-worker",
        runtime_recipe_digest: COMPONENT_RECIPE,
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
                eprintln!("OpenVINO worker stdin failed: {error}");
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
        if protocol::command_protocol(&command) != PROTOCOL_VERSION {
            let _ = emit(WorkerFrame::Error {
                task_id: None,
                code: "unsupported_protocol",
                message: "unsupported native worker protocol",
                retryable: false,
            });
            continue;
        }
        match command {
            WorkerCommand::Quit { .. } => break,
            WorkerCommand::Cancel { task_id, .. } => {
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
                eprintln!("[uta-openvino-worker] node={node_id} model={model_id}");
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
