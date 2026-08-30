use std::time::{SystemTime, UNIX_EPOCH};

use crate::library_db;

use serde::{Deserialize, Serialize};

use super::{
    NodeCapability, StoredWorkflow, WorkflowCompileError, WorkflowDefinition,
    WorkflowExecutionSnapshot, WorkflowLayout, builtin_capabilities, compile_workflow,
    default_workflow,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContinuousF0PolicyV1 {
    Rmvpe,
    Fcpe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BoundaryFusionPolicyV1 {
    Game,
    F0Derived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OnsetFusionPolicyV1 {
    Automatic,
    Acoustic,
    BasicPitch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FusionModeV1 {
    #[default]
    Algorithm,
    AiJudgment,
}

pub fn fusion_mode(definition: &WorkflowDefinition) -> FusionModeV1 {
    let Some(node) = definition
        .nodes
        .iter()
        .find(|node| node.instance_id.as_str() == "evidence_fusion")
    else {
        return FusionModeV1::default();
    };
    match node
        .parameters
        .get("fusion_mode")
        .and_then(serde_json::Value::as_str)
    {
        Some("ai") => FusionModeV1::AiJudgment,
        _ => FusionModeV1::Algorithm,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExpertFusionPolicyV1 {
    pub continuous_f0: ContinuousF0PolicyV1,
    pub note_lengths: BoundaryFusionPolicyV1,
    pub onset_support: OnsetFusionPolicyV1,
}

impl Default for ExpertFusionPolicyV1 {
    fn default() -> Self {
        Self {
            continuous_f0: ContinuousF0PolicyV1::Rmvpe,
            note_lengths: BoundaryFusionPolicyV1::Game,
            onset_support: OnsetFusionPolicyV1::Automatic,
        }
    }
}

/// Projects the Engine's deterministic fusion baseline from Stage 3 evidence
/// participation. This is descriptive/internal policy, never authored Step 4
/// ownership: the Engine may still construct and select challenger states.
pub(crate) fn expert_fusion_policy(
    definition: &WorkflowDefinition,
) -> Result<ExpertFusionPolicyV1, String> {
    if !definition
        .nodes
        .iter()
        .any(|node| node.instance_id.as_str() == "evidence_fusion")
    {
        return Err("workflow has no evidence fusion node".to_string());
    }
    let enabled = |model: &str| {
        definition.nodes.iter().any(|node| {
            node.model_id.as_deref() == Some(model) && !is_disabled(&node.execution_policy)
        })
    };
    let always = |model: &str| {
        definition.nodes.iter().any(|node| {
            node.model_id.as_deref() == Some(model)
                && matches!(node.execution_policy, super::ExecutionPolicy::Always)
        })
    };
    let continuous_f0 = if always("rmvpe") || (!always("fcpe") && enabled("rmvpe")) {
        ContinuousF0PolicyV1::Rmvpe
    } else if enabled("fcpe") {
        ContinuousF0PolicyV1::Fcpe
    } else {
        return Err("at least one continuous F0 expert must remain enabled".to_string());
    };
    Ok(ExpertFusionPolicyV1 {
        continuous_f0,
        note_lengths: if enabled("game") {
            BoundaryFusionPolicyV1::Game
        } else {
            BoundaryFusionPolicyV1::F0Derived
        },
        onset_support: OnsetFusionPolicyV1::Automatic,
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub fn list_workflow_capabilities() -> Vec<NodeCapability> {
    builtin_capabilities()
}

pub fn load_song_workflow(file_hash: &str) -> Result<StoredWorkflow, String> {
    if let Some((json, updated_at_ms)) =
        library_db::song_workflow_get(file_hash).map_err(|error| error.to_string())?
    {
        let mut stored: StoredWorkflow = serde_json::from_str(&json)
            .map_err(|error| format!("invalid saved workflow: {error}"))?;
        migrate_stored_workflow(&mut stored)?;
        stored.updated_at_ms = updated_at_ms;
        return Ok(stored);
    }
    Ok(StoredWorkflow {
        definition: default_workflow(file_hash),
        layout: WorkflowLayout::default(),
        updated_at_ms: 0,
    })
}

pub fn migrate_stored_workflow(stored: &mut StoredWorkflow) -> Result<(), String> {
    match stored.definition.schema_version {
        1 => {
            // Schema 1 serialized the ambiguous local role as `back_vocal`.
            // AudioRole's serde alias reads it as the explicit BackingVocal role;
            // schema 2 then writes `backing_vocal` and keeps HarmonyVocal distinct.
            stored.definition.schema_version = super::WORKFLOW_SCHEMA_VERSION;
        }
        version if version == super::WORKFLOW_SCHEMA_VERSION => {}
        version => {
            return Err(format!(
                "unsupported saved workflow schema version {version}"
            ));
        }
    }
    if let Some(separation) = stored
        .definition
        .nodes
        .iter_mut()
        .find(|node| node.capability_id.as_str() == "audio.separate_vocal_bgm")
    {
        if separation.separation_strategy.is_none() {
            let instrumental = separation
                .parameters
                .get("instrumental_model_id")
                .and_then(serde_json::Value::as_str);
            separation.separation_strategy = match (separation.model_id.as_deref(), instrumental) {
                (Some("bs_roformer_vocals_ep317"), Some("bs_roformer_vocals_ep317")) => {
                    Some(super::SeparationStrategyV1::Ep317VocalResidual)
                }
                (Some("bs_roformer_vocals_ep317"), Some("melband_roformer_inst_v2") | None) => {
                    Some(super::SeparationStrategyV1::IndependentSpecialists)
                }
                _ => {
                    return Err(
                        "legacy Vocal/BGM providers do not map to an executable typed strategy"
                            .to_string(),
                    );
                }
            };
        }
        separation.parameters.remove("instrumental_model_id");
    }
    if let Some(fusion) = stored
        .definition
        .nodes
        .iter_mut()
        .find(|node| node.instance_id.as_str() == "evidence_fusion")
    {
        // Step 4 now owns only the final selector. Legacy owner parameters
        // never rewrite Stage 3 expert participation during migration.
        fusion.parameters.remove("pitch_owner");
        fusion.parameters.remove("boundary_owner");
        fusion.parameters.remove("onset_owner");
        fusion
            .parameters
            .entry("fusion_mode".to_string())
            .or_insert_with(|| serde_json::Value::String("algorithm".to_string()));
    }
    Ok(())
}

pub fn save_song_workflow(
    file_hash: &str,
    mut definition: WorkflowDefinition,
    layout: WorkflowLayout,
) -> Result<StoredWorkflow, String> {
    compile_workflow(&definition).map_err(|error| error.to_string())?;
    let existing_revision = library_db::song_workflow_get(file_hash)
        .map_err(|error| error.to_string())?
        .and_then(|(json, _)| serde_json::from_str::<StoredWorkflow>(&json).ok())
        .map(|stored| stored.definition.revision)
        .unwrap_or(0);
    definition.revision = existing_revision.saturating_add(1);
    let stored = StoredWorkflow {
        definition,
        layout,
        updated_at_ms: now_ms(),
    };
    let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    library_db::song_workflow_set(file_hash, &json, stored.updated_at_ms)
        .map_err(|error| error.to_string())?;
    Ok(stored)
}

pub fn preview_workflow_compile(
    definition: &WorkflowDefinition,
) -> Result<WorkflowExecutionSnapshot, WorkflowCompileError> {
    compile_workflow(definition)
}

/// Reorders two adjacent role-preserving audio transformations by rewriting
/// semantic edges. Layout coordinates are never consulted.
pub fn reorder_audio_transformation(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
    earlier: bool,
) -> Result<(), String> {
    let original = definition.clone();
    let capabilities = builtin_capabilities();
    let role_preserving = definition
        .nodes
        .iter()
        .filter_map(|node| {
            capabilities
                .iter()
                .find(|capability| capability.id == node.capability_id)
                .map(|capability| (node.instance_id.clone(), capability.preserves_audio_role))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let selected = role_preserving
        .get(node_id)
        .copied()
        .ok_or_else(|| "workflow node not found".to_string())?;
    if !selected {
        return Err("only role-preserving audio transformations can be reordered".to_string());
    }

    let selected_incoming = definition
        .edges
        .iter()
        .position(|edge| &edge.to.node == node_id && edge.to.port == "audio")
        .ok_or_else(|| "selected transformation has no audio input".to_string())?;

    if earlier {
        let previous_id = definition.edges[selected_incoming].from.node.clone();
        if !role_preserving.get(&previous_id).copied().unwrap_or(false) {
            return Err("the preceding node is a fixed branch boundary".to_string());
        }
        let previous_incoming = definition
            .edges
            .iter()
            .position(|edge| edge.to.node == previous_id && edge.to.port == "audio")
            .ok_or_else(|| "preceding transformation has no audio input".to_string())?;
        let selected_outgoing = definition
            .edges
            .iter()
            .enumerate()
            .filter_map(|(index, edge)| {
                (edge.from.node == *node_id && edge.from.port == "audio").then_some(index)
            })
            .collect::<Vec<_>>();

        definition.edges[previous_incoming].to.node = node_id.clone();
        definition.edges[selected_incoming].from.node = node_id.clone();
        definition.edges[selected_incoming].to.node = previous_id.clone();
        for index in selected_outgoing {
            definition.edges[index].from.node = previous_id.clone();
        }
        for binding in &mut definition.analyzer_bindings {
            if binding.source.node == *node_id && binding.source.port == "audio" {
                binding.source.node = previous_id.clone();
            }
        }
    } else {
        let selected_outgoing = definition
            .edges
            .iter()
            .position(|edge| &edge.from.node == node_id && edge.from.port == "audio")
            .ok_or_else(|| "selected transformation has no audio output".to_string())?;
        let next_id = definition.edges[selected_outgoing].to.node.clone();
        if !role_preserving.get(&next_id).copied().unwrap_or(false) {
            return Err("the following node is a fixed branch boundary".to_string());
        }
        let next_outgoing = definition
            .edges
            .iter()
            .enumerate()
            .filter_map(|(index, edge)| {
                (edge.from.node == next_id && edge.from.port == "audio").then_some(index)
            })
            .collect::<Vec<_>>();

        definition.edges[selected_incoming].to.node = next_id.clone();
        definition.edges[selected_outgoing].from.node = next_id.clone();
        definition.edges[selected_outgoing].to.node = node_id.clone();
        for index in next_outgoing {
            definition.edges[index].from.node = node_id.clone();
        }
        for binding in &mut definition.analyzer_bindings {
            if binding.source.node == next_id && binding.source.port == "audio" {
                binding.source.node = node_id.clone();
            }
        }
    }
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(())
}

/// Inserts a repeatable role-preserving audio transformation after one concrete
/// audio output. Every downstream edge/binding that consumed that output is
/// redirected to the new node, so the edit changes persisted topology rather
/// than only card order.
pub fn insert_audio_transformation_after_output(
    definition: &mut WorkflowDefinition,
    source_node: &super::WorkflowNodeId,
    source_port: &str,
    capability_id: &super::CapabilityId,
    model_id: Option<String>,
) -> Result<super::WorkflowNodeId, String> {
    let original = definition.clone();
    let capabilities = builtin_capabilities();
    let capability = capabilities
        .iter()
        .find(|capability| &capability.id == capability_id)
        .ok_or_else(|| "workflow capability is unavailable".to_string())?;
    if !capability.preserves_audio_role || !capability.allows_multiple_instances {
        return Err(
            "only repeatable role-preserving audio transformations can be inserted".to_string(),
        );
    }
    let source_capability = definition
        .nodes
        .iter()
        .find(|node| &node.instance_id == source_node)
        .and_then(|node| {
            capabilities
                .iter()
                .find(|capability| capability.id == node.capability_id)
        })
        .ok_or_else(|| "workflow source node not found".to_string())?;
    if !source_capability
        .output(source_port)
        .is_some_and(|output| output.port_type.is_audio())
    {
        return Err("selected workflow output is not audio".to_string());
    }

    let stem = capability_id.as_str().replace('.', "_");
    let instance_id = (1..)
        .map(|suffix| super::WorkflowNodeId::new(format!("{stem}_{suffix}")))
        .find(|candidate| {
            definition
                .nodes
                .iter()
                .all(|node| node.instance_id != *candidate)
        })
        .expect("an unused workflow instance suffix exists");

    let mut redirected = false;
    for edge in &mut definition.edges {
        if edge.from.node == *source_node && edge.from.port == source_port {
            edge.from.node = instance_id.clone();
            edge.from.port = "audio".to_string();
            redirected = true;
        }
    }
    for binding in &mut definition.analyzer_bindings {
        if binding.source.node == *source_node && binding.source.port == source_port {
            binding.source.node = instance_id.clone();
            binding.source.port = "audio".to_string();
            redirected = true;
        }
    }

    definition.nodes.push(super::WorkflowNodeInstance {
        instance_id: instance_id.clone(),
        capability_id: capability_id.clone(),
        model_id,
        separation_strategy: None,
        parameters: std::collections::BTreeMap::new(),
        execution_policy: super::ExecutionPolicy::Always,
        priority: 800,
        skip_if_unchanged: false,
    });
    definition.edges.push(super::WorkflowEdge {
        from: super::WorkflowPortRef {
            node: source_node.clone(),
            port: source_port.to_string(),
        },
        to: super::WorkflowPortRef {
            node: instance_id.clone(),
            port: "audio".to_string(),
        },
    });

    // A terminal branch is also valid: in that case the new processor simply
    // becomes the new branch tail and can receive later processors.
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    let _ = redirected;
    Ok(instance_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalWorkflowCardV1 {
    FireRedTranscript,
    RmvpePitch,
    FcpePitch,
    GameBoundary,
    BasicPitchBoundary,
    RosvotBoundary,
    StarsBoundary,
    StarsTechnique,
    AcousticDsp,
}

impl OptionalWorkflowCardV1 {
    pub fn label(self) -> &'static str {
        match self {
            Self::FireRedTranscript => "FireRed transcript challenger",
            Self::RmvpePitch => "RMVPE continuous pitch",
            Self::FcpePitch => "FCPE continuous pitch",
            Self::GameBoundary => "GAME note regions",
            Self::BasicPitchBoundary => "Basic Pitch onset evidence",
            Self::RosvotBoundary => "ROSVOT note challenger",
            Self::StarsBoundary => "STARS note challenger",
            Self::StarsTechnique => "STARS technique evidence",
            Self::AcousticDsp => "Acoustic DSP evidence",
        }
    }

    fn spec(
        self,
    ) -> (
        &'static str,
        Option<&'static str>,
        &'static str,
        &'static str,
        &'static str,
        super::ExecutionPolicy,
        i32,
        &'static str,
    ) {
        use super::{ConditionalExecution, ExecutionPolicy};
        match self {
            Self::FireRedTranscript => (
                "analysis.asr",
                Some("firered_asr2_aed"),
                "transcript",
                "transcript_fusion",
                "evidence",
                ExecutionPolicy::Conditional {
                    condition: ConditionalExecution::OnDisagreement,
                },
                690,
                "asr_firered",
            ),
            Self::RmvpePitch => (
                "analysis.pitch_f0",
                Some("rmvpe"),
                "pitch",
                "evidence_fusion",
                "pitch",
                ExecutionPolicy::Conditional {
                    condition: ConditionalExecution::DisagreementWindows,
                },
                680,
                "f0_rmvpe",
            ),
            Self::FcpePitch => (
                "analysis.pitch_f0",
                Some("fcpe"),
                "pitch",
                "evidence_fusion",
                "pitch",
                ExecutionPolicy::Conditional {
                    condition: ConditionalExecution::DisagreementWindows,
                },
                670,
                "f0_fcpe",
            ),
            Self::GameBoundary => (
                "analysis.note_boundary",
                Some("game"),
                "boundaries",
                "evidence_fusion",
                "boundaries",
                ExecutionPolicy::Disabled,
                660,
                "boundary_game",
            ),
            Self::BasicPitchBoundary => (
                "analysis.note_boundary",
                Some("basic_pitch"),
                "boundaries",
                "evidence_fusion",
                "boundaries",
                ExecutionPolicy::Conditional {
                    condition: ConditionalExecution::OnDisagreement,
                },
                655,
                "boundary_basic_pitch",
            ),
            Self::RosvotBoundary => (
                "analysis.note_boundary",
                Some("rosvot"),
                "boundaries",
                "evidence_fusion",
                "boundaries",
                ExecutionPolicy::Conditional {
                    condition: ConditionalExecution::MaximumOnly,
                },
                645,
                "boundary_rosvot",
            ),
            Self::StarsBoundary => (
                "analysis.note_boundary",
                Some("stars"),
                "boundaries",
                "evidence_fusion",
                "boundaries",
                ExecutionPolicy::Conditional {
                    condition: ConditionalExecution::MaximumOnly,
                },
                640,
                "boundary_stars",
            ),
            Self::StarsTechnique => (
                "analysis.technique",
                Some("stars"),
                "techniques",
                "evidence_fusion",
                "techniques",
                ExecutionPolicy::Conditional {
                    condition: ConditionalExecution::MaximumOnly,
                },
                635,
                "technique_stars",
            ),
            Self::AcousticDsp => (
                "analysis.acoustic_dsp",
                None,
                "acoustic",
                "evidence_fusion",
                "acoustic",
                ExecutionPolicy::Always,
                650,
                "acoustic_dsp",
            ),
        }
    }
}

pub fn workflow_has_optional_card(
    definition: &WorkflowDefinition,
    card: OptionalWorkflowCardV1,
) -> bool {
    let (capability, model, ..) = card.spec();
    definition.nodes.iter().any(|node| {
        node.capability_id.as_str() == capability
            && match model {
                Some(model) => node.model_id.as_deref() == Some(model),
                None => node.model_id.is_none(),
            }
    })
}

/// Adds one product-approved optional analyzer card and wires it to the exact
/// analysis-audio tail and managed fusion input. This intentionally does not
/// expose an arbitrary raw graph-node constructor to the desktop.
pub fn add_optional_workflow_card(
    definition: &mut WorkflowDefinition,
    audio_source: super::WorkflowPortRef,
    card: OptionalWorkflowCardV1,
) -> Result<super::WorkflowNodeId, String> {
    if workflow_has_optional_card(definition, card) {
        return Err(format!("{} is already present", card.label()));
    }
    let original = definition.clone();
    let (capability, model, output_port, target_node, target_port, policy, priority, base_id) =
        card.spec();
    let capabilities = builtin_capabilities();
    let analyzer = capabilities
        .iter()
        .find(|item| item.id.as_str() == capability)
        .ok_or_else(|| format!("workflow capability {capability} is unavailable"))?;
    if analyzer.class != super::CapabilityClass::Analyzer {
        return Err("optional workflow card is not an analyzer capability".to_string());
    }
    let instance_id = (0..)
        .map(|suffix| {
            if suffix == 0 {
                super::WorkflowNodeId::new(base_id)
            } else {
                super::WorkflowNodeId::new(format!("{base_id}_{suffix}"))
            }
        })
        .find(|candidate| {
            definition
                .nodes
                .iter()
                .all(|node| node.instance_id != *candidate)
        })
        .expect("an unused optional workflow card id exists");
    definition.nodes.push(super::WorkflowNodeInstance {
        instance_id: instance_id.clone(),
        capability_id: super::CapabilityId::new(capability),
        model_id: model.map(str::to_string),
        separation_strategy: None,
        parameters: std::collections::BTreeMap::new(),
        execution_policy: policy,
        priority,
        skip_if_unchanged: false,
    });
    definition.analyzer_bindings.push(super::AnalyzerBinding {
        analyzer_node: instance_id.clone(),
        source: audio_source,
        analyzer_input: "audio".to_string(),
    });
    definition.edges.push(super::WorkflowEdge {
        from: super::WorkflowPortRef {
            node: instance_id.clone(),
            port: output_port.to_string(),
        },
        to: super::WorkflowPortRef {
            node: super::WorkflowNodeId::new(target_node),
            port: target_port.to_string(),
        },
    });
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(instance_id)
}

/// Inserts a second instance of a role-preserving transformation directly
/// after the selected instance. Downstream semantic audio edges and terminal
/// analyzer attachments move to the duplicate so the new instance is real.
pub fn duplicate_audio_transformation(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
) -> Result<super::WorkflowNodeId, String> {
    let original = definition.clone();
    let capabilities = builtin_capabilities();
    let source = definition
        .nodes
        .iter()
        .find(|node| &node.instance_id == node_id)
        .cloned()
        .ok_or_else(|| "workflow node not found".to_string())?;
    let capability = capabilities
        .iter()
        .find(|capability| capability.id == source.capability_id)
        .ok_or_else(|| "workflow capability is unavailable".to_string())?;
    if !capability.preserves_audio_role || !capability.allows_multiple_instances {
        return Err(
            "only repeatable role-preserving audio transformations can be duplicated".to_string(),
        );
    }
    let instance_id = (1..)
        .map(|suffix| super::WorkflowNodeId::new(format!("{}-copy-{suffix}", node_id.as_str())))
        .find(|candidate| {
            definition
                .nodes
                .iter()
                .all(|node| node.instance_id != *candidate)
        })
        .expect("an unused workflow instance suffix exists");
    let mut duplicate = source;
    duplicate.instance_id = instance_id.clone();
    duplicate.priority = duplicate.priority.saturating_sub(1);

    let mut redirected = false;
    for edge in &mut definition.edges {
        if edge.from.node == *node_id && edge.from.port == "audio" {
            edge.from.node = instance_id.clone();
            redirected = true;
        }
    }
    for binding in &mut definition.analyzer_bindings {
        if binding.source.node == *node_id && binding.source.port == "audio" {
            binding.source.node = instance_id.clone();
            redirected = true;
        }
    }
    if !redirected {
        return Err("selected transformation has no audio output".to_string());
    }
    definition.nodes.push(duplicate);
    definition.edges.push(super::WorkflowEdge {
        from: super::WorkflowPortRef {
            node: node_id.clone(),
            port: "audio".to_string(),
        },
        to: super::WorkflowPortRef {
            node: instance_id.clone(),
            port: "audio".to_string(),
        },
    });
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(instance_id)
}

/// Removes an optional workflow card while preserving the typed dataflow.
/// Role-preserving audio transformations are bypassed to their exact input;
/// optional analyzers lose their attachment and evidence edge. Required product
/// stages fail closed instead of being silently replaced.
pub fn remove_workflow_node(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
) -> Result<(), String> {
    let original = definition.clone();
    let capabilities = builtin_capabilities();
    let selected = definition
        .nodes
        .iter()
        .position(|node| &node.instance_id == node_id)
        .ok_or_else(|| "workflow node not found".to_string())?;
    let node = definition.nodes[selected].clone();
    let capability = capabilities
        .iter()
        .find(|capability| capability.id == node.capability_id)
        .ok_or_else(|| "workflow capability is unavailable".to_string())?;
    let removable_analyzer = capability.class == super::CapabilityClass::Analyzer
        && !node_disable_is_product_forbidden(&node);
    if !capability.preserves_audio_role && !removable_analyzer {
        return Err("This workflow stage is required and cannot be deleted.".to_string());
    }

    if capability.preserves_audio_role {
        let upstream = definition
            .edges
            .iter()
            .find(|edge| edge.to.node == *node_id && edge.to.port == "audio")
            .map(|edge| edge.from.clone())
            .ok_or_else(|| "selected transformation has no audio input".to_string())?;
        for edge in &mut definition.edges {
            if edge.from.node == *node_id && edge.from.port == "audio" {
                edge.from = upstream.clone();
            }
        }
        definition
            .edges
            .retain(|edge| edge.to.node != *node_id && edge.from.node != *node_id);
        for binding in &mut definition.analyzer_bindings {
            if binding.source.node == *node_id && binding.source.port == "audio" {
                binding.source = upstream.clone();
            }
        }
    } else {
        definition
            .edges
            .retain(|edge| edge.to.node != *node_id && edge.from.node != *node_id);
        definition
            .analyzer_bindings
            .retain(|binding| binding.analyzer_node != *node_id && binding.source.node != *node_id);
    }
    definition.nodes.remove(selected);

    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(())
}

/// Selects an Engine-v1-compatible provider for a model-selectable card.
/// If that provider already belongs to a sibling card, the two cards swap
/// providers so one physical expert is never represented twice accidentally.
pub fn set_workflow_separation_strategy(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
    strategy: super::SeparationStrategyV1,
) -> Result<(), String> {
    let original = definition.clone();
    let node = definition
        .nodes
        .iter_mut()
        .find(|node| &node.instance_id == node_id)
        .ok_or_else(|| "workflow node not found".to_string())?;
    if node.capability_id.as_str() != "audio.separate_vocal_bgm" {
        return Err("selected workflow card is not Vocal/BGM separation".to_string());
    }
    let descriptor = super::separation_strategy_descriptor(strategy);
    node.separation_strategy = Some(strategy);
    node.model_id = descriptor
        .executions
        .first()
        .map(|execution| execution.provider_id.to_string());
    node.parameters.remove("instrumental_model_id");
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(())
}

pub fn set_workflow_node_model(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
    model_id: &str,
) -> Result<(), String> {
    let original = definition.clone();
    let selected = definition
        .nodes
        .iter()
        .position(|node| &node.instance_id == node_id)
        .ok_or_else(|| "workflow node not found".to_string())?;
    let capability_id = definition.nodes[selected].capability_id.clone();
    let options = super::workflow_model_options(&capability_id);
    if !options.iter().any(|option| option.model_id == model_id) {
        return Err(format!(
            "model {model_id} is not selectable for capability {capability_id}"
        ));
    }
    let previous = definition.nodes[selected]
        .model_id
        .clone()
        .ok_or_else(|| "selected workflow card has no model provider".to_string())?;
    if previous == model_id {
        return Ok(());
    }

    let swapped = definition.nodes.iter().position(|node| {
        node.instance_id != *node_id
            && node.capability_id == capability_id
            && node.model_id.as_deref() == Some(model_id)
    });
    definition.nodes[selected].model_id = Some(model_id.to_string());
    if let Some(index) = swapped {
        definition.nodes[index].model_id = Some(previous.clone());
    }

    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(())
}

fn is_disabled(policy: &super::ExecutionPolicy) -> bool {
    matches!(policy, super::ExecutionPolicy::Disabled)
}

pub(crate) fn validate_expert_fusion_intent(definition: &WorkflowDefinition) -> Result<(), String> {
    expert_fusion_policy(definition).map(|_| ())
}

fn node_disable_is_product_forbidden(node: &super::WorkflowNodeInstance) -> bool {
    matches!(
        node.capability_id.as_str(),
        "audio.source"
            | "audio.separate_vocal_bgm"
            | "fusion.transcript"
            | "analysis.forced_alignment"
            | "fusion.singing_evidence"
            | "fusion.candidate_graph"
            | "finalize.canonical_singing_track"
    ) || (node.capability_id.as_str() == "analysis.asr"
        && node.model_id.as_deref() == Some("qwen3_asr_1_7b"))
}

pub fn set_workflow_execution_policy(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
    policy: super::ExecutionPolicy,
) -> Result<(), String> {
    let original = definition.clone();
    let selected = definition
        .nodes
        .iter()
        .position(|node| &node.instance_id == node_id)
        .ok_or_else(|| "workflow node not found".to_string())?;
    let capability_id = definition.nodes[selected].capability_id.to_string();

    if is_disabled(&policy) && node_disable_is_product_forbidden(&definition.nodes[selected]) {
        return Err(match capability_id.as_str() {
            "analysis.asr" | "fusion.transcript" | "analysis.forced_alignment" => {
                "Lyrics transcription and forced alignment are required workflow stages; choose their model/input behavior instead of disabling the stage."
                    .to_string()
            }
            "audio.separate_vocal_bgm" => {
                "Vocal and BGM are required workflow outputs and cannot be disabled."
                    .to_string()
            }
            _ => "This workflow stage is required and cannot be disabled.".to_string(),
        });
    }

    definition.nodes[selected].execution_policy = policy;

    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(())
}

pub fn set_workflow_priority(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
    priority: i32,
) -> Result<(), String> {
    let node = definition
        .nodes
        .iter_mut()
        .find(|node| &node.instance_id == node_id)
        .ok_or_else(|| "workflow node not found".to_string())?;
    node.priority = priority.clamp(-100, 100);
    Ok(())
}

/// Toggles a Step 1 audio-chain node's "skip if unchanged" cache opt-in.
/// Purely a UI-facing preference -- it has no DAG/topology implications, so
/// unlike `set_workflow_execution_policy` it never needs to recompile or
/// roll back the workflow.
pub fn set_workflow_skip_if_unchanged(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
    skip_if_unchanged: bool,
) -> Result<(), String> {
    let node = definition
        .nodes
        .iter_mut()
        .find(|node| &node.instance_id == node_id)
        .ok_or_else(|| "workflow node not found".to_string())?;
    node.skip_if_unchanged = skip_if_unchanged;
    Ok(())
}

pub fn bind_workflow_analyzer(
    definition: &mut WorkflowDefinition,
    analyzer_node: &super::WorkflowNodeId,
    source: super::WorkflowPortRef,
) -> Result<(), String> {
    let original = definition.clone();
    let binding = definition
        .analyzer_bindings
        .iter_mut()
        .find(|binding| &binding.analyzer_node == analyzer_node)
        .ok_or_else(|| "selected node is not an analyzer attachment".to_string())?;
    binding.source = source;
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(())
}

/// Updates one persisted node parameter and validates the resulting workflow.
/// This is the product-level configuration path for fusion ownership and other
/// model-independent workflow intent; it never mutates runtime-manager state.
pub fn set_workflow_parameter(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
    key: impl Into<String>,
    value: serde_json::Value,
) -> Result<(), String> {
    let original = definition.clone();
    let key = key.into();

    if node_id.as_str() == "evidence_fusion" {
        let selected = value.as_str();
        match key.as_str() {
            "pitch_owner" | "boundary_owner" | "onset_owner" => {
                return Err(format!(
                    "{key} is a legacy Engine-internal fusion detail; configure evidence participation in Stage 3"
                ));
            }
            "fusion_mode" => {
                let mode =
                    selected.ok_or_else(|| "fusion_mode must be algorithm or ai".to_string())?;
                if !matches!(mode, "algorithm" | "ai") {
                    return Err("fusion_mode must be algorithm or ai".to_string());
                }
            }
            _ => {}
        }
    }

    {
        let node = definition
            .nodes
            .iter_mut()
            .find(|node| &node.instance_id == node_id)
            .ok_or_else(|| "workflow node not found".to_string())?;
        node.parameters.insert(key, value);
    }
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(())
}
