use super::*;
use crate::contract::request::tests::valid_request;

fn resource_ids(requirements: &EngineRequirementsV1) -> BTreeSet<&str> {
    requirements
        .resources
        .iter()
        .map(|resource| resource.resource.as_str())
        .collect()
}

fn pitch_only_workflow_with_lead_isolation_policy(lead_policy: &str) -> serde_json::Value {
    let node = |instance: &str,
                capability: &str,
                provider: Option<&str>,
                execution_policy: &str,
                priority: i32| {
        let mut value = serde_json::json!({
            "instance_id": instance,
            "capability_id": capability,
            "execution_policy": execution_policy,
            "priority": priority,
            "provider_preferences": {
                "primary": provider,
                "instrumental": null
            }
        });
        if capability == "audio.separate_vocal_bgm" {
            value["execution_invocations"] = serde_json::json!([{
                "invocation_id": format!("{instance}.vocal"),
                "provider_id": provider.expect("separation fixture has a provider"),
                "capabilities": ["audio.extract_vocals"],
                "output_ports": ["vocal"]
            }]);
        }
        value
    };
    serde_json::json!({
        "contract": crate::workflow::WORKFLOW_EXECUTION_CONTRACT,
        "version": 1,
        "workflow_schema_version": crate::workflow::WORKFLOW_SCHEMA_VERSION,
        "workflow_id": "song:test:workflow",
        "workflow_revision": 1,
        "quality_mode": "balanced",
        "definition_digest": "a".repeat(32),
        "nodes": [
            node("source", "audio.source", None, "always", 1000),
            node("split", "audio.separate_vocal_bgm", Some("bs_roformer_leap_xe90_vocals"), "always", 900),
            node("lead", "audio.lead_isolate", Some("melband_roformer_harmony"), lead_policy, 880),
            node("pitch", "analysis.pitch_f0", Some("rmvpe"), "always", 680)
        ],
        "bindings": [
            {
                "from_node": "source",
                "from_port": "mix",
                "to_node": "split",
                "to_port": "audio",
                "semantic_type": "audio",
                "audio_role": "source_mix",
                "execution_active": true,
                "analyzer_attachment": false
            },
            {
                "from_node": "split",
                "from_port": "vocal",
                "to_node": "lead",
                "to_port": "audio",
                "semantic_type": "audio",
                "audio_role": "vocal",
                "execution_active": lead_policy != "disabled",
                "analyzer_attachment": false
            },
            {
                "from_node": "split",
                "from_port": "vocal",
                "to_node": "pitch",
                "to_port": "audio",
                "semantic_type": "audio",
                "audio_role": "vocal",
                "execution_active": true,
                "analyzer_attachment": true
            }
        ],
        "terminal_outputs": [{
            "node": "pitch",
            "port": "pitch",
            "semantic_type": "pitch_evidence"
        }]
    })
}

fn pitch_workflow_with_enabled_cleanup() -> serde_json::Value {
    let mut workflow = pitch_only_workflow_with_lead_isolation_policy("disabled");
    workflow["nodes"].as_array_mut().unwrap().extend([
        serde_json::json!({
            "instance_id": "denoise",
            "capability_id": "audio.denoise",
            "execution_policy": "always",
            "priority": 860,
            "provider_preferences": {
                "primary": "melband_roformer_denoise_aufr33",
                "instrumental": null
            }
        }),
        serde_json::json!({
            "instance_id": "dereverb",
            "capability_id": "audio.dereverb",
            "execution_policy": "always",
            "priority": 850,
            "provider_preferences": {
                "primary": "melband_roformer_dereverb_anvuew",
                "instrumental": null
            }
        }),
    ]);
    let bindings = workflow["bindings"].as_array_mut().unwrap();
    bindings.retain(|binding| binding["to_node"] != "pitch");
    bindings.extend([
        serde_json::json!({
            "from_node": "split", "from_port": "vocal",
            "to_node": "denoise", "to_port": "audio",
            "semantic_type": "audio", "audio_role": "vocal",
            "execution_active": true, "analyzer_attachment": false
        }),
        serde_json::json!({
            "from_node": "denoise", "from_port": "audio",
            "to_node": "dereverb", "to_port": "audio",
            "semantic_type": "audio", "audio_role": "vocal",
            "execution_active": true, "analyzer_attachment": false
        }),
        serde_json::json!({
            "from_node": "dereverb", "from_port": "audio",
            "to_node": "pitch", "to_port": "audio",
            "semantic_type": "audio", "audio_role": "vocal",
            "execution_active": true, "analyzer_attachment": true
        }),
    ]);
    workflow
}

