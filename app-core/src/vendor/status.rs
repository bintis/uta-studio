use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::*;
use crate::cache::{
    CachePaths, models_dir, normalized_target_path, relocate_directory_contents, songs_cache_dir,
    uta_studio_dir, vendor_dir,
};

pub fn resolve_data_path_input(input: &str) -> Result<PathBuf, String> {
    normalized_target_path(PathBuf::from(input))
}

// ─── Directory Helpers ───────────────────────────────────────────────

pub(crate) fn configured_file_path(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable).and_then(|path| {
        let path = PathBuf::from(path);
        path.is_file().then_some(path)
    })
}

pub(crate) fn executable_on_path(names: &[&str]) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

pub(crate) fn executable_path(variable: &str, names: &[&str]) -> Option<PathBuf> {
    configured_file_path(variable).or_else(|| executable_on_path(names))
}

pub fn ffmpeg_path() -> PathBuf {
    // Packaged builds can provide an explicit path, while development and
    // ordinary desktop launches can inherit ffmpeg through PATH. Both take
    // precedence over the app-managed fallback.
    let names = if cfg!(windows) {
        &["ffmpeg.exe", "ffmpeg"][..]
    } else {
        &["ffmpeg"][..]
    };
    if let Some(path) = executable_path("UTA_STUDIO_FFMPEG_PATH", names) {
        return path;
    }

    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };

    vendor_dir().join(name)
}

pub fn python_path() -> PathBuf {
    if cfg!(windows) {
        vendor_dir().join("venv").join("Scripts").join("python.exe")
    } else {
        vendor_dir().join("venv").join("bin").join("python")
    }
}

pub fn analyzer_dir() -> PathBuf {
    vendor_dir().join("analyzer")
}

pub(crate) fn uv_path() -> PathBuf {
    let names = if cfg!(windows) {
        &["uv.exe", "uv"][..]
    } else {
        &["uv"][..]
    };
    if let Some(path) = executable_path("UTA_STUDIO_UV_PATH", names) {
        return path;
    }

    let name = if cfg!(windows) { "uv.exe" } else { "uv" };
    vendor_dir().join(name)
}

pub(crate) fn configured_python_path() -> Option<PathBuf> {
    let names = if cfg!(windows) {
        &["python3.11.exe", "python3.10.exe", "python.exe"][..]
    } else {
        &["python3.11", "python3.10", "python3", "python"][..]
    };
    executable_path("UTA_STUDIO_PYTHON_PATH", names)
}

pub(crate) fn ready_marker() -> PathBuf {
    vendor_dir().join(".ready")
}

pub(crate) fn configured_backend_name() -> &'static str {
    match crate::config::AppConfig::load().compute_backend.as_deref() {
        Some("cuda") => "cuda",
        Some("intel") => "intel",
        _ => "cpu",
    }
}

pub(crate) fn expected_ready_marker() -> String {
    // Version 5 pins audio-separator==0.44.5 and the offline adapter contract.
    // v4 remains recognizable for data-directory discovery only.
    format!("runtime-v5:{}", configured_backend_name())
}

pub(crate) fn ready_marker_is_compatible(value: &str) -> bool {
    ready_marker_is_compatible_for(value, configured_backend_name())
}

pub(crate) fn ready_marker_is_compatible_for(value: &str, backend: &str) -> bool {
    matches!(
        value.trim(),
        marker if marker == format!("runtime-v5:{backend}")
            || marker == format!("nix-runtime-v5:{backend}")
    )
}

pub(crate) fn ready_marker_is_usable(value: &str) -> bool {
    ready_marker_is_usable_for(value, configured_backend_name())
}

