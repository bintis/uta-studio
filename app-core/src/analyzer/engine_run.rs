use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

use super::*;
use crate::analysis_artifact::{
    ArtifactRevision, ArtifactStore, materialize_artifact_revision_compatibility, revision_to_row,
};
use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
use crate::backend_cli::{
    ANALYSIS_RESULT_CONTRACT, ANALYSIS_RESULT_VERSION, AUDIO_QUALITY_REPORT_CONTRACT,
    AUDIO_QUALITY_REPORT_VERSION, AnalysisCliClient, AnalysisLifecycleFrameWire, AnalysisPlanWire,
    AnalysisResultManifestWire, AnalysisReusePolicyWire, AnalysisStatusWire, AnalyzeRequestWire,
    ArtifactRefWire, AudioQualityReportWire, AudioRoleWire, BackendCliError,
    FusionDecisionProvenanceWire, FusionModeWire, QualityGateRequirementWire,
    QualityGateStatusWire, QualityRegionWire, VocalTopologyModeWire,
};
use crate::library_db::EngineQueueIntent;

const SOURCE_DURATION_METADATA_TOLERANCE: u64 = 100_000;
const SUPPORTED_AUDIO_QUALITY_ALGORITHMS: [&str; 1] = ["audio-quality-gates"];

pub(crate) fn process_engine_queue_intent(
    file_hash: &str,
    cache: &CacheDir,
    intent: EngineQueueIntent,
) {
    let started_at_ms = unix_time_ms();
    ANALYSIS_STARTED
        .lock()
        .unwrap()
        .insert(file_hash.to_string(), started_at_ms);
    let log_path = create_analysis_log(file_hash, started_at_ms);
    let engine_projection = EngineRunHistoryProjection {
        request_id: intent.request_id.clone(),
        request_json: intent.request_json.clone(),
        request_digest: intent.request_digest.clone(),
        plan_json: intent.plan_json.clone(),
        result_json: None,
        fingerprint: None,
        source_sha256: intent.source_sha256.clone(),
    };
    LIVE_ANALYSIS.lock().unwrap().insert(
        file_hash.to_string(),
        AnalysisProgressSnapshot {
            stage: "engine".to_string(),
            overall_progress: 0,
            stage_progress: 0,
            operation: "Starting Analysis Engine".to_string(),
            detail: "Executing the exact request confirmed in Plan Preview.".to_string(),
            implementation: "uta-analyze process protocol".to_string(),
            model: "Engine plan".to_string(),
            device: "Resolved by Engine".to_string(),
            requested_device: "Production policy".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            stage_routes: Vec::new(),
            node_id: None,
            engine_node_id: None,
            capability_id: None,
            node_event: Some("started".to_string()),
            artifact_reused_reason: None,
            analysis_log_path: log_path.clone(),
            engine: Some(engine_projection),
            engine_error: None,
        },
    );
    update_queue_status(file_hash, QueuedStatus::Analyzing(0));

    let result = execute_exact_intent(file_hash, cache, &intent, log_path.as_deref());
    match result {
        Ok(manifest) => {
            if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(file_hash) {
                snapshot.stage = "complete".to_string();
                snapshot.overall_progress = 100;
                snapshot.stage_progress = 100;
                snapshot.operation = "Analysis complete".to_string();
                snapshot.detail =
                    "Engine outputs were validated and published atomically.".to_string();
                snapshot.node_event = Some("completed".to_string());
                if let Some(engine) = snapshot.engine.as_mut() {
                    engine.result_json = serde_json::to_string(&manifest).ok();
                    engine.fingerprint = Some(manifest.fingerprint.clone());
                }
            }
            append_analysis_log_path(log_path.as_deref(), "Engine result validated and published");
            finish_analysis_history(file_hash, "completed", None);
            update_queue_status(file_hash, QueuedStatus::Completed);
            remove_engine_progress_plan(file_hash);
            LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
        }
        Err(error) => {
            append_analysis_log_path(log_path.as_deref(), &error);
            let cancelled = error.starts_with("cancelled:");
            if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(file_hash) {
                snapshot.detail = snapshot.engine_error.as_ref().map_or_else(
                    || error.clone(),
                    |structured| {
                        format!(
                            "{}: {}{}{}",
                            structured.code,
                            structured.message,
                            structured
                                .capability
                                .as_deref()
                                .map(|value| format!(" · capability {value}"))
                                .unwrap_or_default(),
                            structured
                                .resource
                                .as_deref()
                                .map(|value| format!(" · resource {value}"))
                                .unwrap_or_default()
                        )
                    },
                );
                if cancelled {
                    mark_snapshot_cancelled(snapshot, &error);
                } else {
                    snapshot.node_event = Some("failed".to_string());
                }
            }
            if cancelled {
                finish_analysis_history(file_hash, "cancelled", Some(&error));
                remove_from_queue(file_hash);
            } else {
                finish_analysis_history(file_hash, "failed", Some(&error));
                update_queue_status(file_hash, QueuedStatus::Failed(error));
            }
            remove_engine_progress_plan(file_hash);
            LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
        }
    }
}

