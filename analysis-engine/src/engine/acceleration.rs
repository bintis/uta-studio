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
        for node in &plan.execution_nodes {
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
