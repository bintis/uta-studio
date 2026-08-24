use crate::studio::*;

pub(crate) fn spawn_preview_request_summary(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    draft: &PlanPreviewDraft,
    preview: &app_core::EngineRunPreview,
) {
    spawn_text(parent, font.clone(), "EXACT REQUEST", 8.0, theme.primary);
    let quality = draft
        .effective_settings
        .as_ref()
        .map(|effective| format!("{:?}", effective.quality_profile.value))
        .unwrap_or_else(|| "Unavailable".to_string());
    for line in [
        format!(
            "Quality · {quality} · Source: {}",
            preview_quality_source(draft)
        ),
        format!(
            "TrueSource · {:?} · testing policy",
            preview.engine_plan.source_route.input_role
        ),
        format!(
            "Requested outputs · {}",
            preview
                .engine_plan
                .requested_outputs
                .iter()
                .map(|output| artifact_product_label(output))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ] {
        spawn_wrapped_text(parent, font.clone(), line, 9.0, theme.foreground);
    }
    if let Some(workflow) = preview.engine_plan.workflow_execution.as_ref() {
        spawn_wrapped_text(
            parent,
            font,
            format!(
                "Workflow identity · {} · revision {} · schema {} · digest {}",
                workflow.identity.workflow_id,
                workflow.identity.workflow_revision,
                workflow.identity.workflow_schema_version,
                workflow.identity.definition_digest,
            ),
            9.0,
            theme.foreground,
        );
    }
}
