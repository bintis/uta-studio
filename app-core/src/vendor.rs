use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use tracing::info;
use ts_rs::TS;

use crate::{
    cache::{
        CachePaths, models_dir, normalized_target_path, relocate_app_data_path,
        relocate_directory_contents, same_path, songs_cache_dir, uta_studio_dir, vendor_dir,
    },
    vendor_scripts,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum SetupStep {
    PrepareFolders,
    ClearVendor,
    Ffmpeg,
    Uv,
    Python,
    Venv,
    Dependencies,
    ExtractScripts,
    OpenVinoWhisper,
    PitchModel,
    SelectedModels,
    Finish,
}

/// The compute runtime selected before installing the Python environment.
/// It is deliberately explicit: silently picking CUDA on a mixed-GPU system
/// can download several GB of the wrong runtime.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum ComputeBackend {
    #[default]
    Cpu,
    Cuda,
    Intel,
}

/// A concrete, user-visible model family that can be prepared independently.
/// Configurable families read their selected variant from `AppConfig` when the
/// job starts. Explicit optional families use their own variant so their model
/// row can remain available without silently changing the active analysis setup.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq, Hash)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ModelDownloadTarget {
    Whisper,
    WhisperLanguageDetection,
    Parakeet,
    Separator,
    Alignment,
    MmsKaraokeAlignment,
    Pitch,
    OpenVinoWhisper,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ModelInstallStatus {
    pub target: ModelDownloadTarget,
    pub label: String,
    pub description: String,
    pub available: bool,
}

impl ComputeBackend {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
            Self::Intel => "intel",
        }
    }
}

/// Return the normalized distribution name separately from the versioned
/// requirement. `uv --reinstall-package` accepts only the former, while the
/// positional install argument should retain the latter.
fn onnx_runtime_package(backend: ComputeBackend) -> (&'static str, &'static str) {
    match backend {
        ComputeBackend::Cpu => ("onnxruntime", "onnxruntime>=1.17"),
        ComputeBackend::Cuda => ("onnxruntime-gpu", "onnxruntime-gpu>=1.17"),
        ComputeBackend::Intel => ("onnxruntime-openvino", "onnxruntime-openvino>=1.17"),
    }
}

