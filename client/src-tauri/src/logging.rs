use std::collections::VecDeque;
#[cfg(not(debug_assertions))]
use std::io::Write as _;
use std::sync::{Arc, Mutex, OnceLock};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const MAX_LOG_LINES: usize = 2_000;
static LOG_BUFFER: OnceLock<Arc<Mutex<VecDeque<String>>>> = OnceLock::new();

fn buffer() -> Arc<Mutex<VecDeque<String>>> {
    LOG_BUFFER
        .get_or_init(|| Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES))))
        .clone()
}

fn remember(buffer: &Arc<Mutex<VecDeque<String>>>, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = buffer.lock().unwrap();
    for line in text.split_inclusive('\n') {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }
        if lines.len() >= MAX_LOG_LINES {
            lines.pop_front();
        }
        lines.push_back(line.to_string());
    }
}

#[cfg(debug_assertions)]
struct DebugBufferWriter {
    buffer: Arc<Mutex<VecDeque<String>>>,
}

#[cfg(debug_assertions)]
impl std::io::Write for DebugBufferWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        remember(&self.buffer, bytes);
        std::io::stdout().write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().flush()
    }
}

pub fn recent_logs() -> Vec<String> {
    buffer().lock().unwrap().iter().cloned().collect()
}

#[cfg(not(debug_assertions))]
struct LogFileWriter {
    file: Arc<Mutex<std::fs::File>>,
    buffer: Arc<Mutex<VecDeque<String>>>,
}

#[cfg(not(debug_assertions))]
impl std::io::Write for LogFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut f = self.file.lock().unwrap();
        let result = f.write(buf);
        remember(&self.buffer, buf);
        result
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut f = self.file.lock().unwrap();
        f.flush()
    }
}

pub fn init() {
    #[cfg(debug_assertions)]
    {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,app_core=debug,client_lib=debug"));
        let log_buffer = buffer();
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(
                fmt::layer()
                    .with_target(true)
                    .with_writer(move || DebugBufferWriter {
                        buffer: log_buffer.clone(),
                    }),
            )
            .try_init();
    }

    #[cfg(not(debug_assertions))]
    {
        let log_dir = app_core::default_uta_studio_dir();
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("uta-studio.log");

        let file = match std::fs::File::create(&log_path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let shared = Arc::new(Mutex::new(file));
        let writer = shared.clone();
        let log_buffer = buffer();

        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,app_core=debug,client_lib=debug"));

        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(
                fmt::layer()
                    .with_target(true)
                    .with_ansi(false)
                    .with_writer(move || LogFileWriter {
                        file: writer.clone(),
                        buffer: log_buffer.clone(),
                    }),
            )
            .try_init();

        redirect_stderr(&log_path);

        let _ = writeln!(shared.lock().unwrap(), "--- Uta Studio log started ---");
        remember(&buffer(), b"--- Uta Studio log started ---\n");
    }
}

#[cfg(not(debug_assertions))]
fn redirect_stderr(log_path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::io::IntoRawFd;
        if let Ok(file) = std::fs::OpenOptions::new().append(true).open(log_path) {
            let fd = file.into_raw_fd();
            unsafe {
                libc::dup2(fd, 2);
            }
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::io::IntoRawHandle;
        if let Ok(file) = std::fs::OpenOptions::new().append(true).open(log_path) {
            let handle = file.into_raw_handle();
            unsafe {
                windows_sys::Win32::System::Console::SetStdHandle(
                    windows_sys::Win32::System::Console::STD_ERROR_HANDLE,
                    handle as _,
                );
            }
        }
    }
}
