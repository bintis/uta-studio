use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{audio, runtime};

const FIXED_SAFE_EXECUTION_ARGS: [&str; 4] = [
    "--batch-size",
    "1",
    "--vulkan-no-async",
    "--serial-pipeline",
];
const MAX_ENGINE_STDERR_BYTES: usize = 64 * 1024;

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
        "bs_roformer_leap_xe90_vocals" => "guide_vocals",
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
        "bs_roformer_leap_xe90_vocals" => "guide-vocals.flac",
        "melband_roformer_inst_v2" => "instrumental.flac",
        "melband_roformer_denoise_aufr33" => "clean-lead-vocal.flac",
        "melband_roformer_dereverb_anvuew" => "noreverb-vocal.flac",
        "melband_roformer_harmony" => "lead-vocal.flac",
        _ => unreachable!("validated model id"),
    }
}

fn artifact_name(model_id: &str) -> &'static str {
    match model_id {
        "bs_roformer_leap_xe90_vocals" => "guide_vocals",
        "melband_roformer_inst_v2" => "instrumental",
        "melband_roformer_denoise_aufr33" => "clean_lead_vocal",
        "melband_roformer_dereverb_anvuew" => "dereverbed_vocal",
        "melband_roformer_harmony" => "lead_vocal",
        _ => unreachable!("validated model id"),
    }
}

fn ggml_vulkan_command(
    engine: &Path,
    model: &Path,
    input: &Path,
    output: &Path,
    device: u32,
) -> Command {
    let mut command = Command::new(engine);
    command
        .arg(model)
        .arg(input)
        .arg(output)
        .args(&FIXED_SAFE_EXECUTION_ARGS[..2])
        .arg("--vulkan-device")
        .arg(device.to_string())
        .args(&FIXED_SAFE_EXECUTION_ARGS[2..])
        .arg("--machine-progress")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn read_engine_stderr_tail(mut stderr: impl Read) -> Result<Vec<u8>, String> {
    let mut tail = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = stderr
            .read(&mut chunk)
            .map_err(|error| format!("could not read GGML diagnostics: {error}"))?;
        if count == 0 {
            return Ok(tail);
        }
        if count >= MAX_ENGINE_STDERR_BYTES {
            tail.clear();
            tail.extend_from_slice(&chunk[count - MAX_ENGINE_STDERR_BYTES..count]);
            continue;
        }
        let overflow = tail
            .len()
            .saturating_add(count)
            .saturating_sub(MAX_ENGINE_STDERR_BYTES);
        if overflow > 0 {
            tail.drain(..overflow);
        }
        tail.extend_from_slice(&chunk[..count]);
    }
}

pub fn run(
    task_id: &str,
    model_id: &str,
    source: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &'static str, Option<(u64, u64)>),
) -> Result<Vec<PublishedOutput>, String> {
    validate_semantics(model_id, config)?;
    progress(0.02, "Validating pinned GGML Vulkan runtime", None);
    let validated_runtime = runtime::validate_runtime()?;
    progress(0.05, "Validating GGUF model structure", None);
    let model = runtime::validate_model(model_id, &model_path(config)?)?;
    let input = audio::decode_stereo_wav(source, output_dir, task_id)?;
    let engine_output = output_dir.join(format!("{task_id}-ggml-engine.wav"));
    if engine_output.exists() {
        let _ = std::fs::remove_file(&input);
        return Err("GGML engine output target already exists".to_string());
    }
    progress(0.1, "Running GGML model on explicit Vulkan device", None);
    // Machine stdout carries bounded, exact overlap-add chunk records. Human
    // diagnostics remain unparsed and are discarded line by line.
    let mut command = ggml_vulkan_command(
        &validated_runtime.engine,
        &model,
        &input,
        &engine_output,
        vulkan_device(config)?,
    );
    #[cfg(target_os = "linux")]
    prepend_library_path(
        &mut command,
        "LD_LIBRARY_PATH",
        &validated_runtime.library_dir,
    )?;
    #[cfg(target_os = "windows")]
    prepend_library_path(&mut command, "PATH", &validated_runtime.library_dir)?;
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start GGML RoFormer engine: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "GGML RoFormer machine-progress stdout is unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "GGML RoFormer diagnostics stderr is unavailable".to_string())?;
    let stderr_reader = std::thread::spawn(move || read_engine_stderr_tail(stderr));
    let mut last_units = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("could not read GGML progress: {error}"))?;
        let Some((completed, total)) = parse_work_units(&line)? else {
            continue;
        };
        if last_units.is_some_and(|(previous, previous_total)| {
            total != previous_total || completed <= previous
        }) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("GGML RoFormer work units changed identity or regressed".to_string());
        }
        last_units = Some((completed, total));
        progress(
            completed as f32 / total as f32,
            "Running measured GGML overlap-add chunk",
            Some((completed, total)),
        );
    }
    let status = child
        .wait()
        .map_err(|error| format!("could not wait for GGML RoFormer engine: {error}"))?;
    let diagnostics = stderr_reader
        .join()
        .map_err(|_| "GGML RoFormer diagnostics reader panicked".to_string())??;
    if !status.success() || !engine_output.is_file() {
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&engine_output);
        let diagnostics = String::from_utf8_lossy(&diagnostics);
        let diagnostics = diagnostics.trim();
        return Err(if diagnostics.is_empty() {
            format!("GGML RoFormer engine failed with {status}")
        } else {
            format!("GGML RoFormer engine failed with {status}: {diagnostics}")
        });
    }
    if last_units.is_none_or(|(completed, total)| completed != total) {
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&engine_output);
        return Err("GGML RoFormer did not complete its measured chunk route".to_string());
    }

    progress(0.92, "Atomically encoding lossless GGML output", None);
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
        let _ = std::fs::remove_file(output_dir.join("instrumental.flac"));
    }
    progress(1.0, "GGML Vulkan inference complete", None);
    result
}

