use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use uta_runtime_manager::{
    InstallManifest, InstalledFile, ResourceCatalog, ResourceRef, StorePaths,
};

use super::*;
use crate::artifact::{PitchEvidenceV03, TranscriptArtifactV1};
use crate::contract::request::tests::valid_request;
use crate::contract::{AudioRole, TIMELINE_VALID_GATE};
use crate::fusion::{BoundaryCandidateRole, BoundaryEvidenceKind};

mod fingerprint_versions;
mod phase_b;
mod phase_e;

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
            source_experts: vec!["qwen-align".to_string()],
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
            schema_version: 2,
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
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
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
    let source = source_override.unwrap_or(model.source);
    let manifest = InstallManifest {
        schema: uta_runtime_manager::manifest::INSTALL_MANIFEST_SCHEMA.to_string(),
        schema_version: uta_runtime_manager::manifest::INSTALL_MANIFEST_SCHEMA_VERSION,
        resource: resource.clone(),
        catalog_version: uta_runtime_manager::catalog::RUNTIME_CATALOG_VERSION.to_string(),
        source: Some(source.clone()),
        source_sha256: source.sha256,
        model_recipe_digest: Some(model.recipe_digest),
        conversion_recipe_digest: None,
        runtime_recipe_digest: Some(match model_id {
            "qwen3_asr_1_7b" => {
                uta_runtime_manager::runtime_recipe_digest("qwen3_asr_1_7b").unwrap()
            }
            "qwen3_forced_aligner_0_6b" => {
                uta_runtime_manager::runtime_recipe_digest("qwen3_forced_aligner_0_6b").unwrap()
            }
            _ => uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256.to_string(),
        }),
        files: vec![InstalledFile {
            path: PathBuf::from("model.bin"),
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
    fs::write(directory.join("model.bin"), payload).unwrap();
    fs::write(directory.join("install-manifest.json"), bytes).unwrap();
    fs::write(
        root.join("models").join(model_id).join("current.json"),
        format!(r#"{{"generation":"{generation}"}}"#),
    )
    .unwrap();
}

fn fcpe_primary_candidate_workflow() -> serde_json::Value {
    let node = |instance: &str,
                capability: &str,
                model: Option<&str>,
                priority: i32,
                _runtime: &str,
                _parameters: serde_json::Value| {
        let mut value = serde_json::json!({
            "instance_id": instance,
            "capability_id": capability,
            "execution_policy": "always",
            "priority": priority,
            "provider_preferences": {
                "primary": model,
                "instrumental": null
            }
        });
        if capability == "audio.separate_vocal_bgm" {
            value["execution_invocations"] = serde_json::json!([{
                "invocation_id": format!("{instance}.vocal"),
                "provider_id": model.expect("separation fixture has a provider"),
                "capabilities": ["audio.extract_vocals"],
                "output_ports": ["vocal"]
            }]);
        }
        value
    };
    let binding = |from_node: &str,
                   from_port: &str,
                   to_node: &str,
                   to_port: &str,
                   semantic_type: &str,
                   audio_role: Option<&str>,
                   analyzer_attachment: bool| {
        serde_json::json!({
            "from_node": from_node,
            "from_port": from_port,
            "to_node": to_node,
            "to_port": to_port,
            "semantic_type": semantic_type,
            "audio_role": audio_role,
            "execution_active": true,
            "analyzer_attachment": analyzer_attachment
        })
    };
    serde_json::json!({
        "contract": "uta.workflow-execution",
        "version": 1,
        "workflow_schema_version": 2,
        "workflow_id": "fcpe-primary-candidate",
        "workflow_revision": 1,
        "quality_mode": "fast",
        "definition_digest": "fcpe-primary-candidate-fixture-v1",
        "nodes": [
            node("source", "audio.source", None, 1000, "native_dsp", serde_json::json!({})),
            node(
                "vocal_split",
                "audio.separate_vocal_bgm",
                Some("bs_roformer_vocals_ep317"),
                900,
                "vulkan",
                serde_json::json!({})
            ),
            node(
                "lead_isolate",
                "audio.lead_isolate",
                Some("melband_roformer_harmony"),
                880,
                "vulkan",
                serde_json::json!({})
            ),
            node(
                "asr_qwen",
                "analysis.asr",
                Some("qwen3_asr_1_7b"),
                700,
                "pinned_qwen_asr_vulkan",
                serde_json::json!({})
            ),
            node(
                "transcript_fusion",
                "fusion.transcript",
                None,
                600,
                "native_dsp",
                serde_json::json!({})
            ),
            node(
                "forced_alignment",
                "analysis.forced_alignment",
                Some("qwen3_forced_aligner_0_6b"),
                590,
                "pinned_qwen_align_vulkan",
                serde_json::json!({})
            ),
            node(
                "f0_fcpe",
                "analysis.pitch_f0",
                Some("fcpe"),
                680,
                "openvino",
                serde_json::json!({})
            ),
            node(
                "boundary_game",
                "analysis.note_boundary",
                Some("game"),
                660,
                "openvino",
                serde_json::json!({})
            ),
            node(
                "evidence_fusion",
                "fusion.singing_evidence",
                None,
                500,
                "native_dsp",
                serde_json::json!({
                    "pitch_owner": "fcpe",
                    "boundary_owner": "game",
                    "onset_owner": "automatic"
                })
            ),
            node(
                "candidate_graph",
                "fusion.candidate_graph",
                None,
                400,
                "native_dsp",
                serde_json::json!({})
            ),
            node(
                "canonical_track",
                "finalize.canonical_singing_track",
                None,
                300,
                "native_dsp",
                serde_json::json!({})
            )
        ],
        "bindings": [
            binding("source", "mix", "vocal_split", "audio", "audio", Some("source_mix"), false),
            binding("vocal_split", "vocal", "lead_isolate", "audio", "audio", Some("vocal"), false),
            binding("lead_isolate", "lead", "asr_qwen", "audio", "audio", Some("lead_vocal"), true),
            binding("asr_qwen", "transcript", "transcript_fusion", "evidence", "transcript_evidence", None, false),
            binding("lead_isolate", "lead", "forced_alignment", "audio", "audio", Some("lead_vocal"), true),
            binding("transcript_fusion", "lyrics", "forced_alignment", "lyrics", "lyrics", None, false),
            binding("lead_isolate", "lead", "f0_fcpe", "audio", "audio", Some("lead_vocal"), true),
            binding("lead_isolate", "lead", "boundary_game", "audio", "audio", Some("lead_vocal"), true),
            binding("f0_fcpe", "pitch", "evidence_fusion", "pitch", "pitch_evidence", None, false),
            binding("boundary_game", "boundaries", "evidence_fusion", "boundaries", "boundary_evidence", None, false),
            binding("forced_alignment", "alignment", "evidence_fusion", "alignment", "alignment_evidence", None, false),
            binding("evidence_fusion", "evidence", "candidate_graph", "evidence", "evidence_bundle", None, false),
            binding("candidate_graph", "candidates", "canonical_track", "candidates", "candidate_graph", None, false),
            binding("transcript_fusion", "lyrics", "canonical_track", "lyrics", "lyrics", None, false)
        ],
        "terminal_outputs": [{
            "node": "canonical_track",
            "port": "chart",
            "semantic_type": "candidate_chart"
        }],
        "fusion_policy": {
            "continuous_f0": "fcpe",
            "note_lengths": "game",
            "onset_support": "automatic"
        }
    })
}

#[test]
#[cfg(unix)]
fn unavailable_optional_fcpe_resolution_is_explicitly_degraded() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-engine-optional-fcpe-{}-{stamp}",
        std::process::id()
    ));
    let store = root.join("store");
    fs::create_dir_all(&store).unwrap();
    let rmvpe_source = uta_runtime_manager::SourceIdentity {
        filename: Some("model.bin".to_string()),
        sha256: Some(format!("{:x}", Sha256::digest(b"fixture model"))),
        ..uta_runtime_manager::SourceIdentity::default()
    };
    install_fixture_generation(&store, "rmvpe", Some(rmvpe_source.clone()));
    let worker = root.join("openvino-worker");
    let ffmpeg = root.join("ffmpeg");
    executable(&worker, "exit 0");
    executable(&ffmpeg, "exit 0");
    let manager = || {
        let mut catalog = ResourceCatalog::default_catalog().unwrap();
        catalog.models.get_mut("rmvpe").unwrap().source = rmvpe_source.clone();
        RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&store)
                .with_runtime_override("openvino_2026_3", &worker)
                .with_tool_override("ffmpeg", &ffmpeg),
        )
    };
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.analysis.profile = crate::contract::AnalysisProfile::Balanced;
    request.requested_artifacts.vocal_chart = false;
    request.requested_artifacts.singing_analysis = false;
    request.requested_artifacts.transcript = false;
    request.requested_artifacts.alignment = false;
    request.requested_artifacts.pitch_evidence = true;
    request.execution_policy.runtime_policy = uta_runtime_manager::RuntimePolicy::Benchmark;

    let engine = AnalysisEngine::new(manager());
    let plan = engine.plan(&request).unwrap();
    let (resolved, degraded) = engine.resolve_execution_resources(&request, &plan).unwrap();
    assert!(resolved.iter().any(|model| model.model_id == "rmvpe"));
    assert!(!resolved.iter().any(|model| model.model_id == "fcpe"));
    assert!(
        degraded
            .iter()
            .any(|reason| reason.contains("pitch.secondary"))
    );

    fs::remove_dir_all(root).unwrap();
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
    let worker = root.join("openvino-worker");
    let evidence_one = output_one.join("worker/rmvpe/rmvpe-pitch-evidence.json");
    let evidence_two = output_two.join("worker/rmvpe/rmvpe-pitch-evidence.json");
    executable(
        &worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nwhile read line; do\ncase \"$line\" in\n*output-one*) out='{}' ;;\n*output-two*) out='{}' ;;\n*quit*) exit 0 ;;\nesac\nmkdir -p \"$(dirname \"$out\")\"\nprintf '%s\\n' '{{\"schema_version\":1,\"model_id\":\"rmvpe\",\"source_model_sha256\":\"{}\",\"model_manifest_sha256\":\"{}\",\"model_bin_sha256\":\"{}\",\"runtime_manifest_sha256\":\"{}\",\"backend\":\"openvino_gpu\",\"timeline_step_ms\":10,\"sample_rate\":16000,\"frames\":[{{\"time\":0.0,\"hz\":440.25,\"confidence\":0.9,\"voiced\":true}},{{\"time\":0.01,\"hz\":440.0,\"confidence\":0.8,\"voiced\":true}}]}}' > \"$out\"\nprintf '%s\\n' '{{\"type\":\"progress\",\"task_id\":\"task-rmvpe\",\"fraction\":0.5,\"message\":\"measured frame batch\"}}'\nprintf '%s\\n' \"{{\\\"type\\\":\\\"output\\\",\\\"task_id\\\":\\\"task-rmvpe\\\",\\\"artifact\\\":\\\"pitch_evidence\\\",\\\"path\\\":\\\"$out\\\",\\\"media_type\\\":\\\"application/json\\\"}}\"\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"task-rmvpe\",\"status\":\"ok\"}}'\ndone",
            uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
            evidence_one.display(),
            evidence_two.display(),
            "5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd",
            "cdaf2775d8e17796daad2415bdaf7b3c915c4142fd92587c023e8d7b1b3d39fb",
            "d284ea1b4a0908072b6f0a5a1298cb510a65752db7a287e48da6eab1246be67b",
            "d".repeat(64),
        ),
    );
    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    catalog.models.get_mut("rmvpe").unwrap().source = fixture_source;
    let manager = RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&store)
            .with_runtime_override("openvino_2026_3", &worker)
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

