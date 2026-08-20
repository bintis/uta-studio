mod play_artifact_revision_tests {
    //! §7.6 "Play audio artifact". `play_artifact_revision` itself drives
    //! real playback hardware once past the existence check, which is out
    //! of scope for a unit test (see `native-audio/examples/
    //! playback_smoke_test.rs` for that level of verification) -- this just
    //! locks the one thing safe to assert without real audio output: a
    //! missing file is rejected before the player is ever touched.
    use super::{LibraryPlayback, play_artifact_revision};

    #[test]
    fn a_missing_artifact_file_is_rejected_without_touching_the_player() {
        let audio = uta_studio_audio::EditorAudioPlayer::new();
        let mut playback = LibraryPlayback::default();
        let missing =
            std::env::temp_dir().join("uta-studio-play-artifact-test-does-not-exist.flac");

        let result = play_artifact_revision(&audio, &missing, &mut playback);

        assert!(result.is_err());
        assert!(playback.file_hash.is_none());
    }
}

#[cfg(test)]
mod format_artifact_preview_tests {
    //! §7.6 "Preview".
    use super::{PREVIEW_BYTE_LIMIT, format_artifact_preview};
    use std::path::Path;

    #[test]
    fn a_short_file_is_shown_in_full_with_its_byte_count() {
        let copy = format_artifact_preview(Path::new("/cache/song_transcript.json"), b"{}");
        assert!(copy.contains("(2 bytes)"));
        assert!(copy.contains("{}"));
        assert!(!copy.contains("showing first"));
    }

    #[test]
    fn a_long_file_is_truncated_and_says_so() {
        let bytes = vec![b'x'; PREVIEW_BYTE_LIMIT + 500];
        let copy = format_artifact_preview(Path::new("/cache/song_pitch_track.json"), &bytes);
        assert!(copy.contains(&format!("({} bytes", PREVIEW_BYTE_LIMIT + 500)));
        assert!(copy.contains("showing first"));
        // The shown content itself must actually be truncated, not just the
        // label claiming it is -- count only the filler character, which
        // appears nowhere else in the surrounding label text.
        let shown_x_count = copy.matches('x').count();
        assert!(shown_x_count <= PREVIEW_BYTE_LIMIT);
        assert!(shown_x_count > 0);
    }
}

#[cfg(test)]
mod cache_path_tests {
    use super::validate_cache_path;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "uta-studio-cache-path-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn a_file_inside_the_cache_root_is_accepted() {
        let root = temp_dir("inside");
        let file = root.join("song_transcript.json");
        std::fs::write(&file, b"{}").unwrap();

        assert!(validate_cache_path(&file, &root).is_ok());
    }

    #[test]
    fn a_file_outside_the_cache_root_is_rejected() {
        let root = temp_dir("outside-root");
        let outsider_dir = temp_dir("outside-file");
        let file = outsider_dir.join("not_cache.json");
        std::fs::write(&file, b"{}").unwrap();

        assert!(validate_cache_path(&file, &root).is_err());
    }

    #[test]
    fn a_sibling_directory_that_shares_a_path_prefix_is_still_rejected() {
        // Regression guard for the classic `starts_with` string-prefix trap:
        // "/cache-evil" starts with the *string* "/cache" but is not really
        // inside it.
        let base = temp_dir("prefix-guard");
        let root = base.join("cache");
        let sibling = base.join("cache-evil");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let file = sibling.join("not_cache.json");
        std::fs::write(&file, b"{}").unwrap();

        assert!(validate_cache_path(&file, &root).is_err());
    }

    #[test]
    fn a_nonexistent_path_is_rejected_rather_than_panicking() {
        let root = temp_dir("nonexistent");
        let missing = root.join("does_not_exist.json");

        assert!(validate_cache_path(&missing, &root).is_err());
    }
}
