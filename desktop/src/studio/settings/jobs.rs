use std::time::{Duration, Instant};

use super::*;
use crate::studio::*;

pub(crate) fn start_cache_stats_job(cache_stats: &mut CacheStatsJob) {
    if cache_stats.receiver.is_some() {
        return;
    }
    cache_stats.error = None;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(app_core::CacheStats::calculate());
    });
    cache_stats.receiver = Some(Mutex::new(receiver));
}

pub(crate) fn handle_cache_stats_request(
    mut cache_stats: ResMut<CacheStatsJob>,
    mut jobs: ResMut<AsyncJobs>,
) {
    if !jobs.request_cache_stats_refresh {
        return;
    }
    jobs.request_cache_stats_refresh = false;
    if cache_stats.current.is_none() && cache_stats.receiver.is_none() {
        start_cache_stats_job(&mut cache_stats);
    }
}

pub(crate) fn poll_cache_stats(
    mut cache_stats: ResMut<CacheStatsJob>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = cache_stats
        .receiver
        .as_ref()
        .and_then(|receiver| match receiver.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(stats) => Some(Ok(stats)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Cache stats worker exited unexpectedly.".to_string()))
                }
            },
            Err(_) => Some(Err("Cache stats status channel was poisoned.".to_string())),
        });
    let Some(result) = result else {
        return;
    };
    cache_stats.receiver = None;
    match result {
        Ok(stats) => {
            cache_stats.current = Some(stats);
            cache_stats.error = None;
        }
        Err(error) => cache_stats.error = Some(error),
    }
    invalidated.invalidate(UiDirtyRegion::Settings);
}

pub(crate) fn start_model_settings_job(job: &mut ModelSettingsJob) {
    if job.receiver.is_some() {
        return;
    }
    job.error = None;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let runtime_status = app_core::analysis_runtime_status();
        let (audio_catalog, audio_catalog_error) = match app_core::list_audio_models() {
            Ok(catalog) => (catalog, None),
            Err(error) => (
                app_core::AudioModelCatalogSummary {
                    schema_version: 1,
                    catalog_version: "unavailable".to_string(),
                    models: Vec::new(),
                },
                Some(error),
            ),
        };
        let _ = sender.send(Ok(ModelSettingsSnapshot {
            runtime_status,
            audio_catalog,
            audio_catalog_error,
        }));
    });
    job.receiver = Some(Mutex::new(receiver));
}

pub(crate) fn handle_model_settings_request(
    mut jobs: ResMut<AsyncJobs>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !jobs.request_model_settings_refresh {
        return;
    }
    jobs.request_model_settings_refresh = false;
    start_model_settings_job(&mut jobs.model_settings_job);
    invalidated.invalidate(UiDirtyRegion::Settings);
}

pub(crate) fn poll_model_settings_job(
    mut jobs: ResMut<AsyncJobs>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = jobs
        .model_settings_job
        .receiver
        .as_ref()
        .and_then(|receiver| match receiver.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(snapshot) => Some(snapshot),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Model status worker exited unexpectedly.".to_string()))
                }
            },
            Err(_) => Some(Err("Model status channel was poisoned.".to_string())),
        });
    let Some(result) = result else {
        return;
    };
    jobs.model_settings_job.receiver = None;
    match result {
        Ok(snapshot) => {
            jobs.model_settings_job.current = Some(snapshot);
            jobs.model_settings_job.error = None;
        }
        Err(error) => jobs.model_settings_job.error = Some(error),
    }
    invalidated.invalidate(UiDirtyRegion::Settings);
}

