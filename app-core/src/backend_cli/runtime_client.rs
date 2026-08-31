use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};

use serde::de::DeserializeOwned;

use super::error::BackendCliError;
use super::process::{
    discover_executable, native_command, read_machine_frame, spawn_stderr_drain, stderr_text,
};
use super::runtime_wire::*;

#[derive(Debug, Clone)]
pub struct RuntimeCliClient {
    executable: PathBuf,
    store: Option<PathBuf>,
    policy: RuntimePolicyWireV1,
}

impl RuntimeCliClient {
    pub fn discover() -> Result<Self, BackendCliError> {
        Ok(Self::new(discover_executable(
            "UTA_STUDIO_RUNTIME_CLI_PATH",
            "uta-runtime",
        )?))
    }

    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            store: None,
            policy: RuntimePolicyWireV1::Production,
        }
    }

    pub fn with_store(mut self, store: impl Into<PathBuf>) -> Self {
        self.store = Some(store.into());
        self
    }
    pub fn with_policy(mut self, policy: RuntimePolicyWireV1) -> Self {
        self.policy = policy;
        self
    }
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn list(&self) -> Result<Vec<RuntimeResourceStatusWireV1>, BackendCliError> {
        self.read("list", &[])
    }

    pub fn status(
        &self,
        resources: &[RuntimeResourceRefWireV1],
    ) -> Result<Vec<RuntimeResourceStatusWireV1>, BackendCliError> {
        self.read("status", &resource_args(resources))
    }

    pub fn show(
        &self,
        resource: &RuntimeResourceRefWireV1,
    ) -> Result<RuntimeResourceDetailsWireV1, BackendCliError> {
        self.read("show", &[resource.to_string()])
    }

    pub fn resolve(
        &self,
        model_id: &str,
    ) -> Result<RuntimeResolvedIdentityWireV1, BackendCliError> {
        let resource = RuntimeResourceRefWireV1::model(model_id).map_err(BackendCliError::Io)?;
        self.read("resolve", &[resource.to_string()])
    }

    pub fn resolve_tool(
        &self,
        tool_id: &str,
    ) -> Result<RuntimeResolvedToolWireV1, BackendCliError> {
        let resource = RuntimeResourceRefWireV1::tool(tool_id).map_err(BackendCliError::Io)?;
        self.read("resolve", &[resource.to_string()])
    }

    pub fn configure_tool(
        &self,
        tool_id: &str,
        executable: &Path,
    ) -> Result<RuntimeResourceStatusWireV1, BackendCliError> {
        let resource = RuntimeResourceRefWireV1::tool(tool_id).map_err(BackendCliError::Io)?;
        self.read(
            "configure-tool",
            &[
                resource.to_string(),
                "--path".to_string(),
                executable.to_string_lossy().into_owned(),
                "--yes".to_string(),
            ],
        )
    }

    pub fn clear_tool(
        &self,
        tool_id: &str,
    ) -> Result<RuntimeResourceStatusWireV1, BackendCliError> {
        let resource = RuntimeResourceRefWireV1::tool(tool_id).map_err(BackendCliError::Io)?;
        self.read("clear-tool", &[resource.to_string(), "--yes".to_string()])
    }

    pub fn fusion_providers(&self) -> Result<RuntimeFusionProviderReportWireV1, BackendCliError> {
        self.read("fusion-providers", &[])
    }

    pub fn configure_fusion_provider(
        &self,
        provider: &str,
    ) -> Result<RuntimeFusionProviderReportWireV1, BackendCliError> {
        self.read(
            "configure-fusion-provider",
            &[
                "--provider".to_string(),
                provider.to_string(),
                "--yes".to_string(),
            ],
        )
    }

    pub fn clear_fusion_provider(
        &self,
    ) -> Result<RuntimeFusionProviderReportWireV1, BackendCliError> {
        self.read("clear-fusion-provider", &["--yes".to_string()])
    }

    pub fn install(
        &self,
        resources: &[RuntimeResourceRefWireV1],
    ) -> Result<RuntimeMutationResultWireV1, BackendCliError> {
        self.mutate("install", resources)
    }

    pub fn repair(
        &self,
        resources: &[RuntimeResourceRefWireV1],
    ) -> Result<RuntimeMutationResultWireV1, BackendCliError> {
        self.mutate("repair", resources)
    }

    pub fn reinstall(
        &self,
        resources: &[RuntimeResourceRefWireV1],
    ) -> Result<RuntimeMutationResultWireV1, BackendCliError> {
        self.mutate("reinstall", resources)
    }

    pub fn remove(
        &self,
        resources: &[RuntimeResourceRefWireV1],
    ) -> Result<RuntimeMutationResultWireV1, BackendCliError> {
        self.mutate("remove", resources)
    }

    fn mutate(
        &self,
        command: &str,
        resources: &[RuntimeResourceRefWireV1],
    ) -> Result<RuntimeMutationResultWireV1, BackendCliError> {
        if resources.is_empty() {
            return Err(BackendCliError::Io(
                "runtime mutation requires at least one resource".to_string(),
            ));
        }
        let mut arguments = resource_args(resources);
        arguments.push("--yes".to_string());
        self.read(command, &arguments)
    }

    fn read<T: DeserializeOwned>(
        &self,
        command: &str,
        arguments: &[String],
    ) -> Result<T, BackendCliError> {
        let (_, result) = self.run::<T>(command, arguments)?;
        Ok(result)
    }

    pub fn run<T: DeserializeOwned>(
        &self,
        command: &str,
        arguments: &[String],
    ) -> Result<(Vec<RuntimeEventEnvelopeV1>, T), BackendCliError> {
        if !self.executable.is_file() {
            return Err(BackendCliError::ExecutableMissing(self.executable.clone()));
        }
        let mut command_process = native_command(&self.executable);
        command_process.arg(command).args(arguments);
        if let Some(store) = &self.store {
            command_process.args(["--store", &store.to_string_lossy()]);
        }
        command_process
            .args(["--policy", self.policy.as_str(), "--output", "ndjson"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command_process
            .spawn()
            .map_err(|error| BackendCliError::SpawnFailed(error.to_string()))?;
        let mut child = ChildReaper(child);
        let stdout = child.0.stdout.take().ok_or_else(|| {
            BackendCliError::SpawnFailed("runtime stdout was unavailable".to_string())
        })?;
        let stderr_pipe = child.0.stderr.take().ok_or_else(|| {
            BackendCliError::SpawnFailed("runtime stderr was unavailable".to_string())
        })?;
        let (stderr, stderr_thread) = spawn_stderr_drain(stderr_pipe);
        let mut reader = BufReader::new(stdout);
        let mut events = Vec::new();
        let mut result = None;
        let mut domain_error = None;
        while let Some(frame) = read_machine_frame(&mut reader)? {
            let schema = frame
                .get("schema")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    BackendCliError::MalformedFrame("runtime frame omitted schema".to_string())
                })?;
            let version = frame
                .get("schema_version")
                .and_then(serde_json::Value::as_u64);
            if version != Some(1) {
                return Err(BackendCliError::ProtocolMismatch(format!(
                    "runtime frame {schema} has schema_version {version:?}"
                )));
            }
            match schema {
                "uta.runtime.event" => {
                    let event: RuntimeEventEnvelopeV1 =
                        serde_json::from_value(frame).map_err(|error| {
                            BackendCliError::MalformedFrame(format!(
                                "invalid runtime event: {error}"
                            ))
                        })?;
                    events.push(event);
                }
                "uta.runtime.result" => {
                    if result.is_some() {
                        return Err(BackendCliError::MalformedFrame(
                            "runtime emitted duplicate result frames".to_string(),
                        ));
                    }
                    let envelope: RuntimeResultEnvelopeV1<T> = serde_json::from_value(frame)
                        .map_err(|error| {
                            BackendCliError::MalformedFrame(format!(
                                "invalid runtime result: {error}"
                            ))
                        })?;
                    if envelope.frame_type != "result" || envelope.command != command {
                        return Err(BackendCliError::ProtocolMismatch(format!(
                            "runtime result command/type mismatch for {command}"
                        )));
                    }
                    result = Some(envelope.data);
                }
                "uta.runtime.error" => {
                    let error: RuntimeErrorEnvelopeV1 =
                        serde_json::from_value(frame).map_err(|parse| {
                            BackendCliError::MalformedFrame(format!(
                                "invalid runtime error: {parse}"
                            ))
                        })?;
                    if error.frame_type != "error" {
                        return Err(BackendCliError::ProtocolMismatch(
                            "runtime error frame type mismatch".to_string(),
                        ));
                    }
                    domain_error = Some(BackendCliError::Domain {
                        code: error.code,
                        message: error.message,
                        retryable: error.retryable,
                        request_id: None,
                        capability: None,
                        resource: error.resource,
                    });
                }
                other => {
                    return Err(BackendCliError::ProtocolMismatch(format!(
                        "unsupported runtime frame schema {other}"
                    )));
                }
            }
        }
        let status = child.0.wait().map_err(BackendCliError::from)?;
        let _ = stderr_thread.join();
        if let Some(error) = domain_error {
            return Err(error);
        }
        if !status.success() {
            return Err(BackendCliError::UnexpectedExit(format!(
                "runtime {command} returned {status}; stderr: {}",
                stderr_text(&stderr)
            )));
        }
        let result = result.ok_or_else(|| {
            BackendCliError::UnexpectedExit(format!("runtime {command} returned no result frame"))
        })?;
        Ok((events, result))
    }
}

struct ChildReaper(Child);

impl Drop for ChildReaper {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn resource_args(resources: &[RuntimeResourceRefWireV1]) -> Vec<String> {
    resources.iter().map(ToString::to_string).collect()
}
