use std::collections::{BTreeMap, HashMap};

use super::*;
use crate::backend_cli::{
    AnalysisLifecycleFrameWireV1, AnalysisPlanWireV1, ExecutionNodeWireV1, FusionModeWireV1,
};

const REFERENCE_SONG_MILLIS: u64 = 305_813;

#[derive(Debug, Clone)]
struct ProgressUnit {
    weight: u64,
    fraction: f32,
}

#[derive(Debug, Default)]
struct EngineProgressState {
    units: BTreeMap<String, ProgressUnit>,
    node_to_unit: HashMap<String, String>,
}

#[derive(Debug, Default)]
struct HistoricalWeights {
    by_model: HashMap<String, u64>,
    by_capability: HashMap<String, u64>,
}

static LIVE_ENGINE_PROGRESS: LazyLock<Mutex<HashMap<String, EngineProgressState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn register_engine_progress_plan(file_hash: &str, plan: &AnalysisPlanWireV1) {
    let history = historical_weights();
    let mut state = EngineProgressState::default();
    for node in &plan.execution_nodes {
        let invocation = planned_invocation(plan, node);
        let model = invocation
            .map(|invocation| invocation.provider_id.as_str())
            .or_else(|| default_model_for_capability(node.capability.as_str()));
        let unit_id = invocation
            .map(|invocation| format!("invocation:{}", invocation.invocation_id))
            .unwrap_or_else(|| format!("node:{}", node.id));
        state.node_to_unit.insert(node.id.clone(), unit_id.clone());
        state.units.entry(unit_id).or_insert_with(|| ProgressUnit {
            weight: estimated_weight(plan, node.capability.as_str(), model, &history),
            fraction: 0.0,
        });
    }
    if !state.units.is_empty() {
        LIVE_ENGINE_PROGRESS
            .lock()
            .unwrap()
            .insert(file_hash.to_string(), state);
    }
}

pub(super) fn remove_engine_progress_plan(file_hash: &str) {
    LIVE_ENGINE_PROGRESS.lock().unwrap().remove(file_hash);
}

/// Applies one real Engine lifecycle percentage to its planned execution unit
/// and returns whole-run weighted completion. This value is UI estimation, not
/// an execution contract, release gate, or substitute for result validation.
pub(super) fn update_engine_overall_progress(
    file_hash: &str,
    event: &AnalysisLifecycleFrameWireV1,
) -> Option<usize> {
    let mut states = LIVE_ENGINE_PROGRESS.lock().unwrap();
    let state = states.get_mut(file_hash)?;
    let invocation_unit = event
        .presentation_node_id
        .as_ref()
        .map(|invocation| format!("invocation:{invocation}"))
        .filter(|unit| state.units.contains_key(unit));
    let unit_id = invocation_unit.or_else(|| state.node_to_unit.get(&event.node_id).cloned())?;
    let unit = state.units.get_mut(&unit_id)?;
    let reported = match event.frame_type.as_str() {
        "node_completed" => Some(1.0),
        "node_started" => Some(0.0),
        "node_progress" => event.progress,
        _ => None,
    };
    if let Some(reported) = reported {
        // One invocation can expose more than one semantic node. Retain the
        // furthest measured point for that one real provider call.
        unit.fraction = unit.fraction.max(reported.clamp(0.0, 1.0));
    }
    Some(weighted_percent(state).min(99))
}

fn weighted_percent(state: &EngineProgressState) -> usize {
    let total = state.units.values().map(|unit| unit.weight).sum::<u64>();
    if total == 0 {
        return 0;
    }
    let completed = state
        .units
        .values()
        .map(|unit| unit.weight as f64 * f64::from(unit.fraction))
        .sum::<f64>();
    (completed * 100.0 / total as f64).floor().clamp(0.0, 100.0) as usize
}

fn planned_invocation<'a>(
    plan: &'a AnalysisPlanWireV1,
    node: &ExecutionNodeWireV1,
) -> Option<&'a crate::workflow::WorkflowExecutionInvocationWireV1> {
    plan.workflow_execution.as_ref().and_then(|workflow| {
        workflow
            .nodes
            .iter()
            .flat_map(|node| &node.execution_invocations)
            .find(|invocation| {
                invocation
                    .capabilities
                    .iter()
                    .any(|capability| capability == node.capability.as_str())
            })
    })
}

