use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError, mpsc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::contract::{EngineError, EngineErrorCode, EngineResult};
use crate::events::begin_node_for_presentation;

const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_STDERR_BYTES: usize = 1024 * 1024;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(not(test))]
const OPENVINO_PROCESS_QUIESCENCE: Duration = Duration::from_secs(10);
#[cfg(test)]
const OPENVINO_PROCESS_QUIESCENCE: Duration = Duration::ZERO;
const OPENVINO_GATE_POLL: Duration = Duration::from_millis(25);

#[derive(Default)]
struct OpenVinoGate {
    last_exit: Option<Instant>,
}

struct OpenVinoLease {
    gate: MutexGuard<'static, OpenVinoGate>,
}

impl Drop for OpenVinoLease {
    fn drop(&mut self) {
        self.gate.last_exit = Some(Instant::now());
    }
}

fn uses_non_qwen_accelerator_worker(expectation: &WorkerExpectation) -> bool {
    matches!(
        expectation.component.as_str(),
        "uta-openvino-worker" | "uta-ggml-worker"
    )
}

fn acquire_openvino_lease(
    expectation: &WorkerExpectation,
    cancellation: &CancellationToken,
) -> EngineResult<Option<OpenVinoLease>> {
    if !uses_non_qwen_accelerator_worker(expectation) {
        return Ok(None);
    }
    static GATE: OnceLock<Mutex<OpenVinoGate>> = OnceLock::new();
    let gate = GATE.get_or_init(|| Mutex::new(OpenVinoGate::default()));
    let mut guard = loop {
        if cancellation.is_cancelled() {
            return Err(EngineError::new(
                EngineErrorCode::Cancelled,
                "analysis task was cancelled while waiting for the native accelerator runtime",
            ));
        }
        match gate.try_lock() {
            Ok(guard) => break guard,
            Err(TryLockError::Poisoned(error)) => break error.into_inner(),
            Err(TryLockError::WouldBlock) => std::thread::sleep(OPENVINO_GATE_POLL),
        }
    };
    if let Some(last_exit) = guard.last_exit {
        let deadline = last_exit + OPENVINO_PROCESS_QUIESCENCE;
        while Instant::now() < deadline {
            if cancellation.is_cancelled() {
                return Err(EngineError::new(
                    EngineErrorCode::Cancelled,
                    "analysis task was cancelled during native accelerator runtime quiescence",
                ));
            }
            std::thread::sleep(
                OPENVINO_GATE_POLL.min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
    // The lease remains held through process shutdown, preventing another
    // in-process analysis job from creating a concurrent non-Qwen OpenVINO or
    // GGML/Vulkan context.
    guard.last_exit = None;
    Ok(Some(OpenVinoLease { gate: guard }))
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone)]
pub struct WorkerExpectation {
    pub component: String,
    pub runtime_recipe_digest: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NativeTask {
    pub task_id: String,
    pub node_id: String,
    /// Optional compiled Processing Studio card identity when multiple cards
    /// execute the same Engine capability independently.
    pub presentation_node_id: Option<String>,
    pub model_id: String,
    pub input_artifacts: Vec<PathBuf>,
    pub output_dir: PathBuf,
    pub config: serde_json::Value,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgressEvent {
    pub task_id: String,
    pub fraction: f32,
    pub message: String,
    pub work_units_completed: Option<u64>,
    pub work_units_total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTaskOutput {
    pub artifact: String,
    pub path: PathBuf,
    pub media_type: String,
}

pub struct SupervisedWorker;

impl SupervisedWorker {
    pub fn run(
        executable: &Path,
        expectation: &WorkerExpectation,
        task: &NativeTask,
        cancellation: &CancellationToken,
        mut progress: impl FnMut(ProgressEvent),
    ) -> EngineResult<Vec<NativeTaskOutput>> {
        validate_task(task)?;
        if !executable.is_file() {
            return Err(EngineError::new(
                EngineErrorCode::WorkerUnavailable,
                format!("native worker is unavailable: {}", executable.display()),
            ));
        }
        let lifecycle = begin_node_for_presentation(
            &task.node_id,
            &task.node_id,
            Some(&task.model_id),
            &expectation.component,
            task.presentation_node_id.as_deref(),
        );
        let output_root = task.output_dir.canonicalize().map_err(|error| {
            EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                format!("could not authorize worker output directory: {error}"),
            )
        })?;
        let _openvino_lease = acquire_openvino_lease(expectation, cancellation)?;
        let mut process = WorkerProcess::spawn(executable)?;
        let deadline = Instant::now() + task.timeout;
        let ready = process.next_frame(deadline, cancellation)?;
        match ready {
            WorkerFrame::Ready {
                protocol,
                component,
                runtime_recipe_digest,
            } if protocol == PROTOCOL_VERSION
                && component == expectation.component
                && expectation
                    .runtime_recipe_digest
                    .as_ref()
                    .is_none_or(|expected| {
                        runtime_recipe_digest.as_deref() == Some(expected.as_str())
                    }) => {}
            WorkerFrame::Ready { .. } => {
                return Err(EngineError::new(
                    EngineErrorCode::WorkerProtocolMismatch,
                    "native worker ready identity does not match the resolved runtime",
                ));
            }
            _ => {
                return Err(EngineError::new(
                    EngineErrorCode::WorkerProtocolMismatch,
                    "native worker did not emit ready as its first frame",
                ));
            }
        }

        process.send(&WorkerCommand::Run {
            protocol: PROTOCOL_VERSION,
            task_id: &task.task_id,
            node_id: &task.node_id,
            model_id: &task.model_id,
            input_artifacts: &task.input_artifacts,
            output_dir: &output_root,
            config: &task.config,
        })?;

        let mut outputs = Vec::new();
        let mut last_fraction = 0.0_f32;
        let mut last_work_units: Option<(u64, u64)> = None;
        loop {
            match process.next_frame(deadline, cancellation)? {
                WorkerFrame::Progress {
                    task_id,
                    fraction,
                    message,
                    work_units_completed,
                    work_units_total,
                } => {
                    ensure_task_id(&task.task_id, &task_id)?;
                    if !fraction.is_finite()
                        || !(0.0..=1.0).contains(&fraction)
                        || fraction < last_fraction
                    {
                        return Err(protocol_error(
                            "worker progress fraction is invalid or regressed",
                        ));
                    }
                    last_fraction = fraction;
                    let work_units = match (work_units_completed, work_units_total) {
                        (None, None) => None,
                        (Some(completed), Some(total)) if total > 0 && completed <= total => {
                            if let Some((previous_completed, previous_total)) = last_work_units
                                && (total != previous_total || completed < previous_completed)
                            {
                                return Err(protocol_error(
                                    "worker progress work units changed identity or regressed",
                                ));
                            }
                            last_work_units = Some((completed, total));
                            Some((completed, total))
                        }
                        _ => {
                            return Err(protocol_error(
                                "worker progress work units are invalid or incomplete",
                            ));
                        }
                    };
                    let lifecycle_message = format!("[worker task {task_id}] {message}");
                    if let Some((completed, total)) = work_units {
                        lifecycle.measured_progress(
                            fraction,
                            completed,
                            total,
                            task_id.clone(),
                            lifecycle_message,
                        );
                    } else {
                        lifecycle.worker_progress(fraction, task_id.clone(), lifecycle_message);
                    }
                    progress(ProgressEvent {
                        task_id,
                        fraction,
                        message,
                        work_units_completed,
                        work_units_total,
                    });
                }
                WorkerFrame::Output {
                    task_id,
                    artifact,
                    path,
                    media_type,
                } => {
                    ensure_task_id(&task.task_id, &task_id)?;
                    if artifact.trim().is_empty() || media_type.trim().is_empty() {
                        return Err(protocol_error("worker output declaration is invalid"));
                    }
                    lifecycle.artifact(&artifact);
                    outputs.push(NativeTaskOutput {
                        artifact,
                        path: confined_output(&output_root, &path)?,
                        media_type,
                    });
                }
                WorkerFrame::Done { task_id, status } => {
                    ensure_task_id(&task.task_id, &task_id)?;
                    if status != "ok" || outputs.is_empty() {
                        return Err(EngineError::new(
                            EngineErrorCode::OutputValidationFailed,
                            "worker completed without a successful typed artifact",
                        ));
                    }
                    process.send(&WorkerCommand::Quit {
                        protocol: PROTOCOL_VERSION,
                    })?;
                    process.wait_for_exit(SHUTDOWN_TIMEOUT)?;
                    lifecycle.complete();
                    return Ok(outputs);
                }
                WorkerFrame::Error {
                    task_id,
                    code,
                    message,
                    retryable,
                } => {
                    if let Some(frame_task_id) = task_id.as_deref() {
                        ensure_task_id(&task.task_id, frame_task_id)?;
                    }
                    let error_code = if code == "cancelled" {
                        EngineErrorCode::Cancelled
                    } else {
                        EngineErrorCode::WorkerFailed
                    };
                    let mut error = EngineError::new(error_code, message)
                        .for_request(&task.task_id)
                        .with_capability(&task.node_id);
                    error.retryable = retryable;
                    return Err(error);
                }
                WorkerFrame::Ready { .. } => {
                    return Err(protocol_error("worker emitted duplicate ready frame"));
                }
            }
        }
    }
}

fn validate_task(task: &NativeTask) -> EngineResult<()> {
    let valid_id = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    if !valid_id(&task.task_id)
        || !valid_id(&task.node_id)
        || !valid_id(&task.model_id)
        || task.input_artifacts.is_empty()
        || !task.input_artifacts.iter().all(|path| path.is_file())
        || !task.output_dir.is_dir()
        || task.timeout.is_zero()
    {
        return Err(EngineError::new(
            EngineErrorCode::InvalidContract,
            "native worker task identity, inputs, output, or timeout is invalid",
        ));
    }
    Ok(())
}

fn ensure_task_id(expected: &str, actual: &str) -> EngineResult<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(protocol_error("worker frame task identity does not match"))
    }
}

fn confined_output(root: &Path, emitted: &Path) -> EngineResult<PathBuf> {
    let candidate = if emitted.is_absolute() {
        emitted.to_path_buf()
    } else {
        root.join(emitted)
    };
    let metadata = std::fs::symlink_metadata(&candidate).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("worker output is unavailable: {error}"),
        )
    })?;
    let canonical = candidate.canonicalize().map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("worker output could not be confined: {error}"),
        )
    })?;
    if !metadata.file_type().is_file() || !canonical.starts_with(root) {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "worker output escaped the authorized directory or is not a regular file",
        ));
    }
    Ok(canonical)
}

