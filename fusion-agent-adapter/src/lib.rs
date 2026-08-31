//! Native provider adapters for Uta! Studio's bounded Fusion Agent protocol.
//!
//! Runtime Manager owns which adapter executable is selected. This crate owns
//! only the provider-specific process boundary: requests contain candidate
//! metadata and responses are normalized to exact v4 candidate indices.
//! Analysis Engine maps those indices back to its immutable typed candidates. No source
//! audio, project files, or library data are opened here.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Map, Value};

pub const FUSION_AGENT_REQUEST_CONTRACT: &str = "uta.fusion_agent_request";
pub const FUSION_AGENT_RESPONSE_CONTRACT: &str = "uta.fusion_agent_response";
pub const FUSION_AGENT_PROTOCOL_VERSION: u32 = 4;
pub const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PROVIDER_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_PROVIDER_DIAGNOSTICS_BYTES: usize = 64 * 1024;

/// The provider integrations that Runtime Manager can discover and select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provider {
    Pi,
    Codex,
    Claude,
}

impl Provider {
    pub const ALL: [Self; 3] = [Self::Pi, Self::Codex, Self::Claude];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Pi => "Pi",
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
        }
    }

    pub const fn executable_name(self) -> &'static str {
        self.id()
    }

    pub const fn adapter_executable_name(self) -> &'static str {
        match self {
            Self::Pi => "uta-fusion-agent-pi",
            Self::Codex => "uta-fusion-agent-codex",
            Self::Claude => "uta-fusion-agent-claude",
        }
    }

    pub const fn adapter_manifest_name(self) -> &'static str {
        match self {
            Self::Pi => "uta-fusion-agent-pi.uta-fusion-adapter.json",
            Self::Codex => "uta-fusion-agent-codex.uta-fusion-adapter.json",
            Self::Claude => "uta-fusion-agent-claude.uta-fusion-adapter.json",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AdapterError> {
        match value {
            "pi" => Ok(Self::Pi),
            "codex" => Ok(Self::Codex),
            "claude" | "claude_code" | "claude-code" => Ok(Self::Claude),
            other => Err(AdapterError::InvalidProvider(other.to_string())),
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.id())
    }
}

/// Errors intentionally contain no provider stderr. Provider output may carry
/// credentials or other sensitive diagnostics; the adapter only exposes a
/// short typed failure on its own stderr.
#[derive(Debug)]
pub enum AdapterError {
    InvalidRequest(String),
    InvalidProvider(String),
    ProviderUnavailable(String),
    ProviderFailed(String),
    ProviderOutputTooLarge,
    InvalidProviderResponse(String),
    Io(String),
}

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(formatter, "invalid fusion request: {message}"),
            Self::InvalidProvider(provider) => {
                write!(formatter, "unsupported provider: {provider}")
            }
            Self::ProviderUnavailable(provider) => {
                write!(formatter, "provider CLI is unavailable on PATH: {provider}")
            }
            Self::ProviderFailed(provider) => write!(formatter, "provider CLI failed: {provider}"),
            Self::ProviderOutputTooLarge => {
                formatter.write_str("provider output exceeded the bounded protocol limit")
            }
            Self::InvalidProviderResponse(message) => {
                write!(
                    formatter,
                    "provider response is not a valid fusion selection: {message}"
                )
            }
            Self::Io(message) => write!(formatter, "fusion adapter I/O failed: {message}"),
        }
    }
}

impl std::error::Error for AdapterError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FusionRequest {
    contract: String,
    version: u32,
    #[serde(default)]
    instructions: String,
    hard_boundaries: Value,
    lyrics: Value,
    candidate_set_digest: String,
    string_table: Vec<String>,
    option_fields: Vec<String>,
    segments: Vec<Value>,
}