fn inference_runtime_reinstall_args(backend: ComputeBackend, python: &str) -> Vec<&str> {
    let (package_name, package_requirement) = onnx_runtime_package(backend);
    vec![
        "pip",
        "install",
        "--reinstall-package",
        package_name,
        package_requirement,
        "--python",
        python,
    ]
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum SetupTaskState {
    Pending,
    Running,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetupTask {
    pub step: SetupStep,
    pub label: String,
    pub state: SetupTaskState,
    /// Bytes received for direct downloads, or bytes currently installed in
    /// the virtualenv while a package job is running. `None` means that the
    /// upstream tool did not report a meaningful figure.
    pub downloaded_bytes: Option<u64>,
    /// The server's Content-Length where available. Package resolvers usually
    /// do not expose a reliable total, so this is intentionally optional.
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetupProgress {
    pub step: SetupStep,
    pub percent: usize,
    pub action: String,
    pub tasks: Vec<SetupTask>,
}

/// A read-only snapshot used by the UI to explain why analysis is available
/// or unavailable. Merely loading this value never downloads or changes
/// anything on disk.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisRuntimeStatus {
    pub ready: bool,
    pub ffmpeg_available: bool,
    pub uv_available: bool,
    pub system_python_available: bool,
    pub managed_runtime_available: bool,
    pub analyzer_available: bool,
    pub pitch_model_available: bool,
    pub backend_models_available: bool,
    pub selected_models_available: bool,
    pub selected_models: Vec<String>,
    pub models: Vec<ModelInstallStatus>,
    pub compute_backend: String,
    pub ffmpeg_path: Option<String>,
    pub uv_path: Option<String>,
    pub system_python_path: Option<String>,
    pub missing: Vec<String>,
}

pub fn resolve_data_path_input(input: &str) -> Result<PathBuf, String> {
    normalized_target_path(PathBuf::from(input))
}

// ─── Directory Helpers ───────────────────────────────────────────────

fn configured_file_path(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable).and_then(|path| {
        let path = PathBuf::from(path);
        path.is_file().then_some(path)
    })
}

fn executable_on_path(names: &[&str]) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

fn executable_path(variable: &str, names: &[&str]) -> Option<PathBuf> {
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

fn uv_path() -> PathBuf {
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

fn configured_python_path() -> Option<PathBuf> {
    let names = if cfg!(windows) {
        &["python3.11.exe", "python3.10.exe", "python.exe"][..]
    } else {
        &["python3.11", "python3.10", "python3", "python"][..]
    };
    executable_path("UTA_STUDIO_PYTHON_PATH", names)
}

fn ready_marker() -> PathBuf {
    vendor_dir().join(".ready")
}

fn configured_backend_name() -> &'static str {
    match crate::config::AppConfig::load().compute_backend.as_deref() {
        Some("cuda") => "cuda",
        Some("intel") => "intel",
        _ => "cpu",
    }
}

fn expected_ready_marker() -> String {
    // Bump this whenever the managed Python runtime contract changes. Version
    // 4 adds the Intel OpenVINO Whisper and Demucs models.
    format!("runtime-v4:{}", configured_backend_name())
}

fn ready_marker_is_compatible(value: &str) -> bool {
    let backend = configured_backend_name();
    matches!(
        value.trim(),
        marker if marker == format!("runtime-v4:{backend}")
            || marker == format!("nix-runtime-v4:{backend}")
    )
}

fn pitch_model_path() -> PathBuf {
    models_dir().join("pitch").join("rmvpe").join("rmvpe.onnx")
}

fn openvino_whisper_model_dir() -> PathBuf {
    models_dir().join("whisper").join("openvino-large-v3-turbo")
}

fn openvino_whisper_model_ready() -> bool {
    let dir = openvino_whisper_model_dir();
    dir.join("config.json").is_file() && dir.join("openvino_encoder_model.xml").is_file()
}

fn openvino_separator_models_dir() -> PathBuf {
    models_dir().join("separation")
}

fn openvino_separator_models_ready() -> bool {
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

fn huggingface_snapshot_has(repository: &str, required_files: &[&str]) -> bool {
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

fn whisper_repository(model: &str) -> String {
    if model == "large-v3-turbo" {
        "mobiuslabsgmbh/faster-whisper-large-v3-turbo".to_string()
    } else {
        format!("Systran/faster-whisper-{model}")
    }
}

fn parakeet_model_ready(backend: &str) -> bool {
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

fn whisper_model_ready(model: &str) -> bool {
    huggingface_snapshot_has(&whisper_repository(model), &["config.json", "model.bin"])
}

fn separator_model_status(separator: &str) -> (String, String, bool) {
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

fn model_checkpoint_with_prefix_exists(directory: &Path, prefix: &str) -> bool {
    std::fs::read_dir(directory).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.path().is_file() && entry.file_name().to_string_lossy().starts_with(prefix)
        })
    })
}

fn qwen_alignment_model_status(align_backend: &str) -> Option<(String, String, bool)> {
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

fn mms_karaoke_alignment_model_ready() -> bool {
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

fn model_install_statuses_for(params: ModelAvailabilityParams) -> Vec<ModelInstallStatus> {
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

fn selected_models_status() -> (bool, Vec<String>, Vec<String>) {
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
    let marker_available = std::fs::read_to_string(ready_marker())
        .is_ok_and(|value| ready_marker_is_compatible(&value));
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
    if !marker_available && missing.is_empty() {
        missing.push("runtime verification".to_string());
    }
    missing.sort();
    missing.dedup();

    AnalysisRuntimeStatus {
        ready: marker_available
            && ffmpeg_available
            && managed_runtime_available
            && analyzer_available
            && pitch_model_available
            && backend_models_available
            && selected_models_available,
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

fn normalize_optional_path(path: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    path.map(normalized_target_path).transpose()
}

fn normalize_cache_paths(paths: CachePaths) -> Result<CachePaths, String> {
    Ok(CachePaths {
        songs: normalize_optional_path(paths.songs)?,
        models: normalize_optional_path(paths.models)?,
        vendor: normalize_optional_path(paths.vendor)?,
    })
}

fn default_cache_paths_for_data_root() -> CachePaths {
    let root = uta_studio_dir();
    CachePaths {
        songs: Some(root.join("cache")),
        models: Some(root.join("models")),
        vendor: Some(root.join("vendor")),
    }
}

fn relocate_cache_data_to_targets(targets: &CachePaths) -> Result<(), String> {
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

pub fn run_vendor_setup(
    folders: SetupFolders,
    mut on_progress: impl FnMut(SetupProgress) + Send,
    mut on_log: impl FnMut(String) + Send,
    mut on_data_relocated: impl FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let model_target = folders.model_target;
    let mut tasks = setup_tasks();

    if let Some(raw_path) = folders
        .data_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let target = resolve_data_path_input(raw_path)?;
        let current = uta_studio_dir();
        if !same_path(&current, &target) {
            emit_setup_progress(
                &mut on_progress,
                &mut tasks,
                SetupStep::PrepareFolders,
                12,
                "Relocating app data...".to_string(),
                None,
                None,
            );
            let new_path = relocate_app_data_path(target)?;
            on_data_relocated(&new_path)?;
            emit_setup_progress(
                &mut on_progress,
                &mut tasks,
                SetupStep::PrepareFolders,
                18,
                format!("Data relocated to {}", new_path.display()),
                None,
                None,
            );
        }
    }

    emit_setup_progress(
        &mut on_progress,
        &mut tasks,
        SetupStep::PrepareFolders,
        20,
        "Moving cache data to selected folders...".to_string(),
        None,
        None,
    );

    let separate_targets = folders.cache_paths.map(normalize_cache_paths).transpose()?;
    let targets = separate_targets
        .clone()
        .unwrap_or_else(default_cache_paths_for_data_root);
    let old_songs_cache = songs_cache_dir();
    relocate_cache_data_to_targets(&targets)?;
    if let Some(new_songs_cache) = targets.songs.as_ref() {
        crate::library_db::rebase_song_album_art_cache_paths(&old_songs_cache, new_songs_cache)?;
    }

    let mut cfg = crate::config::AppConfig::load();
    cfg.cache_paths = separate_targets;
    cfg.compute_backend = Some(folders.compute_backend.as_str().to_string());
    cfg.save()?;

    // A model choice can make analysis unavailable without invalidating the
    // already verified Python environment. In that case setup should prepare
    // only the newly selected model, not reinstall torch and every package.
    let managed_environment_ready = std::fs::read_to_string(ready_marker())
        .is_ok_and(|value| ready_marker_is_compatible(&value))
        && python_path().is_file();

    emit_setup_progress(
        &mut on_progress,
        &mut tasks,
        SetupStep::Ffmpeg,
        24,
        "Using system ffmpeg...".to_string(),
        None,
        None,
    );
    if !ffmpeg_path().is_file() {
        return Err(
            "System ffmpeg was not found. Install ffmpeg or launch Uta Studio from its packaged environment."
                .to_string(),
        );
    }

    if managed_environment_ready {
        emit_setup_progress(
            &mut on_progress,
            &mut tasks,
            SetupStep::Dependencies,
            70,
            "Reusing the verified analysis runtime...".to_string(),
            None,
            None,
        );
    } else {
        emit_setup_progress(
            &mut on_progress,
            &mut tasks,
            SetupStep::Uv,
            34,
            "Using system uv...".to_string(),
            None,
            None,
        );
        if !uv_path().is_file() {
            return Err(
                "System uv was not found. Install uv or launch Uta Studio from its packaged environment."
                    .to_string(),
            );
        }

        let python_action = if configured_python_path().is_some() {
            "Using system Python..."
        } else {
            "Installing python3.10 via uv..."
        };
        emit_setup_progress(
            &mut on_progress,
            &mut tasks,
            SetupStep::Python,
            46,
            python_action.to_string(),
            None,
            None,
        );
        step_install_python()?;

        emit_setup_progress(
            &mut on_progress,
            &mut tasks,
            SetupStep::Venv,
            58,
            "Setting up .venv...".to_string(),
            None,
            None,
        );
        step_create_venv()?;

        emit_setup_progress(
            &mut on_progress,
            &mut tasks,
            SetupStep::Dependencies,
            70,
            "Installing python dependencies...".to_string(),
            None,
            None,
        );
        step_install_packages_for_backend(
            folders.compute_backend,
            |installed_bytes| {
                emit_setup_progress(
                    &mut on_progress,
                    &mut tasks,
                    SetupStep::Dependencies,
                    70,
                    "Installing Python packages...".to_string(),
                    Some(installed_bytes),
                    None,
                );
            },
            &mut on_log,
        )?;
    }

    emit_setup_progress(
        &mut on_progress,
        &mut tasks,
        SetupStep::ExtractScripts,
        80,
        "Extracting analyzer scripts...".to_string(),
        None,
        None,
    );
    step_extract_scripts()?;

    let selected_config = crate::config::AppConfig::load();
    if model_target.is_none()
        && folders.compute_backend == ComputeBackend::Intel
        && selected_config.asr_engine() == "whisper"
    {
        emit_setup_progress(
            &mut on_progress,
            &mut tasks,
            SetupStep::OpenVinoWhisper,
            84,
            "Downloading OpenVINO Whisper model for Intel GPU...".to_string(),
            None,
            None,
        );
        step_download_openvino_whisper_model_with_output(&mut on_log)?;
    }
    if model_target.is_none()
        && folders.compute_backend == ComputeBackend::Intel
        && selected_config.separator() == "openvino_demucs"
    {
        emit_setup_progress(
            &mut on_progress,
            &mut tasks,
            SetupStep::OpenVinoWhisper,
            86,
            "Downloading OpenVINO Demucs model for Intel GPU...".to_string(),
            None,
            None,
        );
        step_download_openvino_separator_models_with_output(&mut on_log)?;
    }

    if model_target.is_none() {
        emit_setup_progress(
            &mut on_progress,
            &mut tasks,
            SetupStep::PitchModel,
            90,
            "Downloading RMVPE pitch model...".to_string(),
            None,
            None,
        );
        step_download_pitch_model_with_output(&mut on_log)?;
    }

    emit_setup_progress(
        &mut on_progress,
        &mut tasks,
        SetupStep::SelectedModels,
        94,
        match model_target {
            Some(target) => format!("Preparing {}...", model_target_label(target)),
            None => "Preparing the models selected in Settings...".to_string(),
        },
        None,
        None,
    );
    match model_target {
        Some(target) => step_download_model(target, &mut on_log)?,
        None => step_download_selected_models(on_log)?,
    }

    mark_ready()?;
    emit_setup_progress(
        &mut on_progress,
        &mut tasks,
        SetupStep::Finish,
        100,
        "Done".to_string(),
        None,
        None,
    );

    Ok(())
}

fn setup_tasks() -> Vec<SetupTask> {
    [
        (SetupStep::PrepareFolders, "Prepare data folders"),
        (SetupStep::Ffmpeg, "Use system ffmpeg"),
        (SetupStep::Uv, "Use system uv"),
        (SetupStep::Python, "Install Python runtime"),
        (SetupStep::Venv, "Create virtual environment"),
        (SetupStep::Dependencies, "Install AI and audio packages"),
        (SetupStep::ExtractScripts, "Install analyzer scripts"),
        (
            SetupStep::OpenVinoWhisper,
            "Download OpenVINO GPU models for Intel Arc",
        ),
        (SetupStep::PitchModel, "Download RMVPE pitch model"),
        (
            SetupStep::SelectedModels,
            "Prepare selected analysis models",
        ),
    ]
    .into_iter()
    .map(|(step, label)| SetupTask {
        step,
        label: label.to_string(),
        state: SetupTaskState::Pending,
        downloaded_bytes: None,
        total_bytes: None,
    })
    .collect()
}

fn emit_setup_progress(
    on_progress: &mut impl FnMut(SetupProgress),
    tasks: &mut [SetupTask],
    step: SetupStep,
    percent: usize,
    action: String,
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
) {
    for task in tasks.iter_mut() {
        if task.state == SetupTaskState::Running && task.step != step {
            task.state = SetupTaskState::Done;
        }
        if task.step == step {
            task.state = if matches!(step, SetupStep::Finish) {
                SetupTaskState::Done
            } else {
                SetupTaskState::Running
            };
            if downloaded_bytes.is_some() {
                task.downloaded_bytes = downloaded_bytes;
            }
            if total_bytes.is_some() {
                task.total_bytes = total_bytes;
            }
        }
    }
    if matches!(step, SetupStep::Finish) {
        for task in tasks.iter_mut() {
            task.state = SetupTaskState::Done;
        }
    }
    on_progress(SetupProgress {
        step,
        percent,
        action,
        tasks: tasks.to_vec(),
    });
}

// ─── Download helpers ───────────────────────────────────────────────

fn download_to_file(
    url: &str,
    dest: &std::path::Path,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let resp = ureq::get(url).call().map_err(|e| e.to_string())?;
    let total_bytes = resp
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let mut body = resp.into_body();
    let mut reader = body.as_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded = 0_u64;
    loop {
        let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        downloaded += count as u64;
        on_progress(downloaded, total_bytes);
    }
    Ok(())
}

fn extract_archive(archive: &std::path::Path, dest_dir: &std::path::Path) -> Result<(), String> {
    let name = archive.to_string_lossy();

    let output = if name.ends_with(".tar.xz") {
        silent_command("tar")
            .arg("-xmJf")
            .arg(archive)
            .arg("-C")
            .arg(dest_dir)
            .output()
    } else if name.ends_with(".tar.gz") {
        silent_command("tar")
            .arg("-xmzf")
            .arg(archive)
            .arg("-C")
            .arg(dest_dir)
            .output()
    } else if name.ends_with(".zip") {
        #[cfg(windows)]
        {
            silent_command("tar")
                .arg("-xmf")
                .arg(archive)
                .arg("-C")
                .arg(dest_dir)
                .output()
        }
        #[cfg(not(windows))]
        {
            silent_command("unzip")
                .arg("-o")
                .arg(archive)
                .arg("-d")
                .arg(dest_dir)
                .output()
        }
    } else {
        return Err(format!("Unknown archive format: {name}"));
    };

    let output = output.map_err(|e| format!("Failed to run extraction command: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Extraction failed: {stderr}"));
    }
    Ok(())
}

fn find_file_in(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .find(|e| e.file_type().is_file() && e.file_name().to_string_lossy() == name)
        .map(|e| e.into_path())
}

fn mark_executable(_path: &std::path::Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to set permissions: {e}"))?;
    }
    Ok(())
}

// ─── Other Helpers ───────────────────────────────────────────────────

pub fn silent_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

// ─── Step 1: Download ffmpeg ─────────────────────────────────────────

fn ffmpeg_download_url() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => {
            Ok("https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz")
        }
        ("linux", "aarch64") => {
            Ok("https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-arm64-static.tar.xz")
        }
        ("macos", "aarch64") => Ok("https://evermeet.cx/ffmpeg/ffmpeg-8.1.zip"),
        ("macos", "x86_64") => Ok("https://evermeet.cx/ffmpeg/ffmpeg-8.1.zip"),
        ("windows", "x86_64") => Ok(
            "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
        ),
        (os, arch) => Err(format!("Unsupported platform for ffmpeg: {os}-{arch}")),
    }
}

pub fn step_download_ffmpeg() -> Result<(), String> {
    step_download_ffmpeg_with_progress(|_, _| {})
}

fn step_download_ffmpeg_with_progress(
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let dest = ffmpeg_path();
    if dest.is_file() {
        return Ok(());
    }

    let url = ffmpeg_download_url()?;

    let tmp_dir = vendor_dir().join("_tmp_ffmpeg");
    let _ = std::fs::create_dir_all(&tmp_dir);

    let ext = if url.ends_with(".tar.xz") {
        "tar.xz"
    } else {
        "zip"
    };
    let archive = tmp_dir.join(format!("ffmpeg.{ext}"));

    let result: Result<(), String> = (|| {
        download_to_file(url, &archive, &mut on_progress)?;

        extract_archive(&archive, &tmp_dir)?;

        let binary_name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let found = find_file_in(&tmp_dir, binary_name)
            .ok_or_else(|| format!("Could not find {binary_name} in downloaded archive"))?;

        std::fs::copy(&found, &dest).map_err(|e| format!("Failed to copy ffmpeg: {e}"))?;
        mark_executable(&dest)?;
        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&tmp_dir);
    result?;

    Ok(())
}

// ─── Step 2: Download uv ────────────────────────────────────────────

fn uv_download_url() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok(
            "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-unknown-linux-gnu.tar.gz",
        ),
        ("linux", "aarch64") => Ok(
            "https://github.com/astral-sh/uv/releases/latest/download/uv-aarch64-unknown-linux-gnu.tar.gz",
        ),
        ("macos", "aarch64") => Ok(
            "https://github.com/astral-sh/uv/releases/latest/download/uv-aarch64-apple-darwin.tar.gz",
        ),
        ("macos", "x86_64") => Ok(
            "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-apple-darwin.tar.gz",
        ),
        ("windows", "x86_64") => Ok(
            "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-pc-windows-msvc.zip",
        ),
        (os, arch) => Err(format!("Unsupported platform for uv: {os}-{arch}")),
    }
}

pub fn step_download_uv() -> Result<(), String> {
    step_download_uv_with_progress(|_, _| {})
}

fn step_download_uv_with_progress(
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let dest = uv_path();
    if dest.is_file() {
        return Ok(());
    }

    let url = uv_download_url()?;

    let tmp_dir = vendor_dir().join("_tmp_uv");
    let _ = std::fs::create_dir_all(&tmp_dir);

    let ext = if url.ends_with(".zip") {
        "zip"
    } else {
        "tar.gz"
    };
    let archive = tmp_dir.join(format!("uv.{ext}"));

    let result: Result<(), String> = (|| {
        download_to_file(url, &archive, &mut on_progress)?;
        extract_archive(&archive, &tmp_dir)?;

        let binary_name = if cfg!(windows) { "uv.exe" } else { "uv" };
        let found = find_file_in(&tmp_dir, binary_name)
            .ok_or_else(|| format!("Could not find {binary_name} in downloaded archive"))?;

        std::fs::copy(&found, &dest).map_err(|e| format!("Failed to copy uv: {e}"))?;
        mark_executable(&dest)?;
        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&tmp_dir);
    result?;

    Ok(())
}

// ─── Step 3: Install Python via uv ──────────────────────────────────

pub fn step_install_python() -> Result<(), String> {
    if configured_python_path().is_some() {
        return Ok(());
    }

    let python_dir = vendor_dir().join("python");
    if python_dir.is_dir() && has_python_in(&python_dir) {
        return Ok(());
    }

    let output = silent_command(uv_path())
        .args(["python", "install", "3.10", "--install-dir"])
        .arg(&python_dir)
        .output()
        .map_err(|e| format!("Failed to run uv: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("uv python install failed: {stderr}"));
    }

    Ok(())
}

fn has_python_in(dir: &PathBuf) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let target = if cfg!(windows) {
        "python.exe"
    } else {
        "python3.10"
    };
    for entry in walkdir::WalkDir::new(dir)
        .max_depth(5)
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() && entry.file_name().to_string_lossy() == target {
            return true;
        }
    }
    false
}

// ─── Step 4: Create venv ─────────────────────────────────────────────

fn find_installed_python() -> Option<PathBuf> {
    if let Some(path) = configured_python_path() {
        return Some(path);
    }

    let python_dir = vendor_dir().join("python");
    let target = if cfg!(windows) {
        "python.exe"
    } else {
        "python3.10"
    };
    for entry in walkdir::WalkDir::new(&python_dir)
        .max_depth(5)
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() && entry.file_name().to_string_lossy() == target {
            return Some(entry.into_path());
        }
    }
    None
}

pub fn step_create_venv() -> Result<(), String> {
    let venv_dir = vendor_dir().join("venv");
    if python_path().is_file() {
        return Ok(());
    }

    let installed_python = find_installed_python()
        .ok_or("Could not find installed Python — run python install first")?;

    let output = silent_command(uv_path())
        .args(["venv"])
        .arg(&venv_dir)
        .arg("--python")
        .arg(&installed_python)
        .output()
        .map_err(|e| format!("Failed to run uv venv: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("uv venv failed: {stderr}"));
    }

    Ok(())
}

// ─── Step 5: Install packages ────────────────────────────────────────

struct GpuInfo {
    device: &'static str,
    torch_index: &'static str,
    requires_torch_2_2: bool,
}

// WhisperX 3.7.4 requires the 2.8 release line.  In particular, Pyannote in
// its dependency tree still uses `torchaudio.AudioMetaData`, which was removed
// in later TorchAudio releases.  Keep all three packages on the matching
// release and install them from the selected runtime index: a later
// unconstrained dependency install can otherwise replace an Intel XPU wheel
// with PyPI's CUDA wheel.
const TORCH_PACKAGE: &str = "torch==2.8.0";
const TORCHAUDIO_PACKAGE: &str = "torchaudio==2.8.0";
const TORCHVISION_PACKAGE: &str = "torchvision==0.23.0";

fn install_torch_runtime(
    uv: &Path,
    py: &str,
    gpu: &GpuInfo,
    force_reinstall: bool,
    on_progress: &mut impl FnMut(u64),
    on_output: &mut impl FnMut(String),
) -> Result<(), String> {
    let mut args = vec!["pip", "install"];
    if force_reinstall {
        args.extend([
            "--reinstall-package",
            "torch",
            "--reinstall-package",
            "torchaudio",
            "--reinstall-package",
            "torchvision",
        ]);
    }
    args.extend([
        TORCH_PACKAGE,
        TORCHAUDIO_PACKAGE,
        TORCHVISION_PACKAGE,
        "--python",
        py,
        "--index-url",
        gpu.torch_index,
    ]);

    let output = run_uv_pip_command(uv, &args, on_progress, on_output)
        .map_err(|e| format!("Failed to install {} PyTorch: {e}", gpu.device))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{} PyTorch install failed: {stderr}", gpu.device));
    }
    Ok(())
}

fn verify_torch_runtime(
    py: &Path,
    backend: ComputeBackend,
    on_output: &mut impl FnMut(String),
) -> Result<(), String> {
    // A package version alone is not enough: run a tensor operation on the
    // selected device. This catches a wrong wheel, missing driver, or missing
    // Level Zero runtime before setup is marked ready.
    const PROBE: &str = r#"
import sys
import torch

backend = sys.argv[1]
if backend == "cuda":
    if not torch.cuda.is_available():
        raise RuntimeError("CUDA wheel installed but no CUDA device is available")
    device = "cuda"
    name = torch.cuda.get_device_name(0)
elif backend == "intel":
    if not getattr(torch, "xpu", None) or not torch.xpu.is_available():
        raise RuntimeError("Intel XPU wheel installed but no Intel XPU device is available")
    device = "xpu"
    name = torch.xpu.get_device_name(0)
else:
    device = "cpu"
    name = "CPU"

x = torch.arange(256, dtype=torch.float32, device=device).reshape(16, 16)
y = x @ x
if device == "cuda":
    torch.cuda.synchronize()
elif device == "xpu":
    torch.xpu.synchronize()
if y.device.type != device:
    raise RuntimeError(f"expected {device}, got {y.device.type}")
print(f"Uta Studio runtime verified: backend={backend}, device={name}, torch={torch.__version__}")
"#;

    let output = silent_command(py)
        .args(["-c", PROBE, backend.as_str()])
        .output()
        .map_err(|e| format!("Failed to verify {} runtime: {e}", backend.as_str()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        on_output(stdout);
    }
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} runtime verification failed: {}",
            backend.as_str(),
            stderr.trim()
        ));
    }
    Ok(())
}

