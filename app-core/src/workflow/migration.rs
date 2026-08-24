use std::collections::BTreeMap;

use crate::audio_processing::{AudioProcessingSettings, cleanup_model_enabled};

use super::validation::edge;
use super::{
    AnalyzerBinding, CapabilityId, ConditionalExecution, ExecutionPolicy, QualityMode,
    WORKFLOW_SCHEMA_VERSION, WorkflowDefinition, WorkflowId, WorkflowNodeId, WorkflowNodeInstance,
    WorkflowPortRef,
};

fn node(
    id: &str,
    capability: &str,
    model_id: Option<&str>,
    execution_policy: ExecutionPolicy,
    priority: i32,
) -> WorkflowNodeInstance {
    WorkflowNodeInstance {
        instance_id: WorkflowNodeId::new(id),
        capability_id: CapabilityId::new(capability),
        model_id: model_id.map(str::to_string),
        parameters: BTreeMap::new(),
        execution_policy,
        priority,
    }
}

fn binding(analyzer: &str, source_node: &str, source_port: &str) -> AnalyzerBinding {
    AnalyzerBinding {
        analyzer_node: WorkflowNodeId::new(analyzer),
        source: WorkflowPortRef {
            node: WorkflowNodeId::new(source_node),
            port: source_port.to_string(),
        },
        analyzer_input: "audio".to_string(),
    }
}