fn estimated_weight(
    plan: &AnalysisPlanWireV1,
    capability: &str,
    model: Option<&str>,
    history: &HistoricalWeights,
) -> u64 {
    model
        .and_then(|model| history.by_model.get(model).copied())
        .or_else(|| history.by_capability.get(capability).copied())
        .unwrap_or_else(|| fallback_weight(plan, capability, model))
        .max(1)
}

fn historical_weights() -> HistoricalWeights {
    let mut model_samples = HashMap::<String, Vec<u64>>::new();
    let mut capability_samples = HashMap::<String, Vec<u64>>::new();
    for run in load_analysis_history(500) {
        if run.status != "completed" {
            continue;
        }
        let Some(song) = library_db::load_song_by_hash(&run.file_hash).ok().flatten() else {
            continue;
        };
        let song_millis = (song.duration_secs * 1_000.0).round();
        if !song_millis.is_finite() || song_millis < 1.0 || song_millis > u64::MAX as f64 {
            continue;
        }
        let song_millis = song_millis as u64;
        for route in run.snapshot.stage_routes {
            if !matches!(
                route.node_event.as_deref(),
                Some("node_completed" | "completed")
            ) {
                continue;
            }
            let Some(elapsed) = route
                .started_at_ms
                .zip(route.finished_at_ms)
                .and_then(|(started, finished)| finished.checked_sub(started))
                .and_then(|elapsed| u64::try_from(elapsed).ok())
                .filter(|elapsed| *elapsed > 0)
            else {
                continue;
            };
            let normalized = elapsed
                .saturating_mul(REFERENCE_SONG_MILLIS)
                .checked_div(song_millis)
                .unwrap_or(elapsed)
                .max(1);
            if route.model != "Engine native" && !route.model.trim().is_empty() {
                model_samples
                    .entry(route.model)
                    .or_default()
                    .push(normalized);
            }
            if let Some(capability) = route.capability_id.filter(|value| !value.trim().is_empty()) {
                capability_samples
                    .entry(capability)
                    .or_default()
                    .push(normalized);
            }
        }
    }
    HistoricalWeights {
        by_model: model_samples
            .into_iter()
            .filter_map(|(key, samples)| median(samples).map(|median| (key, median)))
            .collect(),
        by_capability: capability_samples
            .into_iter()
            .filter_map(|(key, samples)| median(samples).map(|median| (key, median)))
            .collect(),
    }
}

fn median(mut samples: Vec<u64>) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    samples.sort_unstable();
    let middle = samples.len() / 2;
    if samples.len().is_multiple_of(2) {
        Some(samples[middle - 1].saturating_add(samples[middle]) / 2)
    } else {
        Some(samples[middle])
    }
}

/// Fallback work estimates are milliseconds on the repository's representative
/// ~305.8-second song. Model entries with accepted full-song measurements use
/// those measurements; Leap is extrapolated from its current 6-second run.
fn fallback_weight(plan: &AnalysisPlanWireV1, capability: &str, model: Option<&str>) -> u64 {
    match model {
        Some("bs_roformer_leap_xe90_vocals") => 1_358_000,
        Some("bs_polarformer_public_instrumental") => 208_000,
        Some("melband_roformer_inst_v2") => 174_533,
        Some("melband_roformer_harmony") => 151_525,
        Some("melband_roformer_denoise_aufr33") => 152_887,
        Some("melband_roformer_dereverb_anvuew") => 81_580,
        Some("qwen3_asr_1_7b") => 150_000,
        Some("qwen3_forced_aligner_0_6b") => 75_000,
        Some("firered_asr2_aed") => 87_660,
        Some("rmvpe") => 37_000,
        Some("game") => 19_950,
        Some("fcpe") => 3_560,
        Some("basic_pitch") => 3_800,
        Some("stars") => 17_450,
        Some("rosvot") => 13_050,
        Some("jbm555_cectc_80") => 18_000,
        _ => match capability {
            "audio.decode" => 8_000,
            "analysis.acoustic_dsp" => 10_000,
            "fusion.candidate_graph"
                if plan.workflow_execution.as_ref().is_some_and(|workflow| {
                    workflow.fusion_mode == FusionModeWireV1::AiJudgment
                }) =>
            {
                45_000
            }
            "fusion.singing" | "fusion.candidate_graph" => 8_000,
            "fusion.transcript" | "fusion.alignment" => 2_000,
            "rhythm.quantize" | "finalize.vocal_chart" => 3_000,
            _ => 5_000,
        },
    }
}

