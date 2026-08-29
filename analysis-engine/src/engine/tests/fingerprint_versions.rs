use super::*;

#[test]
fn quantization_and_audio_quality_versions_participate_in_execution_fingerprint() {
    let request = valid_request(AudioRole::LeadVocal);
    let quality_gates = vec![TIMELINE_VALID_GATE.to_string()];
    let identity = |quantization_version, audio_quality_version| ExecutionIdentity {
        request: fingerprint_request(&request).unwrap(),
        resources: Vec::new(),
        acoustic_dsp_version: ACOUSTIC_DSP_VERSION,
        audio_quality_version,
        quality_gates: &quality_gates,
        calibration_version: CALIBRATION_VERSION,
        finalize_vocal_chart_version: FINALIZE_VOCAL_CHART_VERSION,
        fusion_version: FUSION_VERSION,
        fusion_decision: None,
        quantization_version,
        postprocess_version: POSTPROCESS_VERSION,
    };
    let current =
        deterministic_fingerprint(&identity(QUANTIZATION_VERSION, AUDIO_QUALITY_VERSION)).unwrap();
    assert_ne!(
        current,
        deterministic_fingerprint(&identity("rhythm-grid-dp-future", AUDIO_QUALITY_VERSION,))
            .unwrap()
    );
    assert_ne!(
        current,
        deterministic_fingerprint(&identity(
            QUANTIZATION_VERSION,
            "audio-quality-gates-future",
        ))
        .unwrap()
    );
}
