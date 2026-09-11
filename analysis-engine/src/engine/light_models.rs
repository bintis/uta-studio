//! One request-owned AMD queue for independent complete lightweight models.
//!
//! The queue is started only by Super acceleration. Tasks remain serial within
//! this physical-device lane; the Engine may overlap it with the Intel queue.

use super::*;

pub(super) struct LightModelContext {
    pub request: AnalyzeRequest,
    pub plan: EnginePlan,
    pub workflow: Option<WorkflowExecution>,
    pub resolved: Vec<uta_runtime_manager::ResolvedModel>,
    pub workflow_audio: BTreeMap<(String, String), (PathBuf, String)>,
    pub analysis_input: PathBuf,
    pub analysis_role: String,
    pub output_root: PathBuf,
    pub source_start: u64,
    pub source_duration: u64,
    pub cancellation: CancellationToken,
}

pub(super) struct LightModelOutput {
    pub pitch_evidence: Option<PitchEvidence>,
    pub pitch_artifact: Option<crate::contract::ArtifactRef>,
    pub shared_rmvpe_evidence_path: Option<PathBuf>,
    pub fcpe_evidence: Option<PitchEvidence>,
    pub basic_pitch_evidence: Option<BasicPitchEvidence>,
    pub game_evidence: Option<GameEvidence>,
    pub game_conditioned_boundary_count: usize,
    pub timed_note_evidence: Vec<TimedNoteExpertEvidence>,
    pub degraded_reasons: Vec<String>,
}

