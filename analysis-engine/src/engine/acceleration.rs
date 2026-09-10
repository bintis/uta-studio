use super::*;
use crate::execution::{AccelerationGuard, PreloadSpec};

pub(super) fn scope(
    request: &AnalyzeRequest,
    plan: &EnginePlan,
    resolved: &[uta_runtime_manager::ResolvedModel],
    output: &Path,
    cancellation: &CancellationToken,
) -> AccelerationGuard {
    let mut schedule = Vec::new();
    if request.execution_policy.turbo_acceleration {
        let combined_separation = plan
            .execution_nodes
            .iter()
            .any(|node| node.capability.as_str() == "audio.extract_vocals");
        for node in &plan.execution_nodes {
            // Engine's existing combined separator publishes both artifacts
            // in its first invocation, not a second instrumental invocation.
            if !preload_capability(node.capability.as_str(), combined_separation) {
                continue;
            }
            for requirement in &plan.requirements.resources {
                if !requirement
                    .reason
                    .split(',')
                    .any(|reason| reason == node.capability.as_str())
                {
                    continue;
                }
                let Some(model) = resolved
                    .iter()
                    .find(|model| requirement.resource == format!("model:{}", model.model_id))
                else {
                    continue;
                };
                if !preload_model(&model.model_id, request.lyrics.language.as_deref()) {
                    continue;
                }
                if schedule
                    .last()
                    .is_some_and(|previous: &PreloadSpec| previous.model_id == model.model_id)
                {
                    continue;
                }
                match model_dispatch(model, request, "preload") {
                    Ok((_, config)) => schedule.push(PreloadSpec {
                        model_id: model.model_id.clone(),
                        executable: model.runtime_executable.clone(),
                        config,
                    }),
                    Err(error) => {
                        crate::debug_log::record("acceleration_route_unavailable", &error.message)
                    }
                }
            }
        }
    }
    AccelerationGuard::enter(
        request.execution_policy.turbo_acceleration,
        schedule,
        output,
        cancellation,
    )
}

fn preload_capability(capability: &str, combined_separation: bool) -> bool {
    capability != "audio.extract_instrumental" || !combined_separation
}

fn preload_model(model_id: &str, language: Option<&str>) -> bool {
    // This predicts optional residency, never changes execution eligibility.
    // A later detected language may still require an on-demand FireRed run.
    match model_id {
        "firered_asr2_aed" => firered_language_applicable(language, None),
        "stars" => stars_g2p_language_applicable(language),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combined_separation_is_preloaded_once_without_dropping_instrumental_only_work() {
        assert!(!preload_capability("audio.extract_instrumental", true));
        assert!(preload_capability("audio.extract_instrumental", false));
        assert!(preload_capability("audio.extract_vocals", true));
        assert!(preload_capability("audio.denoise", true));
    }
    #[test]
    fn prediction_uses_existing_language_rules_without_changing_later_execution() {
        assert!(!preload_model("firered_asr2_aed", Some("ja")));
        assert!(!preload_model("stars", Some("ja")));
        assert!(preload_model("qwen3_asr_1_7b", Some("ja")));
        assert!(preload_model("firered_asr2_aed", Some("en")));
        assert!(preload_model("stars", Some("zh-CN")));
        assert!(preload_model("firered_asr2_aed", None));
        assert!(firered_language_applicable(Some("ja"), Some("en")));
    }
}
