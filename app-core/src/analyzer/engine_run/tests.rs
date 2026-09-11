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

fn artifact(path: &str, bytes: &[u8]) -> ArtifactRefWire {
    ArtifactRefWire {
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
fn candidate_semantics_are_validated_before_artifact_capture() {
    let root = temp_root("candidate-semantics");
    let valid = serde_json::json!({
        "format":"uta.vocal-chart","format_version":"0.3.0","timebase":1000000,
        "language":"ja","tracks":[{
            "id":"lead","role":"lead","phrases":[{
                "id":"phrase-1","notes":[{
                    "id":"note-1","start":0,"duration":500000,
                    "pitch":{"midi":69,"cents":0},"vocal_mode":"pitched",
                    "bonus":"normal","scoring":{"mode":"pitch","weight":1.0},
                    "lyrics":[{"id":"lyric-1","text":"歌","join_before":"none"}]
                }]
            }]
        }]
    });
    let valid_path = root.join("valid.json");
    std::fs::write(&valid_path, serde_json::to_vec(&valid).unwrap()).unwrap();
    validate_semantic_artifact("candidate_vocal_chart", &valid_path).unwrap();

    let invalid_path = root.join("invalid.json");
    std::fs::write(
        &invalid_path,
        br#"{"contract":"uta.analysis-engine.candidate-vocal-chart","version":1}"#,
    )
    .unwrap();
    assert!(
        validate_semantic_artifact("candidate_vocal_chart", &invalid_path)
            .unwrap_err()
            .contains("projection is invalid")
    );
    let malformed_path = root.join("malformed.json");
    std::fs::write(&malformed_path, b"{").unwrap();
    assert!(
        validate_semantic_artifact("candidate_vocal_chart", &malformed_path)
            .unwrap_err()
            .contains("not valid JSON")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn quantization_wire_result_matches_exact_request_intent() {
    let request: AnalyzeRequestWire = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,"request_id":"quantized",
            "audio_sources":[{"id":"main","kind":"local_file","path":"song.flac","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","role":"lead_vocal","primary":true,"timeline":{"timebase":1000000,"source_start":0}}],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "musical_context":{"bpm":120.0,"time_signature":{"beats":4,"unit":4},"quantization_grid":"sixteenth","authority":"hint"},
            "analysis":{"profile":"fast","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":true},
            "requested_artifacts":{"vocal_chart":true},"execution_policy":{},"extensions":{}
        })).unwrap();
    let mut manifest: AnalysisResultManifestWire = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.result","version":1,"request_id":"quantized","status":"ok",
            "artifacts":{"candidate_vocal_chart":{"path":"candidate/vocal-chart.json","media_type":"application/vnd.uta.vocal-chart+json;version=0.3","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","bytes":1}},
            "diagnostics":{"quantization":{"algorithm":"rhythm-grid-dp","bpm":120.0,"grid":"sixteenth","grid_step":125000,"minimum_note_duration":125000,"source_start":0,"source_end":1000000,"hard_boundary_count":0,"note_count":2,"adjusted_notes":2,"maximum_shift":12000}},
            "provenance":{"resources":[],"calibration_version":"c","fusion_version":"f","hsmm_version":"h","quantization_version":"rhythm-grid-dp","postprocess_version":"p"},
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
    let request: AnalyzeRequestWire = serde_json::from_value(serde_json::json!({
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
    let plan: AnalysisPlanWire = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"quality",
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],"required_capabilities":[],"optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":gates.clone(),
            "fallback_policy":[],"artifact_declarations":[]
        })).unwrap();
    let mut manifest: AnalysisResultManifestWire = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.result","version":1,"request_id":"quality","status":"ok",
            "artifacts":{},
            "diagnostics":{"audio_quality":{"contract":"uta.analysis-engine.audio-quality-report","version":1,"algorithm":"audio-quality-gates","profile":"fast","evaluated_audio_role":"lead_vocal","duration":1000000,"planned_gates":gates,"outcomes":outcomes}},
            "provenance":{"resources":[],"calibration_version":"c","fusion_version":"f","hsmm_version":"h","quantization_version":"q","audio_quality_version":"audio-quality-gates","postprocess_version":"p"},
            "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","degraded_reasons":[]
        })).unwrap();
    validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).unwrap();

    let mut unsupported_algorithm = manifest.clone();
    unsupported_algorithm
        .diagnostics
        .audio_quality
        .as_mut()
        .unwrap()
        .algorithm = "audio-quality-gates-unsupported".to_string();
    unsupported_algorithm.provenance.audio_quality_version =
        "audio-quality-gates-unsupported".to_string();
    assert!(
        validate_audio_quality_result(&request, &plan, &unsupported_algorithm, 1_000_000,).is_err()
    );

    let mut invalid_timebase = request.clone();
    invalid_timebase.audio_sources[0].timeline.timebase = 1_000;
    assert!(validate_audio_quality_result(&invalid_timebase, &plan, &manifest, 1_000_000).is_err());
    let mut duplicate_primary = request.clone();
    let mut duplicate = duplicate_primary.audio_sources[0].clone();
    duplicate.id = "duplicate".to_string();
    duplicate_primary.audio_sources.push(duplicate);
    assert!(
        validate_audio_quality_result(&duplicate_primary, &plan, &manifest, 1_000_000).is_err()
    );
    let mut mismatched_plan = plan.clone();
    mismatched_plan.source_route.primary_source_id = "other".to_string();
    assert!(
        validate_audio_quality_result(&request, &mismatched_plan, &manifest, 1_000_000).is_err()
    );
    let mut mismatched_role = manifest.clone();
    mismatched_role
        .diagnostics
        .audio_quality
        .as_mut()
        .unwrap()
        .evaluated_audio_role = "original_mix".to_string();
    assert!(validate_audio_quality_result(&request, &plan, &mismatched_role, 1_000_000).is_err());

    manifest
        .diagnostics
        .audio_quality
        .as_mut()
        .unwrap()
        .planned_gates
        .pop();
    assert!(validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).is_err());
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
    clipping.status = QualityGateStatusWire::Unknown;
    assert!(validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).is_err());
    manifest.status = AnalysisStatusWire::OkDegraded;
    validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).unwrap();
    let report = manifest.diagnostics.audio_quality.as_mut().unwrap();
    report.duration = 1_100_000;
    report.outcomes[2].regions.push(QualityRegionWire {
        start: 1_000_000,
        end: 1_050_000,
        reason: "outside_app_owned_source".to_string(),
    });
    assert!(validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).is_err());
    let report = manifest.diagnostics.audio_quality.as_mut().unwrap();
    report.outcomes[2].regions.clear();
    report.duration = 1_100_001;
    assert!(validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).is_err());
    let report = manifest.diagnostics.audio_quality.as_mut().unwrap();
    report.duration = 1_000_000;
    report.outcomes[2].regions = vec![
        QualityRegionWire {
            start: 100,
            end: 300,
            reason: "first".to_string(),
        },
        QualityRegionWire {
            start: 200,
            end: 400,
            reason: "overlap".to_string(),
        },
    ];
    assert!(validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).is_err());
}

