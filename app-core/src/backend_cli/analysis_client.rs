use std::collections::BTreeMap;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;

use super::analysis_wire::*;
use super::error::BackendCliError;
use super::process::{
    discover_executable, native_command, read_machine_frame, spawn_stderr_drain, stderr_text,
};
use super::runtime_wire::RuntimePolicyWireV1;

#[derive(Clone)]
pub struct AnalysisCancelHandle {
    stdin: Arc<Mutex<ChildStdin>>,
    process_tree: Arc<AnalysisProcessTree>,
}

impl AnalysisCancelHandle {
    pub fn cancel(&self, request_id: &str) -> Result<(), BackendCliError> {
        write_command(
            &self.stdin,
            &serde_json::json!({
                "type":"cancel", "protocol":ANALYSIS_WORKER_PROTOCOL_VERSION,
                "request_id":request_id
            }),
        )
    }

    /// Immediately terminates the packaged Analysis Engine and every process
    /// it spawned. Unlike `cancel`, this does not wait for a model or adapter
    /// to observe a cooperative cancellation token.
    pub fn force_stop(&self) -> Result<(), BackendCliError> {
        self.process_tree.force_stop()
    }
}

struct AnalysisProcessTree {
    root_pid: u32,
    terminated: std::sync::atomic::AtomicBool,
    #[cfg(windows)]
    job: usize,
}

impl AnalysisProcessTree {
    fn attach(child: &Child) -> Result<Self, BackendCliError> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            };

            // SAFETY: the returned Job Object handle is owned by this value,
            // every pointer references a live local, and Drop closes it once.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err(BackendCliError::SpawnFailed(format!(
                        "could not create Analysis Engine Job Object: {}",
                        std::io::Error::last_os_error()
                    )));
                }
                let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    std::ptr::addr_of!(limits).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) == 0
                {
                    let error = std::io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(BackendCliError::SpawnFailed(format!(
                        "could not configure Analysis Engine Job Object: {error}"
                    )));
                }
                if AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0 {
                    let error = std::io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(BackendCliError::SpawnFailed(format!(
                        "could not contain Analysis Engine process tree: {error}"
                    )));
                }
                return Ok(Self {
                    root_pid: child.id(),
                    terminated: std::sync::atomic::AtomicBool::new(false),
                    job: job as usize,
                });
            }
        }
        #[cfg(not(windows))]
        {
            Ok(Self {
                root_pid: child.id(),
                terminated: std::sync::atomic::AtomicBool::new(false),
            })
        }
    }

    fn force_stop(&self) -> Result<(), BackendCliError> {
        use std::sync::atomic::Ordering;

        if self.terminated.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = {
            #[cfg(target_os = "linux")]
            {
                force_stop_linux_process_tree(self.root_pid)
            }
            #[cfg(all(unix, not(target_os = "linux")))]
            {
                // SAFETY: `connect_path` makes the packaged Engine the leader
                // of a fresh process group, so the negative PID cannot target
                // Studio.
                let result = unsafe { libc::kill(-(self.root_pid as i32), libc::SIGKILL) };
                if result == 0
                    || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                {
                    Ok(())
                } else {
                    Err(BackendCliError::Io(format!(
                        "could not force-stop Analysis Engine process group: {}",
                        std::io::Error::last_os_error()
                    )))
                }
            }
            #[cfg(windows)]
            {
                use windows_sys::Win32::Foundation::HANDLE;
                use windows_sys::Win32::System::JobObjects::TerminateJobObject;
                // SAFETY: `self.job` is a live Job Object owned by this value.
                if unsafe { TerminateJobObject(self.job as HANDLE, 137) } != 0 {
                    Ok(())
                } else {
                    Err(BackendCliError::Io(format!(
                        "could not force-stop Analysis Engine process tree: {}",
                        std::io::Error::last_os_error()
                    )))
                }
            }
            #[cfg(not(any(unix, windows)))]
            {
                Err(BackendCliError::Io(
                    "force-stopping the Analysis Engine process tree is unsupported on this platform"
                        .to_string(),
                ))
            }
        };
        if result.is_err() {
            self.terminated.store(false, Ordering::Release);
        }
        result
    }
}