pub(crate) fn ready_marker_is_usable_for(value: &str, backend: &str) -> bool {
    ready_marker_is_compatible_for(value, backend)
        || matches!(
            value.trim(),
            marker if marker == format!("runtime-v4:{backend}")
                || marker == format!("nix-runtime-v4:{backend}")
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadyMarkerState {
    Current,
    Outdated,
    Missing,
}

pub(crate) fn ready_marker_state() -> ReadyMarkerState {
    match std::fs::read_to_string(ready_marker()) {
        Ok(value) if ready_marker_is_compatible(&value) => ReadyMarkerState::Current,
        Ok(value) if ready_marker_is_usable(&value) => ReadyMarkerState::Outdated,
        _ => ReadyMarkerState::Missing,
    }
}

pub(crate) fn pitch_model_path() -> PathBuf {
    models_dir().join("pitch").join("rmvpe").join("rmvpe.onnx")
}

pub(crate) fn openvino_whisper_model_dir() -> PathBuf {
    models_dir().join("whisper").join("openvino-large-v3-turbo")
}

pub(crate) fn openvino_whisper_model_ready() -> bool {
    let dir = openvino_whisper_model_dir();
    dir.join("config.json").is_file() && dir.join("openvino_encoder_model.xml").is_file()
}

pub(crate) fn openvino_separator_models_dir() -> PathBuf {
    models_dir().join("separation")
}

pub(crate) fn openvino_separator_models_ready() -> bool {
    let dir = openvino_separator_models_dir();
    dir.join("openvino-demucs")
        .join("htdemucs_v4")
        .join("htdemucs_fwd.xml")
        .is_file()
        && dir
            .join("openvino-demucs")
            .join("htdemucs_v4")
            .join("htdemucs_fwd.bin")
            .is_file()
}

pub(crate) fn huggingface_snapshot_has(repository: &str, required_files: &[&str]) -> bool {
    let repository_dir = models_dir()
        .join("huggingface")
        .join("hub")
        .join(format!("models--{}", repository.replace('/', "--")))
        .join("snapshots");
    let Ok(snapshots) = std::fs::read_dir(repository_dir) else {
        return false;
    };
    snapshots.flatten().any(|snapshot| {
        let root = snapshot.path();
        required_files.iter().all(|file| root.join(file).is_file())
    })
}

pub(crate) fn whisper_repository(model: &str) -> String {
    if model == "large-v3-turbo" {
        "mobiuslabsgmbh/faster-whisper-large-v3-turbo".to_string()
    } else {
        format!("Systran/faster-whisper-{model}")
    }
}

pub(crate) fn parakeet_model_ready(backend: &str) -> bool {
    if backend == "cuda" {
        models_dir()
            .join("selected")
            .join("parakeet-cuda.ready")
            .is_file()
    } else {
        huggingface_snapshot_has(
            "istupakov/parakeet-tdt-0.6b-v3-onnx",
            &["encoder-model.int8.onnx", "decoder_joint-model.int8.onnx"],
        )
    }
}

pub(crate) fn whisper_model_ready(model: &str) -> bool {
    huggingface_snapshot_has(&whisper_repository(model), &["config.json", "model.bin"])
}

pub(crate) fn separator_model_status(separator: &str) -> (String, String, bool) {
    match separator {
        "demucs" => (
            "Demucs separator".to_string(),
            "Separates vocals with the selected Demucs model.".to_string(),
            model_checkpoint_with_prefix_exists(
                &models_dir().join("torch").join("hub").join("checkpoints"),
                "955717e8",
            ),
        ),
        "openvino_demucs" => (
            "OpenVINO Demucs separator".to_string(),
            "Intel GPU stem-separation model selected in Analysis.".to_string(),
            openvino_separator_models_ready(),
        ),
        _ => (
            "UVR Karaoke separator".to_string(),
            "Vocal-isolation model selected in Analysis.".to_string(),
            models_dir()
                .join("audio_separator")
                .join("mel_band_roformer_karaoke_aufr33_viperx_sdr_10.1956.ckpt")
                .is_file(),
        ),
    }
}

pub(crate) fn model_checkpoint_with_prefix_exists(directory: &Path, prefix: &str) -> bool {
    std::fs::read_dir(directory).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.path().is_file() && entry.file_name().to_string_lossy().starts_with(prefix)
        })
    })
}

pub(crate) fn qwen_alignment_model_status(align_backend: &str) -> Option<(String, String, bool)> {
    (align_backend == "qwen").then(|| {
        (
            "Qwen forced aligner".to_string(),
            "Optional word-timing model selected for Whisper alignment.".to_string(),
            huggingface_snapshot_has(
                "Qwen/Qwen3-ForcedAligner-0.6B-hf",
                &["config.json", "model.safetensors"],
            ),
        )
    })
}

pub(crate) fn mms_karaoke_alignment_model_ready() -> bool {
    huggingface_snapshot_has(
        "NextFire/mms-300m-ForcedAligner-karaoke-ja-Latn",
        &[
            "config.json",
            "added_tokens.json",
            "model.safetensors",
            "processor_config.json",
            "tokenizer_config.json",
            "vocab.json",
        ],
    )
}

