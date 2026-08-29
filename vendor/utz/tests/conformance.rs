//! Runs the shared conformance fixtures in `format/fixtures/`. Every
//! independent implementation must agree with this suite: `valid/` documents
//! parse and validate, `invalid/` documents are rejected at parse or
//! validation time.

use std::path::{Path, PathBuf};

use utz::{Result, UtzManifest, VocalChart};

fn fixture_dir(kind: &str, verdict: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("format/fixtures")
        .join(kind)
        .join(verdict)
}

fn fixtures(kind: &str, verdict: &str) -> Vec<(String, Vec<u8>)> {
    let dir = fixture_dir(kind, verdict);
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        // Pre-1.0 manifests are development snapshots, not compatibility
        // obligations. Keep historical fixtures in-tree if useful, but the
        // current conformance suite only defines UTZ 0.3 behavior.
        .filter(|path| {
            kind != "manifest"
                || path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("v0.3"))
        })
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no fixtures in {}", dir.display());
    entries
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&path).unwrap())
        })
        .collect()
}

fn check<T, P, V>(kind: &str, parse: P, validate: V)
where
    P: Fn(&[u8]) -> serde_json::Result<T>,
    V: Fn(&T) -> Result<()>,
{
    for (name, bytes) in fixtures(kind, "valid") {
        let value = parse(&bytes)
            .unwrap_or_else(|error| panic!("{kind}/valid/{name} failed to parse: {error}"));
        validate(&value)
            .unwrap_or_else(|error| panic!("{kind}/valid/{name} failed to validate: {error}"));
    }
    for (name, bytes) in fixtures(kind, "invalid") {
        let rejected = match parse(&bytes) {
            Err(_) => true,
            Ok(value) => validate(&value).is_err(),
        };
        assert!(rejected, "{kind}/invalid/{name} was accepted");
    }
}

#[test]
fn vocal_chart_fixtures() {
    check(
        "vocal-chart",
        |bytes| serde_json::from_slice::<VocalChart>(bytes),
        VocalChart::validate,
    );
}

#[test]
fn manifest_fixtures() {
    check(
        "manifest",
        |bytes| serde_json::from_slice::<UtzManifest>(bytes),
        UtzManifest::validate,
    );
}

#[test]
fn vocal_chart_rejects_malformed_json_and_duplicate_ids() {
    assert!(serde_json::from_slice::<VocalChart>(b"{").is_err());
    let bytes = std::fs::read(fixture_dir("vocal-chart", "valid").join("minimal.json")).unwrap();
    let chart: VocalChart = serde_json::from_slice(&bytes).unwrap();

    let mut duplicate_track = chart.clone();
    duplicate_track.tracks.push(chart.tracks[0].clone());
    assert!(
        duplicate_track
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate track id")
    );

    let mut duplicate_phrase = chart.clone();
    let phrase = duplicate_phrase.tracks[0].phrases[0].clone();
    duplicate_phrase.tracks[0].phrases.push(phrase);
    assert!(
        duplicate_phrase
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate phrase id")
    );

    let mut duplicate_note = chart.clone();
    let note = duplicate_note.tracks[0].phrases[0].notes[0].clone();
    duplicate_note.tracks[0].phrases[0].notes.push(note);
    assert!(
        duplicate_note
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate note id")
    );

    let mut duplicate_lyric = chart;
    let lyric = duplicate_lyric.tracks[0].phrases[0].notes[0].lyrics[0].clone();
    duplicate_lyric.tracks[0].phrases[0].notes[0]
        .lyrics
        .push(lyric);
    assert!(
        duplicate_lyric
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate lyric token id")
    );
}
