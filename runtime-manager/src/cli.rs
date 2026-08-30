use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{
    MutationOptions, RequirementSet, ResourceKind, ResourceRef, RuntimeManager,
    RuntimeManagerError, RuntimeManagerResult, StorePaths,
};

const MAX_REQUIREMENTS_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Human,
    Json,
    Ndjson,
}

#[derive(Debug)]
struct CliError {
    error: RuntimeManagerError,
    exit_code: i32,
}

impl From<RuntimeManagerError> for CliError {
    fn from(error: RuntimeManagerError) -> Self {
        let exit_code = exit_code_for_error(&error.code);
        Self { error, exit_code }
    }
}

type CliResult<T> = Result<T, CliError>;

pub fn main_entry() -> i32 {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(output) = output_mode(&arguments) else {
        let error = RuntimeManagerError::new("invalid_cli_usage", "invalid --output mode");
        let _ = print_error(OutputMode::Human, &error);
        return 2;
    };
    match run(arguments, output) {
        Ok(code) => code,
        Err(failure) => {
            let _ = print_error(output, &failure.error);
            failure.exit_code
        }
    }
}

fn run(arguments: Vec<String>, output: OutputMode) -> CliResult<i32> {
    let command = arguments
        .first()
        .map(String::as_str)
        .ok_or_else(invalid_usage)?;
    if matches!(command, "help" | "--help" | "-h") {
        println!(
            "uta-runtime <list|show|status|paths|plan|setup|install|import|verify|repair|reinstall|remove|doctor|smoke|resolve|configure-tool|clear-tool>\n\
             resources are positional kind:id values; resolve accepts models or tools; mutations require --yes"
        );
        return Ok(0);
    }
    validate_options(&arguments)?;
    let policy = option(&arguments, "--policy")
        .map(str::parse)
        .transpose()
        .map_err(CliError::from)?
        .unwrap_or_default();
    let requested_backend = option(&arguments, "--backend")
        .map(str::parse)
        .transpose()
        .map_err(CliError::from)?;
    let mut paths = option(&arguments, "--store")
        .map(PathBuf::from)
        .map_or_else(StorePaths::from_env, |root| {
            StorePaths::from_env().with_store_root(root)
        });
    if let Some(root) = option(&arguments, "--legacy-models") {
        paths = paths.with_legacy_models_root(root);
    }
    let manager = RuntimeManager::with_default_catalog(paths).map_err(CliError::from)?;

    let data = match command {
        "list" => serde_json::to_value(manager.list(policy).map_err(CliError::from)?)?,
        "show" => {
            let resource = exactly_one_resource(&arguments)?;
            serde_json::to_value(
                manager
                    .show_with_backend(&resource, policy, requested_backend)
                    .map_err(CliError::from)?,
            )?
        }
        "status" => {
            let selected = resources(&arguments)?;
            let statuses = if selected.is_empty() {
                manager.list(policy).map_err(CliError::from)?
            } else {
                selected
                    .iter()
                    .map(|resource| {
                        manager.status_with_backend(resource, policy, requested_backend)
                    })
                    .collect::<RuntimeManagerResult<Vec<_>>>()
                    .map_err(CliError::from)?
            };
            let ready = statuses.iter().all(|status| status.usable);
            let status = if ready { "ok" } else { "not_ready" };
            print_result(output, command, status, &statuses)?;
            return Ok(if has_flag(&arguments, "--check") && !ready {
                10
            } else {
                0
            });
        }
        "paths" => serde_json::to_value(manager.paths_summary())?,
        "plan" => {
            let selected = resources_or_requirements(&arguments)?;
            serde_json::to_value(manager.plan(&selected, policy).map_err(CliError::from)?)?
        }
        "setup" => {
            let options = confirmed_mutation_options(&arguments)?;
            let requirements: RequirementSet =
                read_json(Path::new(required_option(&arguments, "--requirements")?))?;
            let selected = requirements
                .resources
                .iter()
                .map(|requirement| requirement.resource.clone())
                .collect::<Vec<_>>();
            print_operation_started(output, command, &selected)?;
            serde_json::to_value(
                manager
                    .setup_requirements(&requirements, policy, &options)
                    .map_err(CliError::from)?,
            )?
        }
        "install" => {
            let options = confirmed_mutation_options(&arguments)?;
            let selected = require_resources(&arguments)?;
            print_operation_started(output, command, &selected)?;
            serde_json::to_value(
                manager
                    .install(&selected, policy, &options)
                    .map_err(CliError::from)?,
            )?
        }
        "import" => {
            let options = confirmed_mutation_options(&arguments)?;
            let resource = exactly_one_resource(&arguments)?;
            let source = PathBuf::from(
                option(&arguments, "--from")
                    .or_else(|| option(&arguments, "--source"))
                    .ok_or_else(invalid_usage)?,
            );
            print_operation_started(output, command, std::slice::from_ref(&resource))?;
            serde_json::to_value(
                manager
                    .import_resource(&resource, &source, &options)
                    .map_err(CliError::from)?,
            )?
        }
        "verify" => {
            let selected = resources(&arguments)?;
            let report = manager.verify(&selected, policy).map_err(CliError::from)?;
            let failed = !report.corrupt.is_empty() || !report.incomplete.is_empty();
            print_result(
                output,
                command,
                if failed { "integrity_failed" } else { "ok" },
                &report,
            )?;
            return Ok(if failed { 11 } else { 0 });
        }
        "repair" => {
            let options = confirmed_mutation_options(&arguments)?;
            let selected = require_resources(&arguments)?;
            print_operation_started(output, command, &selected)?;
            serde_json::to_value(
                manager
                    .repair(&selected, policy, &options)
                    .map_err(CliError::from)?,
            )?
        }
        "reinstall" => {
            let options = confirmed_mutation_options(&arguments)?;
            let selected = require_resources(&arguments)?;
            print_operation_started(output, command, &selected)?;
            serde_json::to_value(
                manager
                    .reinstall(&selected, policy, &options)
                    .map_err(CliError::from)?,
            )?
        }
        "remove" => {
            let options = confirmed_mutation_options(&arguments)?;
            let selected = require_resources(&arguments)?;
            print_operation_started(output, command, &selected)?;
            serde_json::to_value(
                manager
                    .remove(&selected, &options)
                    .map_err(CliError::from)?,
            )?
        }
        "configure-tool" => {
            let _options = confirmed_mutation_options(&arguments)?;
            let resource = exactly_one_resource(&arguments)?;
            if resource.kind != ResourceKind::Tool {
                return Err(CliError::from(
                    RuntimeManagerError::new(
                        "invalid_resource",
                        "configure-tool accepts one tool resource",
                    )
                    .with_resource(resource),
                ));
            }
            let executable = PathBuf::from(required_option(&arguments, "--path")?);
            print_operation_started(output, command, std::slice::from_ref(&resource))?;
            serde_json::to_value(
                manager
                    .configure_external_tool(&resource, &executable)
                    .map_err(CliError::from)?,
            )?
        }
        "clear-tool" => {
            let _options = confirmed_mutation_options(&arguments)?;
            let resource = exactly_one_resource(&arguments)?;
            if resource.kind != ResourceKind::Tool {
                return Err(CliError::from(
                    RuntimeManagerError::new(
                        "invalid_resource",
                        "clear-tool accepts one tool resource",
                    )
                    .with_resource(resource),
                ));
            }
            print_operation_started(output, command, std::slice::from_ref(&resource))?;
            serde_json::to_value(
                manager
                    .clear_external_tool(&resource)
                    .map_err(CliError::from)?,
            )?
        }
        "doctor" => serde_json::to_value(manager.doctor())?,
        "smoke" => {
            let resource = exactly_one_resource(&arguments)?;
            serde_json::to_value(manager.smoke(&resource, policy).map_err(CliError::from)?)?
        }
        "resolve" => {
            let resource = exactly_one_resource(&arguments)?;
            match resource.kind {
                ResourceKind::Model => {
                    let resolved = manager
                        .resolve_model_with_backend(&resource.id, policy, requested_backend)
                        .map_err(CliError::from)?;
                    serde_json::to_value(ResolvedIdentity {
                        resource,
                        generation: resolved.generation,
                        content_digest: resolved.model_content_digest,
                        model_recipe_digest: resolved.model_recipe_digest,
                        runtime: resolved.runtime_id,
                        runtime_generation: resolved.runtime_generation,
                        runtime_content_digest: resolved.runtime_content_digest,
                        runtime_recipe_digest: resolved.runtime_recipe_digest,
                        runtime_executable: resolved.runtime_executable,
                        backend: resolved.backend,
                        policy,
                        validation_state: resolved.validation_state,
                        readiness_reasons: Vec::new(),
                    })?
                }
                ResourceKind::Tool => serde_json::to_value(
                    manager
                        .resolve_tool(&resource.id, policy)
                        .map_err(CliError::from)?,
                )?,
                ResourceKind::Runtime | ResourceKind::Bundle => {
                    return Err(CliError::from(
                        RuntimeManagerError::new(
                            "invalid_resource",
                            "resolve accepts model or tool resources",
                        )
                        .with_resource(resource),
                    ));
                }
            }
        }
        _ => return Err(invalid_usage()),
    };
    print_result(output, command, "ok", &data)?;
    Ok(0)
}

