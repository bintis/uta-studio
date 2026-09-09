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
fn real_analysis_cli_validates_and_projects_the_current_compiled_workflow() {
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
    assert_eq!(workflow.fusion_mode, FusionModeWireV1::Algorithm);
    assert!(
        plan.requirements
            .resources
            .iter()
            .all(|requirement| requirement.resource.as_str() != "tool:fusion_agent_adapter")
    );
}

#[test]
fn real_analysis_cli_explicit_lead_stem_forces_the_disabled_workflow_branch() {
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
fn real_analysis_cli_routes_transcription_to_qwen() {
    let mut request = analysis_request("qwen-transcription-1");
    request["requested_artifacts"]["transcript"] = serde_json::json!(true);
    let mut client = AnalysisCliClient::connect().expect("uta-analyze debug CLI must be built");
    client.validate(&request, "qwen-transcription-1").unwrap();
    let requirements = client
        .requirements(&request, "qwen-transcription-1")
        .unwrap();
    assert!(requirements.resources.iter().any(|requirement| {
        requirement.required && requirement.resource == "model:qwen3_asr_1_7b"
    }));
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
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!("uta-studio-cli-{label}-{}", std::process::id()));
    let staging = path.with_extension("part");
    {
        let mut file = std::fs::File::create(&staging).unwrap();
        file.write_all(format!("#!/bin/sh\n{body}\n").as_bytes())
            .unwrap();
        file.sync_all().unwrap();
    }
    let mut permissions = std::fs::metadata(&staging).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&staging, permissions).unwrap();
    std::fs::rename(staging, &path).unwrap();
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
            "fusion_protocol_version": 4
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
    assert_eq!(resolved.protocol_version, 4);
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
            "{READY}\nread analyze\nprintf '%s\\n' '{{\"type\":\"analysis_started\",\"request_id\":\"events\"}}'\nprintf '%s\\n' '{{\"type\":\"node_started\",\"schema_version\":1,\"request_id\":\"events\",\"node_id\":\"pitch\",\"presentation_node_id\":\"workflow.f0_rmvpe\",\"capability_id\":\"pitch.track\",\"model_id\":\"rmvpe\",\"implementation\":\"ggml_vulkan\",\"event_at_ms\":1}}'\nprintf '%s\\n' '{{\"type\":\"node_progress\",\"schema_version\":1,\"request_id\":\"events\",\"node_id\":\"pitch\",\"presentation_node_id\":\"workflow.f0_rmvpe\",\"capability_id\":\"pitch.track\",\"model_id\":\"rmvpe\",\"implementation\":\"ggml_vulkan\",\"progress\":0.5,\"event_at_ms\":2}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"request_id\":\"events\",\"status\":\"ok\",\"result\":{{}}}}'\nread quit"
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
fn analysis_force_stop_terminates_the_worker_process_tree() {
    let child_pid_path = std::env::temp_dir().join(format!(
        "uta-studio-force-stop-child-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&child_pid_path);
    let fixture = fixture_script(
        "force-stop",
        &format!(
            "{READY}\nread analyze\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nprintf '%s\\n' '{{\"type\":\"analysis_started\",\"request_id\":\"force-stop\"}}'\nwhile :; do sleep 30; done",
            child_pid_path.display()
        ),
    );
    let mut client = AnalysisCliClient::connect_path(&fixture).unwrap();
    let stop = client.cancellation_handle();
    let request = analysis_request("force-stop");
    let worker =
        std::thread::spawn(move || client.analyze(&request, "force-stop", &std::env::temp_dir()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !child_pid_path.is_file() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let child_pid = std::fs::read_to_string(&child_pid_path)
        .unwrap()
        .parse::<u32>()
        .unwrap();

    stop.force_stop().unwrap();
    assert!(worker.join().unwrap().is_err());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while unix_process_is_running(child_pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        !unix_process_is_running(child_pid),
        "force-stop left descendant process {child_pid} alive"
    );
    let _ = std::fs::remove_file(child_pid_path);
    let _ = std::fs::remove_file(fixture);
}

#[cfg(unix)]
fn unix_process_is_running(pid: u32) -> bool {
    // SAFETY: signal 0 performs existence/permission checking only.
    if unsafe { libc::kill(pid as i32, 0) } != 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        && stat
            .rsplit_once(')')
            .and_then(|(_, fields)| fields.split_ascii_whitespace().next())
            == Some("Z")
    {
        return false;
    }
    true
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
    assert!(
        matches!(error, BackendCliError::ProtocolMismatch(_)),
        "unexpected runtime client error: {error:?}"
    );
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
