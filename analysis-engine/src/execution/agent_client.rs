//! Subprocess adapter for the explicit, non-default "AI judgment" fusion
//! mode: a Runtime Manager-resolved Uta Fusion Agent Adapter selects the
//! final non-overlapping candidate path instead of the algorithmic decoder.
//!
//! This is deliberately not the `SupervisedWorker` NDJSON streaming protocol
//! (`execution::client`): this is a bounded single request/response exchange
//! with a manifest-verified adapter. A generic Codex, Claude, or other coding
//! agent CLI is not itself a compatible endpoint: one JSON document is written to stdin,
//! stdin closed to signal end of input, one JSON document read back from
//! stdout after the process exits successfully within a generous timeout.
//!
//! The agent may only *select* from the candidates it was given — every
//! returned candidate must equal (by id and full content) one of the
//! candidates in the request. This keeps the "never fabricate a measured
//! value" invariant that the rest of this crate's fusion code already
//! enforces: the agent chooses a path through real expert evidence, it does
//! not invent new evidence. The caller still runs the result through the
//! same `validate_canonical_singing_track` gate the algorithmic path uses;
//! there is no bypass.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::contract::{EngineError, EngineErrorCode, EngineResult};
use crate::fusion::{HardBoundarySetV1, SegmentCandidate, SingingFusionEvidence};

use super::client::CancellationToken;

const AGENT_REQUEST_CONTRACT: &str = "uta.fusion_agent_request";
const AGENT_RESPONSE_CONTRACT: &str = "uta.fusion_agent_response";
const AGENT_PROTOCOL_VERSION: u32 = crate::contract::FUSION_AGENT_PROTOCOL_VERSION;
const MAX_AGENT_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_AGENT_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const FUSION_CAPABILITY: &str = "fusion.candidate_graph";
const FUSION_ADAPTER_RESOURCE: &str = "tool:fusion_agent_adapter";

/// Owns the platform process-tree boundary for one adapter invocation.
/// Unix keeps the fresh process-group identity; Windows owns a kill-on-close
/// Job Object. Every exit closes the whole tree, including descendants that
/// keep inherited protocol pipes open after the direct child exits.
struct ProcessTreeGuard {
    #[cfg(unix)]
    process_group: Option<i32>,
    #[cfg(windows)]
    job: Option<windows_sys::Win32::Foundation::HANDLE>,
}

impl ProcessTreeGuard {
    fn attach(child: &Child) -> io::Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            };

            // SAFETY: all pointers refer to live local values for the duration
            // of each Win32 call; ownership of the returned handle is retained
            // by this guard and released exactly once in Drop.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err(io::Error::last_os_error());
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
                    let error = io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(error);
                }
                if AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0 {
                    let error = io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(error);
                }
                return Ok(Self { job: Some(job) });
            }
        }
        #[cfg(unix)]
        {
            Ok(Self {
                process_group: Some(child.id() as i32),
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self {})
        }
    }

    fn terminate_descendants(&mut self) {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group.take() {
            // SAFETY: the adapter was spawned as leader of this fresh group.
            unsafe {
                libc::kill(-process_group, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = self.job.take() {
            // SAFETY: this guard uniquely owns the Job Object handle. Closing
            // it terminates every process assigned under kill-on-close.
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(job);
            }
        }
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        self.terminate_descendants();
    }
}

#[cfg(windows)]
fn resume_suspended_child(child: &Child) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    // The child is created suspended, so it cannot create an uncontained
    // descendant before Job Object assignment. Resume every thread belonging
    // to the new process (normally just its primary thread).
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..THREADENTRY32::default()
        };
        let mut found = false;
        let mut has_entry = Thread32First(snapshot, std::ptr::addr_of_mut!(entry)) != 0;
        while has_entry {
            if entry.th32OwnerProcessID == child.id() {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if thread.is_null() {
                    let error = io::Error::last_os_error();
                    CloseHandle(snapshot);
                    return Err(error);
                }
                let resumed = ResumeThread(thread);
                let resume_error = (resumed == u32::MAX).then(io::Error::last_os_error);
                CloseHandle(thread);
                if let Some(error) = resume_error {
                    CloseHandle(snapshot);
                    return Err(error);
                }
                found = true;
            }
            has_entry = Thread32Next(snapshot, std::ptr::addr_of_mut!(entry)) != 0;
        }
        CloseHandle(snapshot);
        if !found {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "suspended fusion agent primary thread was not found",
            ));
        }
    }
    Ok(())
}