pub(crate) fn start_native_setup(
    config: &AppConfig,
    request: SetupRequest,
    setup: &mut NativeSetup,
) {
    let (sender, receiver) = mpsc::channel();
    let folders = setup_folders(config, request);
    std::thread::spawn(move || {
        let progress_sender = sender.clone();
        let log_sender = sender.clone();
        let relocation_sender = sender.clone();
        let result = app_core::run_vendor_setup(
            folders,
            move |progress| {
                let _ = progress_sender.send(SetupEvent::Progress(progress));
            },
            move |line| {
                let _ = log_sender.send(SetupEvent::Log(line));
            },
            move |path| {
                relocation_sender
                    .send(SetupEvent::Log(format!(
                        "Application data relocated to {}",
                        path.display()
                    )))
                    .map_err(|error| error.to_string())
            },
        );
        let _ = sender.send(SetupEvent::Complete(result));
    });
    setup.receiver = Some(Mutex::new(receiver));
    setup.progress = None;
    setup.logs.clear();
    setup.last_ui_refresh = None;
}

pub(crate) fn setup_folders(config: &AppConfig, request: SetupRequest) -> app_core::SetupFolders {
    app_core::SetupFolders {
        data_path: None,
        cache_paths: config.cache_paths.clone(),
        compute_backend: match config.compute_backend.as_deref() {
            Some("openvino" | "intel") => app_core::ComputeBackend::OpenVino,
            Some("vulkan" | "cuda") => app_core::ComputeBackend::Vulkan,
            Some("diagnostic_cpu") => app_core::ComputeBackend::DiagnosticCpu,
            _ => app_core::ComputeBackend::Auto,
        },
        model_target: request.target,
    }
}

pub(crate) fn poll_native_setup(
    mut setup: ResMut<NativeSetup>,
    mut shell: ResMut<ShellState>,
    mut jobs: ResMut<AsyncJobs>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let mut events = Vec::new();
    let mut channel_poisoned = false;
    {
        let Some(receiver) = setup.receiver.as_ref() else {
            return;
        };
        match receiver.lock() {
            Ok(receiver) => loop {
                match receiver.try_recv() {
                    Ok(event) => events.push(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        events.push(SetupEvent::Complete(Err(
                            "Analysis setup worker exited unexpectedly.".to_string(),
                        )));
                        break;
                    }
                }
            },
            Err(_) => channel_poisoned = true,
        }
    }
    if channel_poisoned {
        setup.receiver = None;
        setup.progress = None;
        setup.last_ui_refresh = None;
        shell.notice = Some("Analysis setup status channel was poisoned.".to_string());
        invalidated.invalidate(UiDirtyRegion::Settings);
        return;
    }
    if events.is_empty() {
        return;
    }
    let mut finished = false;
    for event in events {
        match event {
            SetupEvent::Progress(progress) => {
                shell.notice = Some(format!("{} · {}%", progress.action, progress.percent));
                setup.progress = Some(progress);
            }
            SetupEvent::Log(line) => {
                setup.logs.push(line);
                if setup.logs.len() > 200 {
                    let excess = setup.logs.len() - 200;
                    setup.logs.drain(..excess);
                }
            }
            SetupEvent::Complete(result) => {
                setup.receiver = None;
                setup.progress = None;
                setup.last_ui_refresh = None;
                shell.config = AppConfig::load();
                app_core::invalidate_analysis_runtime_status_cache();
                jobs.request_model_settings_refresh = true;
                shell.notice = Some(match result {
                    Ok(()) => "Analysis runtime setup completed.".to_string(),
                    Err(error) => format!("Analysis runtime setup failed: {error}"),
                });
                finished = true;
            }
        }
    }
    // Rebuilding the Models page on every worker progress line steals the
    // scroll container and feels like the page has frozen. Keep the latest
    // progress in memory and paint at most a few times a second until
    // the job finishes.
    let now = Instant::now();
    let due = setup
        .last_ui_refresh
        .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(280));
    if finished || due {
        setup.last_ui_refresh = Some(now);
        invalidated.invalidate(UiDirtyRegion::Settings);
    }
}

