const CONVERTER_PATCH: &str =
    include_str!("../patches/predict-woo-forced-aligner-flat-hf-converter.patch");
#[test]
fn forced_aligner_converter_patch_contains_required_mappings() {
    for required_mapping in [
        "Qwen3ASRForTokenClassification",
        "model.audio_tower.",
        "model.multi_modal_projector.linear_1.",
        "model.multi_modal_projector.linear_2.",
        "model.language_model.",
        "score.weight",
        "output.weight",
        "tokenizer.json",
    ] {
        assert!(
            CONVERTER_PATCH.contains(required_mapping),
            "converter adaptation is missing {required_mapping}"
        );
    }
}
