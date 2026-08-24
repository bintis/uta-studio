use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{NATIVE_WORKER_PROTOCOL_VERSION, ResolvedNativeRuntime, WorkerCommand, WorkerFrame};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeTask {
    pub task_id: String,
    pub node_id: String,
    pub model_id: String,
    pub input_artifacts: Vec<PathBuf>,
    pub output_dir: PathBuf,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeTaskOutput {
    pub artifact: String,
    pub path: PathBuf,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeTaskResult {
    pub outputs: Vec<NativeTaskOutput>,
    pub runtime_recipe_digest: String,
}

fn write_command(
    writer: &mut BufWriter<impl Write>,
    command: &WorkerCommand,
) -> Result<(), String> {
    let json = serde_json::to_string(command).map_err(|error| error.to_string())?;
    writer
        .write_all(json.as_bytes())
        .and_then(|()| writer.write_all(b"\n"))
        .and_then(|()| writer.flush())
        .map_err(|error| error.to_string())
}

fn stop_worker(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_for_clean_exit(child: &mut std::process::Child, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "native worker exited with {status} after completion"
                ));
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                stop_worker(child);
                return Err("native worker did not exit after the quit command".to_string());
            }
            Err(error) => {
                stop_worker(child);
                return Err(format!("could not reap native worker: {error}"));
            }
        }
    }
}

fn output_is_inside(root: &Path, output: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let output = output.canonicalize().map_err(|error| error.to_string())?;
    if output.is_file() && output.starts_with(root) {
        Ok(())
    } else {
        Err(format!(
            "native worker output escaped its run directory: {}",
            output.display()
        ))
    }
}