#[cfg(windows)]
impl Drop for AnalysisProcessTree {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::HANDLE;
        // SAFETY: this value uniquely owns the Job Object handle.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job as HANDLE);
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_process_tree(root_pid: u32) -> Vec<u32> {
    let mut children = std::collections::HashMap::<u32, Vec<u32>>::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return vec![root_pid];
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some(after_name) = stat.rsplit_once(')').map(|(_, fields)| fields.trim()) else {
            continue;
        };
        let Some(parent) = after_name
            .split_ascii_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        children.entry(parent).or_default().push(pid);
    }
    let mut tree = vec![root_pid];
    let mut cursor = 0;
    while cursor < tree.len() {
        if let Some(direct) = children.get(&tree[cursor]) {
            tree.extend(direct.iter().copied());
        }
        cursor += 1;
    }
    tree
}

#[cfg(target_os = "linux")]
fn signal_linux_processes(pids: &[u32], signal: i32) {
    for pid in pids {
        // SAFETY: signals are sent only to the recorded packaged Engine tree.
        unsafe {
            libc::kill(*pid as i32, signal);
        }
    }
}

#[cfg(target_os = "linux")]
fn force_stop_linux_process_tree(root_pid: u32) -> Result<(), BackendCliError> {
    // Freeze the root and its current descendants first. A second snapshot
    // catches a child created immediately before its parent observed SIGSTOP.
    let first = linux_process_tree(root_pid);
    signal_linux_processes(&first, libc::SIGSTOP);
    let second = linux_process_tree(root_pid);
    signal_linux_processes(&second, libc::SIGSTOP);
    let mut all = first;
    all.extend(second);
    all.sort_unstable();
    all.dedup();
    for pid in all.into_iter().rev() {
        // SAFETY: every PID came from a descendant walk rooted at the packaged
        // Analysis Engine process started by this client.
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
    // Report only a genuine root-signal failure; ESRCH means the process
    // exited between the snapshot and signal, which is already stopped.
    let result = unsafe { libc::kill(root_pid as i32, libc::SIGKILL) };
    let error = std::io::Error::last_os_error();
    if result == 0 || error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(BackendCliError::Io(format!(
            "could not force-stop Analysis Engine process tree: {error}"
        )))
    }
}

pub struct AnalysisCliClient {
    executable: PathBuf,
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: BufReader<ChildStdout>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
    ready: AnalysisWorkerReadyV1,
    process_tree: Arc<AnalysisProcessTree>,
}

impl AnalysisCliClient {
    pub fn is_available() -> bool {
        discover_executable("UTA_STUDIO_ANALYSIS_CLI_PATH", "uta-analyze")
            .is_ok_and(|path| path.is_file())
    }
    pub fn connect() -> Result<Self, BackendCliError> {
        Self::connect_path(discover_executable(
            "UTA_STUDIO_ANALYSIS_CLI_PATH",
            "uta-analyze",
        )?)
    }