impl FusionRequest {
    fn validate(self) -> Result<Self, AdapterError> {
        if self.contract != FUSION_AGENT_REQUEST_CONTRACT
            || self.version != FUSION_AGENT_PROTOCOL_VERSION
        {
            return Err(AdapterError::InvalidRequest(
                "unsupported contract or protocol version".to_string(),
            ));
        }
        if !self.hard_boundaries.is_object() {
            return Err(AdapterError::InvalidRequest(
                "hard_boundaries must be an object".to_string(),
            ));
        }
        if !self.lyrics.is_object() {
            return Err(AdapterError::InvalidRequest(
                "lyrics must be an object".to_string(),
            ));
        }
        if self.candidate_set_digest.trim().is_empty() || self.segments.is_empty() {
            return Err(AdapterError::InvalidRequest(
                "candidate decision projection is empty".to_string(),
            ));
        }
        let mut field_names = std::collections::BTreeSet::new();
        for field in &self.option_fields {
            if field.trim().is_empty() || field.len() > 128 || !field_names.insert(field.as_str()) {
                return Err(AdapterError::InvalidRequest(
                    "option_fields must be non-empty, bounded, and unique".to_string(),
                ));
            }
        }
        if !field_names.contains("candidate_index") {
            return Err(AdapterError::InvalidRequest(
                "option_fields omitted candidate_index".to_string(),
            ));
        }
        if self
            .string_table
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err(AdapterError::InvalidRequest(
                "string_table contains an invalid value".to_string(),
            ));
        }
        let _ = self.known_candidate_indices()?;
        Ok(self)
    }

    fn known_candidate_indices(&self) -> Result<std::collections::BTreeSet<usize>, AdapterError> {
        let candidate_index_column = self
            .option_fields
            .iter()
            .position(|field| field == "candidate_index")
            .ok_or_else(|| {
                AdapterError::InvalidRequest("option_fields omitted candidate_index".to_string())
            })?;
        let string_columns = [
            "boundary_source",
            "boundary_kind",
            "boundary_role",
            "pitch_source",
        ]
        .into_iter()
        .filter_map(|field| {
            self.option_fields
                .iter()
                .position(|candidate| candidate == field)
        })
        .collect::<Vec<_>>();
        let mut known = std::collections::BTreeSet::new();
        for segment in &self.segments {
            let segment = segment.as_array().ok_or_else(|| {
                AdapterError::InvalidRequest("every segment must be an array".to_string())
            })?;
            if segment.len() != 3 {
                return Err(AdapterError::InvalidRequest(
                    "every segment must contain start, end, and options".to_string(),
                ));
            }
            let start = segment[0].as_u64().ok_or_else(|| {
                AdapterError::InvalidRequest("segment start must be unsigned".to_string())
            })?;
            let end = segment[1].as_u64().ok_or_else(|| {
                AdapterError::InvalidRequest("segment end must be unsigned".to_string())
            })?;
            if end <= start {
                return Err(AdapterError::InvalidRequest(
                    "segment range must be non-empty".to_string(),
                ));
            }
            let options = segment[2].as_array().ok_or_else(|| {
                AdapterError::InvalidRequest("segment options must be an array".to_string())
            })?;
            if options.is_empty() {
                return Err(AdapterError::InvalidRequest(
                    "segment options must not be empty".to_string(),
                ));
            }
            for option in options {
                let option = option.as_array().ok_or_else(|| {
                    AdapterError::InvalidRequest("candidate option must be an array".to_string())
                })?;
                if option.len() != self.option_fields.len() {
                    return Err(AdapterError::InvalidRequest(
                        "candidate option width does not match option_fields".to_string(),
                    ));
                }
                let index = option[candidate_index_column]
                    .as_u64()
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| {
                        AdapterError::InvalidRequest(
                            "candidate_index must be an unsigned integer".to_string(),
                        )
                    })?;
                if !known.insert(index) {
                    return Err(AdapterError::InvalidRequest(
                        "candidate_index values must be unique".to_string(),
                    ));
                }
                for column in &string_columns {
                    let reference = option[*column]
                        .as_u64()
                        .and_then(|index| usize::try_from(index).ok())
                        .ok_or_else(|| {
                            AdapterError::InvalidRequest(
                                "candidate string reference must be unsigned".to_string(),
                            )
                        })?;
                    if reference >= self.string_table.len() {
                        return Err(AdapterError::InvalidRequest(
                            "candidate string reference is out of range".to_string(),
                        ));
                    }
                }
            }
        }
        Ok(known)
    }
}

