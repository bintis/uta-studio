use super::*;
use std::fs::{self, File};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "uta-studio-lifecycle-log-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn complete_json_lines_are_published_without_closing_the_writer() {
    let fixture = Fixture::new();
    let path = fixture.0.join("run.jsonl");
    let mut file = File::create(&path).unwrap();
    let intent = serde_json::json!({"record_type":"run_requested", "message":"歌詞\n引用\""});
    write_analysis_record(&mut file, &intent).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(contents.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&contents).unwrap(),
        intent
    );
    append_analysis_log_terminal(Some(&path), "failed", Some("native process interrupted"));
    let contents = fs::read_to_string(&path).unwrap();
    let records = contents
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0], intent);
    assert_eq!(records[1]["status"], "failed");
}

#[test]
fn failed_record_write_is_an_error_and_does_not_erase_prior_evidence() {
    let fixture = Fixture::new();
    let path = fixture.0.join("run.jsonl");
    fs::write(&path, b"retained evidence\n").unwrap();
    let mut read_only = File::open(&path).unwrap();
    assert!(
        write_analysis_record(&mut read_only, &serde_json::json!({"phase":"complete"})).is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), b"retained evidence\n");
}