#[test]
fn disabled_lead_isolation_is_omitted_and_pitch_binds_to_vocal_output() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request.requested_artifacts = crate::contract::RequestedArtifactsV1 {
        vocal_chart: false,
        pitch_evidence: true,
        singing_analysis: false,
        transcript: false,
        alignment: false,
        stems: Vec::new(),
    };
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        pitch_only_workflow_with_lead_isolation_policy("disabled"),
    );

    let requirements = Planner::requirements(&request).unwrap();
    let resources = resource_ids(&requirements);
    assert!(resources.contains("model:bs_roformer_leap_xe90_vocals"));
    assert!(resources.contains("model:rmvpe"));
    assert!(!resources.contains("model:melband_roformer_harmony"));
    assert!(!resources.contains("model:game"));
    assert!(!resources.contains("model:fcpe"));

    let plan = Planner::plan(&request, None).unwrap();
    assert!(has_node(&plan, "audio.extract_vocals"));
    assert!(has_node(&plan, "pitch.track"));
    assert!(!has_node(&plan, "audio.lead_isolate"));
    assert!(!has_node(&plan, "notes.game"));
}

#[test]
fn explicit_lead_vocal_output_forces_lead_isolation() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request.requested_artifacts.stems.push(AudioRole::LeadVocal);

    let requirements = Planner::requirements(&request).unwrap();
    assert!(resource_ids(&requirements).contains("model:melband_roformer_harmony"));
    let plan = Planner::plan(&request, None).unwrap();
    assert!(has_node(&plan, "audio.extract_vocals"));
    assert!(has_node(&plan, "audio.lead_isolate"));
}

#[test]
fn explicit_lead_vocal_output_forces_a_disabled_isolation_workflow_for_the_stem_branch() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request.requested_artifacts = crate::contract::RequestedArtifactsV1 {
        vocal_chart: false,
        pitch_evidence: false,
        singing_analysis: false,
        transcript: false,
        alignment: false,
        stems: vec![AudioRole::LeadVocal],
    };
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        pitch_only_workflow_with_lead_isolation_policy("disabled"),
    );

    let requirements = Planner::requirements(&request).unwrap();
    assert!(resource_ids(&requirements).contains("model:melband_roformer_harmony"));
    let plan = Planner::plan(&request, None).unwrap();
    assert!(has_node(&plan, "audio.lead_isolate"));
    assert!(
        !plan
            .source_route
            .preparation
            .iter()
            .any(|capability| capability.as_str() == "audio.lead_isolate")
    );
    let lead = plan
        .workflow_execution
        .as_ref()
        .unwrap()
        .node_for_capability("audio.lead_isolate")
        .unwrap();
    assert_eq!(
        lead.execution_state,
        crate::workflow_executor::WorkflowNodeExecutionStateV1::Ready
    );
    assert_eq!(
        lead.execution_policy,
        crate::workflow::WorkflowExecutionPolicyV1::Disabled
    );
}

#[test]
fn explicit_lead_vocal_output_forces_every_conditional_isolation_policy_ready() {
    for (wire_policy, expected_policy) in [
        (
            "maximum_only",
            crate::workflow::WorkflowExecutionPolicyV1::MaximumOnly,
        ),
        (
            "on_disagreement",
            crate::workflow::WorkflowExecutionPolicyV1::OnDisagreement,
        ),
        (
            "disagreement_windows",
            crate::workflow::WorkflowExecutionPolicyV1::DisagreementWindows,
        ),
    ] {
        let mut request = valid_request(AudioRole::OriginalMix);
        request.requested_artifacts = crate::contract::RequestedArtifactsV1 {
            vocal_chart: false,
            pitch_evidence: true,
            singing_analysis: false,
            transcript: false,
            alignment: false,
            stems: vec![AudioRole::LeadVocal],
        };
        request.extensions.insert(
            crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
            pitch_only_workflow_with_lead_isolation_policy(wire_policy),
        );

        let plan = Planner::plan(&request, None).unwrap();
        let lead = plan
            .workflow_execution
            .as_ref()
            .unwrap()
            .node_for_capability("audio.lead_isolate")
            .unwrap();
        assert_eq!(
            lead.execution_state,
            crate::workflow_executor::WorkflowNodeExecutionStateV1::Ready
        );
        assert_eq!(lead.execution_policy, expected_policy);
        assert!(
            plan.source_route
                .preparation
                .iter()
                .any(|capability| capability.as_str() == "audio.lead_isolate")
        );
    }
}