fn execute_exact_intent(
    file_hash: &str,
    cache: &CacheDir,
    intent: &EngineQueueIntent,
    log_path: Option<&Path>,
) -> Result<AnalysisResultManifestWire, String> {
    if take_force_stop_request(file_hash) {
        return Err("cancelled: analysis was force-stopped".to_string());
    }
    let request: AnalyzeRequestWire = serde_json::from_str(&intent.request_json)
        .map_err(|error| format!("persisted Engine request is malformed: {error}"))?;
    let plan: AnalysisPlanWire = serde_json::from_str(&intent.plan_json)
        .map_err(|error| format!("persisted Engine plan is malformed: {error}"))?;
    if intent.file_hash != file_hash
        || request.request_id != intent.request_id
        || plan.request_id != intent.request_id
    {
        return Err("persisted Engine request identity is inconsistent".to_string());
    }
    crate::analysis_engine_adapter::validate_workflow_plan_identity(&request, &plan)?;
    if !valid_request_id(&intent.request_id)
        || request.contract != crate::backend_cli::ANALYZE_REQUEST_CONTRACT
        || request.version != crate::backend_cli::ANALYZE_REQUEST_VERSION
        || plan.schema != "uta.analysis-engine.plan"
        || plan.schema_version != 1
        || plan.requirements.schema != "uta.runtime.requirements"
        || plan.requirements.schema_version != 1
    {
        return Err("persisted Engine request or plan contract is unsupported".to_string());
    }
    register_engine_progress_plan(file_hash, &plan);
    let source = crate::analysis_engine_adapter::resolve_true_source(file_hash)?;
    let expected_source_duration = app_owned_source_duration(file_hash)?;
    validate_exact_execution_source_binding(&request, &plan, &source.path, &intent.source_path)?;

    let runs_root = cache.path.join("engine-runs");
    std::fs::create_dir_all(&runs_root)
        .map_err(|error| format!("could not create Engine runs root: {error}"))?;
    let output_root = runs_root.join(&intent.request_id);
    if output_root.exists() {
        // A retry reuses the persisted request_id, so a prior attempt that
        // crashed (rather than reaching a terminal outcome) can leave this
        // directory behind. Clear it instead of failing the retry outright.
        std::fs::remove_dir_all(&output_root)
            .map_err(|error| format!("could not clear stale Engine output root: {error}"))?;
    }
    std::fs::create_dir(&output_root)
        .map_err(|error| format!("could not create unique Engine output root: {error}"))?;
    let request_value =
        serde_json::from_str(&intent.request_json).map_err(|error| error.to_string())?;
    let outcome = (|| {
        let mut client = AnalysisCliClient::connect().map_err(|error| error.to_string())?;
        let cancellation = client.cancellation_handle();
        register_active_engine(file_hash, cancellation.clone());
        let analysis = if force_stop_was_requested(file_hash) {
            cancellation.force_stop().and_then(|()| {
                Err(BackendCliError::UnexpectedExit(
                    "force-stopped before analysis execution".to_string(),
                ))
            })
        } else {
            client.analyze_with_events(&request_value, &intent.request_id, &output_root, |event| {
                apply_engine_lifecycle_event(
                    file_hash,
                    log_path,
                    event,
                    &request.execution_policy.model_settings,
                )
            })
        };
        remove_active_engine(file_hash);
        let stderr = client.stderr_log();
        if !stderr.is_empty() {
            append_analysis_log_path(log_path, &format!("uta-analyze stderr: {stderr}"));
        }
        if take_force_stop_request(file_hash) {
            return Err("cancelled: analysis was force-stopped".to_string());
        }
        let manifest = analysis.map_err(|error| {
            preserve_engine_error(file_hash, &error);
            error.to_string()
        })?;
        validate_and_publish_engine_result(
            file_hash,
            cache,
            &output_root,
            expected_source_duration,
            &request,
            &plan,
            &manifest,
        )?;
        Ok(manifest)
    })();
    if outcome.is_ok() {
        let _ = std::fs::remove_dir_all(&output_root);
    }
    outcome
}

fn audio_role_name(role: AudioRoleWire) -> &'static str {
    match role {
        AudioRoleWire::OriginalMix => "original_mix",
        AudioRoleWire::VocalStem => "vocal_stem",
        AudioRoleWire::GuideVocals => "guide_vocals",
        AudioRoleWire::LeadVocal => "lead_vocal",
        AudioRoleWire::CleanLeadVocal => "clean_lead_vocal",
        AudioRoleWire::Instrumental => "instrumental",
        AudioRoleWire::BackingVocal => "backing_vocal",
        AudioRoleWire::HarmonyVocal => "harmony_vocal",
    }
}

fn evaluated_audio_role_matches_plan(plan: &AnalysisPlanWire, evaluated_role: &str) -> bool {
    let baseline_role = if plan
        .source_route
        .preparation
        .iter()
        .any(|capability| capability.as_str() == "audio.lead_isolate")
    {
        AudioRoleWire::LeadVocal
    } else if plan
        .source_route
        .preparation
        .iter()
        .any(|capability| capability.as_str() == "audio.extract_vocals")
    {
        AudioRoleWire::GuideVocals
    } else {
        plan.source_route.input_role
    };
    evaluated_role == audio_role_name(baseline_role)
        || (matches!(
            baseline_role,
            AudioRoleWire::VocalStem
                | AudioRoleWire::GuideVocals
                | AudioRoleWire::LeadVocal
                | AudioRoleWire::CleanLeadVocal
        ) && workflow_analysis_route_uses_cleanup(plan)
            && evaluated_role == audio_role_name(AudioRoleWire::CleanLeadVocal))
}

