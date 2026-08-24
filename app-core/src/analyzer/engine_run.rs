use std::path::{Component, Path, PathBuf};

use super::*;
use crate::analysis_artifact::{ArtifactRevision, ArtifactStore, revision_to_row};
use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
use crate::backend_cli::{
    ANALYSIS_RESULT_CONTRACT, ANALYSIS_RESULT_VERSION, AUDIO_QUALITY_REPORT_CONTRACT,
    AUDIO_QUALITY_REPORT_VERSION, AnalysisCancelHandle, AnalysisCliClient, AnalysisPlanWireV1,
    AnalysisResultManifestWireV1, AnalysisStatusWireV1, AnalyzeRequestWireV1, ArtifactRefWireV1,
    AudioRoleWireV1, QualityGateRequirementWireV1, QualityGateStatusWireV1,
};
use crate::library_db::EngineQueueIntent;

static ACTIVE_ENGINE_CANCELS: LazyLock<Mutex<HashMap<String, (String, AnalysisCancelHandle)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn cancel_active_engine(file_hash: &str) -> Option<Result<(), String>> {
    let guard = ACTIVE_ENGINE_CANCELS.lock().unwrap();
    let (request_id, handle) = guard.get(file_hash)?;
    Some(handle.cancel(request_id).map_err(|error| error.to_string()))
}

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
            requested_device: "Testing / Experimental".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            stage_routes: Vec::new(),
            node_id: None,
            node_event: Some("started".to_string()),
            artifact_reused_reason: None,
            analysis_log_path: log_path.clone(),
            engine: Some(engine_projection),
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
            remove_from_queue(file_hash);
            LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
        }
        Err(error) => {
            append_analysis_log_path(log_path.as_deref(), &error);
            let cancelled = error.starts_with("cancelled:");
            if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(file_hash) {
                snapshot.detail = error.clone();
                snapshot.node_event =
                    Some(if cancelled { "cancelled" } else { "failed" }.to_string());
            }
            if cancelled {
                finish_analysis_history(file_hash, "cancelled", Some(&error));
                remove_from_queue(file_hash);
            } else {
                update_queue_status(file_hash, QueuedStatus::Failed(error));
            }
            LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
        }
    }
}