/// Which separator/ASR/alignment models `model_install_statuses` should
/// check readiness for. Extracted so status can be computed for a specific
/// song's *resolved* config (global defaults overridden by
/// `song_analysis_profile`, see `node_model_availability_for`) instead of
/// only ever the process-global `AppConfig::load()` -- §8.6's real blocker
/// (see docs/plan.md): computing a song's model availability by reading
/// global config directly gets the wrong answer for any song whose profile
/// overrides separator/asr_engine/align_backend.
pub struct ModelAvailabilityParams<'a> {
    pub backend: &'a str,
    pub separator: &'a str,
    pub align_backend: &'a str,
    pub asr_engine: &'a str,
    pub whisper_model: &'a str,
}

pub fn model_install_statuses() -> Vec<ModelInstallStatus> {
    let config = crate::config::AppConfig::load();
    model_install_statuses_for(ModelAvailabilityParams {
        backend: configured_backend_name(),
        separator: config.separator(),
        align_backend: config.align_backend(),
        asr_engine: config.asr_engine(),
        whisper_model: config.whisper_model(),
    })
}

pub(crate) fn model_install_statuses_for(
    params: ModelAvailabilityParams,
) -> Vec<ModelInstallStatus> {
    let ModelAvailabilityParams {
        backend,
        separator,
        align_backend,
        asr_engine,
        whisper_model,
    } = params;
    let mut models = Vec::new();

    if backend == "intel" && asr_engine == "whisper" {
        models.push(ModelInstallStatus {
            target: ModelDownloadTarget::OpenVinoWhisper,
            label: "OpenVINO Whisper large-v3-turbo".to_string(),
            description: "Primary Intel GPU transcription model.".to_string(),
            available: openvino_whisper_model_ready(),
        });
    }

    if asr_engine == "parakeet" {
        models.push(ModelInstallStatus {
            target: ModelDownloadTarget::Parakeet,
            label: "Parakeet v3 transcription".to_string(),
            description: "Primary fast transcription model for supported languages.".to_string(),
            available: parakeet_model_ready(backend),
        });
    }

    if asr_engine == "parakeet" || backend == "intel" {
        models.push(ModelInstallStatus {
            target: ModelDownloadTarget::WhisperLanguageDetection,
            label: "Whisper Tiny language detection".to_string(),
            description: if asr_engine == "parakeet" {
                "Detects the language before choosing Parakeet or the Whisper fallback.".to_string()
            } else {
                "Detects the language before Intel OpenVINO Whisper transcription.".to_string()
            },
            available: whisper_model_ready("tiny"),
        });
    }

    models.push(ModelInstallStatus {
        target: ModelDownloadTarget::Whisper,
        label: format!("Whisper {whisper_model}"),
        description: if asr_engine == "parakeet" || backend == "intel" {
            "Language and compatibility fallback used when the primary engine cannot run."
                .to_string()
        } else {
            "Primary multilingual transcription model.".to_string()
        },
        available: whisper_model_ready(whisper_model),
    });

    let (label, description, available) = separator_model_status(separator);
    models.push(ModelInstallStatus {
        target: ModelDownloadTarget::Separator,
        label,
        description,
        available,
    });

    if let Some((label, description, available)) = qwen_alignment_model_status(align_backend) {
        models.push(ModelInstallStatus {
            target: ModelDownloadTarget::Alignment,
            label,
            description,
            available,
        });
    }

    models.push(ModelInstallStatus {
        target: ModelDownloadTarget::MmsKaraokeAlignment,
        label: "MMS Karaoke Japanese aligner".to_string(),
        description: if align_backend == "mms_karaoke" {
            "Selected 1.26 GB Japanese karaoke alignment model (AGPL-3.0).".to_string()
        } else {
            "Optional 1.26 GB Japanese karaoke alignment model (AGPL-3.0); select MMS Karaoke in Analysis to use it."
                .to_string()
        },
        available: mms_karaoke_alignment_model_ready(),
    });

    models.push(ModelInstallStatus {
        target: ModelDownloadTarget::Pitch,
        label: "RMVPE pitch detection".to_string(),
        description: "Detects the sung melody and creates editable note pitches.".to_string(),
        available: pitch_model_path().is_file(),
    });
    models
}