fn workflow_analysis_route_uses_cleanup(plan: &AnalysisPlanWire) -> bool {
    let Some(workflow) = plan.workflow_execution.as_ref() else {
        return plan
            .execution_nodes
            .iter()
            .any(|node| matches!(node.capability.as_str(), "audio.denoise" | "audio.dereverb"));
    };
    let analyzer_roots = workflow.nodes.iter().flat_map(|node| {
        node.input_bindings.iter().filter_map(|binding| {
            (node.execution_state == crate::backend_cli::WorkflowNodeExecutionStateWire::Ready
                && binding.execution_active
                && binding.analyzer_attachment
                && binding.semantic_type == "audio")
                .then_some(binding.from_node.as_str())
        })
    });
    let terminal_audio_roots = workflow
        .terminal_outputs
        .iter()
        .filter(|output| output.semantic_type == "audio")
        .map(|output| output.node.as_str());
    analyzer_roots
        .chain(terminal_audio_roots)
        .any(|node| workflow_path_uses_cleanup(workflow, node, &mut BTreeSet::new()))
}

fn workflow_path_uses_cleanup(
    workflow: &crate::backend_cli::WorkflowExecutionPlanWire,
    analysis_node: &str,
    visited: &mut BTreeSet<String>,
) -> bool {
    if !visited.insert(analysis_node.to_string()) {
        return false;
    }
    let Some(node) = workflow
        .nodes
        .iter()
        .find(|node| node.analysis_node == analysis_node)
    else {
        return false;
    };
    if node.execution_state != crate::backend_cli::WorkflowNodeExecutionStateWire::Ready {
        return false;
    }
    if node
        .capabilities
        .iter()
        .any(|capability| matches!(capability.as_str(), "audio.denoise" | "audio.dereverb"))
    {
        return true;
    }
    node.input_bindings.iter().any(|binding| {
        binding.execution_active
            && !binding.analyzer_attachment
            && binding.semantic_type == "audio"
            && workflow_path_uses_cleanup(workflow, &binding.from_node, visited)
    })
}

fn app_owned_source_duration(file_hash: &str) -> Result<u64, String> {
    let song = crate::library_db::load_song_by_hash(file_hash)
        .map_err(|error| format!("could not load app-owned source duration: {error}"))?
        .ok_or_else(|| "app-owned source duration is unavailable".to_string())?;
    let duration = song.duration_secs * f64::from(crate::backend_cli::CANONICAL_TIMEBASE);
    if !duration.is_finite() || duration < 1.0 || duration >= u64::MAX as f64 {
        return Err("app-owned source duration is invalid".to_string());
    }
    Ok(duration.round() as u64)
}

fn preserve_engine_error(file_hash: &str, error: &BackendCliError) {
    let BackendCliError::Domain {
        code,
        message,
        retryable,
        request_id,
        capability,
        resource,
    } = error
    else {
        return;
    };
    if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(file_hash) {
        snapshot.engine_error = Some(EngineErrorHistoryProjection {
            code: code.clone(),
            message: message.clone(),
            retryable: *retryable,
            request_id: request_id.clone(),
            capability: capability.clone(),
            resource: resource.clone(),
        });
    }
}

