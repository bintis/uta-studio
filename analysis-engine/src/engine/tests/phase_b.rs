use super::*;

fn workflow_with_missing_cleanup() -> serde_json::Value {
    let mut workflow = fcpe_primary_candidate_workflow();
    let object = workflow.as_object_mut().unwrap();
    let nodes = object["nodes"].as_array_mut().unwrap();
    nodes.extend([
        serde_json::json!({
            "instance_id": "vocal_denoise",
            "capability_id": "audio.denoise",
            "execution_policy": "always",
            "priority": 850,
            "provider_preferences": {
                "primary": "melband_roformer_denoise_aufr33",
                "instrumental": null
            }
        }),
        serde_json::json!({
            "instance_id": "vocal_dereverb",
            "capability_id": "audio.dereverb",
            "execution_policy": "always",
            "priority": 840,
            "provider_preferences": {
                "primary": "melband_roformer_dereverb_anvuew",
                "instrumental": null
            }
        }),
    ]);
    let bindings = object["bindings"].as_array_mut().unwrap();
    for binding in bindings.iter_mut() {
        if binding["analyzer_attachment"] == true && binding["from_node"] == "lead_isolate" {
            binding["from_node"] = serde_json::Value::String("vocal_dereverb".to_string());
            binding["from_port"] = serde_json::Value::String("audio".to_string());
        }
    }
    bindings.extend([
        serde_json::json!({
            "from_node": "lead_isolate",
            "from_port": "lead",
            "to_node": "vocal_denoise",
            "to_port": "audio",
            "semantic_type": "audio",
            "audio_role": "lead_vocal",
            "execution_active": true,
            "analyzer_attachment": false
        }),
        serde_json::json!({
            "from_node": "vocal_denoise",
            "from_port": "audio",
            "to_node": "vocal_dereverb",
            "to_port": "audio",
            "semantic_type": "audio",
            "audio_role": "lead_vocal",
            "execution_active": true,
            "analyzer_attachment": false
        }),
    ]);
    workflow
}

#[test]
#[cfg(unix)]
fn preview_and_execution_degrade_when_optional_cleanup_resources_are_missing() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-engine-missing-cleanup-{}-{stamp}",
        std::process::id()
    ));
    let store = root.join("store");
    let output = root.join("output");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&output).unwrap();
    let source_identity = uta_runtime_manager::SourceIdentity::default();
    install_fixture_generation(&store, "fcpe", Some(source_identity.clone()));

    let source = root.join("source.wav");
    fs::write(&source, b"authorized fixture audio").unwrap();
    let ffmpeg = root.join("ffmpeg");
    executable(&ffmpeg, &non_silent_pcm_script(48_000));
    let pitch_output = output.join("worker/fcpe/pitch.json");
    let worker = root.join("openvino-worker");
    executable(
        &worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\nmkdir -p '{}'\nprintf '%s\\n' '{{\"schema_version\":3,\"model_id\":\"fcpe\",\"source_model_sha256\":\"{}\",\"model_manifest_sha256\":\"{}\",\"model_xml_sha256\":\"{}\",\"model_bin_sha256\":\"{}\",\"runtime_manifest_sha256\":\"{}\",\"backend\":\"openvino_gpu\",\"timeline_step_ms\":10,\"sample_rate\":16000,\"window_samples\":32000,\"window_hop_samples\":32000,\"frames\":[{{\"time\":0.0,\"hz\":440.0}},{{\"time\":0.01,\"hz\":440.0}}]}}' > '{}'\nprintf '%s\\n' '{{\"type\":\"output\",\"task_id\":\"task-fcpe\",\"artifact\":\"pitch_evidence\",\"path\":\"{}\",\"media_type\":\"application/json\"}}'\nprintf '%s\\n' '{{\"type\":\"done\",\"task_id\":\"task-fcpe\",\"status\":\"ok\"}}'\nread quit",
            uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
            pitch_output.parent().unwrap().display(),
            "b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0",
            "bd356b9d018bbf55f7b87bbc8e4a712496b587a306249c941ff30beb5d548df6",
            "9941d7251ff0bdedc7875cabd40c30c2c60db00b36a617c9e957044d669bc237",
            "6b6c62535552181c9efe305837af09a2a8987585ce368b2c522242b59676f824",
            uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
            pitch_output.display(),
            pitch_output.display(),
        ),
    );
    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    catalog.models.get_mut("fcpe").unwrap().source = source_identity;
    let engine = AnalysisEngine::new(RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&store)
            .with_tool_override("ffmpeg", &ffmpeg)
            .with_runtime_override("openvino_2026_3", &worker),
    ));
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.request_id = "missing-optional-cleanup".to_string();
    request.audio_sources[0].path = source;
    request.requested_artifacts.vocal_chart = false;
    request.requested_artifacts.singing_analysis = false;
    request.requested_artifacts.transcript = false;
    request.requested_artifacts.alignment = false;
    request.execution_policy.runtime_policy = uta_runtime_manager::RuntimePolicy::Benchmark;
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        workflow_with_missing_cleanup(),
    );

    let plan = engine.plan(&request).unwrap();
    for model in [
        "melband_roformer_denoise_aufr33",
        "melband_roformer_dereverb_anvuew",
    ] {
        let resource = plan
            .resolved_resources
            .iter()
            .find(|resource| resource.requirement.resource == format!("model:{model}"))
            .unwrap();
        assert!(!resource.requirement.required);
        assert!(
            resource
                .status
                .as_ref()
                .is_some_and(|status| !status.usable)
        );
    }
    let (_, preview_degraded) = engine.resolve_execution_resources(&request, &plan).unwrap();
    assert!(
        preview_degraded
            .iter()
            .any(|reason| reason.contains("audio.denoise"))
    );
    assert!(
        preview_degraded
            .iter()
            .any(|reason| reason.contains("audio.dereverb"))
    );

    let result = engine.analyze(&request, &output).unwrap();
    assert_eq!(result.status, AnalysisStatus::OkDegraded);
    assert!(
        result
            .degraded_reasons
            .iter()
            .any(|reason| reason.contains("audio.denoise"))
    );
    assert!(
        result
            .degraded_reasons
            .iter()
            .any(|reason| reason.contains("audio.dereverb"))
    );
    assert!(result.artifacts.pitch_evidence.is_some());
    fs::remove_dir_all(root).unwrap();
}
