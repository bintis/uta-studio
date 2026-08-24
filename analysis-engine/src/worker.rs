use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Deserialize;
use uta_runtime_manager::{RuntimeManager, RuntimePolicy, StorePaths};

use crate::contract::{
    AnalysisResultManifestV1, AnalyzeRequestV1, EngineError, EngineErrorCode, EngineResult,
    ExportRequestV1,
};
use crate::execution::CancellationToken;
use crate::{AnalysisEngine, ENGINE_VERSION, WORKER_PROTOCOL, WORKER_PROTOCOL_VERSION};

const MAX_WORKER_COMMAND_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum WorkerCommand {
    Hello {
        protocol: u32,
    },
    Capabilities {
        protocol: u32,
        #[serde(default)]
        runtime_policy: RuntimePolicy,
    },
    Validate {
        protocol: u32,
        request: AnalyzeRequestV1,
    },
    Requirements {
        protocol: u32,
        request: AnalyzeRequestV1,
    },
    Plan {
        protocol: u32,
        request: AnalyzeRequestV1,
    },
    Analyze {
        protocol: u32,
        request: AnalyzeRequestV1,
        output_dir: PathBuf,
    },
    Cancel {
        protocol: u32,
        request_id: String,
    },
    Export {
        protocol: u32,
        request: ExportRequestV1,
    },
    Quit {
        protocol: u32,
    },
}

impl WorkerCommand {
    fn protocol(&self) -> u32 {
        match self {
            Self::Hello { protocol }
            | Self::Capabilities { protocol, .. }
            | Self::Validate { protocol, .. }
            | Self::Requirements { protocol, .. }
            | Self::Plan { protocol, .. }
            | Self::Analyze { protocol, .. }
            | Self::Cancel { protocol, .. }
            | Self::Export { protocol, .. }
            | Self::Quit { protocol } => *protocol,
        }
    }
}

struct ActiveAnalysis {
    request_id: String,
    cancellation: CancellationToken,
    result: mpsc::Receiver<EngineResult<AnalysisResultManifestV1>>,
    handle: JoinHandle<()>,
}

enum InputFrame {
    Line(Result<String, String>),
    End,
    Failed(String),
}