#[test]
fn vocal_topology_region_may_span_the_engines_measured_duration_past_the_apps_expected_window() {
    // Regression (real repro): the Engine's own decoded duration can
    // exceed the app's independently-computed `expected_source_duration`
    // by a sub-millisecond decode-vs-metadata rounding amount --
    // report.duration=305813333 vs expected_source_duration=305813000 for
    // a real song. The `vocal_topology_unknown` fallback region
    // legitimately spans that full measured duration, so it must not be
    // rejected just for extending past the app-owned `source_end`; only
    // extending past the Engine's own `report_end` is actually invalid.
    // Every other gate keeps the strict `source_end` bound (see the
    // `outside_app_owned_source` case above).
    let request: AnalyzeRequestWire = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,"request_id":"topology",
            "audio_sources":[{"id":"main","kind":"local_file","path":"song.flac","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","role":"lead_vocal","primary":true,"timeline":{"timebase":1000000,"source_start":0}}],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "analysis":{"profile":"fast","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
            "requested_artifacts":{"pitch_evidence":true},"execution_policy":{},"extensions":{}
        }))
        .unwrap();
    let gates = vec!["timeline_valid", "vocal_topology"];
    let plan: AnalysisPlanWire = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"topology",
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],"required_capabilities":[],"optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":gates,
            "fallback_policy":[],"artifact_declarations":[]
        }))
        .unwrap();
    let manifest: AnalysisResultManifestWire = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.result","version":1,"request_id":"topology","status":"ok_degraded",
            "artifacts":{},
            "diagnostics":{"audio_quality":{
                "contract":"uta.analysis-engine.audio-quality-report","version":1,"algorithm":"audio-quality-gates","profile":"fast","evaluated_audio_role":"lead_vocal",
                "duration":1_000_333,
                "planned_gates":gates,
                "outcomes":[
                    {"gate":"timeline_valid","requirement":"required","status":"passed","summary":"measured","metrics":[],"regions":[]},
                    {"gate":"vocal_topology","requirement":"degrading","status":"unknown","summary":"vocal topology is unknown","metrics":[],"regions":[
                        {"start":0,"end":1_000_333,"reason":"vocal_topology_unknown"}
                    ]}
                ],
                "vocal_topology":{
                    "contract":"uta.analysis-engine.vocal-topology-estimate","version":1,"timebase":1000000,
                    "source_start":0,"duration":1_000_333,"mode":"unknown","confidence":null,
                    "overlap_regions":[],"support_regions":[],
                    "evidence_sources":["caller_or_unpartitioned_vocal_input"]
                }
            }},
            "provenance":{"resources":[],"calibration_version":"c","fusion_version":"f","hsmm_version":"h","quantization_version":"q","audio_quality_version":"audio-quality-gates","postprocess_version":"p"},
            "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","degraded_reasons":["vocal_topology_ambiguous"]
        }))
        .unwrap();

    validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).unwrap();

    // The same overshoot on a non-topology gate stays invalid.
    let mut mismatched = manifest.clone();
    let report = mismatched.diagnostics.audio_quality.as_mut().unwrap();
    report.outcomes[0].regions.push(QualityRegionWire {
        start: 0,
        end: 1_000_333,
        reason: "timeline_valid_past_app_window".to_string(),
    });
    assert!(validate_audio_quality_result(&request, &plan, &mismatched, 1_000_000).is_err());
}

