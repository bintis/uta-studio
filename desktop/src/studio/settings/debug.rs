//! Persistent opt-in DEBUG capture and asynchronous log cleanup.
use crate::studio::*;
use std::sync::OnceLock;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, reload, util::SubscriberInitExt};

type LogFilterHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;
static LOG_FILTER: OnceLock<LogFilterHandle> = OnceLock::new();

fn normal_log_filter() -> EnvFilter {
    EnvFilter::new(format!(
        "info,{},calloop::sources=info",
        crate::studio::startup::studio_log_filter()
    ))
}

pub(crate) fn initialize_studio_logging() {
    let (filter, handle) = reload::Layer::new(normal_log_filter());
    let result = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(crate::studio::startup::AppLogWriter)
                .with_ansi(false),
        )
        .try_init();
    match result {
        Ok(()) => {
            let _ = LOG_FILTER.set(handle);
        }
        Err(error) => {
            app_core::record_log_text(&format!("Could not initialize desktop logging: {error}"))
        }
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        app_core::record_log_text(&format!(
            "PANIC: {info}\n{}",
            std::backtrace::Backtrace::force_capture()
        ));
        previous(info);
    }));
}

fn set_detailed_logging(enabled: bool) -> Result<(), String> {
    LOG_FILTER
        .get()
        .ok_or("Desktop log filter is unavailable")?
        .reload(if enabled {
            EnvFilter::new("debug")
        } else {
            normal_log_filter()
        })
        .map_err(|error| format!("Could not change desktop logging: {error}"))
}

#[derive(Resource, Default)]
pub(crate) struct DebugLogJob {
    receiver: Option<Mutex<mpsc::Receiver<Result<String, String>>>>,
    applied: bool,
    attempted: Option<bool>,
    reported_error: Option<String>,
    pub(crate) clear_requested: bool,
}

impl DebugLogJob {
    pub(crate) fn for_launch(shell: &mut ShellState) -> Self {
        let mut job = Self::default();
        if shell.config.debug_logging {
            job.attempted = Some(true);
            match app_core::start_debug_logging(&debug_context(shell))
                .and_then(|_| set_detailed_logging(true))
            {
                Ok(()) => job.applied = true,
                Err(error) => {
                    app_core::stop_debug_logging();
                    shell.notice = Some(format!("Could not restore DEBUG capture: {error}"));
                }
            }
        }
        job
    }
}

fn debug_context(shell: &ShellState) -> String {
    format!(
        "Uta! Studio {}\nOS: {} / {}\nSettings: {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        serde_json::to_string_pretty(&shell.config).unwrap_or_default()
    )
}

pub(crate) fn toggle_debug_logging(shell: &mut ShellState, job: &mut DebugLogJob) {
    let mut config = shell.config.clone();
    config.debug_logging = !config.debug_logging;
    match config.save() {
        Ok(()) => {
            shell.config = config;
            job.attempted = None;
            shell.notice = Some(
                if job.receiver.is_none() && job.applied == shell.config.debug_logging {
                    format!(
                        "DEBUG {} — setting saved.",
                        if job.applied { "ON" } else { "OFF" }
                    )
                } else {
                    "Updating DEBUG logging…".to_string()
                },
            );
        }
        Err(error) => shell.notice = Some(format!("Could not save DEBUG setting: {error}")),
    }
}