fn execute_exact_intent(
    file_hash: &str,
    cache: &CacheDir,
    intent: &EngineQueueIntent,
    log_path: Option<&Path>,
) -> Result<AnalysisResultManifestWireV1, String> {
    let request: AnalyzeRequestWireV1 = serde_json::from_str(&intent.request_json)
        .map_err(|error| format!("persisted Engine request is malformed: {error}"))?;
    let plan: AnalysisPlanWireV1 = serde_json::from_str(&intent.plan_json)
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
    let source = crate::analysis_engine_adapter::resolve_true_source(file_hash)?;
    let primary = request
        .audio_sources
        .iter()
        .find(|source| source.primary)
        .ok_or_else(|| "persisted Engine request has no primary source".to_string())?;
    if source.path != intent.source_path
        || primary.path != source.path
        || primary.role != source.role
    {
        return Err(
            "source_identity_changed: queued TrueSource no longer matches the exact preview"
                .to_string(),
        );
    }

    let runs_root = cache.path.join("engine-runs");
    std::fs::create_dir_all(&runs_root)
        .map_err(|error| format!("could not create Engine runs root: {error}"))?;
    let output_root = runs_root.join(&intent.request_id);
    std::fs::create_dir(&output_root)
        .map_err(|error| format!("could not create unique Engine output root: {error}"))?;
    let request_value =
        serde_json::from_str(&intent.request_json).map_err(|error| error.to_string())?;
    let outcome = (|| {
        let mut client = AnalysisCliClient::connect().map_err(|error| error.to_string())?;
        ACTIVE_ENGINE_CANCELS.lock().unwrap().insert(
            file_hash.to_string(),
            (intent.request_id.clone(), client.cancellation_handle()),
        );
        let analysis = client.analyze(&request_value, &intent.request_id, &output_root);
        ACTIVE_ENGINE_CANCELS.lock().unwrap().remove(file_hash);
        let stderr = client.stderr_log();
        if !stderr.is_empty() {
            append_analysis_log_path(log_path, &format!("uta-analyze stderr: {stderr}"));
        }
        let manifest = analysis.map_err(|error| error.to_string())?;
        validate_and_publish_engine_result(
            file_hash,
            cache,
            &output_root,
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

fn validate_and_publish_engine_result(
    file_hash: &str,
    cache: &CacheDir,
    output_root: &Path,
    request: &AnalyzeRequestWireV1,
    plan: &AnalysisPlanWireV1,
    manifest: &AnalysisResultManifestWireV1,
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
        AnalysisStatusWireV1::Ok | AnalysisStatusWireV1::OkDegraded
    ) {
        return Err(format!(
            "Engine returned non-success status {:?}",
            manifest.status
        ));
    }
    if matches!(manifest.status, AnalysisStatusWireV1::Ok) && !manifest.degraded_reasons.is_empty()
    {
        return Err("Engine ok result unexpectedly contains degraded reasons".to_string());
    }
    if matches!(manifest.status, AnalysisStatusWireV1::OkDegraded)
        && manifest.degraded_reasons.is_empty()
    {
        return Err("Engine degraded result omitted degraded reasons".to_string());
    }
    for version in [
        &manifest.provenance.calibration_version,
        &manifest.provenance.fusion_version,
        &manifest.provenance.hsmm_version,
        &manifest.provenance.quantization_version,
        &manifest.provenance.audio_quality_version,
        &manifest.provenance.postprocess_version,
    ] {
        if version.trim().is_empty() {
            return Err("Engine result algorithm provenance is incomplete".to_string());
        }
    }
    validate_quantization_result(request, manifest)?;
    validate_audio_quality_result(request, plan, manifest)?;
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
            AudioRoleWireV1::Instrumental
                | AudioRoleWireV1::GuideVocals
                | AudioRoleWireV1::LeadVocal
                | AudioRoleWireV1::BackingVocal
                | AudioRoleWireV1::HarmonyVocal
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
    let store = ArtifactStore::new(&cache.path)?;
    let mut revisions = Vec::new();
    let mut activations = Vec::new();
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
        let (immutable_path, content_hash, byte_size) = store.capture(file_hash, kind, &path)?;
        let revision = ArtifactRevision {
            id: format!("{file_hash}:{semantic}:{content_hash}"),
            file_hash: file_hash.to_string(),
            kind,
            path: immutable_path,
            content_hash,
            producer_node: AnalysisNodeId::new(producer),
            input_revisions: Vec::new(),
            config_hash: format!("engine:{}", manifest.fingerprint),
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
        revisions.push(revision_to_row(&revision));
    }
    crate::library_db::analysis_artifacts_publish_batch(&revisions, &activations)
        .map_err(|error| error.to_string())
}

fn validate_audio_quality_result(
    request: &AnalyzeRequestWireV1,
    plan: &AnalysisPlanWireV1,
    manifest: &AnalysisResultManifestWireV1,
) -> Result<(), String> {
    const GATE_ORDER: &[&str] = &[
        "timeline_valid",
        "finite_samples",
        "clipping",
        "silence_ratio",
        "energy_ratio",
        "lead_purity",
        "cleanup_consistency",
        "vocal_topology",
    ];
    let report = manifest.diagnostics.audio_quality.as_ref().ok_or_else(|| {
        "Engine omitted audio quality diagnostics for an executable Plan".to_string()
    })?;
    if report.contract != AUDIO_QUALITY_REPORT_CONTRACT
        || report.version != AUDIO_QUALITY_REPORT_VERSION
        || report.algorithm != manifest.provenance.audio_quality_version
        || report.profile != request.analysis.profile
        || report.evaluated_audio_role.trim().is_empty()
        || report.duration == 0
        || report.planned_gates != plan.quality_gates
        || report.outcomes.len() != plan.quality_gates.len()
    {
        return Err("Engine audio quality report identity or Plan binding is invalid".to_string());
    }
    let mut previous_order = None;
    let mut degrading_uncertainty = false;
    for (planned, outcome) in plan.quality_gates.iter().zip(&report.outcomes) {
        let order = GATE_ORDER
            .iter()
            .position(|known| *known == planned)
            .ok_or_else(|| format!("Engine Plan contains unknown audio quality gate {planned}"))?;
        let expected_requirement = match planned.as_str() {
            "timeline_valid" | "finite_samples" | "silence_ratio" | "energy_ratio" => {
                QualityGateRequirementWireV1::Required
            }
            _ => QualityGateRequirementWireV1::Degrading,
        };
        if previous_order.is_some_and(|previous| order <= previous)
            || outcome.gate != *planned
            || outcome.requirement != expected_requirement
            || outcome.summary.trim().is_empty()
            || (expected_requirement == QualityGateRequirementWireV1::Required
                && outcome.status != QualityGateStatusWireV1::Passed)
        {
            return Err("Engine audio quality gate outcome is inconsistent".to_string());
        }
        previous_order = Some(order);
        degrading_uncertainty |= expected_requirement == QualityGateRequirementWireV1::Degrading
            && outcome.status != QualityGateStatusWireV1::Passed;
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
        if outcome
            .regions
            .iter()
            .any(|region| region.start >= region.end || region.reason.trim().is_empty())
        {
            return Err("Engine audio quality region is invalid".to_string());
        }
    }
    if degrading_uncertainty && manifest.status != AnalysisStatusWireV1::OkDegraded {
        return Err("Engine audio quality uncertainty was not surfaced as degraded".to_string());
    }
    Ok(())
}

fn validate_quantization_result(
    request: &AnalyzeRequestWireV1,
    manifest: &AnalysisResultManifestWireV1,
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
    manifest: &AnalysisResultManifestWireV1,
) -> Vec<(String, &ArtifactRefWireV1, ArtifactKind, &'static str)> {
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
            AudioRoleWireV1::Instrumental => (
                "instrumental",
                ArtifactKind::InstrumentalStem,
                "extract-instrumental",
            ),
            AudioRoleWireV1::GuideVocals => {
                ("guide_vocals", ArtifactKind::VocalStem, "extract-vocals")
            }
            AudioRoleWireV1::LeadVocal => (
                "lead_vocal",
                ArtifactKind::AnalysisVocalStem,
                "lead-isolate",
            ),
            AudioRoleWireV1::BackingVocal => (
                "backing_vocal",
                ArtifactKind::RawVocalStem,
                "lead-partition",
            ),
            AudioRoleWireV1::HarmonyVocal => (
                "harmony_vocal",
                ArtifactKind::RawVocalStem,
                "lead-partition",
            ),
            _ => continue,
        };
        result.push((format!("stem:{role}"), &stem.artifact, kind, producer));
    }
    result
}

fn validate_artifact(output_root: &Path, artifact: &ArtifactRefWireV1) -> Result<PathBuf, String> {
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
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-engine-result-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }

    fn artifact(path: &str, bytes: &[u8]) -> ArtifactRefWireV1 {
        ArtifactRefWireV1 {
            path: PathBuf::from(path),
            media_type: "application/json".to_string(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            bytes: bytes.len() as u64,
        }
    }

    #[test]
    fn result_artifact_validation_checks_confinement_and_byte_count_without_hash_verification() {
        let root = temp_root("validation");
        std::fs::write(root.join("valid.json"), b"valid").unwrap();
        validate_artifact(&root, &artifact("valid.json", b"valid")).unwrap();

        let mut opaque_hash_metadata = artifact("valid.json", b"other");
        opaque_hash_metadata.bytes = 5;
        validate_artifact(&root, &opaque_hash_metadata).unwrap();
        let mut wrong_bytes = artifact("valid.json", b"valid");
        wrong_bytes.bytes = 99;
        assert!(
            validate_artifact(&root, &wrong_bytes)
                .unwrap_err()
                .contains("byte count")
        );
        assert!(
            validate_artifact(&root, &artifact("../escape.json", b"valid"))
                .unwrap_err()
                .contains("unconfined")
        );
        assert!(
            validate_artifact(&root, &artifact("/tmp/escape.json", b"valid"))
                .unwrap_err()
                .contains("unconfined")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn quantization_wire_result_matches_exact_request_intent() {
        let request: AnalyzeRequestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,"request_id":"quantized",
            "audio_sources":[{"id":"main","kind":"local_file","path":"song.flac","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","role":"lead_vocal","primary":true,"timeline":{"timebase":1000000,"source_start":0}}],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "musical_context":{"bpm":120.0,"time_signature":{"beats":4,"unit":4},"quantization_grid":"sixteenth","authority":"hint"},
            "analysis":{"profile":"fast","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":true},
            "requested_artifacts":{"vocal_chart":true},"execution_policy":{},"extensions":{}
        })).unwrap();
        let mut manifest: AnalysisResultManifestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.result","version":1,"request_id":"quantized","status":"ok",
            "artifacts":{"candidate_vocal_chart":{"path":"candidate/vocal-chart.json","media_type":"application/vnd.uta.vocal-chart+json;version=0.3","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","bytes":1}},
            "diagnostics":{"quantization":{"algorithm":"rhythm-grid-dp-v1","bpm":120.0,"grid":"sixteenth","grid_step":125000,"minimum_note_duration":125000,"source_start":0,"source_end":1000000,"hard_boundary_count":0,"note_count":2,"adjusted_notes":2,"maximum_shift":12000}},
            "provenance":{"resources":[],"calibration_version":"c","fusion_version":"f","hsmm_version":"h","quantization_version":"rhythm-grid-dp-v1","postprocess_version":"p"},
            "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","degraded_reasons":[]
        })).unwrap();
        validate_quantization_result(&request, &manifest).unwrap();
        manifest
            .diagnostics
            .quantization
            .as_mut()
            .unwrap()
            .grid_step = 0;
        assert!(validate_quantization_result(&request, &manifest).is_err());
        manifest.diagnostics.quantization = None;
        assert!(validate_quantization_result(&request, &manifest).is_err());
    }

    #[test]
    fn audio_quality_wire_result_is_bound_to_plan_and_surfaces_uncertainty() {
        let request: AnalyzeRequestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,"request_id":"quality",
            "audio_sources":[{"id":"main","kind":"local_file","path":"song.flac","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","role":"lead_vocal","primary":true,"timeline":{"timebase":1000000,"source_start":0}}],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "analysis":{"profile":"fast","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
            "requested_artifacts":{"pitch_evidence":true},"execution_policy":{},"extensions":{}
        })).unwrap();
        let gates = vec![
            "timeline_valid",
            "finite_samples",
            "clipping",
            "silence_ratio",
            "energy_ratio",
        ];
        let outcomes = gates
            .iter()
            .map(|gate| serde_json::json!({
                "gate":gate,
                "requirement":if matches!(*gate, "timeline_valid" | "finite_samples" | "silence_ratio" | "energy_ratio") { "required" } else { "degrading" },
                "status":"passed","summary":"measured","metrics":[],"regions":[]
            }))
            .collect::<Vec<_>>();
        let plan: AnalysisPlanWireV1 = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"quality",
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],"required_capabilities":[],"optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":gates.clone(),
            "fallback_policy":[],"artifact_declarations":[]
        })).unwrap();
        let mut manifest: AnalysisResultManifestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.result","version":1,"request_id":"quality","status":"ok",
            "artifacts":{},
            "diagnostics":{"audio_quality":{"contract":"uta.analysis-engine.audio-quality-report","version":1,"algorithm":"audio-quality-gates-v1","profile":"fast","evaluated_audio_role":"lead_vocal","duration":1000000,"planned_gates":gates,"outcomes":outcomes}},
            "provenance":{"resources":[],"calibration_version":"c","fusion_version":"f","hsmm_version":"h","quantization_version":"q","audio_quality_version":"audio-quality-gates-v1","postprocess_version":"p"},
            "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","degraded_reasons":[]
        })).unwrap();
        validate_audio_quality_result(&request, &plan, &manifest).unwrap();

        manifest
            .diagnostics
            .audio_quality
            .as_mut()
            .unwrap()
            .planned_gates
            .pop();
        assert!(validate_audio_quality_result(&request, &plan, &manifest).is_err());
        manifest
            .diagnostics
            .audio_quality
            .as_mut()
            .unwrap()
            .planned_gates = plan.quality_gates.clone();
        let clipping = &mut manifest
            .diagnostics
            .audio_quality
            .as_mut()
            .unwrap()
            .outcomes[2];
        clipping.status = QualityGateStatusWireV1::Unknown;
        assert!(validate_audio_quality_result(&request, &plan, &manifest).is_err());
        manifest.status = AnalysisStatusWireV1::OkDegraded;
        validate_audio_quality_result(&request, &plan, &manifest).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn result_artifact_validation_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let root = temp_root("symlink");
        let outside = root
            .parent()
            .unwrap()
            .join(format!("uta-studio-engine-outside-{}", std::process::id()));
        std::fs::write(&outside, b"valid").unwrap();
        symlink(&outside, root.join("link.json")).unwrap();
        assert!(
            validate_artifact(&root, &artifact("link.json", b"valid"))
                .unwrap_err()
                .contains("non-symlink")
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(outside);
    }
}
