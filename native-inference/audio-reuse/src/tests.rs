#![cfg(unix)]
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Barrier};

fn fixture(body: &str) -> (tempfile::TempDir, PathBuf, PathBuf, Cache) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    std::fs::write(&source, b"read-only source").unwrap();
    let ffmpeg = root.path().join("ffmpeg");
    std::fs::write(
        &ffmpeg,
        format!(
            "#!/bin/sh\necho decode >> '{}'\n{body}\n",
            root.path().join("calls").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o755)).unwrap();
    let directory = root.path().join("cache");
    std::fs::create_dir(&directory).unwrap();
    let cache = Cache::new(directory);
    (root, ffmpeg, source, cache)
}
fn representation() -> Representation {
    Representation {
        rate: 16000,
        channels: 1,
        stream: StreamSelection::Automatic,
    }
}
fn collect(
    ffmpeg: &Path,
    source: &Path,
    representation: Representation,
    cache: Option<&Cache>,
) -> Result<(bool, Vec<u8>), String> {
    let mut output = Vec::new();
    let hit = stream(
        ffmpeg,
        source,
        representation,
        true,
        cache,
        &|| false,
        &mut |bytes| {
            output.extend_from_slice(bytes);
            Ok(())
        },
    )?;
    Ok((hit, output))
}

#[test]
fn concurrent_readers_share_one_actual_decoder_and_keep_the_source() {
    let (root, ffmpeg, source, cache) = fixture("printf 'exact PCM bytes'");
    let barrier = Arc::new(Barrier::new(6));
    let results = std::thread::scope(|scope| {
        let handles = (0..6)
            .map(|_| {
                let barrier = barrier.clone();
                let (ffmpeg, source, cache) = (&ffmpeg, &source, &cache);
                scope.spawn(move || {
                    barrier.wait();
                    collect(ffmpeg, source, representation(), Some(cache)).unwrap()
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|(hit, _)| !hit).count(), 1);
    assert!(results.iter().all(|(_, bytes)| bytes == b"exact PCM bytes"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(std::fs::read(&source).unwrap(), b"read-only source");
}

#[test]
fn formats_stream_selection_changed_sources_and_disabled_mode_are_separate() {
    let (root, ffmpeg, source, cache) = fixture("printf 'PCM'");
    let original = representation();
    let variants = [
        original,
        Representation {
            rate: 44100,
            ..original
        },
        Representation {
            channels: 2,
            ..original
        },
        Representation {
            stream: StreamSelection::First,
            ..original
        },
    ];
    for format in variants {
        assert!(!collect(&ffmpeg, &source, format, Some(&cache)).unwrap().0);
    }
    for format in variants {
        assert!(collect(&ffmpeg, &source, format, Some(&cache)).unwrap().0);
    }
    std::fs::write(&source, b"externally changed input").unwrap();
    assert!(!collect(&ffmpeg, &source, original, Some(&cache)).unwrap().0);
    assert!(!collect(&ffmpeg, &source, original, None).unwrap().0);
    assert!(!collect(&ffmpeg, &source, original, None).unwrap().0);
    assert_eq!(
        std::fs::read_to_string(root.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        7
    );
}

#[test]
fn producer_failure_is_published_without_retry_or_partial_read() {
    let (root, ffmpeg, source, cache) = fixture("printf partial; echo failure >&2; exit 7");
    let initial = collect(&ffmpeg, &source, representation(), Some(&cache)).unwrap_err();
    let later = collect(&ffmpeg, &source, representation(), Some(&cache)).unwrap_err();
    assert_eq!(initial, later);
    assert!(initial.contains("failure"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    for entry in std::fs::read_dir(&cache.directory).unwrap().flatten() {
        if entry.path().is_dir() {
            assert!(!entry.path().join("ready.pcm").exists());
            assert!(!entry.path().join("building.pcm").exists());
        }
    }
}

#[test]
fn cancelled_waiter_does_not_cancel_or_reassign_the_producer() {
    let (_root, ffmpeg, source, cache) = fixture("printf PCM");
    let key = key(&ffmpeg, &source, representation()).unwrap();
    let _producer = cache.claim(&key, &|| false).unwrap();
    let start = std::time::Instant::now();
    let result = cache.claim(&key, &|| start.elapsed() > Duration::from_millis(40));
    assert!(result.err().unwrap().contains("cancelled"));
}

#[test]
fn cancellation_reaps_a_decoder_even_after_it_closes_stdout() {
    let (root, ffmpeg, source, cache) =
        fixture("echo $$ > \"$(dirname \"$0\")/pid\"; exec 1>&-; sleep 30");
    let start = std::time::Instant::now();
    let error = stream(
        &ffmpeg,
        &source,
        representation(),
        true,
        Some(&cache),
        &|| start.elapsed() > Duration::from_millis(150),
        &mut |_| Ok(()),
    )
    .unwrap_err();
    assert!(error.contains("cancelled"));
    let pid = std::fs::read_to_string(root.path().join("pid")).unwrap();
    assert!(!Path::new(&format!("/proc/{}", pid.trim())).exists());
}

#[test]
fn wave_readers_use_immutable_links_and_publication_survives_source_rename() {
    let (root, ffmpeg, source, cache) = fixture("printf PCM");
    collect(&ffmpeg, &source, representation(), Some(&cache)).unwrap();
    let wave = root.path().join("initial.wav");
    std::fs::write(&wave, b"wave representation").unwrap();
    cache
        .publish_wave(&ffmpeg, &source, representation(), &wave)
        .unwrap();
    std::fs::remove_file(&wave).unwrap();
    let renamed = root.path().join("published-source");
    std::fs::rename(&source, &renamed).unwrap();
    let target = root.path().join("consumer.wav");
    assert!(
        cache
            .restore_wave(&ffmpeg, &renamed, representation(), &target)
            .unwrap()
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"wave representation");
    assert!(
        cache
            .restore_wave(&ffmpeg, &renamed, representation(), &target)
            .is_err()
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"wave representation");
}
