use sha2::{Digest, Sha256};

const CONVERTER_PATCH: &str =
    include_str!("../patches/predict-woo-forced-aligner-flat-hf-converter.patch");
const CONVERTER_PATCH_SHA256: &str =
    "ffd8a575238c81823509e2a7bf645bf9bb5d38db2903bc3306648afd619b42d6";

#[test]
fn forced_aligner_converter_patch_identity_and_required_mappings_are_pinned() {
    assert_eq!(
        format!("{:x}", Sha256::digest(CONVERTER_PATCH.as_bytes())),
        CONVERTER_PATCH_SHA256
    );
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