#[test]
fn clean_evaluated_role_requires_a_bound_workflow_cleanup_route() {
    let mut plan: AnalysisPlanWire = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"role-route",
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],"required_capabilities":[],
            "optional_capabilities":["audio.denoise"],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":[],
            "fallback_policy":[],"artifact_declarations":[],
            "workflow_execution":{
                "identity":{"contract":"uta.workflow-execution-plan","version":1,
                    "workflow_schema_version":1,"workflow_id":"role-route","workflow_revision":1,
                    "definition_digest":"fixture"},
                "nodes":[
                    {"instance_id":"source","analysis_node":"workflow.source",
                        "capabilities":["audio.source"],"execution_policy":"always",
                        "execution_state":"ready","priority":100,"input_bindings":[]},
                    {"instance_id":"cleanup","analysis_node":"workflow.cleanup",
                        "capabilities":["audio.denoise"],"execution_policy":"always",
                        "execution_state":"ready","priority":90,"input_bindings":[{
                            "from_node":"workflow.source","from_port":"lead","to_node":"workflow.cleanup",
                            "to_port":"audio","semantic_type":"audio","audio_role":"lead_vocal",
                            "execution_active":true,"analyzer_attachment":false}]},
                    {"instance_id":"pitch","analysis_node":"workflow.pitch",
                        "capabilities":["pitch.track"],"execution_policy":"always",
                        "execution_state":"ready","priority":80,"input_bindings":[{
                            "from_node":"workflow.source","from_port":"lead","to_node":"workflow.pitch",
                            "to_port":"audio","semantic_type":"audio","audio_role":"lead_vocal",
                            "execution_active":true,"analyzer_attachment":true}]}
                ],
                "terminal_outputs":[],
                "fusion_mode":"algorithm"
            }
        }))
        .unwrap();
    assert!(evaluated_audio_role_matches_plan(&plan, "lead_vocal"));
    assert!(!evaluated_audio_role_matches_plan(
        &plan,
        "clean_lead_vocal"
    ));

    {
        let workflow = plan.workflow_execution.as_mut().unwrap();
        let pitch = workflow
            .nodes
            .iter_mut()
            .find(|node| node.analysis_node == "workflow.pitch")
            .unwrap();
        pitch.input_bindings[0].from_node = "workflow.cleanup".to_string();
        pitch.input_bindings[0].from_port = "audio".to_string();
    }
    assert!(evaluated_audio_role_matches_plan(&plan, "clean_lead_vocal"));

    {
        let workflow = plan.workflow_execution.as_mut().unwrap();
        let pitch = workflow
            .nodes
            .iter_mut()
            .find(|node| node.analysis_node == "workflow.pitch")
            .unwrap();
        pitch.input_bindings[0].from_node = "workflow.intermediate".to_string();
        let intermediate: crate::backend_cli::WorkflowExecutionNodePlanWire =
            serde_json::from_value(serde_json::json!({
                "instance_id":"intermediate","analysis_node":"workflow.intermediate",
                "capabilities":["audio.refine"],"execution_policy":"disabled",
                "execution_state":"profile_skipped","priority":85,"input_bindings":[{
                    "from_node":"workflow.cleanup","from_port":"audio",
                    "to_node":"workflow.intermediate","to_port":"audio",
                    "semantic_type":"audio","audio_role":"lead_vocal",
                    "execution_active":true,"analyzer_attachment":false}]
            }))
            .unwrap();
        workflow.nodes.push(intermediate);
    }
    assert!(!evaluated_audio_role_matches_plan(
        &plan,
        "clean_lead_vocal"
    ));

    {
        let workflow = plan.workflow_execution.as_mut().unwrap();
        workflow
            .nodes
            .iter_mut()
            .find(|node| node.analysis_node == "workflow.pitch")
            .unwrap()
            .execution_state = crate::backend_cli::WorkflowNodeExecutionStateWire::NotRequested;
        workflow
            .nodes
            .iter_mut()
            .find(|node| node.analysis_node == "workflow.cleanup")
            .unwrap()
            .execution_state = crate::backend_cli::WorkflowNodeExecutionStateWire::Disabled;
        workflow
            .terminal_outputs
            .push(crate::workflow::WorkflowTerminalOutputWire {
                node: "workflow.cleanup".to_string(),
                port: "audio".to_string(),
                semantic_type: "audio".to_string(),
                audio_role: None,
            });
    }
    assert!(!evaluated_audio_role_matches_plan(
        &plan,
        "clean_lead_vocal"
    ));
}

