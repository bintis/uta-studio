use std::path::Path;

use super::*;
use crate::cache::{relocate_app_data_path, same_path, songs_cache_dir, uta_studio_dir};

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
    let resource = super::status::resource_for_target(target)
        .ok_or_else(|| "unknown model target".to_string())?;
    let client = super::status::runtime_client()?;
    let result = client
        .install(std::slice::from_ref(&resource), &[])
        .map_err(|error| error.to_string())?;
    if result.changed.is_empty() {
        on_output(format!(
            "{} is already installed and verified.",
            status.label
        ));
    } else {
        on_output(format!("{} was installed and verified.", status.label));
    }
    Ok(())
}
