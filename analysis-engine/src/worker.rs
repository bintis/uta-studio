use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Deserialize;
use uta_runtime_manager::RuntimePolicy;

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
    events: mpsc::Receiver<crate::events::EngineLifecycleEventV1>,
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
            while let Ok(event) = task.events.try_recv() {
                emit(serde_json::to_value(event).map_err(|error| error.to_string())?)?;
            }
            match task.result.try_recv() {
                Ok(result) => {
                    while let Ok(event) = task.events.try_recv() {
                        emit(serde_json::to_value(event).map_err(|error| error.to_string())?)?;
                    }
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
                let (event_sender, events) = mpsc::channel();
                let handle = std::thread::spawn(move || {
                    let sink = Arc::new(move |event| {
                        let _ = event_sender.send(event);
                    });
                    let outcome = thread_engine.analyze_with_events(
                        &request,
                        output_dir,
                        &thread_token,
                        sink,
                    );
                    let _ = sender.send(outcome);
                });
                active = Some(ActiveAnalysis {
                    request_id,
                    cancellation,
                    result,
                    events,
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

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
}
