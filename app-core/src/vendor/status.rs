use std::path::PathBuf;
use std::process::Command;

use sha2::{Digest, Sha256};

use super::*;
use crate::cache::{
    CachePaths, models_dir, normalized_target_path, relocate_directory_contents, songs_cache_dir,
    uta_studio_dir, vendor_dir,
};
use crate::native_runtime::{
    RMVPE_IR_MANIFEST_SHA256, RMVPE_IR_RELATIVE_DIR, RUNTIME_LOCK_SHA256, component_executable,
    native_analyzer_path, native_runtime_lock,
};

pub fn resolve_data_path_input(input: &str) -> Result<PathBuf, String> {
    normalized_target_path(PathBuf::from(input))
}

pub(crate) fn configured_file_path(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

pub(crate) fn executable_on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|candidate| candidate.is_file())
}

pub(crate) fn executable_path(variable: &str, names: &[&str]) -> Option<PathBuf> {
    configured_file_path(variable).or_else(|| executable_on_path(names))
}

pub fn ffmpeg_path() -> PathBuf {
    let names = if cfg!(windows) {
        &["ffmpeg.exe", "ffmpeg"][..]
    } else {
        &["ffmpeg"][..]
    };
    executable_path("UTA_STUDIO_FFMPEG_PATH", names).unwrap_or_else(|| {
        vendor_dir().join(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        })
    })
}

pub(crate) fn silent_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt;
        let mut command = command;
        command.creation_flags(0x08000000);
        command
    };
    command
}

fn model_file(relative: &str) -> bool {
    models_dir().join(relative).is_file()
}

fn rmvpe_model_file() -> bool {
    let directory = models_dir().join(RMVPE_IR_RELATIVE_DIR);
    let manifest = match std::fs::read(directory.join("manifest.json")) {
        Ok(manifest) => manifest,
        Err(_) => return false,
    };
    if format!("{:x}", Sha256::digest(&manifest)) != RMVPE_IR_MANIFEST_SHA256
        || !directory.join("rmvpe.bin").is_file()
    {
        return false;
    }
    (32..=1024)
        .step_by(32)
        .all(|frames| directory.join(format!("rmvpe-{frames:04}.xml")).is_file())
}

fn model_manifest(relative: &str) -> bool {
    models_dir()
        .join(relative)
        .join("install-manifest.json")
        .is_file()
}

fn openvino_ir_manifest(relative: &str) -> bool {
    models_dir().join(relative).join("manifest.json").is_file()
}