fn protocol_error(message: &str) -> EngineError {
    EngineError::new(EngineErrorCode::WorkerProtocolMismatch, message)
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WorkerCommand<'a> {
    Run {
        protocol: u32,
        task_id: &'a str,
        node_id: &'a str,
        model_id: &'a str,
        input_artifacts: &'a [PathBuf],
        output_dir: &'a Path,
        config: &'a serde_json::Value,
    },
    Quit {
        protocol: u32,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WorkerFrame {
    Ready {
        protocol: u32,
        component: String,
        #[serde(default)]
        runtime_recipe_digest: Option<String>,
    },
    Progress {
        task_id: String,
        fraction: f32,
        message: String,
        work_units_completed: Option<u64>,
        work_units_total: Option<u64>,
    },
    Output {
        task_id: String,
        artifact: String,
        path: PathBuf,
        media_type: String,
    },
    Done {
        task_id: String,
        status: String,
    },
    Error {
        task_id: Option<String>,
        code: String,
        message: String,
        retryable: bool,
    },
}

struct WorkerProcess {
    child: Child,
    stdin: ChildStdin,
    frames: mpsc::Receiver<Result<WorkerFrame, String>>,
    stderr: Arc<std::sync::Mutex<Vec<u8>>>,
}

impl WorkerProcess {
    fn spawn(executable: &Path) -> EngineResult<Self> {
        let mut command = Command::new(executable);
        command
            .arg("--stdio-json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|error| {
            EngineError::new(
                EngineErrorCode::WorkerUnavailable,
                format!("could not start native worker: {error}"),
            )
        })?;
        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(protocol_error("worker stdin unavailable"));
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(protocol_error("worker stdout unavailable"));
        };
        let Some(stderr_pipe) = child.stderr.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(protocol_error("worker stderr unavailable"));
        };
        let (sender, frames) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(line) = read_bounded_line(&mut reader, MAX_FRAME_BYTES) {
                let terminal = line.is_err();
                let frame = line.and_then(|line| {
                    serde_json::from_str::<WorkerFrame>(&line).map_err(|error| error.to_string())
                });
                if sender.send(frame).is_err() || terminal {
                    break;
                }
            }
        });
        let stderr = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = Arc::clone(&stderr);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr_pipe);
            let mut buffer = [0_u8; 8192];
            while let Ok(count) = std::io::Read::read(&mut reader, &mut buffer) {
                if count == 0 {
                    break;
                }
                let mut bytes = captured.lock().unwrap_or_else(|error| error.into_inner());
                let remaining = MAX_STDERR_BYTES.saturating_sub(bytes.len());
                bytes.extend_from_slice(&buffer[..count.min(remaining)]);
            }
        });
        Ok(Self {
            child,
            stdin,
            frames,
            stderr,
        })
    }

    fn send(&mut self, command: &WorkerCommand<'_>) -> EngineResult<()> {
        serde_json::to_writer(&mut self.stdin, command).map_err(|error| {
            protocol_error(&format!("could not encode worker command: {error}"))
        })?;
        self.stdin
            .write_all(b"\n")
            .and_then(|_| self.stdin.flush())
            .map_err(|error| {
                EngineError::new(
                    EngineErrorCode::WorkerFailed,
                    format!("could not send native worker command: {error}"),
                )
            })
    }

    fn next_frame(
        &mut self,
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> EngineResult<WorkerFrame> {
        loop {
            if cancellation.is_cancelled() {
                self.terminate();
                return Err(EngineError::new(
                    EngineErrorCode::Cancelled,
                    "analysis task was cancelled",
                ));
            }
            if Instant::now() >= deadline {
                self.terminate();
                return Err(EngineError::new(
                    EngineErrorCode::WorkerFailed,
                    "native worker task timed out",
                ));
            }
            match self.frames.recv_timeout(Duration::from_millis(25)) {
                Ok(Ok(frame)) => return Ok(frame),
                Ok(Err(error)) => {
                    self.terminate();
                    return Err(protocol_error(&format!("invalid worker frame: {error}")));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(status) = self.child.try_wait().map_err(|error| {
                        EngineError::new(
                            EngineErrorCode::WorkerFailed,
                            format!("could not inspect worker process: {error}"),
                        )
                    })? {
                        let stderr = self.stderr_text();
                        return Err(EngineError::new(
                            EngineErrorCode::WorkerFailed,
                            format!("native worker exited with {status}: {stderr}"),
                        ));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let status = self.child.wait().ok();
                    return Err(EngineError::new(
                        EngineErrorCode::WorkerFailed,
                        format!(
                            "native worker closed protocol output unexpectedly ({status:?}): {}",
                            self.stderr_text()
                        ),
                    ));
                }
            }
        }
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> EngineResult<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait().map_err(|error| {
                EngineError::new(
                    EngineErrorCode::WorkerFailed,
                    format!("could not inspect worker shutdown: {error}"),
                )
            })? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(EngineError::new(
                        EngineErrorCode::WorkerFailed,
                        format!("native worker failed during shutdown: {status}"),
                    ))
                };
            }
            if Instant::now() >= deadline {
                self.terminate();
                return Err(EngineError::new(
                    EngineErrorCode::WorkerFailed,
                    "native worker did not exit after quit",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn stderr_text(&self) -> String {
        String::from_utf8_lossy(
            &self
                .stderr
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
        .trim()
        .to_string()
    }

    fn terminate(&mut self) {
        #[cfg(unix)]
        // SAFETY: the worker is spawned as the leader of a fresh process group,
        // and a negative PID targets only that group.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        #[cfg(not(unix))]
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.terminate();
        }
    }
}

fn read_bounded_line<R: BufRead>(reader: &mut R, limit: usize) -> Option<Result<String, String>> {
    let mut bytes = Vec::new();
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) => return Some(Err(error.to_string())),
        };
        if available.is_empty() {
            return if bytes.is_empty() {
                None
            } else {
                Some(String::from_utf8(bytes).map_err(|error| error.to_string()))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.unwrap_or(available.len());
        if bytes.len().saturating_add(take) > limit {
            return Some(Err("worker protocol frame exceeds size limit".to_string()));
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(newline.map_or(take, |index| index + 1));
        if newline.is_some() {
            return Some(String::from_utf8(bytes).map_err(|error| error.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP_ROOT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn cancellation_token_is_shared() {
        let token = CancellationToken::default();
        let clone = token.clone();
        clone.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn non_qwen_accelerator_workers_use_the_process_quiescence_gate() {
        for component in ["uta-openvino-worker", "uta-ggml-worker"] {
            assert!(uses_non_qwen_accelerator_worker(&WorkerExpectation {
                component: component.to_string(),
                runtime_recipe_digest: None,
            }));
        }
        assert!(!uses_non_qwen_accelerator_worker(&WorkerExpectation {
            component: "uta-qwen-asr-worker".to_string(),
            runtime_recipe_digest: None,
        }));
    }

    fn temporary_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "uta-worker-client-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_TEMP_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[cfg(unix)]
    fn worker_script(root: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        // Publish the executable only after its writable handle is closed.
        // Rapid write-in-place followed by spawn can intermittently surface
        // ETXTBSY in the Nix sandbox even though `fs::write` has returned.
        let path = root.join("worker");
        let staging = root.join("worker.part");
        {
            let mut file = std::fs::File::create(&staging).unwrap();
            file.write_all(format!("#!/bin/sh\n{body}\n").as_bytes())
                .unwrap();
            file.sync_all().unwrap();
        }
        let mut permissions = std::fs::metadata(&staging).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&staging, permissions).unwrap();
        std::fs::rename(staging, &path).unwrap();
        path
    }

    #[test]
    fn confined_output_rejects_escape() {
        let root = temporary_root();
        let outside = root.with_extension("outside");
        std::fs::write(&outside, b"outside").unwrap();
        assert_eq!(
            confined_output(&root.canonicalize().unwrap(), &outside)
                .unwrap_err()
                .code,
            EngineErrorCode::OutputValidationFailed
        );
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn supervised_worker_validates_and_confines_typed_output() {
        let root = temporary_root();
        let input = root.join("input.wav");
        let output = root.join("evidence.json");
        std::fs::write(&input, b"input").unwrap();
        let executable = worker_script(
            &root,
            &format!(
                "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"fixture-worker\",\"runtime_recipe_digest\":\"recipe\"}}'\nread command\nprintf evidence > '{}'\nprintf '%s\\n' '{{\"type\":\"progress\",\"task_id\":\"task-1\",\"fraction\":0.5,\"message\":\"running\",\"work_units_completed\":2,\"work_units_total\":4}}'\nprintf '%s\\n%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-1\",\"artifact\":\"pitch_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}' '{{\"type\":\"done\",\"task_id\":\"task-1\",\"status\":\"ok\"}}'\nread quit",
                output.display(),
                output.display()
            ),
        );
        let task = NativeTask {
            task_id: "task-1".to_string(),
            node_id: "pitch.track".to_string(),
            presentation_node_id: None,
            model_id: "rmvpe".to_string(),
            input_artifacts: vec![input],
            output_dir: root.clone(),
            config: serde_json::Value::Null,
            timeout: Duration::from_secs(5),
        };
        let mut progress = Vec::new();
        let outputs = SupervisedWorker::run(
            &executable,
            &WorkerExpectation {
                component: "fixture-worker".to_string(),
                runtime_recipe_digest: Some("recipe".to_string()),
            },
            &task,
            &CancellationToken::default(),
            |event| progress.push(event),
        )
        .unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].path, output.canonicalize().unwrap());
        assert_eq!(progress[0].fraction, 0.5);
        assert_eq!(progress[0].work_units_completed, Some(2));
        assert_eq!(progress[0].work_units_total, Some(4));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn supervised_worker_rejects_runtime_recipe_mismatch() {
        let root = temporary_root();
        let input = root.join("input.wav");
        std::fs::write(&input, b"input").unwrap();
        let executable = worker_script(
            &root,
            "printf '%s\\n' '{\"type\":\"ready\",\"protocol\":1,\"component\":\"fixture-worker\",\"runtime_recipe_digest\":\"other-recipe\"}'\nsleep 1",
        );
        let error = SupervisedWorker::run(
            &executable,
            &WorkerExpectation {
                component: "fixture-worker".to_string(),
                runtime_recipe_digest: Some("expected-recipe".to_string()),
            },
            &NativeTask {
                task_id: "task-mismatch".to_string(),
                node_id: "pitch.track".to_string(),
                presentation_node_id: None,
                model_id: "rmvpe".to_string(),
                input_artifacts: vec![input],
                output_dir: root.clone(),
                config: serde_json::Value::Null,
                timeout: Duration::from_secs(5),
            },
            &CancellationToken::default(),
            |_| {},
        )
        .expect_err("a mismatched native runtime recipe must fail closed");
        assert_eq!(error.code, EngineErrorCode::WorkerProtocolMismatch);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn supervised_worker_rejects_invalid_or_regressing_progress() {
        let invalid_frames = [
            r#"{"type":"progress","task_id":"task-invalid","fraction":0.5,"message":"incomplete","work_units_completed":1}"#,
            r#"{"type":"progress","task_id":"task-invalid","fraction":0.5,"message":"range","work_units_completed":4,"work_units_total":3}"#,
            r#"{"type":"progress","task_id":"task-invalid","fraction":0.4,"message":"first","work_units_completed":2,"work_units_total":3}
{"type":"progress","task_id":"task-invalid","fraction":0.5,"message":"regressed","work_units_completed":1,"work_units_total":3}"#,
            r#"{"type":"progress","task_id":"task-invalid","fraction":0.6,"message":"first"}
{"type":"progress","task_id":"task-invalid","fraction":0.5,"message":"regressed"}"#,
        ];
        for (index, frames) in invalid_frames.into_iter().enumerate() {
            let root = temporary_root();
            let input = root.join("input.wav");
            std::fs::write(&input, b"input").unwrap();
            let executable = worker_script(
                &root,
                &format!(
                    "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"fixture-worker\"}}'\nread command\nprintf '%s\\n' '{frames}'\nsleep 1"
                ),
            );
            let task = NativeTask {
                task_id: "task-invalid".to_string(),
                node_id: "pitch.track".to_string(),
                presentation_node_id: None,
                model_id: "rmvpe".to_string(),
                input_artifacts: vec![input],
                output_dir: root.clone(),
                config: serde_json::Value::Null,
                timeout: Duration::from_secs(5),
            };
            let error = SupervisedWorker::run(
                &executable,
                &WorkerExpectation {
                    component: "fixture-worker".to_string(),
                    runtime_recipe_digest: None,
                },
                &task,
                &CancellationToken::default(),
                |_| {},
            )
            .expect_err("invalid progress must fail closed");
            assert_eq!(
                error.code,
                EngineErrorCode::WorkerProtocolMismatch,
                "case {index}"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    #[cfg(unix)]
    fn cancellation_kills_and_reaps_worker() {
        let root = temporary_root();
        let input = root.join("input.wav");
        std::fs::write(&input, b"input").unwrap();
        let executable = worker_script(
            &root,
            "printf '%s\\n' '{\"type\":\"ready\",\"protocol\":1,\"component\":\"fixture-worker\"}'\nread command\nsleep 30",
        );
        let task = NativeTask {
            task_id: "task-cancel".to_string(),
            node_id: "pitch.track".to_string(),
            presentation_node_id: None,
            model_id: "rmvpe".to_string(),
            input_artifacts: vec![input],
            output_dir: root.clone(),
            config: serde_json::Value::Null,
            timeout: Duration::from_secs(10),
        };
        let token = CancellationToken::default();
        let cancel = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            cancel.cancel();
        });
        assert_eq!(
            SupervisedWorker::run(
                &executable,
                &WorkerExpectation {
                    component: "fixture-worker".to_string(),
                    runtime_recipe_digest: None,
                },
                &task,
                &token,
                |_| {},
            )
            .unwrap_err()
            .code,
            EngineErrorCode::Cancelled
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
