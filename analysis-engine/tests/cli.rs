use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, Command, Stdio};

fn isolated_command(binary: &str) -> (Command, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "uta-analysis-engine-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut command = Command::new(binary);
    command
        .env("UTA_STUDIO_MODELS_DIR", &root)
        .env("UTA_STUDIO_RUNTIME_STORE", &root)
        .env("UTA_STUDIO_MODELS_PATH", &root)
        .env_remove("UTA_STUDIO_OPENVINO_RUNTIME_PATH")
        .env_remove("UTA_STUDIO_QWEN_ASR_RUNTIME_PATH")
        .env_remove("UTA_STUDIO_QWEN_ALIGN_RUNTIME_PATH")
        .env_remove("UTA_STUDIO_ROFORMER_RUNTIME_PATH");
    (command, root)
}

fn request(request_id: &str, version: u32) -> serde_json::Value {
    serde_json::json!({
        "contract": "uta.analysis-engine.request",
        "version": version,
        "request_id": request_id,
        "audio_sources": [{
            "id": "main",
            "kind": "local_file",
            "path": "/fixture/song.flac",
            "sha256": "a".repeat(64),
            "role": "clean_lead_vocal",
            "primary": true,
            "timeline": {"timebase": 1_000_000, "source_start": 0}
        }],
        "lyrics": {"mode": "none", "tokens": []},
        "boundary_constraints": [],
        "analysis": {
            "profile": "fast",
            "track_target": "lead",
            "preserve_continuous_pitch": true,
            "enable_quantization": false
        },
        "requested_artifacts": {
            "vocal_chart": false,
            "pitch_evidence": false,
            "singing_analysis": false,
            "transcript": true,
            "alignment": false,
            "stems": []
        },
        "execution_policy": {"runtime_policy": "production"},
        "extensions": {}
    })
}

fn matrix_request(case: &str) -> serde_json::Value {
    let mut value = request(case, 1);
    value["requested_artifacts"]["transcript"] = serde_json::json!(false);
    match case {
        "stem-only-vocals" => {
            value["audio_sources"][0]["role"] = serde_json::json!("original_mix");
            value["requested_artifacts"]["stems"] = serde_json::json!(["guide_vocals"]);
        }
        "instrumental-only" => {
            value["audio_sources"][0]["role"] = serde_json::json!("original_mix");
            value["requested_artifacts"]["stems"] = serde_json::json!(["instrumental"]);
        }
        "transcript-only" => {
            value["requested_artifacts"]["transcript"] = serde_json::json!(true);
        }
        "alignment-only" => {
            value["lyrics"] = serde_json::json!({
                "mode": "canonical",
                "language": "en",
                "tokens": [{"id":"word-1","text":"sing"}]
            });
            value["requested_artifacts"]["alignment"] = serde_json::json!(true);
        }
        "pitch-only" => {
            value["requested_artifacts"]["pitch_evidence"] = serde_json::json!(true);
        }
        "full-candidate" => {
            value["audio_sources"][0]["role"] = serde_json::json!("original_mix");
            value["requested_artifacts"]["vocal_chart"] = serde_json::json!(true);
            value["requested_artifacts"]["pitch_evidence"] = serde_json::json!(true);
            value["requested_artifacts"]["singing_analysis"] = serde_json::json!(true);
            value["requested_artifacts"]["transcript"] = serde_json::json!(true);
            value["requested_artifacts"]["alignment"] = serde_json::json!(true);
        }
        _ => unreachable!("known request matrix case"),
    }
    value
}

fn send(stdin: &mut ChildStdin, value: &serde_json::Value) {
    serde_json::to_writer(&mut *stdin, value).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
}

fn read_frame(stdout: &mut BufReader<impl std::io::Read>) -> serde_json::Value {
    let mut line = String::new();
    assert!(stdout.read_line(&mut line).unwrap() > 0, "worker exited");
    serde_json::from_str(&line).expect("stdout line must contain exactly one JSON frame")
}

#[test]
fn standalone_capabilities_is_machine_readable_and_read_only() {
    for binary in [
        env!("CARGO_BIN_EXE_uta-analysis-engine"),
        env!("CARGO_BIN_EXE_uta-analyze"),
    ] {
        let (mut command, store) = isolated_command(binary);
        let output = command.arg("capabilities").output().expect("run CLI");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("one JSON capabilities value");
        let capabilities = value.as_array().expect("capability array");
        assert!(
            capabilities
                .iter()
                .any(|entry| entry["id"] == "audio.decode")
        );
        assert!(
            !store.exists(),
            "read-only CLI created the configured store"
        );
    }
}