#[derive(Serialize)]
struct ResolvedIdentity {
    resource: ResourceRef,
    generation: String,
    content_digest: String,
    model_recipe_digest: String,
    runtime: String,
    runtime_generation: String,
    runtime_content_digest: String,
    runtime_recipe_digest: Option<String>,
    runtime_executable: PathBuf,
    backend: crate::NativeBackend,
    policy: crate::RuntimePolicy,
    validation_state: crate::ValidationState,
    readiness_reasons: Vec<crate::ReadinessReason>,
}

fn resources_or_requirements(arguments: &[String]) -> CliResult<Vec<ResourceRef>> {
    if let Some(path) = option(arguments, "--requirements") {
        let requirements: RequirementSet = read_json(Path::new(path))?;
        requirements.validate().map_err(CliError::from)?;
        Ok(requirements
            .resources
            .into_iter()
            .map(|requirement| requirement.resource)
            .collect())
    } else {
        require_resources(arguments)
    }
}

fn exactly_one_resource(arguments: &[String]) -> CliResult<ResourceRef> {
    let selected = resources(arguments)?;
    if selected.len() != 1 {
        return Err(invalid_usage());
    }
    Ok(selected.into_iter().next().expect("one resource"))
}

fn require_resources(arguments: &[String]) -> CliResult<Vec<ResourceRef>> {
    let selected = resources(arguments)?;
    if selected.is_empty() {
        Err(invalid_usage())
    } else {
        Ok(selected)
    }
}