fn has_node(plan: &EnginePlan, capability: &str) -> bool {
    plan.execution_nodes
        .iter()
        .any(|node| node.capability.as_str() == capability)
}

#[test]
fn japanese_maximum_schedules_the_dual_input_jbm555_expert() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request.analysis.profile = AnalysisProfile::Maximum;
    request.lyrics.language = Some("ja-JP".to_string());

    let requirements = Planner::requirements(&request).unwrap();
    assert!(resource_ids(&requirements).contains("model:jbm555_cectc_80"));

    let plan = Planner::plan(&request, None).unwrap();
    let jbm = plan
        .execution_nodes
        .iter()
        .find(|node| node.capability.as_str() == "notes.jbm555")
        .expect("Japanese Maximum includes the JBM555 expert");
    assert!(
        jbm.depends_on
            .iter()
            .any(|dependency| dependency == "decode")
    );
    assert!(has_node(&plan, "fusion.singing"));
}

#[test]
fn optional_technique_output_is_declared_before_execution() {
    let request = valid_request(AudioRole::OriginalMix);
    let declarations = artifact_declarations(
        &request,
        &[node("stars-technique", "technique.analyze", false, &[])],
    );
    let declaration = declarations
        .iter()
        .find(|item| item.semantic_type == "technique_evidence")
        .unwrap();
    assert!(!declaration.required);
    assert_eq!(
        declaration.media_type,
        "application/vnd.uta.technique-evidence+json;version=1"
    );
}

#[test]
fn enabled_cleanup_is_part_of_the_exact_analyzer_source_route() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request.requested_artifacts = crate::contract::RequestedArtifactsV1 {
        vocal_chart: false,
        pitch_evidence: true,
        singing_analysis: false,
        transcript: false,
        alignment: false,
        stems: Vec::new(),
    };
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        pitch_workflow_with_enabled_cleanup(),
    );

    let requirements = Planner::requirements(&request).unwrap();
    let resources = resource_ids(&requirements);
    assert!(resources.contains("model:melband_roformer_denoise_aufr33"));
    assert!(resources.contains("model:melband_roformer_dereverb_anvuew"));
    let plan = Planner::plan(&request, None).unwrap();
    assert_eq!(
        plan.source_route
            .preparation
            .iter()
            .map(CapabilityId::as_str)
            .collect::<Vec<_>>(),
        ["audio.extract_vocals", "audio.denoise", "audio.dereverb"]
    );
    assert_eq!(
        plan.execution_nodes
            .iter()
            .find(|node| node.capability.as_str() == "audio.dereverb")
            .unwrap()
            .depends_on,
        ["denoise"]
    );
}

#[test]
fn satisfied_cleanup_capabilities_are_skipped_even_though_the_workflow_enables_them() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request.requested_artifacts = crate::contract::RequestedArtifactsV1 {
        vocal_chart: false,
        pitch_evidence: true,
        singing_analysis: false,
        transcript: false,
        alignment: false,
        stems: Vec::new(),
    };
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        pitch_workflow_with_enabled_cleanup(),
    );
    request.satisfied_capabilities =
        vec!["audio.denoise".to_string(), "audio.dereverb".to_string()];

    let requirements = Planner::requirements(&request).unwrap();
    let resources = resource_ids(&requirements);
    assert!(!resources.contains("model:melband_roformer_denoise_aufr33"));
    assert!(!resources.contains("model:melband_roformer_dereverb_anvuew"));

    let plan = Planner::plan(&request, None).unwrap();
    assert!(!has_node(&plan, "audio.denoise"));
    assert!(!has_node(&plan, "audio.dereverb"));
    assert_eq!(
        plan.source_route
            .preparation
            .iter()
            .map(CapabilityId::as_str)
            .collect::<Vec<_>>(),
        ["audio.extract_vocals"]
    );
}

