//! Read-only, current-boot kernel capture for an explicitly enabled DEBUG session.
//! It owns only journalctl; it never starts inference, changes journal settings,
//! elevates privileges, retries a model, or probes GPU counters.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) struct KernelCapture {
    stop: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    worker: Option<JoinHandle<()>>,
}

impl KernelCapture {
    pub(super) fn start(directory: &Path) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let directory = directory.to_owned();
        let worker_stop = Arc::clone(&stop);
        let worker_error = Arc::clone(&error);
        let worker = thread::Builder::new()
            .name("uta-kernel-capture".into())
            .spawn(move || {
                if let Err(failure) = capture(&directory, &worker_stop, &worker_error) {
                    report(
                        &worker_error,
                        format!("Kernel capture unavailable: {failure}"),
                    );
                }
            });
        let worker = match worker {
            Ok(worker) => Some(worker),
            Err(failure) => {
                report(&error, format!("Could not start kernel capture: {failure}"));
                None
            }
        };
        Self {
            stop,
            error,
            worker,
        }
    }

    pub(super) fn error(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl Drop for KernelCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn report(error: &Mutex<Option<String>>, message: String) {
    error
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get_or_insert(message);
}

struct JournalChild(Child);
impl Drop for JournalChild {
    fn drop(&mut self) {
        // Reap only our read-only logger. No PID search or process-group kill.
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn new_log(directory: &Path, name: &str) -> io::Result<File> {
    let path = directory.join(name);
    let file =
        crate::log_storage::open_file(&path, OpenOptions::new().create_new(true).write(true))
            .map_err(io::Error::other)?;
    file.sync_all()?;
    File::open(directory)?.sync_all()?;
    Ok(file)
}

fn status(file: &mut File, phase: &str, detail: serde_json::Value) -> io::Result<()> {
    let record = serde_json::json!({
        "record_type": "kernel_capture",
        "phase": phase,
        "pid": std::process::id(),
        "timestamp_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        "detail": detail,
    });
    let mut encoded = serde_json::to_vec(&record).map_err(io::Error::other)?;
    encoded.push(b'\n');
    file.write_all(&encoded)?;
    file.sync_data()
}

fn capture(
    directory: &Path,
    stop: &AtomicBool,
    error: &Arc<Mutex<Option<String>>>,
) -> io::Result<()> {
    let mut status_log = new_log(directory, "kernel-status.jsonl")?;
    let result = capture_inner(directory, stop, error, &mut status_log);
    if let Err(failure) = &result {
        let _ = status(
            &mut status_log,
            "capture_failed",
            serde_json::json!({"error":failure.to_string()}),
        );
    }
    result
}

// Keep command construction shared with the option-parsing regression check.
// journalctl's kernel selector is --dmesg (-k), not --kernel.
fn journal_command(boot: &str) -> Command {
    // /proc supplies the UUID with hyphens; journalctl's boot descriptor
    // requires the compact journal ID. Keep the original UUID in our status
    // records, but normalize the selector before invoking the host tool.
    let boot = boot.trim().replace('-', "");
    let mut command = Command::new("journalctl");
    command
        .args([
            "--dmesg",
            "--follow",
            "--no-pager",
            "--output=json",
            "--all",
            "--lines=100",
        ])
        .arg(format!("--boot={boot}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
fn capture_inner(
    directory: &Path,
    stop: &AtomicBool,
    error: &Arc<Mutex<Option<String>>>,
    status_log: &mut File,
) -> io::Result<()> {
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    let boot = boot.trim();
    let output = new_log(directory, "kernel.jsonl")?;
    let errors = new_log(directory, "kernel-stderr.log")?;
    let mut command = journal_command(boot);
    status(
        status_log,
        "capture_intent",
        serde_json::json!({
            "boot_id":boot,
            "program":command.get_program().to_string_lossy(),
            "arguments":command.get_args().map(|argument| argument.to_string_lossy()).collect::<Vec<_>>(),
            "source":"current-boot kernel journal; last 100 available records, then live follow",
            "limitation":"permissions or abrupt power loss can prevent delivery; silence is not proof of stability",
        }),
    )?;
    // The spawning thread is this long-lived supervisor, not the temporary
    // snapshot thread. Linux parent-death notification is thread-sensitive.
    let parent = std::process::id() as libc::pid_t;
    unsafe {
        // SAFETY: only async-signal-safe libc operations and errno construction
        // occur between fork and exec. No allocation, lock or app callback.
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(io::Error::from_raw_os_error(libc::ESRCH));
            }
            Ok(())
        });
    }
    let mut child = JournalChild(command.spawn()?);
    status(
        status_log,
        "journal_process_started",
        serde_json::json!({
            "child_pid":child.0.id(), "boot_id":boot,
        }),
    )?;
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("journal stdout missing"))?;
    let stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("journal stderr missing"))?;
    let failed = Arc::new(AtomicBool::new(false));
    let output_reader = reader(stdout, output, "kernel.jsonl", &failed, error)?;
    let error_reader = match reader(stderr, errors, "kernel-stderr.log", &failed, error) {
        Ok(reader) => reader,
        Err(failure) => {
            drop(child);
            let _ = output_reader.join();
            return Err(failure);
        }
    };
    let outcome = loop {
        if stop.load(Ordering::Acquire) || failed.load(Ordering::Acquire) {
            let _ = child.0.kill();
            break child.0.wait();
        }
        match child.0.try_wait() {
            Ok(Some(outcome)) => break Ok(outcome),
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(failure) => break Err(failure),
        }
    };
    drop(child); // close both pipes before joining their readers, even on wait failure
    let output_result = output_reader.join();
    let error_result = error_reader.join();
    let captured = output_result.map_err(|_| io::Error::other("kernel reader panicked"))??;
    error_result.map_err(|_| io::Error::other("kernel error reader panicked"))??;
    let outcome = outcome?;
    let stopping = stop.load(Ordering::Acquire);
    status(
        status_log,
        "capture_stopped",
        serde_json::json!({
            "boot_id":boot, "status":outcome.to_string(), "captured_bytes":captured,
            "requested_stop":stopping, "kernel_coverage_asserted":false,
        }),
    )?;
    if !stopping {
        return Err(io::Error::other(format!(
            "journalctl stopped unexpectedly ({outcome}); see kernel-stderr.log"
        )));
    }
    Ok(())
}

fn reader(
    source: impl Read + Send + 'static,
    mut destination: File,
    name: &'static str,
    failed: &Arc<AtomicBool>,
    error: &Arc<Mutex<Option<String>>>,
) -> io::Result<JoinHandle<io::Result<u64>>> {
    let failed = Arc::clone(failed);
    let error = Arc::clone(error);
    thread::Builder::new()
        .name("uta-kernel-drain".into())
        .spawn(move || {
            let result = copy_records(source, &mut destination, |file| file.sync_data());
            if let Err(failure) = &result {
                report(&error, format!("Kernel capture {name}: {failure}"));
                failed.store(true, Ordering::Release);
            }
            result
        })
}

fn copy_records<W: Write>(
    mut source: impl Read,
    destination: &mut W,
    mut synchronize: impl FnMut(&mut W) -> io::Result<()>,
) -> io::Result<u64> {
    let mut bytes = [0_u8; 16 * 1024];
    let mut copied = 0;
    loop {
        let count = match source.read(&mut bytes) {
            Ok(0) => return Ok(copied),
            Ok(count) => count,
            Err(failure) if failure.kind() == io::ErrorKind::Interrupted => continue,
            Err(failure) => return Err(failure),
        };
        // Preserve raw journal JSON/stderr, including a partial final record.
        // Kernel I/O never runs on the native submission or desktop UI thread.
        destination.write_all(&bytes[..count])?;
        destination.flush()?;
        synchronize(destination)?;
        copied += count as u64;
    }
}

#[cfg(test)]
mod tests;
