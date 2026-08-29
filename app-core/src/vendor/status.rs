use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::*;
use crate::backend_cli::{
    AnalysisCliClient, NativeBackendWireV1, RuntimeCliClient, RuntimePolicyWireV1,
    RuntimeResourceDetailsWireV1, RuntimeResourceRefWireV1, RuntimeResourceStatusWireV1,
    ValidationStateWireV1,
};
use crate::cache::{
    CachePaths, normalized_target_path, relocate_directory_contents, songs_cache_dir,
    uta_studio_dir, vendor_dir,
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

pub(crate) fn runtime_client() -> Result<RuntimeCliClient, String> {
    RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Production))
        .map_err(|error| error.to_string())
}

pub(super) fn resource_for_target(target: ModelDownloadTarget) -> Option<RuntimeResourceRefWireV1> {
    match target {
        ModelDownloadTarget::RoFormer => RuntimeResourceRefWireV1::bundle("roformer"),
        ModelDownloadTarget::FireRed => RuntimeResourceRefWireV1::model("firered_asr2_aed"),
        ModelDownloadTarget::QwenAsr => RuntimeResourceRefWireV1::model("qwen3_asr_1_7b"),
        ModelDownloadTarget::QwenAlign => {
            RuntimeResourceRefWireV1::model("qwen3_forced_aligner_0_6b")
        }
        ModelDownloadTarget::Pitch => RuntimeResourceRefWireV1::model("rmvpe"),
        ModelDownloadTarget::Fcpe => RuntimeResourceRefWireV1::model("fcpe"),
        ModelDownloadTarget::Game => RuntimeResourceRefWireV1::model("game"),
        ModelDownloadTarget::Stars => RuntimeResourceRefWireV1::model("stars"),
        ModelDownloadTarget::BasicPitch => RuntimeResourceRefWireV1::model("basic_pitch"),
    }
    .ok()
}

fn status_copy(target: ModelDownloadTarget) -> (&'static str, &'static str) {
    match target {
        ModelDownloadTarget::RoFormer => (
            "RoFormer audio processing",
            "Vocal, instrumental, harmony, denoise and dereverb resources.",
        ),
        ModelDownloadTarget::FireRed => (
            "FireRed ASR2 AED",
            "Optional transcription challenger for comparison and diagnostics.",
        ),
        ModelDownloadTarget::QwenAsr => ("Qwen3-ASR-1.7B", "Primary local transcription resource."),
        ModelDownloadTarget::QwenAlign => (
            "Qwen3 Forced Aligner 0.6B",
            "Word-level alignment against canonical or transcribed lyrics.",
        ),
        ModelDownloadTarget::Pitch => ("RMVPE", "Primary continuous-pitch evidence."),
        ModelDownloadTarget::Fcpe => ("FCPE", "Optional secondary pitch evidence."),
        ModelDownloadTarget::Game => ("GAME", "Primary note and boundary evidence."),
        ModelDownloadTarget::Stars => ("STARS", "Optional advanced note challenger."),
        ModelDownloadTarget::BasicPitch => (
            "Basic Pitch",
            "Optional note/onset challenger for disagreement review.",
        ),
    }
}

fn status_for(
    statuses: &[RuntimeResourceStatusWireV1],
    target: ModelDownloadTarget,
) -> Option<ModelInstallStatus> {
    let resource = resource_for_target(target)?;
    let status = find_status(statuses, &resource.0)?;
    let (label, description) = status_copy(target);
    Some(ModelInstallStatus {
        target,
        label: label.to_string(),
        description: description.to_string(),
        available: status.usable,
        backend: status
            .selected_backend
            .map(backend_label)
            .unwrap_or("unresolved")
            .to_string(),
        validation: validation_label(status.validation_state).to_string(),
    })
}

fn backend_label(backend: NativeBackendWireV1) -> &'static str {
    match backend {
        NativeBackendWireV1::OpenVino => "openvino",
        NativeBackendWireV1::Vulkan => "vulkan",
        NativeBackendWireV1::NativeDsp => "native",
        NativeBackendWireV1::CpuReference => "diagnostic_cpu",
    }
}

fn validation_label(validation: ValidationStateWireV1) -> &'static str {
    match validation {
        ValidationStateWireV1::ProductionPinned => "production_pinned",
        ValidationStateWireV1::BenchmarkCandidate => "benchmark_candidate",
        ValidationStateWireV1::Experimental => "experimental",
        ValidationStateWireV1::Unsupported => "unsupported",
    }
}