pub(crate) fn poll_native_diagnostics(
    mut diagnostics: ResMut<NativeDiagnostics>,
    mut shell: ResMut<ShellState>,
    mut dialogs: ResMut<DialogState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = diagnostics
        .receiver
        .as_ref()
        .and_then(|receiver| match receiver.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(report) => Some(Ok(report)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Feature diagnostics worker exited unexpectedly.".to_string(),
                )),
            },
            Err(_) => Some(Err(
                "Feature diagnostics status channel was poisoned.".to_string()
            )),
        });
    let Some(result) = result else {
        return;
    };
    diagnostics.receiver = None;
    match result {
        Ok(report) => {
            shell.notice = Some(format!(
                "Diagnostics {}: {} passed, {} failed, {} skipped.",
                if report.ok { "passed" } else { "completed" },
                report.passed,
                report.failed,
                report.skipped,
            ));
            dialogs.diagnostic_report = Some(report);
        }
        Err(error) => shell.notice = Some(error),
    }
    invalidated.invalidate(UiDirtyRegion::Settings);
}

fn settings_scroll_max(
    viewport_height: f32,
    reported_content_height: f32,
    page_height: f32,
) -> f32 {
    let measured_page_height = page_height + SETTINGS_CONTENT_VERTICAL_PADDING * 2.0;
    (reported_content_height.max(measured_page_height) - viewport_height).max(0.0)
}

pub(crate) fn handle_settings_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut shell: ResMut<ShellState>,
    mut contents: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        With<SettingsContent>,
    >,
    pages: Query<&ComputedNode, With<SettingsPageContent>>,
) {
    if shell.route != StudioRoute::Settings {
        return;
    }
    let pointer = windows.single().ok().and_then(Window::cursor_position);
    let page_height = pages
        .iter()
        .map(|page| page.size().y * page.inverse_scale_factor())
        .fold(0.0_f32, f32::max);
    let mut found_content = false;
    let mut pointer_over_content = pointer.is_none();
    let mut max_scroll = 0.0_f32;
    for (computed, transform, _) in contents.iter_mut() {
        found_content = true;
        pointer_over_content |=
            pointer.is_some_and(|position| ui_node_contains_pointer(computed, transform, position));
        let viewport_height = computed.size().y * computed.inverse_scale_factor();
        let reported_content_height = computed.content_size().y * computed.inverse_scale_factor();
        max_scroll = max_scroll.max(settings_scroll_max(
            viewport_height,
            reported_content_height,
            page_height,
        ));
    }
    if !found_content || !pointer_over_content {
        return;
    }
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 38.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            -event.y * scale
        })
        .sum::<f32>();
    if delta.abs() < f32::EPSILON {
        return;
    }

    // A newly rebuilt page can receive a wheel gesture one frame before Bevy
    // publishes its intrinsic height. Preserve that gesture instead of dropping
    // it; the same ScrollPosition becomes valid as soon as layout catches up.
    let tab_index = shell.settings_tab.index();
    let measurement_ready = page_height > 0.0;
    let next = if measurement_ready {
        (shell.settings_scroll_offsets[tab_index] + delta).clamp(0.0, max_scroll)
    } else {
        (shell.settings_scroll_offsets[tab_index] + delta).max(0.0)
    };
    for (computed, _, mut position) in contents.iter_mut() {
        let viewport_height = computed.size().y * computed.inverse_scale_factor();
        let reported_content_height = computed.content_size().y * computed.inverse_scale_factor();
        let local_max = settings_scroll_max(viewport_height, reported_content_height, page_height);
        position.y = if measurement_ready {
            next.clamp(0.0, local_max)
        } else {
            next
        };
    }
    shell.settings_scroll_offsets[tab_index] = next;
}

#[cfg(test)]
mod tests {
    use super::settings_scroll_max;

    #[test]
    fn scroll_extent_uses_measured_page_when_reported_content_is_stale() {
        assert_eq!(settings_scroll_max(800.0, 0.0, 1_200.0), 468.0);
    }

    #[test]
    fn scroll_extent_prefers_larger_reported_content_and_clamps_short_pages() {
        assert_eq!(settings_scroll_max(800.0, 1_000.0, 600.0), 200.0);
        assert_eq!(settings_scroll_max(800.0, 700.0, 600.0), 0.0);
    }
}
