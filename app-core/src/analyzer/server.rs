use super::*;

pub(crate) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) struct ServerProcess {
    pub(crate) child: Child,
    pub(crate) reader: BufReader<TcpStream>,
    pub(crate) writer: BufWriter<TcpStream>,
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let pid = self.child.id();
        info!("[analyzer] Killing server process (pid={pid})");
        SERVER_PID.store(0, Ordering::SeqCst);
        if let Ok(stream) = self.writer.get_ref().try_clone() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) static ANALYZER_SERVER: LazyLock<Mutex<Option<ServerProcess>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Debug, Deserialize)]
pub(crate) struct ReadyHandshake {
    port: u16,
    token: String,
    #[serde(default)]
    device: Option<String>,
}

pub(crate) fn drain_lines_to_log<R: BufRead + Send + 'static>(mut reader: R, label: &'static str) {
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    let trimmed = line.trim_end();
                    if !trimmed.is_empty() {
                        info!("[analyzer {label}] {trimmed}");
                    }
                }
            }
        }
    });
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
                    info!("[analyzer {label}] {trimmed}");
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
        UtaStudioError::Other(format!("{error}\nAnalyzer startup stderr:\n{details}"))
    }
}

pub(crate) fn read_ready_handshake<R: BufRead>(
    reader: &mut R,
) -> Result<ReadyHandshake, UtaStudioError> {
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Err(UtaStudioError::Other(
                "Analyzer server exited before handshake".into(),
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(value) if value.get("event").and_then(|v| v.as_str()) == Some("ready") => {
                return serde_json::from_value::<ReadyHandshake>(value)
                    .map_err(|e| UtaStudioError::Other(format!("Malformed ready handshake: {e}")));
            }
            _ => {
                info!("[analyzer stdout] {trimmed}");
            }
        }
    }
}

pub(crate) fn connect_and_authenticate(
    port: u16,
    token: &str,
) -> Result<(BufReader<TcpStream>, BufWriter<TcpStream>), UtaStudioError> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let stream = TcpStream::connect_timeout(&addr, HANDSHAKE_TIMEOUT)
        .map_err(|e| UtaStudioError::Other(format!("Failed to connect to analyzer server: {e}")))?;
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;

    let writer_stream = stream
        .try_clone()
        .map_err(|e| UtaStudioError::Other(format!("Failed to clone analyzer socket: {e}")))?;
    let mut reader = BufReader::new(stream);
    let mut writer = BufWriter::new(writer_stream);

    let hello = serde_json::json!({"type": "hello", "token": token});
    writer.write_all(serde_json::to_string(&hello).unwrap().as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    let mut line = String::new();
    let bytes = reader.read_line(&mut line)?;
    if bytes == 0 {
        return Err(UtaStudioError::Other(
            "Analyzer server closed connection during handshake".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_str(line.trim())?;
    if value.get("type").and_then(|v| v.as_str()) != Some("hello_ack") {
        return Err(UtaStudioError::Other(format!(
            "Analyzer auth failed: {}",
            line.trim()
        )));
    }

    reader.get_ref().set_read_timeout(None)?;
    reader.get_ref().set_write_timeout(None)?;

    Ok((reader, writer))
}

pub(crate) fn spawn_server() -> Result<ServerProcess, UtaStudioError> {
    let python = python_path();
    let script = analyzer_dir().join("server.py");
    let models = models_dir();
    let compute_backend = AppConfig::load()
        .compute_backend
        .unwrap_or_else(|| "cpu".to_string());
    let ffmpeg = ffmpeg_path();
    let ffmpeg_dir = ffmpeg.parent().unwrap_or(std::path::Path::new("."));
    let path_env = if let Some(existing) = std::env::var_os("PATH") {
        let mut paths = std::env::split_paths(&existing).collect::<Vec<_>>();
        paths.insert(0, ffmpeg_dir.to_path_buf());
        std::env::join_paths(paths).unwrap_or(existing)
    } else {
        ffmpeg_dir.as_os_str().to_os_string()
    };

    let mut cmd = silent_command(&python);
    cmd.env("PATH", &path_env)
        .env("TORCH_HOME", models.join("torch"))
        .env("HF_HOME", models.join("huggingface"))
        .env("PITCH_MODEL_DIR", models.join("pitch").join("rmvpe"))
        .env("FFMPEG_PATH", &ffmpeg)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONWARNINGS", "ignore")
        .env("PYTORCH_ENABLE_MPS_FALLBACK", "1")
        .env("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")
        .env("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
        .env("NLTK_DATA", models.join("nltk_data"))
        .env("NEMO_CACHE_DIR", models.join("nemo"))
        .env("ONNX_ASR_CACHE_DIR", models.join("onnx_asr"))
        .env(
            "OPENVINO_WHISPER_MODEL_DIR",
            models.join("whisper").join("openvino-large-v3-turbo"),
        )
        .env("OPENVINO_SEPARATOR_MODEL_DIR", models.join("separation"))
        .env("UTA_STUDIO_COMPUTE_BACKEND", compute_backend)
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| UtaStudioError::Other(format!("Failed to start analyzer server: {e}")))?;
    let pid = child.id();
    SERVER_PID.store(pid, Ordering::SeqCst);
    info!("[analyzer] Server process spawned (pid={pid})");

    let startup_stderr = Arc::new(Mutex::new(VecDeque::new()));
    let stderr_drain = match child.stderr.take() {
        Some(stderr) => drain_lines_to_log_and_capture(
            BufReader::new(stderr),
            "stderr",
            Arc::clone(&startup_stderr),
        ),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            SERVER_PID.store(0, Ordering::SeqCst);
            return Err(UtaStudioError::Other(
                "Failed to capture server stderr".into(),
            ));
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            SERVER_PID.store(0, Ordering::SeqCst);
            return Err(UtaStudioError::Other(
                "Failed to capture server stdout".into(),
            ));
        }
    };
    let mut stdout_reader = BufReader::new(stdout);

    let handshake = match read_ready_handshake(&mut stdout_reader) {
        Ok(h) => h,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_drain.join();
            SERVER_PID.store(0, Ordering::SeqCst);
            return Err(analyzer_startup_error(e, &startup_stderr));
        }
    };
    if let Some(device) = handshake.device.as_deref() {
        info!(
            "[analyzer] Handshake ok: device={device} port={}",
            handshake.port
        );
    } else {
        info!("[analyzer] Handshake ok: port={}", handshake.port);
    }

    let (reader, writer) = match connect_and_authenticate(handshake.port, &handshake.token) {
        Ok(pair) => pair,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_drain.join();
            SERVER_PID.store(0, Ordering::SeqCst);
            return Err(analyzer_startup_error(e, &startup_stderr));
        }
    };

    drain_lines_to_log(stdout_reader, "stdout");
    drop(stderr_drain);

    Ok(ServerProcess {
        child,
        reader,
        writer,
    })
}

pub(crate) fn ensure_server(
    guard: &mut std::sync::MutexGuard<Option<ServerProcess>>,
) -> Result<(), UtaStudioError> {
    if guard.is_some() {
        return Ok(());
    }
    let server = spawn_server()?;
    **guard = Some(server);
    Ok(())
}