fn apply_engine_lifecycle_event(
    file_hash: &str,
    log_path: Option<&Path>,
    event: AnalysisLifecycleFrameWire,
    settings: &uta_model_settings::ModelSettings,
) {
    append_analysis_lifecycle_log(log_path, &event);
    // Cache a Step 1 audio-chain stem the instant its own worker succeeds,
    // before the lock below and independent of whether this run's later
    // stages ultimately fail -- see `persist_cacheable_stem`'s doc comment
    // for why that independence is the entire point. Never touch the live
    // `LIVE_ANALYSIS` lock for this: capturing/hashing a stem file is real
    // I/O and must not block every other snapshot read/write on its
    // duration.
    if event.frame_type == "artifact"
        && let (Some(artifact), Some(path)) = (event.artifact.as_deref(), event.path.as_deref())
        && let Some(cache) = CacheDir::try_new()
    {
        crate::chain_cache::persist_cacheable_stem(
            &cache.path,
            file_hash,
            artifact,
            Path::new(path),
            settings,
        );
    }
    let weighted_overall = update_engine_overall_progress(file_hash, &event);
    let presentation_node_id = event
        .presentation_node_id
        .clone()
        .unwrap_or_else(|| event.node_id.clone());
    let message = event
        .message
        .clone()
        .unwrap_or_else(|| event.capability_id.clone());
    let model = event
        .model_id
        .clone()
        .unwrap_or_else(|| "Engine native".to_string());
    // A visible node percentage is exact only when the worker supplies real
    // completed/total units tied to a task identity. Unitless fractions remain
    // in the bounded lifecycle log and whole-run estimate, but the node card
    // must stay indeterminate.
    let reported_progress = event
        .work_units_completed
        .zip(event.work_units_total)
        .filter(|(completed, total)| {
            *total > 0
                && completed <= total
                && event
                    .worker_task_id
                    .as_deref()
                    .is_some_and(|task_id| !task_id.trim().is_empty())
        })
        .map(|(completed, total)| (completed.saturating_mul(100) / total).min(100) as usize);
    let terminal = matches!(event.frame_type.as_str(), "node_completed" | "node_failed");
    let started = matches!(
        event.frame_type.as_str(),
        "node_started" | "node_progress" | "artifact"
    );

    let mut live = LIVE_ANALYSIS.lock().unwrap();
    let Some(snapshot) = live.get_mut(file_hash) else {
        return;
    };
    if matches!(event.frame_type.as_str(), "warning" | "degraded") {
        snapshot.detail = message;
        snapshot.node_event = Some(event.frame_type);
        return;
    }
    let previous_overall = snapshot.overall_progress;
    let previous_measured_progress = snapshot
        .stage_routes
        .iter()
        .rev()
        .find(|route| {
            route.node_id.as_deref() == Some(presentation_node_id.as_str())
                && route.engine_node_id.as_deref() == Some(event.node_id.as_str())
                && route.node_event.as_deref() == Some("node_progress")
        })
        .map(|route| route.stage_progress);
    snapshot.stage = event.capability_id.clone();
    snapshot.stage_progress = if event.frame_type == "node_completed" {
        100
    } else if event.frame_type == "node_progress" {
        reported_progress.unwrap_or(0)
    } else if event.frame_type == "node_started" {
        0
    } else {
        previous_measured_progress.unwrap_or(0)
    };
    snapshot.operation = message.clone();
    snapshot.detail = format!("{} · {}", event.capability_id, model);
    snapshot.implementation = event.implementation.clone();
    snapshot.model = model.clone();
    snapshot.device = "Engine-resolved; see Plan/Result provenance".to_string();
    snapshot.requested_device = "Production policy".to_string();
    snapshot.node_id = Some(presentation_node_id.clone());
    snapshot.engine_node_id = Some(event.node_id.clone());
    snapshot.capability_id = Some(event.capability_id.clone());
    snapshot.node_event = Some(event.frame_type.clone());

    let route_exists = snapshot.stage_routes.iter().any(|route| {
        route.node_id.as_deref() == Some(presentation_node_id.as_str())
            && route.engine_node_id.as_deref() == Some(event.node_id.as_str())
    });
    if !route_exists {
        snapshot.stage_routes.push(AnalysisStageRoute {
            stage: event.capability_id.clone(),
            node_id: Some(presentation_node_id),
            engine_node_id: Some(event.node_id),
            capability_id: Some(event.capability_id),
            node_event: None,
            binding_kind: None,
            committed_outputs: Vec::new(),
            input_revision_ids: Vec::new(),
            operation: message,
            implementation: event.implementation,
            model,
            stage_progress: 0,
            requested_device: "Production policy".to_string(),
            actual_device: "Engine-resolved".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms: None,
            finished_at_ms: None,
            event_at_ms: None,
            work_units_completed: None,
            work_units_total: None,
            worker_task_id: None,
        });
    }
    let current_node_id = snapshot.node_id.clone();
    let current_engine_node_id = snapshot.engine_node_id.clone();
    let route = snapshot
        .stage_routes
        .iter_mut()
        .find(|route| {
            route.node_id == current_node_id && route.engine_node_id == current_engine_node_id
        })
        .expect("the lifecycle route was found or inserted");
    if event.frame_type != "artifact" {
        route.node_event = Some(event.frame_type.clone());
    }
    route.operation = snapshot.operation.clone();
    route.implementation = snapshot.implementation.clone();
    route.model = snapshot.model.clone();
    route.stage_progress = snapshot.stage_progress;
    route.event_at_ms = Some(event.event_at_ms);
    if event.frame_type == "node_progress" {
        route.work_units_completed = event.work_units_completed;
        route.work_units_total = event.work_units_total;
        route.worker_task_id = event.worker_task_id;
    }
    if started && route.started_at_ms.is_none() {
        route.started_at_ms = Some(event.event_at_ms);
    }
    if terminal {
        route.finished_at_ms = Some(event.event_at_ms);
    }
    if let Some(weighted_overall) = weighted_overall {
        snapshot.overall_progress = snapshot.overall_progress.max(weighted_overall);
    }
    let overall_changed = snapshot.overall_progress != previous_overall;
    let overall_progress = snapshot.overall_progress;
    drop(live);
    if overall_changed {
        update_queue_status(file_hash, QueuedStatus::Analyzing(overall_progress));
    }
}