/// Pure routing logic behind `node_model_availability_for`: given
/// already-resolved readiness booleans for the individual models a specific
/// asr_engine/backend/align_backend combination actually depends on, decides
/// whether each analysis node's required model(s) are present. Split out
/// from the real filesystem checks so this branching (which check(s) matter
/// for which combination) is unit-testable without touching the real,
/// process-global `models_dir()`.
pub(crate) fn node_model_availability_from_checks(
    separator_ready: bool,
    pitch_ready: bool,
    asr_engine: &str,
    backend: &str,
    primary_asr_model_ready: bool,
    language_detector_ready: bool,
    align_backend: &str,
    align_model_ready: bool,
) -> std::collections::BTreeMap<crate::analysis_graph::AnalysisNodeId, bool> {
    use crate::analysis_graph::AnalysisNodeId;

    let mut map = std::collections::BTreeMap::new();
    map.insert(AnalysisNodeId::new("stems.separate"), separator_ready);
    map.insert(AnalysisNodeId::new("pitch.extract"), pitch_ready);

    // Parakeet and Intel OpenVINO Whisper both also need the tiny Whisper
    // language detector as a real prerequisite step (mirrors
    // `model_install_statuses_for`'s own `asr_engine == "parakeet" ||
    // backend == "intel"` condition) -- the plain CPU/CUDA Whisper path
    // doesn't need a separate detector model, it detects language with the
    // same model it transcribes with.
    let transcribe_ready = if asr_engine == "parakeet" || backend == "intel" {
        primary_asr_model_ready && language_detector_ready
    } else {
        primary_asr_model_ready
    };
    map.insert(AnalysisNodeId::new("lyrics.transcribe"), transcribe_ready);

    // "whisperx"/"ctc" forced alignment resolve their wav2vec2 model
    // per-language on demand (`cjk::align_model_for`) rather than one fixed
    // model tracked up front -- `model_install_statuses` has never listed
    // one for these two backends, so there is nothing to gate on here
    // either; only the two backends with a single fixed, trackable model
    // (qwen, mms_karaoke) can genuinely block the node.
    let align_ready = match align_backend {
        "qwen" | "mms_karaoke" => align_model_ready,
        _ => true,
    };
    map.insert(AnalysisNodeId::new("lyrics.align"), align_ready);

    map
}

/// Per-song model availability for `AnalysisRequest.model_availability`,
/// resolved against `params` (the song's *effective* separator/asr_engine/
/// align_backend/backend -- global defaults overridden by
/// `song_analysis_profile`) rather than global `AppConfig`. This is the
/// real §8.6 wiring: every prior call site left `model_availability` an
/// empty `BTreeMap`, so `build_plan`'s "model missing -> Blocked" branch
/// (`analysis_plan.rs`'s `model_available = ...unwrap_or(true)`) was
/// unreachable in practice, not merely untested.
pub fn node_model_availability_for(
    params: &ModelAvailabilityParams,
) -> std::collections::BTreeMap<crate::analysis_graph::AnalysisNodeId, bool> {
    let separator_ready = separator_model_status(params.separator).2;
    let pitch_ready = pitch_model_path().is_file();
    let primary_asr_model_ready = if params.asr_engine == "parakeet" {
        parakeet_model_ready(params.backend)
    } else if params.backend == "intel" {
        openvino_whisper_model_ready()
    } else {
        whisper_model_ready(params.whisper_model)
    };
    let language_detector_ready = whisper_model_ready("tiny");
    let align_model_ready = match params.align_backend {
        "qwen" => qwen_alignment_model_status(params.align_backend)
            .map(|(_, _, ready)| ready)
            .unwrap_or(true),
        "mms_karaoke" => mms_karaoke_alignment_model_ready(),
        _ => true,
    };
    node_model_availability_from_checks(
        separator_ready,
        pitch_ready,
        params.asr_engine,
        params.backend,
        primary_asr_model_ready,
        language_detector_ready,
        params.align_backend,
        align_model_ready,
    )
}

