use super::*;
use crate::analysis_experience::{
    AnalysisExperienceOverride, AnalysisExperienceSettings, AnalysisSettingSource,
};

fn request_for(settings: &EffectiveAnalysisExperience) -> AnalyzeRequestWire {
    compile_analyze_request(
        AnalysisRequestIntent {
            request_id: "workflow-quantization".to_string(),
            turbo_acceleration: false,
            source: ResolvedAnalysisSource {
                library_file_hash: "fixture".to_string(),
                path: std::env::temp_dir().join("source.flac"),
                sha256: "fixture".to_string(),
                role: AudioRoleWire::OriginalMix,
            },
            lyrics: StudioLyricsContext::default(),
            target_override: Some(AnalysisDefaultTarget::FullCandidate),
            requested_outputs: None,
            compute_backend: None,
            model_settings: Default::default(),
            model_backend_overrides: BTreeMap::new(),
            default_device_class: None,
            model_device_overrides: BTreeMap::new(),
        },
        settings,
    )
    .unwrap()
}

#[test]
fn saved_workflow_quantization_reaches_the_exact_request_and_keeps_defaults_unchanged() {
    let global = AnalysisExperienceSettings {
        enable_quantization: false,
        ..Default::default()
    };
    let mut workflow = crate::workflow::default_workflow("fixture");
    assert!(!workflow.quantization_enabled(false));
    assert!(workflow.quantization_enabled(true));
    for enabled in [true, false] {
        crate::workflow::set_workflow_parameter(
            &mut workflow,
            &crate::workflow::WorkflowNodeId::new("canonical_track"),
            "enable_quantization",
            serde_json::Value::Bool(enabled),
        )
        .unwrap();
        let saved = serde_json::to_string(&workflow).unwrap();
        let loaded: crate::workflow::WorkflowDefinition = serde_json::from_str(&saved).unwrap();
        let settings = loaded.resolve_analysis_experience(&global, None, None);
        assert_eq!(settings.enable_quantization.value, enabled);
        assert_eq!(
            settings.enable_quantization.source,
            AnalysisSettingSource::Song
        );
        crate::workflow::compile_workflow(&loaded).unwrap();
        let mut request = request_for(&settings);
        attach_musical_context(&mut request, &settings, Some(120.0), None).unwrap();
        assert_eq!(request.analysis.enable_quantization, enabled);
        let context = request.musical_context.unwrap();
        assert_eq!(context.bpm, Some(120.0));
        assert_eq!(
            context.quantization_grid,
            enabled.then_some(QuantizationGridWire::Sixteenth)
        );
        assert!(!global.enable_quantization);
    }
}

#[test]
fn workflow_quantization_obeys_run_then_workflow_then_song_then_global_precedence() {
    let global = AnalysisExperienceSettings {
        enable_quantization: false,
        ..Default::default()
    };
    let song = AnalysisExperienceOverride {
        enable_quantization: Some(true),
        ..Default::default()
    };
    let mut workflow = crate::workflow::default_workflow("fixture");
    assert!(
        workflow
            .resolve_analysis_experience(&global, Some(&song), None)
            .enable_quantization
            .value
    );
    crate::workflow::set_workflow_parameter(
        &mut workflow,
        &crate::workflow::WorkflowNodeId::new("canonical_track"),
        "enable_quantization",
        serde_json::Value::Bool(false),
    )
    .unwrap();
    assert!(
        !workflow
            .resolve_analysis_experience(&global, Some(&song), None)
            .enable_quantization
            .value
    );
    let run = AnalysisExperienceOverride {
        enable_quantization: Some(true),
        ..Default::default()
    };
    let effective = workflow.resolve_analysis_experience(&global, Some(&song), Some(&run));
    assert!(effective.enable_quantization.value);
    assert_eq!(
        effective.enable_quantization.source,
        AnalysisSettingSource::Run
    );
}

#[test]
fn enabled_quantization_keeps_the_existing_bpm_requirement_and_skips_non_chart_outputs() {
    let global = AnalysisExperienceSettings {
        enable_quantization: true,
        ..Default::default()
    };
    let workflow = crate::workflow::default_workflow("fixture");
    let settings = workflow.resolve_analysis_experience(&global, None, None);
    let mut request = request_for(&settings);
    assert!(
        attach_musical_context(&mut request, &settings, None, None)
            .unwrap_err()
            .contains("BPM")
    );
    request.requested_artifacts.vocal_chart = false;
    attach_musical_context(&mut request, &settings, None, None).unwrap();
    assert!(!request.analysis.enable_quantization);
    assert!(request.musical_context.is_none());
}
