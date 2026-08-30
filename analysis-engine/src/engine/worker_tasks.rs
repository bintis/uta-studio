// Copyright 2026 Uta! Studio contributors
// Licensed under the Apache License, Version 2.0.

//! Shared native-worker execution and typed-output validation.

use super::*;

pub(super) fn run_openvino_cleanup(
    task: &DenoiseTask<'_>,
    spec: &CleanupSpec<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    let directory = create_task_dir(task.output_root, spec.worker_directory)?;
    let outputs = SupervisedWorker::run(
        task.executable,
        &WorkerExpectation {
            component: roformer_component(task.backend).to_string(),
            runtime_recipe_digest: task.runtime_recipe_digest.map(str::to_string),
        },
        &NativeTask {
            task_id: task.task_id.to_string(),
            node_id: spec.node_id.to_string(),
            presentation_node_id: spec.presentation_node_id.map(str::to_string),
            model_id: spec.model_id.to_string(),
            input_artifacts: vec![task.input.to_path_buf()],
            output_dir: directory.clone(),
            config: serde_json::json!({
                "model_path": task.model_path,
                "backend": task.backend,
                "semantic_output": spec.semantic_output
            }),
            timeout: Duration::from_secs(4 * 60 * 60),
        },
        cancellation,
        |_| {},
    )?;
    let worker_output = typed_worker_output(&outputs, spec.artifact)?;
    if outputs
        .iter()
        .find(|output| output.artifact == spec.artifact)
        .is_none_or(|output| output.media_type != "audio/flac")
    {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "MelBand worker did not publish its declared lossless FLAC stem",
        ));
    }
    let facts = decode_audio(task.ffmpeg, spec.node_id, worker_output)?.facts;
    if facts.sample_rate != 44_100
        || facts.channels != 2
        || facts.frame_count == 0
        || facts.duration.abs_diff(task.source_duration) > 2_000
    {
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "MelBand separation did not preserve the 44.1 kHz stereo source timeline",
        ));
    }
    let relative = PathBuf::from(spec.destination);
    let destination = task.output_root.join(&relative);
    let parent = destination.parent().expect("cleanup stem has parent");
    std::fs::create_dir_all(parent).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not create cleanup stem directory: {error}"),
        )
    })?;
    if destination.exists() {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "cleanup stem target already exists",
        ));
    }
    std::fs::rename(worker_output, &destination).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not atomically publish cleanup stem: {error}"),
        )
    })?;
    std::fs::remove_dir_all(&directory).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not clean cleanup worker directory: {error}"),
        )
    })?;
    Ok(SeparationOutput {
        role: spec.role,
        artifact: artifact_ref_for_existing(task.output_root, &relative, "audio/flac")?,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_native_task(
    model: &uta_runtime_manager::ResolvedModel,
    component: &str,
    task_id: &str,
    node_id: &str,
    input: &Path,
    output_dir: &Path,
    config: serde_json::Value,
    cancellation: &CancellationToken,
) -> EngineResult<Vec<NativeTaskOutput>> {
    SupervisedWorker::run(
        &model.runtime_executable,
        &WorkerExpectation {
            component: component.to_string(),
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
        &NativeTask {
            task_id: task_id.to_string(),
            node_id: node_id.to_string(),
            presentation_node_id: None,
            model_id: model.model_id.clone(),
            input_artifacts: vec![input.to_path_buf()],
            output_dir: output_dir.to_path_buf(),
            config,
            timeout: Duration::from_secs(4 * 60 * 60),
        },
        cancellation,
        |_| {},
    )
}

pub(super) fn typed_worker_output<'a>(
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