pub fn run_native_task(
    runtime: &ResolvedNativeRuntime,
    task: NativeTask,
    timeout: Duration,
    cancel: Arc<AtomicBool>,
    mut on_progress: impl FnMut(f32, &str),
) -> Result<NativeTaskResult, String> {
    std::fs::create_dir_all(&task.output_dir).map_err(|error| error.to_string())?;
    let mut child = Command::new(&runtime.executable)
        .arg("--stdio-json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start native worker: {error}"))?;
    let mut writer = BufWriter::new(
        child
            .stdin
            .take()
            .ok_or_else(|| "native worker stdin is unavailable".to_string())?,
    );
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "native worker stdout is unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "native worker stderr is unavailable".to_string())?;

    let (frame_tx, frame_rx) = mpsc::channel::<Result<WorkerFrame, String>>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let frame = line.map_err(|error| error.to_string()).and_then(|line| {
                serde_json::from_str::<WorkerFrame>(&line).map_err(|error| {
                    format!("native worker polluted stdout with a non-protocol line: {error}")
                })
            });
            if frame_tx.send(frame).is_err() {
                return;
            }
        }
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            tracing::info!("[native worker stderr] {line}");
        }
    });

    let ready = match frame_rx.recv_timeout(Duration::from_secs(60)) {
        Ok(Ok(frame)) => frame,
        Ok(Err(error)) => {
            stop_worker(&mut child);
            return Err(error);
        }
        Err(_) => {
            stop_worker(&mut child);
            return Err("native worker did not become ready".to_string());
        }
    };
    let runtime_recipe_digest = match ready {
        WorkerFrame::Ready {
            protocol,
            runtime_recipe_digest,
            ..
        } if protocol == NATIVE_WORKER_PROTOCOL_VERSION => runtime_recipe_digest,
        WorkerFrame::Ready { protocol, .. } => {
            stop_worker(&mut child);
            return Err(format!("unsupported native worker protocol {protocol}"));
        }
        other => {
            stop_worker(&mut child);
            return Err(format!("expected native worker ready frame, got {other:?}"));
        }
    };

    if let Err(error) = write_command(
        &mut writer,
        &WorkerCommand::Run {
            protocol: NATIVE_WORKER_PROTOCOL_VERSION,
            task_id: task.task_id.clone(),
            node_id: task.node_id,
            model_id: task.model_id,
            input_artifacts: task.input_artifacts,
            output_dir: task.output_dir.clone(),
            config: task.config,
        },
    ) {
        stop_worker(&mut child);
        return Err(error);
    }

    let deadline = Instant::now() + timeout;
    let mut outputs = Vec::new();
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = write_command(
                &mut writer,
                &WorkerCommand::Cancel {
                    protocol: NATIVE_WORKER_PROTOCOL_VERSION,
                    task_id: task.task_id.clone(),
                },
            );
            stop_worker(&mut child);
            return Err("native task cancelled".to_string());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = write_command(
                &mut writer,
                &WorkerCommand::Cancel {
                    protocol: NATIVE_WORKER_PROTOCOL_VERSION,
                    task_id: task.task_id.clone(),
                },
            );
            std::thread::sleep(Duration::from_millis(250));
            stop_worker(&mut child);
            return Err("native task timed out".to_string());
        }
        let frame = frame_rx
            .recv_timeout(remaining.min(Duration::from_millis(100)))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => "poll".to_string(),
                mpsc::RecvTimeoutError::Disconnected => {
                    "native worker exited before a done frame".to_string()
                }
            });
        let frame = match frame {
            Err(message) if message == "poll" => continue,
            Err(message) => {
                stop_worker(&mut child);
                return Err(message);
            }
            Ok(Ok(frame)) => frame,
            Ok(Err(message)) => {
                stop_worker(&mut child);
                return Err(message);
            }
        };
        match frame {
            WorkerFrame::Progress {
                task_id,
                fraction,
                message,
            } if task_id == task.task_id => on_progress(fraction.clamp(0.0, 1.0), &message),
            WorkerFrame::Output {
                task_id,
                artifact,
                path,
                media_type,
            } if task_id == task.task_id => {
                if let Err(error) = output_is_inside(&task.output_dir, &path) {
                    stop_worker(&mut child);
                    return Err(error);
                }
                outputs.push(NativeTaskOutput {
                    artifact,
                    path,
                    media_type,
                });
            }
            WorkerFrame::Done { task_id, status } if task_id == task.task_id && status == "ok" => {
                if let Err(error) = write_command(
                    &mut writer,
                    &WorkerCommand::Quit {
                        protocol: NATIVE_WORKER_PROTOCOL_VERSION,
                    },
                ) {
                    stop_worker(&mut child);
                    return Err(error);
                }
                drop(writer);
                wait_for_clean_exit(&mut child, Duration::from_secs(1))?;
                return Ok(NativeTaskResult {
                    outputs,
                    runtime_recipe_digest,
                });
            }
            WorkerFrame::Done { task_id, status } if task_id == task.task_id => {
                stop_worker(&mut child);
                return Err(format!("native worker completed with status {status}"));
            }
            WorkerFrame::Error {
                task_id, message, ..
            } if task_id.as_deref().is_none_or(|id| id == task.task_id) => {
                stop_worker(&mut child);
                return Err(message);
            }
            _ => {}
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::native_runtime::NativeBackend;

    fn fixture(script: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "uta-native-supervisor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("worker");
        {
            let mut file = std::fs::File::create(&executable).unwrap();
            file.write_all(format!("#!/bin/sh\nset -eu\n{script}\n").as_bytes())
                .unwrap();
            file.sync_all().unwrap();
        }
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&executable, permissions).unwrap();
        (root, executable)
    }

    fn runtime(executable: PathBuf) -> ResolvedNativeRuntime {
        ResolvedNativeRuntime {
            model_id: "fixture".to_string(),
            component_id: "fixture".to_string(),
            backend: NativeBackend::NativeDsp,
            executable,
            runtime_recipe_digest: None,
        }
    }

    fn task(output_dir: PathBuf) -> NativeTask {
        NativeTask {
            task_id: "task-1".to_string(),
            node_id: "fixture.node".to_string(),
            model_id: "fixture".to_string(),
            input_artifacts: Vec::new(),
            output_dir,
            config: serde_json::Value::Null,
        }
    }

    #[test]
    fn completed_worker_receives_quit_and_exits_cleanly() {
        let (root, executable) = fixture(
            r#"
printf '%s\n' '{"type":"ready","protocol":1,"component":"fixture","runtime_recipe_digest":"fixture-lock"}'
IFS= read -r run
printf '%s\n' '{"type":"progress","task_id":"task-1","fraction":0.5,"message":"half"}'
printf '%s\n' '{"type":"done","task_id":"task-1","status":"ok"}'
IFS= read -r quit
"#,
        );
        let output = root.join("output");
        let mut progress = Vec::new();
        let result = run_native_task(
            &runtime(executable),
            task(output),
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
            |fraction, message| progress.push((fraction, message.to_string())),
        )
        .unwrap();
        assert_eq!(result.runtime_recipe_digest, "fixture-lock");
        assert_eq!(progress, [(0.5, "half".to_string())]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn polluted_stdout_is_rejected_and_the_worker_is_reaped() {
        let (root, executable) = fixture(
            r#"
printf '%s\n' '{"type":"ready","protocol":1,"component":"fixture","runtime_recipe_digest":"fixture-lock"}'
IFS= read -r run
printf '%s\n' 'ordinary log on stdout'
IFS= read -r ignored
"#,
        );
        let error = run_native_task(
            &runtime(executable),
            task(root.join("output")),
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
            |_, _| {},
        )
        .unwrap_err();
        assert!(error.contains("polluted stdout"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn timeout_terminates_a_nonresponsive_worker() {
        let (root, executable) = fixture(
            r#"
printf '%s\n' '{"type":"ready","protocol":1,"component":"fixture","runtime_recipe_digest":"fixture-lock"}'
IFS= read -r run
while IFS= read -r ignored; do :; done
"#,
        );
        let error = run_native_task(
            &runtime(executable),
            task(root.join("output")),
            Duration::from_millis(25),
            Arc::new(AtomicBool::new(false)),
            |_, _| {},
        )
        .unwrap_err();
        assert_eq!(error, "native task timed out");
        std::fs::remove_dir_all(root).unwrap();
    }
}
