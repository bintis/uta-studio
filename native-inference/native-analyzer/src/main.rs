use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde::Deserialize;

const PROTOCOL: u32 = 1;
const RUNTIME_LOCK_SHA256: &str =
    "c6f2228718c832323e053cf62815d7ba7ff01309c899bcafe8adc68db4fc200d";

#[derive(Debug, Deserialize)]
struct Command {
    #[serde(rename = "type")]
    kind: String,
    protocol: u32,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    audio_path: Option<PathBuf>,
    #[serde(default)]
    cache_path: Option<PathBuf>,
    #[serde(default)]
    workflow_execution: Option<WorkflowExecution>,
}

#[derive(Debug, Deserialize)]
struct WorkflowExecution {
    #[serde(default)]
    quality_mode: String,
    #[serde(default)]
    node_bindings: Vec<WorkflowNodeBinding>,
}

#[derive(Debug, Deserialize)]
struct WorkflowNodeBinding {
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    runtime: String,
    #[serde(default)]
    execution_policy: ExecutionPolicy,
}

#[derive(Debug, Default, Deserialize)]
struct ExecutionPolicy {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    condition: String,
}

fn emit(value: serde_json::Value) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value).map_err(|error| error.to_string())?;
    stdout.write_all(b"\n").map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}

fn component_available(variable: &str) -> bool {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .is_some_and(|path| path.is_file())
}

fn required_components(command: &Command) -> Result<Vec<(&'static str, &'static str)>, String> {
    let Some(workflow) = command.workflow_execution.as_ref() else {
        return Ok(vec![
            ("UTA_STUDIO_ROFORMER_RUNTIME_PATH", "RoFormer"),
            ("UTA_STUDIO_OPENVINO_RUNTIME_PATH", "OpenVINO"),
            ("UTA_STUDIO_QWEN_ASR_RUNTIME_PATH", "Qwen ASR"),
            ("UTA_STUDIO_QWEN_ALIGN_RUNTIME_PATH", "Qwen aligner"),
        ]);
    };
    let mut required = BTreeMap::new();
    for binding in &workflow.node_bindings {
        let Some(model_id) = binding.model_id.as_deref() else {
            continue;
        };
        if binding.execution_policy.mode == "disabled"
            || (binding.execution_policy.mode == "conditional"
                && binding.execution_policy.condition == "maximum_only"
                && workflow.quality_mode != "maximum")
        {
            continue;
        }
        let component = match binding.runtime.as_str() {
            "vulkan" => ("UTA_STUDIO_ROFORMER_RUNTIME_PATH", "RoFormer"),
            "open_vino" => ("UTA_STUDIO_OPENVINO_RUNTIME_PATH", "OpenVINO"),
            "pinned_qwen_asr_vulkan" => ("UTA_STUDIO_QWEN_ASR_RUNTIME_PATH", "Qwen ASR"),
            "pinned_qwen_align_vulkan" => ("UTA_STUDIO_QWEN_ALIGN_RUNTIME_PATH", "Qwen aligner"),
            "native_dsp" => continue,
            "unresolved" | "" => {
                return Err(format!(
                    "model {model_id} has no production-validated native runtime"
                ));
            }
            runtime => {
                return Err(format!(
                    "model {model_id} selected unknown runtime {runtime}"
                ));
            }
        };
        required.insert(component.0, component.1);
    }
    Ok(required.into_iter().collect())
}

fn analyze(command: &Command) -> Result<(), String> {
    let source = command
        .audio_path
        .as_ref()
        .ok_or_else(|| "analysis command omitted audio_path".to_string())?;
    if !source.is_file() {
        return Err(format!("source media is unavailable: {}", source.display()));
    }
    let output = command
        .cache_path
        .as_ref()
        .ok_or_else(|| "analysis command omitted cache_path".to_string())?;
    if !output.is_dir() {
        return Err(format!(
            "authorized cache directory is unavailable: {}",
            output.display()
        ));
    }
    let required = required_components(command)?;
    let missing = required
        .iter()
        .filter(|(variable, _)| !component_available(variable))
        .map(|(_, label)| *label)
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "native components are unavailable: {}; install them in Settings > Models & runtime",
            missing.join(", ")
        ));
    }

    // The coordinator deliberately fails closed until every selected worker
    // implements the v1 artifact contract. Returning a synthetic transcript,
    // pitch track, or audio file here would incorrectly mark incomplete model
    // execution as a successful chart analysis.
    Err(format!(
        "native workflow execution for {} has no fully validated component set in this build",
        command.hash
    ))
}