const FUSION_AGENT_INSTRUCTIONS: &str = "You are selecting the final non-overlapping sequence of singing note segments for a karaoke chart. You are given `candidates` plus the exact pool-level `hard_boundaries`; each candidate is already in the exact JSON shape you must also use in your response. Return one JSON document of the form {\"contract\":\"uta.fusion_agent_response\",\"version\":3,\"selected\":[...]} where `selected` is an ordered, non-overlapping valid subset that exactly covers represented voiced components and never crosses a hard-boundary edge. Silence, instrumental passages, intros, and outros do not require note coverage. Every object in `selected` must be copied verbatim from `candidates`; do not invent evidence. Return only the response JSON; do not return chain-of-thought.";

fn executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[derive(Serialize)]
struct AgentFusionRequestV1<'a> {
    contract: &'static str,
    version: u32,
    instructions: &'static str,
    hard_boundaries: &'a HardBoundarySetV1,
    candidates: &'a [SegmentCandidate],
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentFusionResponseV1 {
    contract: String,
    version: u32,
    selected: Vec<SegmentCandidate>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FusionAgentDecisionV1 {
    pub selected: Vec<SegmentCandidate>,
    pub candidate_set_digest: String,
    pub response_digest: String,
}

struct DigestWriter(Sha256);

impl Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct BoundedRequestBuffer {
    bytes: Vec<u8>,
    oversized: bool,
}

impl BoundedRequestBuffer {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            oversized: false,
        }
    }
}