fn detect_gpu(backend: ComputeBackend) -> Result<GpuInfo, String> {
    #[cfg(target_os = "macos")]
    {
        if backend == ComputeBackend::Intel {
            return Err(
                "Intel Arc acceleration is available on Linux and Windows, not macOS".into(),
            );
        }
        if backend == ComputeBackend::Cuda {
            return Err("NVIDIA CUDA acceleration is not available on macOS".into());
        }
        if cfg!(target_arch = "x86_64") {
            info!("[vendor] GPU detection: Intel Mac (CPU-only, torch < 2.3)");
            return Ok(GpuInfo {
                device: "cpu",
                torch_index: "https://download.pytorch.org/whl/cpu",
                requires_torch_2_2: true,
            });
        }
        return Ok(GpuInfo {
            device: "mps",
            torch_index: "https://download.pytorch.org/whl/cpu",
            requires_torch_2_2: false,
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        match backend {
            ComputeBackend::Cpu => Ok(GpuInfo {
                device: "cpu",
                torch_index: "https://download.pytorch.org/whl/cpu",
                requires_torch_2_2: false,
            }),
            ComputeBackend::Cuda => {
                let smi = nvidia_smi_path().ok_or(
                    "NVIDIA CUDA was selected, but nvidia-smi is unavailable. Choose CPU or install a working NVIDIA driver.",
                )?;
                let cuda_index = query_cuda_index(smi);
                info!("[vendor] GPU detection: CUDA (index {cuda_index})");
                Ok(GpuInfo {
                    device: "cuda",
                    torch_index: cuda_index,
                    requires_torch_2_2: false,
                })
            }
            ComputeBackend::Intel => {
                info!("[vendor] GPU selection: Intel Arc / PyTorch XPU");
                Ok(GpuInfo {
                    device: "intel",
                    torch_index: "https://download.pytorch.org/whl/xpu",
                    requires_torch_2_2: false,
                })
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn nvidia_smi_path() -> Option<&'static str> {
    let ok = silent_command("nvidia-smi")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());

    if ok {
        info!("[vendor] nvidia-smi found on PATH");
        Some("nvidia-smi")
    } else {
        info!("[vendor] nvidia-smi not found on PATH");
        None
    }
}

#[cfg(not(target_os = "macos"))]
fn query_cuda_index(nvidia_smi: &str) -> &'static str {
    let output = silent_command(nvidia_smi)
        .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
        .output();

    let major = output.ok().filter(|o| o.status.success()).and_then(|o| {
        let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
        info!("[vendor] GPU compute capability: {text}");
        text.split('.').next().and_then(|m| m.parse::<u32>().ok())
    });

    match major {
        Some(v) if v >= 10 => "https://download.pytorch.org/whl/cu128",
        Some(_) => "https://download.pytorch.org/whl/cu126",
        None => {
            info!("[vendor] Could not query compute capability, falling back to cu126");
            "https://download.pytorch.org/whl/cu126"
        }
    }
}

pub fn step_install_packages() -> Result<(), String> {
    step_install_packages_for_backend(ComputeBackend::Cpu, |_| {}, |_| {})
}

fn virtualenv_size() -> u64 {
    walkdir::WalkDir::new(vendor_dir().join("venv"))
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.file_type().is_file().then(|| entry.metadata().ok()))
        .flatten()
        .map(|metadata| metadata.len())
        .sum()
}

/// `uv` deliberately renders its own download bar only to an interactive
/// terminal. The desktop setup runs it without one, so monitor the installed
/// virtualenv while preserving stdout/stderr for useful setup errors.
fn run_uv_pip_command(
    uv: &Path,
    args: &[&str],
    on_progress: &mut impl FnMut(u64),
    on_output: &mut impl FnMut(String),
) -> Result<std::process::Output, String> {
    let mut command = silent_command(uv);
    command
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to start uv pip: {e}"))?;
    let stdout = child.stdout.take().ok_or("Could not capture uv stdout")?;
    let stderr = child.stderr.take().ok_or("Could not capture uv stderr")?;
    let (output_tx, output_rx) = std::sync::mpsc::channel::<String>();
    let read_output = |stream: Box<dyn std::io::Read + Send>,
                       tx: std::sync::mpsc::Sender<String>| {
        std::thread::spawn(move || {
            let mut captured = Vec::new();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                captured.extend_from_slice(line.as_bytes());
                let clean = line.trim_end().to_string();
                if !clean.is_empty() {
                    let _ = tx.send(clean);
                }
                line.clear();
            }
            captured
        })
    };
    let stdout_reader = read_output(Box::new(stdout), output_tx.clone());
    let stderr_reader = read_output(Box::new(stderr), output_tx);

    on_progress(virtualenv_size());
    let status = loop {
        while let Ok(line) = output_rx.try_recv() {
            on_output(line);
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("Failed while waiting for uv pip: {e}"))?
        {
            break status;
        }
        on_progress(virtualenv_size());
        if let Ok(line) = output_rx.recv_timeout(std::time::Duration::from_millis(300)) {
            on_output(line);
        }
    };
    while let Ok(line) = output_rx.try_recv() {
        on_output(line);
    }
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Could not read uv stdout")?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Could not read uv stderr")?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

/// Run a model-preparation command while forwarding human-readable output to
/// the setup dialog. Model hosts often write progress to stderr, so capture
/// both streams and keep draining them until the process exits.
fn run_model_command(
    command: &mut Command,
    mut on_output: impl FnMut(String),
) -> Result<std::process::Output, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start model installer: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Could not capture model installer stdout")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Could not capture model installer stderr")?;
    let (output_tx, output_rx) = std::sync::mpsc::channel::<String>();
    let read_output = |stream: Box<dyn Read + Send>, tx: std::sync::mpsc::Sender<String>| {
        std::thread::spawn(move || {
            let mut captured = Vec::new();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                captured.extend_from_slice(line.as_bytes());
                let clean = line.trim_end_matches(['\r', '\n']).to_string();
                if !clean.trim().is_empty() {
                    let _ = tx.send(clean);
                }
                line.clear();
            }
            captured
        })
    };
    let stdout_reader = read_output(Box::new(stdout), output_tx.clone());
    let stderr_reader = read_output(Box::new(stderr), output_tx);

    let status = loop {
        while let Ok(line) = output_rx.try_recv() {
            on_output(line);
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("Failed while waiting for model installer: {error}"))?
        {
            break status;
        }
        if let Ok(line) = output_rx.recv_timeout(std::time::Duration::from_millis(250)) {
            on_output(line);
        }
    };
    while let Ok(line) = output_rx.try_recv() {
        on_output(line);
    }
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Could not read model installer stdout")?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Could not read model installer stderr")?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn step_install_packages_for_backend(
    backend: ComputeBackend,
    mut on_progress: impl FnMut(u64),
    mut on_output: impl FnMut(String),
) -> Result<(), String> {
    let gpu = detect_gpu(backend)?;

    let uv = uv_path();
    let py = python_path();
    let py_str = py.to_string_lossy().to_string();
    let index = gpu.torch_index;

    let (audio_sep_pkg, whisperx_pkg) = if gpu.requires_torch_2_2 {
        ("audio-separator>=0.24,<0.25", "whisperx>=3.3.0,<3.3.4")
    } else if gpu.device == "cuda" {
        ("audio-separator[gpu]>=0.25", "whisperx==3.7.4")
    } else {
        ("audio-separator>=0.25", "whisperx==3.7.4")
    };

    let cython_args = [
        "pip",
        "install",
        "cython",
        "setuptools",
        "--python",
        &py_str,
    ];
    let cython_out = run_uv_pip_command(&uv, &cython_args, &mut on_progress, &mut on_output)
        .map_err(|e| format!("Failed to install build deps: {e}"))?;
    if !cython_out.status.success() {
        let stderr = String::from_utf8_lossy(&cython_out.stderr);
        return Err(format!("Build deps install failed: {stderr}"));
    }

    // Avoid PyPI's default Linux CUDA wheels for CPU and Intel selections.
    // The selected wheel is installed again at the end, after every other
    // resolver has run, to prevent it from being silently replaced.
    if gpu.device == "cpu" || gpu.device == "intel" {
        install_torch_runtime(&uv, &py_str, &gpu, false, &mut on_progress, &mut on_output)?;
    }

    let (_, onnx_runtime_requirement) = onnx_runtime_package(backend);

    let mut pkg_args: Vec<&str> = vec![
        "pip",
        "install",
        "demucs>=4.0.0",
        whisperx_pkg,
        "soundfile",
        "huggingface_hub>=0.27.0",
        audio_sep_pkg,
        "onnx-asr>=0.5.0",
        onnx_runtime_requirement,
        // Offline vocal pitch estimation. The separately downloaded ONNX
        // weight is kept in the user-selected models cache.
        "rmvpe-onnx==0.2.3",
        "fugashi[unidic-lite]>=1.3",
        "pykakasi>=2.3",
        "jieba>=0.42",
        "pypinyin>=0.50",
        "ToJyutping>=3.0",
        "hangul-romanize>=0.1.0",
        // Tokenizers the Qwen3 forced aligner uses internally for ja/ko.
        "nagisa>=0.2.11",
        "soynlp>=0.0.493",
    ];

    if gpu.requires_torch_2_2 {
        pkg_args.push("torch<2.3");
        pkg_args.push("torchaudio<2.3");
    }

    // OpenVINO GenAI provides the official WhisperPipeline GPU backend used
    // for Intel Arc. Keep it Intel-only so CPU/CUDA installs do not pull in a
    // second inference runtime or its model format.
    if gpu.device == "intel" {
        pkg_args.push("openvino>=2026.1.0");
        pkg_args.push("openvino-genai>=2026.1.0");
    }

    pkg_args.push("--python");
    pkg_args.push(&py_str);

    let output = run_uv_pip_command(&uv, &pkg_args, &mut on_progress, &mut on_output)
        .map_err(|e| format!("Failed to run uv pip install: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Package install failed: {stderr}"));
    }

    if gpu.device == "cuda" {
        let torch_args: Vec<&str> = vec![
            "pip",
            "install",
            "--reinstall-package",
            "torch",
            "--reinstall-package",
            "torchaudio",
            "--reinstall-package",
            "torchvision",
            TORCH_PACKAGE,
            TORCHAUDIO_PACKAGE,
            TORCHVISION_PACKAGE,
            "--python",
            &py_str,
            "--index-url",
            index,
        ];

        let output = run_uv_pip_command(&uv, &torch_args, &mut on_progress, &mut on_output)
            .map_err(|e| format!("Failed to install CUDA PyTorch: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("CUDA PyTorch install failed: {stderr}"));
        }

        let nemo_args: Vec<&str> = vec![
            "pip",
            "install",
            "nemo_toolkit[asr]>=2.0.0",
            "--python",
            &py_str,
        ];

        let output = run_uv_pip_command(&uv, &nemo_args, &mut on_progress, &mut on_output)
            .map_err(|e| format!("Failed to install NeMo: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("NeMo install failed: {stderr}"));
        }
    }

    // Qwen3-ForcedAligner (experimental align backend) needs the Qwen3-ASR
    // integration, which landed in transformers main (PR #43838) but is not in
    // any tagged release yet. Install it from the merge commit over whatever
    // whisperx pulled in; this stays a no-upper-bound override so whisperx's
    // own transformers usage keeps working. Pinned for reproducibility.
    //
    // This MUST run last: on CUDA, `nemo_toolkit[asr]` (installed above) pins
    // transformers back to a tagged release that doesn't recognize the
    // `qwen3_asr` model type, so it has to be re-applied after NeMo to win.
    let transformers_git = concat!(
        "transformers @ git+https://github.com/huggingface/transformers",
        "@967203924487e8e9f64a2d825fc4e1bdbec3f518",
    );
    let transformers_args: Vec<&str> = vec![
        "pip",
        "install",
        "--reinstall-package",
        "transformers",
        transformers_git,
        "--python",
        &py_str,
    ];

    let output = run_uv_pip_command(&uv, &transformers_args, &mut on_progress, &mut on_output)
        .map_err(|e| format!("Failed to install Qwen-capable transformers: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("transformers (Qwen3-ASR) install failed: {stderr}"));
    }

    // ASR dependencies can replace the selected ONNX wheel during resolution.
    // Reassert the backend-specific runtime last so RMVPE can see OpenVINO or
    // CUDA when available; pitch.py still falls back to CPU on unsupported hosts.
    let runtime_args = inference_runtime_reinstall_args(backend, &py_str);
    let output = run_uv_pip_command(&uv, &runtime_args, &mut on_progress, &mut on_output)
        .map_err(|e| format!("Failed to reinstall inference runtime: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Inference runtime install failed: {stderr}"));
    }

    // This must be the final package operation. whisperx, audio-separator and
    // transformers have broad torch requirements; their resolver may select a
    // different wheel family unless we reassert the requested runtime here.
    install_torch_runtime(&uv, &py_str, &gpu, true, &mut on_progress, &mut on_output)?;
    verify_torch_runtime(&py, backend, &mut on_output)?;

    // Essentia sharpens BPM/key detection and adds a few extra descriptors
    // (see `analyze_with_essentia` in key_detect.py), but PyPI ships no
    // Windows wheel for it — a failed or skipped install here must not fail
    // setup on any platform. `key_detect.py` falls back to its own
    // dependency-free estimators whenever the import fails.
    let essentia_args = ["pip", "install", "essentia", "--python", &py_str];
    match run_uv_pip_command(&uv, &essentia_args, &mut on_progress, &mut on_output) {
        Ok(output) if !output.status.success() => {
            on_output(format!(
                "Essentia is unavailable on this platform; using built-in BPM/key detection instead. ({})",
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .last()
                    .unwrap_or_default()
            ));
        }
        Err(e) => on_output(format!(
            "Essentia install skipped; using built-in BPM/key detection instead. ({e})"
        )),
        Ok(_) => {}
    }

    Ok(())
}

// ─── Step 6: Extract analyzer scripts ────────────────────────────────

pub fn step_extract_scripts() -> Result<(), String> {
    vendor_scripts::write_scripts(&analyzer_dir())
        .map_err(|e| format!("Failed to write scripts: {e}"))?;
    Ok(())
}

/// Download the RMVPE ONNX weight during setup rather than making the first
/// song analysis stall on a hidden model download. `pitch.py` owns the exact
/// model location and writes a local manifest once the file can be loaded.
pub fn step_download_pitch_model() -> Result<(), String> {
    step_download_pitch_model_with_output(|_| {})
}

fn step_download_pitch_model_with_output(mut on_output: impl FnMut(String)) -> Result<(), String> {
    if pitch_model_path().is_file() {
        on_output("Using existing RMVPE pitch model".to_string());
        return Ok(());
    }
    let py = python_path();
    let script = analyzer_dir().join("pitch.py");
    let model_dir = models_dir().join("pitch").join("rmvpe");
    std::fs::create_dir_all(&model_dir)
        .map_err(|e| format!("Failed to create pitch model directory: {e}"))?;

    let mut command = silent_command(&py);
    command
        .arg(&script)
        .arg("--download-model")
        .arg("--models-dir")
        .arg(&model_dir);
    let output = run_model_command(&mut command, &mut on_output)
        .map_err(|e| format!("Failed to start pitch-model download: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Pitch-model download failed: {stderr}"));
    }
    Ok(())
}

