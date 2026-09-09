use crate::contract::{
    AnalysisProfile, AnalyzeRequestV1, AudioRole, CLEANUP_CONSISTENCY_GATE, CLIPPING_GATE,
    CapabilityDescriptor, CapabilityId, ENERGY_RATIO_GATE, EngineError, EngineErrorCode,
    EngineRequirementResourceV1, EngineRequirementsV1, EngineResult, FINITE_SAMPLES_GATE,
    LEAD_PURITY_GATE, LyricsMode, MUSICAL_DAMAGE_GATE, SILENCE_RATIO_GATE, TIMELINE_VALID_GATE,
    VOCAL_LEAKAGE_GATE, VOCAL_TOPOLOGY_GATE, capability_registry,
};
use crate::workflow::{FusionModeV1, WorkflowExecutionPolicyV1, WorkflowExecutionV1};
use crate::workflow_executor::CompiledWorkflowExecutionPlanV1;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uta_runtime_manager::{ResourceRef, ResourceStatus, RuntimeManager};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnginePlan {
    pub schema: String,
    pub schema_version: u32,
    pub request_id: String,
    pub source_route: SourceRoute,
    pub requested_outputs: Vec<String>,
    pub required_capabilities: Vec<CapabilityId>,
    pub optional_capabilities: Vec<CapabilityId>,
    pub requirements: EngineRequirementsV1,
    pub resolved_resources: Vec<PlannedResourceStatus>,
    pub execution_nodes: Vec<ExecutionNode>,
    pub quality_gates: Vec<String>,
    pub fallback_policy: Vec<FallbackRule>,
    pub artifact_declarations: Vec<ArtifactDeclaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_execution: Option<CompiledWorkflowExecutionPlanV1>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRoute {
    pub primary_source_id: String,
    pub input_role: AudioRole,
    pub preparation: Vec<CapabilityId>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionNode {
    pub id: String,
    pub capability: CapabilityId,
    pub required: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedResourceStatus {
    pub requirement: EngineRequirementResourceV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ResourceStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_error: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackRule {
    pub capability: CapabilityId,
    pub behavior: String,
    pub fingerprinted: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDeclaration {
    pub semantic_type: String,
    pub required: bool,
    pub media_type: String,
}

#[derive(Debug, Default)]
struct RequirementAccumulator(BTreeMap<String, (bool, BTreeSet<String>)>);

impl RequirementAccumulator {
    fn add(&mut self, model: &str, required: bool, reason: &str) {
        self.add_resource(&format!("model:{model}"), required, reason);
    }

    fn add_resource(&mut self, resource: &str, required: bool, reason: &str) {
        let entry = self
            .0
            .entry(resource.to_string())
            .or_insert_with(|| (false, BTreeSet::new()));
        entry.0 |= required;
        entry.1.insert(reason.to_string());
    }

    fn finish(self) -> EngineRequirementsV1 {
        EngineRequirementsV1::new(
            self.0
                .into_iter()
                .map(
                    |(resource, (required, reasons))| EngineRequirementResourceV1 {
                        resource,
                        required,
                        reason: reasons.into_iter().collect::<Vec<_>>().join(","),
                    },
                )
                .collect(),
        )
    }
}

pub struct Planner;

impl Planner {
    pub fn requirements(request: &AnalyzeRequestV1) -> EngineResult<EngineRequirementsV1> {
        request.validate()?;
        let intent = AnalysisIntent::from_request(request);
        let workflow = WorkflowExecutionV1::from_request(request)?;
        let acoustic_required = workflow.as_ref().is_some_and(|workflow| {
            workflow.nodes.iter().any(|node| {
                node.capability_id == "analysis.acoustic_dsp"
                    && node.execution_policy == WorkflowExecutionPolicyV1::Always
            })
        });
        let primary_role = request.primary_source()?.role;
        let source_supports_lead_isolation = matches!(
            primary_role,
            AudioRole::OriginalMix | AudioRole::VocalStem | AudioRole::GuideVocals
        );
        let run_lead_isolate = source_supports_lead_isolation
            && (intent.requests_lead_stem
                || workflow_selects(
                    workflow.as_ref(),
                    "audio.lead_isolate",
                    request.analysis.profile,
                    false,
                ));
        let mut requirements = RequirementAccumulator::default();
        requirements.add_resource("tool:ffmpeg", true, "audio.decode");
        require_workflow_baseline(workflow.as_ref(), "audio.decode", request)?;
        if intent.needs_notes
            && workflow
                .as_ref()
                .is_some_and(|workflow| workflow.fusion_mode() == FusionModeV1::AiJudgment)
        {
            requirements.add_resource(
                "tool:fusion_agent_adapter",
                true,
                "fusion.candidate_graph / ai_judgment",
            );
        }

        match primary_role {
            AudioRole::OriginalMix => {
                if intent.needs_vocal_path {
                    require_workflow_baseline(workflow.as_ref(), "audio.extract_vocals", request)?;
                    let provider = workflow
                        .as_ref()
                        .and_then(|workflow| {
                            workflow.model_for_engine_capability("audio.extract_vocals")
                        })
                        .unwrap_or("bs_roformer_leap_xe90_vocals");
                    requirements.add(provider, true, "audio.extract_vocals");
                }
                if (intent.needs_vocal_analysis_input || intent.requests_lead_stem)
                    && run_lead_isolate
                {
                    requirements.add("melband_roformer_harmony", true, "audio.lead_isolate");
                    require_workflow_baseline(workflow.as_ref(), "audio.lead_isolate", request)?;
                }
            }
            AudioRole::VocalStem | AudioRole::GuideVocals => {
                if (intent.needs_vocal_analysis_input || intent.requests_lead_stem)
                    && run_lead_isolate
                {
                    requirements.add("melband_roformer_harmony", true, "audio.lead_isolate");
                    require_workflow_baseline(workflow.as_ref(), "audio.lead_isolate", request)?;
                }
            }
            AudioRole::LeadVocal | AudioRole::CleanLeadVocal => {}
            AudioRole::Instrumental | AudioRole::BackingVocal | AudioRole::HarmonyVocal => {
                unreachable!("request validation rejects reference-only primary roles")
            }
        }

        if intent.requests_instrumental
            && !capability_satisfied(request, "audio.extract_instrumental")
        {
            require_workflow_baseline(workflow.as_ref(), "audio.extract_instrumental", request)?;
            let provider = workflow
                .as_ref()
                .and_then(|workflow| {
                    workflow.model_for_engine_capability("audio.extract_instrumental")
                })
                .unwrap_or("bs_roformer_leap_xe90_vocals");
            requirements.add(provider, true, "audio.extract_instrumental");
        }
        if intent.needs_transcript {
            requirements.add("qwen3_asr_1_7b", true, "speech.transcribe");
            require_workflow_baseline(workflow.as_ref(), "speech.transcribe", request)?;
            if workflow.as_ref().is_some_and(|workflow| {
                workflow.should_schedule_model("firered_asr2_aed", request.analysis.profile)
            }) {
                requirements.add("firered_asr2_aed", false, "speech.transcribe.challenger");
            }
        }
        if intent.needs_alignment {
            requirements.add("qwen3_forced_aligner_0_6b", true, "speech.align");
            require_workflow_baseline(workflow.as_ref(), "speech.align", request)?;
        }
        if intent.needs_pitch {
            requirements.add("rmvpe", true, "pitch.track");
            require_workflow_baseline(workflow.as_ref(), "pitch.track", request)?;
            if workflow.as_ref().is_some_and(|workflow| {
                workflow.should_schedule_model("fcpe", request.analysis.profile)
            }) {
                requirements.add("fcpe", false, "pitch.secondary.fcpe");
            }
        }
        if intent.needs_notes {
            let note_experts =
                scheduled_core_note_experts(workflow.as_ref(), request.analysis.profile);
            if note_experts
                .iter()
                .filter(|(_, _, capability)| *capability == "notes.game")
                .count()
                > 1
            {
                return Err(EngineError::new(
                    EngineErrorCode::InvalidContract,
                    "workflow must select exactly one immutable GAME resource",
                )
                .with_capability("notes.game")
                .for_request(&request.request_id));
            }
            if note_experts
                .iter()
                .any(|(_, model_id, _)| *model_id == "jbm555_cectc_80")
                && !request
                    .audio_sources
                    .iter()
                    .any(|source| source.role == AudioRole::OriginalMix)
            {
                return Err(EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "JBM555 requires an original mix in addition to the prepared vocal input",
                )
                .with_capability("notes.jbm555")
                .for_request(&request.request_id));
            }
            for (_, model_id, capability) in note_experts.into_iter().chain(
                scheduled_advanced_experts(workflow.as_ref(), request.analysis.profile),
            ) {
                requirements.add(model_id, true, capability);
            }
        }
        if intent.needs_notes && acoustic_required {
            require_workflow_baseline(workflow.as_ref(), "analysis.acoustic_dsp", request)?;
        }
        if intent.needs_vocal_analysis_input
            && !capability_satisfied(request, "audio.denoise")
            && workflow_selects(
                workflow.as_ref(),
                "audio.denoise",
                request.analysis.profile,
                false,
            )
        {
            requirements.add("melband_roformer_denoise_aufr33", false, "audio.denoise");
        }
        if intent.needs_vocal_analysis_input
            && !capability_satisfied(request, "audio.dereverb")
            && workflow_selects(
                workflow.as_ref(),
                "audio.dereverb",
                request.analysis.profile,
                false,
            )
        {
            requirements.add("melband_roformer_dereverb_anvuew", false, "audio.dereverb");
        }
        if request
            .requested_artifacts
            .stems
            .iter()
            .any(|role| matches!(role, AudioRole::BackingVocal | AudioRole::HarmonyVocal))
        {
            return Err(EngineError::new(
                EngineErrorCode::MissingCapability,
                "backing/harmony stem extraction is future capability work, not Engine v1",
            )
            .with_capability("audio.lead_partition"));
        }
        let requirements = requirements.finish();
        requirements.validate()?;
        Ok(requirements)
    }

    pub fn plan(
        request: &AnalyzeRequestV1,
        runtime_manager: Option<&RuntimeManager>,
    ) -> EngineResult<EnginePlan> {
        request.validate()?;
        let requirements = Self::requirements(request)?;
        let intent = AnalysisIntent::from_request(request);
        let workflow = WorkflowExecutionV1::from_request(request)?;
        let acoustic_required = workflow.as_ref().is_some_and(|workflow| {
            workflow.nodes.iter().any(|node| {
                node.capability_id == "analysis.acoustic_dsp"
                    && node.execution_policy == WorkflowExecutionPolicyV1::Always
            })
        });
        let primary_role = request.primary_source()?.role;
        let source_supports_lead_isolation = matches!(
            primary_role,
            AudioRole::OriginalMix | AudioRole::VocalStem | AudioRole::GuideVocals
        );
        let analyze_with_lead_isolate = source_supports_lead_isolation
            && (workflow_selects(
                workflow.as_ref(),
                "audio.lead_isolate",
                request.analysis.profile,
                false,
            ) || (intent.requests_lead_stem
                && workflow.as_ref().is_some_and(|workflow| {
                    workflow.policy_for_engine_capability("audio.lead_isolate")
                        != Some(WorkflowExecutionPolicyV1::Disabled)
                })));
        let run_lead_isolate = source_supports_lead_isolation
            && (intent.requests_lead_stem || analyze_with_lead_isolate);
        if intent.needs_transcript
            || (request.lyrics.mode == LyricsMode::Canonical
                && (request.requested_artifacts.transcript || intent.needs_alignment))
        {
            require_workflow_baseline(workflow.as_ref(), "fusion.transcript", request)?;
        }
        if intent.needs_notes {
            for capability in ["fusion.singing", "fusion.candidate_graph"] {
                require_workflow_baseline(workflow.as_ref(), capability, request)?;
            }
            if request.requested_artifacts.vocal_chart {
                require_workflow_baseline(workflow.as_ref(), "finalize.vocal_chart", request)?;
            }
        }
        let has_model = |model_id: &str| {
            requirements
                .resources
                .iter()
                .any(|resource| resource.resource == format!("model:{model_id}"))
        };
        let run_fcpe = has_model("fcpe");
        let run_firered = has_model("firered_asr2_aed");
        let run_note_experts =
            scheduled_core_note_experts(workflow.as_ref(), request.analysis.profile)
                .into_iter()
                .filter(|(_, model_id, _)| has_model(model_id))
                .collect::<Vec<_>>();
        let run_advanced_experts =
            scheduled_advanced_experts(workflow.as_ref(), request.analysis.profile)
                .into_iter()
                .filter(|(_, model_id, _)| has_model(model_id))
                .collect::<Vec<_>>();
        let run_acoustic = intent.needs_notes
            && workflow_selects(
                workflow.as_ref(),
                "analysis.acoustic_dsp",
                request.analysis.profile,
                true,
            );
        let run_denoise = has_model("melband_roformer_denoise_aufr33");
        let run_dereverb = has_model("melband_roformer_dereverb_anvuew");
        let run_extract_instrumental = intent.requests_instrumental
            && !capability_satisfied(request, "audio.extract_instrumental");
        let preparation = preparation_capabilities(
            primary_role,
            &intent,
            analyze_with_lead_isolate,
            run_denoise,
            run_dereverb,
            run_extract_instrumental,
        );
        let mut nodes = vec![node("decode", "audio.decode", true, &[])];
        let mut analysis_parent = append_preparation_nodes(
            &mut nodes,
            primary_role,
            &intent,
            run_lead_isolate,
            analyze_with_lead_isolate,
            run_extract_instrumental,
        );
        if run_denoise {
            nodes.push(node("denoise", "audio.denoise", false, &[&analysis_parent]));
            analysis_parent = "denoise".to_string();
        }
        if run_dereverb {
            nodes.push(node(
                "dereverb",
                "audio.dereverb",
                false,
                &[&analysis_parent],
            ));
            analysis_parent = "dereverb".to_string();
        }
        if intent.needs_transcript {
            nodes.push(node(
                "qwen-asr",
                "speech.transcribe",
                true,
                &[&analysis_parent],
            ));
            let mut transcript_dependencies = vec!["qwen-asr"];
            if run_firered {
                nodes.push(node(
                    "firered-asr",
                    "speech.transcribe.challenger",
                    false,
                    &[&analysis_parent],
                ));
                transcript_dependencies.push("firered-asr");
            }
            nodes.push(node(
                "transcript",
                "fusion.transcript",
                true,
                &transcript_dependencies,
            ));
        } else if request.lyrics.mode == LyricsMode::Canonical
            && (request.requested_artifacts.transcript || intent.needs_alignment)
        {
            nodes.push(node("transcript", "fusion.transcript", true, &[]));
        }
        if intent.needs_alignment {
            nodes.push(node(
                "qwen-aligner",
                "speech.align",
                true,
                &[&analysis_parent, "transcript"],
            ));
            nodes.push(node(
                "alignment",
                "fusion.alignment",
                true,
                &["qwen-aligner"],
            ));
        }
        if intent.needs_pitch {
            nodes.push(node("pitch", "pitch.track", true, &[&analysis_parent]));
            if run_fcpe {
                nodes.push(node(
                    "pitch-fcpe",
                    "pitch.secondary.fcpe",
                    false,
                    &[&analysis_parent],
                ));
            }
        }
        if intent.needs_notes {
            if run_acoustic {
                nodes.push(node(
                    "acoustic-dsp",
                    "analysis.acoustic_dsp",
                    acoustic_required,
                    &[&analysis_parent],
                ));
            }
            for (node_id, _, capability) in &run_note_experts {
                nodes.push(node(node_id, capability, true, &[&analysis_parent]));
            }
            for (node_id, _, capability) in &run_advanced_experts {
                let expert_dependencies = if *capability == "technique.analyze"
                    && run_advanced_experts
                        .iter()
                        .any(|(_, _, item)| *item == "notes.stars")
                {
                    vec!["stars"]
                } else {
                    vec![analysis_parent.as_str(), "pitch", "alignment"]
                };
                nodes.push(node(node_id, capability, true, &expert_dependencies));
            }
            let mut dependencies = Vec::new();
            if intent.needs_pitch {
                dependencies.push("pitch");
            }
            if run_fcpe {
                dependencies.push("pitch-fcpe");
            }
            if intent.needs_alignment {
                dependencies.push("alignment");
            }
            if run_acoustic {
                dependencies.push("acoustic-dsp");
            }
            dependencies.extend(run_note_experts.iter().map(|(node_id, _, _)| *node_id));
            dependencies.extend(run_advanced_experts.iter().map(|(node_id, _, _)| *node_id));
            nodes.push(node(
                "singing-fusion",
                "fusion.singing",
                true,
                &dependencies,
            ));
            nodes.push(node(
                "candidate-graph",
                "fusion.candidate_graph",
                true,
                &["singing-fusion"],
            ));
            let finalization_parent = if request.analysis.enable_quantization {
                nodes.push(node(
                    "rhythm-quantize",
                    "rhythm.quantize",
                    true,
                    &["candidate-graph"],
                ));
                "rhythm-quantize"
            } else {
                "candidate-graph"
            };
            if request.requested_artifacts.vocal_chart {
                nodes.push(node(
                    "vocal-chart",
                    "finalize.vocal_chart",
                    true,
                    &[finalization_parent],
                ));
            }
        }

        let requested_workflow_capabilities = nodes
            .iter()
            .map(|node| node.capability.as_str().to_string())
            .collect::<BTreeSet<_>>();
        let workflow_execution = workflow
            .as_ref()
            .map(|workflow| {
                CompiledWorkflowExecutionPlanV1::compile(
                    workflow,
                    request.analysis.profile,
                    Some(&requested_workflow_capabilities),
                    intent.requests_lead_stem && source_supports_lead_isolation,
                )
            })
            .transpose()?;

        let required_capabilities = nodes
            .iter()
            .filter(|node| node.required)
            .map(|node| node.capability.clone())
            .collect();
        let optional_capabilities = nodes
            .iter()
            .filter(|node| !node.required)
            .map(|node| node.capability.clone())
            .collect();

        let resolved_resources = requirements
            .resources
            .iter()
            .cloned()
            .map(|requirement| {
                let Some(manager) = runtime_manager else {
                    return PlannedResourceStatus {
                        requirement,
                        status: None,
                        resolution_error: None,
                    };
                };
                let resource = requirement.resource.parse::<ResourceRef>();
                match resource.and_then(|resource| {
                    let requested_backend = (resource.kind
                        == uta_runtime_manager::ResourceKind::Model)
                        .then(|| request.execution_policy.requested_backend_for(&resource.id))
                        .flatten();
                    manager.status_with_backend(
                        &resource,
                        request.execution_policy.runtime_policy,
                        requested_backend,
                    )
                }) {
                    Ok(status) => PlannedResourceStatus {
                        requirement,
                        status: Some(status),
                        resolution_error: None,
                    },
                    Err(error) => PlannedResourceStatus {
                        requirement,
                        status: None,
                        resolution_error: Some(error.to_string()),
                    },
                }
            })
            .collect();

        let quality_gates = quality_gates(request.analysis.profile, &nodes);
        let artifact_declarations = artifact_declarations(request, &nodes);
        Ok(EnginePlan {
            schema: "uta.analysis-engine.plan".to_string(),
            schema_version: 1,
            request_id: request.request_id.clone(),
            source_route: SourceRoute {
                primary_source_id: request.primary_source()?.id.clone(),
                input_role: primary_role,
                preparation,
            },
            requested_outputs: requested_outputs(request),
            required_capabilities,
            optional_capabilities,
            requirements,
            resolved_resources,
            execution_nodes: nodes,
            quality_gates,
            fallback_policy: fallback_policy(),
            artifact_declarations,
            workflow_execution,
        })
    }

    pub fn capabilities(
        manager: Option<&RuntimeManager>,
        policy: uta_runtime_manager::RuntimePolicy,
    ) -> Vec<CapabilityDescriptor> {
        let mut readiness_by_resource = BTreeMap::new();
        capability_registry()
            .into_iter()
            .map(|mut capability| {
                let resource = if capability.id.as_str() == "audio.decode" {
                    ResourceRef::tool("ffmpeg").ok()
                } else {
                    model_for_capability(capability.id.as_str())
                        .and_then(|model| ResourceRef::model(model).ok())
                };
                if let Some(resource) = resource {
                    capability.runtime_policy_satisfied = *readiness_by_resource
                        .entry(resource.clone())
                        .or_insert_with(|| {
                            manager.is_some_and(|manager| {
                                manager
                                    .status(&resource, policy)
                                    .is_ok_and(|status| status.usable)
                            })
                        });
                }
                capability
            })
            .collect()
    }

    pub fn ensure_required_capabilities(plan: &EnginePlan) -> EngineResult<()> {
        let registry = capability_registry()
            .into_iter()
            .map(|capability| (capability.id.clone(), capability))
            .collect::<BTreeMap<_, _>>();
        for capability in &plan.required_capabilities {
            if registry
                .get(capability)
                .is_none_or(|descriptor| !descriptor.implementation_exists)
            {
                return Err(EngineError::new(
                    EngineErrorCode::MissingCapability,
                    format!("required capability is not implemented: {capability}"),
                )
                .with_capability(capability.to_string()));
            }
        }
        Ok(())
    }
}

fn require_workflow_baseline(
    workflow: Option<&WorkflowExecutionV1>,
    capability: &str,
    request: &AnalyzeRequestV1,
) -> EngineResult<()> {
    let Some(workflow) = workflow else {
        return Ok(());
    };
    let policy = workflow.policy_for_engine_capability(capability);
    if capability == "audio.lead_isolate"
        && request
            .requested_artifacts
            .stems
            .contains(&AudioRole::LeadVocal)
        && policy.is_some()
    {
        return Ok(());
    }
    match policy {
        Some(WorkflowExecutionPolicyV1::Always) => Ok(()),
        Some(policy) => Err(EngineError::new(
            EngineErrorCode::MissingCapability,
            format!(
                "required workflow baseline {capability} must use always policy, got {policy:?}"
            ),
        )
        .with_capability(capability)
        .for_request(&request.request_id)),
        None => Err(EngineError::new(
            EngineErrorCode::MissingCapability,
            format!("compiled workflow does not provide required baseline {capability}"),
        )
        .with_capability(capability)
        .for_request(&request.request_id)),
    }
}

/// True when the caller has asserted (via `satisfied_capabilities`) that the
/// supplied primary audio source already reflects this capability's output,
/// so it must not be re-requested this run. See the field's doc comment in
/// `contract::request` for why this exists alongside `primary_role`.
fn capability_satisfied(request: &AnalyzeRequestV1, capability: &str) -> bool {
    request
        .satisfied_capabilities
        .iter()
        .any(|satisfied| satisfied == capability)
}

fn workflow_selects(
    workflow: Option<&WorkflowExecutionV1>,
    capability: &str,
    profile: AnalysisProfile,
    default_when_workflow_absent: bool,
) -> bool {
    workflow.map_or(default_when_workflow_absent, |workflow| {
        workflow.policy_for_engine_capability(capability).is_some()
            && workflow.should_schedule(capability, profile)
    })
}

#[derive(Debug)]
struct AnalysisIntent {
    needs_vocal_analysis_input: bool,
    needs_vocal_path: bool,
    needs_transcript: bool,
    needs_alignment: bool,
    needs_pitch: bool,
    needs_notes: bool,
    requests_instrumental: bool,
    requests_lead_stem: bool,
}

impl AnalysisIntent {
    fn from_request(request: &AnalyzeRequestV1) -> Self {
        let outputs = &request.requested_artifacts;
        let needs_notes = outputs.vocal_chart || outputs.singing_analysis;
        let needs_pitch = outputs.pitch_evidence || needs_notes;
        let needs_alignment = outputs.alignment || needs_notes;
        let needs_transcript =
            request.lyrics.mode != LyricsMode::Canonical && (outputs.transcript || needs_alignment);
        let needs_vocal_analysis_input =
            needs_transcript || needs_alignment || needs_pitch || needs_notes;
        let requests_lead_stem = outputs.stems.contains(&AudioRole::LeadVocal);
        let needs_vocal_path = needs_vocal_analysis_input
            || requests_lead_stem
            || outputs.stems.contains(&AudioRole::GuideVocals)
            || outputs.stems.contains(&AudioRole::VocalStem);
        Self {
            needs_vocal_analysis_input,
            needs_vocal_path,
            needs_transcript,
            needs_alignment,
            needs_pitch,
            needs_notes,
            requests_instrumental: outputs.stems.contains(&AudioRole::Instrumental),
            requests_lead_stem,
        }
    }
}

fn preparation_capabilities(
    role: AudioRole,
    intent: &AnalysisIntent,
    analyze_with_lead_isolate: bool,
    run_denoise: bool,
    run_dereverb: bool,
    run_extract_instrumental: bool,
) -> Vec<CapabilityId> {
    let mut result = Vec::new();
    match role {
        AudioRole::OriginalMix => {
            if intent.needs_vocal_path {
                result.push(CapabilityId::from_static("audio.extract_vocals"));
            }
            if intent.needs_vocal_analysis_input && analyze_with_lead_isolate {
                result.push(CapabilityId::from_static("audio.lead_isolate"));
            }
            if run_extract_instrumental {
                result.push(CapabilityId::from_static("audio.extract_instrumental"));
            }
        }
        AudioRole::VocalStem | AudioRole::GuideVocals => {
            if intent.needs_vocal_analysis_input && analyze_with_lead_isolate {
                result.push(CapabilityId::from_static("audio.lead_isolate"));
            }
        }
        AudioRole::LeadVocal | AudioRole::CleanLeadVocal => {}
        AudioRole::Instrumental | AudioRole::BackingVocal | AudioRole::HarmonyVocal => {}
    }
    if run_denoise {
        result.push(CapabilityId::from_static("audio.denoise"));
    }
    if run_dereverb {
        result.push(CapabilityId::from_static("audio.dereverb"));
    }
    result
}

fn append_preparation_nodes(
    nodes: &mut Vec<ExecutionNode>,
    role: AudioRole,
    intent: &AnalysisIntent,
    run_lead_isolate: bool,
    analyze_with_lead_isolate: bool,
    run_extract_instrumental: bool,
) -> String {
    let mut analysis_parent = "decode".to_string();
    match role {
        AudioRole::OriginalMix => {
            if intent.needs_vocal_path {
                nodes.push(node(
                    "extract-vocals",
                    "audio.extract_vocals",
                    true,
                    &["decode"],
                ));
                analysis_parent = "extract-vocals".to_string();
            }
            if (intent.needs_vocal_analysis_input || intent.requests_lead_stem) && run_lead_isolate
            {
                nodes.push(node(
                    "lead-isolate",
                    "audio.lead_isolate",
                    true,
                    &["extract-vocals"],
                ));
                if analyze_with_lead_isolate {
                    analysis_parent = "lead-isolate".to_string();
                }
            }
            if run_extract_instrumental {
                // Production instrumental extraction is independent from the
                // analysis-lead branch and always consumes the decoded mix.
                nodes.push(node(
                    "extract-instrumental",
                    "audio.extract_instrumental",
                    true,
                    &["decode"],
                ));
            }
        }
        AudioRole::VocalStem | AudioRole::GuideVocals => {
            if (intent.needs_vocal_analysis_input || intent.requests_lead_stem) && run_lead_isolate
            {
                nodes.push(node(
                    "lead-isolate",
                    "audio.lead_isolate",
                    true,
                    &["decode"],
                ));
                if analyze_with_lead_isolate {
                    analysis_parent = "lead-isolate".to_string();
                }
            }
        }
        AudioRole::LeadVocal | AudioRole::CleanLeadVocal => {}
        AudioRole::Instrumental | AudioRole::BackingVocal | AudioRole::HarmonyVocal => {
            unreachable!("request validation rejects reference-only primary roles")
        }
    }
    analysis_parent
}

fn node(
    id: &str,
    capability: &'static str,
    required: bool,
    dependencies: &[&str],
) -> ExecutionNode {
    ExecutionNode {
        id: id.to_string(),
        capability: CapabilityId::from_static(capability),
        required,
        depends_on: dependencies
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
    }
}

fn model_for_capability(capability: &str) -> Option<&'static str> {
    match capability {
        "audio.extract_vocals" => Some("bs_roformer_leap_xe90_vocals"),
        "audio.extract_instrumental" => Some("bs_polarformer_public_instrumental"),
        "audio.lead_isolate" => Some("melband_roformer_harmony"),
        "audio.denoise" => Some("melband_roformer_denoise_aufr33"),
        "audio.dereverb" => Some("melband_roformer_dereverb_anvuew"),
        "pitch.track" | "pitch.secondary.rmvpe" => Some("rmvpe"),
        "pitch.secondary" | "pitch.secondary.fcpe" => Some("fcpe"),
        "speech.transcribe" => Some("qwen3_asr_1_7b"),
        "speech.align" => Some("qwen3_forced_aligner_0_6b"),
        "notes.game" => Some("game_1_0_3_medium"),
        "notes.basic_pitch" => Some("basic_pitch"),
        "notes.jbm555" => Some("jbm555_cectc_80"),
        "notes.stars" | "technique.analyze" => Some("stars"),
        "notes.rosvot" => Some("rosvot"),
        _ => None,
    }
}

fn scheduled_core_note_experts(
    workflow: Option<&WorkflowExecutionV1>,
    profile: AnalysisProfile,
) -> Vec<(&'static str, &'static str, &'static str)> {
    let Some(workflow) = workflow else {
        return Vec::new();
    };
    [
        ("basic-pitch", "basic_pitch", "notes.basic_pitch"),
        ("game-small", "game_1_0_3_small", "notes.game"),
        ("game-medium", "game_1_0_3_medium", "notes.game"),
        ("game-large", "game_1_0_3_large", "notes.game"),
        ("jbm555", "jbm555_cectc_80", "notes.jbm555"),
    ]
    .into_iter()
    .filter(|(_, model_id, _)| workflow.should_schedule_model(model_id, profile))
    .collect()
}

fn scheduled_advanced_experts(
    workflow: Option<&WorkflowExecutionV1>,
    profile: AnalysisProfile,
) -> Vec<(&'static str, &'static str, &'static str)> {
    let Some(workflow) = workflow else {
        return Vec::new();
    };
    [
        ("stars", "stars", "notes.stars"),
        ("stars-technique", "stars", "technique.analyze"),
        ("rosvot", "rosvot", "notes.rosvot"),
    ]
    .into_iter()
    .filter(|(_, model_id, capability)| {
        workflow.model_for_engine_capability(capability) == Some(*model_id)
            && workflow.should_schedule(capability, profile)
    })
    .collect()
}

fn requested_outputs(request: &AnalyzeRequestV1) -> Vec<String> {
    let outputs = &request.requested_artifacts;
    let mut result = Vec::new();
    for (requested, name) in [
        (outputs.vocal_chart, "candidate_vocal_chart"),
        (outputs.pitch_evidence, "pitch_evidence"),
        (outputs.singing_analysis, "singing_analysis"),
        (outputs.transcript, "transcript"),
        (outputs.alignment, "alignment"),
    ] {
        if requested {
            result.push(name.to_string());
        }
    }
    result.extend(
        outputs
            .stems
            .iter()
            .map(|role| format!("stem:{}", role.as_str())),
    );
    result
}

fn quality_gates(profile: AnalysisProfile, nodes: &[ExecutionNode]) -> Vec<String> {
    let mut gates = vec![
        TIMELINE_VALID_GATE.to_string(),
        FINITE_SAMPLES_GATE.to_string(),
        CLIPPING_GATE.to_string(),
        SILENCE_RATIO_GATE.to_string(),
        ENERGY_RATIO_GATE.to_string(),
    ];
    let has = |capability: &str| {
        nodes
            .iter()
            .any(|node| node.capability.as_str() == capability)
    };
    if has("audio.lead_isolate") {
        gates.push(LEAD_PURITY_GATE.to_string());
    }
    if has("audio.extract_instrumental") {
        gates.push(VOCAL_LEAKAGE_GATE.to_string());
        gates.push(MUSICAL_DAMAGE_GATE.to_string());
    }
    if profile != AnalysisProfile::Fast && (has("audio.denoise") || has("audio.dereverb")) {
        gates.push(CLEANUP_CONSISTENCY_GATE.to_string());
    }
    // Vocal topology needs the independent foreground/residual pair produced
    // by the selected lead-isolation route. A normal Vocal -> analyzers bypass
    // is valid workflow intent; absence of an unselected preprocessing node is
    // not an unknown/failed quality measurement.
    if has("fusion.singing") && has("audio.lead_isolate") {
        gates.push(VOCAL_TOPOLOGY_GATE.to_string());
    }
    gates
}

fn fallback_policy() -> Vec<FallbackRule> {
    vec![
        FallbackRule {
            capability: CapabilityId::from_static("audio.dereverb"),
            behavior: "continue_without_optional_cleanup_as_ok_degraded".to_string(),
            fingerprinted: true,
        },
        FallbackRule {
            capability: CapabilityId::from_static("pitch.secondary.fcpe"),
            behavior: "continue_without_optional_secondary_pitch_as_ok_degraded".to_string(),
            fingerprinted: true,
        },
    ]
}

fn artifact_declarations(
    request: &AnalyzeRequestV1,
    nodes: &[ExecutionNode],
) -> Vec<ArtifactDeclaration> {
    let mut declarations = requested_outputs(request)
        .into_iter()
        .map(|semantic_type| {
            let media_type = match semantic_type.as_str() {
                "candidate_vocal_chart" => "application/vnd.uta.vocal-chart+json;version=0.3",
                "pitch_evidence" => "application/vnd.uta.pitch-evidence+json;version=0.3",
                "singing_analysis" => "application/vnd.uta.singing-analysis+json;version=0.3",
                "transcript" => "application/vnd.uta.transcript+json;version=1",
                "alignment" => "application/vnd.uta.alignment+json;version=1",
                _ if semantic_type.starts_with("stem:") => "audio/flac",
                _ => "application/octet-stream",
            };
            ArtifactDeclaration {
                semantic_type,
                required: true,
                media_type: media_type.to_string(),
            }
        })
        .collect::<Vec<_>>();
    if nodes
        .iter()
        .any(|node| node.capability.as_str() == "technique.analyze")
    {
        declarations.push(ArtifactDeclaration {
            semantic_type: "technique_evidence".to_string(),
            required: false,
            media_type: "application/vnd.uta.technique-evidence+json;version=1".to_string(),
        });
    }
    declarations
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