/// Run one fixed provider adapter. The provider executable is discovered via
/// the normal `PATH`; no shell and no raw provider path are accepted from the
/// Studio protocol.
pub fn run(provider: Provider) -> Result<(), AdapterError> {
    let request = read_bounded_stdin()?;
    let response =
        run_request_with_search_path(provider, &request, std::env::var_os("PATH").as_deref())?;
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&response)
        .map_err(|error| AdapterError::Io(error.to_string()))?;
    stdout
        .write_all(b"\n")
        .map_err(|error| AdapterError::Io(error.to_string()))?;
    stdout
        .flush()
        .map_err(|error| AdapterError::Io(error.to_string()))
}

/// Process a request with an explicitly supplied PATH value. Runtime uses
/// [`run`] so the provider is resolved from the process environment; this
/// variant is also useful to embedders that already own a bounded environment
/// and to isolated fake-CLI tests.
pub fn run_request_with_search_path(
    provider: Provider,
    request: &[u8],
    search_path: Option<&OsStr>,
) -> Result<Vec<u8>, AdapterError> {
    if request.len() > MAX_REQUEST_BYTES {
        return Err(AdapterError::InvalidRequest(
            "request exceeded the bounded protocol limit".to_string(),
        ));
    }
    let request: FusionRequest = serde_json::from_slice::<FusionRequest>(&request)
        .map_err(|error| AdapterError::InvalidRequest(format!("request JSON: {error}")))?
        .validate()?;
    let prompt = build_prompt(&request)?;
    let provider_output =
        invoke_provider_with_search_path(provider, &prompt, &request, search_path)?;
    let response = normalize_provider_response(&request, &provider_output)?;
    serde_json::to_vec(&response)
        .map_err(|error| AdapterError::Io(format!("response JSON: {error}")))
}

/// Main entry used by all four native binaries. The generic binary chooses the
/// first discovered provider in deterministic order; provider-specific binaries
/// are what Runtime Manager presents for explicit selection.
pub fn main_entry(provider: Option<Provider>) -> std::process::ExitCode {
    let provider = provider.or_else(|| {
        Provider::ALL
            .into_iter()
            .find(|candidate| discover_provider(*candidate).is_some())
    });
    let Some(provider) = provider else {
        eprintln!("uta-fusion-agent-adapter: no supported provider CLI was found on PATH");
        return std::process::ExitCode::from(2);
    };
    match run(provider) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("uta-fusion-agent-{provider}: {error}");
            std::process::ExitCode::from(2)
        }
    }
}