/// Pre-download the official OpenVINO Whisper model selected for Intel Arc.
/// The analyzer uses it through `openvino_genai.WhisperPipeline(..., "GPU")`
/// and falls back to the normal CPU Whisper path if GPU loading or inference
/// is unavailable for an individual song.
fn step_download_openvino_whisper_model_with_output(
    mut on_output: impl FnMut(String),
) -> Result<(), String> {
    if openvino_whisper_model_ready() {
        on_output("Using existing OpenVINO Whisper model".to_string());
        return Ok(());
    }
    let py = python_path();
    let script = analyzer_dir().join("openvino_whisper.py");
    let model_dir = openvino_whisper_model_dir();
    std::fs::create_dir_all(&model_dir)
        .map_err(|e| format!("Failed to create OpenVINO Whisper model directory: {e}"))?;

    let mut command = silent_command(&py);
    command
        .arg(&script)
        .arg("--download-model")
        .arg("--model-dir")
        .arg(&model_dir);
    let output = run_model_command(&mut command, &mut on_output)
        .map_err(|e| format!("Failed to start OpenVINO Whisper download: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("OpenVINO Whisper download failed: {stderr}"));
    }
    Ok(())
}

/// Download the Intel GPU stem-separation model during setup. It is cached
/// outside the Python environment, so later setup runs only verify the files
/// after the first download.
fn step_download_openvino_separator_models_with_output(
    mut on_output: impl FnMut(String),
) -> Result<(), String> {
    if openvino_separator_models_ready() {
        on_output("Using existing OpenVINO Demucs model".to_string());
        return Ok(());
    }
    let py = python_path();
    let script = analyzer_dir().join("openvino_separation.py");
    let model_dir = openvino_separator_models_dir();
    std::fs::create_dir_all(&model_dir)
        .map_err(|e| format!("Failed to create OpenVINO separator model directory: {e}"))?;

    let mut command = silent_command(&py);
    command
        .arg(&script)
        .arg("--download-models")
        .arg("--models-dir")
        .arg(&model_dir);
    let output = run_model_command(&mut command, &mut on_output)
        .map_err(|e| format!("Failed to start OpenVINO separator model download: {e}"))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "OpenVINO separator model download failed: {stdout}\n{stderr}"
        ));
    }
    Ok(())
}