#[test]
fn standalone_ndjson_worker_contract_is_correlated_bounded_and_stdout_pure() {
    let (mut command, store) = isolated_command(env!("CARGO_BIN_EXE_uta-analyze"));
    let mut child = command
        .args(["worker", "--stdio-json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn standalone worker");
    let mut stdin = child.stdin.take().expect("worker stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("worker stdout"));

    let ready = read_frame(&mut stdout);
    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["protocol"], 1);
    assert_eq!(ready["protocol_identity"], "uta.analysis-engine.worker");
    assert_eq!(ready["component"], "uta-analysis-engine");
    assert!(ready["engine_version"].is_string());
    assert_eq!(ready["contract_versions"].as_array().unwrap().len(), 2);

    send(
        &mut stdin,
        &serde_json::json!({"type": "hello", "protocol": 1}),
    );
    assert_eq!(read_frame(&mut stdout)["type"], "ready");

    let valid = request("wire-valid", 1);
    for (command_type, response_type) in [
        ("validate", "validation_result"),
        ("requirements", "requirements"),
        ("plan", "plan"),
    ] {
        send(
            &mut stdin,
            &serde_json::json!({
                "type": command_type,
                "protocol": 1,
                "request": valid
            }),
        );
        let response = read_frame(&mut stdout);
        assert_eq!(response["type"], response_type);
        assert_eq!(response["request_id"], "wire-valid");
    }

    send(
        &mut stdin,
        &serde_json::json!({"type":"capabilities","protocol":1,"runtime_policy":"production"}),
    );
    let capabilities = read_frame(&mut stdout);
    assert_eq!(capabilities["type"], "capabilities");
    let descriptors = capabilities["capabilities"].as_array().unwrap();
    for capability in ["analysis.acoustic_dsp", "notes.game"] {
        let descriptor = descriptors
            .iter()
            .find(|entry| entry["id"] == capability)
            .unwrap();
        assert_eq!(descriptor["implementation_exists"], true, "{capability}");
    }
    for capability in [
        "fusion.transcript",
        "fusion.alignment",
        "fusion.singing",
        "fusion.candidate_graph",
        "finalize.vocal_chart",
    ] {
        let descriptor = descriptors
            .iter()
            .find(|entry| entry["id"] == capability)
            .unwrap();
        assert_eq!(descriptor["implementation_exists"], true, "{capability}");
    }

    let matrix = [
        ("stem-only-vocals", vec!["model:bs_roformer_vocals_ep317"]),
        ("instrumental-only", vec!["model:melband_roformer_inst_v2"]),
        ("transcript-only", vec!["model:qwen3_asr_1_7b"]),
        ("alignment-only", vec!["model:qwen3_forced_aligner_0_6b"]),
        ("pitch-only", vec!["model:rmvpe"]),
        (
            "full-candidate",
            vec![
                "model:bs_roformer_vocals_ep317",
                "model:melband_roformer_harmony",
                "model:qwen3_asr_1_7b",
                "model:qwen3_forced_aligner_0_6b",
                "model:rmvpe",
                "model:game",
            ],
        ),
    ];
    for (case, expected_models) in matrix {
        let request = matrix_request(case);
        for command_type in ["validate", "requirements", "plan"] {
            send(
                &mut stdin,
                &serde_json::json!({"type":command_type,"protocol":1,"request":request}),
            );
            let response = read_frame(&mut stdout);
            assert_ne!(
                response["type"], "error",
                "{case}/{command_type}: {response}"
            );
            assert_eq!(response["request_id"], case);
            if command_type == "requirements" {
                let resources = response["requirements"]["resources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| entry["resource"].as_str().unwrap())
                    .collect::<Vec<_>>();
                for model in &expected_models {
                    assert!(
                        resources.contains(model),
                        "{case} omitted {model}: {resources:?}"
                    );
                }
                assert_eq!(
                    resources
                        .iter()
                        .filter(|resource| resource.starts_with("model:"))
                        .count(),
                    expected_models.len(),
                    "{case} gained unrelated model requirements: {resources:?}"
                );
            }
        }
    }

    send(
        &mut stdin,
        &serde_json::json!({
            "type": "validate",
            "protocol": 1,
            "request": request("wire-unsupported", 99)
        }),
    );
    let rejected = read_frame(&mut stdout);
    assert_eq!(rejected["type"], "error");
    assert_eq!(rejected["code"], "unsupported_contract_version");
    assert_eq!(rejected["request_id"], "wire-unsupported");

    // The worker must reject and drain an oversized frame, then continue with
    // the next command instead of desynchronizing or treating bytes as logs.
    stdin.write_all(&vec![b'x'; 16 * 1024 * 1024 + 1]).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
    let oversized = read_frame(&mut stdout);
    assert_eq!(oversized["type"], "error");
    assert_eq!(oversized["code"], "invalid_contract");

    send(
        &mut stdin,
        &serde_json::json!({"type": "quit", "protocol": 1}),
    );
    drop(stdin);
    assert!(child.wait().expect("wait for worker").success());
    assert!(
        !store.exists(),
        "worker contract reads created the configured store"
    );
}
