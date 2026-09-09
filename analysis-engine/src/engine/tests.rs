use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use uta_runtime_manager::{
    InstallManifest, InstalledFile, ResourceCatalog, ResourceRef, StorePaths,
};

use super::*;
use crate::artifact::PitchEvidenceV03;
use crate::contract::request::tests::valid_request;
use crate::contract::{AudioRole, TIMELINE_VALID_GATE};
use crate::fusion::{BoundaryCandidateRole, BoundaryEvidenceKind};

mod fingerprint_versions;

#[test]
fn failed_run_guard_removes_only_children_of_empty_authorized_root() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("uta-run-guard-{stamp}"));
    fs::create_dir_all(&root).unwrap();
    {
        let _guard = OutputRunGuard::new(&root).unwrap();
        fs::create_dir(root.join("worker")).unwrap();
        fs::write(root.join("worker/partial.json"), b"partial").unwrap();
    }
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    fs::remove_dir(root).unwrap();
}

#[test]
fn pre_cancelled_request_stops_before_resource_or_output_work() {
    let request = valid_request(AudioRole::CleanLeadVocal);
    let manager = RuntimeManager::new(
        ResourceCatalog::default_catalog().unwrap(),
        StorePaths::default(),
    );
    let engine = AnalysisEngine::new(manager);
    let token = CancellationToken::default();
    token.cancel();
    assert_eq!(
        engine
            .analyze_with_cancellation(&request, std::env::temp_dir(), &token)
            .unwrap_err()
            .code,
        EngineErrorCode::Cancelled
    );
}

#[test]
fn request_fingerprint_identity_does_not_depend_on_local_path() {
    let mut left = valid_request(AudioRole::LeadVocal);
    let mut right = left.clone();
    left.audio_sources[0].path = PathBuf::from("/library-a/song.flac");
    right.audio_sources[0].path = PathBuf::from("/library-b/song.flac");
    assert_eq!(
        deterministic_fingerprint(&fingerprint_request(&left).unwrap()).unwrap(),
        deterministic_fingerprint(&fingerprint_request(&right).unwrap()).unwrap()
    );
}