#[test]
fn satisfying_only_denoise_still_requests_dereverb() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request.requested_artifacts = crate::contract::RequestedArtifactsV1 {
        vocal_chart: false,
        pitch_evidence: true,
        singing_analysis: false,
        transcript: false,
        alignment: false,
        stems: Vec::new(),
    };
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        pitch_workflow_with_enabled_cleanup(),
    );
    request.satisfied_capabilities = vec!["audio.denoise".to_string()];

    let plan = Planner::plan(&request, None).unwrap();
    assert!(!has_node(&plan, "audio.denoise"));
    assert!(has_node(&plan, "audio.dereverb"));
    assert_eq!(
        plan.execution_nodes
            .iter()
            .find(|node| node.capability.as_str() == "audio.dereverb")
            .unwrap()
            .depends_on,
        ["extract-vocals"]
    );
}

#[test]
fn satisfied_extract_instrumental_is_skipped_even_when_requested() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request
        .requested_artifacts
        .stems
        .push(AudioRole::Instrumental);
    request.satisfied_capabilities = vec!["audio.extract_instrumental".to_string()];

    let requirements = Planner::requirements(&request).unwrap();
    assert!(!resource_ids(&requirements).contains("model:bs_polarformer_public_instrumental"));
    let plan = Planner::plan(&request, None).unwrap();
    assert!(!has_node(&plan, "audio.extract_instrumental"));
}

#[test]
fn an_unrecognized_satisfied_capability_is_rejected() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request.satisfied_capabilities = vec!["audio.lead_isolate".to_string()];
    let error = request.validate().unwrap_err();
    assert_eq!(error.code, EngineErrorCode::InvalidContract);
}

#[test]
fn original_mix_default_bypasses_optional_preprocessing() {
    let request = valid_request(AudioRole::OriginalMix);
    let requirements = Planner::requirements(&request).unwrap();
    let resources = resource_ids(&requirements);
    for expected in [
        "tool:ffmpeg",
        "model:bs_roformer_leap_xe90_vocals",
        "model:qwen3_asr_1_7b",
        "model:qwen3_forced_aligner_0_6b",
        "model:rmvpe",
        "model:game",
    ] {
        assert!(resources.contains(expected), "{expected}");
    }
    for bypassed in [
        "model:melband_roformer_harmony",
        "model:melband_roformer_denoise_aufr33",
        "model:melband_roformer_dereverb_anvuew",
        "model:bs_polarformer_public_instrumental",
    ] {
        assert!(!resources.contains(bypassed), "{bypassed}");
    }
}

#[test]
fn clean_lead_does_not_require_roformer() {
    let request = valid_request(AudioRole::CleanLeadVocal);
    let requirements = Planner::requirements(&request).unwrap();
    assert!(
        requirements
            .resources
            .iter()
            .all(|resource| !resource.resource.contains("roformer"))
    );
}

#[test]
fn explicit_lead_stem_from_an_already_lead_source_reuses_the_declared_semantics() {
    for role in [AudioRole::LeadVocal, AudioRole::CleanLeadVocal] {
        let mut request = valid_request(role);
        request.requested_artifacts.stems.push(AudioRole::LeadVocal);
        let requirements = Planner::requirements(&request).unwrap();
        assert!(!resource_ids(&requirements).contains("model:melband_roformer_harmony"));
        let plan = Planner::plan(&request, None).unwrap();
        assert!(!has_node(&plan, "audio.lead_isolate"));
        assert!(
            !plan
                .source_route
                .preparation
                .iter()
                .any(|capability| capability.as_str() == "audio.lead_isolate")
        );
    }
}

