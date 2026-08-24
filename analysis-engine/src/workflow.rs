//! Independently owned wire contract for compiled Processing Studio execution.
//!
//! app-core serializes a local DTO into `AnalyzeRequestV1.extensions`; this
//! module deliberately mirrors the JSON shape without importing app-core.
//! Validation therefore occurs after the CLI process boundary and cannot be
//! bypassed by a stale or hand-edited Studio snapshot.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::contract::{
    AnalysisProfile, AnalyzeRequestV1, AudioRole, EngineError, EngineErrorCode, EngineResult,
};

pub const WORKFLOW_EXECUTION_EXTENSION_KEY: &str = "uta.workflow_execution.v1";
pub const WORKFLOW_EXECUTION_CONTRACT: &str = "uta.workflow-execution";
pub const WORKFLOW_EXECUTION_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowExecutionPolicyV1 {
    Always,
    Disabled,
    MaximumOnly,
    OnDisagreement,
    DisagreementWindows,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecutionV1 {
    pub contract: String,
    pub version: u32,
    pub workflow_schema_version: u32,
    pub workflow_id: String,
    pub workflow_revision: u64,
    pub quality_mode: String,
    pub definition_digest: String,
    pub nodes: Vec<WorkflowNodeV1>,
    pub bindings: Vec<WorkflowBindingV1>,
    pub terminal_outputs: Vec<WorkflowTerminalOutputV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNodeV1 {
    pub instance_id: String,
    pub capability_id: String,
    pub analysis_node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub execution_policy: WorkflowExecutionPolicyV1,
    pub priority: i32,
    pub runtime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowBindingV1 {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
    pub semantic_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_role: Option<String>,
    pub execution_active: bool,
    pub analyzer_attachment: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTerminalOutputV1 {
    pub node: String,
    pub port: String,
    pub semantic_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_role: Option<String>,
}

impl WorkflowExecutionV1 {
    pub fn from_request(request: &AnalyzeRequestV1) -> EngineResult<Option<Self>> {
        let Some(value) = request.extensions.get(WORKFLOW_EXECUTION_EXTENSION_KEY) else {
            return Ok(None);
        };
        let workflow: Self = serde_json::from_value(value.clone()).map_err(|error| {
            invalid(format!(
                "compiled workflow extension is not valid v1 JSON: {error}"
            ))
            .for_request(&request.request_id)
        })?;
        workflow.validate(request)?;
        Ok(Some(workflow))
    }

    pub fn validate(&self, request: &AnalyzeRequestV1) -> EngineResult<()> {
        if self.contract != WORKFLOW_EXECUTION_CONTRACT
            || self.version != WORKFLOW_EXECUTION_VERSION
            || self.workflow_schema_version != 2
        {
            return Err(EngineError::new(
                EngineErrorCode::UnsupportedContractVersion,
                format!(
                    "expected {WORKFLOW_EXECUTION_CONTRACT}/{WORKFLOW_EXECUTION_VERSION} with workflow schema 2"
                ),
            )
            .for_request(&request.request_id));
        }
        validate_identifier(&self.workflow_id, "workflow_id")?;
        if self.workflow_revision == 0 {
            return Err(
                invalid("workflow_revision must be positive").for_request(&request.request_id)
            );
        }
        if !matches!(
            self.quality_mode.as_str(),
            "fast" | "balanced" | "maximum" | "custom"
        ) {
            return Err(
                invalid("workflow quality_mode is unknown").for_request(&request.request_id)
            );
        }
        if self.nodes.is_empty() || self.nodes.len() > 256 || self.bindings.len() > 2_048 {
            return Err(
                invalid("compiled workflow node or binding count is invalid")
                    .for_request(&request.request_id),
            );
        }

        let mut instances = BTreeSet::new();
        let mut analysis_nodes = BTreeSet::new();
        let mut nodes = BTreeMap::new();
        for node in &self.nodes {
            validate_identifier(&node.instance_id, "workflow node instance")?;
            validate_identifier(&node.analysis_node, "analysis node")?;
            if !instances.insert(node.instance_id.as_str())
                || !analysis_nodes.insert(node.analysis_node.as_str())
            {
                return Err(
                    invalid("compiled workflow contains duplicate node identity")
                        .for_request(&request.request_id),
                );
            }
            validate_node(node, request)?;
            nodes.insert(node.analysis_node.as_str(), node);
        }

        let mut edges = BTreeSet::new();
        for binding in &self.bindings {
            let from = nodes.get(binding.from_node.as_str()).ok_or_else(|| {
                invalid(format!(
                    "workflow binding references unknown producer {}",
                    binding.from_node
                ))
                .for_request(&request.request_id)
            })?;
            let to = nodes.get(binding.to_node.as_str()).ok_or_else(|| {
                invalid(format!(
                    "workflow binding references unknown consumer {}",
                    binding.to_node
                ))
                .for_request(&request.request_id)
            })?;
            let produced = port_contract(from, &binding.from_port, PortDirection::Output)
                .ok_or_else(|| {
                    invalid(format!(
                        "workflow producer {} has no compatible output port {}",
                        from.instance_id, binding.from_port
                    ))
                    .for_request(&request.request_id)
                })?;
            let consumed =
                port_contract(to, &binding.to_port, PortDirection::Input).ok_or_else(|| {
                    invalid(format!(
                        "workflow consumer {} has no compatible input port {}",
                        to.instance_id, binding.to_port
                    ))
                    .for_request(&request.request_id)
                })?;
            if binding.semantic_type != produced.semantic
                || binding.semantic_type != consumed.semantic
                || !known_semantic_type(&binding.semantic_type)
            {
                return Err(invalid(format!(
                    "workflow binding {}:{} -> {}:{} has an incompatible semantic type",
                    from.instance_id, binding.from_port, to.instance_id, binding.to_port
                ))
                .for_request(&request.request_id));
            }
            let expected_active = from.execution_policy != WorkflowExecutionPolicyV1::Disabled
                && to.execution_policy != WorkflowExecutionPolicyV1::Disabled;
            if binding.execution_active != expected_active {
                return Err(invalid(
                    "workflow binding active state disagrees with node execution policies",
                )
                .for_request(&request.request_id));
            }
            if binding.semantic_type == "audio" {
                let role = binding.audio_role.as_deref().ok_or_else(|| {
                    invalid("audio workflow binding is missing its semantic role")
                        .for_request(&request.request_id)
                })?;
                if !known_audio_role(role)
                    || produced.audio_role.is_some_and(|expected| expected != role)
                    || consumed.audio_role.is_some_and(|expected| expected != role)
                {
                    return Err(invalid(format!(
                        "workflow audio role {role} is incompatible with the declared ports"
                    ))
                    .for_request(&request.request_id));
                }
                if matches!(role, "back_vocal" | "backing_vocal" | "harmony_vocal") {
                    return Err(EngineError::new(
                        EngineErrorCode::MissingCapability,
                        "backing/harmony audio stems are future capability work; lead isolation provides only lead_vocal plus vocal_residual",
                    )
                    .with_capability("audio.lead_partition")
                    .for_request(&request.request_id));
                }
            } else if binding.audio_role.is_some() {
                return Err(invalid("non-audio workflow binding declares an audio role")
                    .for_request(&request.request_id));
            }
            if binding.analyzer_attachment
                && (binding.semantic_type != "audio"
                    || binding.audio_role.as_deref() != Some("lead_vocal"))
            {
                return Err(invalid(
                    "Engine v1 analyzers must bind to the compiled lead_vocal artifact",
                )
                .for_request(&request.request_id));
            }
            if binding.execution_active
                && !edges.insert((binding.from_node.as_str(), binding.to_node.as_str()))
            {
                // Multiple ports between the same nodes are valid, but only one graph edge
                // is needed for deterministic cycle validation.
            }
        }
        validate_required_inputs(self, &nodes, request)?;
        validate_acyclic(&nodes, &edges, request)?;
        self.validate_outputs(request, &nodes)?;
        Ok(())
    }

    pub fn policy_for_engine_capability(
        &self,
        capability: &str,
    ) -> Option<WorkflowExecutionPolicyV1> {
        self.nodes
            .iter()
            .filter_map(|node| {
                engine_capabilities(node)
                    .contains(&capability)
                    .then_some((node.priority, node.execution_policy))
            })
            .max_by_key(|(priority, _)| *priority)
            .map(|(_, policy)| policy)
    }

    pub fn node_for_model(&self, model_id: &str) -> Option<&WorkflowNodeV1> {
        self.nodes
            .iter()
            .filter(|node| {
                node.model_id.as_deref() == Some(model_id)
                    || node.parameters.as_object().is_some_and(|parameters| {
                        parameters
                            .values()
                            .any(|value| value.as_str() == Some(model_id))
                    })
            })
            .max_by_key(|node| node.priority)
    }

    pub fn should_schedule(&self, engine_capability_id: &str, profile: AnalysisProfile) -> bool {
        self.nodes
            .iter()
            .filter(|node| engine_capabilities(node).contains(&engine_capability_id))
            .any(|node| match node.execution_policy {
                WorkflowExecutionPolicyV1::Always => true,
                WorkflowExecutionPolicyV1::Disabled => false,
                WorkflowExecutionPolicyV1::MaximumOnly => profile == AnalysisProfile::Maximum,
                WorkflowExecutionPolicyV1::OnDisagreement
                | WorkflowExecutionPolicyV1::DisagreementWindows => {
                    profile != AnalysisProfile::Fast
                }
            })
    }

    fn validate_outputs(
        &self,
        request: &AnalyzeRequestV1,
        nodes: &BTreeMap<&str, &WorkflowNodeV1>,
    ) -> EngineResult<()> {
        for output in &self.terminal_outputs {
            let node = nodes.get(output.node.as_str()).ok_or_else(|| {
                invalid("workflow terminal output references an unknown node")
                    .for_request(&request.request_id)
            })?;
            let contract =
                port_contract(node, &output.port, PortDirection::Output).ok_or_else(|| {
                    invalid("workflow terminal output references an unknown output port")
                        .for_request(&request.request_id)
                })?;
            let terminal_role_valid = if contract.semantic == "audio" {
                output.audio_role.as_deref().is_some_and(known_audio_role)
                    && contract
                        .audio_role
                        .is_none_or(|expected| output.audio_role.as_deref() == Some(expected))
            } else {
                output.audio_role.is_none()
            };
            if node.execution_policy == WorkflowExecutionPolicyV1::Disabled
                || output.semantic_type != contract.semantic
                || !terminal_role_valid
                || !known_semantic_type(&output.semantic_type)
            {
                return Err(
                    invalid("workflow terminal output is invalid").for_request(&request.request_id)
                );
            }
        }
        let active_capabilities = self
            .nodes
            .iter()
            .filter(|node| node.execution_policy != WorkflowExecutionPolicyV1::Disabled)
            .flat_map(engine_capabilities)
            .collect::<BTreeSet<_>>();
        let terminal_types = self
            .terminal_outputs
            .iter()
            .map(|output| output.semantic_type.as_str())
            .collect::<BTreeSet<_>>();
        let require = |condition: bool, capability: &'static str, semantic: Option<&str>| {
            if condition
                && (!active_capabilities.contains(capability)
                    || semantic.is_some_and(|kind| !terminal_types.contains(kind)))
            {
                Err(EngineError::new(
                    EngineErrorCode::MissingCapability,
                    format!("compiled workflow cannot reach requested output through {capability}"),
                )
                .with_capability(capability)
                .for_request(&request.request_id))
            } else {
                Ok(())
            }
        };
        require(
            request.requested_artifacts.vocal_chart,
            "finalize.vocal_chart",
            Some("candidate_chart"),
        )?;
        require(
            request.requested_artifacts.singing_analysis,
            "fusion.candidate_graph",
            None,
        )?;
        require(
            request.requested_artifacts.transcript,
            "fusion.transcript",
            None,
        )?;
        require(request.requested_artifacts.alignment, "speech.align", None)?;
        require(
            request.requested_artifacts.pitch_evidence,
            "pitch.track",
            None,
        )?;
        for role in &request.requested_artifacts.stems {
            let capability = match role {
                AudioRole::GuideVocals | AudioRole::VocalStem => "audio.extract_vocals",
                AudioRole::LeadVocal => "audio.lead_isolate",
                AudioRole::Instrumental => "audio.extract_instrumental",
                AudioRole::BackingVocal | AudioRole::HarmonyVocal => "audio.lead_partition",
                AudioRole::OriginalMix | AudioRole::CleanLeadVocal => continue,
            };
            require(true, capability, None)?;
        }
        Ok(())
    }
}

pub fn engine_capabilities(node: &WorkflowNodeV1) -> Vec<&'static str> {
    let mut capabilities = match (node.capability_id.as_str(), node.model_id.as_deref()) {
        ("audio.source", None) => vec!["audio.decode"],
        ("audio.separate_vocal_bgm", Some("bs_roformer_vocals_ep317")) => {
            vec!["audio.extract_vocals"]
        }
        ("audio.lead_isolate", Some("melband_roformer_harmony")) => {
            vec!["audio.lead_isolate"]
        }
        ("audio.denoise", Some("melband_roformer_denoise_aufr33")) => vec!["audio.denoise"],
        ("audio.dereverb", Some("melband_roformer_dereverb_anvuew")) => {
            vec!["audio.dereverb"]
        }
        ("analysis.asr", Some("qwen3_asr_1_7b")) => vec!["speech.transcribe"],
        ("analysis.asr", Some("firered_asr2_aed")) => vec!["speech.transcribe.challenger"],
        ("analysis.forced_alignment", Some("qwen3_forced_aligner_0_6b")) => {
            vec!["speech.align"]
        }
        ("analysis.pitch_f0", Some("rmvpe")) => vec!["pitch.track"],
        ("analysis.pitch_f0", Some("fcpe")) => vec!["pitch.secondary"],
        ("analysis.note_boundary", Some("game")) => vec!["notes.game"],
        ("analysis.note_boundary", Some("basic_pitch")) => vec!["notes.basic_pitch"],
        ("analysis.note_boundary", Some("rosvot")) => vec!["notes.rosvot"],
        ("analysis.note_boundary", Some("stars")) => vec!["notes.stars"],
        ("analysis.technique", Some("stars")) => vec!["technique.analyze"],
        ("analysis.acoustic_dsp", None) => vec!["analysis.acoustic_dsp"],
        ("lyrics.known", None) => vec!["lyrics.reference"],
        ("fusion.transcript", None) => vec!["fusion.transcript"],
        ("fusion.singing_evidence", None) => vec!["fusion.singing"],
        ("fusion.candidate_graph", None) => vec!["fusion.candidate_graph"],
        ("finalize.canonical_singing_track", None) => vec!["finalize.vocal_chart"],
        _ => Vec::new(),
    };
    if node.capability_id == "audio.separate_vocal_bgm"
        && node
            .parameters
            .get("instrumental_model_id")
            .and_then(serde_json::Value::as_str)
            == Some("melband_roformer_inst_v2")
    {
        capabilities.push("audio.extract_instrumental");
    }
    capabilities
}

fn validate_node(node: &WorkflowNodeV1, request: &AnalyzeRequestV1) -> EngineResult<()> {
    validate_runtime_claim(node, request)?;
    if engine_capabilities(node).is_empty()
        && node.execution_policy != WorkflowExecutionPolicyV1::Disabled
    {
        return Err(EngineError::new(
            EngineErrorCode::MissingCapability,
            format!(
                "workflow node {} has no compatible Engine v1 capability/model pair",
                node.instance_id
            ),
        )
        .with_capability(&node.capability_id)
        .for_request(&request.request_id));
    }
    if !matches!(
        node.runtime.as_str(),
        "openvino"
            | "vulkan"
            | "native_dsp"
            | "cpu_reference"
            | "pinned_qwen_asr_vulkan"
            | "pinned_qwen_align_vulkan"
            | "unresolved"
    ) {
        return Err(invalid(format!(
            "workflow node {} declares unknown runtime {}",
            node.instance_id, node.runtime
        ))
        .for_request(&request.request_id));
    }
    if node.parameters.as_object().is_none() {
        return Err(
            invalid("workflow resolved parameters must be a JSON object")
                .for_request(&request.request_id),
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum PortDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy)]
struct PortContract {
    semantic: &'static str,
    audio_role: Option<&'static str>,
    required: bool,
    multiple: bool,
}

const fn port(
    semantic: &'static str,
    audio_role: Option<&'static str>,
    required: bool,
    multiple: bool,
) -> PortContract {
    PortContract {
        semantic,
        audio_role,
        required,
        multiple,
    }
}

fn port_contract(
    node: &WorkflowNodeV1,
    port_id: &str,
    direction: PortDirection,
) -> Option<PortContract> {
    use PortDirection::{Input, Output};
    match (node.capability_id.as_str(), direction, port_id) {
        ("audio.source", Output, "mix") => Some(port("audio", Some("source_mix"), false, false)),
        ("audio.separate_vocal_bgm", Input, "audio") => {
            Some(port("audio", Some("source_mix"), true, false))
        }
        ("audio.separate_vocal_bgm", Output, "vocal") => {
            Some(port("audio", Some("vocal"), false, false))
        }
        ("audio.separate_vocal_bgm", Output, "instrumental") => {
            Some(port("audio", Some("instrumental"), false, false))
        }
        ("audio.lead_isolate", Input, "audio") => Some(port("audio", Some("vocal"), true, false)),
        ("audio.lead_isolate", Output, "lead") => {
            Some(port("audio", Some("lead_vocal"), false, false))
        }
        ("audio.lead_isolate", Output, "residual") => {
            Some(port("audio", Some("vocal_residual"), false, false))
        }
        ("audio.denoise" | "audio.dereverb", Input, "audio") => {
            Some(port("audio", None, true, false))
        }
        ("audio.denoise" | "audio.dereverb", Output, "audio") => {
            Some(port("audio", None, false, false))
        }
        (
            "analysis.asr"
            | "analysis.pitch_f0"
            | "analysis.note_boundary"
            | "analysis.technique"
            | "analysis.acoustic_dsp",
            Input,
            "audio",
        ) => Some(port("audio", Some("lead_vocal"), true, false)),
        ("analysis.asr", Output, "transcript") => {
            Some(port("transcript_evidence", None, false, false))
        }
        ("analysis.pitch_f0", Output, "pitch") => Some(port("pitch_evidence", None, false, false)),
        ("analysis.note_boundary", Output, "boundaries") => {
            Some(port("boundary_evidence", None, false, false))
        }
        ("analysis.technique", Output, "techniques") => {
            Some(port("technique_evidence", None, false, false))
        }
        ("analysis.acoustic_dsp", Output, "acoustic") => {
            Some(port("acoustic_evidence", None, false, false))
        }
        ("lyrics.known", Output, "lyrics") => Some(port("lyrics", None, false, false)),
        ("fusion.transcript", Input, "evidence") => {
            Some(port("transcript_evidence", None, true, true))
        }
        ("fusion.transcript", Output, "lyrics") => Some(port("lyrics", None, false, false)),
        ("analysis.forced_alignment", Input, "audio") => {
            Some(port("audio", Some("lead_vocal"), true, false))
        }
        ("analysis.forced_alignment", Input, "lyrics") => Some(port("lyrics", None, true, false)),
        ("analysis.forced_alignment", Output, "alignment") => {
            Some(port("alignment_evidence", None, false, false))
        }
        ("fusion.singing_evidence", Input, "pitch") => {
            Some(port("pitch_evidence", None, true, true))
        }
        ("fusion.singing_evidence", Input, "boundaries") => {
            Some(port("boundary_evidence", None, true, true))
        }
        ("fusion.singing_evidence", Input, "alignment") => {
            Some(port("alignment_evidence", None, true, false))
        }
        ("fusion.singing_evidence", Input, "techniques") => {
            Some(port("technique_evidence", None, false, false))
        }
        ("fusion.singing_evidence", Input, "acoustic") => {
            Some(port("acoustic_evidence", None, false, false))
        }
        ("fusion.singing_evidence", Output, "evidence") => {
            Some(port("evidence_bundle", None, false, false))
        }
        ("fusion.candidate_graph", Input, "evidence") => {
            Some(port("evidence_bundle", None, true, false))
        }
        ("fusion.candidate_graph", Output, "candidates") => {
            Some(port("candidate_graph", None, false, false))
        }
        ("finalize.canonical_singing_track", Input, "candidates") => {
            Some(port("candidate_graph", None, true, false))
        }
        ("finalize.canonical_singing_track", Input, "lyrics") => {
            Some(port("lyrics", None, true, false))
        }
        ("finalize.canonical_singing_track", Output, "track") => {
            Some(port("canonical_singing_track", None, false, false))
        }
        ("finalize.canonical_singing_track", Output, "chart") => {
            Some(port("candidate_chart", None, false, false))
        }
        _ => None,
    }
}

fn validate_required_inputs(
    workflow: &WorkflowExecutionV1,
    nodes: &BTreeMap<&str, &WorkflowNodeV1>,
    request: &AnalyzeRequestV1,
) -> EngineResult<()> {
    for node in nodes
        .values()
        .filter(|node| node.execution_policy != WorkflowExecutionPolicyV1::Disabled)
    {
        let input_ports = workflow
            .bindings
            .iter()
            .filter(|binding| binding.to_node == node.analysis_node)
            .fold(
                BTreeMap::<&str, Vec<&WorkflowBindingV1>>::new(),
                |mut map, binding| {
                    map.entry(binding.to_port.as_str())
                        .or_default()
                        .push(binding);
                    map
                },
            );
        for (port_id, bindings) in &input_ports {
            let contract = port_contract(node, port_id, PortDirection::Input)
                .expect("binding ports were validated");
            if !contract.multiple && bindings.len() > 1 {
                return Err(invalid(format!(
                    "workflow node {} input {port_id} accepts only one binding",
                    node.instance_id
                ))
                .for_request(&request.request_id));
            }
        }
        for required_port in [
            "audio",
            "lyrics",
            "evidence",
            "pitch",
            "boundaries",
            "alignment",
            "candidates",
        ] {
            let Some(contract) = port_contract(node, required_port, PortDirection::Input) else {
                continue;
            };
            if contract.required
                && input_ports
                    .get(required_port)
                    .is_none_or(|bindings| bindings.iter().all(|binding| !binding.execution_active))
            {
                return Err(invalid(format!(
                    "workflow node {} is missing required input {required_port}",
                    node.instance_id
                ))
                .for_request(&request.request_id));
            }
            if contract.required
                && node.execution_policy == WorkflowExecutionPolicyV1::Always
                && input_ports.get(required_port).is_some_and(|bindings| {
                    bindings.iter().all(|binding| {
                        nodes
                            .get(binding.from_node.as_str())
                            .is_none_or(|producer| {
                                producer.execution_policy != WorkflowExecutionPolicyV1::Always
                            })
                    })
                })
            {
                return Err(invalid(format!(
                    "workflow node {} required input {required_port} depends only on conditional producers",
                    node.instance_id
                ))
                .for_request(&request.request_id));
            }
        }
    }
    Ok(())
}

fn validate_runtime_claim(node: &WorkflowNodeV1, request: &AnalyzeRequestV1) -> EngineResult<()> {
    if node.execution_policy == WorkflowExecutionPolicyV1::Disabled || node.runtime == "unresolved"
    {
        return Ok(());
    }
    let compatible = match node.model_id.as_deref() {
        None => node.runtime == "native_dsp",
        Some(
            "bs_roformer_vocals_ep317"
            | "melband_roformer_harmony"
            | "melband_roformer_denoise_aufr33"
            | "melband_roformer_dereverb_anvuew",
        ) => node.runtime == "vulkan",
        Some("qwen3_asr_1_7b") => node.runtime == "pinned_qwen_asr_vulkan",
        Some("qwen3_forced_aligner_0_6b") => node.runtime == "pinned_qwen_align_vulkan",
        Some(
            "rmvpe" | "fcpe" | "game" | "basic_pitch" | "firered_asr2_aed" | "rosvot" | "stars",
        ) => matches!(node.runtime.as_str(), "openvino" | "cpu_reference"),
        Some(_) => false,
    };
    if compatible {
        Ok(())
    } else {
        Err(EngineError::new(
            EngineErrorCode::RuntimeResolutionFailed,
            format!(
                "workflow node {} declares a runtime incompatible with its model",
                node.instance_id
            ),
        )
        .with_resource(
            node.model_id
                .as_deref()
                .map_or("workflow:native", |model| model),
        )
        .for_request(&request.request_id))
    }
}

fn validate_acyclic(
    nodes: &BTreeMap<&str, &WorkflowNodeV1>,
    edges: &BTreeSet<(&str, &str)>,
    request: &AnalyzeRequestV1,
) -> EngineResult<()> {
    let mut indegree = nodes
        .keys()
        .map(|node| (*node, 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (from, to) in edges {
        *indegree.entry(*to).or_default() += 1;
        outgoing.entry(*from).or_default().push(*to);
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect::<VecDeque<_>>();
    let mut visited = 0usize;
    while let Some(node) = ready.pop_front() {
        visited += 1;
        if let Some(children) = outgoing.get(node) {
            for child in children {
                let degree = indegree.get_mut(child).expect("validated workflow node");
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(child);
                }
            }
        }
    }
    if visited != nodes.len() {
        return Err(invalid("compiled workflow contains a cycle").for_request(&request.request_id));
    }
    Ok(())
}

fn known_semantic_type(value: &str) -> bool {
    matches!(
        value,
        "audio"
            | "lyrics"
            | "transcript_evidence"
            | "pitch_evidence"
            | "boundary_evidence"
            | "alignment_evidence"
            | "technique_evidence"
            | "acoustic_evidence"
            | "evidence_bundle"
            | "candidate_graph"
            | "canonical_singing_track"
            | "candidate_chart"
    )
}

fn known_audio_role(value: &str) -> bool {
    matches!(
        value,
        "source_mix"
            | "vocal"
            | "lead_vocal"
            | "back_vocal"
            | "backing_vocal"
            | "harmony_vocal"
            | "vocal_residual"
            | "instrumental"
            | "drums"
            | "bass"
            | "guitar"
            | "piano"
            | "other"
    )
}

fn validate_identifier(value: &str, label: &str) -> EngineResult<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        Err(invalid(format!("{label} contains unsupported characters")))
    } else {
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::InvalidContract, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::request::tests::valid_request;

    fn workflow_value() -> serde_json::Value {
        serde_json::json!({
            "contract": WORKFLOW_EXECUTION_CONTRACT,
            "version": 1,
            "workflow_schema_version": 2,
            "workflow_id": "song:test:workflow",
            "workflow_revision": 1,
            "quality_mode": "balanced",
            "definition_digest": "a".repeat(32),
            "nodes": [
                node("source", "audio.source", None, "always", 1000, "native_dsp"),
                node("split", "audio.separate_vocal_bgm", Some("bs_roformer_vocals_ep317"), "always", 900, "vulkan"),
                node("lead", "audio.lead_isolate", Some("melband_roformer_harmony"), "always", 800, "vulkan"),
                node("pitch", "analysis.pitch_f0", Some("rmvpe"), "always", 680, "openvino")
            ],
            "bindings": [
                binding("source", "mix", "split", "audio", "source_mix", false),
                binding("split", "vocal", "lead", "audio", "vocal", false),
                binding("lead", "lead", "pitch", "audio", "lead_vocal", true)
            ],
            "terminal_outputs": [{
                "node": "workflow.pitch",
                "port": "pitch",
                "semantic_type": "pitch_evidence"
            }]
        })
    }

    fn binding(
        from: &str,
        from_port: &str,
        to: &str,
        to_port: &str,
        role: &str,
        analyzer: bool,
    ) -> serde_json::Value {
        serde_json::json!({
            "from_node": format!("workflow.{from}"),
            "from_port": from_port,
            "to_node": format!("workflow.{to}"),
            "to_port": to_port,
            "semantic_type": "audio",
            "audio_role": role,
            "execution_active": true,
            "analyzer_attachment": analyzer
        })
    }

    fn node(
        id: &str,
        capability: &str,
        model: Option<&str>,
        policy: &str,
        priority: i32,
        runtime: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "instance_id": id,
            "capability_id": capability,
            "analysis_node": format!("workflow.{id}"),
            "model_id": model,
            "execution_policy": policy,
            "priority": priority,
            "runtime": runtime,
            "parameters": {}
        })
    }

    #[test]
    fn independently_deserializes_and_validates_workflow_extension() {
        let mut request = valid_request(AudioRole::OriginalMix);
        request.requested_artifacts.vocal_chart = false;
        request.requested_artifacts.singing_analysis = false;
        request.requested_artifacts.transcript = false;
        request.requested_artifacts.alignment = false;
        request.extensions.insert(
            WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
            workflow_value(),
        );
        let workflow = WorkflowExecutionV1::from_request(&request)
            .unwrap()
            .unwrap();
        assert_eq!(
            workflow.policy_for_engine_capability("pitch.track"),
            Some(WorkflowExecutionPolicyV1::Always)
        );
    }

    #[test]
    fn unknown_version_invalid_binding_and_cycle_fail_at_trust_boundary() {
        let mut request = valid_request(AudioRole::OriginalMix);
        request.requested_artifacts.vocal_chart = false;
        request.requested_artifacts.singing_analysis = false;
        request.requested_artifacts.transcript = false;
        request.requested_artifacts.alignment = false;

        let mut unknown = workflow_value();
        unknown["version"] = serde_json::json!(2);
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), unknown);
        assert_eq!(
            WorkflowExecutionV1::from_request(&request)
                .unwrap_err()
                .code,
            EngineErrorCode::UnsupportedContractVersion
        );

        let mut invalid_port = workflow_value();
        invalid_port["bindings"][0]["from_port"] = serde_json::json!("not-an-output");
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), invalid_port);
        assert_eq!(
            WorkflowExecutionV1::from_request(&request)
                .unwrap_err()
                .code,
            EngineErrorCode::InvalidContract
        );

        let mut cycle = workflow_value();
        cycle["nodes"].as_array_mut().unwrap().extend([
            node(
                "cleanup-a",
                "audio.denoise",
                Some("melband_roformer_denoise_aufr33"),
                "always",
                20,
                "vulkan",
            ),
            node(
                "cleanup-b",
                "audio.dereverb",
                Some("melband_roformer_dereverb_anvuew"),
                "always",
                10,
                "vulkan",
            ),
        ]);
        cycle["bindings"].as_array_mut().unwrap().extend([
            binding(
                "cleanup-a",
                "audio",
                "cleanup-b",
                "audio",
                "lead_vocal",
                false,
            ),
            binding(
                "cleanup-b",
                "audio",
                "cleanup-a",
                "audio",
                "lead_vocal",
                false,
            ),
        ]);
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), cycle);
        let error = WorkflowExecutionV1::from_request(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::InvalidContract);
        assert!(error.message.contains("cycle"));
    }

    #[test]
    fn incompatible_runtime_and_forged_terminal_port_fail_closed() {
        let mut request = valid_request(AudioRole::OriginalMix);
        request.requested_artifacts.vocal_chart = false;
        request.requested_artifacts.singing_analysis = false;
        request.requested_artifacts.transcript = false;
        request.requested_artifacts.alignment = false;

        let mut runtime = workflow_value();
        runtime["nodes"][3]["runtime"] = serde_json::json!("vulkan");
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), runtime);
        assert_eq!(
            WorkflowExecutionV1::from_request(&request)
                .unwrap_err()
                .code,
            EngineErrorCode::RuntimeResolutionFailed
        );

        let mut terminal = workflow_value();
        terminal["terminal_outputs"][0]["port"] = serde_json::json!("not-pitch");
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), terminal);
        assert_eq!(
            WorkflowExecutionV1::from_request(&request)
                .unwrap_err()
                .code,
            EngineErrorCode::InvalidContract
        );
    }

    #[test]
    fn active_future_lead_partition_claim_fails_closed() {
        let mut request = valid_request(AudioRole::LeadVocal);
        let mut value = workflow_value();
        value["nodes"].as_array_mut().unwrap().push(node(
            "lead-partition",
            "audio.lead_partition",
            Some("melband_roformer_harmony"),
            "always",
            790,
            "vulkan",
        ));
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), value);
        let error = WorkflowExecutionV1::from_request(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::MissingCapability);
        assert_eq!(error.capability.as_deref(), Some("audio.lead_partition"));
    }

    #[test]
    fn active_technique_claim_requires_the_exact_stars_model() {
        let mut request = valid_request(AudioRole::LeadVocal);
        let mut value = workflow_value();
        value["nodes"].as_array_mut().unwrap().push(node(
            "stars-technique",
            "analysis.technique",
            Some("rosvot"),
            "maximum_only",
            640,
            "openvino",
        ));
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), value);
        let error = WorkflowExecutionV1::from_request(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::MissingCapability);
        assert_eq!(error.capability.as_deref(), Some("analysis.technique"));
    }
}