#[test]
fn optional_technique_artifact_is_committed_when_present_and_omitted_when_absent() {
    let root = temp_root("technique-commit");
    let _db_guard = crate::library_db::reconnect_for_test(&root.join("db"));
    let cache = CacheDir {
        path: root.join("cache"),
    };
    std::fs::create_dir_all(&cache.path).unwrap();
    let gates = vec!["timeline_valid"];
    for present in [false, true] {
        let request_id = if present {
            "technique-present"
        } else {
            "technique-absent"
        };
        let file_hash = format!("file-{request_id}");
        let output = cache.path.join(request_id);
        std::fs::create_dir_all(&output).unwrap();
        let request: AnalyzeRequestWire =
                serde_json::from_value(serde_json::json!({
                    "contract":"uta.analysis-engine.request","version":1,
                    "request_id":request_id,
                    "audio_sources":[{
                        "id":"main","kind":"local_file","path":"song.flac",
                        "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "role":"lead_vocal","primary":true,
                        "timeline":{"timebase":1000000,"source_start":0}
                    }],
                    "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
                    "analysis":{"profile":"maximum","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
                    "requested_artifacts":{"pitch_evidence":true},
                    "execution_policy":{},"extensions":{}
                }))
                .unwrap();
        let plan: AnalysisPlanWire = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,
            "request_id":request_id,
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],
            "required_capabilities":[],"optional_capabilities":["technique.analyze"],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":gates,
            "fallback_policy":[],
            "artifact_declarations":[{
                "semantic_type":"technique_evidence","required":false,
                "media_type":"application/vnd.uta.technique-evidence+json;version=1"
            }]
        }))
        .unwrap();
        let technique = serde_json::to_vec(&serde_json::json!({
            "contract":"uta.analysis-engine.technique-evidence","version":1,
            "model_id":"stars","taxonomy":["breathy"],
            "calibration":"uncalibrated_source_local",
            "intervals":[],"style_scope":"global","styles":[],
            "provenance":{"expert_id":"stars","task":"technique"}
        }))
        .unwrap();
        if present {
            std::fs::write(output.join("technique.json"), &technique).unwrap();
        }
        let technique_ref = present.then(|| ArtifactRefWire {
            path: PathBuf::from("technique.json"),
            media_type: "application/vnd.uta.technique-evidence+json;version=1".to_string(),
            sha256: "b".repeat(64),
            bytes: technique.len() as u64,
        });
        let manifest: AnalysisResultManifestWire = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.result","version":1,
            "request_id":request_id,"status":"ok",
            "artifacts":{"technique_evidence":technique_ref},
            "diagnostics":{"audio_quality":{
                "contract":"uta.analysis-engine.audio-quality-report","version":1,
                "algorithm":"audio-quality-gates","profile":"maximum",
                "evaluated_audio_role":"lead_vocal","duration":1000000,
                "planned_gates":gates,
                "outcomes":[{
                    "gate":"timeline_valid","requirement":"required",
                    "status":"passed","summary":"measured","metrics":[],"regions":[]
                }]
            }},
            "provenance":{
                "resources":[],"calibration_version":"c","fusion_version":"f",
                "hsmm_version":"h","quantization_version":"q",
                "audio_quality_version":"audio-quality-gates","postprocess_version":"p"
            },
            "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "degraded_reasons":[]
        }))
        .unwrap();

        if present {
            let mut wrong_media = manifest.clone();
            wrong_media
                .artifacts
                .technique_evidence
                .as_mut()
                .unwrap()
                .media_type = "application/json".to_string();
            assert!(
                validate_and_publish_engine_result(
                    &file_hash,
                    &cache,
                    &output,
                    1_000_000,
                    &request,
                    &plan,
                    &wrong_media,
                )
                .unwrap_err()
                .contains("media type mismatch")
            );
        }
        validate_and_publish_engine_result(
            &file_hash, &cache, &output, 1_000_000, &request, &plan, &manifest,
        )
        .unwrap();
        let revisions = crate::analysis_artifact::load_artifact_revisions(
            &file_hash,
            ArtifactKind::TechniqueEvidence,
        );
        assert_eq!(revisions.len(), usize::from(present));
        assert!(revisions.iter().all(|revision| {
            revision.active && revision.producer_node.as_str() == "stars-technique"
        }));
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn published_instrumental_stem_reaches_its_legacy_compatibility_path() {
    // Real repro: `Song::refresh_authoring_state` and the editor-open
    // gate read the flat `{hash}_instrumental.*` compatibility file, not
    // the content-addressed artifact store. A published stem that never
    // reaches that path leaves a fully-completed song stuck showing
    // "Analysis incomplete" with an editor that won't open.
    let root = temp_root("stem-compat");
    let _db_guard = crate::library_db::reconnect_for_test(&root.join("db"));
    let cache = CacheDir {
        path: root.join("cache"),
    };
    std::fs::create_dir_all(&cache.path).unwrap();
    let gates = vec!["timeline_valid"];
    let request_id = "stem-compat";
    let file_hash = "file-stem-compat";
    let output = cache.path.join(request_id);
    std::fs::create_dir_all(&output).unwrap();
    let request: AnalyzeRequestWire = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,
            "request_id":request_id,
            "audio_sources":[{
                "id":"main","kind":"local_file","path":"song.flac",
                "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "role":"lead_vocal","primary":true,
                "timeline":{"timebase":1000000,"source_start":0}
            }],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "analysis":{"profile":"maximum","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
            "requested_artifacts":{"pitch_evidence":true},
            "execution_policy":{},"extensions":{}
        }))
        .unwrap();
    let plan: AnalysisPlanWire = serde_json::from_value(serde_json::json!({
        "schema":"uta.analysis-engine.plan","schema_version":1,
        "request_id":request_id,
        "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
        "requested_outputs":["pitch_evidence"],
        "required_capabilities":[],"optional_capabilities":[],
        "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
        "resolved_resources":[],"execution_nodes":[],"quality_gates":gates,
        "fallback_policy":[],
        "artifact_declarations":[{
            "semantic_type":"stem:instrumental","required":true,
            "media_type":"audio/flac"
        }]
    }))
    .unwrap();
    let instrumental_bytes = b"fake-flac-bytes".to_vec();
    std::fs::write(output.join("instrumental.flac"), &instrumental_bytes).unwrap();
    let manifest: AnalysisResultManifestWire = serde_json::from_value(serde_json::json!({
        "contract":"uta.analysis-engine.result","version":1,
        "request_id":request_id,"status":"ok",
        "artifacts":{"stems":[{
            "role":"instrumental",
            "artifact":{
                "path":"instrumental.flac",
                "media_type":"audio/flac",
                "sha256":"b".repeat(64),
                "bytes":instrumental_bytes.len() as u64
            }
        }]},
        "diagnostics":{"audio_quality":{
            "contract":"uta.analysis-engine.audio-quality-report","version":1,
            "algorithm":"audio-quality-gates","profile":"maximum",
            "evaluated_audio_role":"lead_vocal","duration":1000000,
            "planned_gates":gates,
            "outcomes":[{
                "gate":"timeline_valid","requirement":"required",
                "status":"passed","summary":"measured","metrics":[],"regions":[]
            }]
        }},
        "provenance":{
            "resources":[],"calibration_version":"c","fusion_version":"f",
            "hsmm_version":"h","quantization_version":"q",
            "audio_quality_version":"audio-quality-gates","postprocess_version":"p"
        },
        "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "degraded_reasons":[]
    }))
    .unwrap();

    validate_and_publish_engine_result(
        file_hash, &cache, &output, 1_000_000, &request, &plan, &manifest,
    )
    .unwrap();

    assert!(
        cache.instrumental_path(file_hash).is_file(),
        "instrumental stem should reach its legacy compatibility path"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn typed_vocal_topology_wire_is_plan_bound_and_fails_closed_on_shape_conflict() {
    let request: AnalyzeRequestWire = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,"request_id":"topology",
            "audio_sources":[{"id":"main","kind":"local_file","path":"song.flac","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","role":"lead_vocal","primary":true,"timeline":{"timebase":1000000,"source_start":2000000}}],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "analysis":{"profile":"balanced","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
            "requested_artifacts":{"pitch_evidence":true},"execution_policy":{},"extensions":{}
        }))
        .unwrap();
    let plan: AnalysisPlanWire = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"topology",
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],"required_capabilities":[],"optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":["vocal_topology"],
            "fallback_policy":[],"artifact_declarations":[]
        }))
        .unwrap();
    let mut report: AudioQualityReportWire = serde_json::from_value(serde_json::json!({
        "contract":"uta.analysis-engine.audio-quality-report","version":1,
        "algorithm":"audio-quality-gates","profile":"balanced",
        "evaluated_audio_role":"lead_vocal","duration":1000000,
        "planned_gates":["vocal_topology"],"outcomes":[],
        "vocal_topology":{
            "contract":"uta.analysis-engine.vocal-topology-estimate","version":1,
            "timebase":1000000,"source_start":2000000,"duration":1000000,
            "mode":"unknown","overlap_regions":[],"support_regions":[],
            "evidence_sources":["caller_or_unpartitioned_vocal_input"]
        }
    }))
    .unwrap();
    validate_vocal_topology_result(&request, &plan, &report, 1_000_000).unwrap();

    report.vocal_topology.as_mut().unwrap().mode = VocalTopologyModeWire::OverlappingMultiLead;
    assert!(validate_vocal_topology_result(&request, &plan, &report, 1_000_000).is_err());
    report.vocal_topology = None;
    assert!(
        validate_vocal_topology_result(&request, &plan, &report, 1_000_000)
            .unwrap_err()
            .contains("omitted")
    );
}