    pub fn connect_path(executable: impl Into<PathBuf>) -> Result<Self, BackendCliError> {
        let executable = executable.into();
        if !executable.is_file() {
            return Err(BackendCliError::ExecutableMissing(executable));
        }
        let mut command = native_command(&executable);
        command
            .args(["worker", "--stdio-json"])
            .env("UTA_STUDIO_MODELS_DIR", crate::cache::models_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let ffmpeg = crate::vendor::ffmpeg_path();
        if ffmpeg.is_file() {
            command.env("UTA_STUDIO_FFMPEG_PATH", ffmpeg);
        }
        let mut child = command
            .spawn()
            .map_err(|error| BackendCliError::SpawnFailed(error.to_string()))?;
        let process_tree = match AnalysisProcessTree::attach(&child) {
            Ok(process_tree) => Arc::new(process_tree),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let stdin = child.stdin.take().ok_or_else(|| {
            BackendCliError::SpawnFailed("analysis worker stdin was unavailable".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            BackendCliError::SpawnFailed("analysis worker stdout was unavailable".to_string())
        })?;
        let stderr_pipe = child.stderr.take().ok_or_else(|| {
            BackendCliError::SpawnFailed("analysis worker stderr was unavailable".to_string())
        })?;
        let (stderr, stderr_thread) = spawn_stderr_drain(stderr_pipe);
        let mut client = Self {
            executable,
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: BufReader::new(stdout),
            stderr,
            stderr_thread: Some(stderr_thread),
            ready: AnalysisWorkerReadyV1 {
                frame_type: String::new(),
                protocol: 0,
                protocol_identity: String::new(),
                component: String::new(),
                engine_version: String::new(),
                contract_versions: Vec::new(),
            },
            process_tree,
        };
        let frame = client
            .next_frame()?
            .ok_or_else(|| client.unexpected_exit("before ready handshake"))?;
        let ready: AnalysisWorkerReadyV1 = serde_json::from_value(frame).map_err(|error| {
            BackendCliError::MalformedFrame(format!("invalid analysis ready frame: {error}"))
        })?;
        validate_ready(&ready)?;
        client.ready = ready;
        Ok(client)
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }
    pub fn ready(&self) -> &AnalysisWorkerReadyV1 {
        &self.ready
    }

    pub fn cancellation_handle(&self) -> AnalysisCancelHandle {
        AnalysisCancelHandle {
            stdin: Arc::clone(&self.stdin),
            process_tree: Arc::clone(&self.process_tree),
        }
    }

    pub fn stderr_log(&self) -> String {
        stderr_text(&self.stderr)
    }

    pub fn hello(&mut self) -> Result<AnalysisWorkerReadyV1, BackendCliError> {
        self.send(
            &serde_json::json!({"type":"hello", "protocol":ANALYSIS_WORKER_PROTOCOL_VERSION}),
        )?;
        let frame = self.required_frame("hello response")?;
        let ready: AnalysisWorkerReadyV1 = decode(frame, "hello ready frame")?;
        validate_ready(&ready)?;
        Ok(ready)
    }

    pub fn capabilities(
        &mut self,
        runtime_policy: RuntimePolicyWireV1,
    ) -> Result<Vec<CapabilityDescriptorWireV1>, BackendCliError> {
        self.send(&serde_json::json!({
            "type":"capabilities",
            "protocol":ANALYSIS_WORKER_PROTOCOL_VERSION,
            "runtime_policy":runtime_policy.as_str()
        }))?;
        let frame = self.required_frame("capabilities response")?;
        ensure_type(&frame, "capabilities")?;
        decode_field(frame, "capabilities", "capabilities response")
    }

    pub fn validate(
        &mut self,
        request: &serde_json::Value,
        request_id: &str,
    ) -> Result<(), BackendCliError> {
        self.request_command("validate", request, request_id)
            .map(|_: serde_json::Value| ())
    }

    pub fn requirements(
        &mut self,
        request: &serde_json::Value,
        request_id: &str,
    ) -> Result<AnalysisRequirementsWireV1, BackendCliError> {
        let frame: serde_json::Value = self.request_command("requirements", request, request_id)?;
        decode_field(frame, "requirements", "requirements response")
    }

    pub fn plan(
        &mut self,
        request: &serde_json::Value,
        request_id: &str,
    ) -> Result<AnalysisPlanWireV1, BackendCliError> {
        let frame: serde_json::Value = self.request_command("plan", request, request_id)?;
        let plan: AnalysisPlanWireV1 = decode_field(frame, "plan", "plan response")?;
        if plan.schema != "uta.analysis-engine.plan" || plan.schema_version != 1 {
            return Err(BackendCliError::ContractMismatch(format!(
                "unsupported analysis plan {}/{}",
                plan.schema, plan.schema_version
            )));
        }
        if plan.request_id != request_id {
            return Err(BackendCliError::RequestIdMismatch {
                expected: request_id.to_string(),
                actual: Some(plan.request_id),
            });
        }
        Ok(plan)
    }

    pub fn analyze(
        &mut self,
        request: &serde_json::Value,
        request_id: &str,
        output_dir: &Path,
    ) -> Result<AnalysisResultManifestWireV1, BackendCliError> {
        self.analyze_with_events(request, request_id, output_dir, |_| {})
    }

    pub fn analyze_with_events(
        &mut self,
        request: &serde_json::Value,
        request_id: &str,
        output_dir: &Path,
        mut on_event: impl FnMut(AnalysisLifecycleFrameWireV1),
    ) -> Result<AnalysisResultManifestWireV1, BackendCliError> {
        self.send(&serde_json::json!({
            "type":"analyze", "protocol":ANALYSIS_WORKER_PROTOCOL_VERSION,
            "request":request, "output_dir":output_dir
        }))?;
        let started = self.required_frame("analysis_started response")?;
        self.domain_or_frame(started, request_id, "analysis_started")?;
        let mut worker_progress = BTreeMap::new();
        loop {
            let frame = self.required_frame("analysis terminal response")?;
            let frame_type = frame
                .get("type")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    BackendCliError::MalformedFrame("analysis frame omitted type".to_string())
                })?;
            match frame_type {
                "done" => {
                    check_request_id(&frame, request_id)?;
                    if frame.get("status").and_then(serde_json::Value::as_str) != Some("ok") {
                        return Err(BackendCliError::MalformedFrame(
                            "analysis done frame did not report ok".to_string(),
                        ));
                    }
                    return decode_field(frame, "result", "analysis result manifest");
                }
                "cancelled" => {
                    check_request_id(&frame, request_id)?;
                    return Err(BackendCliError::Domain {
                        code: "cancelled".to_string(),
                        message: "analysis was cancelled".to_string(),
                        retryable: false,
                        request_id: Some(request_id.to_string()),
                        capability: None,
                        resource: None,
                    });
                }
                "error" => return Err(domain_error(frame, Some(request_id))?),
                frame_type if AnalysisLifecycleFrameWireV1::is_lifecycle_type(frame_type) => {
                    check_request_id(&frame, request_id)?;
                    let event: AnalysisLifecycleFrameWireV1 =
                        decode(frame, "analysis lifecycle frame")?;
                    validate_lifecycle_event(&event)?;
                    validate_worker_progress_monotonic(&event, &mut worker_progress)?;
                    on_event(event);
                }
                _ => {
                    if frame.get("request_id").is_some() {
                        check_request_id(&frame, request_id)?;
                    }
                }
            }
        }
    }

    pub fn cancel(&self, request_id: &str) -> Result<(), BackendCliError> {
        self.cancellation_handle().cancel(request_id)
    }

    pub fn quit(mut self) -> Result<(), BackendCliError> {
        self.shutdown()
    }

    fn request_command<T: DeserializeOwned>(
        &mut self,
        command: &str,
        request: &serde_json::Value,
        request_id: &str,
    ) -> Result<T, BackendCliError> {
        self.send(&serde_json::json!({"type":command, "protocol":ANALYSIS_WORKER_PROTOCOL_VERSION, "request":request}))?;
        let frame = self.required_frame(&format!("{command} response"))?;
        let expected_type = if command == "validate" {
            "validation_result"
        } else {
            command
        };
        self.domain_or_frame(frame.clone(), request_id, expected_type)?;
        serde_json::from_value(frame).map_err(|error| {
            BackendCliError::MalformedFrame(format!("invalid {command} frame: {error}"))
        })
    }

    fn domain_or_frame(
        &self,
        frame: serde_json::Value,
        request_id: &str,
        expected_type: &str,
    ) -> Result<(), BackendCliError> {
        if frame.get("type").and_then(serde_json::Value::as_str) == Some("error") {
            return Err(domain_error(frame, Some(request_id))?);
        }
        ensure_type(&frame, expected_type)?;
        check_request_id(&frame, request_id)
    }

    fn send(&self, value: &serde_json::Value) -> Result<(), BackendCliError> {
        write_command(&self.stdin, value)
    }

    fn next_frame(&mut self) -> Result<Option<serde_json::Value>, BackendCliError> {
        read_machine_frame(&mut self.stdout)
    }

    fn required_frame(&mut self, context: &str) -> Result<serde_json::Value, BackendCliError> {
        self.next_frame()?
            .ok_or_else(|| self.unexpected_exit(context))
    }

    fn unexpected_exit(&mut self, context: &str) -> BackendCliError {
        let status = self
            .child
            .try_wait()
            .ok()
            .flatten()
            .map_or_else(|| "closed stdout".to_string(), |status| status.to_string());
        let stderr = stderr_text(&self.stderr);
        BackendCliError::UnexpectedExit(format!(
            "{context}: {status}{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!("; stderr: {stderr}")
            }
        ))
    }

