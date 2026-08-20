use std::path::Path;

const ANALYZE_PY: &str = include_str!("../analyzer/analyze.py");
const SERVER_PY: &str = include_str!("../analyzer/server.py");
const PIPELINE_PY: &str = include_str!("../analyzer/pipeline.py");
const PITCH_PY: &str = include_str!("../analyzer/pitch.py");
const KEY_DETECT_PY: &str = include_str!("../analyzer/key_detect.py");
const RHYTHM_PY: &str = include_str!("../analyzer/rhythm.py");
const STEMS_PY: &str = include_str!("../analyzer/stems.py");
const TRANSCRIBE_PY: &str = include_str!("../analyzer/transcribe.py");
const ALIGN_PY: &str = include_str!("../analyzer/align.py");
const CTC_ALIGN_PY: &str = include_str!("../analyzer/ctc_align.py");
const QWEN_ALIGN_PY: &str = include_str!("../analyzer/qwen_align.py");
const MMS_KARAOKE_PY: &str = include_str!("../analyzer/mms_karaoke.py");
const AUDIO_PY: &str = include_str!("../analyzer/audio.py");
const HALLUCINATION_PY: &str = include_str!("../analyzer/hallucination.py");
const LANGUAGE_PY: &str = include_str!("../analyzer/language.py");
const WHISPER_COMPAT_PY: &str = include_str!("../analyzer/whisper_compat.py");
const PARAKEET_PY: &str = include_str!("../analyzer/parakeet.py");
const OPENVINO_WHISPER_PY: &str = include_str!("../analyzer/openvino_whisper.py");
const OPENVINO_SEPARATION_PY: &str = include_str!("../analyzer/openvino_separation.py");
const OPENVINO_MDX_PY: &str = include_str!("../analyzer/openvino_mdx.py");
const GPU_PY: &str = include_str!("../analyzer/gpu.py");
const CJK_PY: &str = include_str!("../analyzer/cjk.py");
const MODEL_SETUP_PY: &str = include_str!("../analyzer/model_setup.py");

const AUDIO_MODELS_INIT_PY: &str = include_str!("../analyzer/audio_models/__init__.py");
const AUDIO_MODELS_CATALOG_PY: &str = include_str!("../analyzer/audio_models/catalog.py");
const AUDIO_MODELS_CATALOG_YAML: &str = include_str!("../analyzer/audio_models/catalog.yaml");
const AUDIO_MODELS_ERRORS_PY: &str = include_str!("../analyzer/audio_models/errors.py");
const AUDIO_MODELS_INSTALL_PY: &str = include_str!("../analyzer/audio_models/install.py");
const AUDIO_MODELS_PARAMETERS_PY: &str = include_str!("../analyzer/audio_models/parameters.py");
const AUDIO_MODELS_PLAN_PY: &str = include_str!("../analyzer/audio_models/plan.py");
const AUDIO_MODELS_SCHEMA_PY: &str = include_str!("../analyzer/audio_models/schema.py");
const AUDIO_MODELS_YAML_UTIL_PY: &str = include_str!("../analyzer/audio_models/yaml_util.py");
const AUDIO_MODELS_CONFIG_BS_ROFORMER: &str =
    include_str!("../analyzer/audio_models/configs/model_bs_roformer_ep_317_sdr_12.9755.yaml");
const AUDIO_MODELS_CONFIG_MELBAND_INST: &str =
    include_str!("../analyzer/audio_models/configs/config_melbandroformer_inst_v2.yaml");
const AUDIO_MODELS_CONFIG_HTDEMUCS: &str =
    include_str!("../analyzer/audio_models/configs/htdemucs_6s.yaml");
const AUDIO_MODELS_CONFIG_DENOISE: &str = include_str!(
    "../analyzer/audio_models/configs/denoise_mel_band_roformer_aufr33_sdr_27.9959_config.yaml"
);
const AUDIO_MODELS_CONFIG_DEREVERB: &str =
    include_str!("../analyzer/audio_models/configs/dereverb_mel_band_roformer_anvuew.yaml");