fn default_model_for_capability(capability: &str) -> Option<&'static str> {
    match capability {
        "audio.extract_vocals" => Some("bs_roformer_leap_xe90_vocals"),
        "audio.extract_instrumental" => Some("bs_polarformer_public_instrumental"),
        "audio.lead_isolate" => Some("melband_roformer_harmony"),
        "audio.denoise" => Some("melband_roformer_denoise_aufr33"),
        "audio.dereverb" => Some("melband_roformer_dereverb_anvuew"),
        "speech.transcribe" => Some("qwen3_asr_1_7b"),
        "speech.transcribe.challenger" => Some("firered_asr2_aed"),
        "speech.align" => Some("qwen3_forced_aligner_0_6b"),
        "pitch.track" | "pitch.secondary.rmvpe" => Some("rmvpe"),
        "pitch.secondary" | "pitch.secondary.fcpe" => Some("fcpe"),
        "notes.game" => Some("game"),
        "notes.basic_pitch" => Some("basic_pitch"),
        "notes.rosvot" => Some("rosvot"),
        "notes.stars" | "technique.analyze" => Some("stars"),
        "notes.jbm555" => Some("jbm555_cectc_80"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_progress_tracks_runtime_cost_instead_of_node_count() {
        let state = EngineProgressState {
            units: BTreeMap::from([
                (
                    "slow".to_string(),
                    ProgressUnit {
                        weight: 90,
                        fraction: 0.5,
                    },
                ),
                (
                    "fast".to_string(),
                    ProgressUnit {
                        weight: 10,
                        fraction: 1.0,
                    },
                ),
            ]),
            node_to_unit: HashMap::new(),
        };
        assert_eq!(weighted_percent(&state), 55);
    }

    #[test]
    fn lifecycle_progress_targets_the_exact_provider_invocation() {
        let file_hash = "weighted-provider-invocation-fixture";
        LIVE_ENGINE_PROGRESS.lock().unwrap().insert(
            file_hash.to_string(),
            EngineProgressState {
                units: BTreeMap::from([(
                    "invocation:notes.stars".to_string(),
                    ProgressUnit {
                        weight: 100,
                        fraction: 0.0,
                    },
                )]),
                node_to_unit: HashMap::from([(
                    "stars".to_string(),
                    "invocation:notes.stars".to_string(),
                )]),
            },
        );
        let event = AnalysisLifecycleFrameWireV1 {
            frame_type: "node_progress".to_string(),
            schema_version: 1,
            request_id: "request".to_string(),
            node_id: "stars".to_string(),
            presentation_node_id: Some("notes.stars".to_string()),
            capability_id: "notes.stars".to_string(),
            model_id: Some("stars".to_string()),
            implementation: "openvino".to_string(),
            progress: Some(0.37),
            work_units_completed: None,
            work_units_total: None,
            worker_task_id: None,
            artifact: None,
            message: None,
            event_at_ms: 1,
        };
        assert_eq!(update_engine_overall_progress(file_hash, &event), Some(37));
        remove_engine_progress_plan(file_hash);
    }

    #[test]
    fn fallback_estimates_rank_heavy_models_above_fast_evidence_models() {
        let plan: AnalysisPlanWireV1 = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"request",
            "source_route":{"primary_source_id":"source","input_role":"original_mix","preparation":[]},
            "requested_outputs":[],"required_capabilities":[],"optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":[],
            "fallback_policy":[],"artifact_declarations":[]
        }))
        .unwrap();
        assert!(
            fallback_weight(
                &plan,
                "audio.extract_vocals",
                Some("bs_roformer_leap_xe90_vocals")
            ) > fallback_weight(&plan, "pitch.secondary.fcpe", Some("fcpe"))
        );
        assert!(
            fallback_weight(
                &plan,
                "audio.extract_instrumental",
                Some("bs_polarformer_public_instrumental")
            ) > fallback_weight(&plan, "notes.game", Some("game"))
        );
    }

    #[test]
    fn median_uses_the_middle_of_observed_history() {
        assert_eq!(median(vec![900, 100, 300]), Some(300));
        assert_eq!(median(vec![400, 100, 300, 200]), Some(250));
    }
}