fn read_bounded_stdin() -> Result<Vec<u8>, AdapterError> {
    let mut bytes = Vec::new();
    let mut stdin = io::stdin().lock();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = stdin
            .read(&mut chunk)
            .map_err(|error| AdapterError::Io(error.to_string()))?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > MAX_REQUEST_BYTES {
            return Err(AdapterError::InvalidRequest(
                "request exceeded the bounded protocol limit".to_string(),
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    if bytes.is_empty() {
        return Err(AdapterError::InvalidRequest("request is empty".to_string()));
    }
    Ok(bytes)
}

fn build_prompt(request: &FusionRequest) -> Result<String, AdapterError> {
    let prompt = format!(
        "You are the Uta! Studio Fusion Agent selector. {instructions}\n\n\
         Read only ./candidates.json, ./lyrics.json, and\
         ./hard_boundaries.json from this temporary directory. Do not inspect\
         any other path and do not create or modify files. Return\
         exactly one JSON object with contract \"{response_contract}\", version\
         {version}, and selected as an array of candidate_index integers.\
         The selected indices\
         must be a valid ordered, non-overlapping path that covers represented\
         voiced components and does not cross hard-boundary edges. Do not return\
         markdown, commentary, or chain-of-thought.\n",
        instructions = if request.instructions.trim().is_empty() {
            "Choose the best valid final path."
        } else {
            request.instructions.trim()
        },
        response_contract = FUSION_AGENT_RESPONSE_CONTRACT,
        version = FUSION_AGENT_PROTOCOL_VERSION,
    );
    if prompt.len() > MAX_REQUEST_BYTES {
        return Err(AdapterError::InvalidRequest(
            "provider prompt exceeded the bounded protocol limit".to_string(),
        ));
    }
    Ok(prompt)
}

fn invoke_provider_with_search_path(
    provider: Provider,
    prompt: &str,
    request: &FusionRequest,
    search_path: Option<&OsStr>,
) -> Result<Vec<u8>, AdapterError> {
    let executable = search_path
        .and_then(|path| discover_provider_in_path(provider, path))
        .ok_or_else(|| AdapterError::ProviderUnavailable(provider.to_string()))?;
    invoke_provider_executable(provider, prompt, request, &executable)
}

fn invoke_provider_executable(
    provider: Provider,
    prompt: &str,
    request: &FusionRequest,
    executable: &Path,
) -> Result<Vec<u8>, AdapterError> {
    let workspace = isolated_workspace(provider)?;
    write_provider_inputs(workspace.path(), request)?;
    let mut command = Command::new(&executable);
    command
        .args(provider_command_args(provider, workspace.path()))
        .current_dir(workspace.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| AdapterError::ProviderUnavailable(format!("{provider}: {error}")))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| AdapterError::Io("provider stdin was unavailable".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AdapterError::Io("provider stdout was unavailable".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AdapterError::Io("provider stderr was unavailable".to_string()))?;
    let prompt = prompt.as_bytes().to_vec();
    let stdin_thread = std::thread::spawn(move || {
        let mut stdin = stdin;
        let result = stdin
            .write_all(&prompt)
            .and_then(|()| stdin.flush())
            .map_err(|error| error.to_string());
        drop(stdin);
        result
    });
    let stdout_thread =
        std::thread::spawn(move || read_pipe_bounded(stdout, MAX_PROVIDER_OUTPUT_BYTES));
    let stderr_thread =
        std::thread::spawn(move || read_pipe_bounded(stderr, MAX_PROVIDER_DIAGNOSTICS_BYTES));
    let status = child
        .wait()
        .map_err(|error| AdapterError::Io(error.to_string()))?;
    let stdin_result = stdin_thread
        .join()
        .map_err(|_| AdapterError::Io("provider request writer panicked".to_string()))?;
    let stdout_result = stdout_thread
        .join()
        .map_err(|_| AdapterError::Io("provider output reader panicked".to_string()))?;
    let _stderr_result = stderr_thread
        .join()
        .map_err(|_| AdapterError::Io("provider diagnostics reader panicked".to_string()))?;
    if let Err(error) = stdin_result {
        // A provider is allowed to stop reading once it has an answer. The
        // process status and bounded stdout remain authoritative.
        if !status.success() {
            return Err(AdapterError::ProviderFailed(format!(
                "{provider} ({error})"
            )));
        }
    }
    if !status.success() {
        return Err(AdapterError::ProviderFailed(format!(
            "{provider} ({})",
            exit_status_label(status)
        )));
    }
    let output = stdout_result?;
    if output.len() > MAX_PROVIDER_OUTPUT_BYTES {
        return Err(AdapterError::ProviderOutputTooLarge);
    }
    Ok(output)
}

fn write_provider_inputs(workspace: &Path, request: &FusionRequest) -> Result<(), AdapterError> {
    let candidates = serde_json::to_vec(&serde_json::json!({
        "candidate_set_digest": request.candidate_set_digest,
        "string_table": request.string_table,
        "option_fields": request.option_fields,
        "segments": request.segments,
    }))
    .map_err(|error| AdapterError::Io(format!("candidate projection JSON: {error}")))?;
    let lyrics = serde_json::to_vec(&request.lyrics)
        .map_err(|error| AdapterError::Io(format!("lyrics JSON: {error}")))?;
    let hard_boundaries = serde_json::to_vec(&request.hard_boundaries)
        .map_err(|error| AdapterError::Io(format!("hard-boundary JSON: {error}")))?;
    let total = candidates
        .len()
        .saturating_add(lyrics.len())
        .saturating_add(hard_boundaries.len());
    if total > MAX_REQUEST_BYTES {
        return Err(AdapterError::InvalidRequest(
            "provider input files exceeded the bounded protocol limit".to_string(),
        ));
    }
    for (name, bytes) in [
        ("candidates.json", candidates),
        ("lyrics.json", lyrics),
        ("hard_boundaries.json", hard_boundaries),
    ] {
        std::fs::write(workspace.join(name), bytes)
            .map_err(|error| AdapterError::Io(format!("could not write {name}: {error}")))?;
    }
    Ok(())
}

fn read_pipe_bounded<R: Read>(mut reader: R, limit: usize) -> Result<Vec<u8>, AdapterError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = reader
            .read(&mut chunk)
            .map_err(|error| AdapterError::Io(error.to_string()))?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(count) > limit {
            // Continue draining the pipe before returning so the provider can
            // exit cleanly and no descendant retains an inherited descriptor.
            let mut discarded = [0_u8; 8192];
            while reader.read(&mut discarded).unwrap_or(0) != 0 {}
            return Err(if limit == MAX_PROVIDER_OUTPUT_BYTES {
                AdapterError::ProviderOutputTooLarge
            } else {
                AdapterError::Io("provider diagnostics exceeded the local bound".to_string())
            });
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn exit_status_label(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "terminated by signal".to_string(),
        |code| format!("exit {code}"),
    )
}

struct TemporaryWorkspace(PathBuf);

impl TemporaryWorkspace {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn isolated_workspace(provider: Provider) -> Result<TemporaryWorkspace, AdapterError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "uta-fusion-agent-{}-{}-{}",
        provider.id(),
        std::process::id(),
        timestamp
    ));
    std::fs::create_dir(&path).map_err(|error| AdapterError::Io(error.to_string()))?;
    Ok(TemporaryWorkspace(path))
}

/// Build provider-specific argv without invoking a shell. The prompt is sent
/// through stdin to keep it out of argv and process listings. The compact
/// decision inputs live only in the isolated temporary workspace.
pub fn provider_command_args(provider: Provider, workspace: &Path) -> Vec<OsString> {
    match provider {
        Provider::Pi => [
            "--print",
            "--no-session",
            "--tools",
            "read",
            "--no-extensions",
            "--no-skills",
            "--no-prompt-templates",
            "--no-themes",
            "--no-context-files",
            "--no-approve",
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
        Provider::Codex => [
            "exec",
            "--ephemeral",
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
            "--ignore-rules",
            "--cd",
        ]
        .into_iter()
        .map(OsString::from)
        .chain([workspace.as_os_str().to_os_string(), OsString::from("-")])
        .collect(),
        Provider::Claude => [
            "--print",
            "--output-format",
            "text",
            "--no-session-persistence",
            "--bare",
            "--permission-mode",
            "dontAsk",
            "--tools",
            "Read",
            "--disable-slash-commands",
            "--no-chrome",
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
    }
}

/// Discover one provider through a normal PATH lookup. Runtime Manager uses
/// the same pure helper while keeping the selected adapter path internal.
pub fn discover_provider(provider: Provider) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    discover_provider_in_path(provider, &path)
}

pub fn discover_provider_in_path(provider: Provider, search_path: &OsStr) -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        format!("{}.exe", provider.executable_name())
    } else {
        provider.executable_name().to_string()
    };
    std::env::split_paths(search_path)
        .map(|directory| directory.join(&filename))
        .find(|path| executable_file(path))
}

fn executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    }
    #[cfg(not(any(unix, windows)))]
    {
        path.is_file()
    }
}

