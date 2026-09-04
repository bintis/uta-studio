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
pub const WORKFLOW_SCHEMA_VERSION: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowExecutionPolicyV1 {
    Always,
    Disabled,
    MaximumOnly,
    OnDisagreement,
    DisagreementWindows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuousF0SourceV1 {
    Rmvpe,
    Fcpe,
}

impl ContinuousF0SourceV1 {
    pub fn model_id(self) -> &'static str {
        match self {
            Self::Rmvpe => "rmvpe",
            Self::Fcpe => "fcpe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteLengthSourceV1 {
    Game,
    F0Derived,
}

impl NoteLengthSourceV1 {
    pub fn parameter_value(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::F0Derived => "f0",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnsetSupportSourceV1 {
    Automatic,
    Acoustic,
    BasicPitch,
}

impl OnsetSupportSourceV1 {
    pub fn parameter_value(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Acoustic => "acoustic",
            Self::BasicPitch => "basic_pitch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FusionModeV1 {
    /// The algorithmic HSMM candidate-graph decode. Default and only
    /// production-pinned path.
    #[default]
    Algorithm,
    /// Explicit non-default opt-in: a locally installed AI agent CLI selects
    /// the final candidate path from the same evidence instead of the HSMM
    /// decoder. Never a silent fallback for the algorithmic path.
    AiJudgment,
}

impl FusionModeV1 {
    pub fn parameter_value(self) -> &'static str {
        match self {
            Self::Algorithm => "algorithm",
            Self::AiJudgment => "ai",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpertFusionPolicyV1 {
    pub continuous_f0: ContinuousF0SourceV1,
    pub note_lengths: NoteLengthSourceV1,
    pub onset_support: OnsetSupportSourceV1,
}

impl Default for ExpertFusionPolicyV1 {
    fn default() -> Self {
        Self {
            continuous_f0: ContinuousF0SourceV1::Rmvpe,
            note_lengths: NoteLengthSourceV1::Game,
            onset_support: OnsetSupportSourceV1::Automatic,
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fusion_policy: Option<ExpertFusionPolicyV1>,
    #[serde(default)]
    pub fusion_mode: FusionModeV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowProviderPreferencesV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrumental: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecutionInvocationV1 {
    pub invocation_id: String,
    pub provider_id: String,
    pub capabilities: Vec<String>,
    pub output_ports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNodeV1 {
    pub instance_id: String,
    pub capability_id: String,
    pub execution_policy: WorkflowExecutionPolicyV1,
    pub priority: i32,
    #[serde(default)]
    pub provider_preferences: WorkflowProviderPreferencesV1,
    #[serde(default)]
    pub execution_invocations: Vec<WorkflowExecutionInvocationV1>,
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
            || self.workflow_schema_version != WORKFLOW_SCHEMA_VERSION
        {
            return Err(EngineError::new(
                EngineErrorCode::UnsupportedContractVersion,
                format!(
                    "expected {WORKFLOW_EXECUTION_CONTRACT}/{WORKFLOW_EXECUTION_VERSION} with workflow schema {WORKFLOW_SCHEMA_VERSION}"
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
        let mut invocation_ids = BTreeSet::new();
        let mut nodes = BTreeMap::new();
        for node in &self.nodes {
            validate_identifier(&node.instance_id, "workflow node instance")?;
            if !instances.insert(node.instance_id.as_str()) {
                return Err(
                    invalid("compiled workflow contains duplicate node identity")
                        .for_request(&request.request_id),
                );
            }
            validate_node(node, request)?;
            validate_execution_invocations(node, &mut invocation_ids, request)?;
            nodes.insert(node.instance_id.as_str(), node);
        }
        self.validate_expert_fusion_policy(request)?;

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
                    || !matches!(binding.audio_role.as_deref(), Some("lead_vocal" | "vocal")))
            {
                return Err(invalid(
                    "Engine analyzers must bind to a compiled vocal or lead_vocal artifact",
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
                    .then_some((node.execution_policy, node.priority))
            })
            .max_by_key(|(policy, priority)| {
                let policy_rank = match policy {
                    WorkflowExecutionPolicyV1::Always => 4,
                    WorkflowExecutionPolicyV1::OnDisagreement
                    | WorkflowExecutionPolicyV1::DisagreementWindows => 3,
                    WorkflowExecutionPolicyV1::MaximumOnly => 2,
                    WorkflowExecutionPolicyV1::Disabled => 0,
                };
                (policy_rank, *priority)
            })
            .map(|(policy, _)| policy)
    }

    pub fn presentation_node_for_engine_execution(
        &self,
        capability: &str,
        model_id: Option<&str>,
    ) -> Option<String> {
        let node = self
            .nodes
            .iter()
            .filter(|node| engine_capabilities(node).contains(&capability))
            .filter(|node| {
                model_id.is_none_or(|model| {
                    engine_model_for_capability(node, capability) == Some(model)
                })
            })
            .max_by_key(|node| {
                let policy_rank = match node.execution_policy {
                    WorkflowExecutionPolicyV1::Always => 4,
                    WorkflowExecutionPolicyV1::OnDisagreement
                    | WorkflowExecutionPolicyV1::DisagreementWindows => 3,
                    WorkflowExecutionPolicyV1::MaximumOnly => 2,
                    WorkflowExecutionPolicyV1::Disabled => 0,
                };
                (policy_rank, node.priority)
            })?;
        if let Some(invocation) = node.execution_invocations.iter().find(|invocation| {
            invocation
                .capabilities
                .iter()
                .any(|item| item == capability)
                && model_id.is_none_or(|model| invocation.provider_id == model)
        }) {
            return Some(invocation.invocation_id.clone());
        }
        let suffix = match capability {
            "audio.extract_vocals" => ".vocal",
            "audio.extract_instrumental" => ".instrumental",
            _ => "",
        };
        Some(format!("{}{suffix}", node.instance_id))
    }

    pub fn model_for_engine_capability(&self, capability: &str) -> Option<&str> {
        self.nodes
            .iter()
            .filter(|node| node.execution_policy != WorkflowExecutionPolicyV1::Disabled)
            .filter(|node| engine_capabilities(node).contains(&capability))
            .max_by_key(|node| {
                let policy_rank = match node.execution_policy {
                    WorkflowExecutionPolicyV1::Always => 4,
                    WorkflowExecutionPolicyV1::OnDisagreement
                    | WorkflowExecutionPolicyV1::DisagreementWindows => 3,
                    WorkflowExecutionPolicyV1::MaximumOnly => 2,
                    WorkflowExecutionPolicyV1::Disabled => 0,
                };
                (policy_rank, node.priority)
            })
            .and_then(|node| engine_model_for_capability(node, capability))
    }

    pub fn policy_for_model(&self, model_id: &str) -> Option<WorkflowExecutionPolicyV1> {
        self.nodes
            .iter()
            .find(|node| node.provider_preferences.primary.as_deref() == Some(model_id))
            .map(|node| node.execution_policy)
    }

    pub fn should_schedule_model(&self, model_id: &str, profile: AnalysisProfile) -> bool {
        self.policy_for_model(model_id)
            .is_some_and(|policy| match policy {
                WorkflowExecutionPolicyV1::Always => true,
                WorkflowExecutionPolicyV1::Disabled => false,
                WorkflowExecutionPolicyV1::MaximumOnly => profile == AnalysisProfile::Maximum,
                WorkflowExecutionPolicyV1::OnDisagreement
                | WorkflowExecutionPolicyV1::DisagreementWindows => {
                    profile != AnalysisProfile::Fast
                }
            })
    }

    pub fn parameters_for_engine_capability(&self, capability: &str) -> Option<serde_json::Value> {
        let node = self
            .nodes
            .iter()
            .filter(|node| engine_capabilities(node).contains(&capability))
            .max_by_key(|node| node.priority)?;
        Some(self.engine_resolved_parameters(node))
    }

    pub fn engine_resolved_parameters(&self, node: &WorkflowNodeV1) -> serde_json::Value {
        if node.capability_id == "fusion.singing_evidence" {
            serde_json::json!({
                "fusion_mode": self.fusion_mode.parameter_value(),
            })
        } else {
            serde_json::json!({})
        }
    }

    /// Resolves the Engine's internal baseline from Stage 3 participation.
    /// The deserialize-only `fusion_policy` field is intentionally ignored.
    pub fn resolved_expert_fusion_policy(
        &self,
        profile: AnalysisProfile,
    ) -> Option<ExpertFusionPolicyV1> {
        self.nodes
            .iter()
            .any(|node| {
                node.capability_id == "fusion.singing_evidence"
                    && node.execution_policy != WorkflowExecutionPolicyV1::Disabled
            })
            .then(|| ExpertFusionPolicyV1 {
                continuous_f0: if self.should_schedule_model("rmvpe", profile) {
                    ContinuousF0SourceV1::Rmvpe
                } else {
                    ContinuousF0SourceV1::Fcpe
                },
                note_lengths: if self.should_schedule_model("game", profile) {
                    NoteLengthSourceV1::Game
                } else {
                    NoteLengthSourceV1::F0Derived
                },
                onset_support: OnsetSupportSourceV1::Automatic,
            })
    }

    pub fn fusion_mode(&self) -> FusionModeV1 {
        self.fusion_mode
    }

    fn validate_expert_fusion_policy(&self, request: &AnalyzeRequestV1) -> EngineResult<()> {
        let fusion_node = self
            .nodes
            .iter()
            .find(|node| node.capability_id == "fusion.singing_evidence");
        let Some(fusion_node) = fusion_node else {
            return Ok(());
        };
        if fusion_node.execution_policy == WorkflowExecutionPolicyV1::Disabled {
            return Ok(());
        }
        if !self.should_schedule_model("rmvpe", request.analysis.profile)
            && !self.should_schedule_model("fcpe", request.analysis.profile)
        {
            return Err(invalid(
                "Stage 3 must schedule at least one continuous F0 expert for final fusion",
            )
            .for_request(&request.request_id));
        }
        Ok(())
    }

    pub fn node_for_model(&self, model_id: &str) -> Option<&WorkflowNodeV1> {
        self.nodes
            .iter()
            .filter(|node| {
                node.provider_preferences.primary.as_deref() == Some(model_id)
                    || node.provider_preferences.instrumental.as_deref() == Some(model_id)
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
        let primary_role = request.primary_source()?.role;
        let force_lead_isolation = request
            .requested_artifacts
            .stems
            .contains(&AudioRole::LeadVocal)
            && !matches!(
                primary_role,
                AudioRole::LeadVocal | AudioRole::CleanLeadVocal
            );
        let active_capabilities = self
            .nodes
            .iter()
            .filter(|node| {
                node.execution_policy != WorkflowExecutionPolicyV1::Disabled
                    || (force_lead_isolation
                        && engine_capabilities(node).contains(&"audio.lead_isolate"))
            })
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
                AudioRole::LeadVocal
                    if matches!(
                        primary_role,
                        AudioRole::LeadVocal | AudioRole::CleanLeadVocal
                    ) =>
                {
                    continue;
                }
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

fn engine_model_for_capability<'a>(node: &'a WorkflowNodeV1, capability: &str) -> Option<&'a str> {
    if capability == "audio.extract_instrumental" {
        return node.provider_preferences.instrumental.as_deref();
    }
    node.provider_preferences.primary.as_deref()
}

pub fn engine_capabilities(node: &WorkflowNodeV1) -> Vec<&'static str> {
    let mut capabilities = match (
        node.capability_id.as_str(),
        node.provider_preferences.primary.as_deref(),
    ) {
        ("audio.source", None) => vec!["audio.decode"],
        (
            "audio.separate_vocal_bgm",
            Some("bs_roformer_leap_xe90_vocals" | "bs_polarformer_public_instrumental"),
        ) => {
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
            vec!["speech.align", "fusion.alignment"]
        }
        ("analysis.pitch_f0", Some("rmvpe")) => {
            vec!["pitch.track", "pitch.secondary.rmvpe"]
        }
        ("analysis.pitch_f0", Some("fcpe")) => {
            vec!["pitch.track", "pitch.secondary", "pitch.secondary.fcpe"]
        }
        ("analysis.note_boundary", Some("game")) => vec!["notes.game"],
        ("analysis.note_boundary", Some("basic_pitch")) => vec!["notes.basic_pitch"],
        ("analysis.note_boundary", Some("rosvot")) => vec!["notes.rosvot"],
        ("analysis.note_boundary", Some("stars")) => vec!["notes.stars"],
        ("analysis.note_boundary", Some("jbm555_cectc_80")) => vec!["notes.jbm555"],
        ("analysis.technique", Some("stars")) => vec!["technique.analyze"],
        ("analysis.acoustic_dsp", None) => vec!["analysis.acoustic_dsp"],
        ("lyrics.known", None) => vec!["lyrics.reference"],
        ("fusion.transcript", None) => vec!["fusion.transcript"],
        ("fusion.singing_evidence", None) => vec!["fusion.singing"],
        ("fusion.candidate_graph", None) => vec!["fusion.candidate_graph"],
        ("finalize.canonical_singing_track", None) => {
            vec!["rhythm.quantize", "finalize.vocal_chart"]
        }
        _ => Vec::new(),
    };
    if node.capability_id == "audio.separate_vocal_bgm"
        && matches!(
            node.provider_preferences.instrumental.as_deref(),
            Some("bs_polarformer_public_instrumental" | "melband_roformer_inst_v2")
        )
    {
        capabilities.push("audio.extract_instrumental");
    }
    capabilities
}

fn validate_execution_invocations<'a>(
    node: &'a WorkflowNodeV1,
    invocation_ids: &mut BTreeSet<&'a str>,
    request: &AnalyzeRequestV1,
) -> EngineResult<()> {
    let available = engine_capabilities(node);
    if node.execution_invocations.is_empty() {
        if available
            .iter()
            .any(|capability| capability.starts_with("audio.extract_"))
        {
            return Err(invalid(
                "workflow omits the required typed provider execution invocation topology",
            )
            .for_request(&request.request_id));
        }
        return Ok(());
    }
    let mut declared_capabilities = BTreeSet::new();
    let mut declared_outputs = BTreeSet::new();
    for invocation in &node.execution_invocations {
        validate_identifier(&invocation.invocation_id, "workflow execution invocation")?;
        validate_identifier(&invocation.provider_id, "workflow execution provider")?;
        if !invocation_ids.insert(&invocation.invocation_id) {
            return Err(
                invalid("workflow contains duplicate execution invocation identity")
                    .for_request(&request.request_id),
            );
        }
        if invocation.capabilities.is_empty() || invocation.output_ports.is_empty() {
            return Err(
                invalid("workflow execution invocation has no capability or output")
                    .for_request(&request.request_id),
            );
        }
        for capability in &invocation.capabilities {
            if !available.contains(&capability.as_str())
                || engine_model_for_capability(node, capability) != Some(&invocation.provider_id)
                || !declared_capabilities.insert(capability.as_str())
            {
                return Err(invalid(
                    "workflow execution invocation disagrees with typed provider capability binding",
                )
                .for_request(&request.request_id));
            }
            let required_output = match capability.as_str() {
                "audio.extract_vocals" => Some("vocal"),
                "audio.extract_instrumental" => Some("instrumental"),
                _ => None,
            };
            if required_output.is_some_and(|required_output| {
                !invocation
                    .output_ports
                    .iter()
                    .any(|output| output == required_output)
            }) {
                return Err(invalid(
                    "workflow execution invocation disagrees with semantic capability/output binding",
                )
                .for_request(&request.request_id));
            }
        }
        for output in &invocation.output_ports {
            if port_contract(node, output, PortDirection::Output).is_none()
                || !declared_outputs.insert(output.as_str())
            {
                return Err(invalid(
                    "workflow execution invocation has an unknown or duplicate output binding",
                )
                .for_request(&request.request_id));
            }
            let required_capability = match output.as_str() {
                "vocal" => Some("audio.extract_vocals"),
                "instrumental" => Some("audio.extract_instrumental"),
                _ => None,
            };
            if required_capability.is_some_and(|required_capability| {
                !invocation
                    .capabilities
                    .iter()
                    .any(|capability| capability == required_capability)
            }) {
                return Err(invalid(
                    "workflow execution invocation disagrees with semantic capability/output binding",
                )
                .for_request(&request.request_id));
            }
        }
    }
    for capability in available
        .iter()
        .filter(|capability| capability.starts_with("audio.extract_"))
    {
        if !declared_capabilities.contains(capability) {
            return Err(invalid(
                "workflow execution invocation topology omits a provider execution",
            )
            .for_request(&request.request_id));
        }
    }
    Ok(())
}

fn validate_node(node: &WorkflowNodeV1, request: &AnalyzeRequestV1) -> EngineResult<()> {
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
    for (slot, provider) in [
        ("primary", node.provider_preferences.primary.as_deref()),
        (
            "instrumental",
            node.provider_preferences.instrumental.as_deref(),
        ),
    ] {
        if let Some(provider) = provider {
            validate_identifier(provider, &format!("workflow {slot} provider preference"))?;
        }
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
        ) => Some(port("audio", None, true, false)),
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
        ("analysis.forced_alignment", Input, "audio") => Some(port("audio", None, true, false)),
        ("analysis.forced_alignment", Input, "lyrics") => Some(port("lyrics", None, true, false)),
        ("analysis.forced_alignment", Output, "alignment") => {
            Some(port("alignment_evidence", None, false, false))
        }
        ("fusion.singing_evidence", Input, "pitch") => {
            Some(port("pitch_evidence", None, true, true))
        }
        ("fusion.singing_evidence", Input, "boundaries") => {
            Some(port("boundary_evidence", None, false, true))
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

fn producer_policy_covers_consumer(
    producer: WorkflowExecutionPolicyV1,
    consumer: WorkflowExecutionPolicyV1,
) -> bool {
    match consumer {
        WorkflowExecutionPolicyV1::Disabled => true,
        WorkflowExecutionPolicyV1::Always => producer == WorkflowExecutionPolicyV1::Always,
        WorkflowExecutionPolicyV1::OnDisagreement
        | WorkflowExecutionPolicyV1::DisagreementWindows => matches!(
            producer,
            WorkflowExecutionPolicyV1::Always
                | WorkflowExecutionPolicyV1::OnDisagreement
                | WorkflowExecutionPolicyV1::DisagreementWindows
        ),
        WorkflowExecutionPolicyV1::MaximumOnly => matches!(
            producer,
            WorkflowExecutionPolicyV1::Always | WorkflowExecutionPolicyV1::MaximumOnly
        ),
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
            .filter(|binding| binding.to_node == node.instance_id)
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
                && input_ports.get(required_port).is_some_and(|bindings| {
                    bindings.iter().all(|binding| {
                        nodes
                            .get(binding.from_node.as_str())
                            .is_none_or(|producer| {
                                !producer_policy_covers_consumer(
                                    producer.execution_policy,
                                    node.execution_policy,
                                )
                            })
                    })
                })
            {
                return Err(invalid(format!(
                    "workflow node {} required input {required_port} depends only on producers whose execution policy may omit it",
                    node.instance_id
                ))
                .for_request(&request.request_id));
            }
        }
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
            "workflow_schema_version": WORKFLOW_SCHEMA_VERSION,
            "workflow_id": "song:test:workflow",
            "workflow_revision": 1,
            "quality_mode": "balanced",
            "definition_digest": "a".repeat(32),
            "nodes": [
                node("source", "audio.source", None, "always", 1000, "native_dsp"),
                node("split", "audio.separate_vocal_bgm", Some("bs_roformer_leap_xe90_vocals"), "always", 900, "vulkan"),
                node("lead", "audio.lead_isolate", Some("melband_roformer_harmony"), "always", 800, "vulkan"),
                node("pitch", "analysis.pitch_f0", Some("rmvpe"), "always", 680, "ggml_vulkan")
            ],
            "bindings": [
                binding("source", "mix", "split", "audio", "source_mix", false),
                binding("split", "vocal", "lead", "audio", "vocal", false),
                binding("lead", "lead", "pitch", "audio", "lead_vocal", true)
            ],
            "terminal_outputs": [{
                "node": "pitch",
                "port": "pitch",
                "semantic_type": "pitch_evidence"
            }]
        })
    }

    fn workflow_with_typed_fusion(policy: &str) -> serde_json::Value {
        let mut value = workflow_value();
        let fusion = node(
            "fusion",
            "fusion.singing_evidence",
            None,
            policy,
            500,
            "native_dsp",
        );
        value["nodes"].as_array_mut().unwrap().push(fusion);
        value["fusion_policy"] = serde_json::json!({
            "continuous_f0": "rmvpe",
            "note_lengths": "game",
            "onset_support": "automatic"
        });
        value
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
            "from_node": from,
            "from_port": from_port,
            "to_node": to,
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
        let _ = runtime;
        let mut value = serde_json::json!({
            "instance_id": id,
            "capability_id": capability,
            "execution_policy": policy,
            "priority": priority,
            "provider_preferences": {
                "primary": model
            }
        });
        if capability == "audio.separate_vocal_bgm" {
            value["execution_invocations"] = serde_json::json!([{
                "invocation_id": format!("{id}.vocal"),
                "provider_id": model.expect("separation fixture has a provider"),
                "capabilities": ["audio.extract_vocals"],
                "output_ports": ["vocal"]
            }]);
        }
        value
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
    fn separation_models_map_to_independent_presentation_nodes() {
        let mut value = workflow_value();
        value["nodes"][1]["provider_preferences"]["instrumental"] =
            serde_json::json!("bs_polarformer_public_instrumental");
        value["nodes"][1]["execution_invocations"] = serde_json::json!([
            {
                "invocation_id": "split.vocal",
                "provider_id": "bs_roformer_leap_xe90_vocals",
                "capabilities": ["audio.extract_vocals"],
                "output_ports": ["vocal"]
            },
            {
                "invocation_id": "split.instrumental",
                "provider_id": "bs_polarformer_public_instrumental",
                "capabilities": ["audio.extract_instrumental"],
                "output_ports": ["instrumental"]
            }
        ]);
        let workflow: WorkflowExecutionV1 = serde_json::from_value(value).unwrap();
        assert_eq!(
            workflow.presentation_node_for_engine_execution(
                "audio.extract_vocals",
                Some("bs_roformer_leap_xe90_vocals")
            ),
            Some("split.vocal".to_string())
        );
        assert_eq!(
            workflow.presentation_node_for_engine_execution(
                "audio.extract_instrumental",
                Some("bs_polarformer_public_instrumental")
            ),
            Some("split.instrumental".to_string())
        );
        assert_eq!(
            workflow.model_for_engine_capability("audio.extract_instrumental"),
            Some("bs_polarformer_public_instrumental")
        );
    }

    #[test]
    fn removed_single_provider_residual_strategy_is_rejected() {
        let mut value = workflow_value();
        value["nodes"][1]["provider_preferences"]["instrumental"] =
            serde_json::json!("bs_roformer_leap_xe90_vocals");
        value["nodes"][1]["execution_invocations"] = serde_json::json!([{
            "invocation_id": "split",
            "provider_id": "bs_roformer_leap_xe90_vocals",
            "capabilities": ["audio.extract_vocals", "audio.extract_instrumental"],
            "output_ports": ["vocal", "instrumental"]
        }]);
        let mut request = valid_request(AudioRole::OriginalMix);
        request.requested_artifacts.vocal_chart = false;
        request.requested_artifacts.singing_analysis = false;
        request.requested_artifacts.transcript = false;
        request.requested_artifacts.alignment = false;
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), value);
        assert!(WorkflowExecutionV1::from_request(&request).is_err());
    }

    #[test]
    fn separation_requires_typed_provider_invocation_topology() {
        let mut value = workflow_value();
        value["nodes"][1]["execution_invocations"] = serde_json::json!([]);
        let mut request = valid_request(AudioRole::OriginalMix);
        request.requested_artifacts.vocal_chart = false;
        request.requested_artifacts.singing_analysis = false;
        request.requested_artifacts.transcript = false;
        request.requested_artifacts.alignment = false;
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), value);
        let error = WorkflowExecutionV1::from_request(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::InvalidContract);
        assert!(error.message.contains("invocation topology"));
    }

    #[test]
    fn forged_provider_invocation_topology_fails_at_the_engine_boundary() {
        let mut value = workflow_value();
        value["nodes"][1]["execution_invocations"] = serde_json::json!([{
            "invocation_id": "split.vocal",
            "provider_id": "bs_polarformer_public_instrumental",
            "capabilities": ["audio.extract_vocals"],
            "output_ports": ["vocal"]
        }]);
        let mut request = valid_request(AudioRole::OriginalMix);
        request.requested_artifacts.vocal_chart = false;
        request.requested_artifacts.singing_analysis = false;
        request.requested_artifacts.transcript = false;
        request.requested_artifacts.alignment = false;
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), value);
        let error = WorkflowExecutionV1::from_request(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::InvalidContract);
        assert!(error.message.contains("typed provider capability binding"));
    }

    #[test]
    fn swapped_separation_output_bindings_fail_at_the_engine_boundary() {
        let mut value = workflow_value();
        value["nodes"][1]["provider_preferences"]["instrumental"] =
            serde_json::json!("bs_polarformer_public_instrumental");
        value["nodes"][1]["execution_invocations"] = serde_json::json!([
            {
                "invocation_id": "split.vocal",
                "provider_id": "bs_roformer_leap_xe90_vocals",
                "capabilities": ["audio.extract_vocals"],
                "output_ports": ["instrumental"]
            },
            {
                "invocation_id": "split.instrumental",
                "provider_id": "bs_polarformer_public_instrumental",
                "capabilities": ["audio.extract_instrumental"],
                "output_ports": ["vocal"]
            }
        ]);
        let mut request = valid_request(AudioRole::OriginalMix);
        request.requested_artifacts.vocal_chart = false;
        request.requested_artifacts.singing_analysis = false;
        request.requested_artifacts.transcript = false;
        request.requested_artifacts.alignment = false;
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), value);
        let error = WorkflowExecutionV1::from_request(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::InvalidContract);
        assert!(error.message.contains("semantic capability/output binding"));
    }

    #[test]
    fn engine_native_substages_reuse_their_owning_workflow_card() {
        let mut value = workflow_value();
        value["nodes"].as_array_mut().unwrap().push(node(
            "alignment",
            "analysis.forced_alignment",
            Some("qwen3_forced_aligner_0_6b"),
            "always",
            600,
            "vulkan",
        ));
        value["nodes"].as_array_mut().unwrap().push(node(
            "canonical",
            "finalize.canonical_singing_track",
            None,
            "always",
            300,
            "native_dsp",
        ));
        let workflow: WorkflowExecutionV1 = serde_json::from_value(value).unwrap();
        assert_eq!(
            workflow.presentation_node_for_engine_execution("fusion.alignment", None),
            Some("alignment".to_string())
        );
        assert_eq!(
            workflow.presentation_node_for_engine_execution("rhythm.quantize", None),
            Some("canonical".to_string())
        );
    }

    #[test]
    fn consumer_policy_rejects_required_input_from_less_available_producer() {
        let mut value = workflow_value();
        value["nodes"][2]["execution_policy"] = serde_json::json!("maximum_only");
        value["nodes"][3]["execution_policy"] = serde_json::json!("on_disagreement");
        let mut request = valid_request(AudioRole::OriginalMix);
        request.requested_artifacts.vocal_chart = false;
        request.requested_artifacts.singing_analysis = false;
        request.requested_artifacts.transcript = false;
        request.requested_artifacts.alignment = false;
        request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), value);
        let error = WorkflowExecutionV1::from_request(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::InvalidContract);
        assert!(error.message.contains("execution policy may omit it"));

        let mut reverse = workflow_value();
        reverse["nodes"][2]["execution_policy"] = serde_json::json!("on_disagreement");
        reverse["nodes"][3]["execution_policy"] = serde_json::json!("maximum_only");
        let mut reverse_request = valid_request(AudioRole::OriginalMix);
        reverse_request.requested_artifacts.vocal_chart = false;
        reverse_request.requested_artifacts.singing_analysis = false;
        reverse_request.requested_artifacts.transcript = false;
        reverse_request.requested_artifacts.alignment = false;
        reverse_request
            .extensions
            .insert(WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(), reverse);
        let reverse_error = WorkflowExecutionV1::from_request(&reverse_request).unwrap_err();
        assert_eq!(reverse_error.code, EngineErrorCode::InvalidContract);
        assert!(
            reverse_error
                .message
                .contains("execution policy may omit it")
        );
    }

    #[test]
    fn stage_four_parameters_expose_only_fusion_mode() {
        let workflow: WorkflowExecutionV1 =
            serde_json::from_value(workflow_with_typed_fusion("always")).unwrap();
        let fusion = workflow
            .nodes
            .iter()
            .find(|node| node.capability_id == "fusion.singing_evidence")
            .unwrap();
        assert_eq!(
            workflow.engine_resolved_parameters(fusion),
            serde_json::json!({"fusion_mode": "algorithm"})
        );
        assert_eq!(
            workflow.resolved_expert_fusion_policy(AnalysisProfile::Balanced),
            Some(ExpertFusionPolicyV1 {
                continuous_f0: ContinuousF0SourceV1::Rmvpe,
                note_lengths: NoteLengthSourceV1::F0Derived,
                onset_support: OnsetSupportSourceV1::Automatic,
            })
        );
    }

    #[test]
    fn legacy_fusion_policy_is_ignored_in_favor_of_stage_three() {
        let mut value = workflow_with_typed_fusion("always");
        value["fusion_policy"]["continuous_f0"] = serde_json::json!("fcpe");
        value["fusion_policy"]["note_lengths"] = serde_json::json!("game");
        value["fusion_policy"]["onset_support"] = serde_json::json!("basic_pitch");
        let workflow: WorkflowExecutionV1 = serde_json::from_value(value).unwrap();
        assert_eq!(
            workflow.resolved_expert_fusion_policy(AnalysisProfile::Balanced),
            Some(ExpertFusionPolicyV1 {
                continuous_f0: ContinuousF0SourceV1::Rmvpe,
                note_lengths: NoteLengthSourceV1::F0Derived,
                onset_support: OnsetSupportSourceV1::Automatic,
            })
        );
    }

    #[test]
    fn conditional_game_participation_resolves_by_profile() {
        let mut value = workflow_with_typed_fusion("always");
        value["nodes"].as_array_mut().unwrap().push(node(
            "game",
            "analysis.note_boundary",
            Some("game"),
            "maximum_only",
            600,
            "openvino",
        ));
        let workflow: WorkflowExecutionV1 = serde_json::from_value(value).unwrap();
        assert_eq!(
            workflow
                .resolved_expert_fusion_policy(AnalysisProfile::Fast)
                .unwrap()
                .note_lengths,
            NoteLengthSourceV1::F0Derived
        );
        assert_eq!(
            workflow
                .resolved_expert_fusion_policy(AnalysisProfile::Maximum)
                .unwrap()
                .note_lengths,
            NoteLengthSourceV1::Game
        );
    }

    #[test]
    fn active_fcpe_primary_wins_over_a_higher_priority_disabled_rmvpe_node() {
        let mut value = workflow_value();
        value["nodes"][3]["execution_policy"] = serde_json::json!("disabled");
        value["nodes"].as_array_mut().unwrap().push(node(
            "fcpe",
            "analysis.pitch_f0",
            Some("fcpe"),
            "always",
            670,
            "openvino",
        ));
        let workflow: WorkflowExecutionV1 = serde_json::from_value(value).unwrap();
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
        unknown["version"] = serde_json::json!(99);
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
    fn studio_runtime_claim_and_forged_terminal_port_fail_closed() {
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
        let error = WorkflowExecutionV1::from_request(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::InvalidContract);
        assert!(error.message.contains("unknown field `runtime`"));

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
