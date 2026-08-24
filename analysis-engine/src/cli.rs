use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use uta_runtime_manager::RuntimePolicy;

use crate::AnalysisEngine;
use crate::contract::{
    AnalyzeRequestV1, EngineError, EngineErrorCode, EngineResult, ExportRequestV1,
};

const MAX_REQUEST_BYTES: u64 = 16 * 1024 * 1024;

pub fn main_entry() -> i32 {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => 0,
        Err(error) => {
            let value = serde_json::json!({"type": "error", "error": error});
            let _ = serde_json::to_writer(std::io::stdout().lock(), &value);
            println!();
            1
        }
    }
}

fn run(arguments: Vec<String>) -> EngineResult<()> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(usage_error());
    };
    if command == "worker" {
        if !arguments.iter().any(|argument| argument == "--stdio-json") {
            return Err(usage_error());
        }
        return crate::worker::worker_main()
            .map_err(|error| EngineError::new(EngineErrorCode::WorkerFailed, error));
    }

    let engine = AnalysisEngine::from_env()?;
    match command {
        "capabilities" => {
            let policy = option_value(&arguments, "--runtime-policy")
                .map(|value| value.parse())
                .transpose()
                .map_err(EngineError::from)?
                .unwrap_or(RuntimePolicy::Experimental);
            print_json(&engine.capabilities(policy))
        }
        "validate" => {
            let request: AnalyzeRequestV1 = read_option_json(&arguments, "--request")?;
            engine.validate(&request)?;
            print_json(&serde_json::json!({
                "type": "validation_result",
                "request_id": request.request_id,
                "valid": true
            }))
        }
        "requirements" => {
            let request: AnalyzeRequestV1 = read_option_json(&arguments, "--request")?;
            print_json(&engine.requirements(&request)?)
        }
        "plan" => {
            let request: AnalyzeRequestV1 = read_option_json(&arguments, "--request")?;
            print_json(&engine.plan(&request)?)
        }
        "analyze" => {
            let request: AnalyzeRequestV1 = read_option_json(&arguments, "--request")?;
            let output = required_option(&arguments, "--output-dir")?;
            print_json(&engine.analyze(&request, PathBuf::from(output))?)
        }
        "export" => {
            let request: ExportRequestV1 = read_option_json(&arguments, "--request")?;
            engine.export(&request)
        }
        "doctor" => print_json(&engine.runtime_manager().doctor()),
        "help" | "--help" | "-h" => {
            println!(
                "uta-analyze <capabilities|validate|requirements|plan|analyze|export|doctor|worker>\n\
                 canonical requests use --request <json>; analyze also requires --output-dir <dir>"
            );
            Ok(())
        }
        _ => Err(usage_error()),
    }
}

fn read_option_json<T: DeserializeOwned>(arguments: &[String], name: &str) -> EngineResult<T> {
    let path = PathBuf::from(required_option(arguments, name)?);
    read_json(&path)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> EngineResult<T> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            format!("could not inspect request {}: {error}", path.display()),
        )
    })?;
    if metadata.len() > MAX_REQUEST_BYTES {
        return Err(EngineError::new(
            EngineErrorCode::InvalidContract,
            "request exceeds the v1 size limit",
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            format!("could not read request {}: {error}", path.display()),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        EngineError::new(
            EngineErrorCode::InvalidContract,
            format!("request JSON is invalid: {error}"),
        )
    })
}

fn required_option<'a>(arguments: &'a [String], name: &str) -> EngineResult<&'a str> {
    option_value(arguments, name).ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::InvalidContract,
            format!("required option is missing: {name}"),
        )
    })
}

fn option_value<'a>(arguments: &'a [String], name: &str) -> Option<&'a str> {
    arguments
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| arguments.get(index + 1))
        .map(String::as_str)
}

fn print_json(value: &impl serde::Serialize) -> EngineResult<()> {
    serde_json::to_writer_pretty(std::io::stdout().lock(), value).map_err(|error| {
        EngineError::new(
            EngineErrorCode::InternalError,
            format!("could not serialize command result: {error}"),
        )
    })?;
    println!();
    Ok(())
}

fn usage_error() -> EngineError {
    EngineError::new(
        EngineErrorCode::InvalidContract,
        "usage: uta-analyze <capabilities|validate|requirements|plan|analyze|export|doctor|worker>",
    )
}