    fn shutdown(&mut self) -> Result<(), BackendCliError> {
        if self
            .child
            .try_wait()
            .map_err(BackendCliError::from)?
            .is_none()
        {
            let _ = self.send(
                &serde_json::json!({"type":"quit", "protocol":ANALYSIS_WORKER_PROTOCOL_VERSION}),
            );
            let status = self.child.wait().map_err(BackendCliError::from)?;
            if !status.success() {
                return Err(BackendCliError::UnexpectedExit(format!(
                    "quit returned {status}"
                )));
            }
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
        Ok(())
    }
}

impl Drop for AnalysisCliClient {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.send(
                &serde_json::json!({"type":"quit", "protocol":ANALYSIS_WORKER_PROTOCOL_VERSION}),
            );
            if self.child.wait().is_err() {
                let _ = self.child.kill();
            }
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
    }
}

fn write_command(
    stdin: &Arc<Mutex<ChildStdin>>,
    value: &serde_json::Value,
) -> Result<(), BackendCliError> {
    let mut stdin = stdin
        .lock()
        .map_err(|_| BackendCliError::Io("analysis worker stdin lock was poisoned".to_string()))?;
    serde_json::to_writer(&mut *stdin, value)
        .map_err(|error| BackendCliError::Io(error.to_string()))?;
    stdin.write_all(b"\n").map_err(BackendCliError::from)?;
    stdin.flush().map_err(BackendCliError::from)
}

