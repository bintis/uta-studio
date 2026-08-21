use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use super::*;
use crate::{
    cache::{
        models_dir, relocate_app_data_path, same_path, songs_cache_dir, uta_studio_dir, vendor_dir,
    },
    vendor_scripts,
};
use tracing::info;

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

pub(crate) fn setup_tasks() -> Vec<SetupTask> {
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

pub(crate) fn emit_setup_progress(
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

pub(crate) fn download_to_file(
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

pub(crate) fn extract_archive(
    archive: &std::path::Path,
    dest_dir: &std::path::Path,
) -> Result<(), String> {
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

pub(crate) fn find_file_in(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .find(|e| e.file_type().is_file() && e.file_name().to_string_lossy() == name)
        .map(|e| e.into_path())
}

pub(crate) fn mark_executable(_path: &std::path::Path) -> Result<(), String> {
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
    let cmd = Command::new(program);
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut cmd = cmd;
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    };
    cmd
}

// ─── Step 1: Download ffmpeg ─────────────────────────────────────────

pub(crate) fn ffmpeg_download_url() -> Result<&'static str, String> {
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

pub(crate) fn step_download_ffmpeg_with_progress(
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

pub(crate) fn uv_download_url() -> Result<&'static str, String> {
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

pub(crate) fn step_download_uv_with_progress(
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

pub(crate) fn has_python_in(dir: &PathBuf) -> bool {
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

pub(crate) fn find_installed_python() -> Option<PathBuf> {
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

pub(crate) const TORCH_RUNTIME_PROBE: &str = r#"
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
    # get_device_name() enters the same device-properties query that is known
    # to segfault on Battlemage with affected PyTorch/compute-runtime stacks.
    # The tensor operation below is the real runtime verification; a marketing
    # name adds no evidence and is unsafe to query here.
    name = "Intel XPU"
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

pub(crate) fn verify_torch_runtime(
    py: &Path,
    backend: ComputeBackend,
    on_output: &mut impl FnMut(String),
) -> Result<(), String> {
    // A package version alone is not enough: run a tensor operation on the
    // selected device. This catches a wrong wheel, missing driver, or missing
    // Level Zero runtime before setup is marked ready.
    let output = silent_command(py)
        .args(["-c", TORCH_RUNTIME_PROBE, backend.as_str()])
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
pub(crate) fn nvidia_smi_path() -> Option<&'static str> {
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
pub(crate) fn query_cuda_index(nvidia_smi: &str) -> &'static str {
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

pub(crate) fn virtualenv_size() -> u64 {
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
pub(crate) fn run_uv_pip_command(
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
pub(crate) fn run_model_command(
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

pub(crate) fn step_install_packages_for_backend(
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
        ("audio-separator==0.24.1", "whisperx>=3.3.0,<3.3.4")
    } else if gpu.device == "cuda" {
        ("audio-separator[gpu]==0.44.5", "whisperx==3.7.4")
    } else {
        ("audio-separator==0.44.5", "whisperx==3.7.4")
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

pub(crate) fn step_download_pitch_model_with_output(
    mut on_output: impl FnMut(String),
) -> Result<(), String> {
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
pub(crate) fn step_download_openvino_whisper_model_with_output(
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
pub(crate) fn step_download_openvino_separator_models_with_output(
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

pub(crate) fn model_target_label(target: ModelDownloadTarget) -> &'static str {
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
