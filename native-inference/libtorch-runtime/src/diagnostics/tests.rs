use super::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Default)]
struct ObservedWriter {
    bytes: Vec<u8>,
    calls: RefCell<Vec<&'static str>>,
    fail_write: bool,
    fail_sync: bool,
}
impl Write for ObservedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.calls.borrow_mut().push("write");
        if self.fail_write {
            return Err(io::Error::other("write failure"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.calls.borrow_mut().push("flush");
        Ok(())
    }
}
impl DurableWrite for ObservedWriter {
    fn sync_record(&self) -> io::Result<()> {
        self.calls.borrow_mut().push("sync");
        if self.fail_sync {
            Err(io::Error::other("sync failure"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn an_intent_returns_only_after_its_complete_record_is_synchronized() {
    let mut writer = ObservedWriter::default();
    let value =
        serde_json::json!({"phase": "device_create_begin", "detail": "line\nquote\"日本語"});
    RecordBuffer::default()
        .append(&mut writer, &value, true, Duration::ZERO)
        .unwrap();
    assert_eq!(*writer.calls.borrow(), ["write", "flush", "sync"]);
    assert_eq!(
        writer.bytes.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&writer.bytes).unwrap(),
        value
    );
}

#[test]
fn a_write_failure_does_not_claim_a_successful_sync() {
    let mut writer = ObservedWriter {
        fail_write: true,
        ..Default::default()
    };
    assert!(
        RecordBuffer::default()
            .append(
                &mut writer,
                &serde_json::json!({"phase":"begin"}),
                true,
                Duration::ZERO,
            )
            .is_err()
    );
    assert_eq!(*writer.calls.borrow(), ["write"]);
}

#[test]
fn a_sync_failure_is_not_reported_as_a_persisted_record() {
    let mut writer = ObservedWriter {
        fail_sync: true,
        ..Default::default()
    };
    let error = RecordBuffer::default()
        .append(
            &mut writer,
            &serde_json::json!({"phase":"begin"}),
            true,
            Duration::ZERO,
        )
        .unwrap_err();
    assert!(error.to_string().contains("sync failure"));
    assert_eq!(*writer.calls.borrow(), ["write", "flush", "sync"]);
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-native-trace-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn file_is_readable_while_writer_is_alive_and_does_not_invent_completion() {
    let fixture = Fixture::new();
    let mut journal = Journal::create(&fixture.0).unwrap();
    journal.record("device_create_begin", "xpu").unwrap();
    let files = fs::read_dir(&fixture.0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let bytes = fs::read_to_string(files[0].path()).unwrap();
    let records = bytes
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["phase"], "journal_opened");
    assert_eq!(records[1]["phase"], "journal_policy");
    assert_eq!(records[2]["phase"], "device_create_begin");
    assert_eq!(records[2]["sequence"], 2);
    assert!(records[2].get("boot_id").is_some());
    assert!(!bytes.contains("device_create_complete"));
    // Ordinary file visibility is not a simulated power-loss qualification.
    // The separate writer oracle verifies that RecordBuffer invokes sync.
}

#[test]
fn detailed_events_share_writes_and_a_boundary_flushes_them_in_order() {
    let mut buffer = RecordBuffer::default();
    let mut writer = ObservedWriter::default();
    for sequence in 0..100 {
        buffer
            .append(
                &mut writer,
                &serde_json::json!({"sequence":sequence}),
                false,
                Duration::from_millis(10),
            )
            .unwrap();
    }
    assert!(writer.calls.borrow().is_empty());
    buffer
        .append(
            &mut writer,
            &serde_json::json!({"phase":"compute_complete"}),
            true,
            Duration::from_millis(20),
        )
        .unwrap();
    assert_eq!(*writer.calls.borrow(), ["write", "flush", "sync"]);
    let records: Vec<serde_json::Value> = String::from_utf8(writer.bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 101);
    for (sequence, record) in records[..100].iter().enumerate() {
        assert_eq!(record["sequence"], sequence);
    }
    assert_eq!(records[100]["phase"], "compute_complete");
}

#[test]
fn interval_sync_occurs_on_the_next_event_not_a_background_timer() {
    let mut buffer = RecordBuffer::default();
    let mut writer = ObservedWriter::default();
    buffer
        .append(
            &mut writer,
            &serde_json::json!({"sequence":0}),
            false,
            Duration::from_millis(999),
        )
        .unwrap();
    assert!(writer.calls.borrow().is_empty());
    buffer
        .append(
            &mut writer,
            &serde_json::json!({"sequence":1}),
            false,
            Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(*writer.calls.borrow(), ["write", "flush", "sync"]);
}

#[test]
fn byte_limit_flushes_without_forcing_a_sync_for_each_detail() {
    let mut buffer = RecordBuffer::default();
    let mut writer = ObservedWriter::default();
    let detail = "x".repeat(4096);
    for sequence in 0..100 {
        buffer
            .append(
                &mut writer,
                &serde_json::json!({"sequence":sequence,"detail":detail}),
                false,
                Duration::ZERO,
            )
            .unwrap();
    }
    assert!(!writer.bytes.is_empty());
    assert!(!writer.calls.borrow().contains(&"sync"));
    assert!(
        writer
            .calls
            .borrow()
            .iter()
            .filter(|call| **call == "write")
            .count()
            < 10
    );
    buffer
        .append(
            &mut writer,
            &serde_json::json!({"phase":"native_error"}),
            true,
            Duration::ZERO,
        )
        .unwrap();
    assert_eq!(
        writer
            .calls
            .borrow()
            .iter()
            .filter(|call| **call == "sync")
            .count(),
        1
    );
    assert_eq!(
        String::from_utf8(writer.bytes).unwrap().lines().count(),
        101
    );
}

#[test]
fn focus_is_a_component_prefix_not_a_similar_layer_or_an_empty_match() {
    assert!(focus_matches(
        Some("blk.12.freq"),
        "blk.12.freq.feed_forward.start.116736"
    ));
    assert!(focus_matches(Some("blk.12.freq"), "blk.12.freq batches=64"));
    assert!(!focus_matches(Some("blk.1"), "blk.12.freq"));
    assert!(!focus_matches(Some(""), "blk.12.freq"));
    assert!(!focus_matches(None, "blk.12.freq"));
    assert!(is_detail("roformer_operator_begin"));
    for phase in [
        "native_error",
        "device_create_begin",
        "model_request",
        "forward_begin",
        "compute_begin",
        "compute_complete",
        "library_unload_complete",
        "roformer_feed_forward_begin",
    ] {
        assert!(!is_detail(phase));
    }
}