pub(crate) fn poll_debug_log_job(
    mut job: ResMut<DebugLogJob>,
    mut shell: ResMut<ShellState>,
    mut cache_stats: ResMut<CacheStatsJob>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = job
        .receiver
        .as_ref()
        .and_then(|receiver| match receiver.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Log worker exited unexpectedly".to_string()))
                }
            },
            Err(_) => Some(Err("Log status channel was poisoned".to_string())),
        });
    if let Some(result) = result {
        job.receiver = None;
        job.applied = app_core::debug_logging_enabled();
        shell.notice = Some(match result {
            Ok(message) => message,
            Err(error) => format!("Log operation failed: {error}"),
        });
        cache_stats.log_refresh = true;
        invalidated.invalidate(UiDirtyRegion::Settings);
    }
    if let Some(error) = app_core::debug_logging_error()
        && job.reported_error.as_ref() != Some(&error)
    {
        shell.notice = Some(format!("DEBUG capture failed: {error}"));
        job.reported_error = Some(error);
        invalidated.invalidate(UiDirtyRegion::Settings);
    }
    if job.receiver.is_some() {
        return;
    }
    let enabled = shell.config.debug_logging;
    // Cleanup is independent of filter changes: never discard a confirmed
    // clear request because enabling/disabling detailed capture failed.
    let change = !job.clear_requested && job.applied != enabled && job.attempted != Some(enabled);
    if !change && !job.clear_requested {
        return;
    }
    let clear = std::mem::take(&mut job.clear_requested);
    if change {
        job.attempted = Some(enabled);
    }
    job.reported_error = None;
    let context = debug_context(&shell);
    let (sender, receiver) = mpsc::channel();
    job.receiver = Some(Mutex::new(receiver));
    std::thread::spawn(move || {
        let result = (|| {
            if change {
                if enabled {
                    let path = app_core::start_debug_logging(&context)?;
                    if let Err(error) = set_detailed_logging(true) {
                        app_core::stop_debug_logging();
                        return Err(error);
                    }
                    if !clear {
                        return Ok(format!(
                            "DEBUG ON — {}. Setting saved for future launches. Local only; logs may contain paths and lyrics.",
                            path.display()
                        ));
                    }
                } else {
                    set_detailed_logging(false)?;
                    app_core::stop_debug_logging();
                }
            }
            if clear {
                app_core::clear_logs()?;
                Ok("Logs cleared. Songs, charts and models were not changed. If DEBUG is on, new logs continue accumulating.".to_string())
            } else {
                Ok("DEBUG OFF — detailed capture stopped. Normal error logs remain. Already-running workers may retain their startup log level until they exit.".to_string())
            }
        })();
        let _ = sender.send(result);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn completed_job(result: Result<String, String>) -> App {
        let (sender, receiver) = mpsc::channel();
        sender.send(result).unwrap();
        let mut app = App::new();
        app.insert_resource(DebugLogJob {
            receiver: Some(Mutex::new(receiver)),
            ..default()
        });
        app.insert_resource(ShellState {
            config: AppConfig::default(),
            route: StudioRoute::Settings,
            documentation: DocumentationState::default(),
            settings_tab: SettingsTab::General,
            notice: None,
            settings_scroll_offsets: [0.0; 4],
        });
        app.insert_resource(CacheStatsJob::default());
        app.insert_resource(UiInvalidated::default());
        app.add_systems(Update, poll_debug_log_job);
        app
    }

    #[test]
    fn failed_log_operation_is_visible_and_refreshes_measured_size() {
        let mut app = completed_job(Err("isolated filesystem error".to_string()));
        app.update();
        assert!(
            app.world()
                .resource::<ShellState>()
                .notice
                .as_deref()
                .unwrap()
                .contains("isolated filesystem error")
        );
        assert!(app.world().resource::<CacheStatsJob>().log_refresh);
        assert!(app.world().resource::<DebugLogJob>().receiver.is_none());
    }

    #[test]
    fn completed_log_operation_displays_result_and_refreshes_size() {
        let mut app = completed_job(Ok("Logs cleared.".to_string()));
        app.update();
        assert_eq!(
            app.world().resource::<ShellState>().notice.as_deref(),
            Some("Logs cleared.")
        );
        assert!(app.world().resource::<CacheStatsJob>().log_refresh);
    }

    #[test]
    fn off_filter_does_not_enable_debug() {
        let filter = normal_log_filter().to_string();
        assert!(!filter.contains("debug"));
        assert!(filter.contains("icu_provider=error"));
    }
    #[test]
    fn debug_toggle_is_a_registered_mutation() {
        let request = UiAction::from(AppCommand::ToggleDebugLogging).api_request();
        assert_eq!(request.command, "ui.app.toggle_debug_logging");
        assert_eq!(request.access, "mutation");
    }
}