fn validate_ready(ready: &AnalysisWorkerReadyV1) -> Result<(), BackendCliError> {
    if ready.frame_type != "ready"
        || ready.protocol != ANALYSIS_WORKER_PROTOCOL_VERSION
        || ready.protocol_identity != ANALYSIS_WORKER_IDENTITY
        || ready.component != ANALYSIS_COMPONENT
    {
        return Err(BackendCliError::ProtocolMismatch(format!(
            "unexpected ready identity: {ready:?}"
        )));
    }
    if ready.engine_version.trim().is_empty() {
        return Err(BackendCliError::ProtocolMismatch(
            "analysis ready frame omitted engine version".to_string(),
        ));
    }
    for contract in [
        "uta.analysis-engine.request/1",
        "uta.analysis-engine.result/1",
    ] {
        if !ready
            .contract_versions
            .iter()
            .any(|version| version == contract)
        {
            return Err(BackendCliError::ContractMismatch(format!(
                "analysis worker does not support {contract}"
            )));
        }
    }
    Ok(())
}

fn validate_worker_progress_monotonic(
    event: &AnalysisLifecycleFrameWireV1,
    states: &mut BTreeMap<(String, String), (f32, Option<(u64, u64)>)>,
) -> Result<(), BackendCliError> {
    let Some(task_id) = event.worker_task_id.as_ref() else {
        return Ok(());
    };
    let progress = event.progress.ok_or_else(|| {
        BackendCliError::MalformedFrame(
            "worker-correlated lifecycle progress omitted its fraction".to_string(),
        )
    })?;
    let key = (event.node_id.clone(), task_id.clone());
    let units = event.work_units_completed.zip(event.work_units_total);
    if let Some((previous_progress, previous_units)) = states.get(&key) {
        let units_regressed = match (*previous_units, units) {
            (Some((previous_completed, previous_total)), Some((completed, total))) => {
                total != previous_total || completed < previous_completed
            }
            _ => false,
        };
        if progress < *previous_progress || units_regressed {
            return Err(BackendCliError::MalformedFrame(
                "worker-correlated lifecycle progress regressed".to_string(),
            ));
        }
    }
    let retained_units = units.or_else(|| states.get(&key).and_then(|(_, units)| *units));
    states.insert(key, (progress, retained_units));
    Ok(())
}

fn validate_lifecycle_event(event: &AnalysisLifecycleFrameWireV1) -> Result<(), BackendCliError> {
    let progress_valid = event
        .progress
        .is_none_or(|value| value.is_finite() && (0.0..=1.0).contains(&value));
    let work_units_valid = match (event.work_units_completed, event.work_units_total) {
        (None, None) => true,
        (Some(completed), Some(total)) => {
            total > 0
                && completed <= total
                && event
                    .worker_task_id
                    .as_deref()
                    .is_some_and(|task_id| !task_id.trim().is_empty())
        }
        _ => false,
    };
    if event.schema_version != 1
        || event.request_id.trim().is_empty()
        || event.node_id.trim().is_empty()
        || event.capability_id.trim().is_empty()
        || event
            .presentation_node_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        || event
            .model_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        || event
            .worker_task_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        || (event.worker_task_id.is_some() && event.frame_type != "node_progress")
        || event.implementation.trim().is_empty()
        || event.event_at_ms <= 0
        || !progress_valid
        || !work_units_valid
        || (event.frame_type == "node_progress" && event.progress.is_none())
        || (event.frame_type == "artifact"
            && event
                .artifact
                .as_deref()
                .is_none_or(|value| value.trim().is_empty()))
        || (matches!(event.frame_type.as_str(), "warning" | "degraded")
            && event
                .message
                .as_deref()
                .is_none_or(|value| value.trim().is_empty()))
    {
        return Err(BackendCliError::MalformedFrame(
            "analysis lifecycle frame violates its typed contract".to_string(),
        ));
    }
    Ok(())
}

