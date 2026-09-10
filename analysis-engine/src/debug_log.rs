//! Opt-in diagnostics on stderr; stdout remains the machine protocol.

use std::io::{self, Read, Write};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("UTA_STUDIO_DEBUG").as_deref() == Ok("1"))
}

pub(crate) fn record(source: &str, value: &impl serde::Serialize) {
    if !enabled() {
        return;
    }
    let _ = write_record(&mut io::stderr().lock(), source, value);
}

fn write_record(
    writer: &mut impl Write,
    source: &str,
    value: &impl serde::Serialize,
) -> io::Result<()> {
    let event = serde_json::json!({
        "debug": source,
        "pid": std::process::id(),
        "time_unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        "value": value,
    });
    serde_json::to_writer(&mut *writer, &event)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Drain all stderr, mirror each read immediately, and retain only the existing
/// bounded failure excerpt. Mirror failure must not stop draining the worker.
pub(crate) fn capture_stderr(
    mut reader: impl Read,
    captured: &std::sync::Mutex<Vec<u8>>,
    limit: usize,
    mut mirror: impl FnMut(&[u8]) -> io::Result<()>,
) {
    let mut buffer = [0_u8; 8192];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let _ = mirror(&buffer[..count]);
        let mut bytes = captured.lock().unwrap_or_else(|error| error.into_inner());
        let remaining = limit.saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..count.min(remaining)]);
    }
}

pub(crate) fn mirror_stderr(bytes: &[u8]) -> io::Result<()> {
    if enabled() {
        let mut stderr = io::stderr().lock();
        stderr.write_all(bytes)?;
        stderr.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn stderr_mirror_preserves_bytes_beyond_the_failure_excerpt() {
        let input = vec![0xff; 1024 * 1024 + 123];
        let captured = Mutex::new(Vec::new());
        let mut mirrored = Vec::new();
        capture_stderr(input.as_slice(), &captured, 1024 * 1024, |bytes| {
            mirrored.extend_from_slice(bytes);
            Ok(())
        });
        assert_eq!(mirrored, input);
        assert_eq!(*captured.lock().unwrap(), input[..1024 * 1024]);
    }

    #[test]
    fn a_failed_debug_sink_does_not_stop_stderr_capture() {
        let input = vec![b'x'; 20000];
        let captured = Mutex::new(Vec::new());
        capture_stderr(input.as_slice(), &captured, input.len(), |_| {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        });
        assert_eq!(*captured.lock().unwrap(), input);
    }

    #[test]
    fn debug_records_are_complete_single_lines_and_flush_immediately() {
        #[derive(Default)]
        struct Sink {
            bytes: Vec<u8>,
            flushed: bool,
        }
        impl Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.flushed = true;
                Ok(())
            }
        }
        let mut sink = Sink::default();
        write_record(&mut sink, "worker_stdout", &"progress\n日本語").unwrap();
        assert!(sink.flushed);
        assert_eq!(sink.bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
        let event: serde_json::Value = serde_json::from_slice(&sink.bytes).unwrap();
        assert_eq!(event["debug"], "worker_stdout");
        assert_eq!(event["value"], "progress\n日本語");
    }
}
