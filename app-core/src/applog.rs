//! Real app-log capture: a bounded in-memory ring buffer plus a best-effort
//! on-disk file, fed by a genuine `tracing_subscriber` layer
//! (`desktop/src/studio/startup.rs`'s `AppLogWriter`/`app_log_custom_layer`).
//! Backs `get_log_path`/`get_recent_logs` (`app-core/src/api.rs`'s
//! `API_CAPABILITIES` catalogue entries) for application lifecycle and
//! unscoped IPC diagnostics. Analysis runs use separate JSONL files.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

const MAX_BUFFERED_LINES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogLine {
    pub timestamp_ms: i64,
    /// Already fully formatted by `tracing_subscriber::fmt` -- one real
    /// line as it would have appeared on stdout, not a reconstruction.
    pub text: String,
}

static LOG_BUFFER: LazyLock<Mutex<VecDeque<LogLine>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(MAX_BUFFERED_LINES)));

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Called once per formatted line by the desktop-side tracing writer.
/// Pushes into the bounded ring buffer and best-effort appends to the real
/// log file -- a write failure here must never panic or itself log through
/// `tracing` (that would recurse back into this same function).
pub fn record_log_text(text: &str) {
    let text = text.trim_end_matches('\n');
    if text.is_empty() {
        return;
    }
    let line = LogLine {
        timestamp_ms: now_ms(),
        text: text.to_string(),
    };
    {
        let mut buffer = LOG_BUFFER.lock().unwrap();
        if buffer.len() == MAX_BUFFERED_LINES {
            buffer.pop_front();
        }
        buffer.push_back(line);
    }
    if let Some(path) = get_log_path()
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
    {
        let _ = writeln!(file, "{text}");
    }
}

/// Non-panicking, mirrors `CacheDir::try_new()`'s exact reasoning (`nix
/// build`'s sandboxed `checkPhase` has an unwritable `$HOME`) -- `None`
/// when the containing directory can't be created, not a panic.
pub fn get_log_path() -> Option<PathBuf> {
    let dir = crate::cache::uta_studio_dir();
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("app.log"))
}

/// Most recent `limit` lines, oldest first (chronological reading order).
pub fn get_recent_logs(limit: usize) -> Vec<LogLine> {
    let buffer = LOG_BUFFER.lock().unwrap();
    let skip = buffer.len().saturating_sub(limit);
    buffer.iter().skip(skip).cloned().collect()
}

/// Lines whose timestamp falls within `[start_ms, end_ms]` (inclusive on
/// both ends), oldest first.
pub fn log_lines_in_window(start_ms: i64, end_ms: i64) -> Vec<LogLine> {
    let buffer = LOG_BUFFER.lock().unwrap();
    buffer
        .iter()
        .filter(|line| line.timestamp_ms >= start_ms && line.timestamp_ms <= end_ms)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// `LOG_BUFFER` is a process-wide singleton shared with every other
    /// test in this module (and, in a real run, the whole app); serialize
    /// tests that touch it so they can't interleave and observe each
    /// other's pushed lines, same reasoning as `analyzer.rs`'s
    /// `PENDING_NODE_INTENTS` test guard.
    static GUARD: StdMutex<()> = StdMutex::new(());

    fn clear_buffer() {
        LOG_BUFFER.lock().unwrap().clear();
    }

    #[test]
    fn pushing_past_the_cap_drops_the_oldest_line() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        clear_buffer();
        for i in 0..MAX_BUFFERED_LINES + 5 {
            record_log_text(&format!("line-{i}"));
        }
        let buffer = LOG_BUFFER.lock().unwrap();
        assert_eq!(buffer.len(), MAX_BUFFERED_LINES);
        assert_eq!(buffer.front().unwrap().text, "line-5");
        assert_eq!(
            buffer.back().unwrap().text,
            format!("line-{}", MAX_BUFFERED_LINES + 4)
        );
    }

    #[test]
    fn empty_and_whitespace_only_lines_are_not_recorded() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        clear_buffer();
        record_log_text("");
        record_log_text("\n");
        record_log_text("real line");
        let buffer = LOG_BUFFER.lock().unwrap();
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer[0].text, "real line");
    }

    #[test]
    fn get_recent_logs_returns_the_most_recent_n_in_chronological_order() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        clear_buffer();
        for i in 0..10 {
            record_log_text(&format!("line-{i}"));
        }
        let recent = get_recent_logs(3);
        let texts: Vec<&str> = recent.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["line-7", "line-8", "line-9"]);
    }

    #[test]
    fn get_recent_logs_with_a_limit_larger_than_the_buffer_returns_everything() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        clear_buffer();
        record_log_text("only line");
        assert_eq!(get_recent_logs(80).len(), 1);
    }

    #[test]
    fn log_lines_in_window_is_inclusive_on_both_ends_and_excludes_outside() {
        let _guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        clear_buffer();
        {
            let mut buffer = LOG_BUFFER.lock().unwrap();
            buffer.push_back(LogLine {
                timestamp_ms: 100,
                text: "before".to_string(),
            });
            buffer.push_back(LogLine {
                timestamp_ms: 200,
                text: "start-boundary".to_string(),
            });
            buffer.push_back(LogLine {
                timestamp_ms: 250,
                text: "inside".to_string(),
            });
            buffer.push_back(LogLine {
                timestamp_ms: 300,
                text: "end-boundary".to_string(),
            });
            buffer.push_back(LogLine {
                timestamp_ms: 400,
                text: "after".to_string(),
            });
        }
        let lines = log_lines_in_window(200, 300);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["start-boundary", "inside", "end-boundary"]);
    }

    /// Mirrors `CacheDir`'s `try_new_never_panics_and_matches_new_when_it_succeeds`
    /// pattern (`app-core/src/cache.rs`): correct behavior in an unwritable
    /// environment (like `nix build`'s sandboxed `checkPhase`) is `None`,
    /// not a panic -- this test only asserts non-panicking, since whether
    /// the real environment running it is writable varies.
    #[test]
    fn get_log_path_never_panics_regardless_of_environment_writability() {
        let _ = get_log_path();
    }
}