fn ensure_type(frame: &serde_json::Value, expected: &str) -> Result<(), BackendCliError> {
    let actual = frame.get("type").and_then(serde_json::Value::as_str);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(BackendCliError::MalformedFrame(format!(
            "expected {expected} frame, got {actual:?}"
        )))
    }
}

fn check_request_id(frame: &serde_json::Value, expected: &str) -> Result<(), BackendCliError> {
    let actual = frame
        .get("request_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    if actual.as_deref() == Some(expected) {
        Ok(())
    } else {
        Err(BackendCliError::RequestIdMismatch {
            expected: expected.to_string(),
            actual,
        })
    }
}

fn domain_error(
    frame: serde_json::Value,
    expected_request_id: Option<&str>,
) -> Result<BackendCliError, BackendCliError> {
    let error: AnalysisErrorWireV1 = decode(frame, "analysis error frame")?;
    if let Some(expected) = expected_request_id
        && error.request_id.as_deref() != Some(expected)
    {
        return Err(BackendCliError::RequestIdMismatch {
            expected: expected.to_string(),
            actual: error.request_id,
        });
    }
    Ok(BackendCliError::Domain {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
        request_id: error.request_id,
        capability: error.capability,
        resource: error.resource,
    })
}

fn decode<T: DeserializeOwned>(
    frame: serde_json::Value,
    label: &str,
) -> Result<T, BackendCliError> {
    serde_json::from_value(frame)
        .map_err(|error| BackendCliError::MalformedFrame(format!("invalid {label}: {error}")))
}

fn decode_field<T: DeserializeOwned>(
    mut frame: serde_json::Value,
    field: &str,
    label: &str,
) -> Result<T, BackendCliError> {
    let value = frame
        .get_mut(field)
        .map(serde_json::Value::take)
        .ok_or_else(|| BackendCliError::MalformedFrame(format!("{label} omitted {field}")))?;
    decode(value, label)
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    fn event(progress: f32, completed: u64, total: u64) -> AnalysisLifecycleFrameWireV1 {
        AnalysisLifecycleFrameWireV1 {
            frame_type: "node_progress".to_string(),
            schema_version: 1,
            request_id: "request".to_string(),
            node_id: "pitch.track".to_string(),
            presentation_node_id: Some("workflow.f0_rmvpe".to_string()),
            capability_id: "pitch.track".to_string(),
            model_id: Some("rmvpe".to_string()),
            implementation: "openvino".to_string(),
            progress: Some(progress),
            work_units_completed: Some(completed),
            work_units_total: Some(total),
            worker_task_id: Some("rmvpe-task-7".to_string()),
            artifact: None,
            path: None,
            message: Some("window inference".to_string()),
            event_at_ms: 1,
        }
    }

    #[test]
    fn worker_progress_requires_identity_and_rejects_cross_frame_regression() {
        let first = event(0.5, 5, 10);
        validate_lifecycle_event(&first).unwrap();
        let mut states = BTreeMap::new();
        validate_worker_progress_monotonic(&first, &mut states).unwrap();
        validate_worker_progress_monotonic(&event(0.6, 6, 10), &mut states).unwrap();

        let error =
            validate_worker_progress_monotonic(&event(0.7, 4, 10), &mut states).unwrap_err();
        assert!(error.to_string().contains("progress regressed"));
        let error =
            validate_worker_progress_monotonic(&event(0.4, 7, 10), &mut states).unwrap_err();
        assert!(error.to_string().contains("progress regressed"));

        let mut missing_identity = event(0.5, 5, 10);
        missing_identity.worker_task_id = None;
        assert!(validate_lifecycle_event(&missing_identity).is_err());
    }
}
