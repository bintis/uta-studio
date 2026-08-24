use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{audio, runtime};

const FIXED_SAFE_EXECUTION_ARGS: [&str; 4] = [
    "--batch-size",
    "1",
    "--vulkan-no-async",
    "--serial-pipeline",
];

pub struct PublishedOutput {
    pub artifact: &'static str,
    pub path: PathBuf,
}

fn model_path(config: &serde_json::Value) -> Result<PathBuf, String> {
    config
        .get("model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "GGML task requires Runtime Manager-resolved config.model_path".to_string())
}

fn vulkan_device(config: &serde_json::Value) -> Result<u32, String> {
    let device = config
        .get("vulkan_device")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    u32::try_from(device)
        .ok()
        .filter(|device| *device <= 255)
        .ok_or_else(|| "GGML Vulkan device index is invalid".to_string())
}

fn validate_semantics(model_id: &str, config: &serde_json::Value) -> Result<(), String> {
    if config.get("backend").and_then(serde_json::Value::as_str) != Some("ggml_vulkan") {
        return Err("GGML worker requires the explicit ggml_vulkan backend".to_string());
    }
    let semantic = config
        .get("semantic_output")
        .and_then(serde_json::Value::as_str);
    let expected = match model_id {
        "bs_roformer_vocals_ep317" => "guide_vocals",
        "melband_roformer_inst_v2" => "instrumental",
        "melband_roformer_denoise_aufr33" => "dry",
        "melband_roformer_dereverb_anvuew" => "noreverb",
        "melband_roformer_harmony" => "lead_vocal+backing_vocal_residual",
        _ => return Err(format!("model {model_id} has no GGML Vulkan executor")),
    };
    if semantic != Some(expected) {
        return Err(format!(
            "GGML {model_id} task requires semantic_output={expected}"
        ));
    }
    if model_id == "melband_roformer_harmony"
        && config
            .get("input_semantics")
            .and_then(serde_json::Value::as_str)
            != Some("all_vocals")
    {
        return Err("GGML Harmony requires explicit all_vocals input semantics".to_string());
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn prepend_library_path(
    command: &mut Command,
    variable: &str,
    directory: &Path,
) -> Result<(), String> {
    let inherited = std::env::var_os(variable).unwrap_or_default();
    let combined = std::env::join_paths(
        std::iter::once(directory.to_path_buf()).chain(std::env::split_paths(&inherited)),
    )
    .map_err(|error| format!("could not construct GGML runtime {variable}: {error}"))?;
    command.env(variable, combined);
    Ok(())
}

fn output_name(model_id: &str) -> &'static str {
    match model_id {
        "bs_roformer_vocals_ep317" => "guide-vocals.flac",
        "melband_roformer_inst_v2" => "instrumental.flac",
        "melband_roformer_denoise_aufr33" => "clean-lead-vocal.flac",
        "melband_roformer_dereverb_anvuew" => "noreverb-vocal.flac",
        "melband_roformer_harmony" => "lead-vocal.flac",
        _ => unreachable!("validated model id"),
    }
}

fn artifact_name(model_id: &str) -> &'static str {
    match model_id {
        "bs_roformer_vocals_ep317" => "guide_vocals",
        "melband_roformer_inst_v2" => "instrumental",
        "melband_roformer_denoise_aufr33" => "clean_lead_vocal",
        "melband_roformer_dereverb_anvuew" => "dereverbed_vocal",
        "melband_roformer_harmony" => "lead_vocal",
        _ => unreachable!("validated model id"),
    }
}

pub fn run(
    task_id: &str,
    model_id: &str,
    source: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &'static str),
) -> Result<Vec<PublishedOutput>, String> {
    validate_semantics(model_id, config)?;
    progress(0.02, "Validating pinned GGML Vulkan runtime");
    let validated_runtime = runtime::validate_runtime()?;
    progress(0.05, "Validating exact GGUF model identity");
    let model = runtime::validate_model(model_id, &model_path(config)?)?;
    let input = audio::decode_stereo_wav(source, output_dir, task_id)?;
    let engine_output = output_dir.join(format!("{task_id}-ggml-engine.wav"));
    if engine_output.exists() {
        let _ = std::fs::remove_file(&input);
        return Err("GGML engine output target already exists".to_string());
    }
    progress(0.1, "Running GGML model on explicit Vulkan device");
    let mut command = Command::new(&validated_runtime.engine);
    command
        .arg(&model)
        .arg(&input)
        .arg(&engine_output)
        .args(&FIXED_SAFE_EXECUTION_ARGS[..2])
        .arg("--vulkan-device")
        .arg(vulkan_device(config)?.to_string())
        .args(&FIXED_SAFE_EXECUTION_ARGS[2..])
        .env(
            "UTA_STUDIO_GGML_RUNTIME_MANIFEST_SHA256",
            validated_runtime.manifest_sha256,
        )
        .stdin(Stdio::null())
        // The legacy engine prints a progress bar. Never accumulate unbounded
        // child output inside the long-lived protocol worker.
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "linux")]
    prepend_library_path(
        &mut command,
        "LD_LIBRARY_PATH",
        &validated_runtime.library_dir,
    )?;
    #[cfg(target_os = "windows")]
    prepend_library_path(&mut command, "PATH", &validated_runtime.library_dir)?;
    let output = command
        .status()
        .map_err(|error| format!("could not start GGML RoFormer engine: {error}"));
    match output {
        Ok(status) if status.success() && engine_output.is_file() => {}
        Ok(status) => {
            let _ = std::fs::remove_file(&input);
            let _ = std::fs::remove_file(&engine_output);
            return Err(format!("GGML RoFormer engine failed with {status}"));
        }
        Err(error) => {
            let _ = std::fs::remove_file(&input);
            return Err(error);
        }
    }

    progress(0.92, "Atomically encoding lossless GGML output");
    let destination = output_dir.join(output_name(model_id));
    let result = (|| {
        audio::encode_flac(&engine_output, &destination)?;
        let mut published = vec![PublishedOutput {
            artifact: artifact_name(model_id),
            path: destination.clone(),
        }];
        if model_id == "melband_roformer_harmony" {
            let residual = output_dir.join("vocal-residual.flac");
            audio::encode_vocal_residual_flac(&input, &engine_output, &residual)?;
            published.push(PublishedOutput {
                artifact: "vocal_residual",
                path: residual,
            });
        }
        Ok(published)
    })();
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&engine_output);
    if result.is_err() {
        let _ = std::fs::remove_file(&destination);
        let _ = std::fs::remove_file(output_dir.join("vocal-residual.flac"));
    }
    progress(1.0, "GGML Vulkan inference complete");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_invocation_pins_all_three_safety_controls() {
        assert_eq!(
            FIXED_SAFE_EXECUTION_ARGS,
            [
                "--batch-size",
                "1",
                "--vulkan-no-async",
                "--serial-pipeline"
            ]
        );
    }

    #[test]
    fn semantic_routes_are_explicit_and_non_substituting() {
        assert!(
            validate_semantics(
                "bs_roformer_vocals_ep317",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"guide_vocals"
                })
            )
            .is_ok()
        );
        assert!(
            validate_semantics(
                "bs_roformer_vocals_ep317",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"instrumental"
                })
            )
            .is_err()
        );
        assert!(
            validate_semantics(
                "rmvpe",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"pitch"
                })
            )
            .is_err()
        );
        assert!(
            validate_semantics(
                "melband_roformer_harmony",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "input_semantics":"all_vocals",
                    "semantic_output":"lead_vocal+backing_vocal_residual"
                })
            )
            .is_ok()
        );
        assert!(
            validate_semantics(
                "melband_roformer_harmony",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "input_semantics":"all_vocals",
                    "semantic_output":"lead_vocal+backing_vocal"
                })
            )
            .is_err()
        );
    }
}
