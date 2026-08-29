use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::artifact::artifact_ref_for_existing;
use crate::audio::decode_audio;
use crate::contract::{AudioRole, EngineError, EngineErrorCode, EngineResult};
use crate::execution::{
    CancellationToken, NativeTask, NativeTaskOutput, SupervisedWorker, WorkerExpectation,
};
use crate::separation::SeparationOutput;

pub(crate) struct DenoiseTask<'a> {
    pub model_path: &'a Path,
    pub executable: &'a Path,
    pub runtime_recipe_digest: Option<&'a str>,
    pub ffmpeg: &'a Path,
    pub input: &'a Path,
    pub output_root: &'a Path,
    pub source_duration: u64,
    pub task_id: &'a str,
}

pub(crate) fn run_openvino_denoise(
    task: &DenoiseTask<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    let directory = task.output_root.join("worker/denoise");
    if directory.exists() {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "Denoise task directory already exists",
        ));
    }
    std::fs::create_dir_all(&directory).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not create Denoise task directory: {error}"),
        )
    })?;
    let outputs = SupervisedWorker::run(
        task.executable,
        &WorkerExpectation {
            component: "uta-openvino-worker".to_string(),
            runtime_recipe_digest: task.runtime_recipe_digest.map(str::to_string),
        },
        &NativeTask {
            task_id: task.task_id.to_string(),
            node_id: "audio.denoise".to_string(),
            presentation_node_id: None,
            model_id: "melband_roformer_denoise_aufr33".to_string(),
            input_artifacts: vec![task.input.to_path_buf()],
            output_dir: directory.clone(),
            config: serde_json::json!({
                "model_path": task.model_path,
                "backend": "openvino_gpu",
                "semantic_output": "dry"
            }),
            timeout: Duration::from_secs(4 * 60 * 60),
        },
        cancellation,
        |_| {},
    )?;
    let worker_output = typed_worker_output(&outputs, "clean_lead_vocal")?;
    if outputs
        .iter()
        .find(|output| output.artifact == "clean_lead_vocal")
        .is_none_or(|output| output.media_type != "audio/flac")
    {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "Denoise worker did not publish a lossless FLAC dry stem",
        ));
    }
    let facts = decode_audio(task.ffmpeg, "denoise-dry", worker_output)?.facts;
    if facts.sample_rate != 44_100
        || facts.channels != 2
        || facts.frame_count == 0
        || facts.duration.abs_diff(task.source_duration) > 2_000
    {
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "Denoise dry stem did not preserve the 44.1 kHz stereo source timeline",
        ));
    }
    let relative = PathBuf::from("stems/clean_lead_vocal.flac");
    let destination = task.output_root.join(&relative);
    let parent = destination.parent().expect("Denoise stem has parent");
    std::fs::create_dir_all(parent).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not create Denoise stem directory: {error}"),
        )
    })?;
    if destination.exists() {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "Denoise stem target already exists",
        ));
    }
    std::fs::rename(worker_output, &destination).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not atomically publish Denoise dry stem: {error}"),
        )
    })?;
    std::fs::remove_dir_all(&directory).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not clean Denoise worker directory: {error}"),
        )
    })?;
    Ok(SeparationOutput {
        role: AudioRole::CleanLeadVocal,
        artifact: artifact_ref_for_existing(task.output_root, &relative, "audio/flac")?,
    })
}

fn typed_worker_output<'a>(
    outputs: &'a [NativeTaskOutput],
    artifact: &str,
) -> EngineResult<&'a Path> {
    let mut matching = outputs.iter().filter(|output| output.artifact == artifact);
    let output = matching.next().ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("worker omitted required typed output {artifact}"),
        )
    })?;
    if matching.next().is_some() {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("worker emitted duplicate typed output {artifact}"),
        ));
    }
    Ok(&output.path)
}
