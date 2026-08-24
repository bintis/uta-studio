mod audio;
#[cfg(test)]
mod converter_contract;
mod engine;
mod protocol;
mod runtime;

use std::io::BufRead;

use protocol::{PROTOCOL_VERSION, WorkerCommand, WorkerFrame, emit};

#[derive(Debug, Clone, Copy)]
pub enum WorkerKind {
    Asr,
    Align,
}

impl WorkerKind {
    pub fn component(self) -> &'static str {
        match self {
            Self::Asr => "uta-qwen-asr-worker",
            Self::Align => "uta-qwen-align-worker",
        }
    }

    pub fn engine_id(self) -> &'static str {
        match self {
            Self::Asr => "qwen3_asr_1_7b",
            Self::Align => "qwen3_forced_aligner_0_6b",
        }
    }

    pub fn model_id(self) -> &'static str {
        self.engine_id()
    }

    pub fn recipe_digest(self) -> &'static str {
        match self {
            Self::Asr => "53083b7b39dd2a805f441453ae07c797",
            Self::Align => "3ec367aaf3f723079851e2fbdbd375f8",
        }
    }

    pub fn source_commit(self) -> &'static str {
        match self {
            Self::Asr => "ea077b87590bcfb090d7c38c03ab36cd1c7005d3",
            Self::Align => "6dcc586e5073fd6e85ee5728e75f0903d6c70c6c",
        }
    }

    pub fn model_relative_path(self) -> &'static str {
        match self {
            Self::Asr => "qwen-asr/Qwen3-ASR-1.7B-Q4_K_M.gguf",
            Self::Align => "qwen-align/qwen3-forced-aligner-predict-woo-f16.gguf",
        }
    }

    fn artifact(self) -> &'static str {
        match self {
            Self::Asr => "transcript_evidence",
            Self::Align => "alignment_evidence",
        }
    }
}

fn valid_task_id(task_id: &str) -> bool {
    !task_id.is_empty()
        && task_id.len() <= 128
        && task_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn run_task(
    kind: WorkerKind,
    task_id: &str,
    model_id: &str,
    input_artifacts: &[std::path::PathBuf],
    output_dir: &std::path::Path,
    config: &serde_json::Value,
) -> Result<(), String> {
    if !valid_task_id(task_id) || model_id != kind.model_id() || !output_dir.is_dir() {
        return Err("Qwen task identity or output directory is invalid".to_string());
    }
    let source = input_artifacts
        .first()
        .ok_or_else(|| "Qwen task has no input audio artifact".to_string())?;
    emit(WorkerFrame::Progress {
        task_id,
        fraction: 0.02,
        message: "Validating pinned Qwen runtime and model",
    })?;
    let runtime = runtime::validate(kind)?;
    emit(WorkerFrame::Progress {
        task_id,
        fraction: 0.08,
        message: "Decoding audio to the pinned Qwen input contract",
    })?;
    let decoded = audio::decode_wav(source, output_dir, task_id)?;
    emit(WorkerFrame::Progress {
        task_id,
        fraction: 0.15,
        message: "Running pinned Qwen Vulkan engine",
    })?;
    let result = engine::run(kind, &runtime, &decoded, output_dir, config);
    let _ = std::fs::remove_file(&decoded);
    let output = result?;
    emit(WorkerFrame::Output {
        task_id,
        artifact: kind.artifact(),
        path: &output,
        media_type: "application/json",
    })?;
    emit(WorkerFrame::Done {
        task_id,
        status: "ok",
    })
}

pub fn main_stdio(kind: WorkerKind) {
    if !std::env::args().any(|argument| argument == "--stdio-json") {
        eprintln!("{} requires --stdio-json", kind.component());
        std::process::exit(2);
    }
    if emit(WorkerFrame::Ready {
        protocol: PROTOCOL_VERSION,
        component: kind.component(),
        runtime_recipe_digest: kind.recipe_digest(),
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
                eprintln!("Qwen worker stdin failed: {error}");
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
                eprintln!("[{}] node={node_id} model={model_id}", kind.component());
                if let Err(message) = run_task(
                    kind,
                    &task_id,
                    &model_id,
                    &input_artifacts,
                    &output_dir,
                    &config,
                ) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_ids_cannot_escape_the_output_directory() {
        assert!(valid_task_id("song_01-node-a"));
        assert!(!valid_task_id("../escape"));
        assert!(!valid_task_id("with/slash"));
    }

    #[test]
    fn qwen_components_keep_independent_recipe_identities() {
        assert_ne!(
            WorkerKind::Asr.recipe_digest(),
            WorkerKind::Align.recipe_digest()
        );
        assert_ne!(
            WorkerKind::Asr.source_commit(),
            WorkerKind::Align.source_commit()
        );
    }
}
