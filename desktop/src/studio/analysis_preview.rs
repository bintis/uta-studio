use crate::studio::*;

fn analysis_route_summary(plan: &app_core::AnalysisPlanWireV1) -> String {
    let capabilities = plan
        .source_route
        .preparation
        .iter()
        .map(|capability| capability.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let enabled = |capability: &str| capabilities.contains(capability);
    let mut route = vec![format!("{:?}", plan.source_route.input_role)];
    for capability in &plan.source_route.preparation {
        match capability.as_str() {
            "audio.extract_vocals" => route.push("Vocal".to_string()),
            "audio.lead_isolate" => route.push("LeadVocal".to_string()),
            "audio.denoise" => route.push("Denoise".to_string()),
            "audio.dereverb" => route.push("Dereverb".to_string()),
            _ => {}
        }
    }
    format!(
        "Analyzer route · {} → analyzers · Lead isolation {} · Denoise {} · Dereverb {}",
        route.join(" → "),
        if enabled("audio.lead_isolate") {
            "On"
        } else {
            "Off"
        },
        if enabled("audio.denoise") {
            "On"
        } else {
            "Off"
        },
        if enabled("audio.dereverb") {
            "On"
        } else {
            "Off"
        },
    )
}

fn workflow_execution_topology(workflow: &app_core::WorkflowExecutionPlanWireV1) -> Vec<String> {
    workflow
        .nodes
        .iter()
        .flat_map(|node| {
            node.execution_invocations.iter().map(|invocation| {
                format!(
                    "{} · {} ({}) → {}",
                    invocation.invocation_id,
                    app_core::workflow_model_label(&invocation.provider_id),
                    invocation.provider_id,
                    invocation.output_ports.join(" + ")
                )
            })
        })
        .collect()
}

fn workflow_planned_evidence(workflow: &app_core::WorkflowExecutionPlanWireV1) -> Vec<String> {
    workflow
        .nodes
        .iter()
        .filter_map(|node| {
            let label =
                node.capabilities
                    .iter()
                    .find_map(|capability| match capability.as_str() {
                        "pitch.track" => Some("Primary continuous F0"),
                        "notes.game" => Some("GAME note regions"),
                        "pitch.secondary" | "pitch.secondary.fcpe" => Some("FCPE secondary F0"),
                        "pitch.secondary.rmvpe" => Some("RMVPE secondary F0"),
                        "notes.basic_pitch" => Some("Basic Pitch onset/note"),
                        "notes.rosvot" => Some("ROSVOT note challenger"),
                        "notes.stars" => Some("STARS note challenger"),
                        "technique.analyze" => Some("STARS technique context"),
                        "analysis.acoustic_dsp" => Some("Acoustic DSP context"),
                        _ => None,
                    })?;
            let state = match node.execution_state {
                app_core::WorkflowNodeExecutionStateWireV1::Ready => "ready",
                app_core::WorkflowNodeExecutionStateWireV1::Deferred => "conditional",
                app_core::WorkflowNodeExecutionStateWireV1::ProfileSkipped => "profile skipped",
                app_core::WorkflowNodeExecutionStateWireV1::Disabled
                | app_core::WorkflowNodeExecutionStateWireV1::NotRequested => return None,
            };
            Some(format!("{label} ({state})"))
        })
        .collect()
}

pub(crate) fn spawn_preview_request_summary(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    draft: &PlanPreviewDraft,
    preview: &app_core::EngineRunPreview,
) {
    spawn_text(parent, font.clone(), "RUN SUMMARY", 8.0, theme.primary);
    let quality = draft
        .effective_settings
        .as_ref()
        .map(|effective| format!("{:?}", effective.quality_profile.value))
        .unwrap_or_else(|| "Unavailable".to_string());
    for line in [
        format!("Quality · {quality} · {}", preview_quality_source(draft)),
        format!("Input · {:?}", preview.engine_plan.source_route.input_role),
        analysis_route_summary(&preview.engine_plan),
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
            font.clone(),
            format!(
                "Workflow · {} · revision {}",
                workflow.identity.workflow_id, workflow.identity.workflow_revision
            ),
            9.0,
            theme.muted_foreground,
        );
        for topology in workflow_execution_topology(workflow) {
            spawn_wrapped_text(
                parent,
                font.clone(),
                format!("Execution topology · {topology}"),
                9.0,
                theme.muted_foreground,
            );
        }
        let decision_mode = match workflow.fusion_mode {
            app_core::FusionModeWireV1::Algorithm => "Algorithm",
            app_core::FusionModeWireV1::AiJudgment => "AI judgment",
        };
        spawn_wrapped_text(
            parent,
            font.clone(),
            format!("Decision mode · {decision_mode}"),
            9.0,
            if workflow.fusion_mode == app_core::FusionModeWireV1::AiJudgment {
                theme.editor_warning
            } else {
                theme.primary
            },
        );
        if let Some(policy) = workflow.fusion_policy {
            let continuous_f0 = match policy.continuous_f0 {
                app_core::ContinuousF0SourceWireV1::Rmvpe => "RMVPE",
                app_core::ContinuousF0SourceWireV1::Fcpe => "FCPE",
            };
            let note_lengths = match policy.note_lengths {
                app_core::NoteLengthSourceWireV1::Game => "GAME note regions",
                app_core::NoteLengthSourceWireV1::F0Derived => {
                    "F0-derived fallback regions · review required"
                }
            };
            let onset_support = match policy.onset_support {
                app_core::OnsetSupportSourceWireV1::Automatic => "Automatic",
                app_core::OnsetSupportSourceWireV1::Acoustic => "Acoustic DSP",
                app_core::OnsetSupportSourceWireV1::BasicPitch => "Basic Pitch",
            };
            spawn_wrapped_text(
                parent,
                font.clone(),
                format!(
                    "Resolved fusion · F0 {continuous_f0} · {note_lengths} · onset {onset_support}"
                ),
                9.0,
                theme.primary,
            );
        }
        let planned_evidence = workflow_planned_evidence(workflow);
        if !planned_evidence.is_empty() {
            spawn_wrapped_text(
                parent,
                font,
                format!("Planned evidence · {}", planned_evidence.join(" · ")),
                8.0,
                theme.muted_foreground,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{analysis_route_summary, workflow_execution_topology, workflow_planned_evidence};

    #[test]
    fn plan_preview_keeps_an_independent_lead_stem_out_of_the_analyzer_route() {
        let plan: app_core::AnalysisPlanWireV1 =
            serde_json::from_value(serde_json::json!({
                "schema": "uta.analysis-engine.plan",
                "schema_version": 1,
                "request_id": "preview-route",
                "source_route": {
                    "primary_source_id": "main",
                    "input_role": "original_mix",
                    "preparation": ["audio.extract_vocals"]
                },
                "requested_outputs": ["pitch_evidence"],
                "required_capabilities": ["audio.extract_vocals", "pitch.track"],
                "optional_capabilities": [],
                "requirements": {"schema": "uta.analysis-engine.requirements", "schema_version": 1, "resources": []},
                "resolved_resources": [],
                "execution_nodes": [
                    {"id": "decode", "capability": "audio.decode", "required": true},
                    {"id": "extract", "capability": "audio.extract_vocals", "required": true, "depends_on": ["decode"]},
                    {"id": "lead-stem", "capability": "audio.lead_isolate", "required": true, "depends_on": ["extract"]},
                    {"id": "pitch", "capability": "pitch.track", "required": true, "depends_on": ["extract"]}
                ],
                "quality_gates": [],
                "fallback_policy": [],
                "artifact_declarations": []
            }))
            .unwrap();

        assert_eq!(
            analysis_route_summary(&plan),
            "Analyzer route · OriginalMix → Vocal → analyzers · Lead isolation Off · Denoise Off · Dereverb Off"
        );
    }

    #[test]
    fn plan_preview_preserves_the_exact_preparation_order() {
        let plan: app_core::AnalysisPlanWireV1 =
            serde_json::from_value(serde_json::json!({
                "schema": "uta.analysis-engine.plan",
                "schema_version": 1,
                "request_id": "preview-order",
                "source_route": {
                    "primary_source_id": "main",
                    "input_role": "original_mix",
                    "preparation": ["audio.extract_vocals", "audio.dereverb", "audio.denoise"]
                },
                "requested_outputs": ["pitch_evidence"],
                "required_capabilities": ["audio.extract_vocals", "audio.dereverb", "audio.denoise", "pitch.track"],
                "optional_capabilities": [],
                "requirements": {"schema": "uta.analysis-engine.requirements", "schema_version": 1, "resources": []},
                "resolved_resources": [],
                "execution_nodes": [],
                "quality_gates": [],
                "fallback_policy": [],
                "artifact_declarations": []
            }))
            .unwrap();

        assert_eq!(
            analysis_route_summary(&plan),
            "Analyzer route · OriginalMix → Vocal → Dereverb → Denoise → analyzers · Lead isolation Off · Denoise On · Dereverb On"
        );
    }

    #[test]
    fn plan_preview_names_all_evidence_present_in_the_exact_plan() {
        let workflow: app_core::WorkflowExecutionPlanWireV1 =
            serde_json::from_value(serde_json::json!({
                "identity": {
                    "contract": "uta.workflow-execution",
                    "version": 1,
                    "workflow_schema_version": 2,
                    "workflow_id": "song:test",
                    "workflow_revision": 3,
                    "definition_digest": "a".repeat(32)
                },
                "nodes": [
                    {
                        "instance_id": "primary-f0",
                        "analysis_node": "workflow.primary-f0",
                        "capabilities": ["pitch.track"],
                        "execution_policy": "always",
                        "execution_state": "ready",
                        "priority": 800,
                        "depends_on": [],
                        "input_bindings": []
                    },
                    {
                        "instance_id": "game",
                        "analysis_node": "workflow.game",
                        "capabilities": ["notes.game"],
                        "execution_policy": "always",
                        "execution_state": "ready",
                        "priority": 750,
                        "depends_on": [],
                        "input_bindings": []
                    },
                    {
                        "instance_id": "secondary-rmvpe",
                        "analysis_node": "workflow.secondary-rmvpe",
                        "capabilities": ["pitch.secondary.rmvpe"],
                        "execution_policy": "always",
                        "execution_state": "ready",
                        "priority": 700,
                        "depends_on": [],
                        "input_bindings": []
                    },
                    {
                        "instance_id": "basic",
                        "analysis_node": "workflow.basic",
                        "capabilities": ["notes.basic_pitch"],
                        "execution_policy": "disagreement_windows",
                        "execution_state": "deferred",
                        "priority": 650,
                        "depends_on": [],
                        "input_bindings": []
                    },
                    {
                        "instance_id": "stars-note",
                        "analysis_node": "workflow.stars-note",
                        "capabilities": ["notes.stars"],
                        "execution_policy": "disabled",
                        "execution_state": "disabled",
                        "priority": 640,
                        "depends_on": [],
                        "input_bindings": []
                    },
                    {
                        "instance_id": "stars-technique",
                        "analysis_node": "workflow.stars-technique",
                        "capabilities": ["technique.analyze"],
                        "execution_policy": "maximum_only",
                        "execution_state": "profile_skipped",
                        "priority": 630,
                        "depends_on": [],
                        "input_bindings": []
                    }
                ],
                "terminal_outputs": [],
                "fusion_policy": null,
                "fusion_mode": "algorithm"
            }))
            .unwrap();

        assert_eq!(
            workflow_planned_evidence(&workflow),
            [
                "Primary continuous F0 (ready)",
                "GAME note regions (ready)",
                "RMVPE secondary F0 (ready)",
                "Basic Pitch onset/note (conditional)",
                "STARS technique context (profile skipped)"
            ]
        );
    }

    #[test]
    fn plan_preview_exposes_exact_typed_provider_invocations() {
        let workflow: app_core::WorkflowExecutionPlanWireV1 =
            serde_json::from_value(serde_json::json!({
                "identity": {
                    "contract": "uta.workflow-execution",
                    "version": 1,
                    "workflow_schema_version": 2,
                    "workflow_id": "song:topology",
                    "workflow_revision": 1,
                    "definition_digest": "b".repeat(32)
                },
                "nodes": [{
                    "instance_id": "vocal_bgm_split",
                    "analysis_node": "workflow.vocal_bgm_split",
                    "capabilities": ["audio.extract_vocals", "audio.extract_instrumental"],
                    "execution_policy": "always",
                    "execution_state": "ready",
                    "priority": 900,
                    "depends_on": [],
                    "input_bindings": [],
                    "execution_invocations": [{
                        "invocation_id": "vocal_bgm_split.dual",
                        "provider_id": "dual_output_separator",
                        "capabilities": ["audio.extract_vocals", "audio.extract_instrumental"],
                        "output_ports": ["vocal", "instrumental"]
                    }]
                }],
                "terminal_outputs": [],
                "fusion_policy": null,
                "fusion_mode": "algorithm"
            }))
            .unwrap();

        assert_eq!(
            workflow_execution_topology(&workflow),
            [
                "vocal_bgm_split.dual · dual_output_separator (dual_output_separator) → vocal + instrumental"
            ]
        );
    }
}