fn model_target_label(target: ModelDownloadTarget) -> &'static str {
    match target {
        ModelDownloadTarget::Whisper => "the selected Whisper model",
        ModelDownloadTarget::WhisperLanguageDetection => "Whisper Tiny language detection",
        ModelDownloadTarget::Parakeet => "Parakeet v3",
        ModelDownloadTarget::Separator => "the selected separator model",
        ModelDownloadTarget::Alignment => "the selected alignment model",
        ModelDownloadTarget::MmsKaraokeAlignment => "MMS Karaoke Japanese alignment",
        ModelDownloadTarget::Pitch => "RMVPE pitch detection",
        ModelDownloadTarget::OpenVinoWhisper => "OpenVINO Whisper",
    }
}

/// Prepare exactly one model family named by the confirmed UI action. The
/// common runtime is established by `run_vendor_setup` before this function is
/// reached; this function never downloads unrelated model families.
pub fn step_download_model(
    target: ModelDownloadTarget,
    mut on_output: impl FnMut(String),
) -> Result<(), String> {
    match target {
        ModelDownloadTarget::Pitch => return step_download_pitch_model_with_output(on_output),
        ModelDownloadTarget::OpenVinoWhisper => {
            return step_download_openvino_whisper_model_with_output(on_output);
        }
        ModelDownloadTarget::Separator
            if crate::config::AppConfig::load().separator() == "openvino_demucs" =>
        {
            return step_download_openvino_separator_models_with_output(on_output);
        }
        _ => {}
    }

    let config = crate::config::AppConfig::load();
    if target == ModelDownloadTarget::Parakeet && config.asr_engine() != "parakeet" {
        return Err("Select Parakeet in Settings before downloading its model".to_string());
    }
    if target == ModelDownloadTarget::Alignment
        && !matches!(config.align_backend(), "qwen" | "mms_karaoke")
    {
        return Err(
            "The selected alignment backend does not require a standalone model".to_string(),
        );
    }

    let py = python_path();
    let script = analyzer_dir().join("model_setup.py");
    let models = models_dir();
    let mut command = silent_command(&py);
    let align_backend = if target == ModelDownloadTarget::MmsKaraokeAlignment {
        "mms_karaoke"
    } else {
        config.align_backend()
    };
    command
        .env("HF_HOME", models.join("huggingface"))
        .env("TORCH_HOME", models.join("torch"))
        .env("NEMO_CACHE_DIR", models.join("nemo"))
        .arg(&script)
        .arg("--models-dir")
        .arg(&models)
        .arg("--backend")
        .arg(configured_backend_name())
        .arg("--engine")
        .arg(config.asr_engine())
        .arg("--whisper-model")
        .arg(config.whisper_model())
        .arg("--separator")
        .arg(config.separator())
        .arg("--align-backend")
        .arg(align_backend)
        .arg("--target")
        .arg(match target {
            ModelDownloadTarget::Whisper => "whisper",
            ModelDownloadTarget::WhisperLanguageDetection => "language_detection",
            ModelDownloadTarget::Parakeet => "parakeet",
            ModelDownloadTarget::Separator => "separator",
            ModelDownloadTarget::Alignment => "alignment",
            ModelDownloadTarget::MmsKaraokeAlignment => "alignment",
            ModelDownloadTarget::Pitch | ModelDownloadTarget::OpenVinoWhisper => unreachable!(),
        });
    let output = run_model_command(&mut command, &mut on_output).map_err(|error| {
        format!(
            "Failed to start {} download: {error}",
            model_target_label(target)
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} download failed: {}",
            model_target_label(target),
            stderr.trim()
        ));
    }
    Ok(())
}

