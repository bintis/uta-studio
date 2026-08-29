use std::path::{Path, PathBuf};

use super::*;

fn analysis_request(request_id: &str) -> serde_json::Value {
    serde_json::json!({
        "contract":"uta.analysis-engine.request", "version":1, "request_id":request_id,
        "audio_sources":[{
            "id":"main", "kind":"local_file", "path":"/tmp/uta-studio-contract.wav",
            "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "role":"original_mix", "primary":true,
            "timeline":{"timebase":1000000,"source_start":0}
        }],
        "lyrics":{"mode":"none","tokens":[]}, "boundary_constraints":[],
        "analysis":{"profile":"fast","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
        "requested_artifacts":{"vocal_chart":false,"pitch_evidence":true,"singing_analysis":false,"transcript":false,"alignment":false,"stems":[]},
        "execution_policy":{"runtime_policy":"production"}, "extensions":{}
    })
}

#[test]
fn omitted_analysis_policy_defaults_to_production() {
    let mut request = analysis_request("default-policy");
    request.as_object_mut().unwrap().remove("execution_policy");
    let request: AnalyzeRequestWireV1 = serde_json::from_value(request).unwrap();
    assert_eq!(
        request.execution_policy.runtime_policy,
        RuntimePolicyWireV1::Production
    );
}

#[test]
fn real_analysis_cli_ready_validate_requirements_plan_and_error_contract() {
    let mut client = AnalysisCliClient::connect().expect("uta-analyze debug CLI must be built");
    assert_eq!(client.ready().protocol_identity, ANALYSIS_WORKER_IDENTITY);
    let request = analysis_request("studio-contract-1");
    client.validate(&request, "studio-contract-1").unwrap();
    let requirements = client.requirements(&request, "studio-contract-1").unwrap();
    assert_eq!(requirements.schema, "uta.runtime.requirements");
    let plan = client.plan(&request, "studio-contract-1").unwrap();
    assert_eq!(plan.request_id, "studio-contract-1");
    assert!(
        plan.required_capabilities
            .iter()
            .any(|capability| capability.as_str() == "pitch.track")
    );

    let mut invalid = request;
    invalid["version"] = serde_json::json!(999);
    let error = client.validate(&invalid, "studio-contract-1").unwrap_err();
    assert!(
        matches!(error, BackendCliError::Domain { code, .. } if code == "unsupported_contract_version")
    );
}

#[test]
fn real_analysis_cli_projects_quantization_between_candidate_and_finalization() {
    let mut request = analysis_request("quantization-contract-1");
    request["audio_sources"][0]["role"] = serde_json::json!("lead_vocal");
    request["analysis"]["enable_quantization"] = serde_json::json!(true);
    request["requested_artifacts"]["vocal_chart"] = serde_json::json!(true);
    request["musical_context"] = serde_json::json!({
        "bpm":120.0,"time_signature":{"beats":4,"unit":4},
        "quantization_grid":"sixteenth","authority":"hint"
    });
    let workflow =
        crate::workflow::compile_workflow(&crate::workflow::default_workflow("quantized-song"))
            .unwrap();
    request["extensions"][crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY] =
        crate::workflow::workflow_execution_extension(&workflow).unwrap();
    let mut client = AnalysisCliClient::connect().expect("uta-analyze debug CLI must be built");
    client
        .validate(&request, "quantization-contract-1")
        .unwrap();
    let plan = client.plan(&request, "quantization-contract-1").unwrap();
    let index = |capability: &str| {
        plan.execution_nodes
            .iter()
            .position(|node| node.capability.as_str() == capability)
            .unwrap()
    };
    assert!(index("fusion.candidate_graph") < index("rhythm.quantize"));
    assert!(index("rhythm.quantize") < index("finalize.vocal_chart"));
}

#[test]
fn real_analysis_cli_validates_and_projects_exact_compiled_workflow() {
    let snapshot =
        crate::workflow::compile_workflow(&crate::workflow::default_workflow("contract-song"))
            .unwrap();
    let extension = crate::workflow::workflow_execution_extension(&snapshot).unwrap();
    let mut request = analysis_request("workflow-contract-1");
    request["analysis"]["profile"] = serde_json::json!("balanced");
    request["extensions"][crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY] = extension;

    let mut client = AnalysisCliClient::connect().expect("uta-analyze debug CLI must be built");
    client
        .validate(&request, "workflow-contract-1")
        .expect("backend must independently validate the Studio workflow DTO");
    let plan = client.plan(&request, "workflow-contract-1").unwrap();
    let request: AnalyzeRequestWireV1 = serde_json::from_value(request).unwrap();
    crate::analysis_engine_adapter::validate_workflow_plan_identity(&request, &plan).unwrap();
    let workflow = plan.workflow_execution.unwrap();
    assert_eq!(workflow.identity.workflow_id, snapshot.workflow_id);
    assert_eq!(
        workflow.identity.workflow_revision,
        snapshot.workflow_revision
    );
    assert_eq!(
        workflow.identity.definition_digest,
        snapshot.definition_digest
    );
    assert!(workflow.nodes.iter().any(|node| {
        node.execution_policy == "disagreement_windows"
            && node.execution_state == WorkflowNodeExecutionStateWireV1::Deferred
    }));
    assert_eq!(workflow.fusion_mode, FusionModeWireV1::Algorithm);
    assert!(
        plan.requirements
            .resources
            .iter()
            .all(|requirement| { requirement.resource.as_str() != "tool:fusion_agent_adapter" })
    );
}

#[test]
fn real_analysis_cli_explicit_lead_stem_forces_only_the_disabled_workflow_branch() {
    let snapshot =
        crate::workflow::compile_workflow(&crate::workflow::default_workflow("lead-stem-song"))
            .unwrap();
    let mut request = analysis_request("lead-stem-workflow-1");
    request["requested_artifacts"]["stems"] = serde_json::json!(["lead_vocal"]);
    request["extensions"][crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY] =
        crate::workflow::workflow_execution_extension(&snapshot).unwrap();

    let mut client = AnalysisCliClient::connect().expect("uta-analyze debug CLI must be built");
    client.validate(&request, "lead-stem-workflow-1").unwrap();
    let plan = client.plan(&request, "lead-stem-workflow-1").unwrap();
    let request: AnalyzeRequestWireV1 = serde_json::from_value(request).unwrap();
    crate::analysis_engine_adapter::validate_workflow_plan_identity(&request, &plan).unwrap();
    assert!(
        plan.execution_nodes
            .iter()
            .any(|node| node.capability.as_str() == "audio.lead_isolate")
    );
    assert!(
        !plan
            .source_route
            .preparation
            .iter()
            .any(|capability| capability.as_str() == "audio.lead_isolate")
    );
    let lead = plan
        .workflow_execution
        .unwrap()
        .nodes
        .into_iter()
        .find(|node| {
            node.capabilities
                .iter()
                .any(|capability| capability.as_str() == "audio.lead_isolate")
        })
        .unwrap();
    assert_eq!(lead.execution_policy, "disabled");
    assert_eq!(
        lead.execution_state,
        WorkflowNodeExecutionStateWireV1::Ready
    );
}

#[test]
fn real_analysis_cli_ai_judgment_plan_requires_the_verified_adapter() {
    let mut definition = crate::workflow::default_workflow("ai-judgment-contract-song");
    crate::workflow::set_workflow_parameter(
        &mut definition,
        &crate::workflow::WorkflowNodeId::new("evidence_fusion"),
        "fusion_mode",
        serde_json::Value::String("ai".to_string()),
    )
    .unwrap();
    let snapshot = crate::workflow::compile_workflow(&definition).unwrap();
    let extension = crate::workflow::workflow_execution_extension(&snapshot).unwrap();
    let mut request = analysis_request("workflow-ai-judgment-contract-1");
    request["analysis"]["profile"] = serde_json::json!("balanced");
    request["requested_artifacts"]["vocal_chart"] = serde_json::json!(true);
    request["extensions"][crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY] = extension;

    let mut client = AnalysisCliClient::connect().expect("uta-analyze debug CLI must be built");
    client
        .validate(&request, "workflow-ai-judgment-contract-1")
        .expect("backend must independently validate the AI judgment workflow DTO");
    let requirements = client
        .requirements(&request, "workflow-ai-judgment-contract-1")
        .unwrap();
    assert!(requirements.resources.iter().any(|requirement| {
        requirement.resource.as_str() == "tool:fusion_agent_adapter" && requirement.required
    }));
    let plan = client
        .plan(&request, "workflow-ai-judgment-contract-1")
        .unwrap();
    let request: AnalyzeRequestWireV1 = serde_json::from_value(request).unwrap();
    crate::analysis_engine_adapter::validate_workflow_plan_identity(&request, &plan).unwrap();
    let workflow = plan.workflow_execution.as_ref().unwrap();
    assert_eq!(workflow.fusion_mode, FusionModeWireV1::AiJudgment);
    assert!(plan.resolved_resources.iter().any(|resource| {
        resource.requirement.resource.as_str() == "tool:fusion_agent_adapter"
            && resource.requirement.required
    }));
}

#[test]
fn real_analysis_cli_ignores_a_legacy_typed_fusion_policy() {
    let snapshot =
        crate::workflow::compile_workflow(&crate::workflow::default_workflow("tampered-policy"))
            .unwrap();
    let mut extension = crate::workflow::workflow_execution_extension(&snapshot).unwrap();
    extension["fusion_policy"] = serde_json::json!({
        "continuous_f0": "fcpe",
        "note_lengths": "game",
        "onset_support": "basic_pitch"
    });
    let mut request = analysis_request("workflow-policy-tamper-1");
    request["extensions"][crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY] = extension;

    let mut client = AnalysisCliClient::connect().expect("uta-analyze debug CLI must be built");
    client
        .validate(&request, "workflow-policy-tamper-1")
        .expect("legacy policy cannot override Stage 3 evidence participation");
    let plan = client.plan(&request, "workflow-policy-tamper-1").unwrap();
    let resolved = plan
        .workflow_execution
        .and_then(|workflow| workflow.fusion_policy)
        .unwrap();
    assert_eq!(resolved.continuous_f0, ContinuousF0SourceWireV1::Rmvpe);
}

#[test]
fn real_analysis_cli_accepts_f0_region_fallback_without_game() {
    let mut definition = crate::workflow::default_workflow("f0-fallback-song");
    crate::workflow::set_workflow_execution_policy(
        &mut definition,
        &crate::workflow::WorkflowNodeId::new("boundary_game"),
        crate::workflow::ExecutionPolicy::Disabled,
    )
    .unwrap();
    let snapshot = crate::workflow::compile_workflow(&definition).unwrap();
    let extension = crate::workflow::workflow_execution_extension(&snapshot).unwrap();
    let mut request = analysis_request("workflow-f0-fallback-1");
    request["requested_artifacts"]["vocal_chart"] = serde_json::json!(true);
    request["requested_artifacts"]["singing_analysis"] = serde_json::json!(true);
    request["requested_artifacts"]["transcript"] = serde_json::json!(true);
    request["requested_artifacts"]["alignment"] = serde_json::json!(true);
    request["extensions"][crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY] = extension;

    let mut client = AnalysisCliClient::connect().expect("uta-analyze debug CLI must be built");
    client
        .validate(&request, "workflow-f0-fallback-1")
        .expect("Engine trust boundary must accept the Studio F0 fallback policy");
    let requirements = client
        .requirements(&request, "workflow-f0-fallback-1")
        .unwrap();
    assert!(
        requirements
            .resources
            .iter()
            .all(|resource| resource.resource != "model:game")
    );
    let plan = client.plan(&request, "workflow-f0-fallback-1").unwrap();
    assert!(
        plan.execution_nodes
            .iter()
            .all(|node| node.capability.as_str() != "notes.game")
    );
    let policy = plan
        .workflow_execution
        .as_ref()
        .and_then(|workflow| workflow.fusion_policy)
        .expect("exact plan must expose resolved fusion policy");
    assert_eq!(policy.continuous_f0, ContinuousF0SourceWireV1::Rmvpe);
    assert_eq!(policy.note_lengths, NoteLengthSourceWireV1::F0Derived);
    assert_eq!(policy.onset_support, OnsetSupportSourceWireV1::Automatic);
}

#[test]
fn real_analysis_cli_ignores_forged_f0_fallback_while_game_remains_enabled() {
    let snapshot =
        crate::workflow::compile_workflow(&crate::workflow::default_workflow("forged-f0-policy"))
            .unwrap();
    let mut extension = crate::workflow::workflow_execution_extension(&snapshot).unwrap();
    extension["fusion_policy"] = serde_json::json!({
        "continuous_f0": "fcpe",
        "note_lengths": "f0_derived",
        "onset_support": "acoustic"
    });
    assert!(extension["nodes"].as_array().unwrap().iter().any(|node| {
        node["provider_preferences"]["primary"] == "game" && node["execution_policy"] != "disabled"
    }));
    let mut request = analysis_request("workflow-forged-f0-game-1");
    request["extensions"][crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY] = extension;

    let mut client = AnalysisCliClient::connect().expect("uta-analyze debug CLI must be built");
    client
        .validate(&request, "workflow-forged-f0-game-1")
        .expect("legacy policy cannot disable configured GAME evidence");
    let plan = client.plan(&request, "workflow-forged-f0-game-1").unwrap();
    let resolved = plan
        .workflow_execution
        .and_then(|workflow| workflow.fusion_policy)
        .unwrap();
    assert_eq!(resolved.note_lengths, NoteLengthSourceWireV1::Game);
}

#[test]
fn real_runtime_cli_result_error_status_and_read_paths_are_non_mutating() {
    let root = std::env::temp_dir().join(format!(
        "uta-studio-runtime-read-contract-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let client = RuntimeCliClient::discover()
        .expect("uta-runtime debug CLI must be built")
        .with_store(&root);
    let statuses = client.list().unwrap();
    assert!(!statuses.is_empty());
    let rmvpe = RuntimeResourceRefWireV1::model("rmvpe").unwrap();
    let status = client.status(std::slice::from_ref(&rmvpe)).unwrap();
    assert_eq!(status[0].resource, rmvpe);
    assert!(!root.join("downloads").exists());
    assert!(!root.join("staging").exists());
    let unknown = RuntimeResourceRefWireV1::model("definitely_unknown").unwrap();
    let error = client.show(&unknown).unwrap_err();
    assert!(matches!(error, BackendCliError::Domain { code, .. } if code == "unknown_resource"));
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
fn fixture_script(label: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!("uta-studio-cli-{label}-{}", std::process::id()));
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

#[cfg(unix)]
const READY: &str = r#"printf '%s\n' '{"type":"ready","protocol":1,"protocol_identity":"uta.analysis-engine.worker","component":"uta-analysis-engine","engine_version":"fixture","contract_versions":["uta.analysis-engine.request/1","uta.analysis-engine.result/1"]}'"#;

#[cfg(unix)]
#[test]
fn runtime_client_configures_observes_resolves_and_clears_the_fusion_adapter() {
    use std::os::unix::fs::PermissionsExt;

    let runtime = RuntimeCliClient::discover()
        .expect("uta-runtime debug CLI must be built")
        .executable()
        .to_path_buf();
    let root = std::env::temp_dir().join(format!(
        "uta-studio-runtime-tool-client-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = root.join("store");
    let adapter = root.join("uta-fusion-agent-adapter");
    let launched = root.join("adapter-launched");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        &adapter,
        format!("#!/bin/sh\nprintf launched > '{}'\n", launched.display()),
    )
    .unwrap();
    std::fs::set_permissions(&adapter, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        format!("{}.uta-fusion-adapter.json", adapter.display()),
        serde_json::to_vec(&serde_json::json!({
            "contract": "uta.fusion_agent_adapter",
            "version": 1,
            "adapter_id": "fusion_agent_adapter",
            "adapter_version": "app-core-smoke",
            "fusion_protocol_version": 3
        }))
        .unwrap(),
    )
    .unwrap();
    let wrapper = fixture_script(
        "runtime-tool-client",
        &format!(
            "unset UTA_STUDIO_FUSION_AGENT_ADAPTER_PATH UTA_STUDIO_FUSION_AGENT_CLI_PATH\nexec '{}' \"$@\"",
            runtime.display()
        ),
    );
    let client = RuntimeCliClient::new(&wrapper).with_store(&store);

    let configured = client
        .configure_tool("fusion_agent_adapter", &adapter)
        .unwrap();
    assert!(configured.usable);
    assert_eq!(configured.tool_version.as_deref(), Some("app-core-smoke"));
    let resource = RuntimeResourceRefWireV1::tool("fusion_agent_adapter").unwrap();
    let status = client.status(std::slice::from_ref(&resource)).unwrap();
    assert!(status[0].usable);
    let resolved = client.resolve_tool("fusion_agent_adapter").unwrap();
    assert_eq!(
        resolved.executable,
        std::fs::canonicalize(&adapter).unwrap()
    );
    assert_eq!(resolved.protocol_version, 3);
    client.clear_tool("fusion_agent_adapter").unwrap();
    assert!(
        !launched.exists(),
        "readiness operations launched the adapter"
    );

    let _ = std::fs::remove_file(wrapper);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn analysis_client_fails_closed_on_protocol_pollution_correlation_size_and_exit() {
    let wrong = fixture_script(
        "wrong-ready",
        "printf '%s\\n' '{\"type\":\"ready\",\"protocol\":2,\"protocol_identity\":\"wrong\",\"component\":\"wrong\",\"engine_version\":\"x\",\"contract_versions\":[]}'",
    );
    assert!(matches!(
        AnalysisCliClient::connect_path(&wrong),
        Err(BackendCliError::ProtocolMismatch(_))
    ));

    let pollution = fixture_script("pollution", "printf '%s\\n' 'human log on stdout'");
    assert!(matches!(
        AnalysisCliClient::connect_path(&pollution),
        Err(BackendCliError::StdoutPollution(_))
    ));

    let exit = fixture_script("exit", "exit 0");
    assert!(matches!(
        AnalysisCliClient::connect_path(&exit),
        Err(BackendCliError::UnexpectedExit(_))
    ));

    let correlation = fixture_script(
        "correlation",
        &format!(
            "{READY}\nread command\nprintf '%s\\n' '{{\"type\":\"validation_result\",\"request_id\":\"wrong-id\",\"valid\":true}}'\nread quit"
        ),
    );
    let mut client = AnalysisCliClient::connect_path(&correlation).unwrap();
    assert!(matches!(
        client.validate(&analysis_request("expected-id"), "expected-id"),
        Err(BackendCliError::RequestIdMismatch { .. })
    ));
    drop(client);

    let missing_error_correlation = fixture_script(
        "missing-error-correlation",
        &format!(
            "{READY}\nread command\nprintf '%s\\n' '{{\"type\":\"error\",\"code\":\"failed\",\"message\":\"missing correlation\",\"retryable\":false}}'\nread quit"
        ),
    );
    let mut client = AnalysisCliClient::connect_path(&missing_error_correlation).unwrap();
    assert!(matches!(
        client.validate(&analysis_request("expected-id"), "expected-id"),
        Err(BackendCliError::RequestIdMismatch { actual: None, .. })
    ));
    drop(client);

    let oversized = fixture_script(
        "oversized",
        "head -c 16777217 /dev/zero | tr '\\000' x; printf '\\n'",
    );
    assert!(matches!(
        AnalysisCliClient::connect_path(&oversized),
        Err(BackendCliError::FrameTooLarge { .. })
    ));

    for path in [
        wrong,
        pollution,
        exit,
        correlation,
        missing_error_correlation,
        oversized,
    ] {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
#[test]
fn analysis_client_delivers_typed_correlated_lifecycle_frames() {
    let fixture = fixture_script(
        "lifecycle",
        &format!(
            "{READY}\nread analyze\nprintf '%s\\n' '{{\"type\":\"analysis_started\",\"request_id\":\"events\"}}'\nprintf '%s\\n' '{{\"type\":\"node_started\",\"schema_version\":1,\"request_id\":\"events\",\"node_id\":\"pitch\",\"presentation_node_id\":\"workflow.f0_rmvpe\",\"capability_id\":\"pitch.track\",\"model_id\":\"rmvpe\",\"implementation\":\"openvino\",\"event_at_ms\":1}}'\nprintf '%s\\n' '{{\"type\":\"node_progress\",\"schema_version\":1,\"request_id\":\"events\",\"node_id\":\"pitch\",\"presentation_node_id\":\"workflow.f0_rmvpe\",\"capability_id\":\"pitch.track\",\"model_id\":\"rmvpe\",\"implementation\":\"openvino\",\"progress\":0.5,\"event_at_ms\":2}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"request_id\":\"events\",\"status\":\"ok\",\"result\":{{}}}}'\nread quit"
        ),
    );
    let mut client = AnalysisCliClient::connect_path(&fixture).unwrap();
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let target = std::sync::Arc::clone(&events);
    let result = client.analyze_with_events(
        &analysis_request("events"),
        "events",
        &std::env::temp_dir(),
        move |event| target.lock().unwrap().push(event),
    );
    assert!(matches!(result, Err(BackendCliError::MalformedFrame(_))));
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].node_id, "pitch");
    assert_eq!(
        events[0].presentation_node_id.as_deref(),
        Some("workflow.f0_rmvpe")
    );
    assert_eq!(events[1].progress, Some(0.5));
    let _ = std::fs::remove_file(fixture);

    let malformed = fixture_script(
        "malformed-lifecycle",
        &format!(
            "{READY}\nread analyze\nprintf '%s\\n' '{{\"type\":\"analysis_started\",\"request_id\":\"bad-event\"}}'\nprintf '%s\\n' '{{\"type\":\"node_progress\",\"schema_version\":1,\"request_id\":\"bad-event\",\"node_id\":\"pitch\",\"capability_id\":\"pitch.track\",\"implementation\":\"openvino\",\"event_at_ms\":1}}'"
        ),
    );
    let mut client = AnalysisCliClient::connect_path(&malformed).unwrap();
    assert!(matches!(
        client.analyze(
            &analysis_request("bad-event"),
            "bad-event",
            &std::env::temp_dir()
        ),
        Err(BackendCliError::MalformedFrame(_))
    ));
    let _ = std::fs::remove_file(malformed);
}

#[cfg(unix)]
#[test]
fn analysis_cancel_handle_correlates_with_the_active_request() {
    let fixture = fixture_script(
        "cancel",
        &format!(
            "{READY}\nread analyze\nprintf '%s\\n' '{{\"type\":\"analysis_started\",\"request_id\":\"cancel-me\"}}'\nprintf '%s\\n' '{{\"type\":\"node_started\",\"schema_version\":1,\"request_id\":\"cancel-me\",\"node_id\":\"decode\",\"capability_id\":\"audio.decode\",\"implementation\":\"ffmpeg\",\"event_at_ms\":1}}'\nread cancel\nprintf '%s\\n' '{{\"type\":\"cancelled\",\"request_id\":\"cancel-me\"}}'\nread quit"
        ),
    );
    let mut client = AnalysisCliClient::connect_path(&fixture).unwrap();
    let cancel = client.cancellation_handle();
    let request = analysis_request("cancel-me");
    let output = std::env::temp_dir();
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let target = std::sync::Arc::clone(&events);
    let worker = std::thread::spawn(move || {
        client.analyze_with_events(&request, "cancel-me", &output, move |event| {
            target.lock().unwrap().push(event.frame_type)
        })
    });
    std::thread::sleep(std::time::Duration::from_millis(25));
    cancel.cancel("cancel-me").unwrap();
    let error = worker.join().unwrap().unwrap_err();
    assert!(matches!(error, BackendCliError::Domain { code, .. } if code == "cancelled"));
    assert_eq!(*events.lock().unwrap(), ["node_started"]);
    let _ = std::fs::remove_file(fixture);
}

#[cfg(unix)]
#[test]
fn analysis_client_can_reconnect_after_a_worker_crash() {
    let marker = std::env::temp_dir().join(format!(
        "uta-studio-cli-restart-marker-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let fixture = fixture_script(
        "crash-restart",
        &format!(
            "{READY}\nif [ ! -f '{}' ]; then\n  touch '{}'\n  read analyze\n  printf '%s\\n' '{{\"type\":\"analysis_started\",\"request_id\":\"crash\"}}'\n  exit 7\nfi\nread validate\nprintf '%s\\n' '{{\"type\":\"validation_result\",\"request_id\":\"restart\",\"valid\":true}}'\nread quit",
            marker.display(),
            marker.display()
        ),
    );
    let mut crashed = AnalysisCliClient::connect_path(&fixture).unwrap();
    assert!(matches!(
        crashed.analyze(&analysis_request("crash"), "crash", &std::env::temp_dir()),
        Err(BackendCliError::UnexpectedExit(_))
    ));
    drop(crashed);

    let mut restarted = AnalysisCliClient::connect_path(&fixture).unwrap();
    restarted
        .validate(&analysis_request("restart"), "restart")
        .unwrap();
    drop(restarted);
    let _ = std::fs::remove_file(marker);
    let _ = std::fs::remove_file(fixture);
}

#[cfg(unix)]
#[test]
fn runtime_client_rejects_schema_mismatch_and_missing_executables() {
    let fixture = fixture_script(
        "runtime-schema",
        "printf '%s\\n' '{\"schema\":\"uta.runtime.result\",\"schema_version\":2,\"type\":\"result\",\"command\":\"list\",\"status\":\"ok\",\"data\":[]}'",
    );
    let error = RuntimeCliClient::new(&fixture).list().unwrap_err();
    assert!(matches!(error, BackendCliError::ProtocolMismatch(_)));
    let missing = std::env::temp_dir().join("uta-studio-missing-analysis-cli");
    assert!(matches!(
        AnalysisCliClient::connect_path(missing),
        Err(BackendCliError::ExecutableMissing(_))
    ));
    let _ = std::fs::remove_file(fixture);
}

#[test]
fn confined_path_helper_fixture_is_absolute() {
    assert!(Path::new("/tmp/uta-studio-contract.wav").is_absolute());
}