#[test]
fn instrumental_is_required_only_when_requested() {
    let mut request = valid_request(AudioRole::OriginalMix);
    let without = Planner::requirements(&request).unwrap();
    assert!(!resource_ids(&without).contains("model:bs_polarformer_public_instrumental"));
    request
        .requested_artifacts
        .stems
        .push(AudioRole::Instrumental);
    let with = Planner::requirements(&request).unwrap();
    assert!(resource_ids(&with).contains("model:bs_polarformer_public_instrumental"));
}

#[test]
fn removed_single_provider_residual_workflow_is_rejected() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request.requested_artifacts = crate::contract::RequestedArtifactsV1 {
        vocal_chart: false,
        pitch_evidence: true,
        singing_analysis: false,
        transcript: false,
        alignment: false,
        stems: vec![AudioRole::Instrumental],
    };
    let mut workflow = pitch_only_workflow_with_lead_isolation_policy("disabled");
    workflow["nodes"][1]["provider_preferences"]["instrumental"] =
        serde_json::json!("bs_roformer_leap_xe90_vocals");
    workflow["nodes"][1]["execution_invocations"] = serde_json::json!([{
        "invocation_id": "split",
        "provider_id": "bs_roformer_leap_xe90_vocals",
        "capabilities": ["audio.extract_vocals", "audio.extract_instrumental"],
        "output_ports": ["vocal", "instrumental"]
    }]);
    workflow["terminal_outputs"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "node": "split",
            "port": "instrumental",
            "semantic_type": "audio",
            "audio_role": "instrumental"
        }));
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        workflow,
    );

    assert!(Planner::requirements(&request).is_err());
}

#[test]
fn balanced_challengers_are_optional_and_fast_omits_them() {
    let fast = valid_request(AudioRole::LeadVocal);
    let fast_requirements = Planner::requirements(&fast).unwrap();
    for model in ["model:fcpe", "model:basic_pitch", "model:firered_asr2_aed"] {
        assert!(!resource_ids(&fast_requirements).contains(model));
    }
    let mut balanced = fast;
    balanced.analysis.profile = AnalysisProfile::Balanced;
    let plan = Planner::plan(&balanced, None).unwrap();
    for (model, capability) in [
        ("model:fcpe", "pitch.secondary.fcpe"),
        ("model:basic_pitch", "notes.basic_pitch"),
        ("model:firered_asr2_aed", "speech.transcribe.challenger"),
    ] {
        let requirement = plan
            .requirements
            .resources
            .iter()
            .find(|resource| resource.resource == model)
            .unwrap();
        assert!(!requirement.required);
        assert!(
            plan.optional_capabilities
                .iter()
                .any(|optional| optional.as_str() == capability)
        );
        assert!(
            plan.execution_nodes
                .iter()
                .any(|node| node.capability.as_str() == capability),
            "optional expert must have a real Engine execution/consumption node: {capability}"
        );
    }
}