fn resources(arguments: &[String]) -> CliResult<Vec<ResourceRef>> {
    let mut selected = Vec::new();
    let mut index = 1;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--resource" {
            let value = arguments.get(index + 1).ok_or_else(invalid_usage)?;
            selected.push(value.parse().map_err(CliError::from)?);
            index += 2;
        } else if option_takes_value(argument) {
            index += 2;
        } else if argument.starts_with('-') {
            index += 1;
        } else {
            selected.push(argument.parse().map_err(CliError::from)?);
            index += 1;
        }
    }
    selected.sort();
    selected.dedup();
    Ok(selected)
}

fn validate_options(arguments: &[String]) -> CliResult<()> {
    let mut index = 1;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument == "--resource" || option_takes_value(argument) {
            if arguments.get(index + 1).is_none() {
                return Err(invalid_usage());
            }
            index += 2;
        } else if matches!(argument, "--yes" | "--confirm" | "--check")
            || !argument.starts_with('-')
        {
            index += 1;
        } else {
            return Err(invalid_usage());
        }
    }
    Ok(())
}

fn option_takes_value(argument: &str) -> bool {
    matches!(
        argument,
        "--output"
            | "--store"
            | "--legacy-models"
            | "--policy"
            | "--backend"
            | "--requirements"
            | "--from"
            | "--source"
            | "--path"
    )
}

fn mutation_options(confirmed: bool) -> MutationOptions {
    MutationOptions { confirmed }
}

fn confirmed_mutation_options(arguments: &[String]) -> CliResult<MutationOptions> {
    if has_flag(arguments, "--yes") || has_flag(arguments, "--confirm") {
        return Ok(mutation_options(true));
    }
    if !std::io::stdin().is_terminal() {
        return Err(CliError {
            error: RuntimeManagerError::new(
                "confirmation_required",
                "non-interactive mutation requires explicit --yes confirmation",
            ),
            exit_code: 16,
        });
    }
    eprint!("Proceed? [y/N] ");
    std::io::stderr()
        .flush()
        .map_err(|error| CliError::from(RuntimeManagerError::internal(error.to_string())))?;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|error| CliError::from(RuntimeManagerError::internal(error.to_string())))?;
    if matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(mutation_options(true))
    } else {
        Err(CliError {
            error: RuntimeManagerError::new("cancelled", "operation cancelled"),
            exit_code: 17,
        })
    }
}

fn output_mode(arguments: &[String]) -> Option<OutputMode> {
    match option(arguments, "--output") {
        Some("json") => Some(OutputMode::Json),
        Some("ndjson") => Some(OutputMode::Ndjson),
        Some("human") | None => Some(OutputMode::Human),
        Some(_) => None,
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> CliResult<T> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        CliError::from(RuntimeManagerError::new(
            "invalid_requirements",
            format!("could not inspect {}: {error}", path.display()),
        ))
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_REQUIREMENTS_BYTES {
        return Err(CliError::from(RuntimeManagerError::new(
            "invalid_requirements",
            "requirements file size is invalid",
        )));
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|error| {
        CliError::from(RuntimeManagerError::new(
            "invalid_requirements",
            format!("could not read {}: {error}", path.display()),
        ))
    })?)
    .map_err(|error| {
        CliError::from(RuntimeManagerError::new(
            "invalid_requirements",
            format!("requirements JSON is invalid: {error}"),
        ))
    })
}