pub fn worker_main() -> Result<(), String> {
    let engine = AnalysisEngine::from_env().map_err(|error| error.to_string())?;
    emit_ready()?;
    let (input_sender, input_receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            match read_bounded_line(&mut stdin) {
                Ok(Some(line)) => {
                    if input_sender.send(InputFrame::Line(line)).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = input_sender.send(InputFrame::End);
                    break;
                }
                Err(error) => {
                    let _ = input_sender.send(InputFrame::Failed(error));
                    break;
                }
            }
        }
    });

    let mut active: Option<ActiveAnalysis> = None;
    loop {
        if let Some(task) = active.as_mut() {
            match task.result.try_recv() {
                Ok(result) => {
                    emit_analysis_result(&task.request_id, result)?;
                    let task = active.take().expect("active task exists");
                    task.handle
                        .join()
                        .map_err(|_| "analysis worker thread panicked".to_string())?;
                    continue;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let task = active.take().expect("active task exists");
                    let _ = task.handle.join();
                    emit_error(
                        EngineError::new(
                            EngineErrorCode::InternalError,
                            "analysis worker thread ended without a result",
                        )
                        .for_request(task.request_id),
                    )?;
                    continue;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        let input = match input_receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(input) => input,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => InputFrame::End,
        };
        let line = match input {
            InputFrame::Line(Ok(line)) => line,
            InputFrame::Line(Err(message)) => {
                emit_protocol_error("invalid_contract", message)?;
                continue;
            }
            InputFrame::Failed(message) => return Err(message),
            InputFrame::End => {
                cancel_and_join(active.take());
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let command = match serde_json::from_str::<WorkerCommand>(&line) {
            Ok(command) => command,
            Err(error) => {
                emit_protocol_error("invalid_contract", error.to_string())?;
                continue;
            }
        };
        if command.protocol() != WORKER_PROTOCOL_VERSION {
            emit_protocol_error(
                "worker_protocol_mismatch",
                format!("unsupported worker protocol {}", command.protocol()),
            )?;
            continue;
        }
        match command {
            WorkerCommand::Hello { .. } => emit_ready()?,
            WorkerCommand::Capabilities { runtime_policy, .. } => emit(serde_json::json!({
                "type": "capabilities",
                "capabilities": engine.capabilities(runtime_policy)
            }))?,
            WorkerCommand::Validate { request, .. } => match engine.validate(&request) {
                Ok(()) => emit(serde_json::json!({
                    "type": "validation_result",
                    "request_id": request.request_id,
                    "valid": true
                }))?,
                Err(error) => emit_error(error.for_request(&request.request_id))?,
            },
            WorkerCommand::Requirements { request, .. } => match engine.requirements(&request) {
                Ok(requirements) => emit(serde_json::json!({
                    "type": "requirements",
                    "request_id": request.request_id,
                    "requirements": requirements
                }))?,
                Err(error) => emit_error(error.for_request(&request.request_id))?,
            },
            WorkerCommand::Plan { request, .. } => match engine.plan(&request) {
                Ok(plan) => emit(serde_json::json!({
                    "type": "plan",
                    "request_id": request.request_id,
                    "plan": plan
                }))?,
                Err(error) => emit_error(error.for_request(&request.request_id))?,
            },
            WorkerCommand::Analyze {
                request,
                output_dir,
                ..
            } => {
                if active.is_some() {
                    emit_error(
                        EngineError::new(
                            EngineErrorCode::WorkerFailed,
                            "this worker already has an active analysis request",
                        )
                        .for_request(request.request_id),
                    )?;
                    continue;
                }
                emit(serde_json::json!({
                    "type": "analysis_started",
                    "request_id": &request.request_id
                }))?;
                let request_id = request.request_id.clone();
                let cancellation = CancellationToken::default();
                let thread_token = cancellation.clone();
                let thread_engine = engine.clone();
                let (sender, result) = mpsc::channel();
                let handle = std::thread::spawn(move || {
                    let outcome = thread_engine.analyze_with_cancellation(
                        &request,
                        output_dir,
                        &thread_token,
                    );
                    let _ = sender.send(outcome);
                });
                active = Some(ActiveAnalysis {
                    request_id,
                    cancellation,
                    result,
                    handle,
                });
            }
            WorkerCommand::Cancel { request_id, .. } => {
                if let Some(task) = active.as_ref().filter(|task| task.request_id == request_id) {
                    task.cancellation.cancel();
                } else {
                    emit_error(
                        EngineError::new(
                            EngineErrorCode::InvalidContract,
                            "request is not active and cannot be cancelled",
                        )
                        .for_request(request_id),
                    )?;
                }
            }
            WorkerCommand::Export { request, .. } => match engine.export(&request) {
                Ok(()) => emit(serde_json::json!({
                    "type": "done",
                    "request_id": request.request_id,
                    "status": "ok"
                }))?,
                Err(error) => emit_error(error.for_request(&request.request_id))?,
            },
            WorkerCommand::Quit { .. } => {
                cancel_and_join(active.take());
                break;
            }
        }
    }
    Ok(())
}

fn emit_analysis_result(
    request_id: &str,
    result: EngineResult<AnalysisResultManifestV1>,
) -> Result<(), String> {
    match result {
        Ok(result) => emit(serde_json::json!({
            "type": "done",
            "request_id": request_id,
            "status": "ok",
            "result": result
        })),
        Err(error) if error.code == EngineErrorCode::Cancelled => emit(serde_json::json!({
            "type": "cancelled",
            "request_id": request_id
        })),
        Err(error) => emit_error(error.for_request(request_id)),
    }
}

fn cancel_and_join(active: Option<ActiveAnalysis>) {
    if let Some(task) = active {
        task.cancellation.cancel();
        let _ = task.handle.join();
    }
}

fn emit_protocol_error(code: &str, message: impl Into<String>) -> Result<(), String> {
    emit(serde_json::json!({
        "type": "error",
        "code": code,
        "message": message.into(),
        "retryable": false
    }))
}

fn emit_ready() -> Result<(), String> {
    emit(serde_json::json!({
        "type": "ready",
        "protocol": WORKER_PROTOCOL_VERSION,
        "protocol_identity": WORKER_PROTOCOL,
        "component": "uta-analysis-engine",
        "engine_version": ENGINE_VERSION,
        "contract_versions": [
            "uta.analysis-engine.request/1",
            "uta.analysis-engine.result/1"
        ]
    }))
}

fn emit_error(error: EngineError) -> Result<(), String> {
    let mut frame = serde_json::to_value(error).map_err(|error| error.to_string())?;
    frame["type"] = serde_json::json!("error");
    emit(frame)
}

fn emit(value: serde_json::Value) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value).map_err(|error| error.to_string())?;
    stdout.write_all(b"\n").map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}

#[derive(Debug, Deserialize)]
struct CompatibilityCommand {
    #[serde(rename = "type")]
    kind: String,
    protocol: u32,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    audio_path: Option<PathBuf>,
    #[serde(default)]
    cache_path: Option<PathBuf>,
    #[serde(default)]
    workflow_execution: Option<CompatibilityWorkflow>,
}

#[derive(Debug, Deserialize)]
struct CompatibilityWorkflow {
    #[serde(default)]
    quality_mode: String,
    #[serde(default)]
    node_bindings: Vec<CompatibilityBinding>,
}

#[derive(Debug, Deserialize)]
struct CompatibilityBinding {
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    runtime: String,
    #[serde(default)]
    execution_policy: CompatibilityPolicy,
}

#[derive(Debug, Default, Deserialize)]
struct CompatibilityPolicy {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    condition: String,
}

/// Temporary v0.5 Studio adapter. It retains the old command shape while all
/// parsing and fail-closed behavior live in the standalone Engine crate.
pub fn compatibility_worker_main() -> Result<(), String> {
    emit(serde_json::json!({
        "type": "ready",
        "protocol": WORKER_PROTOCOL_VERSION,
        "component": "uta-native-analyzer",
        "runtime_recipe_digest": uta_runtime_manager::runtime_lock::RUNTIME_LOCK_SHA256
    }))?;
    let mut stdin = std::io::stdin().lock();
    while let Some(line) = read_bounded_line(&mut stdin)? {
        let line = match line {
            Ok(line) => line,
            Err(message) => {
                emit(serde_json::json!({
                    "type": "error",
                    "kind": "invalid_command",
                    "msg": message
                }))?;
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let command = match serde_json::from_str::<CompatibilityCommand>(&line) {
            Ok(command) => command,
            Err(error) => {
                emit(serde_json::json!({
                    "type": "error",
                    "kind": "invalid_command",
                    "msg": error.to_string()
                }))?;
                continue;
            }
        };
        if command.protocol != WORKER_PROTOCOL_VERSION {
            emit(serde_json::json!({
                "type": "error",
                "kind": "unsupported_protocol",
                "msg": format!("unsupported native analyzer protocol {}", command.protocol)
            }))?;
            continue;
        }
        if command.kind == "quit" {
            break;
        }
        if command.kind != "analyze" {
            emit(serde_json::json!({
                "type": "error",
                "kind": "unsupported_command",
                "msg": format!("unsupported native analyzer command: {}", command.kind)
            }))?;
            continue;
        }
        let error = compatibility_analyze(&command).unwrap_err_or_else();
        emit(serde_json::json!({
            "type": "error",
            "kind": "native_runtime_unavailable",
            "msg": error
        }))?;
    }
    Ok(())
}

trait CompatibilityResultExt {
    fn unwrap_err_or_else(self) -> String;
}

impl CompatibilityResultExt for Result<(), String> {
    fn unwrap_err_or_else(self) -> String {
        match self {
            Ok(()) => "compatibility analysis unexpectedly returned without artifacts".to_string(),
            Err(error) => error,
        }
    }
}

fn read_bounded_line<R: BufRead>(reader: &mut R) -> Result<Option<Result<String, String>>, String> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(|error| error.to_string())?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Ok(Some(
                String::from_utf8(line).map_err(|error| error.to_string()),
            ));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.unwrap_or(available.len());
        if line.len().saturating_add(take) > MAX_WORKER_COMMAND_BYTES {
            let consumed = newline.map_or(available.len(), |index| index + 1);
            reader.consume(consumed);
            if newline.is_none() {
                drain_line(reader)?;
            }
            return Ok(Some(Err(format!(
                "worker command exceeds the {} byte limit",
                MAX_WORKER_COMMAND_BYTES
            ))));
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(newline.map_or(take, |index| index + 1));
        if newline.is_some() {
            return Ok(Some(
                String::from_utf8(line).map_err(|error| error.to_string()),
            ));
        }
    }
}

fn drain_line<R: BufRead>(reader: &mut R) -> Result<(), String> {
    loop {
        let available = reader.fill_buf().map_err(|error| error.to_string())?;
        if available.is_empty() {
            return Ok(());
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            reader.consume(newline + 1);
            return Ok(());
        }
        let consumed = available.len();
        reader.consume(consumed);
    }
}

fn compatibility_analyze(command: &CompatibilityCommand) -> Result<(), String> {
    let source = command
        .audio_path
        .as_ref()
        .ok_or_else(|| "analysis command omitted audio_path".to_string())?;
    if !source.is_file() {
        return Err(format!("source media is unavailable: {}", source.display()));
    }
    let output = command
        .cache_path
        .as_ref()
        .ok_or_else(|| "analysis command omitted cache_path".to_string())?;
    if !output.is_dir() {
        return Err(format!(
            "authorized cache directory is unavailable: {}",
            output.display()
        ));
    }
    let manager = RuntimeManager::with_default_catalog(StorePaths::from_env())
        .map_err(|error| error.to_string())?;
    for model_id in compatibility_models(command)? {
        manager
            .resolve_model(&model_id, RuntimePolicy::Experimental)
            .map_err(|error| {
                format!(
                    "native resource {model_id} is unavailable for testing: {}; install or repair it in Settings > Models & runtime",
                    error.message
                )
            })?;
    }
    Err(format!(
        "native workflow execution for {} is not implemented by the compatibility adapter",
        command.hash
    ))
}

fn compatibility_models(command: &CompatibilityCommand) -> Result<Vec<String>, String> {
    let Some(workflow) = command.workflow_execution.as_ref() else {
        return Ok([
            "bs_roformer_vocals_ep317",
            "melband_roformer_harmony",
            "qwen3_asr_1_7b",
            "qwen3_forced_aligner_0_6b",
            "rmvpe",
            "game",
        ]
        .into_iter()
        .map(str::to_string)
        .collect());
    };
    let mut required = BTreeMap::new();
    for binding in &workflow.node_bindings {
        let Some(model_id) = binding.model_id.as_deref() else {
            continue;
        };
        if binding.execution_policy.mode == "disabled"
            || (binding.execution_policy.mode == "conditional"
                && binding.execution_policy.condition == "maximum_only"
                && workflow.quality_mode != "maximum")
        {
            continue;
        }
        match binding.runtime.as_str() {
            "vulkan"
            | "openvino"
            | "open_vino"
            | "cpu_reference"
            | "pinned_qwen_asr_vulkan"
            | "pinned_qwen_align_vulkan" => {
                required.insert(model_id.to_string(), binding.runtime.clone());
            }
            "native_dsp" => continue,
            "unresolved" | "" => {
                return Err(format!(
                    "model {model_id} has no local runtime route selected for testing"
                ));
            }
            runtime => {
                return Err(format!(
                    "model {model_id} selected unknown runtime {runtime}"
                ));
            }
        }
    }
    Ok(required.into_keys().collect())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn compatibility_command(workflow: serde_json::Value) -> CompatibilityCommand {
        CompatibilityCommand {
            kind: "analyze".to_string(),
            protocol: WORKER_PROTOCOL_VERSION,
            hash: "fixture".to_string(),
            audio_path: None,
            cache_path: None,
            workflow_execution: Some(serde_json::from_value(workflow).unwrap()),
        }
    }

    #[test]
    fn bounded_reader_rejects_and_drains_oversized_frames() {
        let mut bytes = vec![b'a'; MAX_WORKER_COMMAND_BYTES + 1];
        bytes.extend_from_slice(b"\n{}\n");
        let mut reader = Cursor::new(bytes);
        assert!(read_bounded_line(&mut reader).unwrap().unwrap().is_err());
        assert_eq!(
            read_bounded_line(&mut reader).unwrap().unwrap().unwrap(),
            "{}"
        );
        assert!(read_bounded_line(&mut reader).unwrap().is_none());
    }

    #[test]
    fn compatibility_routing_uses_runtime_selection_not_model_name() {
        let command = compatibility_command(serde_json::json!({
            "quality_mode": "balanced",
            "node_bindings": [
                {
                    "model_id": "separation-model",
                    "runtime": "vulkan",
                    "execution_policy": {"mode": "always"}
                },
                {
                    "model_id": "pitch-model",
                    "runtime": "open_vino",
                    "execution_policy": {"mode": "always"}
                }
            ]
        }));
        let required = compatibility_models(&command).unwrap();
        assert_eq!(required, ["pitch-model", "separation-model"]);
    }

    #[test]
    fn compatibility_routing_fails_closed_for_unresolved_model() {
        let command = compatibility_command(serde_json::json!({
            "quality_mode": "balanced",
            "node_bindings": [{
                "model_id": "candidate-only",
                "runtime": "unresolved",
                "execution_policy": {"mode": "always"}
            }]
        }));
        assert!(
            compatibility_models(&command)
                .unwrap_err()
                .contains("no local runtime route selected for testing")
        );
    }
}
