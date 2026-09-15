use super::*;
use std::cell::RefCell;
use std::path::PathBuf;

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
    persist(&mut writer, &value).unwrap();
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
    assert!(persist(&mut writer, &serde_json::json!({"phase":"begin"})).is_err());
    assert_eq!(*writer.calls.borrow(), ["write"]);
}

#[test]
fn a_sync_failure_is_not_reported_as_a_persisted_record() {
    let mut writer = ObservedWriter {
        fail_sync: true,
        ..Default::default()
    };
    let error = persist(&mut writer, &serde_json::json!({"phase":"begin"})).unwrap_err();
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
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["phase"], "journal_opened");
    assert_eq!(records[1]["phase"], "device_create_begin");
    assert_eq!(records[1]["sequence"], 1);
    assert!(records[1].get("boot_id").is_some());
    assert!(!bytes.contains("device_create_complete"));
    // Ordinary file visibility is not a simulated power-loss qualification.
    // The separate writer oracle verifies that persist actually invokes sync.
}