#[test]
fn artifact_event_does_not_erase_last_measured_node_progress() {
    let file_hash = "engine-lifecycle-progress-fixture";
    let snapshot: AnalysisProgressSnapshot = serde_json::from_value(serde_json::json!({
        "stage":"Preparing","overall_progress":0,"stage_progress":0,
        "operation":"Preparing","detail":"","implementation":"uta-analysis-engine",
        "model":"Engine native","device":"Engine-resolved",
        "requested_device":"Production policy","fallback_from":null,
        "fallback_reason":null,"backend_fallback_from":null,
        "backend_fallback_reason":null,"stage_routes":[]
    }))
    .unwrap();
    LIVE_ANALYSIS
        .lock()
        .unwrap()
        .insert(file_hash.to_string(), snapshot);
    let event = |frame_type: &str,
                 progress: Option<f32>,
                 work_units: Option<(u64, u64)>,
                 artifact: Option<&str>| {
        AnalysisLifecycleFrameWire {
            frame_type: frame_type.to_string(),
            schema_version: 1,
            request_id: "request".to_string(),
            node_id: "pitch.track".to_string(),
            presentation_node_id: Some("workflow.f0_rmvpe".to_string()),
            capability_id: "pitch.track".to_string(),
            model_id: Some("rmvpe".to_string()),
            implementation: "openvino".to_string(),
            progress,
            work_units_completed: work_units.map(|(completed, _)| completed),
            work_units_total: work_units.map(|(_, total)| total),
            worker_task_id: work_units.map(|_| "rmvpe-task-7".to_string()),
            artifact: artifact.map(str::to_string),
            path: None,
            message: None,
            event_at_ms: 1,
        }
    };
    apply_engine_lifecycle_event(
        file_hash,
        None,
        event("node_started", None, None, None),
        &Default::default(),
    );
    apply_engine_lifecycle_event(
        file_hash,
        None,
        event("node_progress", Some(0.42), Some((21, 50)), None),
        &Default::default(),
    );
    apply_engine_lifecycle_event(
        file_hash,
        None,
        event("artifact", None, None, Some("pitch_evidence")),
        &Default::default(),
    );
    let snapshot = LIVE_ANALYSIS.lock().unwrap().remove(file_hash).unwrap();
    assert_eq!(snapshot.stage_progress, 42);
    assert_eq!(snapshot.node_event.as_deref(), Some("artifact"));
    assert_eq!(snapshot.stage_routes.len(), 1);
    assert_eq!(snapshot.stage_routes[0].stage_progress, 42);
    assert_eq!(snapshot.stage_routes[0].work_units_completed, Some(21));
    assert_eq!(snapshot.stage_routes[0].work_units_total, Some(50));
    assert_eq!(
        snapshot.stage_routes[0].worker_task_id.as_deref(),
        Some("rmvpe-task-7")
    );
    assert_eq!(
        snapshot.stage_routes[0].node_event.as_deref(),
        Some("node_progress")
    );

    LIVE_ANALYSIS
        .lock()
        .unwrap()
        .insert(file_hash.to_string(), snapshot);
    apply_engine_lifecycle_event(
        file_hash,
        None,
        event("node_progress", Some(0.9), None, None),
        &Default::default(),
    );
    let unitless = LIVE_ANALYSIS.lock().unwrap().remove(file_hash).unwrap();
    assert_eq!(unitless.stage_progress, 0);
    assert_eq!(unitless.stage_routes[0].stage_progress, 0);
    assert_eq!(unitless.stage_routes[0].work_units_completed, None);
    assert_eq!(unitless.stage_routes[0].work_units_total, None);
    assert_eq!(unitless.stage_routes[0].worker_task_id, None);
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