pub fn workflow_from_audio_settings(
    file_hash: &str,
    settings: &AudioProcessingSettings,
) -> WorkflowDefinition {
    let mut nodes = vec![
        node(
            "source",
            "audio.source",
            None,
            ExecutionPolicy::Always,
            1000,
        ),
        node(
            "vocal_bgm_split",
            "audio.separate_vocal_bgm",
            settings.vocal_model_id.as_deref(),
            ExecutionPolicy::Always,
            900,
        ),
    ];
    if let Some(split) = nodes.get_mut(1) {
        if let Some(model) = settings.accompaniment_model_id.as_deref() {
            split.parameters.insert(
                "instrumental_model_id".to_string(),
                serde_json::Value::String(model.to_string()),
            );
        }
        if let Some(model) = settings.karaoke_model_id.as_deref() {
            split.parameters.insert(
                "karaoke_model_id".to_string(),
                serde_json::Value::String(model.to_string()),
            );
        }
        if let Some(model) = settings.multistem_model_id.as_deref() {
            split.parameters.insert(
                "multistem_model_id".to_string(),
                serde_json::Value::String(model.to_string()),
            );
        }
    }
    let mut edges = vec![edge("source", "mix", "vocal_bgm_split", "audio")];

    nodes.push(node(
        "lead_isolate",
        "audio.lead_isolate",
        Some("melband_roformer_harmony"),
        ExecutionPolicy::Always,
        880,
    ));
    edges.push(edge("vocal_bgm_split", "vocal", "lead_isolate", "audio"));
    let mut vocal_tail = ("lead_isolate".to_string(), "lead".to_string());
    for (index, model_id) in settings
        .vocal_cleanup_chain
        .iter()
        .filter(|model_id| cleanup_model_enabled(model_id))
        .enumerate()
    {
        let id = format!("vocal_cleanup_{}", index + 1);
        let capability = if model_id.contains("denoise") {
            "audio.denoise"
        } else {
            "audio.dereverb"
        };
        nodes.push(node(
            &id,
            capability,
            Some(model_id),
            ExecutionPolicy::Always,
            850 - index as i32,
        ));
        edges.push(edge(&vocal_tail.0, &vocal_tail.1, &id, "audio"));
        vocal_tail = (id, "audio".to_string());
    }

    let mut bgm_tail = ("vocal_bgm_split".to_string(), "instrumental".to_string());
    for (index, model_id) in settings
        .accompaniment_cleanup_chain
        .iter()
        .filter(|model_id| cleanup_model_enabled(model_id))
        .enumerate()
    {
        let id = format!("bgm_cleanup_{}", index + 1);
        let capability = if model_id.contains("denoise") {
            "audio.denoise"
        } else {
            "audio.dereverb"
        };
        nodes.push(node(
            &id,
            capability,
            Some(model_id),
            ExecutionPolicy::Always,
            840 - index as i32,
        ));
        edges.push(edge(&bgm_tail.0, &bgm_tail.1, &id, "audio"));
        bgm_tail = (id, "audio".to_string());
    }

    nodes.extend([
        node(
            "asr_qwen",
            "analysis.asr",
            Some("qwen3_asr_1_7b"),
            ExecutionPolicy::Always,
            700,
        ),
        node(
            "asr_firered",
            "analysis.asr",
            Some("firered_asr2_aed"),
            ExecutionPolicy::Conditional {
                condition: ConditionalExecution::OnDisagreement,
            },
            690,
        ),
        node(
            "transcript_fusion",
            "fusion.transcript",
            None,
            ExecutionPolicy::Always,
            600,
        ),
        node(
            "forced_alignment",
            "analysis.forced_alignment",
            Some("qwen3_forced_aligner_0_6b"),
            ExecutionPolicy::Always,
            590,
        ),
        node(
            "f0_rmvpe",
            "analysis.pitch_f0",
            Some("rmvpe"),
            ExecutionPolicy::Always,
            680,
        ),
        node(
            "f0_fcpe",
            "analysis.pitch_f0",
            Some("fcpe"),
            ExecutionPolicy::Conditional {
                condition: ConditionalExecution::DisagreementWindows,
            },
            670,
        ),
        node(
            "boundary_game",
            "analysis.note_boundary",
            Some("game"),
            ExecutionPolicy::Always,
            660,
        ),
        node(
            "boundary_basic_pitch",
            "analysis.note_boundary",
            Some("basic_pitch"),
            ExecutionPolicy::Conditional {
                condition: ConditionalExecution::OnDisagreement,
            },
            655,
        ),
        node(
            "boundary_rosvot",
            "analysis.note_boundary",
            Some("rosvot"),
            ExecutionPolicy::Conditional {
                condition: ConditionalExecution::MaximumOnly,
            },
            645,
        ),
        node(
            "boundary_stars",
            "analysis.note_boundary",
            Some("stars"),
            ExecutionPolicy::Conditional {
                condition: ConditionalExecution::MaximumOnly,
            },
            640,
        ),
        node(
            "technique_stars",
            "analysis.technique",
            Some("stars"),
            ExecutionPolicy::Conditional {
                condition: ConditionalExecution::MaximumOnly,
            },
            635,
        ),
        node(
            "acoustic_dsp",
            "analysis.acoustic_dsp",
            None,
            ExecutionPolicy::Always,
            650,
        ),
        node(
            "evidence_fusion",
            "fusion.singing_evidence",
            None,
            ExecutionPolicy::Always,
            500,
        ),
        node(
            "candidate_graph",
            "fusion.candidate_graph",
            None,
            ExecutionPolicy::Always,
            400,
        ),
        node(
            "canonical_track",
            "finalize.canonical_singing_track",
            None,
            ExecutionPolicy::Always,
            300,
        ),
    ]);

    edges.extend([
        edge("asr_qwen", "transcript", "transcript_fusion", "evidence"),
        edge("asr_firered", "transcript", "transcript_fusion", "evidence"),
        edge("transcript_fusion", "lyrics", "forced_alignment", "lyrics"),
        edge("f0_rmvpe", "pitch", "evidence_fusion", "pitch"),
        edge("f0_fcpe", "pitch", "evidence_fusion", "pitch"),
        edge(
            "boundary_game",
            "boundaries",
            "evidence_fusion",
            "boundaries",
        ),
        edge(
            "boundary_basic_pitch",
            "boundaries",
            "evidence_fusion",
            "boundaries",
        ),
        edge(
            "boundary_rosvot",
            "boundaries",
            "evidence_fusion",
            "boundaries",
        ),
        edge(
            "boundary_stars",
            "boundaries",
            "evidence_fusion",
            "boundaries",
        ),
        edge(
            "forced_alignment",
            "alignment",
            "evidence_fusion",
            "alignment",
        ),
        edge(
            "technique_stars",
            "techniques",
            "evidence_fusion",
            "techniques",
        ),
        edge("acoustic_dsp", "acoustic", "evidence_fusion", "acoustic"),
        edge("evidence_fusion", "evidence", "candidate_graph", "evidence"),
        edge(
            "candidate_graph",
            "candidates",
            "canonical_track",
            "candidates",
        ),
        edge("transcript_fusion", "lyrics", "canonical_track", "lyrics"),
    ]);

    let analyzer_bindings = [
        "asr_qwen",
        "asr_firered",
        "forced_alignment",
        "f0_rmvpe",
        "f0_fcpe",
        "boundary_game",
        "boundary_basic_pitch",
        "boundary_rosvot",
        "boundary_stars",
        "technique_stars",
        "acoustic_dsp",
    ]
    .into_iter()
    .map(|analyzer| binding(analyzer, &vocal_tail.0, &vocal_tail.1))
    .collect();

    WorkflowDefinition {
        schema_version: WORKFLOW_SCHEMA_VERSION,
        workflow_id: WorkflowId(format!("song:{file_hash}:workflow")),
        revision: 1,
        quality_mode: QualityMode::Balanced,
        preset_id: Some("balanced-v1".to_string()),
        nodes,
        edges,
        analyzer_bindings,
    }
}

pub fn default_workflow(file_hash: &str) -> WorkflowDefinition {
    workflow_from_audio_settings(
        file_hash,
        &AudioProcessingSettings::from_legacy_separator("karaoke"),
    )
}