pub fn model_install_statuses() -> Vec<ModelInstallStatus> {
    vec![
        ModelInstallStatus {
            target: ModelDownloadTarget::SharedRuntime,
            label: "Native analysis components".to_string(),
            description: "Packaged local workers; no network service is used.".to_string(),
            available: native_analyzer_path().is_some(),
            backend: "native".to_string(),
            validation: "packaged".to_string(),
        },
        ModelInstallStatus {
            target: ModelDownloadTarget::RoFormer,
            label: "RoFormer separation".to_string(),
            description: "Vocal, BGM, harmony, denoise, and dereverb native model family."
                .to_string(),
            available: component_executable("roformer_runtime").is_some()
                && crate::audio_model::REQUIRED_AUDIO_MODEL_IDS
                    .iter()
                    .all(|model| model_manifest(&format!("audio-processing/{model}"))),
            backend: "vulkan".to_string(),
            validation: "benchmark_candidate_short_smoke_only".to_string(),
        },
        ModelInstallStatus {
            target: ModelDownloadTarget::FireRed,
            label: "FireRedASR2-AED".to_string(),
            description: "Primary Chinese and Chinese-singing transcript expert.".to_string(),
            available: component_executable("openvino_runtime").is_some()
                && openvino_ir_manifest("firered-asr2-aed/openvino-ir-2026.3.0-smoke"),
            backend: "openvino".to_string(),
            validation: "candidate".to_string(),
        },
        ModelInstallStatus {
            target: ModelDownloadTarget::QwenAsr,
            label: "Qwen3-ASR-1.7B".to_string(),
            description: "Pinned multilingual transcript expert.".to_string(),
            available: component_executable("qwen_asr_runtime").is_some()
                && model_file("qwen-asr/Qwen3-ASR-1.7B-Q4_K_M.gguf"),
            backend: "vulkan".to_string(),
            validation: "benchmark_candidate_pinned_recipe".to_string(),
        },
        ModelInstallStatus {
            target: ModelDownloadTarget::QwenAlign,
            label: "Qwen3 Forced Aligner".to_string(),
            description: "Pinned word and character alignment expert.".to_string(),
            available: component_executable("qwen_align_runtime").is_some()
                && model_manifest("qwen-align"),
            backend: "vulkan".to_string(),
            validation: "benchmark_candidate_pinned_recipe".to_string(),
        },
        ModelInstallStatus {
            target: ModelDownloadTarget::Pitch,
            label: "RMVPE".to_string(),
            description: "Primary continuous singing F0 expert.".to_string(),
            available: component_executable("openvino_runtime").is_some() && rmvpe_model_file(),
            backend: "openvino".to_string(),
            validation: "production_pinned".to_string(),
        },
        ModelInstallStatus {
            target: ModelDownloadTarget::Fcpe,
            label: "FCPE".to_string(),
            description: "Independent F0 disagreement expert.".to_string(),
            available: component_executable("openvino_runtime").is_some()
                && openvino_ir_manifest("pitch/fcpe/openvino-ir-2026.3.0-smoke"),
            backend: "openvino".to_string(),
            validation: "candidate".to_string(),
        },
        ModelInstallStatus {
            target: ModelDownloadTarget::Game,
            label: "GAME".to_string(),
            description: "Primary singing note-boundary expert.".to_string(),
            available: model_manifest("boundary/game"),
            backend: "openvino".to_string(),
            validation: "candidate".to_string(),
        },
        ModelInstallStatus {
            target: ModelDownloadTarget::Stars,
            label: "STARS".to_string(),
            description: "Maximum-quality boundary and technique expert.".to_string(),
            available: model_manifest("technique/stars"),
            backend: "unresolved".to_string(),
            validation: "experimental".to_string(),
        },
        ModelInstallStatus {
            target: ModelDownloadTarget::BasicPitch,
            label: "Basic Pitch".to_string(),
            description: "Auxiliary onset and activation evidence.".to_string(),
            available: component_executable("openvino_runtime").is_some()
                && openvino_ir_manifest("boundary/basic-pitch/openvino-ir-2026.3.0-smoke"),
            backend: "openvino".to_string(),
            validation: "candidate".to_string(),
        },
    ]
}

pub struct ModelAvailabilityParams<'a> {
    pub _profile: &'a crate::analysis_profile::AnalysisProfileSnapshot,
}

pub fn model_availability_params_for_profile(
    profile: &crate::analysis_profile::AnalysisProfileSnapshot,
) -> ModelAvailabilityParams<'_> {
    ModelAvailabilityParams { _profile: profile }
}

pub fn node_model_availability_for(
    _params: &ModelAvailabilityParams<'_>,
) -> std::collections::BTreeMap<crate::analysis_graph::AnalysisNodeId, bool> {
    use crate::analysis_graph::AnalysisNodeId;
    let statuses = model_install_statuses();
    let available = |target| {
        statuses
            .iter()
            .find(|status| status.target == target)
            .is_some_and(|status| status.available)
    };
    [
        ("stems.separate", available(ModelDownloadTarget::RoFormer)),
        ("stems.vocals", available(ModelDownloadTarget::RoFormer)),
        (
            "stems.instrumental",
            available(ModelDownloadTarget::RoFormer),
        ),
        ("pitch.extract", available(ModelDownloadTarget::Pitch)),
        ("lyrics.transcribe", available(ModelDownloadTarget::FireRed)),
        ("lyrics.align", available(ModelDownloadTarget::QwenAlign)),
    ]
    .into_iter()
    .map(|(node, ready)| (AnalysisNodeId::new(node), ready))
    .collect()
}

