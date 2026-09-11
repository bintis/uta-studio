//! Advisory whole-model placement for Super acceleration.
//!
//! Estimates seed the scheduler from complete-task observations on the target
//! Intel discrete GPU and AMD integrated GPU. They choose a route; they are not
//! acceptance thresholds and never authorize CPU inference or split one model.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use uta_runtime_manager::{NativeBackend, NativeDeviceClass};

const ESTIMATE_SOURCE: &str = "measured_complete_task_seed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DeviceLane {
    IntelDiscrete,
    AmdIntegrated,
}

impl DeviceLane {
    fn backend(self) -> NativeBackend {
        match self {
            Self::IntelDiscrete => NativeBackend::LibtorchXpu,
            Self::AmdIntegrated => NativeBackend::Ggml,
        }
    }

    fn device(self) -> NativeDeviceClass {
        match self {
            Self::IntelDiscrete => NativeDeviceClass::Gpu,
            Self::AmdIntegrated => NativeDeviceClass::IntegratedGpu,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::IntelDiscrete => "intel_discrete_xpu",
            Self::AmdIntegrated => "amd_integrated_vulkan",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SchedulingPhase {
    AudioPreparation,
    IndependentEvidence,
    ConditionedEvidence,
}

#[derive(Debug, Clone, Copy)]
struct TaskProfile {
    model_id: &'static str,
    phase: SchedulingPhase,
    dependencies: &'static [&'static str],
    intel_millis: u64,
    amd_millis: u64,
}

impl TaskProfile {
    fn estimate(self, lane: DeviceLane) -> u64 {
        match lane {
            DeviceLane::IntelDiscrete => self.intel_millis,
            DeviceLane::AmdIntegrated => self.amd_millis,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ModelPlacement {
    pub model_id: String,
    pub backend: NativeBackend,
    pub device: NativeDeviceClass,
    pub lane: String,
    pub predicted_ready_millis: u64,
    pub predicted_finish_millis: u64,
    pub estimate_source: &'static str,
}

impl ModelPlacement {
    pub(crate) fn candidate_backends(&self) -> [NativeBackend; 2] {
        match self.backend {
            NativeBackend::LibtorchXpu => [NativeBackend::LibtorchXpu, NativeBackend::Ggml],
            NativeBackend::Ggml => [NativeBackend::Ggml, NativeBackend::LibtorchXpu],
        }
    }
}

pub(crate) fn schedule_models<'a>(
    model_ids: impl IntoIterator<Item = &'a str>,
) -> Vec<ModelPlacement> {
    let requested = model_ids.into_iter().collect::<BTreeSet<_>>();
    let mut placements = Vec::new();
    let mut finishes = BTreeMap::<&str, u64>::new();

    for phase in [
        SchedulingPhase::AudioPreparation,
        SchedulingPhase::IndependentEvidence,
        SchedulingPhase::ConditionedEvidence,
    ] {
        let mut lane_available = BTreeMap::from([
            (DeviceLane::IntelDiscrete, 0_u64),
            (DeviceLane::AmdIntegrated, 0_u64),
        ]);
        let mut remaining = profiles()
            .iter()
            .copied()
            .filter(|profile| profile.phase == phase && requested.contains(profile.model_id))
            .collect::<Vec<_>>();

        while !remaining.is_empty() {
            let selected = remaining
                .iter()
                .position(|profile| {
                    profile
                        .dependencies
                        .iter()
                        .all(|dependency| !requested.contains(dependency) || finishes.contains_key(dependency))
                })
                .unwrap_or(0);
            let profile = remaining.remove(selected);
            let dependency_ready = profile
                .dependencies
                .iter()
                .filter_map(|dependency| finishes.get(dependency))
                .copied()
                .max()
                .unwrap_or(0);
            let (lane, start, finish) = [DeviceLane::IntelDiscrete, DeviceLane::AmdIntegrated]
                .into_iter()
                .map(|lane| {
                    let start = dependency_ready.max(lane_available[&lane]);
                    let finish = start.saturating_add(profile.estimate(lane));
                    (lane, start, finish)
                })
                .min_by_key(|(lane, _, finish)| (*finish, *lane))
                .expect("scheduler always has two GPU lanes");
            lane_available.insert(lane, finish);
            finishes.insert(profile.model_id, finish);
            placements.push(ModelPlacement {
                model_id: profile.model_id.to_string(),
                backend: lane.backend(),
                device: lane.device(),
                lane: lane.label().to_string(),
                predicted_ready_millis: start,
                predicted_finish_millis: finish,
                estimate_source: ESTIMATE_SOURCE,
            });
        }
    }

    placements
}

pub(crate) fn placement_for(model_id: &str) -> ModelPlacement {
    schedule_models([model_id])
        .into_iter()
        .next()
        .unwrap_or_else(|| ModelPlacement {
            model_id: model_id.to_string(),
            backend: NativeBackend::Ggml,
            device: NativeDeviceClass::IntegratedGpu,
            lane: DeviceLane::AmdIntegrated.label().to_string(),
            predicted_ready_millis: 0,
            predicted_finish_millis: 60_000,
            estimate_source: ESTIMATE_SOURCE,
        })
}

fn profiles() -> &'static [TaskProfile] {
    &[
        // Audio preparation remains a serial dependency chain. Complete heavy
        // models strongly favor the B580 LibTorch XPU route in current evidence.
        profile("bs_roformer_leap_xe90_vocals", SchedulingPhase::AudioPreparation, &[], 24_140, 180_000),
        profile("bs_roformer_leap_xe90_instrumental", SchedulingPhase::AudioPreparation, &[], 24_140, 180_000),
        profile("bs_polarformer_public_instrumental", SchedulingPhase::AudioPreparation, &[], 9_100, 120_000),
        profile("melband_roformer_harmony", SchedulingPhase::AudioPreparation, &[], 7_500, 120_000),
        profile("melband_roformer_denoise_aufr33", SchedulingPhase::AudioPreparation, &[], 7_490, 120_000),
        profile("melband_roformer_dereverb_anvuew", SchedulingPhase::AudioPreparation, &[], 7_500, 120_000),
        // Once prepared vocal audio exists, Intel runs the large speech models
        // while the AMD queue consumes short complete pitch/note tasks.
        profile("qwen3_asr_1_7b", SchedulingPhase::IndependentEvidence, &[], 6_400, 48_000),
        profile("firered_asr2_aed", SchedulingPhase::IndependentEvidence, &["qwen3_asr_1_7b"], 6_080, 40_000),
        profile("rmvpe", SchedulingPhase::IndependentEvidence, &[], 2_280, 503),
        profile("fcpe", SchedulingPhase::IndependentEvidence, &[], 1_680, 201),
        profile("basic_pitch", SchedulingPhase::IndependentEvidence, &[], 1_930, 370),
        profile("game_1_0_3_small", SchedulingPhase::IndependentEvidence, &[], 2_000, 648),
        profile("game_1_0_3_medium", SchedulingPhase::IndependentEvidence, &[], 2_350, 1_211),
        profile("game_1_0_3_large", SchedulingPhase::IndependentEvidence, &[], 4_000, 2_282),
        profile("jbm555_cectc_80", SchedulingPhase::IndependentEvidence, &[], 2_000, 331),
        profile("qwen3_forced_aligner_0_6b", SchedulingPhase::ConditionedEvidence, &["qwen3_asr_1_7b"], 1_200, 8_000),
        profile("stars", SchedulingPhase::ConditionedEvidence, &["rmvpe", "qwen3_forced_aligner_0_6b"], 700, 811),
        profile("rosvot", SchedulingPhase::ConditionedEvidence, &["rmvpe", "qwen3_forced_aligner_0_6b"], 300, 281),
    ]
}

const fn profile(
    model_id: &'static str,
    phase: SchedulingPhase,
    dependencies: &'static [&'static str],
    intel_millis: u64,
    amd_millis: u64,
) -> TaskProfile {
    TaskProfile {
        model_id,
        phase,
        dependencies,
        intel_millis,
        amd_millis,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavy_and_independent_light_models_fill_distinct_gpu_queues() {
        let schedule = schedule_models([
            "qwen3_asr_1_7b",
            "rmvpe",
            "fcpe",
            "basic_pitch",
            "game_1_0_3_medium",
        ]);
        let placement = |model_id: &str| {
            schedule
                .iter()
                .find(|placement| placement.model_id == model_id)
                .unwrap()
        };
        assert_eq!(
            placement("qwen3_asr_1_7b").backend,
            NativeBackend::LibtorchXpu
        );
        for model_id in ["rmvpe", "fcpe", "basic_pitch", "game_1_0_3_medium"] {
            assert_eq!(placement(model_id).backend, NativeBackend::Ggml);
            assert_eq!(
                placement(model_id).device,
                NativeDeviceClass::IntegratedGpu
            );
        }
    }

    #[test]
    fn dependencies_delay_conditioned_tasks_without_splitting_a_model() {
        let schedule = schedule_models([
            "qwen3_asr_1_7b",
            "qwen3_forced_aligner_0_6b",
            "rmvpe",
            "stars",
        ]);
        let placement = |model_id: &str| {
            schedule
                .iter()
                .find(|placement| placement.model_id == model_id)
                .unwrap()
        };
        let stars = placement("stars");
        assert!(
            stars.predicted_ready_millis
                >= placement("qwen3_forced_aligner_0_6b").predicted_finish_millis
        );
        assert!(stars.predicted_ready_millis >= placement("rmvpe").predicted_finish_millis);
        assert!(matches!(
            stars.device,
            NativeDeviceClass::Gpu | NativeDeviceClass::IntegratedGpu
        ));
    }

    #[test]
    fn unknown_models_stay_on_a_gpu_and_never_select_cpu() {
        let placement = placement_for("future_model");
        assert_eq!(placement.backend, NativeBackend::Ggml);
        assert_eq!(placement.device, NativeDeviceClass::IntegratedGpu);
    }
}