#[test]
fn quality_gate_claims_follow_the_exact_executable_audio_route() {
    let fast = Planner::plan(&valid_request(AudioRole::OriginalMix), None).unwrap();
    assert_eq!(
        fast.quality_gates,
        [
            TIMELINE_VALID_GATE,
            FINITE_SAMPLES_GATE,
            CLIPPING_GATE,
            SILENCE_RATIO_GATE,
            ENERGY_RATIO_GATE,
        ]
    );

    let mut balanced_request = valid_request(AudioRole::OriginalMix);
    balanced_request.analysis.profile = AnalysisProfile::Balanced;
    let balanced = Planner::plan(&balanced_request, None).unwrap();
    assert_eq!(
        balanced.quality_gates,
        [
            TIMELINE_VALID_GATE,
            FINITE_SAMPLES_GATE,
            CLIPPING_GATE,
            SILENCE_RATIO_GATE,
            ENERGY_RATIO_GATE,
        ]
    );

    let mut isolated_nodes = balanced.execution_nodes.clone();
    isolated_nodes.push(ExecutionNode {
        id: "lead-isolate".to_string(),
        capability: CapabilityId::from_static("audio.lead_isolate"),
        required: true,
        depends_on: vec!["extract-vocals".to_string()],
    });
    let isolated_gates = quality_gates(AnalysisProfile::Balanced, &isolated_nodes);
    assert!(isolated_gates.contains(&LEAD_PURITY_GATE.to_string()));
    assert!(isolated_gates.contains(&VOCAL_TOPOLOGY_GATE.to_string()));

    let mut clean_pitch = valid_request(AudioRole::CleanLeadVocal);
    clean_pitch.analysis.profile = AnalysisProfile::Balanced;
    clean_pitch.requested_artifacts.vocal_chart = false;
    clean_pitch.requested_artifacts.singing_analysis = false;
    clean_pitch.requested_artifacts.transcript = false;
    clean_pitch.requested_artifacts.alignment = false;
    let clean_plan = Planner::plan(&clean_pitch, None).unwrap();
    assert_eq!(
        clean_plan.quality_gates,
        [
            TIMELINE_VALID_GATE,
            FINITE_SAMPLES_GATE,
            CLIPPING_GATE,
            SILENCE_RATIO_GATE,
            ENERGY_RATIO_GATE,
        ]
    );

    let mut instrumental_request = valid_request(AudioRole::OriginalMix);
    instrumental_request.requested_artifacts.vocal_chart = false;
    instrumental_request.requested_artifacts.pitch_evidence = false;
    instrumental_request.requested_artifacts.singing_analysis = false;
    instrumental_request.requested_artifacts.transcript = false;
    instrumental_request.requested_artifacts.alignment = false;
    instrumental_request
        .requested_artifacts
        .stems
        .push(AudioRole::Instrumental);
    let instrumental = Planner::plan(&instrumental_request, None).unwrap();
    assert!(
        instrumental
            .quality_gates
            .contains(&VOCAL_LEAKAGE_GATE.to_string())
    );
    assert!(
        instrumental
            .quality_gates
            .contains(&MUSICAL_DAMAGE_GATE.to_string())
    );
}

#[test]
fn canonical_lyrics_do_not_request_firered_challenger() {
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.analysis.profile = AnalysisProfile::Balanced;
    request.lyrics.mode = LyricsMode::Canonical;
    request.lyrics.tokens = vec![crate::contract::LyricTokenV1 {
        id: "token-1".to_string(),
        text: "唱".to_string(),
        reading: None,
        phonemes: None,
        start: None,
        end: None,
    }];
    let requirements = Planner::requirements(&request).unwrap();
    assert!(
        !resource_ids(&requirements).contains("model:firered_asr2_aed"),
        "optional ASR must not override canonical caller lyrics"
    );
}

#[test]
fn optional_experts_cannot_substitute_for_required_primaries() {
    let mut request = valid_request(AudioRole::LeadVocal);
    request.analysis.profile = AnalysisProfile::Balanced;
    let requirements = Planner::requirements(&request).unwrap();
    let resources = resource_ids(&requirements);
    for (primary, challenger) in [
        ("model:rmvpe", "model:fcpe"),
        ("model:game", "model:basic_pitch"),
    ] {
        let required = requirements
            .resources
            .iter()
            .find(|item| item.resource == primary)
            .unwrap();
        let optional = requirements
            .resources
            .iter()
            .find(|item| item.resource == challenger)
            .unwrap();
        assert!(required.required, "{primary}");
        assert!(!optional.required, "{challenger}");
        assert!(resources.contains(primary));
        assert!(resources.contains(challenger));
    }
}

#[test]
fn guide_vocals_only_does_not_request_lead_isolation() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request.requested_artifacts = crate::contract::RequestedArtifactsV1 {
        vocal_chart: false,
        pitch_evidence: false,
        singing_analysis: false,
        transcript: false,
        alignment: false,
        stems: vec![AudioRole::GuideVocals],
    };
    let requirements = Planner::requirements(&request).unwrap();
    let resources = resource_ids(&requirements);
    assert!(resources.contains("model:bs_roformer_leap_xe90_vocals"));
    assert!(!resources.contains("model:melband_roformer_harmony"));
}

#[test]
fn instrumental_branch_does_not_depend_on_lead_isolation() {
    let mut request = valid_request(AudioRole::OriginalMix);
    request
        .requested_artifacts
        .stems
        .push(AudioRole::Instrumental);
    let plan = Planner::plan(&request, None).unwrap();
    let instrumental = plan
        .execution_nodes
        .iter()
        .find(|node| node.id == "extract-instrumental")
        .unwrap();
    assert_eq!(instrumental.depends_on, ["decode"]);
    assert_eq!(plan.requested_outputs.last().unwrap(), "stem:instrumental");
}