#[test]
#[cfg(unix)]
fn basic_pitch_helper_uses_resolved_worker_and_parses_schema_three() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-engine-basic-pitch-fixture-{}-{stamp}",
        std::process::id()
    ));
    let store = root.join("store");
    let output = root.join("output");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&output).unwrap();
    let fixture_source = uta_runtime_manager::SourceIdentity::default();
    install_fixture_generation(&store, "basic_pitch", Some(fixture_source.clone()));
    let source = root.join("source.wav");
    fs::write(&source, b"authorized fixture audio").unwrap();
    let evidence = output.join("worker/basic-pitch/basic-pitch-activation-evidence.json");
    let worker = root.join("openvino-worker");
    executable(
        &worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\nmkdir -p '{}'\nprintf '%s\\n' '{{\"schema_version\":3,\"model_id\":\"basic_pitch\",\"source_model_sha256\":\"2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec\",\"model_manifest_sha256\":\"01b35925daaeb40995f4e49b495e6f1ce9db47c7f41987b19fdc1b5c35f2c1b7\",\"model_xml_sha256\":\"9df134bf18c66dde7b678be49329299ff6ca13be465f3df5b10ff38a75e5aa34\",\"model_bin_sha256\":\"50856c2bac689bb6fdc43ae21818e2a63c37f35207dc5adea22d52fc601efab3\",\"runtime_manifest_sha256\":\"{}\",\"backend\":\"openvino_gpu\",\"sample_rate\":22050,\"window_samples\":43844,\"window_hop_samples\":36164,\"fft_hop_samples\":256,\"overlap_frames\":30,\"padding_samples\":3840,\"frames_per_window\":172,\"owned_frames_per_window\":142,\"frames\":[{{\"time\":0.0,\"note_max\":0.2,\"onset_max\":0.3,\"contour_class\":4,\"contour_score\":0.4}},{{\"time\":0.011609977324263039,\"note_max\":0.5,\"onset_max\":0.6,\"contour_class\":5,\"contour_score\":0.7}}]}}' > '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-basic-pitch\",\"artifact\":\"basic_pitch_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"task-basic-pitch\",\"status\":\"ok\"}}'\nread quit",
            uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
            evidence.parent().unwrap().display(),
            "e".repeat(64),
            evidence.display(),
            evidence.display(),
        ),
    );
    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    catalog.models.get_mut("basic_pitch").unwrap().source = fixture_source;
    let manager = RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&store)
            .with_runtime_override("openvino_2026_3", &worker),
    );
    let model = manager
        .resolve_model("basic_pitch", uta_runtime_manager::RuntimePolicy::Benchmark)
        .unwrap();
    let parsed = run_basic_pitch_schedule(
        &model,
        std::path::Path::new("unused-for-full-input"),
        &source,
        &output,
        TimeRange {
            start: 1_000_000,
            end: 1_020_000,
        },
        &ScheduledExecution::FullInput,
        &CancellationToken::default(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(parsed.frames.len(), 2);
    assert_eq!(parsed.frames[1].time, 1_011_610);
    assert_eq!(parsed.frames[1].onset_activation, 0.6);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn advanced_note_helper_correlates_timed_transcript_and_resolved_generation() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-engine-advanced-note-fixture-{}-{stamp}",
        std::process::id()
    ));
    let store = root.join("store");
    let output = root.join("output");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&output).unwrap();
    let fixture_source = uta_runtime_manager::SourceIdentity::default();
    install_fixture_generation(&store, "rosvot", Some(fixture_source.clone()));
    let current: serde_json::Value =
        serde_json::from_slice(&fs::read(store.join("models/rosvot/current.json")).unwrap())
            .unwrap();
    let generation = current["generation"].as_str().unwrap();
    let source_start = 1_000_000;
    let source_duration = 1_000_000;
    let words = vec![crate::fusion::CanonicalWordBoundary {
        word_id: "word-0".to_string(),
        text: "sing".to_string(),
        range: crate::fusion::TimeRange::new(1_000_000, 1_500_000).unwrap(),
        confidence: None,
        disagreement: None,
        source_experts: vec!["qwen3_forced_aligner_0_6b".to_string()],
    }];
    let word_config = vec![serde_json::json!({
        "id": "word-0",
        "text": "sing",
        "start": 1_000_000,
        "duration": 500_000
    })];
    let timed_generation = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&serde_json::json!({
                "schema": "uta.timed-transcript/1",
                "source_start": source_start,
                "source_duration": source_duration,
                "words": word_config
            }))
            .unwrap()
        )
    );
    let fixture = root.join("advanced-note-evidence.json");
    let evidence = serde_json::json!({
        "schema_version": 1,
        "model_id": "rosvot",
        "capability": "notes.rosvot",
        "upstream_commit": "3c8332bf43adae35f6e4d64971862f2f6139b310",
        "checkpoint_sha256": "7501fb5f913d971c2f51bcb3063b930027b03206581820a4d2bfdc394c9c3fcb",
        "config_sha256": "2ad2cb756623418c471b7dc2f56175cce88b69a70b4a2c354fa1a78525aa54e2",
        "model_generation": generation,
        "runtime_manifest_sha256": "a".repeat(64),
        "backend": "openvino_gpu",
        "shared_frontend_profile": "shared-singing-frontend-24k-v1",
        "shared_frontend_generation": "986327618f2055873a98fca481893db83ffff2e386b6c522532a5272a1597a2c",
        "annotation_rmvpe_sha256": "19dc1809cf4cdb0a18db93441816bc327e14e5644b72eeaae5220560c6736fe2",
        "word_boundary_source": "timed_transcript",
        "frame_step_num": 128,
        "frame_step_den": 24_000,
        "valid_frames": 188,
        "note_boundary_logits": vec![0.0_f32; 188],
        "regulated_note_boundaries": [10],
        "notes": [{
            "start_frame": 0,
            "end_frame": 10,
            "pitch_logits": vec![0.0_f32; 89],
            "midi": 69
        }],
        "dependencies": [
            {"kind":"shared_frontend","generation":"986327618f2055873a98fca481893db83ffff2e386b6c522532a5272a1597a2c"},
            {"kind":"annotation_rmvpe","generation":"986327618f2055873a98fca481893db83ffff2e386b6c522532a5272a1597a2c"},
            {"kind":"timed_transcript","generation":timed_generation}
        ]
    });
    fs::write(&fixture, serde_json::to_vec(&evidence).unwrap()).unwrap();
    let source = root.join("source.wav");
    fs::write(&source, b"authorized fixture audio").unwrap();
    let output_evidence = output.join("worker/rosvot/advanced-note-evidence.json");
    let worker = root.join("openvino-worker");
    executable(
        &worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\ncase \"$run\" in *'\"model_generation\":\"{}\"'*) ;; *) exit 9 ;; esac\ncase \"$run\" in *'\"timed_transcript_generation\":\"{}\"'*) ;; *) exit 9 ;; esac\ncase \"$run\" in *'\"id\":\"word-0\"'*) ;; *) exit 9 ;; esac\ncase \"$run\" in *'\"device\":\"gpu\"'*) ;; *) exit 9 ;; esac\ncp '{}' '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-rosvot\",\"artifact\":\"advanced_note_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"task-rosvot\",\"status\":\"ok\"}}'\nread quit",
            uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
            generation,
            evidence["dependencies"][2]["generation"].as_str().unwrap(),
            fixture.display(),
            output_evidence.display(),
            output_evidence.display(),
        ),
    );
    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    catalog.models.get_mut("rosvot").unwrap().source = fixture_source;
    let manager = RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&store)
            .with_runtime_override("openvino_2026_3", &worker),
    );
    let model = manager
        .resolve_model("rosvot", uta_runtime_manager::RuntimePolicy::Benchmark)
        .unwrap();
    let parsed = run_advanced_note_challenger(
        &model,
        &source,
        &output,
        &words,
        source_start,
        source_duration,
        false,
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(parsed.model_generation, model.generation);
    assert_eq!(
        parsed
            .canonical_notes(source_start, source_duration)
            .unwrap()
            .len(),
        1
    );
    let provenance = parsed.provenance();
    assert_eq!(provenance.expert_id, "rosvot");
    assert!(
        provenance
            .correlation_group
            .as_deref()
            .unwrap()
            .contains("timed_transcript:")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires explicit local advanced-note package and native worker paths"]
fn real_advanced_note_helper_cpu_consumes_pinned_generation() {
    let store = PathBuf::from(
        std::env::var("UTA_STUDIO_TEST_ADVANCED_STORE")
            .expect("UTA_STUDIO_TEST_ADVANCED_STORE is required"),
    );
    let worker = PathBuf::from(
        std::env::var("UTA_STUDIO_TEST_ADVANCED_WORKER")
            .expect("UTA_STUDIO_TEST_ADVANCED_WORKER is required"),
    );
    let audio = PathBuf::from(
        std::env::var("UTA_STUDIO_TEST_ADVANCED_AUDIO")
            .expect("UTA_STUDIO_TEST_ADVANCED_AUDIO is required"),
    );
    let model_id = std::env::var("UTA_STUDIO_TEST_ADVANCED_MODEL")
        .expect("UTA_STUDIO_TEST_ADVANCED_MODEL is required");
    assert!(matches!(model_id.as_str(), "stars" | "rosvot"));
    let include_technique = model_id == "stars"
        && std::env::var_os("UTA_STUDIO_TEST_ADVANCED_INCLUDE_TECHNIQUE").is_some();
    assert_eq!(
        std::env::var("UTA_STUDIO_ADVANCED_NOTE_DIAGNOSTIC_DEVICE").as_deref(),
        Ok("cpu")
    );
    let manager = RuntimeManager::new(
        ResourceCatalog::default_catalog().unwrap(),
        StorePaths::default()
            .with_store_root(store)
            .with_runtime_override("openvino_2026_3", worker),
    );
    let model = manager
        .resolve_model(&model_id, uta_runtime_manager::RuntimePolicy::Benchmark)
        .unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let output = std::env::temp_dir().join(format!(
        "uta-engine-real-{model_id}-cpu-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&output).unwrap();
    let duration = std::env::var("UTA_STUDIO_TEST_ADVANCED_DURATION_US")
        .ok()
        .map(|value| value.parse::<u64>().unwrap())
        .unwrap_or(1_000_000);
    let words = vec![crate::fusion::CanonicalWordBoundary {
        word_id: "word-0".to_string(),
        text: if model_id == "stars" {
            "你好世界"
        } else {
            "la"
        }
        .to_string(),
        range: crate::fusion::TimeRange::new(0, duration).unwrap(),
        confidence: None,
        disagreement: None,
        source_experts: vec!["timed-transcript-fixture".to_string()],
    }];
    let evidence = run_advanced_note_challenger(
        &model,
        &audio,
        &output,
        &words,
        0,
        duration,
        include_technique,
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(evidence.model_id, model_id);
    assert_eq!(evidence.model_generation, model.generation);
    assert_eq!(evidence.backend, "openvino_cpu");
    assert!(!evidence.notes.is_empty());
    assert_eq!(
        evidence.canonical_notes(0, duration).unwrap().len(),
        evidence.notes.len()
    );
    let claimed_pitch = evidence
        .notes
        .iter()
        .filter(|note| note.midi.is_some())
        .count();
    let evidence_path = output
        .join("worker")
        .join(&model_id)
        .join("advanced-note-evidence.json");
    let first_bytes = fs::read(&evidence_path).unwrap();
    println!(
        "real advanced-note CPU evidence: model={} frames={} notes={} claimed_pitch={claimed_pitch} sha256={:x}",
        evidence.model_id,
        evidence.valid_frames,
        evidence.notes.len(),
        Sha256::digest(&first_bytes)
    );
    if std::env::var_os("UTA_STUDIO_TEST_ADVANCED_REQUIRE_PITCH").is_some() {
        assert!(claimed_pitch > 0, "semantic fixture produced no MIDI claim");
    }
    assert!(evidence.provenance().correlation_group.is_some());
    if include_technique {
        let technique = evidence.technique_artifact(0, duration).unwrap().unwrap();
        assert_eq!(technique.taxonomy.len(), 9);
        assert!(!technique.intervals.is_empty());
        assert!(!technique.styles.is_empty());
        assert_eq!(
            technique.provenance.task,
            crate::fusion::ExpertTask::Technique
        );
        assert!(technique.provenance.correlation_group.is_some());
    }
    if std::env::var_os("UTA_STUDIO_TEST_ADVANCED_REPEAT").is_some() {
        let repeat_output = output.with_extension("repeat");
        fs::create_dir(&repeat_output).unwrap();
        let repeated = run_advanced_note_challenger(
            &model,
            &audio,
            &repeat_output,
            &words,
            0,
            duration,
            include_technique,
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(repeated, evidence);
        assert_eq!(
            fs::read(
                repeat_output
                    .join("worker")
                    .join(&model_id)
                    .join("advanced-note-evidence.json")
            )
            .unwrap(),
            first_bytes
        );
        fs::remove_dir_all(repeat_output).unwrap();
    }
    fs::remove_dir_all(output).unwrap();
}

#[test]
#[ignore = "requires explicit local advanced-note package and native worker paths"]
fn real_advanced_note_helper_active_cancel_has_no_partial_artifact() {
    let store = PathBuf::from(
        std::env::var("UTA_STUDIO_TEST_ADVANCED_STORE")
            .expect("UTA_STUDIO_TEST_ADVANCED_STORE is required"),
    );
    let worker = PathBuf::from(
        std::env::var("UTA_STUDIO_TEST_ADVANCED_WORKER")
            .expect("UTA_STUDIO_TEST_ADVANCED_WORKER is required"),
    );
    let audio = PathBuf::from(
        std::env::var("UTA_STUDIO_TEST_ADVANCED_AUDIO")
            .expect("UTA_STUDIO_TEST_ADVANCED_AUDIO is required"),
    );
    let model_id = std::env::var("UTA_STUDIO_TEST_ADVANCED_MODEL")
        .expect("UTA_STUDIO_TEST_ADVANCED_MODEL is required");
    let include_technique = model_id == "stars"
        && std::env::var_os("UTA_STUDIO_TEST_ADVANCED_INCLUDE_TECHNIQUE").is_some();
    let duration = std::env::var("UTA_STUDIO_TEST_ADVANCED_DURATION_US")
        .expect("UTA_STUDIO_TEST_ADVANCED_DURATION_US is required")
        .parse::<u64>()
        .unwrap();
    let manager = RuntimeManager::new(
        ResourceCatalog::default_catalog().unwrap(),
        StorePaths::default()
            .with_store_root(store)
            .with_runtime_override("openvino_2026_3", worker),
    );
    let model = manager
        .resolve_model(&model_id, uta_runtime_manager::RuntimePolicy::Benchmark)
        .unwrap();
    let output = std::env::temp_dir().join(format!(
        "uta-engine-real-{model_id}-cancel-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&output).unwrap();
    let words = vec![crate::fusion::CanonicalWordBoundary {
        word_id: "word-0".to_string(),
        text: if model_id == "stars" {
            "你好世界"
        } else {
            "la"
        }
        .to_string(),
        range: crate::fusion::TimeRange::new(0, duration).unwrap(),
        confidence: None,
        disagreement: None,
        source_experts: vec!["timed-transcript-fixture".to_string()],
    }];
    let cancellation = CancellationToken::default();
    let trigger = cancellation.clone();
    let cancel_after_ms = std::env::var("UTA_STUDIO_TEST_ADVANCED_CANCEL_AFTER_MS")
        .ok()
        .map(|value| value.parse::<u64>().unwrap())
        .unwrap_or(5_000);
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(cancel_after_ms));
        trigger.cancel();
    });
    let error = run_advanced_note_challenger(
        &model,
        &audio,
        &output,
        &words,
        0,
        duration,
        include_technique,
        &cancellation,
    )
    .unwrap_err();
    canceller.join().unwrap();
    assert_eq!(error.code, EngineErrorCode::Cancelled);
    assert!(
        !output
            .join("worker")
            .join(&model_id)
            .join("advanced-note-evidence.json")
            .exists()
    );
    fs::remove_dir_all(output).unwrap();
}

#[test]
#[cfg(unix)]
fn fcpe_secondary_waits_for_typed_disagreement_in_standalone_balanced() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-engine-fcpe-fixture-{}-{stamp}",
        std::process::id()
    ));
    let store = root.join("store");
    let output = root.join("output");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&output).unwrap();
    let rmvpe_source = uta_runtime_manager::SourceIdentity {
        filename: Some("model.bin".to_string()),
        sha256: Some(format!("{:x}", Sha256::digest(b"fixture model"))),
        ..uta_runtime_manager::SourceIdentity::default()
    };
    let fcpe_source = uta_runtime_manager::SourceIdentity::default();
    install_fixture_generation(&store, "rmvpe", Some(rmvpe_source.clone()));
    install_fixture_generation(&store, "fcpe", Some(fcpe_source.clone()));
    let source = root.join("source.wav");
    fs::write(&source, b"authorized fixture audio").unwrap();
    let ffmpeg = root.join("ffmpeg");
    executable(&ffmpeg, &non_silent_pcm_script(48_000));
    let rmvpe_output = output.join("worker/rmvpe/rmvpe-pitch-evidence.json");
    let fcpe_output = output.join("worker/fcpe/fcpe-pitch-evidence.json");
    let worker = root.join("openvino-worker");
    executable(
        &worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\ncase \"$run\" in\n*'\"model_id\":\"rmvpe\"'*) task=task-rmvpe; mkdir -p '{}'; printf '%s\\n' '{{\"schema_version\":1,\"model_id\":\"rmvpe\",\"source_model_sha256\":\"5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd\",\"model_manifest_sha256\":\"cdaf2775d8e17796daad2415bdaf7b3c915c4142fd92587c023e8d7b1b3d39fb\",\"model_bin_sha256\":\"d284ea1b4a0908072b6f0a5a1298cb510a65752db7a287e48da6eab1246be67b\",\"runtime_manifest_sha256\":\"{}\",\"backend\":\"openvino_gpu\",\"timeline_step_ms\":10,\"sample_rate\":16000,\"frames\":[{{\"time\":0.0,\"hz\":440.0,\"confidence\":0.9,\"voiced\":true}},{{\"time\":0.01,\"hz\":441.0,\"confidence\":0.9,\"voiced\":true}}]}}' > '{}'; printf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-rmvpe\",\"artifact\":\"pitch_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}';;\n*) task=task-fcpe; mkdir -p '{}'; printf '%s\\n' '{{\"schema_version\":3,\"model_id\":\"fcpe\",\"source_model_sha256\":\"b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0\",\"model_manifest_sha256\":\"bd356b9d018bbf55f7b87bbc8e4a712496b587a306249c941ff30beb5d548df6\",\"model_xml_sha256\":\"9941d7251ff0bdedc7875cabd40c30c2c60db00b36a617c9e957044d669bc237\",\"model_bin_sha256\":\"6b6c62535552181c9efe305837af09a2a8987585ce368b2c522242b59676f824\",\"runtime_manifest_sha256\":\"{}\",\"backend\":\"openvino_gpu\",\"timeline_step_ms\":10,\"sample_rate\":16000,\"window_samples\":32000,\"window_hop_samples\":32000,\"frames\":[{{\"time\":0.0,\"hz\":440.2}},{{\"time\":0.01,\"hz\":440.8}}]}}' > '{}'; printf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-fcpe\",\"artifact\":\"pitch_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}';;\nesac\nprintf '%s\\n' \"{{\\\"type\\\":\\\"done\\\",\\\"task_id\\\":\\\"$task\\\",\\\"status\\\":\\\"ok\\\"}}\"\nread quit",
            uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
            rmvpe_output.parent().unwrap().display(),
            "c".repeat(64),
            rmvpe_output.display(),
            rmvpe_output.display(),
            fcpe_output.parent().unwrap().display(),
            "d".repeat(64),
            fcpe_output.display(),
            fcpe_output.display(),
        ),
    );
    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    catalog.models.get_mut("rmvpe").unwrap().source = rmvpe_source;
    catalog.models.get_mut("fcpe").unwrap().source = fcpe_source;
    let manager = RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&store)
            .with_runtime_override("openvino_2026_3", &worker)
            .with_tool_override("ffmpeg", &ffmpeg),
    );
    let engine = AnalysisEngine::new(manager);
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.request_id = "fcpe-fixture".to_string();
    request.audio_sources[0].path = source.clone();
    request.audio_sources[0].sha256 = "a".repeat(64);
    request.analysis.profile = crate::contract::AnalysisProfile::Balanced;
    request.execution_policy.runtime_policy = uta_runtime_manager::RuntimePolicy::Benchmark;
    request.requested_artifacts.vocal_chart = false;
    request.requested_artifacts.singing_analysis = false;
    request.requested_artifacts.transcript = false;
    request.requested_artifacts.alignment = false;
    request.requested_artifacts.pitch_evidence = true;
    let plan = engine.plan(&request).unwrap();
    let (_, resolution_degraded) = engine.resolve_execution_resources(&request, &plan).unwrap();
    assert!(resolution_degraded.is_empty(), "{resolution_degraded:?}");
    let result = engine.analyze(&request, &output).unwrap();
    assert_eq!(result.status, AnalysisStatus::Ok);
    assert!(
        result.degraded_reasons.is_empty(),
        "{:?}",
        result.degraded_reasons
    );
    assert!(result.diagnostics.evidence["fcpe_frame_count"].is_null());
    assert!(
        result.diagnostics.evidence["conditional_schedule"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["capability"] == "pitch.secondary.fcpe"
                    && record["decision"] == "skipped:no_relevant_disagreement"
            })
    );
    let pitch_ref = result.artifacts.pitch_evidence.unwrap();
    let pitch: PitchEvidenceV03 =
        serde_json::from_slice(&fs::read(output.join(pitch_ref.path)).unwrap()).unwrap();
    assert_eq!(pitch.model["id"], "rmvpe");
    assert_eq!(
        pitch.confidence,
        [Some(0.9_f32 as f64), Some(0.9_f32 as f64)]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn firered_challenger_runs_full_input_for_typed_reference_disagreement() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-engine-firered-fixture-{}-{stamp}",
        std::process::id()
    ));
    let store = root.join("store");
    let output = root.join("output");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&output).unwrap();
    let fixture_source = uta_runtime_manager::SourceIdentity::default();
    install_fixture_generation(&store, "qwen3_asr_1_7b", Some(fixture_source.clone()));
    install_fixture_generation(&store, "firered_asr2_aed", Some(fixture_source.clone()));
    let source = root.join("source.wav");
    fs::write(&source, b"authorized fixture audio").unwrap();
    let ffmpeg = root.join("ffmpeg");
    executable(&ffmpeg, &non_silent_pcm_script(48_000));

    let qwen_output = output.join("worker/asr/qwen-asr-transcript-evidence.json");
    let qwen_worker = root.join("qwen-asr-worker");
    executable(
        &qwen_worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-qwen-asr-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\nmkdir -p '{}'\nprintf '%s\\n' '{{\"schema_version\":2,\"model_id\":\"qwen3_asr_1_7b\",\"model_sha256\":\"b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e\",\"backend\":\"vulkan\",\"runtime_manifest_sha256\":\"{}\",\"language_contract\":{{\"version\":1,\"explicit_hint_policy\":\"reject\",\"evidence_source\":\"runtime_detected\"}},\"language\":\"en\",\"text\":\"sing now\"}}' > '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-asr\",\"artifact\":\"transcript_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"task-asr\",\"status\":\"ok\"}}'\nread quit",
            uta_runtime_manager::runtime_recipe_digest("qwen3_asr_1_7b").unwrap(),
            qwen_output.parent().unwrap().display(),
            "a".repeat(64),
            qwen_output.display(),
            qwen_output.display(),
        ),
    );
    let firered_output = output.join("worker/firered/firered-transcript-evidence.json");
    let openvino_worker = root.join("openvino-worker");
    executable(
        &openvino_worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\nmkdir -p '{}'\nprintf '%s\\n' '{{\"schema_version\":3,\"model_id\":\"firered_asr2_aed\",\"selected_source_revision\":\"42ailab/FireRedASR2-AED-ONNX@13f950858934f7b6a0d3ce52bae65af0dc022258\",\"source_graph_sha256\":{{\"encoder\":\"0fe4038f5e5cd340171535b7b5f2e184482e90e22aeb2ed0f7abe81af10783f9\",\"decoder\":\"aeef22670d95aa90d78a1927242c2a6e4fbb8b44c1af8d3ae988c46fd67ae833\",\"ctc\":\"8881d31c17bca30a7972299d5395daaa6424da6328a818ba496719c3118c32b4\"}},\"model_manifest_sha256\":\"093335b6a113e5eead88bb011a7870d61f18319e8d0204523c3ce9d82e6c8c35\",\"runtime_manifest_sha256\":\"{}\",\"backend\":\"openvino_gpu\",\"contract_scope\":\"windowed_230_feature_frame_sequence\",\"input_samples\":74398,\"window_samples\":37199,\"window_count\":2,\"feature_frames\":230,\"encoder_frames\":58,\"decoder_cache_max\":10,\"text\":\"你好世界 你好世界\",\"token_ids\":[1202,2246,1019,4710,1202,2246,1019,4710],\"ctc_frames\":58,\"windows\":[{{\"index\":0,\"start_sample\":0,\"end_sample\":37199,\"text\":\"你好世界\",\"token_ids\":[1202,2246,1019,4710]}},{{\"index\":1,\"start_sample\":37199,\"end_sample\":74398,\"text\":\"你好世界\",\"token_ids\":[1202,2246,1019,4710]}}]}}' > '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-firered\",\"artifact\":\"transcript_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"task-firered\",\"status\":\"ok\"}}'\nread quit",
            uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
            firered_output.parent().unwrap().display(),
            "b".repeat(64),
            firered_output.display(),
            firered_output.display(),
        ),
    );
    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    catalog.models.get_mut("qwen3_asr_1_7b").unwrap().source = fixture_source.clone();
    catalog.models.get_mut("firered_asr2_aed").unwrap().source = fixture_source;
    let manager = RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&store)
            .with_runtime_override("qwen_asr_runtime", &qwen_worker)
            .with_runtime_override("openvino_2026_3", &openvino_worker)
            .with_tool_override("ffmpeg", &ffmpeg),
    );
    let engine = AnalysisEngine::new(manager);
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.request_id = "firered-fixture".to_string();
    request.audio_sources[0].path = source.clone();
    request.audio_sources[0].sha256 = "a".repeat(64);
    request.analysis.profile = crate::contract::AnalysisProfile::Balanced;
    request.execution_policy.runtime_policy = uta_runtime_manager::RuntimePolicy::Benchmark;
    request.requested_artifacts.vocal_chart = false;
    request.requested_artifacts.pitch_evidence = false;
    request.requested_artifacts.singing_analysis = false;
    request.requested_artifacts.transcript = true;
    request.requested_artifacts.alignment = false;
    request.lyrics.mode = crate::contract::LyricsMode::Reference;
    request.lyrics.language = Some("en".to_string());
    request.lyrics.tokens = vec![crate::contract::LyricTokenV1 {
        id: "reference-0".to_string(),
        text: "sing know".to_string(),
        reading: None,
        phonemes: None,
        start: None,
        end: None,
    }];
    let result = engine.analyze(&request, &output).unwrap();
    assert_eq!(result.status, AnalysisStatus::Ok);
    assert!(
        result.degraded_reasons.is_empty(),
        "{:?}",
        result.degraded_reasons
    );
    let transcript_ref = result.artifacts.transcript.unwrap();
    let transcript: TranscriptArtifactV1 =
        serde_json::from_slice(&fs::read(output.join(transcript_ref.path)).unwrap()).unwrap();
    assert_eq!(transcript.text, "sing know");
    assert_eq!(transcript.confidence, None);
    assert!(
        transcript
            .alternatives
            .iter()
            .any(|text| text == "sing now")
    );
    assert!(
        transcript
            .alternatives
            .iter()
            .any(|text| text == "你好世界 你好世界")
    );
    assert!(
        result.diagnostics.evidence["conditional_schedule"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["capability"] == "speech.transcribe.challenger"
                    && record["decision"] == "full_input"
            })
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn fcpe_primary_candidate_runs_validate_requirements_plan_and_analyze() {
    use crate::artifact::{CandidateVocalChartV1, SingingAnalysisV1};
    use crate::contract::{LyricTokenV1, LyricsMode};

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-engine-fcpe-primary-candidate-{}-{stamp}",
        std::process::id()
    ));
    let store = root.join("store");
    let output = root.join("output");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&output).unwrap();
    let source_identity = uta_runtime_manager::SourceIdentity::default();
    for model in ["fcpe", "game", "qwen3_forced_aligner_0_6b"] {
        install_fixture_generation(&store, model, Some(source_identity.clone()));
    }

    let source = root.join("source.wav");
    fs::write(&source, b"authorized fixture audio").unwrap();
    let ffmpeg = root.join("ffmpeg");
    executable(&ffmpeg, &non_silent_pcm_script(48_000));

    let fcpe_fixture = root.join("fcpe.json");
    let frames = (0..100)
        .map(|index| serde_json::json!({"time": f64::from(index) / 100.0, "hz": 440.0}))
        .collect::<Vec<_>>();
    fs::write(
        &fcpe_fixture,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 3,
            "model_id": "fcpe",
            "source_model_sha256": "b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0",
            "model_manifest_sha256": "bd356b9d018bbf55f7b87bbc8e4a712496b587a306249c941ff30beb5d548df6",
            "model_xml_sha256": "9941d7251ff0bdedc7875cabd40c30c2c60db00b36a617c9e957044d669bc237",
            "model_bin_sha256": "6b6c62535552181c9efe305837af09a2a8987585ce368b2c522242b59676f824",
            "runtime_manifest_sha256": uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
            "backend": "openvino_gpu",
            "timeline_step_ms": 10,
            "sample_rate": 16000,
            "window_samples": 32000,
            "window_hop_samples": 32000,
            "frames": frames
        }))
        .unwrap(),
    )
    .unwrap();
    let game_fixture = root.join("game.json");
    fs::write(
        &game_fixture,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "model_id": "game",
            "variant": "GAME-1.0.3-medium-onnx",
            "source_asset_sha256": "5b7a21e64c6310efac399f5d12838fffa70565be162436b5a4a65f290721e7d8",
            "source_commit": "475a8ee781fe8cca980b3b12fbe6c80c768a813a",
            "model_manifest_sha256": "aa9f3a4c2d107527913ef3947f337b41bff7b6de39de6c91ce46b82ced15ac87",
            "runtime_manifest_sha256": uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
            "backend": "openvino_gpu",
            "sample_rate": 44100,
            "timestep_ms": 10,
            "d3pm_steps": 8,
            "estimator_note_buckets": [32, 64, 128, 256, 512, 1024],
            "boundary_decision_threshold": 0.2,
            "presence_decision_threshold": 0.2,
            "notes": [{"start": 0.08, "duration": 0.72, "midi": 60.0, "voiced": true}]
        }))
        .unwrap(),
    )
    .unwrap();
    let alignment_fixture = root.join("alignment.json");
    fs::write(
        &alignment_fixture,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "model_id": "qwen3_forced_aligner_0_6b",
            "model_sha256": "c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b",
            "backend": "vulkan",
            "runtime_manifest_sha256": uta_runtime_manager::runtime_recipe_digest("qwen3_forced_aligner_0_6b").unwrap(),
            "text_normalization_profile": "qwen-align-text-preserve-v1",
            "language_normalization_profile": "qwen-align-language-v1",
            "alignment_semantics_profile": "qwen-align-token-word-80ms-v1",
            "transcript": "sing",
            "language": "en",
            "runtime_language": "english",
            "words": [{"word": "sing", "start": 0.08, "end": 0.8}]
        }))
        .unwrap(),
    )
    .unwrap();

    let fcpe_output = output.join("worker/fcpe/fcpe.json");
    let game_output = output.join("worker/game/game.json");
    let openvino = root.join("openvino-worker");
    executable(
        &openvino,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\ncase \"$run\" in\n*'\"model_id\":\"fcpe\"'*) task=task-fcpe; artifact=pitch_evidence; path='{}'; fixture='{}';;\n*) task=task-game; artifact=note_candidate_evidence; path='{}'; fixture='{}';;\nesac\nmkdir -p \"$(dirname \"$path\")\"\ncp \"$fixture\" \"$path\"\nprintf '%s\\n' \"{{\\\"type\\\":\\\"output\\\",\\\"task_id\\\":\\\"$task\\\",\\\"artifact\\\":\\\"$artifact\\\",\\\"path\\\":\\\"$path\\\",\\\"media_type\\\":\\\"application/json\\\"}}\"\nprintf '%s\\n' \"{{\\\"type\\\":\\\"done\\\",\\\"task_id\\\":\\\"$task\\\",\\\"status\\\":\\\"ok\\\"}}\"\nread quit",
            uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
            fcpe_output.display(),
            fcpe_fixture.display(),
            game_output.display(),
            game_fixture.display(),
        ),
    );
    let alignment_output = output.join("worker/alignment/alignment.json");
    let align_worker = root.join("qwen-align-worker");
    executable(
        &align_worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-qwen-align-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\nmkdir -p '{}'\ncp '{}' '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-align\",\"artifact\":\"alignment_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"task-align\",\"status\":\"ok\"}}'\nread quit",
            uta_runtime_manager::runtime_recipe_digest("qwen3_forced_aligner_0_6b").unwrap(),
            alignment_output.parent().unwrap().display(),
            alignment_fixture.display(),
            alignment_output.display(),
            alignment_output.display(),
        ),
    );

    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    for model in ["fcpe", "game", "qwen3_forced_aligner_0_6b"] {
        catalog.models.get_mut(model).unwrap().source = source_identity.clone();
    }
    let engine = AnalysisEngine::new(RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&store)
            .with_tool_override("ffmpeg", &ffmpeg)
            .with_runtime_override("openvino_2026_3", &openvino)
            .with_runtime_override("qwen_align_runtime", &align_worker),
    ));
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.request_id = "fcpe-primary-candidate".to_string();
    request.audio_sources[0].path = source;
    request.lyrics.mode = LyricsMode::Canonical;
    request.lyrics.language = Some("en".to_string());
    request.lyrics.tokens = vec![LyricTokenV1 {
        id: "lyric-1".to_string(),
        text: "sing".to_string(),
        reading: None,
        phonemes: None,
        start: None,
        end: None,
    }];
    request.execution_policy.runtime_policy = uta_runtime_manager::RuntimePolicy::Benchmark;
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        fcpe_primary_candidate_workflow(),
    );

    engine.validate(&request).unwrap();
    let requirements = engine.requirements(&request).unwrap();
    assert!(requirements.resources.iter().any(|item| {
        item.resource == "model:fcpe" && item.required && item.reason == "pitch.track"
    }));
    assert!(
        !requirements
            .resources
            .iter()
            .any(|item| item.resource == "model:rmvpe")
    );
    let plan = engine.plan(&request).unwrap();
    assert!(
        plan.execution_nodes
            .iter()
            .any(|node| node.capability.as_str() == "pitch.track")
    );
    assert!(plan.resolved_resources.iter().any(|resource| {
        resource.requirement.resource == "model:fcpe" && resource.status.is_some()
    }));

    let result = engine.analyze(&request, &output).unwrap();
    assert_eq!(result.status, AnalysisStatus::Ok);
    assert!(result.degraded_reasons.is_empty());
    assert_eq!(
        result
            .diagnostics
            .audio_quality
            .as_ref()
            .and_then(|quality| quality.vocal_topology.as_ref()),
        None,
        "an already-clean lead source does not execute the workflow's isolation node, so topology is not an applicable gate"
    );
    let pitch_ref = result.artifacts.pitch_evidence.as_ref().unwrap();
    let pitch: PitchEvidenceV03 =
        serde_json::from_slice(&fs::read(output.join(&pitch_ref.path)).unwrap()).unwrap();
    assert_eq!(pitch.model["id"], "fcpe");
    let singing_ref = result.artifacts.singing_analysis.as_ref().unwrap();
    let singing: SingingAnalysisV1 =
        serde_json::from_slice(&fs::read(output.join(&singing_ref.path)).unwrap()).unwrap();
    assert!(singing.candidate_evidence.iter().any(|candidate| {
        candidate.target_pitch_source == "fcpe" && candidate.rmvpe_center_hz.is_none()
    }));
    assert!(!singing.review_regions.iter().any(|region| {
        region
            .reasons
            .contains(&SingingReviewReason::VocalTopologyUnknown)
    }));
    let chart_ref = result.artifacts.candidate_vocal_chart.as_ref().unwrap();
    let chart: CandidateVocalChartV1 =
        serde_json::from_slice(&fs::read(output.join(&chart_ref.path)).unwrap()).unwrap();
    chart.validate().unwrap();
    assert!(
        chart
            .tracks
            .iter()
            .any(|track| track.phrases.iter().any(|phrase| !phrase.notes.is_empty()))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn qwen_fixture_path_executes_transcript_and_alignment_fusion() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-engine-qwen-fixture-{}-{stamp}",
        std::process::id()
    ));
    let store = root.join("store");
    let output = root.join("output");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&output).unwrap();
    let fixture_source = uta_runtime_manager::SourceIdentity::default();
    install_fixture_generation(&store, "qwen3_asr_1_7b", Some(fixture_source.clone()));
    install_fixture_generation(
        &store,
        "qwen3_forced_aligner_0_6b",
        Some(fixture_source.clone()),
    );
    let source = root.join("source.wav");
    fs::write(&source, b"authorized fixture audio").unwrap();
    let ffmpeg = root.join("ffmpeg");
    executable(&ffmpeg, &non_silent_pcm_script(48_000));

    let asr_output = output.join("worker/asr/qwen-asr-transcript-evidence.json");
    let asr_worker = root.join("qwen-asr-worker");
    executable(
        &asr_worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-qwen-asr-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\ncase \"$run\" in *'\"language\"'*) exit 9;; esac\nmkdir -p '{}'\nprintf '%s\\n' '{{\"schema_version\":2,\"model_id\":\"qwen3_asr_1_7b\",\"model_sha256\":\"b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e\",\"backend\":\"vulkan\",\"runtime_manifest_sha256\":\"{}\",\"language_contract\":{{\"version\":1,\"explicit_hint_policy\":\"reject\",\"evidence_source\":\"runtime_detected\"}},\"language\":\"en\",\"text\":\"sing now\"}}' > '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-asr\",\"artifact\":\"transcript_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"task-asr\",\"status\":\"ok\"}}'\nread quit",
            uta_runtime_manager::runtime_recipe_digest("qwen3_asr_1_7b").unwrap(),
            asr_output.parent().unwrap().display(),
            "a".repeat(64),
            asr_output.display(),
            asr_output.display(),
        ),
    );
    let align_output = output.join("worker/alignment/qwen-alignment-evidence.json");
    let align_worker = root.join("qwen-align-worker");
    executable(
        &align_worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-qwen-align-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\nmkdir -p '{}'\nprintf '%s\\n' '{{\"schema_version\":2,\"model_id\":\"qwen3_forced_aligner_0_6b\",\"model_sha256\":\"c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b\",\"backend\":\"vulkan\",\"runtime_manifest_sha256\":\"{}\",\"text_normalization_profile\":\"qwen-align-text-preserve-v1\",\"language_normalization_profile\":\"qwen-align-language-v1\",\"alignment_semantics_profile\":\"qwen-align-token-word-80ms-v1\",\"transcript\":\"sing now\",\"language\":\"en\",\"runtime_language\":\"english\",\"words\":[{{\"word\":\"sing\",\"start\":0.0,\"end\":0.4}},{{\"word\":\"now\",\"start\":0.48,\"end\":0.88}}]}}' > '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-align\",\"artifact\":\"alignment_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"task-align\",\"status\":\"ok\"}}'\nread quit",
            uta_runtime_manager::runtime_recipe_digest("qwen3_forced_aligner_0_6b").unwrap(),
            align_output.parent().unwrap().display(),
            "b".repeat(64),
            align_output.display(),
            align_output.display(),
        ),
    );
    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    catalog.models.get_mut("qwen3_asr_1_7b").unwrap().source = fixture_source.clone();
    catalog
        .models
        .get_mut("qwen3_forced_aligner_0_6b")
        .unwrap()
        .source = fixture_source;
    let manager = RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&store)
            .with_runtime_override("qwen_asr_runtime", &asr_worker)
            .with_runtime_override("qwen_align_runtime", &align_worker)
            .with_tool_override("ffmpeg", &ffmpeg),
    );
    let engine = AnalysisEngine::new(manager);
    for model_id in ["qwen3_asr_1_7b", "qwen3_forced_aligner_0_6b"] {
        let status = engine
            .runtime_manager()
            .status(
                &ResourceRef::model(model_id).unwrap(),
                uta_runtime_manager::RuntimePolicy::Benchmark,
            )
            .unwrap();
        assert!(status.usable, "{model_id}: {status:?}");
    }
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.request_id = "qwen-fixture".to_string();
    request.audio_sources[0].path = source.clone();
    request.audio_sources[0].sha256 = "a".repeat(64);
    request.audio_sources[0].timeline.source_start = 3_000_000;
    request.execution_policy.runtime_policy = uta_runtime_manager::RuntimePolicy::Benchmark;
    request.requested_artifacts.vocal_chart = false;
    request.requested_artifacts.pitch_evidence = false;
    request.requested_artifacts.singing_analysis = false;
    request.requested_artifacts.transcript = true;
    request.requested_artifacts.alignment = true;
    let result = engine.analyze(&request, &output).unwrap();
    assert_eq!(result.status, AnalysisStatus::Ok);
    let transcript_ref = result.artifacts.transcript.unwrap();
    let transcript: TranscriptArtifactV1 =
        serde_json::from_slice(&fs::read(output.join(transcript_ref.path)).unwrap()).unwrap();
    assert_eq!(request.lyrics.language, None);
    assert_eq!(transcript.language.as_deref(), Some("en"));
    assert_eq!(transcript.text, "sing now");
    assert_eq!(transcript.confidence, None);
    assert!(
        transcript
            .tokens
            .iter()
            .all(|token| token.confidence.is_none())
    );
    let alignment_ref = result.artifacts.alignment.unwrap();
    let alignment: AlignmentArtifactV1 =
        serde_json::from_slice(&fs::read(output.join(alignment_ref.path)).unwrap()).unwrap();
    assert_eq!(alignment.transcript, "sing now");
    assert_eq!(alignment.language.as_deref(), Some("en"));
    assert!(alignment.items.iter().all(|item| item.confidence.is_none()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn denoise_route_uses_supervised_worker_and_atomically_publishes_flac() {
    let Some(ffmpeg) = std::env::var_os("UTA_STUDIO_FFMPEG_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    else {
        return;
    };
    let root = std::env::temp_dir().join(format!(
        "uta-engine-denoise-route-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let output_root = root.join("output");
    let model_path = root.join("model");
    fs::create_dir_all(&output_root).unwrap();
    fs::create_dir_all(&model_path).unwrap();
    let wav = root.join("input.wav");
    let flac = root.join("input.flac");
    let frames = 4_410_u32;
    let channels = 2_u16;
    let data_bytes = frames * u32::from(channels) * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&44_100_u32.to_le_bytes());
    bytes.extend_from_slice(&(44_100_u32 * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    bytes.resize(44 + data_bytes as usize, 0);
    fs::write(&wav, bytes).unwrap();
    assert!(
        std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&wav)
            .args(["-c:a", "flac", "-y"])
            .arg(&flac)
            .status()
            .unwrap()
            .success()
    );
    let source_duration = decode_audio(&ffmpeg, "fixture", &flac)
        .unwrap()
        .facts
        .duration;
    let harmony_worker = root.join("harmony-worker");
    let harmony_lead = output_root.join("worker/lead-isolate/lead-vocal.flac");
    let harmony_residual = output_root.join("worker/lead-isolate/vocal-residual.flac");
    executable(
        &harmony_worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\"}}'\nread run\ncp -- '{}' '{}'\ncp -- '{}' '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"harmony-route\",\"artifact\":\"lead_vocal\",\"path\":\"{}\",\"media_type\":\"audio/flac\"}}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"harmony-route\",\"artifact\":\"vocal_residual\",\"path\":\"{}\",\"media_type\":\"audio/flac\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"harmony-route\",\"status\":\"ok\"}}'\nread quit",
            flac.display(),
            harmony_lead.display(),
            flac.display(),
            harmony_residual.display(),
            harmony_lead.display(),
            harmony_residual.display(),
        ),
    );
    let harmony = run_openvino_harmony(
        &DenoiseTask {
            model_path: &model_path,
            executable: &harmony_worker,
            runtime_recipe_digest: None,
            backend: "openvino_gpu",
            ffmpeg: &ffmpeg,
            input: &flac,
            output_root: &output_root,
            source_duration,
            task_id: "harmony-route",
        },
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(harmony.stem.role, AudioRole::LeadVocal);
    assert_eq!(
        harmony.stem.artifact.path,
        PathBuf::from("stems/lead_vocal.flac")
    );
    assert!(output_root.join(harmony.stem.artifact.path).is_file());
    assert!(!harmony.lead_profile.windows.is_empty());
    assert!(!harmony.residual_profile.windows.is_empty());
    assert!(!output_root.join("worker/lead-isolate").exists());
    assert!(!output_root.join("stems/vocal_residual.flac").exists());

    let worker = root.join("openvino-worker");
    let worker_output = output_root.join("worker/denoise/clean-lead-vocal.flac");
    executable(
        &worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\"}}'\nread run\ncp -- '{}' '{}'\nprintf '%s\\n' '{{\"type\":\"progress\",\"task_id\":\"denoise-route\",\"fraction\":0.5,\"message\":\"dry\"}}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"denoise-route\",\"artifact\":\"clean_lead_vocal\",\"path\":\"{}\",\"media_type\":\"audio/flac\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"denoise-route\",\"status\":\"ok\"}}'\nread quit",
            flac.display(),
            worker_output.display(),
            worker_output.display(),
        ),
    );
    let output = run_openvino_denoise(
        &DenoiseTask {
            model_path: &model_path,
            executable: &worker,
            runtime_recipe_digest: None,
            backend: "openvino_gpu",
            ffmpeg: &ffmpeg,
            input: &flac,
            output_root: &output_root,
            source_duration,
            task_id: "denoise-route",
        },
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(output.role, AudioRole::CleanLeadVocal);
    assert_eq!(output.artifact.media_type, "audio/flac");
    assert!(output_root.join(output.artifact.path).is_file());
    assert!(!output_root.join("worker/denoise").exists());

    let dereverb_worker = root.join("dereverb-worker");
    let dereverb_output = output_root.join("worker/dereverb/noreverb-vocal.flac");
    executable(
        &dereverb_worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\"}}'\nread run\ncp -- '{}' '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"dereverb-route\",\"artifact\":\"dereverbed_vocal\",\"path\":\"{}\",\"media_type\":\"audio/flac\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"dereverb-route\",\"status\":\"ok\"}}'\nread quit",
            flac.display(),
            dereverb_output.display(),
            dereverb_output.display(),
        ),
    );
    let dereverbed = run_openvino_dereverb(
        &DenoiseTask {
            model_path: &model_path,
            executable: &dereverb_worker,
            runtime_recipe_digest: None,
            backend: "openvino_gpu",
            ffmpeg: &ffmpeg,
            input: &flac,
            output_root: &output_root,
            source_duration,
            task_id: "dereverb-route",
        },
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(dereverbed.role, AudioRole::CleanLeadVocal);
    assert!(output_root.join(dereverbed.artifact.path).is_file());
    assert!(!output_root.join("worker/dereverb").exists());

    let instrumental_worker = root.join("instrumental-worker");
    let instrumental_output = output_root.join("worker/instrumental/instrumental.flac");
    executable(
        &instrumental_worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\"}}'\nread run\ncp -- '{}' '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"instrumental-route\",\"artifact\":\"instrumental\",\"path\":\"{}\",\"media_type\":\"audio/flac\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"instrumental-route\",\"status\":\"ok\"}}'\nread quit",
            flac.display(),
            instrumental_output.display(),
            instrumental_output.display(),
        ),
    );
    let instrumental = run_openvino_instrumental(
        &DenoiseTask {
            model_path: &model_path,
            executable: &instrumental_worker,
            runtime_recipe_digest: None,
            backend: "openvino_gpu",
            ffmpeg: &ffmpeg,
            input: &flac,
            output_root: &output_root,
            source_duration,
            task_id: "instrumental-route",
        },
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(instrumental.role, AudioRole::Instrumental);
    assert!(output_root.join(instrumental.artifact.path).is_file());
    assert!(!output_root.join("worker/instrumental").exists());
    fs::remove_dir_all(root).unwrap();
}