pub(super) fn run(context: LightModelContext) -> EngineResult<LightModelOutput> {
    let request = &context.request;
    let plan = &context.plan;
    let resolved = &context.resolved;
    let cancellation = &context.cancellation;
    let mut degraded_reasons = Vec::new();
    let mut pitch_artifact = None;
    let mut shared_rmvpe_evidence_path = None;

    let pitch_evidence = if has_capability(plan, "pitch.track") {
        let (input, _) = workflow_bound_audio(
            plan.workflow_execution.as_ref(),
            "pitch.track",
            &context.workflow_audio,
            &context.analysis_input,
            &context.analysis_role,
        )?;
        let model = resolved_model(resolved, "rmvpe")?;
        let directory = create_task_dir(&context.output_root, "worker/rmvpe")?;
        let (component, config) = pitch_dispatch(model, request)?;
        let outputs = run_native_task(
            model,
            component,
            "task-rmvpe",
            "pitch.track",
            &input,
            &directory,
            config,
            cancellation,
        )?;
        let worker_evidence = typed_worker_output(&outputs, "pitch_evidence")?;
        let pitch = parse_rmvpe_pitch(
            worker_evidence,
            context.source_start,
            context.source_duration,
        )?;
        shared_rmvpe_evidence_path = Some(worker_evidence.to_path_buf());
        if request.requested_artifacts.pitch_evidence {
            pitch_artifact = Some(write_json_artifact(
                &context.output_root,
                Path::new("pitch/pitch-evidence.json"),
                PITCH_MEDIA_TYPE,
                &pitch,
            )?);
        }
        Some(pitch)
    } else {
        None
    };

    let fcpe_evidence = if has_capability(plan, "pitch.secondary.fcpe") {
        if let Some(model) = resolved.iter().find(|model| model.model_id == "fcpe") {
            let (input, _) = workflow_bound_audio(
                plan.workflow_execution.as_ref(),
                "pitch.secondary.fcpe",
                &context.workflow_audio,
                &context.analysis_input,
                &context.analysis_role,
            )?;
            let directory = create_task_dir(&context.output_root, "worker/fcpe")?;
            let result = (|| {
                let (component, config) = pitch_dispatch(model, request)?;
                let outputs = run_native_task(
                    model,
                    component,
                    "task-fcpe",
                    "pitch.secondary.fcpe",
                    &input,
                    &directory,
                    config,
                    cancellation,
                )?;
                parse_fcpe_pitch(
                    typed_worker_output(&outputs, "pitch_evidence")?,
                    context.source_start,
                    context.source_duration,
                )
            })();
            match result {
                Ok(evidence) => Some(evidence),
                Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                Err(error) => {
                    degraded_reasons.push(format!(
                        "optional capability pitch.secondary.fcpe failed: {}",
                        error.message
                    ));
                    None
                }
            }
        } else {
            degraded_reasons.push(
                "optional capability pitch.secondary.fcpe skipped: model was not resolved"
                    .to_string(),
            );
            None
        }
    } else {
        None
    };

    let basic_pitch_evidence = if has_capability(plan, "notes.basic_pitch") {
        let (input, _) = workflow_bound_audio(
            plan.workflow_execution.as_ref(),
            "notes.basic_pitch",
            &context.workflow_audio,
            &context.analysis_input,
            &context.analysis_role,
        )?;
        let model = resolved_model(resolved, "basic_pitch")?;
        let directory = create_task_dir(&context.output_root, "worker/basic-pitch")?;
        let (component, config) = model_dispatch(model, request, "note+onset+contour_activation")?;
        let outputs = run_native_task(
            model,
            component,
            &format!("{}-basic-pitch", request.request_id),
            "notes.basic_pitch",
            &input,
            &directory,
            config,
            cancellation,
        )?;
        Some(parse_basic_pitch_evidence(
            typed_worker_output(&outputs, "basic_pitch_evidence")?,
            context.source_start,
            context.source_duration,
        )?)
    } else {
        None
    };

    let game_models = resolved
        .iter()
        .filter(|model| {
            matches!(
                model.model_id.as_str(),
                "game_1_0_3_small" | "game_1_0_3_medium" | "game_1_0_3_large"
            )
        })
        .collect::<Vec<_>>();
    if game_models.len() > 1 {
        return Err(EngineError::new(
            EngineErrorCode::InvalidContract,
            "workflow must select exactly one immutable GAME resource",
        )
        .with_capability("notes.game"));
    }
    let mut game_conditioned_boundary_count = 0_usize;
    let game_evidence = if has_capability(plan, "notes.game") {
        let model = game_models.first().copied().ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::RuntimeResolutionFailed,
                "planned GAME expert was not resolved",
            )
            .with_capability("notes.game")
        })?;
        let (input, _) = workflow_bound_audio(
            plan.workflow_execution.as_ref(),
            "notes.game",
            &context.workflow_audio,
            &context.analysis_input,
            &context.analysis_role,
        )?;
        let source_end = context.source_start.saturating_add(context.source_duration);
        let mut known_boundaries = request
            .boundary_constraints
            .iter()
            .filter(|constraint| constraint.authority == BoundaryAuthority::Hard)
            .flat_map(|constraint| [constraint.start, constraint.end().unwrap_or(u64::MAX)])
            .filter(|time| (context.source_start..=source_end).contains(time))
            .map(|time| time - context.source_start)
            .collect::<Vec<_>>();
        known_boundaries.sort_unstable();
        known_boundaries.dedup();
        game_conditioned_boundary_count = known_boundaries.len();
        let directory = create_task_dir(&context.output_root, "worker/game")?;
        let (component, mut config) = model_dispatch(model, request, "note_candidate_evidence")?;
        config["language"] = serde_json::json!(request.lyrics.language);
        config["known_boundaries_us"] = serde_json::json!(known_boundaries);
        let outputs = run_native_task(
            model,
            component,
            &format!("{}-{}", request.request_id, model.model_id),
            "notes.game",
            &input,
            &directory,
            config,
            cancellation,
        )?;
        Some(parse_game_evidence(
            typed_worker_output(&outputs, "game_evidence")?,
            context.source_start,
            context.source_duration,
        )?)
    } else {
        None
    };

    let mut timed_note_evidence = Vec::new();
    if has_capability(plan, "notes.jbm555") {
        let mix = request
            .audio_sources
            .iter()
            .find(|source| source.role == crate::contract::AudioRole::OriginalMix)
            .ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "JBM555 requires the original mix input",
                )
                .with_capability("notes.jbm555")
            })?;
        let (vocal, _) = workflow_bound_audio(
            plan.workflow_execution.as_ref(),
            "notes.jbm555",
            &context.workflow_audio,
            &context.analysis_input,
            &context.analysis_role,
        )?;
        let model = resolved_model(resolved, "jbm555_cectc_80")?;
        let primary = request.primary_source()?;
        let mix_audio_identity = format!("source:{}", mix.sha256);
        let vocal_audio_identity = format!(
            "analysis:{}:{}:{}",
            primary.sha256,
            context.analysis_role,
            plan.source_route
                .preparation
                .iter()
                .map(|capability| capability.as_str())
                .collect::<Vec<_>>()
                .join("+")
        );
        let separator_model_generation = plan
            .source_route
            .preparation
            .iter()
            .any(|capability| capability.as_str() == "audio.extract_vocals")
            .then(|| {
                context
                    .workflow
                    .as_ref()
                    .and_then(|workflow| {
                        workflow.model_for_engine_capability("audio.extract_vocals")
                    })
                    .unwrap_or("bs_roformer_leap_xe90_vocals")
            })
            .and_then(|model_id| {
                resolved
                    .iter()
                    .find(|resolved| resolved.model_id == model_id)
                    .map(|resolved| resolved.generation.clone())
            })
            .unwrap_or_else(|| "caller-supplied-vocal".to_string());
        let vocal_preparation_generation = resolved
            .iter()
            .filter(|resolved| {
                matches!(
                    resolved.model_id.as_str(),
                    "melband_roformer_harmony"
                        | "melband_roformer_denoise_aufr33"
                        | "melband_roformer_dereverb_anvuew"
                )
            })
            .map(|resolved| format!("{}@{}", resolved.model_id, resolved.generation))
            .collect::<Vec<_>>()
            .join("+");
        let vocal_preparation_generation = if vocal_preparation_generation.is_empty() {
            "analysis-ready-lead".to_string()
        } else {
            vocal_preparation_generation
        };
        let expected = Jbm555ExpectedInputs {
            source_start: context.source_start,
            source_duration: context.source_duration,
            mix_audio_identity: &mix_audio_identity,
            vocal_audio_identity: &vocal_audio_identity,
            separator_model_generation: &separator_model_generation,
            vocal_preparation_generation: &vocal_preparation_generation,
        };
        let directory = create_task_dir(&context.output_root, "worker/jbm555")?;
        let (component, mut config) = model_dispatch(model, request, "note_candidate_evidence")?;
        config["source_start"] = serde_json::json!(context.source_start);
        config["source_duration"] = serde_json::json!(context.source_duration);
        config["upstream_revision"] = serde_json::json!("jbm555-public");
        config["checkpoint_identity"] = serde_json::json!(model.model_content_digest);
        config["config_identity"] = serde_json::json!("cectc80-public");
        config["conversion_identity"] = serde_json::json!("gguf-f32");
        config["model_generation"] = serde_json::json!(model.generation);
        config["mix_audio_identity"] = serde_json::json!(mix_audio_identity);
        config["vocal_audio_identity"] = serde_json::json!(vocal_audio_identity);
        config["separator_model_generation"] = serde_json::json!(separator_model_generation);
        config["vocal_preparation_generation"] = serde_json::json!(vocal_preparation_generation);
        let outputs = run_native_task_with_inputs(
            model,
            component,
            &format!("{}-jbm555", request.request_id),
            "notes.jbm555",
            &[mix.path.clone(), vocal],
            &directory,
            config,
            cancellation,
        )?;
        let evidence =
            parse_jbm555_evidence(typed_worker_output(&outputs, "jbm555_evidence")?, expected)?;
        timed_note_evidence.push(evidence.timed_note_evidence(expected)?);
    }

    Ok(LightModelOutput {
        pitch_evidence,
        pitch_artifact,
        shared_rmvpe_evidence_path,
        fcpe_evidence,
        basic_pitch_evidence,
        game_evidence,
        game_conditioned_boundary_count,
        timed_note_evidence,
        degraded_reasons,
    })
}
