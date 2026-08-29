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

const EP317_DUAL_ROLES: &[SeparationOutputRoleV1] = &[
    SeparationOutputRoleV1::Vocal,
    SeparationOutputRoleV1::Instrumental,
];
const VOCAL_ROLE: &[SeparationOutputRoleV1] = &[SeparationOutputRoleV1::Vocal];
const INSTRUMENTAL_ROLE: &[SeparationOutputRoleV1] = &[SeparationOutputRoleV1::Instrumental];
const EP317_DUAL_EXECUTIONS: &[SeparationProviderExecutionV1] = &[SeparationProviderExecutionV1 {
    provider_id: "bs_roformer_vocals_ep317",
    output_roles: EP317_DUAL_ROLES,
}];
const SPECIALIST_EXECUTIONS: &[SeparationProviderExecutionV1] = &[
    SeparationProviderExecutionV1 {
        provider_id: "bs_roformer_vocals_ep317",
        output_roles: VOCAL_ROLE,
    },
    SeparationProviderExecutionV1 {
        provider_id: "melband_roformer_inst_v2",
        output_roles: INSTRUMENTAL_ROLE,
    },
];
const SEPARATION_STRATEGIES: &[SeparationStrategyOptionV1] = &[
    SeparationStrategyOptionV1 {
        strategy: SeparationStrategyV1::Ep317VocalResidual,
        label: "EP317 vocal + residual Instrumental",
        description: "One EP317 inference estimates GuideVocals; the same native invocation publishes SourceMix − GuideVocals as the deterministic Instrumental residual.",
        executions: EP317_DUAL_EXECUTIONS,
    },
    SeparationStrategyOptionV1 {
        strategy: SeparationStrategyV1::IndependentSpecialists,
        label: "Independent vocal + Instrumental specialists",
        description: "EP317 extracts GuideVocals and MelBand Inst V2 independently extracts Instrumental, with separate progress and logs.",
        executions: SPECIALIST_EXECUTIONS,
    },
];

pub fn separation_strategy_options() -> &'static [SeparationStrategyOptionV1] {
    SEPARATION_STRATEGIES
}

pub fn separation_strategy_descriptor(
    strategy: SeparationStrategyV1,
) -> &'static SeparationStrategyOptionV1 {
    SEPARATION_STRATEGIES
        .iter()
        .find(|option| option.strategy == strategy)
        .expect("every typed separation strategy has a descriptor")
}

pub fn workflow_model_label(model_id: &str) -> &str {
    match model_id {
        "bs_roformer_vocals_ep317" => "BS-RoFormer Vocals EP317",
        "melband_roformer_inst_v2" => "MelBand-RoFormer Inst V2",
        "melband_roformer_harmony" => "MelBand-RoFormer Lead Isolation",
        "melband_roformer_denoise_aufr33" => "MelBand-RoFormer Denoise",
        "melband_roformer_dereverb_anvuew" => "MelBand-RoFormer Dereverb",
        "qwen3_asr_1_7b" => "Qwen3-ASR 1.7B",
        "firered_asr2_aed" => "FireRedASR2-AED",
        "qwen3_forced_aligner_0_6b" => "Qwen3 Forced Aligner 0.6B",
        "rmvpe" => "RMVPE",
        "fcpe" => "FCPE",
        "game" => "GAME",
        "basic_pitch" => "Basic Pitch",
        "rosvot" => "ROSVOT",
        "stars" => "STARS",
        other => other,
    }
}

/// Exact Engine-v1 provider choices that are interchangeable inside one
/// Processing Studio capability card. Fixed-role and single-provider
/// capabilities return no choices so the desktop does not render a fake selector.
pub fn workflow_model_options(capability_id: &CapabilityId) -> &'static [WorkflowModelOption] {
    const PITCH: &[WorkflowModelOption] = &[
        WorkflowModelOption {
            model_id: "rmvpe",
            label: "RMVPE",
        },
        WorkflowModelOption {
            model_id: "fcpe",
            label: "FCPE",
        },
    ];
    const NOTE_BOUNDARY: &[WorkflowModelOption] = &[
        WorkflowModelOption {
            model_id: "game",
            label: "GAME",
        },
        WorkflowModelOption {
            model_id: "basic_pitch",
            label: "Basic Pitch",
        },
        WorkflowModelOption {
            model_id: "rosvot",
            label: "ROSVOT",
        },
        WorkflowModelOption {
            model_id: "stars",
            label: "STARS",
        },
    ];
    const EMPTY: &[WorkflowModelOption] = &[];

    match capability_id.as_str() {
        "analysis.pitch_f0" => PITCH,
        "analysis.note_boundary" => NOTE_BOUNDARY,
        _ => EMPTY,
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
            port("alignment", AlignmentEvidence, true),
            port("techniques", TechniqueEvidence, false),
            port("acoustic", AcousticEvidence, false),
        ],
        vec![port("evidence", EvidenceBundle, false)],
    );
    evidence_fusion.hard_dependencies = vec![
        CapabilityId::new("analysis.pitch_f0"),
        CapabilityId::new("analysis.forced_alignment"),
    ];
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
            port("lyrics", Lyrics, true),
        ],
        vec![
            port("track", CanonicalSingingTrack, false),
            port("chart", CandidateChart, false),
        ],
    );
    canonical.hard_dependencies = vec![
        CapabilityId::new("fusion.candidate_graph"),
        CapabilityId::new("fusion.transcript"),
    ];
    result.push(canonical);

    result
}