fn validate_and_publish_engine_result(
    file_hash: &str,
    cache: &CacheDir,
    output_root: &Path,
    expected_source_duration: u64,
    request: &AnalyzeRequestWire,
    plan: &AnalysisPlanWire,
    manifest: &AnalysisResultManifestWire,
) -> Result<(), String> {
    if manifest.contract != ANALYSIS_RESULT_CONTRACT || manifest.version != ANALYSIS_RESULT_VERSION
    {
        return Err("Engine result contract identity is invalid".to_string());
    }
    if manifest.request_id != request.request_id || plan.request_id != request.request_id {
        return Err("Engine result request_id does not match the queued snapshot".to_string());
    }
    if !matches!(
        manifest.status,
        AnalysisStatusWire::Ok | AnalysisStatusWire::OkDegraded
    ) {
        return Err(format!(
            "Engine returned non-success status {:?}",
            manifest.status
        ));
    }
    if matches!(manifest.status, AnalysisStatusWire::Ok) && !manifest.degraded_reasons.is_empty() {
        return Err("Engine ok result unexpectedly contains degraded reasons".to_string());
    }
    if matches!(manifest.status, AnalysisStatusWire::OkDegraded)
        && manifest.degraded_reasons.is_empty()
    {
        return Err("Engine degraded result omitted degraded reasons".to_string());
    }
    for version in [
        &manifest.provenance.calibration_version,
        &manifest.provenance.fusion_version,
        &manifest.provenance.quantization_version,
        &manifest.provenance.audio_quality_version,
        &manifest.provenance.postprocess_version,
    ] {
        if version.trim().is_empty() {
            return Err("Engine result algorithm provenance is incomplete".to_string());
        }
    }
    validate_fusion_decision_result(plan, manifest)?;
    validate_quantization_result(request, manifest)?;
    validate_audio_quality_result(request, plan, manifest, expected_source_duration)?;
    for resource in &manifest.provenance.resources {
        for field in [
            "resource",
            "generation",
            "content_digest",
            "runtime",
            "runtime_generation",
            "backend",
            "device",
        ] {
            if resource
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Err(format!("Engine resource provenance omitted {field}"));
            }
        }
    }

    let declared = plan
        .artifact_declarations
        .iter()
        .map(|item| {
            (
                item.semantic_type.as_str(),
                item.media_type.as_str(),
                item.required,
            )
        })
        .collect::<Vec<_>>();
    let mut stem_roles = std::collections::BTreeSet::new();
    for stem in &manifest.artifacts.stems {
        if !stem_roles.insert(stem.role) {
            return Err("Engine result contains a duplicate stem role".to_string());
        }
        if !matches!(
            stem.role,
            AudioRoleWire::Instrumental
                | AudioRoleWire::GuideVocals
                | AudioRoleWire::LeadVocal
                | AudioRoleWire::CleanLeadVocal
                | AudioRoleWire::BackingVocal
                | AudioRoleWire::HarmonyVocal
        ) {
            return Err("Engine result contains an unsupported output stem role".to_string());
        }
    }
    let artifacts = result_artifacts(manifest);
    let mut actual_semantics = std::collections::BTreeSet::new();
    if artifacts
        .iter()
        .any(|(semantic, _, _, _)| !actual_semantics.insert(semantic.as_str()))
    {
        return Err("Engine result contains a duplicate artifact semantic".to_string());
    }
    for (semantic, _, required) in &declared {
        if *required && !artifacts.iter().any(|(actual, _, _, _)| actual == semantic) {
            return Err(format!(
                "Engine result omitted required artifact {semantic}"
            ));
        }
    }
    if artifacts
        .iter()
        .any(|(semantic, _, _, _)| !declared.iter().any(|(expected, _, _)| expected == semantic))
    {
        return Err("Engine result contains an undeclared artifact".to_string());
    }

    let output_root = output_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let chain_fingerprints: crate::chain_cache::ChainFingerprints = request
        .extensions
        .get(crate::chain_cache::CHAIN_FINGERPRINTS_EXTENSION_KEY)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    let store = ArtifactStore::new(&cache.path)?;
    let mut revisions = Vec::new();
    let mut activations = Vec::new();
    let mut complete_chart_revisions = Vec::new();
    let mut published_revisions = Vec::new();
    let created_at_ms = unix_time_ms();
    for (semantic, artifact, kind, producer) in artifacts {
        let expected_media = declared
            .iter()
            .find(|(name, _, _)| *name == semantic)
            .map(|(_, media, _)| *media)
            .ok_or_else(|| format!("missing declaration for {semantic}"))?;
        if artifact.media_type != expected_media {
            return Err(format!("Engine artifact {semantic} media type mismatch"));
        }
        let path = validate_artifact(&output_root, artifact)?;
        validate_semantic_artifact(&semantic, &path)?;
        let (immutable_path, content_hash, byte_size) = store.capture(file_hash, kind, &path)?;
        // The Step 1 chain cache (`chain_cache::plan_chain_cache`) matches a
        // future run's freshly computed fingerprint against exactly this
        // value, so these specific kinds record their own per-unit
        // fingerprint instead of the whole-manifest one every other kind
        // uses -- a downstream-only change must not invalidate an unrelated
        // upstream stage's cache eligibility.
        let chain_config_hash = match kind {
            ArtifactKind::VocalStem => chain_fingerprints.separation.clone(),
            ArtifactKind::InstrumentalStem => chain_fingerprints.instrumental.clone(),
            ArtifactKind::AnalysisVocalStem => chain_fingerprints.isolate.clone(),
            ArtifactKind::DereverbedVocalStem => chain_fingerprints.cleanup.clone(),
            ArtifactKind::DenoisedInstrumentalStem | ArtifactKind::DereverbedInstrumentalStem => {
                chain_fingerprints.cleanup.clone()
            }
            _ => None,
        };
        let config_hash =
            chain_config_hash.unwrap_or_else(|| format!("engine:{}", manifest.fingerprint));
        let revision = ArtifactRevision {
            id: format!("{file_hash}:{semantic}:{content_hash}"),
            file_hash: file_hash.to_string(),
            kind,
            path: immutable_path,
            content_hash,
            producer_node: AnalysisNodeId::new(producer),
            input_revisions: Vec::new(),
            config_hash,
            algorithm_version: format!("analysis-engine-result/{}", manifest.version),
            created_at_ms,
            byte_size,
            active: false,
            legacy: false,
            invalidated: false,
        };
        activations.push((
            file_hash.to_string(),
            serde_json::to_string(&kind).unwrap_or_default(),
            revision.id.clone(),
        ));
        if kind == ArtifactKind::CandidateChart {
            complete_chart_revisions.push(revision.clone());
        }
        revisions.push(revision_to_row(&revision));
        published_revisions.push(revision);
    }
    let analyzed_file_hashes = (!complete_chart_revisions.is_empty())
        .then(|| file_hash.to_string())
        .into_iter()
        .collect::<Vec<_>>();
    crate::library_db::analysis_artifacts_publish_batch(
        &revisions,
        &activations,
        &analyzed_file_hashes,
    )
    .map_err(|error| error.to_string())?;
    // Materialize every published revision, not just CandidateChart:
    // `refresh_authoring_state` (and
    // other legacy readiness checks) key off flat `{hash}_instrumental.*` /
    // `{hash}_vocals.*` files, not the content-addressed artifact store, so
    // skipping stem kinds here leaves completed songs "Analysis incomplete".
    // `compatibility_paths` already no-ops for kinds with no legacy
    // location, so this is safe to call unconditionally.
    for revision in &published_revisions {
        if let Err(error) = materialize_artifact_revision_compatibility(&cache.path, revision) {
            warn!(
                "[analyzer] Published {:?} {} but could not refresh compatibility output: {error}",
                revision.kind, revision.id
            );
        }
    }
    Ok(())
}

