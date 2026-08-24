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

    let oversized = fixture_script(
        "oversized",
        "head -c 16777217 /dev/zero | tr '\\000' x; printf '\\n'",
    );
    assert!(matches!(
        AnalysisCliClient::connect_path(&oversized),
        Err(BackendCliError::FrameTooLarge { .. })
    ));

    for path in [wrong, pollution, exit, correlation, oversized] {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
#[test]
fn analysis_cancel_handle_correlates_with_the_active_request() {
    let fixture = fixture_script(
        "cancel",
        &format!(
            "{READY}\nread analyze\nprintf '%s\\n' '{{\"type\":\"analysis_started\",\"request_id\":\"cancel-me\"}}'\nread cancel\nprintf '%s\\n' '{{\"type\":\"cancelled\",\"request_id\":\"cancel-me\"}}'\nread quit"
        ),
    );
    let mut client = AnalysisCliClient::connect_path(&fixture).unwrap();
    let cancel = client.cancellation_handle();
    let request = analysis_request("cancel-me");
    let output = std::env::temp_dir();
    let worker = std::thread::spawn(move || client.analyze(&request, "cancel-me", &output));
    std::thread::sleep(std::time::Duration::from_millis(25));
    cancel.cancel("cancel-me").unwrap();
    let error = worker.join().unwrap().unwrap_err();
    assert!(matches!(error, BackendCliError::Domain { code, .. } if code == "cancelled"));
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
