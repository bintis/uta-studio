use super::*;

#[test]
fn raw_kernel_data_is_copied_and_synchronized_before_reporting_progress() {
    // No child process, live journal, device access, or user file is used.
    let bytes = b"{\"MESSAGE\":\"fixture error\"}\npartial\xff";
    let mut output = Vec::new();
    let mut synchronizations = 0;
    let copied = copy_records(bytes.as_slice(), &mut output, |destination| {
        assert_eq!(destination.as_slice(), bytes);
        synchronizations += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(copied, bytes.len() as u64);
    assert_eq!(output, bytes);
    assert_eq!(synchronizations, 1);
}

#[test]
fn large_streams_are_drained_in_bounded_chunks_without_dropping_bytes() {
    let bytes = vec![b'x'; 100_003];
    let mut output = Vec::new();
    let mut previous = 0;
    copy_records(bytes.as_slice(), &mut output, |destination| {
        assert!(destination.len() > previous);
        assert!(destination.len() - previous <= 16 * 1024);
        previous = destination.len();
        Ok(())
    })
    .unwrap();
    assert_eq!(output, bytes);
}

#[test]
fn a_sync_failure_stops_capture_without_claiming_a_complete_copy() {
    let bytes = vec![b'x'; 40_000];
    let mut output = Vec::new();
    let failure = copy_records(bytes.as_slice(), &mut output, |_| {
        Err(io::Error::other("fixture sync failure"))
    })
    .unwrap_err();
    assert!(failure.to_string().contains("sync failure"));
    assert_eq!(output.len(), 16 * 1024);
}

#[test]
fn missing_capture_is_an_observable_error_not_evidence_of_a_clean_kernel() {
    let error = Mutex::new(None);
    report(&error, "journal permissions unavailable".to_owned());
    report(&error, "later error".to_owned());
    assert_eq!(
        error.into_inner().unwrap().as_deref(),
        Some("journal permissions unavailable")
    );
    let mut empty = Vec::new();
    let mut synchronizations = 0;
    assert_eq!(
        copy_records(&b""[..], &mut empty, |_| {
            synchronizations += 1;
            Ok(())
        })
        .unwrap(),
        0
    );
    assert_eq!(synchronizations, 0);
}

#[test]
fn capture_uses_documented_kernel_options_and_the_requested_boot() {
    let boot = "01234567-89ab-cdef-0123-456789abcdef";
    let command = journal_command(boot);
    assert_eq!(command.get_program(), std::ffi::OsStr::new("journalctl"));
    let arguments = command
        .get_args()
        .map(|argument| argument.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        arguments,
        [
            "--dmesg",
            "--follow",
            "--no-pager",
            "--output=json",
            "--all",
            "--lines=100",
            "--boot=0123456789abcdef0123456789abcdef",
        ]
    );
}

#[test]
fn proc_boot_selector_is_compact_and_never_changes_to_the_current_boot() {
    // The newline is present in the real /proc file. Checking --version alone
    // misses this regression: the tool can exit before resolving --boot.
    let command = journal_command("01234567-89ab-cdef-0123-456789abcdef\n");
    let selector = command.get_args().last().unwrap().to_str().unwrap();
    assert_eq!(selector, "--boot=0123456789abcdef0123456789abcdef");
    let selected = selector.strip_prefix("--boot=").unwrap();
    assert_eq!(selected.len(), 32);
    assert!(selected.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(selected, "0");
}

#[test]
#[ignore = "explicit host-tool option check; --version exits without reading or following the journal"]
fn installed_journalctl_parses_the_actual_capture_arguments() {
    // Exercise the same command builder, not a mock that accepts every flag.
    // All capture options precede --version, so an invalid selector is rejected
    // during argument parsing. This does not validate boot resolution or
    // journal permissions; those require an explicit read-only journal check.
    let mut command = journal_command("01234567-89ab-cdef-0123-456789abcdef");
    let output = command
        .arg("--version")
        .output()
        .expect("journalctl is required for this explicit check");
    assert!(
        output.status.success(),
        "journalctl rejected capture arguments: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
