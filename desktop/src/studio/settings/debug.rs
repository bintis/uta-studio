//! Explicit, local debug capture for reproducing rendering failures.
use crate::studio::*;
use std::sync::OnceLock;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, reload, util::SubscriberInitExt};

type LogFilterHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;
static LOG_FILTER: OnceLock<LogFilterHandle> = OnceLock::new();

fn normal_log_filter(environment: &str) -> EnvFilter {
    let defaults = EnvFilter::new(format!(
        "info,{}",
        crate::studio::startup::studio_log_filter()
    ));
    environment
        .split(',')
        .filter(|directive| !directive.is_empty())
        .try_fold(defaults.clone(), |filter, directive| {
            directive
                .parse::<tracing_subscriber::filter::Directive>()
                .map(|directive| filter.add_directive(directive))
        })
        .unwrap_or(defaults)
}

pub(crate) fn initialize_studio_logging() {
    let filter = if std::env::var("UTA_STUDIO_DEBUG").as_deref() == Ok("1") {
        EnvFilter::new("debug")
    } else {
        normal_log_filter(&std::env::var("RUST_LOG").unwrap_or_default())
    };
    let (filter, handle) = reload::Layer::new(filter);
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

fn enable_detailed_logging() -> Result<(), String> {
    LOG_FILTER
        .get()
        .ok_or("Desktop log filter is unavailable")?
        .reload(EnvFilter::new("debug"))
        .map_err(|error| format!("Could not enable detailed desktop logging: {error}"))
}

#[derive(Resource, Default)]
pub(crate) struct DebugLogJob {
    receiver: Option<Mutex<mpsc::Receiver<Result<std::path::PathBuf, String>>>>,
    capture_active: bool,
    reported_error: Option<String>,
}

pub(crate) fn start_debug_log_job(job: &mut DebugLogJob, context: String) -> String {
    if job.receiver.is_some() {
        return "Debug log export is already running…".to_string();
    }
    let (sender, receiver) = mpsc::channel();
    job.receiver = Some(Mutex::new(receiver));
    std::thread::spawn(move || {
        let result = app_core::start_debug_logging(&context).and_then(|path| {
            enable_detailed_logging()
                .map_err(|error| format!("Logs saved to {}, but {error}", path.display()))?;
            bevy::log::debug!("DEBUG capture enabled; reproduce the issue now");
            Ok(path)
        });
        let _ = sender.send(result);
    });
    "Saving full retained logs and enabling live DEBUG capture…".to_string()
}

pub(crate) fn poll_debug_log_job(
    mut job: ResMut<DebugLogJob>,
    mut shell: ResMut<ShellState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if job.capture_active
        && let Some(error) = app_core::debug_logging_error()
        && job.reported_error.as_ref() != Some(&error)
    {
        shell.notice = Some(format!("DEBUG live capture failed: {error}"));
        job.reported_error = Some(error);
        invalidated.invalidate(UiDirtyRegion::Settings);
    }
    let Some(receiver) = job.receiver.as_ref() else {
        return;
    };
    let result = match receiver.lock() {
        Ok(receiver) => match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Debug log export worker exited unexpectedly".to_string())
            }
        },
        Err(_) => Err("Debug log export status channel was poisoned".to_string()),
    };
    job.receiver = None;
    if result.is_ok() {
        job.capture_active = true;
        job.reported_error = None;
    }
    shell.notice = Some(match result {
        Ok(path) => format!(
            "DEBUG logs: {} — detailed capture stays on until exit. Reproduce the issue now; earlier filtered events cannot be recovered. Logs may contain local paths and lyrics; nothing is uploaded.",
            path.display()
        ),
        Err(error) => format!("DEBUG log export failed: {error}"),
    });
    invalidated.invalidate(UiDirtyRegion::Settings);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_app(result: Result<std::path::PathBuf, String>) -> App {
        let (sender, receiver) = mpsc::channel();
        sender.send(result).unwrap();
        let mut app = App::new();
        app.insert_resource(DebugLogJob {
            receiver: Some(Mutex::new(receiver)),
            ..default()
        });
        app.insert_resource(ShellState {
            config: app_core::AppConfig::default(),
            route: StudioRoute::Settings,
            documentation: DocumentationState::default(),
            settings_tab: SettingsTab::General,
            notice: None,
            settings_scroll_offsets: [0.0; 4],
        });
        app.insert_resource(UiInvalidated::default());
        app.add_systems(Update, poll_debug_log_job);
        app
    }

    #[test]
    fn completed_export_displays_location_and_capture_scope() {
        let mut app = job_app(Ok(std::path::PathBuf::from("isolated-debug-output")));
        app.update();
        let notice = app
            .world()
            .resource::<ShellState>()
            .notice
            .as_deref()
            .unwrap();
        assert!(notice.contains("isolated-debug-output"));
        assert!(notice.contains("until exit"));
        assert!(app.world().resource::<DebugLogJob>().receiver.is_none());
    }

    #[test]
    fn failed_export_displays_error_without_reporting_success() {
        let mut app = job_app(Err("isolated disk error".to_string()));
        app.update();
        let notice = app
            .world()
            .resource::<ShellState>()
            .notice
            .as_deref()
            .unwrap();
        assert!(notice.contains("failed: isolated disk error"));
        assert!(!app.world().resource::<DebugLogJob>().capture_active);
    }

    #[test]
    fn target_filter_retains_default_desktop_and_icu_directives() {
        let filter = normal_log_filter("uta_studio=debug").to_string();
        assert!(filter.split(',').any(|directive| directive == "info"));
        assert!(filter.contains("icu_provider=error"));
        assert!(filter.contains("uta_studio=debug"));
    }

    #[test]
    fn debug_command_is_a_registered_mutation() {
        let command = AppCommand::StartDebugLogging;
        let request = UiAction::from(command).api_request();
        assert_eq!(request.command, "ui.app.start_debug_logging");
        assert_eq!(request.access, "mutation");
        assert!(
            include_str!("general.rs")
                .contains("Some((\"DEBUG\", UiAction::from(AppCommand::StartDebugLogging)))")
        );
    }
}