#[test]
fn unsupported_backing_and_harmony_stems_fail_closed() {
    for role in [AudioRole::BackingVocal, AudioRole::HarmonyVocal] {
        let mut request = valid_request(AudioRole::LeadVocal);
        request.requested_artifacts.stems = vec![role];
        let error = Planner::requirements(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::MissingCapability);
        assert_eq!(error.capability.as_deref(), Some("audio.lead_partition"));
    }
}

#[test]
fn canonical_lyrics_alignment_does_not_require_asr() {
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.lyrics.mode = LyricsMode::Canonical;
    request.lyrics.tokens = vec![crate::contract::LyricTokenV1 {
        id: "token-1".to_string(),
        text: "sing".to_string(),
        reading: None,
        phonemes: None,
        start: None,
        end: None,
    }];
    request.requested_artifacts.vocal_chart = false;
    request.requested_artifacts.pitch_evidence = false;
    request.requested_artifacts.singing_analysis = false;
    request.requested_artifacts.transcript = false;
    request.requested_artifacts.alignment = true;
    let requirements = Planner::requirements(&request).unwrap();
    let resources = resource_ids(&requirements);
    assert!(!resources.contains("model:qwen3_asr_1_7b"));
    assert!(resources.contains("model:qwen3_forced_aligner_0_6b"));
}

#[test]
fn canonical_transcript_preserves_caller_identity_without_asr() {
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    request.lyrics.mode = LyricsMode::Canonical;
    request.lyrics.tokens = vec![crate::contract::LyricTokenV1 {
        id: "token-1".to_string(),
        text: "唱".to_string(),
        reading: None,
        phonemes: None,
        start: None,
        end: None,
    }];
    request.requested_artifacts.vocal_chart = false;
    request.requested_artifacts.pitch_evidence = false;
    request.requested_artifacts.singing_analysis = false;
    request.requested_artifacts.transcript = true;
    request.requested_artifacts.alignment = false;
    let plan = Planner::plan(&request, None).unwrap();
    assert!(
        !plan
            .requirements
            .resources
            .iter()
            .any(|item| item.resource == "model:qwen3_asr_1_7b")
    );
    assert!(has_node(&plan, "fusion.transcript"));
    assert!(!has_node(&plan, "speech.transcribe"));
}

#[test]
fn singing_analysis_runs_candidate_graph_without_unrequested_chart_finalizer() {
    let mut request = valid_request(AudioRole::LeadVocal);
    request.requested_artifacts.vocal_chart = false;
    request.requested_artifacts.pitch_evidence = false;
    request.requested_artifacts.transcript = false;
    request.requested_artifacts.alignment = false;
    request.requested_artifacts.singing_analysis = true;
    let plan = Planner::plan(&request, None).unwrap();
    assert!(has_node(&plan, "fusion.singing"));
    assert!(has_node(&plan, "fusion.candidate_graph"));
    assert!(!has_node(&plan, "finalize.vocal_chart"));
}

#[test]
fn quantization_is_ordered_between_candidate_graph_and_chart_finalization() {
    let mut request = valid_request(AudioRole::LeadVocal);
    request.analysis.enable_quantization = true;
    request.musical_context = Some(crate::contract::MusicalContextV1 {
        bpm: Some(120.0),
        key: None,
        time_signature: Some(crate::contract::TimeSignatureV1 { beats: 4, unit: 4 }),
        quantization_grid: Some(crate::contract::QuantizationGridV1::Sixteenth),
        authority: crate::contract::ContextAuthority::Hint,
    });
    let plan = Planner::plan(&request, None).unwrap();
    let index = |capability: &str| {
        plan.execution_nodes
            .iter()
            .position(|node| node.capability.as_str() == capability)
            .unwrap()
    };
    assert!(index("fusion.candidate_graph") < index("rhythm.quantize"));
    assert!(index("rhythm.quantize") < index("finalize.vocal_chart"));
    let quantize = plan
        .execution_nodes
        .iter()
        .find(|node| node.capability.as_str() == "rhythm.quantize")
        .unwrap();
    assert_eq!(quantize.depends_on, ["candidate-graph"]);
    assert_eq!(
        plan.execution_nodes
            .iter()
            .find(|node| node.capability.as_str() == "finalize.vocal_chart")
            .unwrap()
            .depends_on,
        ["rhythm-quantize"]
    );

    request.analysis.enable_quantization = false;
    request.musical_context = None;
    let disabled = Planner::plan(&request, None).unwrap();
    assert!(!has_node(&disabled, "rhythm.quantize"));
}