const MODEL_STATUS_TARGETS: [ModelDownloadTarget; 9] = [
    ModelDownloadTarget::RoFormer,
    ModelDownloadTarget::FireRed,
    ModelDownloadTarget::QwenAsr,
    ModelDownloadTarget::QwenAlign,
    ModelDownloadTarget::Pitch,
    ModelDownloadTarget::Fcpe,
    ModelDownloadTarget::Game,
    ModelDownloadTarget::Stars,
    ModelDownloadTarget::BasicPitch,
];

fn model_install_statuses_from_statuses(
    statuses: &[RuntimeResourceStatusWireV1],
) -> Vec<ModelInstallStatus> {
    MODEL_STATUS_TARGETS
        .into_iter()
        .filter_map(|target| status_for(statuses, target))
        .collect()
}

#[cfg(test)]
pub(super) fn model_install_statuses_with_client(
    client: &RuntimeCliClient,
) -> Vec<ModelInstallStatus> {
    client
        .list()
        .map(|statuses| model_install_statuses_from_statuses(&statuses))
        .unwrap_or_default()
}

pub fn model_install_statuses() -> Vec<ModelInstallStatus> {
    analysis_runtime_status().models
}

const ANALYSIS_STRATEGY_RESOURCES: [(&str, &str, &str, &str); 5] = [
    (
        "vocal_extraction",
        "Vocal extraction",
        "bs_roformer_vocals_ep317",
        "audio.extract_vocals",
    ),
    (
        "instrumental_extraction",
        "Instrumental extraction",
        "melband_roformer_inst_v2",
        "audio.extract_instrumental",
    ),
    (
        "lead_isolation",
        "Lead isolation",
        "melband_roformer_harmony",
        "audio.lead_isolate",
    ),
    ("pitch", "Continuous pitch", "rmvpe", "pitch.track"),
    ("note_boundaries", "Note boundaries", "game", "notes.game"),
];

