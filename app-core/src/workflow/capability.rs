use serde::{Deserialize, Serialize};

use super::{AudioRole, CapabilityId, SeparationStrategyV1, WorkflowPortSpec, WorkflowPortType};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityClass {
    Source,
    AudioTransformation,
    Analyzer,
    Fusion,
    Finalization,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCapability {
    pub id: CapabilityId,
    pub label: String,
    pub class: CapabilityClass,
    pub inputs: Vec<WorkflowPortSpec>,
    pub outputs: Vec<WorkflowPortSpec>,
    #[serde(default)]
    pub allows_multiple_instances: bool,
    #[serde(default)]
    pub preserves_audio_role: bool,
    #[serde(default)]
    pub hard_dependencies: Vec<CapabilityId>,
}

impl NodeCapability {
    pub fn input(&self, id: &str) -> Option<&WorkflowPortSpec> {
        self.inputs.iter().find(|port| port.id == id)
    }

    pub fn output(&self, id: &str) -> Option<&WorkflowPortSpec> {
        self.outputs.iter().find(|port| port.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowModelOption {
    pub model_id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeparationOutputRoleV1 {
    Vocal,
    Instrumental,
}

impl SeparationOutputRoleV1 {
    pub const fn output_port(self) -> &'static str {
        match self {
            Self::Vocal => "vocal",
            Self::Instrumental => "instrumental",
        }
    }

    pub const fn engine_capability(self) -> &'static str {
        match self {
            Self::Vocal => "audio.extract_vocals",
            Self::Instrumental => "audio.extract_instrumental",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeparationProviderExecutionV1 {
    pub provider_id: &'static str,
    pub output_roles: &'static [SeparationOutputRoleV1],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeparationStrategyOptionV1 {
    pub strategy: SeparationStrategyV1,
    pub label: &'static str,
    pub description: &'static str,
    pub executions: &'static [SeparationProviderExecutionV1],
}

const VOCAL_INSTRUMENTAL_ROLES: &[SeparationOutputRoleV1] = &[
    SeparationOutputRoleV1::Vocal,
    SeparationOutputRoleV1::Instrumental,
];
const LEAP_DUAL_OUTPUT_EXECUTION: &[SeparationProviderExecutionV1] =
    &[SeparationProviderExecutionV1 {
        provider_id: "bs_roformer_leap_xe90_vocals",
        output_roles: VOCAL_INSTRUMENTAL_ROLES,
    }];
const LEAP_INSTRUMENTAL_DIRECT_EXECUTION: &[SeparationProviderExecutionV1] =
    &[SeparationProviderExecutionV1 {
        provider_id: "bs_roformer_leap_xe90_instrumental",
        output_roles: VOCAL_INSTRUMENTAL_ROLES,
    }];
const POLARFORMER_DUAL_OUTPUT_EXECUTION: &[SeparationProviderExecutionV1] =
    &[SeparationProviderExecutionV1 {
        provider_id: "bs_polarformer_public_instrumental",
        output_roles: VOCAL_INSTRUMENTAL_ROLES,
    }];
const SEPARATION_STRATEGIES: &[SeparationStrategyOptionV1] = &[
    SeparationStrategyOptionV1 {
        strategy: SeparationStrategyV1::LeapDualOutput,
        label: "Leap XE90 · Vocals + Instrumental",
        description: "Default. One Leap XE90 inference publishes the trained vocal estimate and its mix-minus-vocals Instrumental residual.",
        executions: LEAP_DUAL_OUTPUT_EXECUTION,
    },
    SeparationStrategyOptionV1 {
        strategy: SeparationStrategyV1::LeapInstrumentalDirect,
        label: "Leap XE90 · Fuller BGM",
        description: "Optional. The instrumental-target model publishes its direct, fuller Instrumental estimate and a mix-minus-instrumental vocal residual. It may retain more vocal bleed than the default.",
        executions: LEAP_INSTRUMENTAL_DIRECT_EXECUTION,
    },
    SeparationStrategyOptionV1 {
        strategy: SeparationStrategyV1::PolarformerBoth,
        label: "Experimental · PolarFormer · Vocals + Instrumental",
        description: "Disabled by default. One public PolarFormer inference publishes the trained vocal estimate and its mix-minus-vocals Instrumental residual for explicit A/B evaluation.",
        executions: POLARFORMER_DUAL_OUTPUT_EXECUTION,
    },
];

pub fn separation_strategy_options() -> &'static [SeparationStrategyOptionV1] {
    SEPARATION_STRATEGIES
}

pub fn separation_strategy_descriptor(
    strategy: SeparationStrategyV1,
) -> &'static SeparationStrategyOptionV1 {
    if strategy == SeparationStrategyV1::Ep317VocalResidual {
        return &SEPARATION_STRATEGIES[0];
    }
    SEPARATION_STRATEGIES
        .iter()
        .find(|option| option.strategy == strategy)
        .expect("every typed separation strategy has a descriptor")
}

pub fn workflow_model_label(model_id: &str) -> &str {
    match model_id {
        "bs_roformer_leap_xe90_vocals" => "BS-RoFormer Leap XE90 Vocals",
        "bs_roformer_leap_xe90_instrumental" => "BS-RoFormer Leap XE90 Instrumental",
        "bs_polarformer_public_instrumental" => "BS-PolarFormer Public Instrumental",
        "melband_roformer_harmony" => "MelBand-RoFormer Lead Isolation",
        "melband_roformer_denoise_aufr33" => "MelBand-RoFormer Denoise",
        "melband_roformer_dereverb_anvuew" => "MelBand-RoFormer Dereverb",
        "rmvpe" => "RMVPE",
        "fcpe" => "FCPE",
        "qwen3_asr_1_7b" => "Qwen3-ASR 1.7B",
        "qwen3_forced_aligner_0_6b" => "Qwen3 Forced Aligner 0.6B",
        "basic_pitch" => "Basic Pitch",
        "game" | "game_1_0_3_medium" => "GAME 1.0.3 Medium",
        "game_1_0_3_small" => "GAME 1.0.3 Small",
        "game_1_0_3_large" => "GAME 1.0.3 Large",
        "jbm555" | "jbm555_cectc_80" => "JBM555 CECTC-80",
        "stars" => "STARS",
        "rosvot" => "ROSVOT",
        "firered_asr2_aed" => "FireRedASR2-AED",
        other => other,
    }
}

const GAME_MODEL_OPTIONS: &[WorkflowModelOption] = &[
    WorkflowModelOption {
        model_id: "game_1_0_3_small",
        label: "Small",
    },
    WorkflowModelOption {
        model_id: "game_1_0_3_medium",
        label: "Medium · default",
    },
    WorkflowModelOption {
        model_id: "game_1_0_3_large",
        label: "Large",
    },
];

/// Exact Engine-v1 provider choices that are interchangeable inside one
/// Processing Studio card. Only the GAME card exposes variants; independent
/// Basic Pitch/JBM555/STARS/ROSVOT experts cannot be silently repurposed into
/// duplicate GAME executions.
pub fn workflow_model_options(
    capability_id: &CapabilityId,
    current_model_id: Option<&str>,
) -> &'static [WorkflowModelOption] {
    if capability_id.as_str() == "analysis.note_boundary"
        && current_model_id.is_some_and(|model| {
            matches!(
                model,
                "game" | "game_1_0_3_small" | "game_1_0_3_medium" | "game_1_0_3_large"
            )
        })
    {
        GAME_MODEL_OPTIONS
    } else {
        &[]
    }
}

fn port(id: &str, port_type: WorkflowPortType, required: bool) -> WorkflowPortSpec {
    WorkflowPortSpec {
        id: id.to_string(),
        port_type,
        required,
        multiple: false,
    }
}

fn capability(
    id: &str,
    label: &str,
    class: CapabilityClass,
    inputs: Vec<WorkflowPortSpec>,
    outputs: Vec<WorkflowPortSpec>,
) -> NodeCapability {
    NodeCapability {
        id: CapabilityId::new(id),
        label: label.to_string(),
        class,
        inputs,
        outputs,
        allows_multiple_instances: false,
        preserves_audio_role: false,
        hard_dependencies: Vec::new(),
    }
}

pub fn builtin_capabilities() -> Vec<NodeCapability> {
    use AudioRole::*;
    use CapabilityClass::*;
    use WorkflowPortType::*;

    let mut result = vec![
        capability(
            "audio.source",
            "Original mix",
            Source,
            vec![],
            vec![port("mix", Audio(SourceMix), false)],
        ),
        capability(
            "audio.separate_vocal_bgm",
            "Vocal / BGM separation",
            AudioTransformation,
            vec![port("audio", Audio(SourceMix), true)],
            vec![
                port("vocal", Audio(Vocal), false),
                port("instrumental", Audio(Instrumental), false),
            ],
        ),
        capability(
            "audio.lead_isolate",
            "Lead vocal isolation",
            AudioTransformation,
            vec![port("audio", Audio(Vocal), true)],
            vec![
                port("lead", Audio(LeadVocal), false),
                port("residual", Audio(VocalResidual), false),
            ],
        ),
    ];

    for (id, label) in [
        ("audio.denoise", "Denoise"),
        ("audio.dereverb", "Dereverb"),
        ("audio.refine", "Stem refinement"),
    ] {
        let mut item = capability(
            id,
            label,
            AudioTransformation,
            vec![port("audio", Audio(Vocal), true)],
            vec![port("audio", Audio(Vocal), false)],
        );
        item.allows_multiple_instances = true;
        item.preserves_audio_role = true;
        result.push(item);
    }

    for (id, label, output_id, output_type) in [
        (
            "analysis.asr",
            "Singing transcription",
            "transcript",
            TranscriptEvidence,
        ),
        (
            "analysis.pitch_f0",
            "Continuous pitch",
            "pitch",
            PitchEvidence,
        ),
        (
            "analysis.note_boundary",
            "Note boundaries",
            "boundaries",
            BoundaryEvidence,
        ),
        (
            "analysis.technique",
            "Singing technique",
            "techniques",
            TechniqueEvidence,
        ),
        (
            "analysis.acoustic_dsp",
            "Acoustic DSP",
            "acoustic",
            AcousticEvidence,
        ),
    ] {
        let mut item = capability(
            id,
            label,
            Analyzer,
            vec![port("audio", Audio(Vocal), true)],
            vec![port(output_id, output_type, false)],
        );
        item.allows_multiple_instances = true;
        result.push(item);
    }

    result.push(capability(
        "lyrics.known",
        "Known lyrics",
        Source,
        vec![],
        vec![port("lyrics", Lyrics, false)],
    ));

    let mut transcript_fusion = capability(
        "fusion.transcript",
        "Transcript fusion",
        Fusion,
        vec![WorkflowPortSpec {
            id: "evidence".to_string(),
            port_type: TranscriptEvidence,
            required: true,
            multiple: true,
        }],
        vec![port("lyrics", Lyrics, false)],
    );
    transcript_fusion.hard_dependencies = vec![CapabilityId::new("analysis.asr")];
    result.push(transcript_fusion);

    let mut align = capability(
        "analysis.forced_alignment",
        "Forced alignment",
        Analyzer,
        vec![
            port("audio", Audio(Vocal), true),
            port("lyrics", Lyrics, true),
        ],
        vec![port("alignment", AlignmentEvidence, false)],
    );
    align.hard_dependencies = vec![CapabilityId::new("fusion.transcript")];
    result.push(align);

    let mut evidence_fusion = capability(
        "fusion.singing_evidence",
        "Singing evidence fusion",
        Fusion,
        vec![
            WorkflowPortSpec {
                id: "pitch".to_string(),
                port_type: PitchEvidence,
                required: true,
                multiple: true,
            },
            WorkflowPortSpec {
                id: "boundaries".to_string(),
                port_type: BoundaryEvidence,
                required: false,
                multiple: true,
            },
            port("alignment", AlignmentEvidence, false),
            port("techniques", TechniqueEvidence, false),
            port("acoustic", AcousticEvidence, false),
        ],
        vec![port("evidence", EvidenceBundle, false)],
    );
    evidence_fusion.hard_dependencies = vec![CapabilityId::new("analysis.pitch_f0")];
    result.push(evidence_fusion);

    let mut candidate = capability(
        "fusion.candidate_graph",
        "Candidate graph",
        Fusion,
        vec![port("evidence", EvidenceBundle, true)],
        vec![port("candidates", CandidateGraph, false)],
    );
    candidate.hard_dependencies = vec![CapabilityId::new("fusion.singing_evidence")];
    result.push(candidate);

    let mut canonical = capability(
        "finalize.canonical_singing_track",
        "Canonical singing track",
        Finalization,
        vec![
            port("candidates", CandidateGraph, true),
            port("lyrics", Lyrics, false),
        ],
        vec![
            port("track", CanonicalSingingTrack, false),
            port("chart", CandidateChart, false),
        ],
    );
    canonical.hard_dependencies = vec![CapabilityId::new("fusion.candidate_graph")];
    result.push(canonical);

    result
}