fn required_option<'a>(arguments: &'a [String], name: &str) -> CliResult<&'a str> {
    option(arguments, name).ok_or_else(invalid_usage)
}

fn option<'a>(arguments: &'a [String], name: &str) -> Option<&'a str> {
    arguments
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| arguments.get(index + 1))
        .map(String::as_str)
}

fn has_flag(arguments: &[String], flag: &str) -> bool {
    arguments.iter().any(|argument| argument == flag)
}

fn print_operation_started(
    output: OutputMode,
    operation: &str,
    resources: &[ResourceRef],
) -> CliResult<()> {
    if output != OutputMode::Ndjson {
        return Ok(());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let operation_id = format!("op-{}-{nanos}", std::process::id());
    write_value(
        output,
        &serde_json::json!({
            "schema": "uta.runtime.event",
            "schema_version": 1,
            "type": "operation_started",
            "operation_id": &operation_id,
            "operation": operation,
        }),
    )?;
    for resource in resources {
        write_value(
            output,
            &serde_json::json!({
                "schema": "uta.runtime.event",
                "schema_version": 1,
                "type": "resource_started",
                "operation_id": &operation_id,
                "resource": resource,
            }),
        )?;
    }
    Ok(())
}

fn print_result(
    output: OutputMode,
    command: &str,
    status: &str,
    data: &impl Serialize,
) -> CliResult<()> {
    if output == OutputMode::Human {
        serde_json::to_writer_pretty(std::io::stdout().lock(), data)
            .map_err(|error| CliError::from(RuntimeManagerError::internal(error.to_string())))?;
        println!();
        return Ok(());
    }
    let value = serde_json::json!({
        "schema":"uta.runtime.result",
        "schema_version":1,
        "type":"result",
        "command":command,
        "status":status,
        "data":data
    });
    write_value(output, &value)
}

fn print_error(output: OutputMode, error: &RuntimeManagerError) -> CliResult<()> {
    if output == OutputMode::Human {
        eprintln!("{}: {}", error.code, error.message);
        return Ok(());
    }
    let value = serde_json::json!({
        "schema":"uta.runtime.error",
        "schema_version":1,
        "type":"error",
        "code":error.code,
        "message":error.message,
        "resource":error.resource,
        "retryable":error.retryable
    });
    write_value(output, &value)
}

fn write_value(output: OutputMode, value: &impl Serialize) -> CliResult<()> {
    if output == OutputMode::Ndjson {
        serde_json::to_writer(std::io::stdout().lock(), value)
    } else {
        serde_json::to_writer_pretty(std::io::stdout().lock(), value)
    }
    .map_err(|error| CliError::from(RuntimeManagerError::internal(error.to_string())))?;
    println!();
    Ok(())
}

fn invalid_usage() -> CliError {
    CliError {
        error: RuntimeManagerError::new(
            "invalid_cli_usage",
            "invalid command usage; run uta-runtime help",
        ),
        exit_code: 2,
    }
}

fn exit_code_for_error(code: &str) -> i32 {
    match code {
        "invalid_cli_usage" | "invalid_resource" | "invalid_requirements" | "invalid_policy" => 2,
        "resource_missing" | "unknown_resource" => 10,
        "resource_corrupt" | "integrity_mismatch" | "source_identity_mismatch" => 11,
        "resource_unvalidated"
        | "no_validated_backend"
        | "tool_protocol_mismatch"
        | "tool_unusable"
        | "runtime_missing"
        | "worker_capability_missing"
        | "unsupported_platform" => 12,
        "network_failed" => 13,
        "resource_not_acquirable"
        | "repair_requires_source"
        | "insufficient_space"
        | "conversion_failed"
        | "publish_failed"
        | "smoke_failed" => 14,
        "resource_in_use" | "unmanaged_files_present" => 15,
        "confirmation_required" => 16,
        "cancelled" => 17,
        _ => 70,
    }
}

impl From<serde_json::Error> for CliError {
    fn from(error: serde_json::Error) -> Self {
        CliError::from(RuntimeManagerError::internal(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_and_explicit_resources_are_canonical() {
        let arguments = vec![
            "plan".to_string(),
            "model:rmvpe".to_string(),
            "--resource".to_string(),
            "tool:ffmpeg".to_string(),
            "--output".to_string(),
            "json".to_string(),
        ];
        assert_eq!(
            resources(&arguments).unwrap(),
            [
                ResourceRef::model("rmvpe").unwrap(),
                ResourceRef::tool("ffmpeg").unwrap()
            ]
        );
    }

    #[test]
    fn mutation_requires_explicit_confirmation_flag() {
        let options = mutation_options(true);
        assert!(options.confirmed);
    }
}