const AUDIO_MODELS_CONFIG_MDX_KARA: &str =
    include_str!("../analyzer/audio_models/configs/mdx_uvr_mdxnet_kara_2.json");
const AUDIO_MODELS_CONFIG_KARAOKE: &str =
    include_str!("../analyzer/audio_models/configs/mel_band_roformer_karaoke_aufr33_viperx.yaml");

const AUDIO_PROCESSORS_INIT_PY: &str = include_str!("../analyzer/audio_processors/__init__.py");
const AUDIO_PROCESSORS_CONTRACTS_PY: &str =
    include_str!("../analyzer/audio_processors/contracts.py");
const AUDIO_PROCESSORS_EXECUTOR_PY: &str = include_str!("../analyzer/audio_processors/executor.py");
const AUDIO_PROCESSORS_OUTPUTS_PY: &str = include_str!("../analyzer/audio_processors/outputs.py");
const AUDIO_PROCESSORS_RUNNERS_INIT_PY: &str =
    include_str!("../analyzer/audio_processors/runners/__init__.py");
const AUDIO_PROCESSORS_RUNNERS_BASE_PY: &str =
    include_str!("../analyzer/audio_processors/runners/base.py");
const AUDIO_PROCESSORS_RUNNERS_DEMUCS_PY: &str =
    include_str!("../analyzer/audio_processors/runners/demucs_torch.py");
const AUDIO_PROCESSORS_RUNNERS_MDX_ONNX_PY: &str =
    include_str!("../analyzer/audio_processors/runners/mdx_onnx.py");
const AUDIO_PROCESSORS_RUNNERS_MDXC_PY: &str =
    include_str!("../analyzer/audio_processors/runners/mdxc_torch.py");

const AUDIO_SEPARATOR_ADAPTER_INIT_PY: &str =
    include_str!("../analyzer/audio_separator_adapter/__init__.py");
const AUDIO_SEPARATOR_ADAPTER_OFFLINE_PY: &str =
    include_str!("../analyzer/audio_separator_adapter/offline.py");

