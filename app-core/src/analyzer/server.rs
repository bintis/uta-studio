use std::io::{BufRead, BufReader, BufWriter};
use std::process::{ChildStdin, ChildStdout};

use super::*;

pub(crate) struct ServerProcess {
    pub(crate) child: Arc<Mutex<Child>>,
    pub(crate) reader: BufReader<ChildStdout>,
    pub(crate) writer: BufWriter<ChildStdin>,
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self
            .writer
            .write_all(b"{\"type\":\"quit\",\"protocol\":1}\n");
        let _ = self.writer.flush();
        let Ok(mut child) = self.child.lock() else {
            return;
        };
        let pid = child.id();
        info!("[native analyzer] stopping worker process (pid={pid})");
        SERVER_PID.store(0, Ordering::SeqCst);
        let _ = child.kill();
        let _ = child.wait();
        ACTIVE_SERVER_CHILD.lock().unwrap().take();
    }
}

pub(crate) static ANALYZER_SERVER: LazyLock<Mutex<Option<ServerProcess>>> =
    LazyLock::new(|| Mutex::new(None));
pub(crate) static ACTIVE_SERVER_CHILD: LazyLock<Mutex<Option<Arc<Mutex<Child>>>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Debug, Deserialize)]
pub(crate) struct ReadyHandshake {
    protocol: u32,
    component: String,
    #[serde(default)]
    runtime_recipe_digest: String,
}

pub(crate) fn drain_lines_to_log_and_capture<R: BufRead + Send + 'static>(
    mut reader: R,
    label: &'static str,
    captured: Arc<Mutex<VecDeque<String>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    let trimmed = line.trim_end();
                    if trimmed.is_empty() {
                        continue;
                    }
                    info!("[native analyzer {label}] {trimmed}");
                    if let Ok(mut lines) = captured.lock() {
                        if lines.len() == 24 {
                            lines.pop_front();
                        }
                        lines.push_back(trimmed.to_string());
                    }
                }
            }
        }
    })
}

pub(crate) fn analyzer_startup_error(
    error: UtaStudioError,
    captured: &Arc<Mutex<VecDeque<String>>>,
) -> UtaStudioError {
    let details = captured
        .lock()
        .ok()
        .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    if details.is_empty() {
        error
    } else {
        UtaStudioError::Other(format!("{error}\nNative analyzer stderr:\n{details}"))
    }
}

pub(crate) fn read_ready_handshake<R: BufRead>(
    reader: &mut R,
) -> Result<ReadyHandshake, UtaStudioError> {
    let mut line = String::new();
    line.clear();
    if reader.read_line(&mut line)? == 0 {
        return Err(UtaStudioError::Other(
            "Native analyzer exited before its ready frame".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_str(line.trim()).map_err(|error| {
        UtaStudioError::Other(format!(
            "Native analyzer stdout must contain NDJSON frames only: {error}"
        ))
    })?;
    if value.get("type").and_then(|value| value.as_str()) != Some("ready") {
        return Err(UtaStudioError::Other(
            "Native analyzer did not emit a ready frame first".into(),
        ));
    }
    let handshake: ReadyHandshake = serde_json::from_value(value)
        .map_err(|error| UtaStudioError::Other(format!("Malformed native ready frame: {error}")))?;
    if handshake.protocol != crate::native_runtime::NATIVE_WORKER_PROTOCOL_VERSION {
        return Err(UtaStudioError::Other(format!(
            "Unsupported native analyzer protocol {}",
            handshake.protocol
        )));
    }
    if handshake.runtime_recipe_digest != crate::native_runtime::RUNTIME_LOCK_SHA256 {
        return Err(UtaStudioError::Other(
            "Native analyzer runtime-lock identity does not match this Uta Studio build".into(),
        ));
    }
    Ok(handshake)
}

pub(crate) fn spawn_server() -> Result<ServerProcess, UtaStudioError> {
    let executable = crate::native_runtime::native_analyzer_path().ok_or_else(|| {
        UtaStudioError::Other(
            "Native analysis runtime is unavailable. Install it in Settings > Models & runtime."
                .to_string(),
        )
    })?;
    let mut command = Command::new(&executable);
    command
        .arg("--stdio-json")
        .env(
            "UTA_STUDIO_RUNTIME_LOCK_SHA256",
            crate::native_runtime::RUNTIME_LOCK_SHA256,
        )
        .env("UTA_STUDIO_MODELS_PATH", models_dir())
        .env("UTA_STUDIO_FFMPEG_PATH", ffmpeg_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        UtaStudioError::Other(format!("Failed to start native analyzer: {error}"))
    })?;
    let pid = child.id();
    SERVER_PID.store(pid, Ordering::SeqCst);
    info!("[native analyzer] worker process spawned (pid={pid})");

    let startup_stderr = Arc::new(Mutex::new(VecDeque::new()));
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| UtaStudioError::Other("Failed to capture native analyzer stderr".into()))?;
    let stderr_drain = drain_lines_to_log_and_capture(
        BufReader::new(stderr),
        "stderr",
        Arc::clone(&startup_stderr),
    );
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| UtaStudioError::Other("Failed to capture native analyzer stdout".into()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| UtaStudioError::Other("Failed to capture native analyzer stdin".into()))?;
    let mut reader = BufReader::new(stdout);
    let handshake = match read_ready_handshake(&mut reader) {
        Ok(handshake) => handshake,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_drain.join();
            SERVER_PID.store(0, Ordering::SeqCst);
            return Err(analyzer_startup_error(error, &startup_stderr));
        }
    };
    info!(
        "[native analyzer] ready component={} recipe={}",
        handshake.component, handshake.runtime_recipe_digest
    );

    let child = Arc::new(Mutex::new(child));
    *ACTIVE_SERVER_CHILD.lock().unwrap() = Some(Arc::clone(&child));
    Ok(ServerProcess {
        child,
        reader,
        writer: BufWriter::new(stdin),
    })
}

pub(crate) fn ensure_server(
    guard: &mut std::sync::MutexGuard<Option<ServerProcess>>,
) -> Result<(), UtaStudioError> {
    if guard.is_none() {
        **guard = Some(spawn_server()?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_handshake_requires_exact_protocol_and_runtime_lock() {
        let valid = format!(
            "{{\"type\":\"ready\",\"protocol\":1,\"component\":\"fixture\",\"runtime_recipe_digest\":\"{}\"}}\n",
            crate::native_runtime::RUNTIME_LOCK_SHA256
        );
        assert!(read_ready_handshake(&mut valid.as_bytes()).is_ok());

        let wrong_lock =
            b"{\"type\":\"ready\",\"protocol\":1,\"component\":\"fixture\",\"runtime_recipe_digest\":\"wrong\"}\n";
        assert!(
            read_ready_handshake(&mut &wrong_lock[..])
                .unwrap_err()
                .to_string()
                .contains("runtime-lock identity")
        );

        let wrong_protocol = format!(
            "{{\"type\":\"ready\",\"protocol\":2,\"component\":\"fixture\",\"runtime_recipe_digest\":\"{}\"}}\n",
            crate::native_runtime::RUNTIME_LOCK_SHA256
        );
        assert!(
            read_ready_handshake(&mut wrong_protocol.as_bytes())
                .unwrap_err()
                .to_string()
                .contains("Unsupported")
        );
    }

    #[test]
    fn ready_handshake_rejects_stdout_pollution() {
        let mut bytes = b"ordinary log line\n".as_slice();
        assert!(
            read_ready_handshake(&mut bytes)
                .unwrap_err()
                .to_string()
                .contains("NDJSON")
        );
    }
}
