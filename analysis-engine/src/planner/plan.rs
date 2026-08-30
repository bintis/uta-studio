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
        let fusion_policy = workflow
            .as_ref()
            .and_then(|workflow| workflow.resolved_expert_fusion_policy(request.analysis.profile))
            .unwrap_or_default();
        let pitch_owner = fusion_policy.continuous_f0.model_id();
        let boundary_owner = fusion_policy.note_lengths.parameter_value();
        let basic_pitch_required = workflow.as_ref().is_some_and(|workflow| {
            workflow.policy_for_model("basic_pitch") == Some(WorkflowExecutionPolicyV1::Always)
        });
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
                        .unwrap_or("bs_roformer_vocals_ep317");
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

        if intent.requests_instrumental && !capability_satisfied(request, "audio.extract_instrumental")
        {
            require_workflow_baseline(workflow.as_ref(), "audio.extract_instrumental", request)?;
            let provider = workflow
                .as_ref()
                .and_then(|workflow| {
                    workflow.model_for_engine_capability("audio.extract_instrumental")
                })
                .unwrap_or("melband_roformer_inst_v2");
            requirements.add(provider, true, "audio.extract_instrumental");
        }
        if intent.needs_transcript {
            requirements.add("qwen3_asr_1_7b", true, "speech.transcribe");
            require_workflow_baseline(workflow.as_ref(), "speech.transcribe", request)?;
        }
        if intent.needs_alignment {
            requirements.add("qwen3_forced_aligner_0_6b", true, "speech.align");
            require_workflow_baseline(workflow.as_ref(), "speech.align", request)?;
        }
        if intent.needs_pitch {
            let primary_model = if pitch_owner == "fcpe" {
                "fcpe"
            } else {
                "rmvpe"
            };
            requirements.add(primary_model, true, "pitch.track");
            require_workflow_baseline(workflow.as_ref(), "pitch.track", request)?;
        }
        if intent.needs_notes
            && matches!(boundary_owner, "automatic" | "game")
            && workflow_selects(
                workflow.as_ref(),
                "notes.game",
                request.analysis.profile,
                true,
            )
        {
            requirements.add("game", true, "notes.game");
            require_workflow_baseline(workflow.as_ref(), "notes.game", request)?;
        } else if intent.needs_notes
            && workflow_selects(
                workflow.as_ref(),
                "notes.game",
                request.analysis.profile,
                true,
            )
        {
            requirements.add("game", false, "notes.game");
        }

        let secondary_pitch_model = if pitch_owner == "fcpe" {
            "rmvpe"
        } else {
            "fcpe"
        };
        if intent.needs_pitch
            && workflow_selects_model(
                workflow.as_ref(),
                secondary_pitch_model,
                request.analysis.profile,
                request.analysis.profile != AnalysisProfile::Fast,
            )
        {
            requirements.add(secondary_pitch_model, false, "pitch.secondary");
        }
        if intent.needs_notes
            && workflow_selects(
                workflow.as_ref(),
                "notes.basic_pitch",
                request.analysis.profile,
                request.analysis.profile != AnalysisProfile::Fast,
            )
        {
            requirements.add("basic_pitch", basic_pitch_required, "notes.basic_pitch");
            if basic_pitch_required {
                require_workflow_baseline(workflow.as_ref(), "notes.basic_pitch", request)?;
            }
        }
        if intent.needs_notes && acoustic_required {
            require_workflow_baseline(workflow.as_ref(), "analysis.acoustic_dsp", request)?;
        }
        if intent.needs_transcript
            && workflow_selects(
                workflow.as_ref(),
                "speech.transcribe.challenger",
                request.analysis.profile,
                request.analysis.profile != AnalysisProfile::Fast,
            )
        {
            requirements.add("firered_asr2_aed", false, "speech.transcribe.challenger");
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
        if intent.needs_notes
            && intent.needs_alignment
            && workflow_selects(
                workflow.as_ref(),
                "notes.rosvot",
                request.analysis.profile,
                request.analysis.profile == AnalysisProfile::Maximum,
            )
        {
            requirements.add("rosvot", false, "notes.rosvot");
        }
        if intent.needs_notes
            && intent.needs_alignment
            && matches!(request.lyrics.language.as_deref(), Some("zh" | "yue"))
            && workflow_selects(
                workflow.as_ref(),
                "notes.stars",
                request.analysis.profile,
                request.analysis.profile == AnalysisProfile::Maximum,
            )
        {
            requirements.add("stars", false, "notes.stars");
        }
        if intent.needs_alignment
            && matches!(request.lyrics.language.as_deref(), Some("zh" | "yue"))
            && workflow_selects(
                workflow.as_ref(),
                "technique.analyze",
                request.analysis.profile,
                false,
            )
        {
            requirements.add("stars", false, "technique.analyze");
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
        let fusion_policy = workflow
            .as_ref()
            .and_then(|workflow| workflow.resolved_expert_fusion_policy(request.analysis.profile))
            .unwrap_or_default();
        let pitch_owner = fusion_policy.continuous_f0.model_id();
        let boundary_owner = fusion_policy.note_lengths.parameter_value();
        let basic_pitch_required = workflow.as_ref().is_some_and(|workflow| {
            workflow.policy_for_model("basic_pitch") == Some(WorkflowExecutionPolicyV1::Always)
        });
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
        let run_firered = has_model("firered_asr2_aed");
        let secondary_pitch_model = if pitch_owner == "fcpe" {
            "rmvpe"
        } else {
            "fcpe"
        };
        let run_secondary_pitch = has_model(secondary_pitch_model);
        let secondary_pitch_capability = if secondary_pitch_model == "rmvpe" {
            "pitch.secondary.rmvpe"
        } else {
            "pitch.secondary.fcpe"
        };
        let run_basic_pitch = has_model("basic_pitch");
        let run_rosvot = has_model("rosvot");
        let run_stars = has_model("stars");
        let run_game = has_model("game");
        let run_acoustic = intent.needs_notes
            && workflow_selects(
                workflow.as_ref(),
                "analysis.acoustic_dsp",
                request.analysis.profile,
                true,
            );
        let run_stars_notes = run_stars
            && workflow_selects(
                workflow.as_ref(),
                "notes.stars",
                request.analysis.profile,
                request.analysis.profile == AnalysisProfile::Maximum,
            );
        let run_stars_technique = run_stars
            && workflow_selects(
                workflow.as_ref(),
                "technique.analyze",
                request.analysis.profile,
                false,
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
                "transcript-evidence",
                "speech.transcribe",
                true,
                &[&analysis_parent],
            ));
            let mut transcript_dependencies = vec!["transcript-evidence"];
            if run_firered {
                nodes.push(node(
                    "transcript-challenger",
                    "speech.transcribe.challenger",
                    false,
                    &[&analysis_parent, "transcript-evidence"],
                ));
                transcript_dependencies.push("transcript-challenger");
            }
            nodes.push(node(
                "transcript",
                "fusion.transcript",
                true,
                &transcript_dependencies,
            ));
        }
        if !intent.needs_transcript
            && request.lyrics.mode == LyricsMode::Canonical
            && (request.requested_artifacts.transcript || intent.needs_alignment)
        {
            nodes.push(node("transcript", "fusion.transcript", true, &[]));
        }
        if intent.needs_alignment {
            nodes.push(node(
                "alignment-evidence",
                "speech.align",
                true,
                &[&analysis_parent, "transcript"],
            ));
            nodes.push(node(
                "alignment",
                "fusion.alignment",
                true,
                &["alignment-evidence"],
            ));
        }
        if intent.needs_pitch {
            nodes.push(node("pitch", "pitch.track", true, &[&analysis_parent]));
            if run_secondary_pitch {
                nodes.push(node(
                    "pitch-secondary",
                    secondary_pitch_capability,
                    false,
                    &[&analysis_parent, "pitch"],
                ));
            }
        }
        if intent.needs_notes {
            if run_game {
                nodes.push(node(
                    "game",
                    "notes.game",
                    boundary_owner == "game",
                    &[&analysis_parent],
                ));
            }
            if run_basic_pitch {
                let mut basic_dependencies = vec![analysis_parent.as_str(), "pitch"];
                if run_game {
                    basic_dependencies.push("game");
                }
                nodes.push(node(
                    "basic-pitch",
                    "notes.basic_pitch",
                    basic_pitch_required,
                    &basic_dependencies,
                ));
            }
            if run_acoustic {
                nodes.push(node(
                    "acoustic-dsp",
                    "analysis.acoustic_dsp",
                    acoustic_required,
                    &[&analysis_parent],
                ));
            }
            if run_rosvot {
                let mut dependencies = vec![analysis_parent.as_str(), "alignment"];
                if run_game {
                    dependencies.push("game");
                }
                nodes.push(node("rosvot", "notes.rosvot", false, &dependencies));
            }
            if run_stars_notes {
                let mut dependencies = vec![analysis_parent.as_str(), "alignment"];
                if run_game {
                    dependencies.push("game");
                }
                nodes.push(node("stars", "notes.stars", false, &dependencies));
            }
            if run_stars_technique {
                nodes.push(node(
                    "stars-technique",
                    "technique.analyze",
                    false,
                    &[&analysis_parent, "alignment"],
                ));
            }
            let mut dependencies = Vec::new();
            if intent.needs_pitch {
                dependencies.push("pitch");
            }
            if intent.needs_alignment {
                dependencies.push("alignment");
            }
            if run_game {
                dependencies.push("game");
            }
            if run_acoustic {
                dependencies.push("acoustic-dsp");
            }
            if run_secondary_pitch {
                dependencies.push("pitch-secondary");
            }
            if run_basic_pitch {
                dependencies.push("basic-pitch");
            }
            if run_rosvot {
                dependencies.push("rosvot");
            }
            if run_stars_notes {
                dependencies.push("stars");
            }
            if run_stars_technique {
                dependencies.push("stars-technique");
            }
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

fn workflow_selects_model(
    workflow: Option<&WorkflowExecutionV1>,
    model_id: &str,
    profile: AnalysisProfile,
    default_when_workflow_absent: bool,
) -> bool {
    workflow.map_or(default_when_workflow_absent, |workflow| {
        workflow.should_schedule_model(model_id, profile)
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
        "audio.extract_vocals" => Some("bs_roformer_vocals_ep317"),
        "audio.extract_instrumental" => Some("melband_roformer_inst_v2"),
        "audio.lead_isolate" => Some("melband_roformer_harmony"),
        "audio.denoise" => Some("melband_roformer_denoise_aufr33"),
        "audio.dereverb" => Some("melband_roformer_dereverb_anvuew"),
        "speech.transcribe" => Some("qwen3_asr_1_7b"),
        "speech.transcribe.challenger" => Some("firered_asr2_aed"),
        "speech.align" => Some("qwen3_forced_aligner_0_6b"),
        "pitch.track" => Some("rmvpe"),
        "pitch.secondary" | "pitch.secondary.fcpe" => Some("fcpe"),
        "pitch.secondary.rmvpe" => Some("rmvpe"),
        "notes.game" => Some("game"),
        "notes.basic_pitch" => Some("basic_pitch"),
        "notes.rosvot" => Some("rosvot"),
        "notes.stars" | "technique.analyze" => Some("stars"),
        _ => None,
    }
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
            capability: CapabilityId::from_static("pitch.track"),
            behavior: "fail_without_explicit_validated_primary_f0_fallback".to_string(),
            fingerprinted: true,
        },
        FallbackRule {
            capability: CapabilityId::from_static("pitch.secondary"),
            behavior: "continue_with_primary_rmvpe_as_ok_degraded".to_string(),
            fingerprinted: true,
        },
        FallbackRule {
            capability: CapabilityId::from_static("notes.basic_pitch"),
            behavior: "continue_with_primary_game_as_ok_degraded".to_string(),
            fingerprinted: true,
        },
        FallbackRule {
            capability: CapabilityId::from_static("speech.transcribe.challenger"),
            behavior: "continue_without_optional_transcript_challenger_as_ok_degraded".to_string(),
            fingerprinted: true,
        },
        FallbackRule {
            capability: CapabilityId::from_static("audio.dereverb"),
            behavior: "continue_without_optional_cleanup_as_ok_degraded".to_string(),
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
mod tests {
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
            "workflow_schema_version": 2,
            "workflow_id": "song:test:workflow",
            "workflow_revision": 1,
            "quality_mode": "balanced",
            "definition_digest": "a".repeat(32),
            "nodes": [
                node("source", "audio.source", None, "always", 1000),
                node("split", "audio.separate_vocal_bgm", Some("bs_roformer_vocals_ep317"), "always", 900),
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
        assert!(resources.contains("model:bs_roformer_vocals_ep317"));
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
        request.satisfied_capabilities = vec!["audio.denoise".to_string(), "audio.dereverb".to_string()];

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
        request.requested_artifacts.stems.push(AudioRole::Instrumental);
        request.satisfied_capabilities = vec!["audio.extract_instrumental".to_string()];

        let requirements = Planner::requirements(&request).unwrap();
        assert!(!resource_ids(&requirements).contains("model:melband_roformer_inst_v2"));
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
            "model:bs_roformer_vocals_ep317",
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
            "model:melband_roformer_inst_v2",
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
        assert!(!resource_ids(&without).contains("model:melband_roformer_inst_v2"));
        request
            .requested_artifacts
            .stems
            .push(AudioRole::Instrumental);
        let with = Planner::requirements(&request).unwrap();
        assert!(resource_ids(&with).contains("model:melband_roformer_inst_v2"));
    }

    #[test]
    fn ep317_residual_workflow_resolves_one_provider_for_both_stem_roles() {
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
            serde_json::json!("bs_roformer_vocals_ep317");
        workflow["nodes"][1]["execution_invocations"] = serde_json::json!([{
            "invocation_id": "split",
            "provider_id": "bs_roformer_vocals_ep317",
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

        let requirements = Planner::requirements(&request).unwrap();
        let resources = resource_ids(&requirements);
        assert!(resources.contains("model:bs_roformer_vocals_ep317"));
        assert!(!resources.contains("model:melband_roformer_inst_v2"));
        let plan = Planner::plan(&request, None).unwrap();
        let split = plan
            .workflow_execution
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|node| node.instance_id == "split")
            .unwrap();
        assert_eq!(split.execution_invocations.len(), 1);
        assert_eq!(
            split.execution_invocations[0].capabilities,
            ["audio.extract_vocals", "audio.extract_instrumental"]
        );
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
        assert!(resources.contains("model:bs_roformer_vocals_ep317"));
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
            "bs_roformer_vocals_ep317".to_string(),
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
            backend("bs_roformer_vocals_ep317"),
            Some(uta_runtime_manager::NativeBackend::Vulkan)
        );
        assert_eq!(
            backend("rmvpe"),
            Some(uta_runtime_manager::NativeBackend::OpenVino)
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
            Some(uta_runtime_manager::NativeBackend::OpenVino)
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
            Some(uta_runtime_manager::NativeBackend::OpenVino)
        );
        assert!(
            !benchmark_fcpe.usable,
            "an absent model must remain unusable"
        );
    }
}