pub fn step_download_selected_models(mut on_output: impl FnMut(String)) -> Result<(), String> {
    let config = crate::config::AppConfig::load();
    let py = python_path();
    let script = analyzer_dir().join("model_setup.py");
    let models = models_dir();
    let mut command = silent_command(&py);
    command
        .env("HF_HOME", models.join("huggingface"))
        .env("TORCH_HOME", models.join("torch"))
        .env("NEMO_CACHE_DIR", models.join("nemo"))
        .arg(&script)
        .arg("--models-dir")
        .arg(&models)
        .arg("--backend")
        .arg(configured_backend_name())
        .arg("--engine")
        .arg(config.asr_engine())
        .arg("--whisper-model")
        .arg(config.whisper_model())
        .arg("--separator")
        .arg(config.separator())
        .arg("--align-backend")
        .arg(config.align_backend())
        .arg("--target")
        .arg("all");
    let output = run_model_command(&mut command, &mut on_output)
        .map_err(|error| format!("Failed to start selected-model setup: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Selected-model setup failed: {stderr}"));
    }
    Ok(())
}

/// Refresh the embedded analyzer scripts on top of an already-set-up vendor dir.
/// No-op when setup hasn't completed yet — initial extraction is handled by
/// `step_extract_scripts` during the setup flow.
pub fn refresh_analyzer_scripts_if_ready() -> Result<(), String> {
    let managed_environment_ready = std::fs::read_to_string(ready_marker())
        .is_ok_and(|value| ready_marker_is_compatible(&value))
        && python_path().is_file();
    if !managed_environment_ready {
        return Ok(());
    }

    vendor_scripts::write_scripts(&analyzer_dir())
        .map_err(|e| format!("Failed to refresh analyzer scripts: {e}"))
}

