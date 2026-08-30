use super::*;

#[test]
fn firered_schedule_skips_without_typed_transcript_disagreement() {
    let transcript = TranscriptArtifactV1 {
        contract: "uta.analysis-engine.transcript".to_string(),
        version: 1,
        text: "sing now".to_string(),
        language: Some("en".to_string()),
        tokens: Vec::new(),
        confidence: Some(0.95),
        authority: crate::artifact::TranscriptAuthorityV1::Generated,
        source_experts: vec!["qwen3_asr_1_7b".to_string()],
        alternatives: Vec::new(),
        model_sha256: Some("a".repeat(64)),
        runtime_manifest_sha256: Some("b".repeat(64)),
        backend: "vulkan".to_string(),
    };
    let source_range = TimeRange::new(0, 1_000_000).unwrap();
    let regions = build_transcript_disagreement_regions(
        &transcript,
        Some("sing now"),
        Some("en"),
        source_range,
    );
    assert!(regions.is_empty());
    let scheduled = schedule(ConditionalScheduleRequest {
        capability: "speech.transcribe.challenger",
        policy: WorkflowExecutionPolicyV1::OnDisagreement,
        profile: crate::contract::AnalysisProfile::Balanced,
        source_range,
        review_regions: &regions,
        relevant_reasons: &[
            SingingReviewReason::TranscriptLowConfidence,
            SingingReviewReason::TranscriptReferenceMismatch,
            SingingReviewReason::TranscriptLanguageMismatch,
        ],
        optional_usable: true,
        required: false,
        supports_windowed_input: false,
        full_input_on_disagreement: true,
    })
    .unwrap();
    assert_eq!(
        scheduled,
        ScheduledExecution::Skip(ScheduleSkipReason::NoRelevantDisagreement)
    );
}

#[test]
#[cfg(unix)]
fn firered_worker_failure_degrades_and_preserves_qwen_baseline() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-engine-firered-failure-{}-{stamp}",
        std::process::id()
    ));
    let store = root.join("store");
    let output = root.join("output");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&output).unwrap();
    let source_identity = uta_runtime_manager::SourceIdentity::default();
    for model in ["qwen3_asr_1_7b", "firered_asr2_aed"] {
        install_fixture_generation(&store, model, Some(source_identity.clone()));
    }
    let source = root.join("source.wav");
    fs::write(&source, b"authorized fixture audio").unwrap();
    let ffmpeg = root.join("ffmpeg");
    executable(&ffmpeg, &non_silent_pcm_script(48_000));

    let qwen_output = output.join("worker/asr/qwen.json");
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
    let firered_worker = root.join("openvino-worker");
    executable(
        &firered_worker,
        &format!(
            "printf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"component\":\"uta-openvino-worker\",\"runtime_recipe_digest\":\"{}\"}}'\nread run\nprintf '%s\\n' '{{\"type\":\"error\",\"task_id\":\"task-firered\",\"code\":\"fixture_failure\",\"message\":\"challenger failed\",\"retryable\":false}}'",
            uta_runtime_manager::OPENVINO_WORKER_RECIPE_SHA256,
        ),
    );

    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    for model in ["qwen3_asr_1_7b", "firered_asr2_aed"] {
        catalog.models.get_mut(model).unwrap().source = source_identity.clone();
    }
    let engine = AnalysisEngine::new(RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&store)
            .with_tool_override("ffmpeg", &ffmpeg)
            .with_runtime_override("qwen_asr_runtime", &qwen_worker)
            .with_runtime_override("openvino_2026_3", &firered_worker),
    ));
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.request_id = "firered-failure".to_string();
    request.audio_sources[0].path = source;
    request.analysis.profile = crate::contract::AnalysisProfile::Balanced;
    request.execution_policy.runtime_policy = uta_runtime_manager::RuntimePolicy::Benchmark;
    request.requested_artifacts.vocal_chart = false;
    request.requested_artifacts.pitch_evidence = false;
    request.requested_artifacts.singing_analysis = false;
    request.requested_artifacts.alignment = false;
    request.lyrics.mode = crate::contract::LyricsMode::Reference;
    request.lyrics.language = Some("en".to_string());
    request.lyrics.tokens = vec![crate::contract::LyricTokenV1 {
        id: "reference-1".to_string(),
        text: "sing know".to_string(),
        reading: None,
        phonemes: None,
        start: None,
        end: None,
    }];

    let result = engine.analyze(&request, &output).unwrap();
    assert_eq!(result.status, AnalysisStatus::OkDegraded);
    assert!(result.degraded_reasons.iter().any(|reason| {
        reason.contains("speech.transcribe.challenger failed")
            && reason.contains("challenger failed")
    }));
    let transcript_ref = result.artifacts.transcript.unwrap();
    let transcript: TranscriptArtifactV1 =
        serde_json::from_slice(&fs::read(output.join(transcript_ref.path)).unwrap()).unwrap();
    assert_eq!(transcript.text, "sing know");
    assert!(
        transcript
            .alternatives
            .iter()
            .any(|text| text == "sing now")
    );
    assert!(!output.join("worker/firered").exists());
    fs::remove_dir_all(root).unwrap();
}