fn main() {
    if !std::env::args().any(|argument| argument == "--stdio-json") {
        eprintln!("uta-native-analyzer requires --stdio-json");
        std::process::exit(2);
    }
    if std::env::var("UTA_STUDIO_RUNTIME_LOCK_SHA256")
        .ok()
        .is_some_and(|expected| expected != RUNTIME_LOCK_SHA256)
    {
        eprintln!("runtime-lock identity does not match this native analyzer build");
        std::process::exit(3);
    }
    if emit(serde_json::json!({
        "type": "ready",
        "protocol": PROTOCOL,
        "component": "uta-native-analyzer",
        "runtime_recipe_digest": RUNTIME_LOCK_SHA256,
    }))
    .is_err()
    {
        std::process::exit(3);
    }

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) if !line.trim().is_empty() => line,
            Ok(_) => continue,
            Err(error) => {
                eprintln!("stdin error: {error}");
                break;
            }
        };
        let command: Command = match serde_json::from_str(&line) {
            Ok(command) => command,
            Err(error) => {
                let _ = emit(serde_json::json!({
                    "type": "error",
                    "kind": "invalid_command",
                    "msg": error.to_string(),
                }));
                continue;
            }
        };
        if command.protocol != PROTOCOL {
            let _ = emit(serde_json::json!({
                "type": "error",
                "kind": "unsupported_protocol",
                "msg": format!("unsupported native analyzer protocol {}", command.protocol),
            }));
            continue;
        }
        match command.kind.as_str() {
            "quit" => break,
            "analyze" => match analyze(&command) {
                Ok(()) => {
                    let _ = emit(serde_json::json!({"type": "done"}));
                }
                Err(message) => {
                    let _ = emit(serde_json::json!({
                        "type": "error",
                        "kind": "native_runtime_unavailable",
                        "msg": message,
                    }));
                }
            },
            _ => {
                let _ = emit(serde_json::json!({
                    "type": "error",
                    "kind": "unsupported_command",
                    "msg": format!("unsupported native analyzer command: {}", command.kind),
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(workflow: serde_json::Value) -> Command {
        Command {
            kind: "analyze".to_string(),
            protocol: PROTOCOL,
            hash: "fixture".to_string(),
            audio_path: None,
            cache_path: None,
            workflow_execution: Some(serde_json::from_value(workflow).unwrap()),
        }
    }

    #[test]
    fn selected_components_follow_resolved_runtime_not_model_name_guessing() {
        let command = command(serde_json::json!({
            "quality_mode": "balanced",
            "node_bindings": [
                {
                    "model_id": "separation-model",
                    "runtime": "vulkan",
                    "execution_policy": {"mode": "always"}
                },
                {
                    "model_id": "pitch-model",
                    "runtime": "open_vino",
                    "execution_policy": {"mode": "always"}
                }
            ]
        }));
        let required = required_components(&command).unwrap();
        assert_eq!(required.len(), 2);
        assert!(required.iter().any(|(_, label)| *label == "RoFormer"));
        assert!(required.iter().any(|(_, label)| *label == "OpenVINO"));
    }

    #[test]
    fn maximum_only_component_is_not_required_by_balanced_workflow() {
        let command = command(serde_json::json!({
            "quality_mode": "balanced",
            "node_bindings": [{
                "model_id": "maximum-expert",
                "runtime": "open_vino",
                "execution_policy": {
                    "mode": "conditional",
                    "condition": "maximum_only"
                }
            }]
        }));
        assert!(required_components(&command).unwrap().is_empty());
    }

    #[test]
    fn unresolved_selected_model_fails_closed() {
        let command = command(serde_json::json!({
            "quality_mode": "balanced",
            "node_bindings": [{
                "model_id": "candidate-only",
                "runtime": "unresolved",
                "execution_policy": {"mode": "always"}
            }]
        }));
        assert!(
            required_components(&command)
                .unwrap_err()
                .contains("no production-validated")
        );
    }
}