pub fn mark_ready() -> Result<(), String> {
    std::fs::write(ready_marker(), expected_ready_marker())
        .map_err(|e| format!("Failed to mark ready: {e}"))
}

#[cfg(test)]
mod node_model_availability_tests {
    use super::node_model_availability_from_checks;
    use crate::analysis_graph::AnalysisNodeId;

    #[test]
    fn separator_and_pitch_map_directly_to_their_own_node() {
        let map = node_model_availability_from_checks(
            false, true, "whisper", "cpu", true, true, "whisperx", true,
        );
        assert_eq!(map[&AnalysisNodeId::new("stems.separate")], false);
        assert_eq!(map[&AnalysisNodeId::new("pitch.extract")], true);
    }

    #[test]
    fn plain_cpu_whisper_does_not_need_the_language_detector() {
        // Whisper detects language with the same model it transcribes
        // with -- unlike parakeet/intel, which need a separate tiny model
        // first. A missing (false) detector must not block this path.
        let map = node_model_availability_from_checks(
            true, true, "whisper", "cpu", true, false, "whisperx", true,
        );
        assert_eq!(map[&AnalysisNodeId::new("lyrics.transcribe")], true);
    }

    #[test]
    fn parakeet_requires_both_its_own_model_and_the_language_detector() {
        let map = node_model_availability_from_checks(
            true, true, "parakeet", "cuda", true, false, "whisperx", true,
        );
        assert_eq!(map[&AnalysisNodeId::new("lyrics.transcribe")], false);

        let map = node_model_availability_from_checks(
            true, true, "parakeet", "cuda", true, true, "whisperx", true,
        );
        assert_eq!(map[&AnalysisNodeId::new("lyrics.transcribe")], true);
    }