fn normalize_provider_response(
    request: &FusionRequest,
    output: &[u8],
) -> Result<Value, AdapterError> {
    let root = extract_protocol_value(output)?;
    let object = root.as_object().ok_or_else(|| {
        AdapterError::InvalidProviderResponse("response must be a JSON object".to_string())
    })?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "contract" | "version" | "selected"))
    {
        return Err(AdapterError::InvalidProviderResponse(
            "response contains unknown fields".to_string(),
        ));
    }
    if object.get("contract").and_then(Value::as_str) != Some(FUSION_AGENT_RESPONSE_CONTRACT)
        || object.get("version").and_then(Value::as_u64)
            != Some(u64::from(FUSION_AGENT_PROTOCOL_VERSION))
    {
        return Err(AdapterError::InvalidProviderResponse(
            "response contract or version is unsupported".to_string(),
        ));
    }
    let selected = object
        .get("selected")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AdapterError::InvalidProviderResponse("selected must be an array".to_string())
        })?;
    if selected.is_empty() {
        return Err(AdapterError::InvalidProviderResponse(
            "selected must not be empty".to_string(),
        ));
    }
    let known = request.known_candidate_indices()?;
    let mut normalized = Vec::with_capacity(selected.len());
    let mut indices = std::collections::BTreeSet::new();
    for entry in selected {
        let index = entry
            .as_u64()
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(|| {
                AdapterError::InvalidProviderResponse(
                    "selected entries must be candidate_index integers".to_string(),
                )
            })?;
        if !known.contains(&index) {
            return Err(AdapterError::InvalidProviderResponse(
                "selected candidate is not in the supplied pool".to_string(),
            ));
        }
        if !indices.insert(index) {
            return Err(AdapterError::InvalidProviderResponse(
                "selected candidate indices must be unique".to_string(),
            ));
        }
        normalized.push(Value::Number(serde_json::Number::from(index as u64)));
    }
    let mut response = Map::new();
    response.insert(
        "contract".to_string(),
        Value::String(FUSION_AGENT_RESPONSE_CONTRACT.to_string()),
    );
    response.insert(
        "version".to_string(),
        Value::Number(serde_json::Number::from(FUSION_AGENT_PROTOCOL_VERSION)),
    );
    response.insert("selected".to_string(), Value::Array(normalized));
    Ok(Value::Object(response))
}