fn parse_work_units(line: &str) -> Result<Option<(u64, u64)>, String> {
    let Some(values) = line.strip_prefix("UTA_WORK_UNITS v1 ") else {
        return Ok(None);
    };
    let mut values = values.split_ascii_whitespace();
    let completed = values
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "GGML RoFormer completed chunk count is invalid".to_string())?;
    let total = values
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "GGML RoFormer total chunk count is invalid".to_string())?;
    if values.next().is_some() || total == 0 || completed == 0 || completed > total {
        return Err("GGML RoFormer work-unit record is invalid".to_string());
    }
    Ok(Some((completed, total)))
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
        let command = ggml_vulkan_command(
            Path::new("engine"),
            Path::new("model.gguf"),
            Path::new("input.wav"),
            Path::new("output.wav"),
            7,
        );
        let args = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "model.gguf",
                "input.wav",
                "output.wav",
                "--batch-size",
                "1",
                "--vulkan-device",
                "7",
                "--vulkan-no-async",
                "--serial-pipeline",
                "--machine-progress",
            ]
        );
    }

    #[test]
    fn engine_diagnostics_keep_the_failure_tail() {
        let mut input = vec![b'x'; MAX_ENGINE_STDERR_BYTES + 32];
        input.extend_from_slice(b"final write failure");
        let captured = read_engine_stderr_tail(input.as_slice()).unwrap();
        assert_eq!(captured.len(), MAX_ENGINE_STDERR_BYTES);
        assert!(captured.ends_with(b"final write failure"));
    }

    #[test]
    fn semantic_routes_are_explicit_and_non_substituting() {
        assert!(
            validate_semantics(
                "bs_roformer_leap_xe90_vocals",
                &serde_json::json!({
                    "backend":"ggml_vulkan",
                    "semantic_output":"guide_vocals"
                })
            )
            .is_ok()
        );
        assert!(
            validate_semantics(
                "bs_roformer_leap_xe90_vocals",
                &serde_json::json!({
                    "backend":"openvino_gpu",
                    "semantic_output":"guide_vocals"
                })
            )
            .is_err()
        );
        assert!(
            validate_semantics(
                "bs_roformer_leap_xe90_vocals",
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

    #[test]
    fn machine_progress_parser_accepts_only_real_bounded_chunks() {
        assert_eq!(parse_work_units("human log").unwrap(), None);
        assert_eq!(
            parse_work_units("UTA_WORK_UNITS v1 3 10").unwrap(),
            Some((3, 10))
        );
        for invalid in [
            "UTA_WORK_UNITS v1 0 10",
            "UTA_WORK_UNITS v1 11 10",
            "UTA_WORK_UNITS v1 1 0",
            "UTA_WORK_UNITS v1 1 10 extra",
        ] {
            assert!(parse_work_units(invalid).is_err(), "{invalid}");
        }
    }
}