const FILES: &[(&str, &str)] = &[
    ("analyze.py", ANALYZE_PY),
    ("server.py", SERVER_PY),
    ("pipeline.py", PIPELINE_PY),
    ("pitch.py", PITCH_PY),
    ("key_detect.py", KEY_DETECT_PY),
    ("rhythm.py", RHYTHM_PY),
    ("stems.py", STEMS_PY),
    ("transcribe.py", TRANSCRIBE_PY),
    ("align.py", ALIGN_PY),
    ("ctc_align.py", CTC_ALIGN_PY),
    ("qwen_align.py", QWEN_ALIGN_PY),
    ("mms_karaoke.py", MMS_KARAOKE_PY),
    ("audio.py", AUDIO_PY),
    ("hallucination.py", HALLUCINATION_PY),
    ("language.py", LANGUAGE_PY),
    ("whisper_compat.py", WHISPER_COMPAT_PY),
    ("parakeet.py", PARAKEET_PY),
    ("openvino_whisper.py", OPENVINO_WHISPER_PY),
    ("openvino_separation.py", OPENVINO_SEPARATION_PY),
    ("openvino_mdx.py", OPENVINO_MDX_PY),
    ("gpu.py", GPU_PY),
    ("cjk.py", CJK_PY),
    ("model_setup.py", MODEL_SETUP_PY),
    ("audio_models/__init__.py", AUDIO_MODELS_INIT_PY),
    ("audio_models/catalog.py", AUDIO_MODELS_CATALOG_PY),
    ("audio_models/catalog.yaml", AUDIO_MODELS_CATALOG_YAML),
    ("audio_models/errors.py", AUDIO_MODELS_ERRORS_PY),
    ("audio_models/install.py", AUDIO_MODELS_INSTALL_PY),
    ("audio_models/parameters.py", AUDIO_MODELS_PARAMETERS_PY),
    ("audio_models/plan.py", AUDIO_MODELS_PLAN_PY),
    ("audio_models/schema.py", AUDIO_MODELS_SCHEMA_PY),
    ("audio_models/yaml_util.py", AUDIO_MODELS_YAML_UTIL_PY),
    (
        "audio_models/configs/model_bs_roformer_ep_317_sdr_12.9755.yaml",
        AUDIO_MODELS_CONFIG_BS_ROFORMER,
    ),
    (
        "audio_models/configs/config_melbandroformer_inst_v2.yaml",
        AUDIO_MODELS_CONFIG_MELBAND_INST,
    ),
    (
        "audio_models/configs/htdemucs_6s.yaml",
        AUDIO_MODELS_CONFIG_HTDEMUCS,
    ),
    (
        "audio_models/configs/denoise_mel_band_roformer_aufr33_sdr_27.9959_config.yaml",
        AUDIO_MODELS_CONFIG_DENOISE,
    ),
    (
        "audio_models/configs/dereverb_mel_band_roformer_anvuew.yaml",
        AUDIO_MODELS_CONFIG_DEREVERB,
    ),
    (
        "audio_models/configs/mdx_uvr_mdxnet_kara_2.json",
        AUDIO_MODELS_CONFIG_MDX_KARA,
    ),
    (
        "audio_models/configs/mel_band_roformer_karaoke_aufr33_viperx.yaml",
        AUDIO_MODELS_CONFIG_KARAOKE,
    ),
    ("audio_processors/__init__.py", AUDIO_PROCESSORS_INIT_PY),
    (
        "audio_processors/contracts.py",
        AUDIO_PROCESSORS_CONTRACTS_PY,
    ),
    ("audio_processors/executor.py", AUDIO_PROCESSORS_EXECUTOR_PY),
    ("audio_processors/outputs.py", AUDIO_PROCESSORS_OUTPUTS_PY),
    (
        "audio_processors/runners/__init__.py",
        AUDIO_PROCESSORS_RUNNERS_INIT_PY,
    ),
    (
        "audio_processors/runners/base.py",
        AUDIO_PROCESSORS_RUNNERS_BASE_PY,
    ),
    (
        "audio_processors/runners/demucs_torch.py",
        AUDIO_PROCESSORS_RUNNERS_DEMUCS_PY,
    ),
    (
        "audio_processors/runners/mdx_onnx.py",
        AUDIO_PROCESSORS_RUNNERS_MDX_ONNX_PY,
    ),
    (
        "audio_processors/runners/mdxc_torch.py",
        AUDIO_PROCESSORS_RUNNERS_MDXC_PY,
    ),
    (
        "audio_separator_adapter/__init__.py",
        AUDIO_SEPARATOR_ADAPTER_INIT_PY,
    ),
    (
        "audio_separator_adapter/offline.py",
        AUDIO_SEPARATOR_ADAPTER_OFFLINE_PY,
    ),
];

pub fn write_scripts(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;

    for (name, content) in FILES {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn write_scripts_includes_audio_processing_packages() {
        let dir = std::env::temp_dir().join(format!(
            "uta-studio-vendor-scripts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        super::write_scripts(&dir).expect("write analyzer scripts");
        for relative in [
            "pipeline.py",
            "openvino_mdx.py",
            "audio_models/__init__.py",
            "audio_models/plan.py",
            "audio_models/catalog.yaml",
            "audio_models/configs/mel_band_roformer_karaoke_aufr33_viperx.yaml",
            "audio_processors/executor.py",
            "audio_processors/runners/mdxc_torch.py",
            "audio_separator_adapter/offline.py",
        ] {
            let path = dir.join(relative);
            assert!(
                path.is_file(),
                "expected shipped analyzer file {}",
                path.display()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