fn valid_decision_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_fusion_decision_result(
    plan: &AnalysisPlanWire,
    manifest: &AnalysisResultManifestWire,
) -> Result<(), String> {
    let candidate_graph_planned = plan
        .execution_nodes
        .iter()
        .any(|node| node.capability.as_str() == "fusion.candidate_graph");
    let Some(decision) = manifest.provenance.fusion_decision.as_ref() else {
        return if candidate_graph_planned {
            Err("Engine result omitted final fusion decision provenance".to_string())
        } else {
            Ok(())
        };
    };
    if !candidate_graph_planned {
        return Err("Engine result reported a fusion decision for an unplanned stage".to_string());
    }
    let planned_mode = plan
        .workflow_execution
        .as_ref()
        .map_or(FusionModeWire::Algorithm, |workflow| workflow.fusion_mode);
    let (candidate_set_digest, selected_candidate_ids) = match decision {
        FusionDecisionProvenanceWire::Algorithm {
            selector,
            selector_version,
            candidate_set_digest,
            selected_candidate_ids,
            reuse_policy,
        } => {
            if planned_mode != FusionModeWire::Algorithm
                || selector != "hsmm_viterbi"
                || selector_version != "hsmm"
                || *reuse_policy != AnalysisReusePolicyWire::Deterministic
            {
                return Err(
                    "Engine algorithmic fusion provenance does not match the exact plan"
                        .to_string(),
                );
            }
            (candidate_set_digest, selected_candidate_ids)
        }
        FusionDecisionProvenanceWire::AiJudgment {
            adapter_resource,
            adapter_protocol,
            adapter_protocol_version,
            adapter_identity,
            adapter_version,
            candidate_set_digest,
            selected_candidate_ids,
            response_digest,
            reuse_policy,
        } => {
            if planned_mode != FusionModeWire::AiJudgment
                || adapter_resource != "tool:fusion_agent_adapter"
                || adapter_protocol != "uta.fusion_agent_request/uta.fusion_agent_response"
                || *adapter_protocol_version != 4
                || adapter_identity.trim().is_empty()
                || adapter_version.trim().is_empty()
                || !valid_decision_digest(response_digest)
                || *reuse_policy != AnalysisReusePolicyWire::PreservedRevisionOnly
            {
                return Err(
                    "Engine AI judgment provenance does not match the exact plan and adapter contract"
                        .to_string(),
                );
            }
            (candidate_set_digest, selected_candidate_ids)
        }
    };
    let mut unique_ids = BTreeSet::new();
    if !valid_decision_digest(candidate_set_digest)
        || selected_candidate_ids.is_empty()
        || selected_candidate_ids
            .iter()
            .any(|id| id.trim().is_empty() || !unique_ids.insert(id))
    {
        return Err("Engine fusion decision candidate identity is invalid".to_string());
    }
    Ok(())
}

