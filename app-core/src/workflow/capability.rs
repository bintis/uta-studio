use serde::{Deserialize, Serialize};

use super::{AudioRole, CapabilityId, WorkflowPortSpec, WorkflowPortType};

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
            vec![port("audio", Audio(LeadVocal), true)],
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
            port("audio", Audio(LeadVocal), true),
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
                required: true,
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
        CapabilityId::new("analysis.note_boundary"),
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