#[test]
fn full_candidate_reports_no_unwired_required_capability() {
    let request = valid_request(AudioRole::OriginalMix);
    let plan = Planner::plan(&request, None).unwrap();
    let registry = capability_registry()
        .into_iter()
        .map(|capability| (capability.id, capability.implementation_exists))
        .collect::<BTreeMap<_, _>>();
    let missing = plan
        .required_capabilities
        .iter()
        .filter(|capability| !registry.get(*capability).copied().unwrap_or(false))
        .map(CapabilityId::as_str)
        .collect::<Vec<_>>();
    assert!(missing.is_empty());
    Planner::ensure_required_capabilities(&plan).unwrap();
}

#[test]
fn per_model_backend_override_is_forwarded_without_changing_other_models() {
    let manager = RuntimeManager::new(
        uta_runtime_manager::ResourceCatalog::default_catalog().unwrap(),
        uta_runtime_manager::StorePaths::default(),
    );
    let mut request = valid_request(AudioRole::OriginalMix);
    request.execution_policy.model_backend_overrides.insert(
        "bs_roformer_leap_xe90_vocals".to_string(),
        uta_runtime_manager::NativeBackend::Vulkan,
    );
    let plan = Planner::plan(&request, Some(&manager)).unwrap();
    let backend = |id: &str| {
        plan.resolved_resources
            .iter()
            .find(|item| item.requirement.resource == format!("model:{id}"))
            .and_then(|item| item.status.as_ref())
            .and_then(|status| status.selected_backend)
    };
    assert_eq!(
        backend("bs_roformer_leap_xe90_vocals"),
        Some(uta_runtime_manager::NativeBackend::Vulkan)
    );
    assert_eq!(
        backend("rmvpe"),
        Some(uta_runtime_manager::NativeBackend::Vulkan)
    );
    assert_eq!(
        backend("qwen3_asr_1_7b"),
        Some(uta_runtime_manager::NativeBackend::Vulkan)
    );
}

#[test]
fn runtime_policy_is_forwarded_to_candidate_resolution() {
    let manager = RuntimeManager::new(
        uta_runtime_manager::ResourceCatalog::default_catalog().unwrap(),
        uta_runtime_manager::StorePaths::default(),
    );
    let mut production = valid_request(AudioRole::LeadVocal);
    production.execution_policy.runtime_policy = uta_runtime_manager::RuntimePolicy::Production;
    production.analysis.profile = AnalysisProfile::Balanced;
    let production_plan = Planner::plan(&production, Some(&manager)).unwrap();
    let production_fcpe = production_plan
        .resolved_resources
        .iter()
        .find(|item| item.requirement.resource == "model:fcpe")
        .unwrap()
        .status
        .as_ref()
        .unwrap();
    assert_eq!(
        production_fcpe.selected_backend,
        Some(uta_runtime_manager::NativeBackend::NativeDsp)
    );
    assert!(
        !production_fcpe.usable,
        "Production admission must not hide a missing installation"
    );

    let mut benchmark = production;
    benchmark.execution_policy.runtime_policy = uta_runtime_manager::RuntimePolicy::Benchmark;
    let benchmark_plan = Planner::plan(&benchmark, Some(&manager)).unwrap();
    let benchmark_fcpe = benchmark_plan
        .resolved_resources
        .iter()
        .find(|item| item.requirement.resource == "model:fcpe")
        .unwrap()
        .status
        .as_ref()
        .unwrap();
    assert_eq!(
        benchmark_fcpe.selected_backend,
        Some(uta_runtime_manager::NativeBackend::NativeDsp)
    );
    assert!(
        !benchmark_fcpe.usable,
        "an absent model must remain unusable"
    );
}