#[test]
fn pure_typed_candidate_outputs_are_published_and_manifest_valid() {
    use crate::artifact::{CandidateVocalChartV1, SingingAnalysisV1};
    use crate::candidate_pipeline::{CandidatePathDecisionV1, SingingStagesOutput};
    use crate::fusion::{
        AcousticCandidateFeatures, CanonicalLyrics, CanonicalNote, CanonicalNoteEvidence,
        CanonicalSingingTrack, CanonicalWordBoundary, HarmonyMetadata, LyricsAuthority,
        SegmentCandidate, SingingFusionEvidence, SingingReviewReason, SingingReviewRegion,
        TechniqueScores, TimeRange,
    };

    let range = TimeRange::new(100_000, 400_000).unwrap();
    let lyrics = CanonicalLyrics {
        text: "sing".to_string(),
        language: Some("en".to_string()),
        authority: LyricsAuthority::CallerCanonical,
        tokens: Vec::new(),
        confidence: None,
        source_experts: vec!["caller.canonical_lyrics".to_string()],
        alternatives: Vec::new(),
    };
    let track = CanonicalSingingTrack {
        schema_version: 1,
        transcript: lyrics,
        words: vec![CanonicalWordBoundary {
            word_id: "word-0".to_string(),
            text: "sing".to_string(),
            range,
            confidence: None,
            disagreement: None,
            source_experts: vec!["alignment-reference".to_string()],
        }],
        notes: vec![CanonicalNote {
            id: "game-note-0".to_string(),
            range,
            midi_note: 69,
            center_pitch_hz: 440.0,
            center_offset_cents: 0.0,
            confidence: None,
            uncertain: true,
            alternatives: Vec::new(),
            f0_curve: Vec::new(),
            pitch_bend: Vec::new(),
            techniques: TechniqueScores::default(),
            word_id: Some("word-0".to_string()),
            evidence: CanonicalNoteEvidence {
                source_experts: vec!["game".to_string()],
                decision_trace: Default::default(),
                boundary_source: "game".to_string(),
                boundary_kind: BoundaryEvidenceKind::Game,
                boundary_role: BoundaryCandidateRole::Primary,
                boundary_fractional_midi: Some(69.0),
                boundary_decision_parameter: Some(0.2),
                presence_decision_parameter: Some(0.2),
                boundary_calibrated_confidence: None,
                target_pitch_source: "game".to_string(),
                target_pitch_source_local_score: None,
                target_pitch_calibrated_confidence: None,
                rmvpe_center_hz: None,
                rmvpe_confidence: None,
                rmvpe_cents_difference: None,
                rmvpe_voiced_ratio: None,
                rmvpe_pitch_mad_cents: None,
                fcpe_center_hz: None,
                fcpe_observed_ratio: None,
                fcpe_pitch_mad_cents: None,
                fcpe_cents_from_rmvpe: None,
                fcpe_supports_rmvpe: None,
                acoustic: Some(AcousticCandidateFeatures {
                    frame_count: 30,
                    mean_rms: 0.2,
                    mean_periodicity: 0.8,
                    fundamental_center_hz: Some(440.0),
                    mean_snr_db: 20.0,
                    mean_vibrato_activation: 0.0,
                    mean_glide_activation: 0.0,
                    mean_ornament_activation: 0.0,
                    mean_breath_activation: 0.0,
                    max_voicing_transition_activation: 0.0,
                    onset_flux: Some(0.3),
                    preceding_flux: Some(0.01),
                    onset_supported: Some(true),
                }),
                basic_pitch: None,
                boundary_alternatives: Vec::new(),
                technique_evidence: Vec::new(),
            },
        }],
        f0_curve: Vec::new(),
        harmony_metadata: HarmonyMetadata::default(),
        provenance: Vec::new(),
    };
    let singing = SingingStagesOutput {
        fusion: SingingFusionEvidence {
            schema_version: 1,
            candidates: vec![SegmentCandidate {
                id: "game-note-0".to_string(),
                range,
                target_midi: 69,
                boundary_source: "game".to_string(),
                boundary_kind: BoundaryEvidenceKind::Game,
                boundary_role: BoundaryCandidateRole::Primary,
                boundary_fractional_midi: Some(69.0),
                boundary_decision_parameter: Some(0.2),
                presence_decision_parameter: Some(0.2),
                boundary_hard: false,
                boundary_support: None,
                boundary_calibrated_confidence: None,
                target_pitch_source: "game".to_string(),
                target_pitch_source_local_score: None,
                target_pitch_calibrated_confidence: None,
                center_pitch_hz: 440.0,
                rmvpe_center_hz: None,
                rmvpe_confidence: None,
                rmvpe_cents_difference: None,
                rmvpe_voiced_ratio: None,
                rmvpe_pitch_mad_cents: None,
                fcpe_center_hz: None,
                fcpe_observed_ratio: None,
                fcpe_pitch_mad_cents: None,
                fcpe_cents_from_rmvpe: None,
                fcpe_supports_rmvpe: None,
                acoustic: None,
                basic_pitch: None,
                boundary_alternatives: Vec::new(),
                boundary_constraints: Vec::new(),
                technique_evidence: Vec::new(),
                techniques: TechniqueScores::default(),
                word_id: Some("word-0".to_string()),
                alternatives: Vec::new(),
            }],
            hard_boundaries: Default::default(),
        },
        track,
        review_regions: vec![SingingReviewRegion {
            id: "review-100000-400000".to_string(),
            range,
            confidence: None,
            reasons: vec![SingingReviewReason::UnknownConfidence],
            evidence_experts: vec!["game".to_string()],
            reviewed: false,
        }],
        decision: CandidatePathDecisionV1::Algorithm {
            candidate_set_digest: "a".repeat(64),
            selected_candidate_ids: vec!["game-note-0".to_string()],
        },
    };
    let candidate_digest = crate::execution::candidate_set_digest(&singing.fusion).unwrap();
    let decision_provenance = FusionDecisionProvenanceV1::Algorithm {
        selector: HSMM_VITERBI_SELECTOR.to_string(),
        selector_version: HSMM_VERSION.to_string(),
        candidate_set_digest: candidate_digest,
        selected_candidate_ids: vec!["game-note-0".to_string()],
        reuse_policy: AnalysisReusePolicyV1::Deterministic,
    };
    let root = std::env::temp_dir().join(format!(
        "uta-engine-pure-candidate-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let mut artifacts = AnalysisArtifactsV1::default();
    let fingerprint = "9".repeat(64);
    publish_candidate_artifacts(
        &root,
        true,
        true,
        true,
        &fingerprint,
        Some(&decision_provenance),
        Some(&singing),
        None,
        None,
        &mut artifacts,
        &CancellationToken::default(),
    )
    .unwrap();
    let singing_ref = artifacts.singing_analysis.as_ref().unwrap();
    assert_eq!(
        singing_ref.path,
        Path::new("analysis/singing-analysis.json")
    );
    let analysis: SingingAnalysisV1 =
        serde_json::from_slice(&fs::read(root.join(&singing_ref.path)).unwrap()).unwrap();
    analysis.validate().unwrap();
    assert!(analysis.track.is_none());
    assert_eq!(analysis.chart_references.track_id, "lead");
    assert_eq!(analysis.chart_references.note_ids, ["game-note-0"]);
    assert_eq!(analysis.candidate_evidence.len(), 1);
    let chart_ref = artifacts.candidate_vocal_chart.as_ref().unwrap();
    assert_eq!(chart_ref.path, Path::new("candidate/vocal-chart.json"));
    let chart: CandidateVocalChartV1 =
        serde_json::from_slice(&fs::read(root.join(&chart_ref.path)).unwrap()).unwrap();
    chart.validate().unwrap();
    assert_eq!(chart.format, utz::VOCAL_CHART_FORMAT);
    assert_eq!(chart.tracks[0].phrases[0].notes[0].id, "game-note-0");
    assert_eq!(chart.tracks[0].phrases[0].notes[0].scoring.weight, 1.0);

    let quantized_root = root.join("quantized");
    fs::create_dir(&quantized_root).unwrap();
    let mut quantized_track = singing.track.clone();
    quantized_track.notes[0].range = TimeRange::new(125_000, 375_000).unwrap();
    let report = crate::quantization::QuantizationReportV1 {
        algorithm: QUANTIZATION_VERSION.to_string(),
        bpm: 120.0,
        grid: crate::contract::QuantizationGridV1::Sixteenth,
        grid_step: 125_000,
        minimum_note_duration: 125_000,
        source_start: 0,
        source_end: 1_000_000,
        hard_boundary_count: 0,
        note_count: 1,
        adjusted_notes: 1,
        maximum_shift: 25_000,
    };
    let mut quantized_artifacts = AnalysisArtifactsV1::default();
    publish_candidate_artifacts(
        &quantized_root,
        true,
        true,
        true,
        &fingerprint,
        Some(&decision_provenance),
        Some(&singing),
        Some(&quantized_track),
        Some(&report),
        &mut quantized_artifacts,
        &CancellationToken::default(),
    )
    .unwrap();
    let raw_analysis: SingingAnalysisV1 = serde_json::from_slice(
        &fs::read(
            quantized_root.join(&quantized_artifacts.singing_analysis.as_ref().unwrap().path),
        )
        .unwrap(),
    )
    .unwrap();
    let quantized_chart: CandidateVocalChartV1 = serde_json::from_slice(
        &fs::read(
            quantized_root.join(
                &quantized_artifacts
                    .candidate_vocal_chart
                    .as_ref()
                    .unwrap()
                    .path,
            ),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(raw_analysis.track.is_none());
    assert_eq!(raw_analysis.chart_references.note_ids, ["game-note-0"]);
    let quantized_note = &quantized_chart.tracks[0].phrases[0].notes[0];
    assert_eq!(quantized_note.id, "game-note-0");
    assert_eq!(quantized_note.start, quantized_track.notes[0].range.start);
    assert_eq!(
        quantized_note.duration,
        quantized_track.notes[0].range.end - quantized_track.notes[0].range.start
    );
    let quantized_manifest = AnalysisResultManifestV1 {
        contract: ANALYSIS_RESULT_CONTRACT.to_string(),
        version: ANALYSIS_RESULT_VERSION,
        request_id: "quantized-fixture".to_string(),
        status: AnalysisStatus::Ok,
        artifacts: quantized_artifacts,
        diagnostics: AnalysisDiagnosticsV1 {
            quantization: Some(report),
            ..AnalysisDiagnosticsV1::default()
        },
        provenance: AnalysisProvenanceV1 {
            resources: Vec::new(),
            calibration_version: CALIBRATION_VERSION.to_string(),
            fusion_version: FUSION_VERSION.to_string(),
            fusion_decision: Some(decision_provenance.clone()),
            quantization_version: QUANTIZATION_VERSION.to_string(),
            audio_quality_version: String::new(),
            postprocess_version: POSTPROCESS_VERSION.to_string(),
        },
        fingerprint: fingerprint.clone(),
        degraded_reasons: Vec::new(),
    };
    quantized_manifest.validate().unwrap();

    let manifest = AnalysisResultManifestV1 {
        contract: ANALYSIS_RESULT_CONTRACT.to_string(),
        version: ANALYSIS_RESULT_VERSION,
        request_id: "pure-fixture".to_string(),
        status: AnalysisStatus::Ok,
        artifacts,
        diagnostics: AnalysisDiagnosticsV1::default(),
        provenance: AnalysisProvenanceV1 {
            resources: Vec::new(),
            calibration_version: CALIBRATION_VERSION.to_string(),
            fusion_version: FUSION_VERSION.to_string(),
            fusion_decision: Some(decision_provenance),
            quantization_version: QUANTIZATION_VERSION.to_string(),
            audio_quality_version: String::new(),
            postprocess_version: POSTPROCESS_VERSION.to_string(),
        },
        fingerprint,
        degraded_reasons: Vec::new(),
    };
    manifest.validate().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelled_candidate_publication_writes_no_artifact() {
    let root = std::env::temp_dir().join(format!(
        "uta-engine-cancelled-candidate-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let token = CancellationToken::default();
    token.cancel();
    let mut artifacts = AnalysisArtifactsV1::default();
    let error = publish_candidate_artifacts(
        &root,
        true,
        true,
        true,
        &"8".repeat(64),
        None,
        None,
        None,
        None,
        &mut artifacts,
        &token,
    )
    .unwrap_err();
    assert_eq!(error.code, EngineErrorCode::Cancelled);
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    fs::remove_dir(root).unwrap();
}

#[cfg(unix)]
fn executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    let staging = path.with_extension("part");
    fs::write(&staging, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut permissions = fs::metadata(&staging).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&staging, permissions).unwrap();
    fs::rename(staging, path).unwrap();
}

#[cfg(unix)]
fn non_silent_pcm_script(sample_count: usize) -> String {
    format!(
        "LC_ALL=C awk 'BEGIN {{ for (i=0; i<{sample_count}; i++) printf \"%c%c%c%c\",0,0,128,62 }}'"
    )
}

fn install_fixture_generation(
    root: &Path,
    model_id: &str,
    source_override: Option<uta_runtime_manager::SourceIdentity>,
) {
    let resource = ResourceRef::model(model_id).unwrap();
    let payload = b"fixture model";
    let payload_sha = format!("{:x}", Sha256::digest(payload));
    let model = ResourceCatalog::default_catalog()
        .unwrap()
        .model(model_id)
        .unwrap()
        .clone();
    let source = source_override.unwrap_or(model.source.clone());
    let artifact_filename = model
        .runtime_artifacts
        .iter()
        .find(|artifact| artifact.name == "model")
        .map(|artifact| artifact.filename.clone())
        .expect("every catalog model declares its named runtime artifact set");
    let manifest = InstallManifest {
        schema: uta_runtime_manager::manifest::INSTALL_MANIFEST_SCHEMA.to_string(),
        schema_version: None,
        resource: resource.clone(),
        catalog_version: None,
        source: Some(source.clone()),
        source_sha256: source.sha256,
        model_recipe_digest: Some(model.recipe_digest),
        conversion_recipe_digest: None,
        runtime_recipe_digest: Some(uta_runtime_manager::GGML_RUNTIME_RECIPE_SHA256.to_string()),
        files: vec![InstalledFile {
            path: PathBuf::from(&artifact_filename),
            sha256: payload_sha,
            size: payload.len() as u64,
        }],
        created_timestamp: "fixture".to_string(),
    };
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let generation = uta_runtime_manager::manifest::generation_id(&bytes);
    let directory = root
        .join("models")
        .join(model_id)
        .join("generations")
        .join(&generation);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(&artifact_filename), payload).unwrap();
    fs::write(directory.join("install-manifest.json"), bytes).unwrap();
    fs::write(
        root.join("models").join(model_id).join("current.json"),
        format!(r#"{{"generation":"{generation}"}}"#),
    )
    .unwrap();
}

#[test]
#[cfg(unix)]
fn rmvpe_partial_pipeline_emits_hashed_result_and_stable_fingerprint() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-engine-rmvpe-fixture-{}-{stamp}",
        std::process::id()
    ));
    let store = root.join("store");
    let output_one = root.join("output-one");
    let output_two = root.join("output-two");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&output_one).unwrap();
    fs::create_dir_all(&output_two).unwrap();
    let fixture_source = uta_runtime_manager::SourceIdentity {
        filename: Some("model.bin".to_string()),
        sha256: Some(format!("{:x}", Sha256::digest(b"fixture model"))),
        ..uta_runtime_manager::SourceIdentity::default()
    };
    install_fixture_generation(&store, "rmvpe", Some(fixture_source.clone()));

    let source = root.join("source.wav");
    fs::write(&source, b"authorized fixture audio").unwrap();
    let ffmpeg = root.join("ffmpeg");
    executable(&ffmpeg, &non_silent_pcm_script(480));
    let worker = root.join("ggml-worker");
    let evidence_one = output_one.join("worker/rmvpe/rmvpe-pitch-evidence.json");
    let evidence_two = output_two.join("worker/rmvpe/rmvpe-pitch-evidence.json");
    executable(
        &worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"component\":\"uta-ggml-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nwhile read line; do\ncase \"$line\" in\n*output-one*) out='{}' ;;\n*output-two*) out='{}' ;;\n*quit*) exit 0 ;;\nesac\nmkdir -p \"$(dirname \"$out\")\"\nprintf '%s\\n' '{{\"schema_version\":1,\"model_id\":\"rmvpe\",\"source_model_sha256\":\"{}\",\"model_gguf_sha256\":\"{}\",\"runtime_manifest_sha256\":\"{}\",\"backend\":\"ggml_vulkan\",\"timeline_step_ms\":10,\"sample_rate\":16000,\"frames\":[{{\"time\":0.0,\"hz\":440.25,\"confidence\":0.9,\"voiced\":true}},{{\"time\":0.01,\"hz\":440.0,\"confidence\":0.8,\"voiced\":true}}]}}' > \"$out\"\nprintf '%s\\n' '{{\"type\":\"progress\",\"task_id\":\"task-rmvpe\",\"fraction\":0.5,\"message\":\"measured frame batch\"}}'\nprintf '%s\\n' \"{{\\\"type\\\":\\\"output\\\",\\\"task_id\\\":\\\"task-rmvpe\\\",\\\"artifact\\\":\\\"pitch_evidence\\\",\\\"path\\\":\\\"$out\\\",\\\"media_type\\\":\\\"application/json\\\"}}\"\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"task-rmvpe\",\"status\":\"ok\"}}'\ndone",
            uta_runtime_manager::GGML_RUNTIME_RECIPE_SHA256,
            evidence_one.display(),
            evidence_two.display(),
            "5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd",
            "1b4095d1b57818f5e812b1986ea5a7d7e6d64ccd9e1b1d7b71f4091304513fd2",
            "d".repeat(64),
        ),
    );
    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    catalog.models.get_mut("rmvpe").unwrap().source = fixture_source;
    let manager = RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&store)
            .with_runtime_override("ggml_vulkan", &worker)
            .with_tool_override("ffmpeg", &ffmpeg),
    );
    let engine = AnalysisEngine::new(manager);
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.request_id = "rmvpe-fixture".to_string();
    request.audio_sources[0].path = source.clone();
    request.audio_sources[0].sha256 = "a".repeat(64);
    request.audio_sources[0].timeline.source_start = 2_000_000;
    request.requested_artifacts.vocal_chart = false;
    request.requested_artifacts.singing_analysis = false;
    request.requested_artifacts.transcript = false;
    request.requested_artifacts.alignment = false;
    request.requested_artifacts.pitch_evidence = true;

    let planned_gates = engine.plan(&request).unwrap().quality_gates;
    let lifecycle_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let lifecycle_target = std::sync::Arc::clone(&lifecycle_events);
    let first = engine
        .analyze_with_events(
            &request,
            &output_one,
            &CancellationToken::default(),
            std::sync::Arc::new(move |event| {
                lifecycle_target.lock().unwrap().push(event);
            }),
        )
        .unwrap();
    let second = engine.analyze(&request, &output_two).unwrap();
    let lifecycle_events = lifecycle_events.lock().unwrap();
    assert!(lifecycle_events.iter().any(|event| {
        event.kind == crate::events::EngineLifecycleKindV1::NodeStarted
            && event.capability_id == "pitch.track"
            && event.model_id.as_deref() == Some("rmvpe")
    }));
    assert!(lifecycle_events.iter().any(|event| {
        event.kind == crate::events::EngineLifecycleKindV1::NodeProgress && event.progress.is_some()
    }));
    assert!(lifecycle_events.iter().any(|event| {
        event.kind == crate::events::EngineLifecycleKindV1::NodeCompleted
            && event.capability_id == "pitch.track"
    }));
    assert_eq!(first.status, AnalysisStatus::Ok);
    let quality = first
        .diagnostics
        .audio_quality
        .as_ref()
        .expect("every executable Plan gate has a typed result");
    assert_eq!(quality.planned_gates, planned_gates);
    assert_eq!(
        quality
            .outcomes
            .iter()
            .map(|outcome| outcome.gate.clone())
            .collect::<Vec<_>>(),
        planned_gates
    );
    assert_eq!(
        first.provenance.audio_quality_version,
        AUDIO_QUALITY_VERSION
    );
    assert_eq!(first.fingerprint, second.fingerprint);
    let pitch = first.artifacts.pitch_evidence.unwrap();
    assert_eq!(pitch.media_type, PITCH_MEDIA_TYPE);
    assert_eq!(pitch.sha256.len(), 64);
    let pitch_value: PitchEvidenceV03 =
        serde_json::from_slice(&fs::read(output_one.join(&pitch.path)).unwrap()).unwrap();
    assert_eq!(pitch_value.start, 2_000_000);
    assert!(output_one.join("analysis-result.json").is_file());
    assert!(first.artifacts.candidate_vocal_chart.is_none());
    fs::remove_dir_all(root).unwrap();
}