fn extract_protocol_value(output: &[u8]) -> Result<Value, AdapterError> {
    if output.len() > MAX_PROVIDER_OUTPUT_BYTES {
        return Err(AdapterError::ProviderOutputTooLarge);
    }
    let text = std::str::from_utf8(output)
        .map_err(|_| AdapterError::InvalidProviderResponse("output is not UTF-8".to_string()))?;
    let mut candidates = Vec::<&str>::new();
    let mut cursor = 0;
    while let Some(start) = text[cursor..].find("```") {
        let fence_start = cursor + start + 3;
        let content_start = text[fence_start..]
            .find('\n')
            .map(|offset| fence_start + offset + 1)
            .unwrap_or(fence_start);
        let Some(end_offset) = text[content_start..].find("```") else {
            break;
        };
        let end = content_start + end_offset;
        candidates.push(text[content_start..end].trim());
        cursor = end + 3;
    }
    candidates.push(text.trim());
    let mut ranges = balanced_json_ranges(text);
    ranges.reverse();
    for (start, end) in ranges {
        candidates.push(&text[start..end]);
    }
    for candidate in candidates {
        if let Ok(value) = serde_json::from_str::<Value>(candidate.trim()) {
            if value.is_object() {
                return Ok(value);
            }
        }
    }
    Err(AdapterError::InvalidProviderResponse(
        "no JSON response object was found".to_string(),
    ))
}

