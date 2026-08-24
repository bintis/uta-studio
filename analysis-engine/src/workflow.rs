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
            || self.workflow_schema_version != 1
        {
            return Err(EngineError::new(
                EngineErrorCode::UnsupportedContractVersion,
                format!(
                    "expected {WORKFLOW_EXECUTION_CONTRACT}/{WORKFLOW_EXECUTION_VERSION} with workflow schema 1"
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
        if self.definition_digest.len() != 32
            || !self
                .definition_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid(
                "workflow definition_digest must be 32 lowercase hexadecimal characters",
            )
            .for_request(&request.request_id));
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
            if binding.from_port.trim().is_empty()
                || binding.to_port.trim().is_empty()
                || !known_semantic_type(&binding.semantic_type)
            {
                return Err(invalid("workflow binding port or semantic type is invalid")
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
                if !known_audio_role(role) {
                    return Err(invalid(format!("unknown workflow audio role {role}"))
                        .for_request(&request.request_id));
                }
                if role == "back_vocal" {
                    return Err(EngineError::new(
                        EngineErrorCode::MissingCapability,
                        "BackVocal is not a truthful alias for the RoFormer vocal residual",
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
        match self.policy_for_engine_capability(engine_capability_id) {
            None | Some(WorkflowExecutionPolicyV1::Always) => true,
            Some(WorkflowExecutionPolicyV1::Disabled) => false,
            Some(WorkflowExecutionPolicyV1::MaximumOnly) => profile == AnalysisProfile::Maximum,
            Some(
                WorkflowExecutionPolicyV1::OnDisagreement
                | WorkflowExecutionPolicyV1::DisagreementWindows,
            ) => profile != AnalysisProfile::Fast,
        }
    }

    fn validate_outputs(
        &self,
        request: &AnalyzeRequestV1,
        nodes: &BTreeMap<&str, &WorkflowNodeV1>,
    ) -> EngineResult<()> {
        for output in &self.terminal_outputs {
            if !nodes.contains_key(output.node.as_str())
                || output.port.trim().is_empty()
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
    if node.capability_id == "analysis.technique"
        && node.execution_policy != WorkflowExecutionPolicyV1::Disabled
    {
        return Err(EngineError::new(
            EngineErrorCode::MissingCapability,
            "STARS currently exposes note evidence only; technique/style execution is unavailable",
        )
        .with_capability("technique.analyze")
        .for_request(&request.request_id));
    }
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
    if let Some(digest) = &node.runtime_recipe_digest
        && (digest.len() != 32
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    {
        return Err(
            invalid("workflow runtime recipe digest is invalid").for_request(&request.request_id)
        );
    }
    Ok(())
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
            "workflow_schema_version": 1,
            "workflow_id": "song:test:workflow",
            "workflow_revision": 1,
            "quality_mode": "balanced",
            "definition_digest": "a".repeat(32),
            "nodes": [
                node("source", "audio.source", None, "always", 1000, "native_dsp"),
                node("qwen", "analysis.asr", Some("qwen3_asr_1_7b"), "always", 700, "pinned_qwen_asr_vulkan"),
                node("align", "analysis.forced_alignment", Some("qwen3_forced_aligner_0_6b"), "always", 590, "pinned_qwen_align_vulkan"),
                node("pitch", "analysis.pitch_f0", Some("rmvpe"), "always", 680, "openvino"),
                node("game", "analysis.note_boundary", Some("game"), "always", 660, "openvino"),
                node("acoustic", "analysis.acoustic_dsp", None, "always", 650, "native_dsp"),
                node("transcript", "fusion.transcript", None, "always", 600, "native_dsp"),
                node("fusion", "fusion.singing_evidence", None, "always", 500, "native_dsp"),
                node("graph", "fusion.candidate_graph", None, "always", 400, "native_dsp"),
                node("final", "finalize.canonical_singing_track", None, "always", 300, "native_dsp")
            ],
            "bindings": [],
            "terminal_outputs": [{
                "node": "workflow.final",
                "port": "chart",
                "semantic_type": "candidate_chart"
            }]
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
        let mut request = valid_request(AudioRole::LeadVocal);
        request.extensions.insert(
            WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
            workflow_value(),
        );
        let workflow = WorkflowExecutionV1::from_request(&request)
            .unwrap()
            .unwrap();
        assert_eq!(
            workflow.policy_for_engine_capability("speech.transcribe"),
            Some(WorkflowExecutionPolicyV1::Always)
        );
    }

    #[test]
    fn active_technique_claim_fails_closed() {
        let mut request = valid_request(AudioRole::LeadVocal);
        let mut value = workflow_value();
        value["nodes"].as_array_mut().unwrap().push(node(
            "stars-technique",
            "analysis.technique",
            Some("stars"),
            "maximum_only",
            640,
            "openvino",
        ));
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), value);
        let error = WorkflowExecutionV1::from_request(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::MissingCapability);
        assert_eq!(error.capability.as_deref(), Some("technique.analyze"));
    }
}