    #[test]
    fn intel_backend_requires_the_language_detector_regardless_of_asr_engine() {
        let map = node_model_availability_from_checks(
            true, true, "whisper", "intel", true, false, "whisperx", true,
        );
        assert_eq!(map[&AnalysisNodeId::new("lyrics.transcribe")], false);
    }

    #[test]
    fn missing_primary_asr_model_blocks_transcription_even_with_a_ready_detector() {
        let map = node_model_availability_from_checks(
            true, true, "parakeet", "cuda", false, true, "whisperx", true,
        );
        assert_eq!(map[&AnalysisNodeId::new("lyrics.transcribe")], false);
    }

    #[test]
    fn whisperx_and_ctc_alignment_are_never_blocked_by_a_missing_fixed_model() {
        // Neither backend has one fixed, trackable model -- they resolve a
        // per-language wav2vec2 model on demand, so `align_model_ready` must
        // simply be ignored for them, not used as a gate.
        for backend in ["whisperx", "ctc"] {
            let map = node_model_availability_from_checks(
                true, true, "whisper", "cpu", true, true, backend, false,
            );
            assert_eq!(map[&AnalysisNodeId::new("lyrics.align")], true);
        }
    }

    #[test]
    fn qwen_and_mms_karaoke_alignment_are_blocked_when_their_model_is_missing() {
        for backend in ["qwen", "mms_karaoke"] {
            let map = node_model_availability_from_checks(
                true, true, "whisper", "cpu", true, true, backend, false,
            );
            assert_eq!(map[&AnalysisNodeId::new("lyrics.align")], false);

            let map = node_model_availability_from_checks(
                true, true, "whisper", "cpu", true, true, backend, true,
            );
            assert_eq!(map[&AnalysisNodeId::new("lyrics.align")], true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ComputeBackend, inference_runtime_reinstall_args, onnx_runtime_package};

    #[test]
    fn inference_runtime_reinstall_uses_a_bare_distribution_name() {
        for backend in [
            ComputeBackend::Cpu,
            ComputeBackend::Cuda,
            ComputeBackend::Intel,
        ] {
            let (name, requirement) = onnx_runtime_package(backend);
            let args = inference_runtime_reinstall_args(backend, "/test/python");
            let reinstall_index = args
                .iter()
                .position(|arg| *arg == "--reinstall-package")
                .expect("runtime install must request an explicit reinstall");

            assert_eq!(args[reinstall_index + 1], name);
            assert!(
                !name
                    .chars()
                    .any(|ch| matches!(ch, '<' | '>' | '=' | '!' | '~'))
            );
            assert_eq!(args[reinstall_index + 2], requirement);
            assert!(requirement.starts_with(name));
        }
    }

    #[test]
    fn inference_runtime_packages_match_each_compute_backend() {
        assert_eq!(
            onnx_runtime_package(ComputeBackend::Cpu),
            ("onnxruntime", "onnxruntime>=1.17")
        );
        assert_eq!(
            onnx_runtime_package(ComputeBackend::Cuda),
            ("onnxruntime-gpu", "onnxruntime-gpu>=1.17")
        );
        assert_eq!(
            onnx_runtime_package(ComputeBackend::Intel),
            ("onnxruntime-openvino", "onnxruntime-openvino>=1.17")
        );
    }
}