fn validate_audio_quality_result(
    request: &AnalyzeRequestWire,
    plan: &AnalysisPlanWire,
    manifest: &AnalysisResultManifestWire,
    expected_source_duration: u64,
) -> Result<(), String> {
    const GATE_ORDER: &[&str] = &[
        "timeline_valid",
        "finite_samples",
        "clipping",
        "silence_ratio",
        "energy_ratio",
        "lead_purity",
        "vocal_leakage",
        "musical_damage",
        "cleanup_consistency",
        "vocal_topology",
    ];
    let report = manifest.diagnostics.audio_quality.as_ref().ok_or_else(|| {
        "Engine omitted audio quality diagnostics for an executable Plan".to_string()
    })?;
    let primary = validated_primary_source_binding(request, plan)?;
    let source_start = primary.timeline.source_start;
    let source_end = source_start
        .checked_add(expected_source_duration)
        .ok_or_else(|| "App-owned source timeline overflows".to_string())?;
    let report_end = source_start
        .checked_add(report.duration)
        .ok_or_else(|| "Engine audio quality timeline overflows".to_string())?;
    if report.contract != AUDIO_QUALITY_REPORT_CONTRACT
        || report.version != AUDIO_QUALITY_REPORT_VERSION
        || !SUPPORTED_AUDIO_QUALITY_ALGORITHMS.contains(&report.algorithm.as_str())
        || report.algorithm != manifest.provenance.audio_quality_version
        || report.profile != request.analysis.profile
        || !evaluated_audio_role_matches_plan(plan, &report.evaluated_audio_role)
        || report.duration == 0
        || report.duration.abs_diff(expected_source_duration) > SOURCE_DURATION_METADATA_TOLERANCE
        || report.planned_gates != plan.quality_gates
        || report.outcomes.len() != plan.quality_gates.len()
    {
        return Err("Engine audio quality report identity or Plan binding is invalid".to_string());
    }
    validate_vocal_topology_result(request, plan, report, expected_source_duration)?;
    let mut previous_order = None;
    let mut degrading_uncertainty = false;
    for (planned, outcome) in plan.quality_gates.iter().zip(&report.outcomes) {
        let order = GATE_ORDER
            .iter()
            .position(|known| *known == planned)
            .ok_or_else(|| format!("Engine Plan contains unknown audio quality gate {planned}"))?;
        let expected_requirement = match planned.as_str() {
            "timeline_valid" | "finite_samples" | "silence_ratio" | "energy_ratio" => {
                QualityGateRequirementWire::Required
            }
            _ => QualityGateRequirementWire::Degrading,
        };
        if previous_order.is_some_and(|previous| order <= previous)
            || outcome.gate != *planned
            || outcome.requirement != expected_requirement
            || outcome.summary.trim().is_empty()
            || (expected_requirement == QualityGateRequirementWire::Required
                && outcome.status != QualityGateStatusWire::Passed)
        {
            return Err("Engine audio quality gate outcome is inconsistent".to_string());
        }
        previous_order = Some(order);
        degrading_uncertainty |= expected_requirement == QualityGateRequirementWire::Degrading
            && outcome.status != QualityGateStatusWire::Passed;
        for metric in &outcome.metrics {
            if metric.name.trim().is_empty()
                || metric.unit.trim().is_empty()
                || !metric.value.is_finite()
                || metric.lower_bound.is_some_and(|value| !value.is_finite())
                || metric.upper_bound.is_some_and(|value| !value.is_finite())
                || matches!((metric.lower_bound, metric.upper_bound), (Some(low), Some(high)) if low > high)
            {
                return Err("Engine audio quality metric is invalid".to_string());
            }
        }
        // Every other gate's regions describe activity within the app-owned
        // requested window (`source_start..source_end`), so they're held to
        // it exactly. `vocal_topology` is the one gate whose region can
        // legitimately span the Engine's own measured full duration -- its
        // `vocal_topology_unknown` fallback region is built from
        // `report.duration`, not the app's request -- so it is bound only by
        // `report_end`, already checked below with zero tolerance.
        let is_vocal_topology = planned == "vocal_topology";
        if let Some(region) = outcome.regions.iter().find(|region| {
            (!is_vocal_topology && (region.start < source_start || region.end > source_end))
                || region.end > report_end
                || region.start >= region.end
                || region.reason.trim().is_empty()
        }) {
            return Err(format!(
                "Engine audio quality region is invalid: gate={planned} region=[{}, {}) reason={:?} (source_start={source_start} source_end={source_end} report_end={report_end})",
                region.start, region.end, region.reason
            ));
        }
        if let Some(pair) = outcome
            .regions
            .windows(2)
            .find(|pair| pair[0].end > pair[1].start)
        {
            return Err(format!(
                "Engine audio quality region is invalid: gate={planned} overlapping regions [{}, {}) and [{}, {})",
                pair[0].start, pair[0].end, pair[1].start, pair[1].end
            ));
        }
    }
    if degrading_uncertainty && manifest.status != AnalysisStatusWire::OkDegraded {
        return Err("Engine audio quality uncertainty was not surfaced as degraded".to_string());
    }
    Ok(())
}

fn validate_vocal_topology_result(
    request: &AnalyzeRequestWire,
    plan: &AnalysisPlanWire,
    report: &AudioQualityReportWire,
    expected_source_duration: u64,
) -> Result<(), String> {
    let required = plan
        .quality_gates
        .iter()
        .any(|gate| gate == "vocal_topology");
    let Some(topology) = report.vocal_topology.as_ref() else {
        return if required {
            Err("Engine omitted planned typed vocal topology evidence".to_string())
        } else {
            Ok(())
        };
    };
    let source_start = validated_primary_source_binding(request, plan)?
        .timeline
        .source_start;
    let source_end = source_start
        .checked_add(expected_source_duration)
        .ok_or_else(|| "App-owned vocal topology timeline overflows".to_string())?;
    let topology_end = source_start
        .checked_add(topology.duration)
        .ok_or_else(|| "Engine vocal topology timeline overflows".to_string())?;
    let valid_regions = |regions: &[QualityRegionWire]| {
        regions.iter().all(|region| {
            region.start >= source_start
                && region.end <= source_end
                && region.end <= topology_end
                && region.start < region.end
                && !region.reason.trim().is_empty()
        }) && regions.windows(2).all(|pair| pair[0].end <= pair[1].start)
    };
    let mode_shape_valid = match topology.mode {
        VocalTopologyModeWire::SingleLead | VocalTopologyModeWire::Unknown => {
            topology.overlap_regions.is_empty() && topology.support_regions.is_empty()
        }
        VocalTopologyModeWire::AlternatingMultiLead => topology.overlap_regions.is_empty(),
        VocalTopologyModeWire::OverlappingMultiLead => !topology.overlap_regions.is_empty(),
        VocalTopologyModeWire::LeadWithSupport => !topology.support_regions.is_empty(),
    };
    if topology.contract != "uta.analysis-engine.vocal-topology-estimate"
        || topology.version != 1
        || topology.timebase != 1_000_000
        || topology.source_start != source_start
        || topology.duration != report.duration
        || topology.evidence_sources.is_empty()
        || topology
            .evidence_sources
            .iter()
            .any(|source| source.trim().is_empty())
        || topology
            .confidence
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        || !valid_regions(&topology.overlap_regions)
        || !valid_regions(&topology.support_regions)
        || !mode_shape_valid
    {
        return Err("Engine typed vocal topology evidence is invalid".to_string());
    }
    Ok(())
}

