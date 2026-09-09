use super::*;
use crate::contract::request::tests::valid_request;

fn resource_ids(requirements: &EngineRequirementsV1) -> BTreeSet<&str> {
    requirements
        .resources
        .iter()
        .map(|resource| resource.resource.as_str())
        .collect()
}

fn select_outputs(request: &mut AnalyzeRequestV1, pitch: bool, stems: Vec<AudioRole>) {
    request.requested_artifacts = crate::contract::RequestedArtifactsV1 {
        vocal_chart: false,
        pitch_evidence: pitch,
        singing_analysis: false,
        transcript: false,
        alignment: false,
        stems,
    };
}

fn has_node(plan: &EnginePlan, capability: &str) -> bool {
    plan.execution_nodes
        .iter()
        .any(|node| node.capability.as_str() == capability)
}

#[test]
fn pitch_only_original_mix_uses_leap_and_rmvpe() {
    let mut request = valid_request(AudioRole::OriginalMix);
    select_outputs(&mut request, true, Vec::new());

    let requirements = Planner::requirements(&request).unwrap();
    let resources = resource_ids(&requirements);
    assert!(resources.contains("model:bs_roformer_leap_xe90_vocals"));
    assert!(resources.contains("model:rmvpe"));
    assert_eq!(resources.len(), 3, "ffmpeg plus two implemented models");

    let plan = Planner::plan(&request, None).unwrap();
    assert!(has_node(&plan, "audio.extract_vocals"));
    assert!(has_node(&plan, "pitch.track"));
    assert!(!has_node(&plan, "speech.transcribe"));
    assert!(!has_node(&plan, "speech.align"));
}

#[test]
fn clean_lead_pitch_does_not_require_roformer() {
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    select_outputs(&mut request, true, Vec::new());
    let requirements = Planner::requirements(&request).unwrap();
    let resources = resource_ids(&requirements);
    assert!(resources.contains("model:rmvpe"));
    assert!(
        !resources
            .iter()
            .any(|resource| resource.contains("roformer"))
    );
}

#[test]
fn explicit_lead_output_forces_the_harmony_separator() {
    let mut request = valid_request(AudioRole::OriginalMix);
    select_outputs(&mut request, false, vec![AudioRole::LeadVocal]);
    let requirements = Planner::requirements(&request).unwrap();
    assert!(resource_ids(&requirements).contains("model:melband_roformer_harmony"));
    let plan = Planner::plan(&request, None).unwrap();
    assert!(has_node(&plan, "audio.lead_isolate"));
}

#[test]
fn leap_dual_output_satisfies_vocal_and_instrumental_stems() {
    let mut request = valid_request(AudioRole::OriginalMix);
    select_outputs(
        &mut request,
        false,
        vec![AudioRole::GuideVocals, AudioRole::Instrumental],
    );
    let requirements = Planner::requirements(&request).unwrap();
    let resources = resource_ids(&requirements);
    assert!(resources.contains("model:bs_roformer_leap_xe90_vocals"));
    assert!(!resources.contains("model:bs_polarformer_public_instrumental"));
    let plan = Planner::plan(&request, None).unwrap();
    assert!(has_node(&plan, "audio.extract_vocals"));
    assert!(has_node(&plan, "audio.extract_instrumental"));
}

#[test]
fn generated_transcript_request_uses_qwen_asr() {
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    select_outputs(&mut request, false, Vec::new());
    request.requested_artifacts.transcript = true;
    request.lyrics.mode = LyricsMode::None;
    let requirements = Planner::requirements(&request).unwrap();
    assert!(resource_ids(&requirements).contains("model:qwen3_asr_1_7b"));
    let plan = Planner::plan(&request, None).unwrap();
    assert!(has_node(&plan, "speech.transcribe"));
    assert!(has_node(&plan, "fusion.transcript"));
}

#[test]
fn canonical_alignment_request_uses_qwen_forced_aligner() {
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    select_outputs(&mut request, false, Vec::new());
    request.requested_artifacts.alignment = true;
    request.lyrics.mode = LyricsMode::Canonical;
    request.lyrics.tokens.push(crate::contract::LyricTokenV1 {
        id: "line-1".to_string(),
        text: "sing".to_string(),
        reading: None,
        phonemes: None,
        start: None,
        end: None,
    });
    let requirements = Planner::requirements(&request).unwrap();
    assert!(resource_ids(&requirements).contains("model:qwen3_forced_aligner_0_6b"));
    assert!(!resource_ids(&requirements).contains("model:qwen3_asr_1_7b"));
    let plan = Planner::plan(&request, None).unwrap();
    assert!(has_node(&plan, "speech.align"));
    assert!(has_node(&plan, "fusion.alignment"));
}

#[test]
fn unsupported_singer_partition_stems_fail_closed() {
    for role in [AudioRole::BackingVocal, AudioRole::HarmonyVocal] {
        let mut request = valid_request(AudioRole::OriginalMix);
        select_outputs(&mut request, false, vec![role]);
        let error = Planner::requirements(&request).unwrap_err();
        assert_eq!(error.code, EngineErrorCode::MissingCapability);
        assert_eq!(error.capability.as_deref(), Some("audio.lead_partition"));
    }
}

#[test]
fn unknown_satisfied_capability_is_rejected() {
    let mut request = valid_request(AudioRole::CleanLeadVocal);
    select_outputs(&mut request, true, Vec::new());
    request
        .satisfied_capabilities
        .push("unknown.capability".to_string());
    assert_eq!(
        Planner::requirements(&request).unwrap_err().code,
        EngineErrorCode::InvalidContract
    );
}