pub fn analysis_runtime_status() -> AnalysisRuntimeStatus {
    let ffmpeg = ffmpeg_path();
    let models = model_install_statuses();
    let selected_models = models
        .iter()
        .map(|model| model.label.clone())
        .collect::<Vec<_>>();
    let selected_models_available = models
        .iter()
        .filter(|model| {
            matches!(
                model.target,
                ModelDownloadTarget::SharedRuntime
                    | ModelDownloadTarget::RoFormer
                    | ModelDownloadTarget::FireRed
                    | ModelDownloadTarget::QwenAsr
                    | ModelDownloadTarget::QwenAlign
                    | ModelDownloadTarget::Pitch
                    | ModelDownloadTarget::Game
            )
        })
        .all(|model| model.available);
    let runtime_lock_valid = native_runtime_lock().is_ok();
    let native_analyzer_available = native_analyzer_path().is_some();
    let roformer_runtime_available = models
        .iter()
        .find(|model| model.target == ModelDownloadTarget::RoFormer)
        .is_some_and(|model| model.available);
    let openvino_runtime_available = component_executable("openvino_runtime").is_some();
    let qwen_asr_runtime_available = component_executable("qwen_asr_runtime").is_some();
    let qwen_align_runtime_available = component_executable("qwen_align_runtime").is_some();
    let pitch_model_available = rmvpe_model_file();
    let mut missing = Vec::new();
    if !ffmpeg.is_file() {
        missing.push("ffmpeg".to_string());
    }
    if !native_analyzer_available {
        missing.push("native analyzer".to_string());
    }
    if !runtime_lock_valid {
        missing.push("runtime lock".to_string());
    }
    missing.extend(
        models
            .iter()
            .filter(|model| !model.available)
            .map(|model| model.label.clone()),
    );
    missing.sort();
    missing.dedup();
    AnalysisRuntimeStatus {
        ready: ffmpeg.is_file()
            && native_analyzer_available
            && runtime_lock_valid
            && selected_models_available,
        runtime_contract_current: runtime_lock_valid,
        ffmpeg_available: ffmpeg.is_file(),
        native_analyzer_available,
        roformer_runtime_available,
        openvino_runtime_available,
        qwen_asr_runtime_available,
        qwen_align_runtime_available,
        runtime_lock_valid,
        pitch_model_available,
        selected_models_available,
        selected_models,
        models,
        compute_backend: "pinned OpenVINO IR; pinned Qwen Vulkan exceptions".to_string(),
        ffmpeg_path: ffmpeg
            .is_file()
            .then(|| ffmpeg.to_string_lossy().into_owned()),
        runtime_lock_sha256: RUNTIME_LOCK_SHA256.to_string(),
        missing,
    }
}

pub fn is_ready() -> bool {
    analysis_runtime_status().ready
}

pub(crate) fn normalize_optional_path(path: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    path.map(normalized_target_path).transpose()
}

pub(crate) fn normalize_cache_paths(paths: CachePaths) -> Result<CachePaths, String> {
    Ok(CachePaths {
        songs: normalize_optional_path(paths.songs)?,
        models: normalize_optional_path(paths.models)?,
        vendor: normalize_optional_path(paths.vendor)?,
    })
}

pub(crate) fn default_cache_paths_for_data_root() -> CachePaths {
    let root = uta_studio_dir();
    CachePaths {
        songs: Some(root.join("cache")),
        models: Some(root.join("models")),
        vendor: Some(root.join("vendor")),
    }
}

pub(crate) fn relocate_cache_data_to_targets(targets: &CachePaths) -> Result<(), String> {
    if let Some(target) = targets.songs.as_ref() {
        relocate_directory_contents(&songs_cache_dir(), target)?;
    }
    if let Some(target) = targets.models.as_ref() {
        relocate_directory_contents(&models_dir(), target)?;
    }
    Ok(())
}
