use std::collections::BTreeMap;

use crate::audio_model::{DEFAULT_BGM_MODEL_ID, DEFAULT_VOCAL_MODEL_ID};

use super::validation::edge;
use super::{
    AnalyzerBinding, CapabilityId, ExecutionPolicy, QualityMode, WORKFLOW_SCHEMA_VERSION,
    WorkflowDefinition, WorkflowId, WorkflowNodeId, WorkflowNodeInstance, WorkflowPortRef,
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
        separation_strategy: None,
        parameters: BTreeMap::new(),
        execution_policy,
        priority,
        skip_if_unchanged: false,
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

pub fn default_workflow(file_hash: &str) -> WorkflowDefinition {
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
            Some(DEFAULT_VOCAL_MODEL_ID),
            ExecutionPolicy::Always,
            900,
        ),
    ];
    nodes[1].separation_strategy = Some(super::SeparationStrategyV1::LeapDualOutput);
    // Separation is the expensive, stable Step 1 boundary. New workflows
    // reuse its lossless vocal/instrumental pair whenever source, models and
    // parameters still match; later analysis stages remain fresh by default.
    nodes[1].skip_if_unchanged = true;
    debug_assert_eq!(DEFAULT_VOCAL_MODEL_ID, "bs_roformer_leap_xe90_vocals");
    debug_assert_eq!(DEFAULT_BGM_MODEL_ID, "bs_roformer_leap_xe90_vocals");
    let mut edges = vec![edge("source", "mix", "vocal_bgm_split", "audio")];

    nodes.push(node(
        "lead_isolate",
        "audio.lead_isolate",
        Some("melband_roformer_harmony"),
        ExecutionPolicy::Disabled,
        880,
    ));
    // Reusable like separation above: a quality profile that enables this
    // node (e.g. "maximum") pays for it once and, on an unchanged
    // source/model/parameters, reuses it on every later run instead of
    // repeating the GPU work -- confirmed against real production songs
    // whose downstream stages (ASR, forced alignment) kept failing and
    // retrying while this and the two cleanup nodes below silently redid
    // their own already-paid-for output every single attempt.
    nodes.last_mut().unwrap().skip_if_unchanged = true;
    edges.push(edge("vocal_bgm_split", "vocal", "lead_isolate", "audio"));
    nodes.push(node(
        "vocal_cleanup_1",
        "audio.denoise",
        Some("melband_roformer_denoise_aufr33"),
        ExecutionPolicy::Disabled,
        850,
    ));
    nodes.last_mut().unwrap().skip_if_unchanged = true;
    edges.push(edge("lead_isolate", "lead", "vocal_cleanup_1", "audio"));
    nodes.push(node(
        "vocal_dereverb_1",
        "audio.dereverb",
        Some("melband_roformer_dereverb_anvuew"),
        ExecutionPolicy::Disabled,
        840,
    ));
    nodes.last_mut().unwrap().skip_if_unchanged = true;
    edges.push(edge(
        "vocal_cleanup_1",
        "audio",
        "vocal_dereverb_1",
        "audio",
    ));
    let vocal_tail = ("vocal_dereverb_1".to_string(), "audio".to_string());

    nodes.extend([
        node(
            "asr_qwen",
            "analysis.asr",
            Some("qwen3_asr_1_7b"),
            ExecutionPolicy::Always,
            760,
        ),
        node(
            "asr_firered",
            "analysis.asr",
            Some("firered_asr2_aed"),
            ExecutionPolicy::Conditional {
                condition: super::ConditionalExecution::MaximumOnly,
            },
            755,
        ),
        node(
            "transcript_fusion",
            "fusion.transcript",
            None,
            ExecutionPolicy::Always,
            750,
        ),
        node(
            "align_qwen",
            "analysis.forced_alignment",
            Some("qwen3_forced_aligner_0_6b"),
            ExecutionPolicy::Always,
            740,
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
                condition: super::ConditionalExecution::MaximumOnly,
            },
            670,
        ),
        node(
            "game_notes",
            "analysis.note_boundary",
            Some("game_1_0_3_medium"),
            ExecutionPolicy::Always,
            660,
        ),
        node(
            "basic_pitch_onsets",
            "analysis.note_boundary",
            Some("basic_pitch"),
            ExecutionPolicy::Conditional {
                condition: super::ConditionalExecution::OnDisagreement,
            },
            650,
        ),
        node(
            "jbm555_notes",
            "analysis.note_boundary",
            Some("jbm555_cectc_80"),
            ExecutionPolicy::Conditional {
                condition: super::ConditionalExecution::MaximumOnly,
            },
            640,
        ),
        node(
            "stars_notes",
            "analysis.note_boundary",
            Some("stars"),
            ExecutionPolicy::Conditional {
                condition: super::ConditionalExecution::MaximumOnly,
            },
            630,
        ),
        node(
            "stars_technique",
            "analysis.technique",
            Some("stars"),
            ExecutionPolicy::Conditional {
                condition: super::ConditionalExecution::MaximumOnly,
            },
            620,
        ),
        node(
            "rosvot_notes",
            "analysis.note_boundary",
            Some("rosvot"),
            ExecutionPolicy::Conditional {
                condition: super::ConditionalExecution::MaximumOnly,
            },
            610,
        ),
        node(
            "acoustic_dsp",
            "analysis.acoustic_dsp",
            None,
            ExecutionPolicy::Always,
            600,
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
    nodes
        .iter_mut()
        .find(|node| node.instance_id.as_str() == "evidence_fusion")
        .expect("default workflow has evidence fusion")
        .parameters
        .insert(
            "fusion_mode".to_string(),
            serde_json::Value::String("algorithm".to_string()),
        );

    edges.extend([
        edge("asr_qwen", "transcript", "transcript_fusion", "evidence"),
        edge("asr_firered", "transcript", "transcript_fusion", "evidence"),
        edge("transcript_fusion", "lyrics", "align_qwen", "lyrics"),
        edge("transcript_fusion", "lyrics", "canonical_track", "lyrics"),
        edge("align_qwen", "alignment", "evidence_fusion", "alignment"),
        edge("f0_rmvpe", "pitch", "evidence_fusion", "pitch"),
        edge("f0_fcpe", "pitch", "evidence_fusion", "pitch"),
        edge("game_notes", "boundaries", "evidence_fusion", "boundaries"),
        edge(
            "basic_pitch_onsets",
            "boundaries",
            "evidence_fusion",
            "boundaries",
        ),
        edge(
            "jbm555_notes",
            "boundaries",
            "evidence_fusion",
            "boundaries",
        ),
        edge("stars_notes", "boundaries", "evidence_fusion", "boundaries"),
        edge(
            "rosvot_notes",
            "boundaries",
            "evidence_fusion",
            "boundaries",
        ),
        edge(
            "stars_technique",
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
    ]);

    let analyzer_bindings = [
        "asr_qwen",
        "asr_firered",
        "align_qwen",
        "f0_rmvpe",
        "f0_fcpe",
        "game_notes",
        "basic_pitch_onsets",
        "jbm555_notes",
        "stars_notes",
        "stars_technique",
        "rosvot_notes",
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