fn balanced_json_ranges(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut stack = Vec::<(u8, usize)>::new();
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => stack.push((byte, index)),
            b'}' | b']' => {
                let expected = if byte == b'}' { b'{' } else { b'[' };
                if stack.last().is_some_and(|(open, _)| *open == expected) {
                    let (_, start) = stack.pop().expect("checked stack is non-empty");
                    ranges.push((start, index + 1));
                } else {
                    stack.clear();
                }
            }
            _ => {}
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(unix)]
    fn fake_provider(root: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn request() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "contract": FUSION_AGENT_REQUEST_CONTRACT,
            "version": FUSION_AGENT_PROTOCOL_VERSION,
            "instructions": "Choose the coherent path.",
            "hard_boundaries": {"boundaries": []},
            "lyrics": {
                "text": "la la",
                "language": "en",
                "word_fields": ["id", "text", "start", "end", "confidence"],
                "words": [["word-0", "la", 0, 100, 0.9]]
            },
            "candidate_set_digest": "fixture-candidate-set",
            "string_table": ["game", "primary", "rmvpe"],
            "option_fields": [
                "candidate_index", "midi", "boundary_source", "boundary_kind",
                "boundary_role", "hard", "boundary_support", "boundary_confidence",
                "pitch_source", "pitch_support", "pitch_confidence", "context_support",
                "acoustic_periodicity", "acoustic_snr_db", "basic_pitch_onset"
            ],
            "segments": [
                [0, 100, [[0, 60, 0, 0, 1, false, null, null, 2, null, null, 0.5, null, null, null]]],
                [100, 200, [[1, 62, 0, 0, 1, false, null, null, 2, null, null, 0.6, null, null, null]]]
            ]
        }))
        .unwrap()
    }

    #[test]
    fn provider_args_disable_sessions_and_context_for_each_provider() {
        let workspace = Path::new("/tmp/uta-fusion-agent-test");
        let pi = provider_command_args(Provider::Pi, workspace);
        let pi = pi
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(pi.iter().any(|arg| arg == "--no-session"));
        assert!(pi.iter().any(|arg| arg == "--tools"));
        assert!(pi.iter().any(|arg| arg == "read"));
        assert!(pi.iter().any(|arg| arg == "--no-context-files"));
        let codex = provider_command_args(Provider::Codex, workspace);
        let codex = codex
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(codex.iter().any(|arg| arg == "--ephemeral"));
        assert!(codex.iter().any(|arg| arg == "--sandbox"));
        assert!(codex.iter().any(|arg| arg == "read-only"));
        assert!(codex.iter().any(|arg| arg == "--ignore-rules"));
        let claude = provider_command_args(Provider::Claude, workspace);
        let claude = claude
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(claude.iter().any(|arg| arg == "--no-session-persistence"));
        assert!(claude.iter().any(|arg| arg == "--bare"));
        assert!(claude.iter().any(|arg| arg == "--tools"));
        assert!(claude.iter().any(|arg| arg == "Read"));
    }

    #[test]
    fn provider_discovery_uses_path_entries_without_running_a_cli() {
        let path = PathBuf::from("/tmp/uta-fusion-agent-does-not-exist");
        assert!(discover_provider_in_path(Provider::Pi, path.as_os_str()).is_none());
    }

    #[test]
    fn provider_prompt_names_scoped_files_without_embedding_their_contents() {
        let parsed: FusionRequest = serde_json::from_slice::<FusionRequest>(&request())
            .unwrap()
            .validate()
            .unwrap();
        let prompt = build_prompt(&parsed).unwrap();
        for name in ["candidates.json", "lyrics.json", "hard_boundaries.json"] {
            assert!(prompt.contains(name));
        }
        assert!(!prompt.contains("fixture-candidate-set"));
        assert!(!prompt.contains("la la"));
    }

    #[test]
    fn markdown_fence_and_index_selection_are_normalized() {
        let request = String::from_utf8(request()).unwrap();
        let parsed: FusionRequest = serde_json::from_str::<FusionRequest>(&request)
            .unwrap()
            .validate()
            .unwrap();
        let output = br#"provider note
```json
{"contract":"uta.fusion_agent_response","version":4,"selected":[1]}
```
"#;
        let value = normalize_provider_response(&parsed, output).unwrap();
        assert_eq!(value["selected"][0], 1);
    }

    #[test]
    fn unknown_candidate_index_is_rejected() {
        let parsed: FusionRequest = serde_json::from_slice::<FusionRequest>(&request())
            .unwrap()
            .validate()
            .unwrap();
        let output = serde_json::to_vec(&json!({
            "contract": FUSION_AGENT_RESPONSE_CONTRACT,
            "version": FUSION_AGENT_PROTOCOL_VERSION,
            "selected": [99]
        }))
        .unwrap();
        assert!(matches!(
            normalize_provider_response(&parsed, &output),
            Err(AdapterError::InvalidProviderResponse(message)) if message.contains("supplied pool")
        ));
    }

    #[test]
    fn invalid_request_and_oversized_provider_output_fail_closed() {
        let error =
            serde_json::from_slice::<FusionRequest>(br#"{"contract":"wrong"}"#).unwrap_err();
        assert!(error.to_string().contains("missing field"));
        assert!(matches!(
            extract_protocol_value(&vec![b'x'; MAX_PROVIDER_OUTPUT_BYTES + 1]),
            Err(AdapterError::ProviderOutputTooLarge)
        ));
    }

    #[test]
    fn balanced_extraction_ignores_unrelated_provider_prose() {
        let output = br#"thinking: no files
{"contract":"uta.fusion_agent_response","version":4,"selected":[]}
"#;
        let value = extract_protocol_value(output).unwrap();
        assert_eq!(value["contract"], FUSION_AGENT_RESPONSE_CONTRACT);
    }

    #[cfg(unix)]
    #[test]
    fn fake_provider_reads_scoped_files_and_returns_v4_indices() {
        let root = std::env::temp_dir().join(format!(
            "uta-fusion-agent-fake-provider-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let _provider = fake_provider(
            &root,
            "pi",
            "read prompt\ntest -s candidates.json || exit 8\ntest -s lyrics.json || exit 8\ntest -s hard_boundaries.json || exit 8\nprintf '%s\\n' '```json' '{\"contract\":\"uta.fusion_agent_response\",\"version\":4,\"selected\":[0]}' '```'",
        );
        let response =
            run_request_with_search_path(Provider::Pi, &request(), Some(root.as_os_str())).unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(value["contract"], FUSION_AGENT_RESPONSE_CONTRACT);
        assert_eq!(value["version"], FUSION_AGENT_PROTOCOL_VERSION);
        assert_eq!(value["selected"][0], 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn fake_provider_nonzero_exit_and_missing_provider_are_failures() {
        let root = std::env::temp_dir().join(format!(
            "uta-fusion-agent-fake-failure-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let workspace_marker = root.join("provider-workspace");
        let _provider = fake_provider(
            &root,
            "codex",
            &format!("pwd > '{}'\nexit 9", workspace_marker.display()),
        );
        assert!(matches!(
            run_request_with_search_path(Provider::Codex, &request(), Some(root.as_os_str())),
            Err(AdapterError::ProviderFailed(_))
        ));
        let workspace = std::fs::read_to_string(&workspace_marker).unwrap();
        assert!(
            !Path::new(workspace.trim()).exists(),
            "failed provider workspace was not removed"
        );
        assert!(matches!(
            run_request_with_search_path(Provider::Claude, &request(), Some(root.as_os_str())),
            Err(AdapterError::ProviderUnavailable(_))
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}
