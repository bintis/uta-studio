use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use super::*;
use crate::cache::{
    models_dir, relocate_app_data_path, same_path, songs_cache_dir, uta_studio_dir,
};

const QWEN_ASR_URL: &str = "https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf/resolve/main/Qwen3-ASR-1.7B-Q4_K_M.gguf";
const QWEN_ASR_SHA256: &str = "b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e";

fn tasks() -> Vec<SetupTask> {
    [
        (SetupStep::PrepareFolders, "Prepare data folders"),
        (SetupStep::Ffmpeg, "Verify ffmpeg"),
        (SetupStep::NativeComponents, "Verify native components"),
        (SetupStep::RuntimeLock, "Verify runtime lock"),
        (SetupStep::SelectedModels, "Prepare selected model"),
        (SetupStep::Finish, "Finish"),
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

fn emit(
    callback: &mut impl FnMut(SetupProgress),
    tasks: &mut [SetupTask],
    step: SetupStep,
    percent: usize,
    action: impl Into<String>,
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
) {
    for task in tasks.iter_mut() {
        if task.step == step {
            task.state = if percent == 100 || matches!(step, SetupStep::Finish) {
                SetupTaskState::Done
            } else {
                SetupTaskState::Running
            };
            task.downloaded_bytes = downloaded_bytes;
            task.total_bytes = total_bytes;
        } else if task.state == SetupTaskState::Running {
            task.state = SetupTaskState::Done;
        }
    }
    callback(SetupProgress {
        step,
        percent,
        action: action.into(),
        tasks: tasks.to_vec(),
    });
}

pub fn run_vendor_setup(
    folders: SetupFolders,
    mut on_progress: impl FnMut(SetupProgress) + Send,
    mut on_log: impl FnMut(String) + Send,
    mut on_data_relocated: impl FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let mut task_list = tasks();
    emit(
        &mut on_progress,
        &mut task_list,
        SetupStep::PrepareFolders,
        10,
        "Preparing app-owned folders...",
        None,
        None,
    );
    if let Some(raw) = folders
        .data_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let target = resolve_data_path_input(raw)?;
        if !same_path(&uta_studio_dir(), &target) {
            let new_path = relocate_app_data_path(target)?;
            on_data_relocated(&new_path)?;
        }
    }
    let separate_targets = folders.cache_paths.map(normalize_cache_paths).transpose()?;
    let targets = separate_targets
        .clone()
        .unwrap_or_else(default_cache_paths_for_data_root);
    let old_song_cache = songs_cache_dir();
    relocate_cache_data_to_targets(&targets)?;
    if let Some(new_song_cache) = targets.songs.as_ref() {
        crate::library_db::rebase_song_album_art_cache_paths(&old_song_cache, new_song_cache)?;
    }
    let mut config = crate::config::AppConfig::load();
    config.cache_paths = separate_targets;
    config.compute_backend = Some(folders.compute_backend.as_str().to_string());
    config.save()?;

    emit(
        &mut on_progress,
        &mut task_list,
        SetupStep::Ffmpeg,
        25,
        "Verifying packaged or system ffmpeg...",
        None,
        None,
    );
    if !ffmpeg_path().is_file() {
        return Err(
            "ffmpeg is unavailable; install it or use the packaged application".to_string(),
        );
    }

    emit(
        &mut on_progress,
        &mut task_list,
        SetupStep::NativeComponents,
        40,
        "Verifying local native workers...",
        None,
        None,
    );
    if crate::native_runtime::native_analyzer_path().is_none() {
        return Err("the packaged native analyzer component is unavailable".to_string());
    }

    emit(
        &mut on_progress,
        &mut task_list,
        SetupStep::RuntimeLock,
        55,
        "Verifying pinned runtime identities...",
        None,
        None,
    );
    crate::native_runtime::native_runtime_lock()?;

    emit(
        &mut on_progress,
        &mut task_list,
        SetupStep::SelectedModels,
        65,
        "Preparing the explicitly selected model...",
        None,
        None,
    );
    if let Some(target) = folders.model_target {
        step_download_model(target, &mut on_log)?;
    } else {
        on_log("Native components verified. Install missing model families from their individual rows.".to_string());
    }

    emit(
        &mut on_progress,
        &mut task_list,
        SetupStep::Finish,
        100,
        "Done",
        None,
        None,
    );
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn download_qwen_asr(mut on_output: impl FnMut(String)) -> Result<(), String> {
    let directory = models_dir().join("qwen-asr");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let destination = directory.join("Qwen3-ASR-1.7B-Q4_K_M.gguf");
    if destination.is_file() {
        return if sha256_file(&destination)? == QWEN_ASR_SHA256 {
            Ok(())
        } else {
            Err("the existing Qwen ASR model has the wrong hash; use the explicit remove/reinstall action".to_string())
        };
    }
    let temporary = directory.join(format!(".Qwen3-ASR-1.7B-Q4_K_M.{}.tmp", std::process::id()));
    let result = (|| {
        let response = ureq::get(QWEN_ASR_URL)
            .call()
            .map_err(|error| error.to_string())?;
        let total = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        let mut body = response.into_body();
        let mut reader = body.as_reader();
        let mut output = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        let mut buffer = [0_u8; 128 * 1024];
        let mut downloaded = 0_u64;
        loop {
            let count = reader
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            output
                .write_all(&buffer[..count])
                .map_err(|error| error.to_string())?;
            downloaded += count as u64;
            on_output(match total {
                Some(total) => format!("Downloaded {downloaded} / {total} bytes"),
                None => format!("Downloaded {downloaded} bytes"),
            });
        }
        output.sync_all().map_err(|error| error.to_string())?;
        let actual = sha256_file(&temporary)?;
        if actual != QWEN_ASR_SHA256 {
            return Err(format!(
                "Qwen ASR model hash mismatch: expected {QWEN_ASR_SHA256}, got {actual}"
            ));
        }
        std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
        Ok(())
    })();
    if temporary.exists() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub fn step_download_model(
    target: ModelDownloadTarget,
    mut on_output: impl FnMut(String),
) -> Result<(), String> {
    let status = model_install_statuses()
        .into_iter()
        .find(|status| status.target == target)
        .ok_or_else(|| "unknown model target".to_string())?;
    if status.available {
        on_output(format!(
            "{} is already available and verified.",
            status.label
        ));
        return Ok(());
    }
    match target {
        ModelDownloadTarget::QwenAsr => download_qwen_asr(on_output),
        ModelDownloadTarget::SharedRuntime => Err(
            "native runtime components are supplied by the application package; reinstall the package"
                .to_string(),
        ),
        _ => Err(format!(
            "{} has no audited distributable artifact in this build; it remains unavailable rather than using an unverified fallback",
            status.label
        )),
    }
}