impl Write for BoundedRequestBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_AGENT_REQUEST_BYTES.saturating_sub(self.bytes.len());
        if bytes.len() > remaining {
            self.oversized = true;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fusion agent request exceeded the bounded protocol limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn candidate_set_digest<T: Serialize + ?Sized>(pool: &T) -> EngineResult<String> {
    let mut writer = DigestWriter(Sha256::new());
    serde_json::to_writer(&mut writer, pool).map_err(|error| {
        worker_failed(format!(
            "could not encode fusion candidate-set identity: {error}"
        ))
    })?;
    Ok(format!("{:x}", writer.0.finalize()))
}

fn encode_agent_request(request: &AgentFusionRequestV1<'_>) -> EngineResult<Vec<u8>> {
    let mut writer = BoundedRequestBuffer::new();
    if let Err(error) = serde_json::to_writer(&mut writer, request) {
        return if writer.oversized {
            Err(agent_error(
                EngineErrorCode::OutputValidationFailed,
                "fusion agent request exceeded the bounded protocol limit",
            ))
        } else {
            Err(worker_failed(format!(
                "could not encode fusion agent request: {error}"
            )))
        };
    }
    Ok(writer.bytes)
}

fn agent_error(code: EngineErrorCode, message: impl Into<String>) -> EngineError {
    EngineError::new(code, message)
        .with_capability(FUSION_CAPABILITY)
        .with_resource(FUSION_ADAPTER_RESOURCE)
}

fn worker_unavailable(message: impl Into<String>) -> EngineError {
    agent_error(EngineErrorCode::WorkerUnavailable, message)
}

fn worker_failed(message: impl Into<String>) -> EngineError {
    agent_error(EngineErrorCode::WorkerFailed, message)
}

fn protocol_mismatch(message: impl Into<String>) -> EngineError {
    agent_error(EngineErrorCode::WorkerProtocolMismatch, message)
}

/// Compatibility entry for candidate-only callers. Production selection uses
/// `run_fusion_agent_for_pool` so structural hard-boundary identity is never
/// detached from the candidates it governs.
pub fn run_fusion_agent(
    executable: &Path,
    candidates: &[SegmentCandidate],
    timeout: Duration,
    cancellation: &CancellationToken,
) -> EngineResult<FusionAgentDecisionV1> {
    let pool = SingingFusionEvidence {
        schema_version: 2,
        candidates: candidates.to_vec(),
        hard_boundaries: HardBoundarySetV1::default(),
    };
    run_fusion_agent_for_pool(executable, &pool, timeout, cancellation)
}

/// Run the configured fusion agent over one complete selector-independent
/// pool. Never falls back to Algorithm on any provider or protocol failure.
pub fn run_fusion_agent_for_pool(
    executable: &Path,
    pool: &SingingFusionEvidence,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> EngineResult<FusionAgentDecisionV1> {
    if pool.candidates.is_empty() {
        return Err(worker_failed(
            "fusion agent has no candidates to select from",
        ));
    }
    pool.hard_boundaries
        .validate()
        .map_err(|message| agent_error(EngineErrorCode::OutputValidationFailed, message))?;
    if !executable_file(executable) {
        return Err(worker_unavailable(format!(
            "fusion agent executable is unavailable: {}",
            executable.display()
        )));
    }
    let candidate_set_digest = candidate_set_digest(pool)?;
    let request = AgentFusionRequestV1 {
        contract: AGENT_REQUEST_CONTRACT,
        version: AGENT_PROTOCOL_VERSION,
        instructions: FUSION_AGENT_INSTRUCTIONS,
        hard_boundaries: &pool.hard_boundaries,
        candidates: &pool.candidates,
    };
    let payload = encode_agent_request(&request)?;
    let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
        agent_error(
            EngineErrorCode::WorkerTimeout,
            "fusion agent timeout exceeds the supported monotonic clock range",
        )
    })?;

    let mut command = Command::new(executable);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
    }
    let mut child = command
        .spawn()
        .map_err(|error| worker_unavailable(format!("could not start fusion agent: {error}")))?;
    let mut process_tree = match ProcessTreeGuard::attach(&child) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(worker_unavailable(format!(
                "could not supervise fusion agent process tree: {error}"
            )));
        }
    };
    #[cfg(windows)]
    if let Err(error) = resume_suspended_child(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(worker_unavailable(format!(
            "could not resume supervised fusion agent: {error}"
        )));
    }

    let stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            terminate(&mut child);
            return Err(protocol_mismatch("fusion agent stdin unavailable"));
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate(&mut child);
            return Err(protocol_mismatch("fusion agent stdout unavailable"));
        }
    };
    let stderr_pipe = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            terminate(&mut child);
            return Err(protocol_mismatch("fusion agent stderr unavailable"));
        }
    };

    let (stdout_tx, stdout_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = stdout;
        let mut buffer = Vec::new();
        let mut oversized = false;
        let mut chunk = [0_u8; 8192];
        loop {
            match std::io::Read::read(&mut reader, &mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    let remaining = MAX_AGENT_RESPONSE_BYTES.saturating_sub(buffer.len());
                    buffer.extend_from_slice(&chunk[..count.min(remaining)]);
                    oversized |= count > remaining;
                }
                Err(_) => break,
            }
        }
        let _ = stdout_tx.send((buffer, oversized));
    });

    std::thread::spawn(move || {
        let mut reader = stderr_pipe;
        let mut buffer = [0_u8; 4096];
        while let Ok(count) = std::io::Read::read(&mut reader, &mut buffer) {
            if count == 0 {
                break;
            }
        }
    });

    let (stdin_tx, stdin_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stdin = stdin;
        let result = stdin.write_all(&payload).map_err(|_| ());
        // Dropping stdin closes the write end and signals request EOF.
        drop(stdin);
        let _ = stdin_tx.send(result);
    });

    let mut request_sent = false;
    let status = loop {
        if !request_sent {
            match stdin_rx.try_recv() {
                Ok(Ok(())) => request_sent = true,
                Ok(Err(())) => {
                    // The provider closed (or never opened) its stdin read
                    // end before the writer finished, so the write failed
                    // with a broken pipe. That is not proof the provider
                    // failed: a provider is free to decide its answer
                    // without draining the whole candidate pool. Keep
                    // waiting for its real exit status and response instead
                    // of failing the invocation on a transport detail.
                    request_sent = true;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    terminate(&mut child);
                    return Err(worker_failed(
                        "fusion agent request writer exited unexpectedly",
                    ));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if cancellation.is_cancelled() {
            terminate(&mut child);
            return Err(agent_error(
                EngineErrorCode::Cancelled,
                "fusion agent was cancelled",
            ));
        }
        if Instant::now() >= deadline {
            terminate(&mut child);
            return Err(agent_error(
                EngineErrorCode::WorkerTimeout,
                "fusion agent timed out",
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(error) => {
                terminate(&mut child);
                return Err(worker_failed(format!(
                    "could not inspect fusion agent process: {error}"
                )));
            }
        }
    };

    // The direct adapter has exited. Close its whole process tree before
    // waiting for protocol I/O so a detached descendant cannot retain an
    // inherited pipe or outlive this invocation.
    process_tree.terminate_descendants();
    if !status.success() {
        return Err(worker_failed(format!(
            "fusion agent exited unsuccessfully ({status}); provider diagnostics were not retained"
        )));
    }
    while !request_sent {
        match stdin_rx.try_recv() {
            Ok(Ok(())) => request_sent = true,
            // See the matching comment above: the provider already exited
            // successfully, so a broken-pipe write failure here is not a
            // transport failure worth reporting.
            Ok(Err(())) => request_sent = true,
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err(worker_failed(
                    "fusion agent closed before the bounded request was sent",
                ));
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if cancellation.is_cancelled() {
            return Err(agent_error(
                EngineErrorCode::Cancelled,
                "fusion agent was cancelled",
            ));
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(agent_error(
                EngineErrorCode::WorkerTimeout,
                "fusion agent timed out while closing its request stream",
            ));
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
    let (stdout_bytes, oversized) = loop {
        match stdout_rx.try_recv() {
            Ok(response) => break response,
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err(protocol_mismatch(
                    "fusion agent closed stdout without a response",
                ));
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if cancellation.is_cancelled() {
            return Err(agent_error(
                EngineErrorCode::Cancelled,
                "fusion agent was cancelled",
            ));
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(agent_error(
                EngineErrorCode::WorkerTimeout,
                "fusion agent timed out while closing its response stream",
            ));
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    };
    if oversized {
        return Err(protocol_mismatch(
            "fusion agent response exceeded the bounded protocol limit",
        ));
    }
    let response_digest = format!("{:x}", Sha256::digest(&stdout_bytes));
    let response: AgentFusionResponseV1 = serde_json::from_slice(&stdout_bytes)
        .map_err(|_| protocol_mismatch("fusion agent response is not valid protocol JSON"))?;
    if response.contract != AGENT_RESPONSE_CONTRACT || response.version != AGENT_PROTOCOL_VERSION {
        return Err(protocol_mismatch(
            "fusion agent response has an unsupported contract or version",
        ));
    }
    if response.selected.is_empty() {
        return Err(agent_error(
            EngineErrorCode::OutputValidationFailed,
            "fusion agent selected no candidates",
        ));
    }
    let known: BTreeMap<&str, &SegmentCandidate> = pool
        .candidates
        .iter()
        .map(|candidate| (candidate.id.as_str(), candidate))
        .collect();
    for selected in &response.selected {
        match known.get(selected.id.as_str()) {
            Some(original) if *original == selected => {}
            _ => {
                return Err(agent_error(
                    EngineErrorCode::OutputValidationFailed,
                    "fusion agent selected a candidate that was not verbatim in the given candidate pool",
                ));
            }
        }
    }
    Ok(FusionAgentDecisionV1 {
        selected: response.selected,
        candidate_set_digest,
        response_digest,
    })
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    // SAFETY: the agent is spawned as the leader of a fresh process group,
    // and a negative PID targets only that group.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    use super::*;
    use crate::fusion::{
        BoundaryCandidateRole, BoundaryEvidenceKind, HardBoundaryV1, TechniqueScores, TimeRange,
    };

    fn candidate(id: &str, start_seconds: f64, end_seconds: f64) -> SegmentCandidate {
        SegmentCandidate {
            id: id.to_string(),
            range: TimeRange::from_seconds(start_seconds, end_seconds).unwrap(),
            target_midi: 60,
            boundary_source: "game".to_string(),
            boundary_kind: BoundaryEvidenceKind::Game,
            boundary_role: BoundaryCandidateRole::Primary,
            boundary_fractional_midi: Some(60.0),
            boundary_decision_parameter: Some(0.2),
            presence_decision_parameter: Some(0.2),
            boundary_hard: false,
            boundary_support: None,
            boundary_calibrated_confidence: None,
            target_pitch_source: "game".to_string(),
            target_pitch_source_local_score: None,
            target_pitch_calibrated_confidence: None,
            center_pitch_hz: 261.6,
            rmvpe_center_hz: None,
            rmvpe_confidence: None,
            rmvpe_cents_difference: None,
            rmvpe_voiced_ratio: None,
            rmvpe_pitch_mad_cents: None,
            fcpe_center_hz: None,
            fcpe_observed_ratio: None,
            fcpe_pitch_mad_cents: None,
            fcpe_cents_from_rmvpe: None,
            fcpe_supports_rmvpe: None,
            acoustic: None,
            basic_pitch: None,
            boundary_alternatives: Vec::new(),
            boundary_constraints: Vec::new(),
            technique_evidence: Vec::new(),
            techniques: TechniqueScores::default(),
            word_id: Some(id.to_string()),
            alternatives: Vec::new(),
        }
    }

    fn script_executable(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        let staging = dir.join(format!("{name}.part"));
        {
            let mut file = std::fs::File::create(&staging).unwrap();
            file.write_all(body.as_bytes()).unwrap();
            file.sync_all().unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::rename(staging, &path).unwrap();
        path
    }

    #[test]
    fn agent_selection_round_trips_verbatim_candidates() {
        let candidates = vec![candidate("a", 0.0, 1.0), candidate("b", 1.0, 2.0)];
        let response = serde_json::json!({
            "contract": AGENT_RESPONSE_CONTRACT,
            "version": AGENT_PROTOCOL_VERSION,
            "selected": [serde_json::to_value(&candidates[0]).unwrap()],
        });
        let dir = tempdir();
        let script = script_executable(
            dir.path(),
            "agent.sh",
            &format!("#!/bin/sh\ncat > /dev/null\ncat <<'EOF'\n{response}\nEOF\n"),
        );
        let cancellation = CancellationToken::default();
        let decision =
            run_fusion_agent(&script, &candidates, Duration::from_secs(10), &cancellation)
                .expect("well-formed verbatim selection must succeed");
        assert_eq!(decision.selected, vec![candidates[0].clone()]);
        assert_eq!(decision.candidate_set_digest.len(), 64);
        assert_eq!(decision.response_digest.len(), 64);
    }

    #[cfg(unix)]
    fn unix_process_is_running(pid: i32) -> bool {
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| stat.rsplit_once(") ").map(|(_, tail)| tail.to_string()))
            .and_then(|tail| tail.chars().next());
        if state == Some('Z') {
            return false;
        }
        // SAFETY: signal 0 only probes process existence/permission.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    #[cfg(unix)]
    fn successful_agent_exit_kills_descendants_that_inherit_protocol_pipes() {
        let candidates = vec![candidate("a", 0.0, 1.0)];
        let response = serde_json::json!({
            "contract": AGENT_RESPONSE_CONTRACT,
            "version": AGENT_PROTOCOL_VERSION,
            "selected": [serde_json::to_value(&candidates[0]).unwrap()],
        });
        let dir = tempdir();
        let pid_path = dir.path().join("descendant.pid");
        let script = script_executable(
            dir.path(),
            "agent-with-descendant.sh",
            &format!(
                "#!/bin/sh\ncat > /dev/null\nsleep 30 &\necho $! > '{}'\ncat <<'EOF'\n{response}\nEOF\n",
                pid_path.display()
            ),
        );
        let started = Instant::now();
        let decision = run_fusion_agent(
            &script,
            &candidates,
            Duration::from_secs(5),
            &CancellationToken::default(),
        )
        .expect("a successful direct adapter must not wait for a detached descendant");
        assert_eq!(decision.selected, candidates);
        assert!(started.elapsed() < Duration::from_secs(2));
        let descendant_pid = std::fs::read_to_string(&pid_path)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while unix_process_is_running(descendant_pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!unix_process_is_running(descendant_pid));
    }

    #[test]
    fn agent_request_and_digest_include_exact_pool_hard_boundaries() {
        let candidates = vec![candidate("a", 0.0, 0.5), candidate("b", 0.5, 1.0)];
        let pool = SingingFusionEvidence {
            schema_version: 2,
            candidates: candidates.clone(),
            hard_boundaries: HardBoundarySetV1 {
                boundaries: vec![HardBoundaryV1 {
                    source: "caller".to_string(),
                    level: crate::BoundaryLevel::Word,
                    range: TimeRange::from_seconds(0.5, 1.0).unwrap(),
                }],
            },
        };
        let response = serde_json::json!({
            "contract": AGENT_RESPONSE_CONTRACT,
            "version": AGENT_PROTOCOL_VERSION,
            "selected": [serde_json::to_value(&candidates[0]).unwrap()],
        });
        let dir = tempdir();
        let request_path = dir.path().join("request.json");
        let script = script_executable(
            dir.path(),
            "agent-with-boundaries.sh",
            &format!(
                "#!/bin/sh\ncat > '{}'\ncat <<'EOF'\n{response}\nEOF\n",
                request_path.display()
            ),
        );
        let decision = run_fusion_agent_for_pool(
            &script,
            &pool,
            Duration::from_secs(10),
            &CancellationToken::default(),
        )
        .unwrap();
        let request: serde_json::Value =
            serde_json::from_slice(&std::fs::read(request_path).unwrap()).unwrap();
        assert_eq!(request["version"], AGENT_PROTOCOL_VERSION);
        assert_eq!(
            request["hard_boundaries"]["boundaries"][0]["source"],
            "caller"
        );
        assert_eq!(
            decision.candidate_set_digest,
            candidate_set_digest(&pool).unwrap()
        );

        let candidate_only_pool = SingingFusionEvidence {
            schema_version: 2,
            candidates,
            hard_boundaries: HardBoundarySetV1::default(),
        };
        assert_ne!(
            candidate_set_digest(&pool).unwrap(),
            candidate_set_digest(&candidate_only_pool).unwrap()
        );
    }

    fn assert_adapter_error_context(error: &EngineError) {
        assert_eq!(error.capability.as_deref(), Some(FUSION_CAPABILITY));
        assert_eq!(error.resource.as_deref(), Some(FUSION_ADAPTER_RESOURCE));
    }

    #[test]
    fn fabricated_candidate_is_rejected() {
        let candidates = vec![candidate("a", 0.0, 1.0)];
        let mut fabricated = candidates[0].clone();
        fabricated.id = "fabricated".to_string();
        fabricated.center_pitch_hz = 999.0;
        let response = serde_json::json!({
            "contract": AGENT_RESPONSE_CONTRACT,
            "version": AGENT_PROTOCOL_VERSION,
            "selected": [serde_json::to_value(&fabricated).unwrap()],
        });
        let dir = tempdir();
        let script = script_executable(
            dir.path(),
            "agent.sh",
            &format!("#!/bin/sh\ncat > /dev/null\ncat <<'EOF'\n{response}\nEOF\n"),
        );
        let cancellation = CancellationToken::default();
        let error = run_fusion_agent(&script, &candidates, Duration::from_secs(10), &cancellation)
            .expect_err("fabricated candidate must be rejected");
        assert_eq!(error.code, EngineErrorCode::OutputValidationFailed);
        assert_adapter_error_context(&error);
    }

    #[test]
    fn nonzero_exit_is_reported_as_worker_failed() {
        let candidates = vec![candidate("a", 0.0, 1.0)];
        let dir = tempdir();
        let script = script_executable(
            dir.path(),
            "agent.sh",
            "#!/bin/sh\ncat > /dev/null\necho 'boom' 1>&2\nexit 3\n",
        );
        let cancellation = CancellationToken::default();
        let error = run_fusion_agent(&script, &candidates, Duration::from_secs(10), &cancellation)
            .expect_err("non-zero exit must fail closed");
        assert_eq!(error.code, EngineErrorCode::WorkerFailed);
        assert!(!error.message.contains("boom"));
        assert!(error.message.contains("diagnostics were not retained"));
        assert_adapter_error_context(&error);
    }

    #[test]
    fn malformed_provider_output_is_not_retained_in_the_error() {
        let candidates = vec![candidate("a", 0.0, 1.0)];
        let dir = tempdir();
        let script = script_executable(
            dir.path(),
            "agent.sh",
            "#!/bin/sh\ncat > /dev/null\nprintf 'provider-secret-chain-of-thought'\n",
        );
        let cancellation = CancellationToken::default();
        let error = run_fusion_agent(&script, &candidates, Duration::from_secs(10), &cancellation)
            .expect_err("malformed provider output must fail closed");
        assert_eq!(error.code, EngineErrorCode::WorkerProtocolMismatch);
        assert!(!error.message.contains("provider-secret-chain-of-thought"));
        assert_adapter_error_context(&error);
    }

    #[cfg(windows)]
    fn windows_process_is_alive(pid: u32) -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: the queried handle is closed on every successful open and
        // the exit-code pointer refers to a live local `u32`.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let mut exit_code = 0_u32;
            let queried = GetExitCodeProcess(process, &mut exit_code) != 0;
            CloseHandle(process);
            queried && exit_code == STILL_ACTIVE as u32
        }
    }

    #[cfg(windows)]
    #[test]
    fn job_object_close_terminates_adapter_descendants() {
        let dir = tempdir();
        let pid_path = dir.path().join("descendant.pid");
        let escaped_pid_path = pid_path.to_string_lossy().replace('\'', "''");
        let script = dir.path().join("spawn-descendant.ps1");
        std::fs::write(
            &script,
            format!(
                "$child = Start-Process -FilePath \"$env:SystemRoot\\System32\\ping.exe\" -ArgumentList \"-t\",\"127.0.0.1\" -PassThru -WindowStyle Hidden\nSet-Content -LiteralPath '{escaped_pid_path}' -Value $child.Id\nWait-Process -Id $child.Id\n"
            ),
        )
        .unwrap();
        let mut parent = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let job = ProcessTreeGuard::attach(&parent).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !pid_path.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let descendant_pid = std::fs::read_to_string(&pid_path)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert!(windows_process_is_alive(descendant_pid));

        drop(job);
        let _ = parent.wait();
        let deadline = Instant::now() + Duration::from_secs(5);
        while windows_process_is_alive(descendant_pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!windows_process_is_alive(descendant_pid));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_is_typed_and_keeps_adapter_context() {
        let candidates = vec![candidate("a", 0.0, 1.0)];
        let dir = tempdir();
        let script = script_executable(
            dir.path(),
            "agent.sh",
            "#!/bin/sh\ncat > /dev/null\nsleep 2\n",
        );
        let cancellation = CancellationToken::default();
        let error = run_fusion_agent(
            &script,
            &candidates,
            Duration::from_millis(20),
            &cancellation,
        )
        .expect_err("adapter timeout must fail closed");
        assert_eq!(error.code, EngineErrorCode::WorkerTimeout);
        assert_adapter_error_context(&error);
    }

    #[cfg(unix)]
    #[test]
    fn active_cancellation_terminates_the_adapter_and_keeps_context() {
        let candidates = vec![candidate("a", 0.0, 1.0)];
        let dir = tempdir();
        let script = script_executable(
            dir.path(),
            "agent.sh",
            "#!/bin/sh\ncat > /dev/null\nsleep 10\n",
        );
        let cancellation = CancellationToken::default();
        let canceller = cancellation.clone();
        let cancel_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            canceller.cancel();
        });
        let error = run_fusion_agent(&script, &candidates, Duration::from_secs(10), &cancellation)
            .expect_err("active cancellation must fail closed");
        cancel_thread.join().unwrap();
        assert_eq!(error.code, EngineErrorCode::Cancelled);
        assert_adapter_error_context(&error);
    }

    fn backpressured_candidates() -> Vec<SegmentCandidate> {
        let mut value = candidate("large", 0.0, 1.0);
        value.boundary_source = "x".repeat(2 * 1024 * 1024);
        vec![value]
    }

    #[cfg(unix)]
    #[test]
    fn backpressured_request_write_obeys_timeout() {
        let candidates = backpressured_candidates();
        let dir = tempdir();
        let script = script_executable(dir.path(), "agent.sh", "#!/bin/sh\nsleep 10\n");
        let cancellation = CancellationToken::default();
        let error = run_fusion_agent(
            &script,
            &candidates,
            Duration::from_millis(40),
            &cancellation,
        )
        .expect_err("a blocked stdin writer must remain under the deadline");
        assert_eq!(error.code, EngineErrorCode::WorkerTimeout);
        assert_adapter_error_context(&error);
    }

    #[cfg(unix)]
    #[test]
    fn backpressured_request_write_obeys_active_cancellation() {
        let candidates = backpressured_candidates();
        let dir = tempdir();
        let script = script_executable(dir.path(), "agent.sh", "#!/bin/sh\nsleep 10\n");
        let cancellation = CancellationToken::default();
        let canceller = cancellation.clone();
        let cancel_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            canceller.cancel();
        });
        let error = run_fusion_agent(&script, &candidates, Duration::from_secs(10), &cancellation)
            .expect_err("a blocked stdin writer must remain cancellable");
        cancel_thread.join().unwrap();
        assert_eq!(error.code, EngineErrorCode::Cancelled);
        assert_adapter_error_context(&error);
    }

    #[cfg(unix)]
    #[test]
    fn oversized_request_is_rejected_before_the_adapter_is_spawned() {
        let mut value = candidate("oversized", 0.0, 1.0);
        value.boundary_source = "x".repeat(MAX_AGENT_REQUEST_BYTES + 1);
        let candidates = vec![value];
        let dir = tempdir();
        let marker = dir.path().join("spawned");
        let script = script_executable(
            dir.path(),
            "agent.sh",
            &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        );
        let cancellation = CancellationToken::default();
        let error = run_fusion_agent(&script, &candidates, Duration::from_secs(10), &cancellation)
            .expect_err("an oversized request must fail before spawn");
        assert_eq!(error.code, EngineErrorCode::OutputValidationFailed);
        assert!(error.message.contains("bounded protocol limit"));
        assert!(!marker.exists());
        assert_adapter_error_context(&error);
    }

    #[cfg(unix)]
    #[test]
    fn oversized_response_is_rejected_without_retaining_provider_output() {
        let candidates = vec![candidate("a", 0.0, 1.0)];
        let dir = tempdir();
        let script = script_executable(
            dir.path(),
            "agent.sh",
            "#!/bin/sh\ncat > /dev/null\nhead -c 8388609 /dev/zero\n",
        );
        let cancellation = CancellationToken::default();
        let error = run_fusion_agent(&script, &candidates, Duration::from_secs(10), &cancellation)
            .expect_err("oversized provider output must fail closed");
        assert_eq!(error.code, EngineErrorCode::WorkerProtocolMismatch);
        assert!(error.message.contains("bounded protocol limit"));
        assert_adapter_error_context(&error);
    }

    #[test]
    fn missing_executable_fails_closed_without_env_fallback() {
        let candidates = vec![candidate("a", 0.0, 1.0)];
        let cancellation = CancellationToken::default();
        let missing = PathBuf::from("/nonexistent/uta-fusion-agent-test-binary");
        let error = run_fusion_agent(&missing, &candidates, Duration::from_secs(1), &cancellation)
            .expect_err("missing executable must fail closed");
        assert_eq!(error.code, EngineErrorCode::WorkerUnavailable);
        assert_adapter_error_context(&error);
    }

    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        let mut base = std::env::temp_dir();
        base.push(format!(
            "uta-agent-client-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&base).unwrap();
        TempDir(base)
    }
}
