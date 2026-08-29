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
        .env_remove("UTA_STUDIO_QWEN_ALIGN_RUNTIME_PATH");
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
        "lead-stem-only" => {
            value["audio_sources"][0]["role"] = serde_json::json!("original_mix");
            value["requested_artifacts"]["stems"] = serde_json::json!(["lead_vocal"]);
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
    let binary = env!("CARGO_BIN_EXE_uta-analyze");
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
        (
            "lead-stem-only",
            vec![
                "model:bs_roformer_vocals_ep317",
                "model:melband_roformer_harmony",
            ],
        ),
        ("instrumental-only", vec!["model:melband_roformer_inst_v2"]),
        ("transcript-only", vec!["model:qwen3_asr_1_7b"]),
        ("alignment-only", vec!["model:qwen3_forced_aligner_0_6b"]),
        ("pitch-only", vec!["model:rmvpe"]),
        (
            "full-candidate",
            vec![
                "model:bs_roformer_vocals_ep317",
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

fn f0_derived_full_candidate_request(request_id: &str) -> serde_json::Value {
    let mut value = matrix_request("full-candidate");
    value["request_id"] = serde_json::json!(request_id);
    value["extensions"]["uta.workflow_execution.v1"] = serde_json::json!({
        "contract": "uta.workflow-execution",
        "version": 1,
        "workflow_schema_version": 2,
        "workflow_id": "song:test:f0-derived",
        "workflow_revision": 1,
        "quality_mode": "fast",
        "definition_digest": "f".repeat(32),
        "fusion_policy": {
            "continuous_f0": "rmvpe",
            "note_lengths": "f0_derived",
            "onset_support": "automatic"
        },
        "nodes": [
            workflow_node("source", "audio.source", None, "always", 1000, "native_dsp", serde_json::json!({})),
            workflow_node("split", "audio.separate_vocal_bgm", Some("bs_roformer_vocals_ep317"), "always", 900, "vulkan", serde_json::json!({})),
            workflow_node("lead", "audio.lead_isolate", Some("melband_roformer_harmony"), "always", 800, "vulkan", serde_json::json!({})),
            workflow_node("asr", "analysis.asr", Some("qwen3_asr_1_7b"), "always", 700, "pinned_qwen_asr_vulkan", serde_json::json!({})),
            workflow_node("transcript", "fusion.transcript", None, "always", 690, "native_dsp", serde_json::json!({})),
            workflow_node("align", "analysis.forced_alignment", Some("qwen3_forced_aligner_0_6b"), "always", 680, "pinned_qwen_align_vulkan", serde_json::json!({})),
            workflow_node("pitch", "analysis.pitch_f0", Some("rmvpe"), "always", 670, "openvino", serde_json::json!({})),
            workflow_node(
                "fusion",
                "fusion.singing_evidence",
                None,
                "always",
                500,
                "native_dsp",
                serde_json::json!({
                    "pitch_owner": "rmvpe",
                    "boundary_owner": "f0",
                    "onset_owner": "automatic"
                }),
            ),
            workflow_node("candidates", "fusion.candidate_graph", None, "always", 400, "native_dsp", serde_json::json!({})),
            workflow_node("canonical", "finalize.canonical_singing_track", None, "always", 300, "native_dsp", serde_json::json!({}))
        ],
        "bindings": [
            workflow_binding("source", "mix", "split", "audio", "audio", Some("source_mix"), false),
            workflow_binding("split", "vocal", "lead", "audio", "audio", Some("vocal"), false),
            workflow_binding("lead", "lead", "asr", "audio", "audio", Some("lead_vocal"), true),
            workflow_binding("lead", "lead", "align", "audio", "audio", Some("lead_vocal"), true),
            workflow_binding("lead", "lead", "pitch", "audio", "audio", Some("lead_vocal"), true),
            workflow_binding("asr", "transcript", "transcript", "evidence", "transcript_evidence", None, false),
            workflow_binding("transcript", "lyrics", "align", "lyrics", "lyrics", None, false),
            workflow_binding("transcript", "lyrics", "canonical", "lyrics", "lyrics", None, false),
            workflow_binding("align", "alignment", "fusion", "alignment", "alignment_evidence", None, false),
            workflow_binding("pitch", "pitch", "fusion", "pitch", "pitch_evidence", None, false),
            workflow_binding("fusion", "evidence", "candidates", "evidence", "evidence_bundle", None, false),
            workflow_binding("candidates", "candidates", "canonical", "candidates", "candidate_graph", None, false)
        ],
        "terminal_outputs": [
            {
                "node": "canonical",
                "port": "track",
                "semantic_type": "canonical_singing_track"
            },
            {
                "node": "canonical",
                "port": "chart",
                "semantic_type": "candidate_chart"
            }
        ]
    });
    value
}

fn workflow_node(
    instance_id: &str,
    capability_id: &str,
    model_id: Option<&str>,
    execution_policy: &str,
    priority: i32,
    _runtime: &str,
    _parameters: serde_json::Value,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "instance_id": instance_id,
        "capability_id": capability_id,
        "execution_policy": execution_policy,
        "priority": priority,
        "provider_preferences": {
            "primary": model_id,
            "instrumental": null
        }
    });
    if capability_id == "audio.separate_vocal_bgm" {
        value["execution_invocations"] = serde_json::json!([{
            "invocation_id": format!("{instance_id}.vocal"),
            "provider_id": model_id.expect("separation fixture has a provider"),
            "capabilities": ["audio.extract_vocals"],
            "output_ports": ["vocal"]
        }]);
    }
    value
}

fn workflow_binding(
    from_node: &str,
    from_port: &str,
    to_node: &str,
    to_port: &str,
    semantic_type: &str,
    audio_role: Option<&str>,
    analyzer_attachment: bool,
) -> serde_json::Value {
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
}

fn run_request_command(command_name: &str, request: &serde_json::Value) -> serde_json::Value {
    let request_path = std::env::temp_dir().join(format!(
        "uta-analysis-engine-f0-derived-{}-{}-{command_name}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&request_path, serde_json::to_vec(request).unwrap()).unwrap();
    let (mut command, store) = isolated_command(env!("CARGO_BIN_EXE_uta-analyze"));
    let output = command
        .args([
            command_name,
            "--request",
            request_path
                .to_str()
                .expect("temporary request path is UTF-8"),
        ])
        .output()
        .expect("run packaged uta-analyze");
    let _ = std::fs::remove_file(&request_path);
    let _ = std::fs::remove_dir_all(store);
    assert!(
        output.status.success(),
        "{command_name} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("CLI command must emit one JSON value")
}

#[test]
fn f0_derived_fusion_crosses_the_packaged_cli_boundary() {
    let request = f0_derived_full_candidate_request("f0-derived-cli");

    let validation = run_request_command("validate", &request);
    assert_eq!(validation["valid"], true);

    let requirements = run_request_command("requirements", &request);
    let resources = requirements["resources"]
        .as_array()
        .expect("requirements resources")
        .iter()
        .filter_map(|resource| resource["resource"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(resources.contains("model:rmvpe"));
    assert!(!resources.contains("model:game"));

    let plan = run_request_command("plan", &request);
    let capabilities = plan["execution_nodes"]
        .as_array()
        .expect("execution nodes")
        .iter()
        .filter_map(|node| node["capability"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(capabilities.contains("fusion.singing"));
    assert!(capabilities.contains("fusion.candidate_graph"));
    assert!(!capabilities.contains("notes.game"));
}

#[test]
fn legacy_private_fusion_parameters_fail_closed() {
    let mut request = f0_derived_full_candidate_request("fusion-policy-conflict");
    let nodes = request["extensions"]["uta.workflow_execution.v1"]["nodes"]
        .as_array_mut()
        .expect("workflow nodes");
    let fusion = nodes
        .iter_mut()
        .find(|node| node["capability_id"] == "fusion.singing_evidence")
        .expect("fusion node");
    fusion["parameters"]["boundary_owner"] = serde_json::json!("game");

    let request_path = std::env::temp_dir().join(format!(
        "uta-analysis-engine-fusion-conflict-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let (mut command, store) = isolated_command(env!("CARGO_BIN_EXE_uta-analyze"));
    let output = command
        .args([
            "validate",
            "--request",
            request_path
                .to_str()
                .expect("temporary request path is UTF-8"),
        ])
        .output()
        .expect("run packaged uta-analyze");
    let _ = std::fs::remove_file(request_path);
    let _ = std::fs::remove_dir_all(store);
    assert!(!output.status.success());
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("unknown field `parameters`"),
        "unexpected diagnostics: {diagnostics}"
    );
}