pub(super) fn strategy_resource_statuses_from_details(
    details: &[RuntimeResourceDetailsWireV1],
) -> Vec<AnalysisStrategyResourceStatus> {
    ANALYSIS_STRATEGY_RESOURCES
        .into_iter()
        .map(|(strategy_id, label, model_id, capability)| {
            let resource = format!("model:{model_id}");
            let detail = details.iter().find(|detail| detail.resource.0 == resource);
            let capability_reported = detail.is_some_and(|detail| {
                detail
                    .metadata
                    .capabilities
                    .iter()
                    .any(|reported| reported == capability)
            });
            let mut reasons = detail
                .map(|detail| {
                    detail
                        .status
                        .reasons
                        .iter()
                        .map(|reason| format!("{reason:?}").to_ascii_lowercase())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| vec!["status_unavailable".to_string()]);
            if detail.is_some() && !capability_reported {
                reasons.push("capability_missing".to_string());
            }
            AnalysisStrategyResourceStatus {
                strategy_id: strategy_id.to_string(),
                label: label.to_string(),
                model_id: model_id.to_string(),
                capability: capability.to_string(),
                available: detail.is_some_and(|detail| detail.status.usable) && capability_reported,
                backend: detail
                    .and_then(|detail| detail.status.selected_backend)
                    .map(backend_label)
                    .unwrap_or("unresolved")
                    .to_string(),
                validation: detail
                    .map(|detail| validation_label(detail.status.validation_state))
                    .unwrap_or("unsupported")
                    .to_string(),
                reasons,
            }
        })
        .collect()
}

pub(super) fn analysis_strategy_resource_statuses_with_client(
    client: &RuntimeCliClient,
) -> Result<Vec<AnalysisStrategyResourceStatus>, String> {
    let details = ANALYSIS_STRATEGY_RESOURCES
        .iter()
        .map(|(_, _, model_id, _)| {
            let resource = RuntimeResourceRefWireV1::model(model_id)?;
            client.show(&resource).map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(strategy_resource_statuses_from_details(&details))
}

/// Read exact model/capability readiness from Runtime Manager. This query is
/// read-only and intentionally does not consult or reinterpret the aggregate
/// RoFormer bundle.
pub fn analysis_strategy_resource_statuses() -> Result<Vec<AnalysisStrategyResourceStatus>, String>
{
    analysis_strategy_resource_statuses_with_client(&runtime_client()?)
}

fn find_status<'a>(
    statuses: &'a [RuntimeResourceStatusWireV1],
    resource: &str,
) -> Option<&'a RuntimeResourceStatusWireV1> {
    statuses.iter().find(|status| status.resource.0 == resource)
}

pub(super) fn analysis_runtime_status_with_clients(
    analysis_ready: bool,
    runtime_client: Option<&RuntimeCliClient>,
    ffmpeg: PathBuf,
) -> AnalysisRuntimeStatus {
    let statuses = runtime_client
        .and_then(|client| client.list().ok())
        .unwrap_or_default();
    let models = model_install_statuses_from_statuses(&statuses);
    let ffmpeg_available =
        find_status(&statuses, "tool:ffmpeg").is_some_and(|status| status.usable);
    let runtime_executable_ready = |id: &str| {
        find_status(&statuses, &format!("runtime:{id}"))
            .is_some_and(|status| status.executable_ready)
    };
    let selected_models = statuses
        .iter()
        .filter_map(|status| status.resource.0.strip_prefix("model:").map(str::to_string))
        .collect::<Vec<_>>();
    let selected_models_available = statuses
        .iter()
        .filter(|status| status.resource.0.starts_with("model:"))
        .all(|status| status.usable);
    let mut missing = Vec::new();
    if !analysis_ready {
        missing.push("Analysis Engine CLI".to_string());
    }
    if runtime_client.is_none() {
        missing.push("Runtime Manager CLI".to_string());
    }
    if !ffmpeg_available {
        missing.push("ffmpeg".to_string());
    }
    AnalysisRuntimeStatus {
        ready: analysis_ready && runtime_client.is_some() && ffmpeg_available,
        runtime_contract_current: analysis_ready && runtime_client.is_some(),
        ffmpeg_available,
        openvino_runtime_available: runtime_executable_ready("openvino_2026_3"),
        ggml_vulkan_runtime_available: runtime_executable_ready("ggml_vulkan_v1"),
        qwen_asr_runtime_available: runtime_executable_ready("qwen_asr_runtime"),
        qwen_align_runtime_available: runtime_executable_ready("qwen_align_runtime"),
        pitch_model_available: find_status(&statuses, "model:rmvpe")
            .is_some_and(|status| status.usable),
        selected_models_available,
        selected_models,
        models,
        compute_backend: "Runtime Manager Production policy".to_string(),
        ffmpeg_path: ffmpeg
            .is_file()
            .then(|| ffmpeg.to_string_lossy().into_owned()),
        missing,
    }
}

const RUNTIME_STATUS_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct RuntimeStatusCache {
    value: Option<AnalysisRuntimeStatus>,
    refreshed_at: Option<Instant>,
}

static RUNTIME_STATUS_CACHE: OnceLock<Mutex<RuntimeStatusCache>> = OnceLock::new();

fn runtime_status_cache() -> &'static Mutex<RuntimeStatusCache> {
    RUNTIME_STATUS_CACHE.get_or_init(|| Mutex::new(RuntimeStatusCache::default()))
}

pub fn invalidate_analysis_runtime_status_cache() {
    if let Ok(mut cache) = runtime_status_cache().lock() {
        cache.value = None;
        cache.refreshed_at = None;
    }
    crate::audio_processing::invalidate_audio_model_catalog_cache();
    crate::runtime_presentation::invalidate_runtime_presentation_cache();
}

fn compute_analysis_runtime_status() -> AnalysisRuntimeStatus {
    let analysis_ready = AnalysisCliClient::is_available();
    let runtime = runtime_client().ok();
    analysis_runtime_status_with_clients(analysis_ready, runtime.as_ref(), ffmpeg_path())
}

pub fn analysis_runtime_status() -> AnalysisRuntimeStatus {
    if let Ok(cache) = runtime_status_cache().lock()
        && cache
            .refreshed_at
            .is_some_and(|refreshed| refreshed.elapsed() < RUNTIME_STATUS_CACHE_TTL)
        && let Some(status) = cache.value.as_ref()
    {
        return status.clone();
    }

    let status = compute_analysis_runtime_status();
    if let Ok(mut cache) = runtime_status_cache().lock() {
        cache.value = Some(status.clone());
        cache.refreshed_at = Some(Instant::now());
    }
    status
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
        relocate_directory_contents(&crate::cache::models_dir(), target)?;
    }
    Ok(())
}