fn validate_quantization_result(
    request: &AnalyzeRequestWire,
    manifest: &AnalysisResultManifestWire,
) -> Result<(), String> {
    match (
        request.analysis.enable_quantization,
        manifest.diagnostics.quantization.as_ref(),
    ) {
        (false, None) => Ok(()),
        (false, Some(_)) => {
            Err("Engine returned quantization diagnostics for a disabled stage".to_string())
        }
        (true, None) => {
            Err("Engine omitted quantization diagnostics for an enabled stage".to_string())
        }
        (true, Some(report)) => {
            let context = request
                .musical_context
                .as_ref()
                .ok_or_else(|| "Quantized Engine result has no musical context".to_string())?;
            if manifest.artifacts.candidate_vocal_chart.is_none()
                || report.algorithm != manifest.provenance.quantization_version
                || !report.bpm.is_finite()
                || context.bpm != Some(report.bpm)
                || context.quantization_grid != Some(report.grid)
                || report.grid_step == 0
                || report.minimum_note_duration != report.grid_step
                || report.source_end <= report.source_start
                || report.adjusted_notes > report.note_count
                || report.maximum_shift > report.grid_step
            {
                return Err("Engine quantization result contract is inconsistent".to_string());
            }
            Ok(())
        }
    }
}

fn result_artifacts(
    manifest: &AnalysisResultManifestWire,
) -> Vec<(String, &ArtifactRefWire, ArtifactKind, &'static str)> {
    let mut result = Vec::new();
    for (semantic, artifact, kind, producer) in [
        (
            "candidate_vocal_chart",
            manifest.artifacts.candidate_vocal_chart.as_ref(),
            ArtifactKind::CandidateChart,
            "vocal-chart",
        ),
        (
            "pitch_evidence",
            manifest.artifacts.pitch_evidence.as_ref(),
            ArtifactKind::PitchEvidence,
            "pitch",
        ),
        (
            "technique_evidence",
            manifest.artifacts.technique_evidence.as_ref(),
            ArtifactKind::TechniqueEvidence,
            "stars-technique",
        ),
        (
            "singing_analysis",
            manifest.artifacts.singing_analysis.as_ref(),
            ArtifactKind::EvidenceBundle,
            "singing-fusion",
        ),
        (
            "transcript",
            manifest.artifacts.transcript.as_ref(),
            ArtifactKind::TranscriptEvidence,
            "transcript",
        ),
        (
            "alignment",
            manifest.artifacts.alignment.as_ref(),
            ArtifactKind::AlignmentEvidence,
            "alignment",
        ),
    ] {
        if let Some(artifact) = artifact {
            result.push((semantic.to_string(), artifact, kind, producer));
        }
    }
    for stem in &manifest.artifacts.stems {
        let (role, kind, producer) = match stem.role {
            AudioRoleWire::Instrumental => {
                let (kind, producer) = instrumental_result_kind(&stem.artifact.path);
                ("instrumental", kind, producer)
            }
            AudioRoleWire::GuideVocals => {
                ("guide_vocals", ArtifactKind::VocalStem, "extract-vocals")
            }
            AudioRoleWire::LeadVocal => (
                "lead_vocal",
                ArtifactKind::AnalysisVocalStem,
                "lead-isolate",
            ),
            AudioRoleWire::BackingVocal => (
                "backing_vocal",
                ArtifactKind::RawVocalStem,
                "lead-partition",
            ),
            AudioRoleWire::HarmonyVocal => (
                "harmony_vocal",
                ArtifactKind::RawVocalStem,
                "lead-partition",
            ),
            AudioRoleWire::CleanLeadVocal => (
                "clean_lead_vocal",
                ArtifactKind::DereverbedVocalStem,
                "cleanup",
            ),
            _ => continue,
        };
        result.push((format!("stem:{role}"), &stem.artifact, kind, producer));
    }
    result
}

fn instrumental_result_kind(path: &Path) -> (ArtifactKind, &'static str) {
    if !path.starts_with(Path::new("workflow-audio")) {
        return (ArtifactKind::InstrumentalStem, "extract-instrumental");
    }
    let kind = if path.to_string_lossy().contains("-dereverb.flac") {
        ArtifactKind::DereverbedInstrumentalStem
    } else {
        ArtifactKind::DenoisedInstrumentalStem
    };
    (kind, "cleanup")
}

fn validate_semantic_artifact(semantic: &str, path: &Path) -> Result<(), String> {
    if semantic != "candidate_vocal_chart" {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(path).map_err(|error| format!("could not read Candidate chart: {error}"))?,
    )
    .map_err(|error| format!("Engine Candidate chart is not valid JSON: {error}"))?;
    let chart = if value.get("contract").and_then(serde_json::Value::as_str)
        == Some("uta.analysis-engine.candidate-vocal-chart")
    {
        crate::vocal_chart::migrate_engine_candidate_chart(&value)
            .map_err(|error| format!("Engine Candidate projection is invalid: {error}"))?
    } else {
        serde_json::from_value::<utz::VocalChart>(value)
            .map_err(|error| format!("Engine Candidate VocalChart is invalid: {error}"))?
    };
    chart
        .validate()
        .map_err(|error| format!("Engine Candidate VocalChart failed validation: {error}"))
}

fn validate_artifact(output_root: &Path, artifact: &ArtifactRefWire) -> Result<PathBuf, String> {
    if artifact.path.is_absolute()
        || artifact.path.as_os_str().is_empty()
        || artifact.path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || artifact.bytes == 0
    {
        return Err("Engine artifact reference is invalid or unconfined".to_string());
    }
    let path = output_root.join(&artifact.path);
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("Engine artifact is missing: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Engine artifact is not a regular non-symlink file".to_string());
    }
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    if !canonical.starts_with(output_root) {
        return Err("Engine artifact escaped its authorized output root".to_string());
    }
    if metadata.len() != artifact.bytes {
        return Err("Engine artifact byte count mismatch".to_string());
    }
    Ok(canonical)
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests;
