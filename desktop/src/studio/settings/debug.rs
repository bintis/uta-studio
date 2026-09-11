//! Explicit, local debug capture for reproducing rendering failures.
use crate::studio::*;
use std::sync::OnceLock;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, reload, util::SubscriberInitExt};

type LogFilterHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;
static LOG_FILTER: OnceLock<LogFilterHandle> = OnceLock::new();

pub(crate) fn initialize_studio_logging() {
    let filter = if std::env::var("UTA_STUDIO_DEBUG").as_deref() == Ok("1") {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(format!(
                "info,{}",
                crate::studio::startup::studio_log_filter()
            ))
        })
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
    shell.notice = Some(match result {
        Ok(path) => format!(
            "DEBUG logs: {} — detailed capture stays on until exit. Reproduce the issue now; earlier filtered events cannot be recovered. Logs may contain local paths and lyrics; nothing is uploaded.",
            path.display()
        ),
        Err(error) => format!("DEBUG log export failed: {error}"),
    });
    invalidated.invalidate(UiDirtyRegion::Settings);
}