/// Builds `ModelAvailabilityParams` from a song's resolved analysis profile
/// (`song_analysis_profile`, already merged with global defaults by
/// `get_song_analysis_profile`) plus the process-global compute backend --
/// `compute_backend` (cuda/intel/cpu) and the specific Whisper model size
/// are not among the fields `song_analysis_profile` can override (see
/// `AnalysisProfileSnapshot`), so those two stay sourced from `AppConfig`
/// while separator/asr_engine/align_backend come from the song's own
/// profile. The real fix for §8.6: every caller used to build this from
/// `AppConfig::load()` alone, silently ignoring a song's own profile.
pub fn model_availability_params_for_profile(
    profile: &crate::analysis_profile::AnalysisProfileSnapshot,
) -> ModelAvailabilityParams<'_> {
    let config = crate::config::AppConfig::load();
    ModelAvailabilityParams {
        backend: configured_backend_name(),
        separator: &profile.separator,
        align_backend: &profile.alignment_backend,
        asr_engine: &profile.asr_engine,
        whisper_model: match config.whisper_model() {
            "tiny" => "tiny",
            "base" => "base",
            "small" => "small",
            "medium" => "medium",
            "large-v3" => "large-v3",
            "large-v3-turbo" => "large-v3-turbo",
            _ => "large-v3",
        },
    }
}

pub(crate) fn selected_models_status() -> (bool, Vec<String>, Vec<String>) {
    let config = crate::config::AppConfig::load();
    let models = model_install_statuses();
    let selected = models
        .iter()
        .filter(|model| {
            model.target != ModelDownloadTarget::MmsKaraokeAlignment
                || config.align_backend() == "mms_karaoke"
        })
        .map(|model| model.label.clone())
        .collect();
    let missing: Vec<_> = models
        .iter()
        .filter(|model| {
            model.target != ModelDownloadTarget::MmsKaraokeAlignment
                || config.align_backend() == "mms_karaoke"
        })
        .filter(|model| !model.available)
        .map(|model| format!("{} model", model.label))
        .collect();
    (missing.is_empty(), selected, missing)
}

pub fn analysis_runtime_status() -> AnalysisRuntimeStatus {
    let ffmpeg = ffmpeg_path();
    let uv = uv_path();
    let system_python = configured_python_path();
    let backend = configured_backend_name();
    let marker_state = ready_marker_state();
    let runtime_contract_current = marker_state == ReadyMarkerState::Current;
    let runtime_usable = marker_state != ReadyMarkerState::Missing;
    let ffmpeg_available = ffmpeg.is_file();
    let uv_available = uv.is_file();
    let managed_runtime_available = python_path().is_file();
    let analyzer_available = analyzer_dir().join("analyze.py").is_file();
    let pitch_model_available = pitch_model_path().is_file();
    let config = crate::config::AppConfig::load();
    let backend_models_available =
        backend != "intel" || config.asr_engine() != "whisper" || openvino_whisper_model_ready();
    let (selected_models_available, selected_models, selected_models_missing) =
        selected_models_status();
    let models = model_install_statuses();

    let mut missing = Vec::new();
    if !ffmpeg_available {
        missing.push("ffmpeg".to_string());
    }
    if !managed_runtime_available {
        missing.push("analysis runtime".to_string());
    }
    if !analyzer_available {
        missing.push("analyzer scripts".to_string());
    }
    missing.extend(selected_models_missing);
    if !runtime_usable && missing.is_empty() {
        missing.push("setup has not finished".to_string());
    }
    missing.sort();
    missing.dedup();

    AnalysisRuntimeStatus {
        ready: runtime_usable
            && ffmpeg_available
            && managed_runtime_available
            && analyzer_available
            && pitch_model_available
            && backend_models_available
            && selected_models_available,
        runtime_contract_current,
        ffmpeg_available,
        uv_available,
        system_python_available: system_python.is_some(),
        managed_runtime_available,
        analyzer_available,
        pitch_model_available,
        backend_models_available,
        selected_models_available,
        selected_models,
        models,
        compute_backend: backend.to_string(),
        ffmpeg_path: ffmpeg_available.then(|| ffmpeg.to_string_lossy().into_owned()),
        uv_path: uv_available.then(|| uv.to_string_lossy().into_owned()),
        system_python_path: system_python.map(|path| path.to_string_lossy().into_owned()),
        missing,
    }
}

pub fn is_ready() -> bool {
    analysis_runtime_status().ready
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SetupFolders {
    pub data_path: Option<String>,
    pub cache_paths: Option<CachePaths>,
    #[serde(default)]
    pub compute_backend: ComputeBackend,
    #[serde(default)]
    pub model_target: Option<ModelDownloadTarget>,
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
    let source_songs = songs_cache_dir();
    let source_models = models_dir();

    if let Some(target) = targets.songs.as_ref() {
        relocate_directory_contents(&source_songs, target)?;
    }
    if let Some(target) = targets.models.as_ref() {
        relocate_directory_contents(&source_models, target)?;
    }

    Ok(())
}
