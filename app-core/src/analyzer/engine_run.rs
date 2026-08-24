use std::io::Read;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use super::*;
use crate::analysis_artifact::{ArtifactRevision, ArtifactStore, revision_to_row};
use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
use crate::backend_cli::{
    ANALYSIS_RESULT_CONTRACT, ANALYSIS_RESULT_VERSION, AnalysisCancelHandle, AnalysisCliClient,
    AnalysisPlanWireV1, AnalysisResultManifestWireV1, AnalysisStatusWireV1, AnalyzeRequestWireV1,
    ArtifactRefWireV1, AudioRoleWireV1,
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
    if crate::analysis_engine_adapter::digest_json(&intent.request_json) != intent.request_digest {
        return Err("persisted Engine request digest mismatch".to_string());
    }
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
        || source.sha256 != intent.source_sha256
        || primary.path != source.path
        || primary.sha256 != source.sha256
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
    if !valid_sha256(&manifest.fingerprint) {
        return Err("Engine result fingerprint is invalid".to_string());
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
        &manifest.provenance.postprocess_version,
    ] {
        if version.trim().is_empty() {
            return Err("Engine result algorithm provenance is incomplete".to_string());
        }
    }
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
        || !valid_sha256(&artifact.sha256)
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
    let actual = sha256_file(&canonical)?;
    if actual != artifact.sha256 {
        return Err("Engine artifact SHA-256 mismatch".to_string());
    }
    Ok(canonical)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut input = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
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
    fn result_artifact_validation_checks_confinement_hash_and_byte_count() {
        let root = temp_root("validation");
        std::fs::write(root.join("valid.json"), b"valid").unwrap();
        validate_artifact(&root, &artifact("valid.json", b"valid")).unwrap();

        let mut wrong_hash = artifact("valid.json", b"other");
        wrong_hash.bytes = 5;
        assert!(
            validate_artifact(&root, &wrong_hash)
                .unwrap_err()
                .contains("SHA-256")
        );
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
