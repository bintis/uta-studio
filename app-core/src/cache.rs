use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export)]
pub struct CachePaths {
    pub songs: Option<PathBuf>,
    pub models: Option<PathBuf>,
    pub vendor: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct CacheDir {
    pub path: PathBuf,
}

impl Default for CacheDir {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheDir {
    pub fn new() -> Self {
        let path = songs_cache_dir();
        std::fs::create_dir_all(&path).expect("could not create cache directory");
        Self { path }
    }

    /// Non-panicking alternative to `new()`, for a best-effort caller that
    /// would rather silently do without cache-backed data than crash
    /// outright (docs/plan.md §9.4's `nix build` checkPhase note: `$HOME`
    /// is unwritable inside the Nix build sandbox, and the eager `.expect`
    /// in `new()` used to be reachable from a purely cosmetic read path --
    /// `desktop/src/studio/editor/state.rs::load_editor_beats` -- that had
    /// no real need to panic just because the beat grid couldn't load).
    pub fn try_new() -> Option<Self> {
        let path = songs_cache_dir();
        std::fs::create_dir_all(&path).ok()?;
        Some(Self { path })
    }

    pub fn transcript_path(&self, hash: &str) -> PathBuf {
        self.path.join(format!("{hash}_transcript.json"))
    }

    /// §4.4 split artifacts (docs/plan.md §4.4 /
    /// app-core/src/analysis_graph.rs's `ArtifactKind::RecognizedText` /
    /// `AsrSegments` / `TimedTranscript`). `transcript_path` above keeps
    /// being written unchanged as a permanent compatibility file -- these
    /// are additive, not a rename.
    pub fn recognized_text_path(&self, hash: &str) -> PathBuf {
        self.path.join(format!("{hash}_recognized_text.json"))
    }

    /// The `lyrics.transcribe` node's own final output (word/segment-timed,
    /// pre chart-build patching). Only written for ASR routes -- absent for
    /// known-lyrics/Timed-LRC songs, matching the DAG (`lyrics.align`/
    /// `lyrics.import_timed` never produce this kind).
    pub fn asr_segments_path(&self, hash: &str) -> PathBuf {
        self.path.join(format!("{hash}_asr_segments.json"))
    }

    /// The chart-ready transcript `chart.build_candidate` consumes,
    /// regardless of lyrics route -- written alongside (byte-identical to)
    /// `transcript_path` by every producer. Use `resolve_timed_transcript_path`
    /// to read one that works for songs analyzed before this file existed.
    pub fn timed_transcript_path(&self, hash: &str) -> PathBuf {
        self.path.join(format!("{hash}_timed_transcript.json"))
    }

    /// Prefers the dedicated `timed_transcript.json`; falls back to the
    /// combined compatibility `transcript.json` for songs analyzed before
    /// the §4.4 split existed. Chart-building/freshness readers should use
    /// this instead of `transcript_path` directly.
    pub fn resolve_timed_transcript_path(&self, hash: &str) -> PathBuf {
        let dedicated = self.timed_transcript_path(hash);
        if dedicated.is_file() {
            dedicated
        } else {
            self.transcript_path(hash)
        }
    }

    /// Authoritative UTZ 0.2 authoring document. Transcript and analyzer note
    /// files remain migration/evidence inputs, not parallel chart truth.
    pub fn vocal_chart_path(&self, hash: &str) -> PathBuf {
        self.path.join(format!("{hash}_vocal_chart.json"))
    }

    pub fn variant_transcript_path(&self, hash: &str, tempo: f64) -> PathBuf {
        self.path
            .join(format!("{hash}_transcript_{}.json", format_tempo(tempo)))
    }

    pub fn instrumental_path(&self, hash: &str) -> PathBuf {
        self.audio_stem_path(&format!("{hash}_instrumental"), hash)
    }

    pub fn vocals_path(&self, hash: &str) -> PathBuf {
        self.audio_stem_path(&format!("{hash}_vocals"), hash)
    }

    pub fn variant_instrumental_path(&self, hash: &str, key: &str, tempo: f64) -> PathBuf {
        self.audio_stem_path(
            &format!(
                "{hash}_instrumental_{}_{}",
                sanitize_key(key),
                format_tempo(tempo)
            ),
            hash,
        )
    }

    pub fn variant_instrumental_path_with_extension(
        &self,
        hash: &str,
        key: &str,
        tempo: f64,
        extension: &str,
    ) -> PathBuf {
        self.path.join(format!(
            "{hash}_instrumental_{}_{}.{}",
            sanitize_key(key),
            format_tempo(tempo),
            extension
        ))
    }

    pub fn variant_vocals_path(&self, hash: &str, key: &str, tempo: f64) -> PathBuf {
        self.audio_stem_path(
            &format!(
                "{hash}_vocals_{}_{}",
                sanitize_key(key),
                format_tempo(tempo)
            ),
            hash,
        )
    }

    /// Resolve either lossless FLAC or compact MP3 cache assets. Existing
    /// files always win; new variants inherit FLAC when this song already has
    /// lossless stems, otherwise MP3 remains the compact fallback.
    fn audio_stem_path(&self, basename: &str, hash: &str) -> PathBuf {
        for extension in ["flac", "mp3"] {
            let candidate = self.path.join(format!("{basename}.{extension}"));
            if candidate.is_file() {
                return candidate;
            }
        }
        let lossless_prefixes = [format!("{hash}_instrumental"), format!("{hash}_vocals")];
        let has_lossless_stem = std::fs::read_dir(&self.path)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .any(|name| {
                name.ends_with(".flac")
                    && lossless_prefixes
                        .iter()
                        .any(|prefix| name.starts_with(prefix))
            });
        self.path.join(format!(
            "{basename}.{}",
            if has_lossless_stem { "flac" } else { "mp3" }
        ))
    }

    fn stems_exist(&self, hash: &str) -> bool {
        (self.instrumental_path(hash).is_file() && self.vocals_path(hash).is_file())
            || self.has_variant_stems(hash)
    }

    pub fn has_variant_stems(&self, hash: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(&self.path) else {
            return false;
        };

        let inst_prefix = format!("{hash}_instrumental_");
        let voc_prefix = format!("{hash}_vocals_");
        let mut inst_suffixes = std::collections::HashSet::new();
        let mut voc_suffixes = std::collections::HashSet::new();

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(suffix) = stem_suffix(&name, &inst_prefix) {
                inst_suffixes.insert(suffix.to_string());
            } else if let Some(suffix) = stem_suffix(&name, &voc_prefix) {
                voc_suffixes.insert(suffix.to_string());
            }
        }

        inst_suffixes.iter().any(|s| voc_suffixes.contains(s))
    }

    pub fn lyrics_path(&self, hash: &str) -> PathBuf {
        self.path.join(format!("{hash}_lyrics.json"))
    }

    /// Frame-level pitch evidence. Kept separate from the transcript so a
    /// pitch rebuild never invalidates lyrics.
    pub fn pitch_track_path(&self, hash: &str) -> PathBuf {
        self.path.join(format!("{hash}_pitch_track.json"))
    }

    /// Segmented, editable note data derived from the pitch track.
    pub fn pitch_notes_path(&self, hash: &str) -> PathBuf {
        self.path.join(format!("{hash}_pitch_notes.json"))
    }

    pub fn cover_path(&self, hash: &str) -> PathBuf {
        self.path.join(format!("{hash}_cover.jpg"))
    }

    /// Structured key/rhythm analysis (tonic/scale/confidence, BPM,
    /// per-beat timestamps, and — when Essentia is installed — a few extra
    /// descriptors), written once during analysis by `analyze_music` in
    /// `pipeline.py`. Kept out of the transcript so the beat-timestamp
    /// array never has to round-trip through it.
    pub fn music_analysis_path(&self, hash: &str) -> PathBuf {
        self.path.join(format!("{hash}_music_analysis.json"))
    }

    pub fn transcript_exists(&self, hash: &str) -> bool {
        let transcript = self.transcript_path(hash);
        if !transcript.is_file() {
            return false;
        }
        // A transcript built from provided LRC without stem separation is
        // complete on its own; stems are only required for the normal pipeline.
        self.stems_exist(hash) || transcript_marks_no_stems(&transcript)
    }

    /// Explicit, user-confirmed full wipe (the "Delete cache" action) --
    /// includes the Authored Chart. Automatic reanalysis resets must use
    /// `delete_analysis_outputs_keep_chart` instead; see that method's docs.
    pub fn delete_song_cache(&self, hash: &str) {
        self.delete_analysis_outputs_keep_chart(hash);
        self.invalidate_authored_chart(hash);
    }

    /// Same file set as `delete_song_cache`, minus the Authored Chart.
    /// The "Reanalyze all" action regenerates every analysis artifact but
    /// must not discard chart edits by default
    /// (docs/analysis-dag-redesign.md §6/Phase 5) -- callers that really do
    /// want the chart gone too (the explicit, confirmed "Delete cache"
    /// action) keep using `delete_song_cache` directly.
    pub fn delete_analysis_outputs_keep_chart(&self, hash: &str) {
        for path in self.analysis_output_paths_keep_chart(hash) {
            if path.is_file() {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// Enumerates (without touching) exactly the paths
    /// `delete_analysis_outputs_keep_chart` would remove -- a real,
    /// filesystem-backed "what would this delete" query, not a guess,
    /// so a caller can back each one up first instead of deleting blind
    /// (docs/analysis-dag-redesign.md Phase 5 status note on
    /// `realign`/`reanalyze_full`'s eager-delete gap). Only paths that
    /// currently exist are returned.
    pub fn analysis_output_paths_keep_chart(&self, hash: &str) -> Vec<PathBuf> {
        let direct_audio = ["instrumental", "vocals"].into_iter().flat_map(|stem| {
            ["flac", "mp3"]
                .into_iter()
                .map(move |extension| self.path.join(format!("{hash}_{stem}.{extension}")))
        });
        let mut paths: Vec<PathBuf> = [
            self.transcript_path(hash),
            self.recognized_text_path(hash),
            self.asr_segments_path(hash),
            self.timed_transcript_path(hash),
            self.lyrics_path(hash),
            self.pitch_track_path(hash),
            self.pitch_notes_path(hash),
            self.music_analysis_path(hash),
        ]
        .into_iter()
        .chain(direct_audio)
        .filter(|path| path.is_file())
        .collect();

        if let Ok(entries) = std::fs::read_dir(&self.path) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if name.starts_with(&format!("{hash}_instrumental_"))
                    || name.starts_with(&format!("{hash}_vocals_"))
                    || name.starts_with(&format!("{hash}_editor_"))
                    || is_variant_transcript_file(name, hash)
                {
                    paths.push(path);
                }
            }
        }
        paths
    }

    pub fn editor_preview_path(&self, hash: &str, source: &str, extension: &str) -> PathBuf {
        self.path
            .join(format!("{hash}_editor_{source}.{extension}"))
    }

    /// Drops the authored chart so the next load rebuilds it from analyzer
    /// output. Call this whenever transcription, alignment, or pitch analysis
    /// is about to replace what the chart was authored from; otherwise the
    /// stale chart would hide the new analysis.
    pub fn invalidate_authored_chart(&self, hash: &str) {
        let path = self.vocal_chart_path(hash);
        if path.is_file() {
            let _ = std::fs::remove_file(&path);
        }
    }

    pub fn delete_transcript_variants(&self, hash: &str) {
        for path in self.transcript_variant_paths(hash) {
            let _ = std::fs::remove_file(&path);
        }
    }

    /// Enumerates (without touching) exactly the paths
    /// `delete_transcript_variants` would remove. See
    /// `analysis_output_paths_keep_chart`'s doc comment for why this exists.
    pub fn transcript_variant_paths(&self, hash: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.path) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if is_variant_transcript_file(name, hash) {
                    paths.push(path);
                }
            }
        }
        paths
    }

    pub fn clear_all(&self) {
        if self.path.is_dir() {
            let _ = std::fs::remove_dir_all(&self.path);
            let _ = std::fs::create_dir_all(&self.path);
        }
    }
}

// Phase 0 regression baseline for the analysis DAG redesign
// (docs/analysis-dag-redesign.md §13). These lock down current,
// intentional cache-invalidation behavior so later phases can be verified
// against a known-good baseline instead of guessing what "still works"
// means from the UI.
#[cfg(test)]
mod invalidation_tests {
    use super::CacheDir;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_cache_dir() -> CacheDir {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-cache-invalidation-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp cache dir");
        CacheDir { path }
    }

    #[test]
    fn try_new_never_panics_and_matches_new_when_it_succeeds() {
        // The real regression this guards: `try_new` must not panic where
        // `new` might -- only the failure mode differs (`None` instead of a
        // panic when the directory truly can't be created, e.g. `$HOME`
        // unwritable inside a `nix build` sandbox -- docs/plan.md §9.4).
        // Whether it *can* succeed depends on the ambient environment this
        // test doesn't control (a previous version of this test asserted
        // unconditional success and broke exactly the sandboxed case the
        // fix exists for -- `nix build`'s checkPhase runs with an
        // unwritable `$HOME`, so `try_new()` correctly returns `None`
        // there), so this asserts the implication, not "always `Some`".
        if let Some(via_try_new) = CacheDir::try_new() {
            assert_eq!(via_try_new.path, CacheDir::new().path);
        }
    }

    fn touch(path: &std::path::Path) {
        std::fs::write(path, b"{}").expect("write fixture file");
    }

    /// Every artifact file for `hash` that a fully-analyzed song would have,
    /// so tests can assert exactly which ones a given operation removes.
    fn seed_full_song(cache: &CacheDir, hash: &str) {
        touch(&cache.vocal_chart_path(hash));
        touch(&cache.transcript_path(hash));
        touch(&cache.variant_transcript_path(hash, 1.2));
        touch(&cache.lyrics_path(hash));
        touch(&cache.pitch_track_path(hash));
        touch(&cache.pitch_notes_path(hash));
        touch(&cache.music_analysis_path(hash));
        touch(&cache.vocals_path(hash));
        touch(&cache.instrumental_path(hash));
    }

    #[test]
    fn invalidate_authored_chart_only_removes_the_chart() {
        // Baseline for docs/analysis-dag-redesign.md §6: today this call is
        // unconditional and is the *only* file it touches. Phase 5 changes
        // when this gets called (via ChartUpdatePolicy), not what it deletes
        // when it is called — so this narrow-scope guarantee must hold
        // before and after that phase.
        let cache = temp_cache_dir();
        let hash = "songA";
        seed_full_song(&cache, hash);

        cache.invalidate_authored_chart(hash);

        assert!(!cache.vocal_chart_path(hash).is_file());
        assert!(cache.transcript_path(hash).is_file());
        assert!(cache.variant_transcript_path(hash, 1.2).is_file());
        assert!(cache.lyrics_path(hash).is_file());
        assert!(cache.pitch_track_path(hash).is_file());
        assert!(cache.pitch_notes_path(hash).is_file());
        assert!(cache.music_analysis_path(hash).is_file());
        assert!(cache.vocals_path(hash).is_file());
        assert!(cache.instrumental_path(hash).is_file());

        cache.clear_all();
    }

    #[test]
    fn invalidate_authored_chart_is_unconditional_today() {
        // Named baseline documenting current behavior explicitly (see
        // docs/analysis-dag-redesign.md §6 and §13): there is no "was this
        // chart actually stale" check. Calling this on an up-to-date,
        // freshly-authored chart still deletes it. When Phase 5 introduces
        // ChartUpdatePolicy::CreateCandidate, this test's assertion should
        // flip in the same PR that changes the call sites, making the
        // policy change visible in the diff instead of silent.
        let cache = temp_cache_dir();
        let hash = "songB";
        touch(&cache.vocal_chart_path(hash));

        cache.invalidate_authored_chart(hash);

        assert!(!cache.vocal_chart_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn resolve_timed_transcript_path_falls_back_to_the_compatibility_file_when_absent() {
        let cache = temp_cache_dir();
        let hash = "songResolveFallback";
        touch(&cache.transcript_path(hash));

        assert_eq!(
            cache.resolve_timed_transcript_path(hash),
            cache.transcript_path(hash)
        );
        cache.clear_all();
    }

    #[test]
    fn resolve_timed_transcript_path_prefers_the_dedicated_file_when_present() {
        let cache = temp_cache_dir();
        let hash = "songResolveDedicated";
        touch(&cache.transcript_path(hash));
        touch(&cache.timed_transcript_path(hash));

        assert_eq!(
            cache.resolve_timed_transcript_path(hash),
            cache.timed_transcript_path(hash)
        );
        cache.clear_all();
    }

    #[test]
    fn analysis_output_paths_keep_chart_includes_the_split_transcript_files_when_present() {
        let cache = temp_cache_dir();
        let hash = "songSplitEnum";
        seed_full_song(&cache, hash);
        touch(&cache.recognized_text_path(hash));
        touch(&cache.asr_segments_path(hash));
        touch(&cache.timed_transcript_path(hash));

        let enumerated = cache.analysis_output_paths_keep_chart(hash);
        assert!(enumerated.contains(&cache.recognized_text_path(hash)));
        assert!(enumerated.contains(&cache.asr_segments_path(hash)));
        assert!(enumerated.contains(&cache.timed_transcript_path(hash)));
        assert!(!enumerated.contains(&cache.vocal_chart_path(hash)));

        cache.delete_analysis_outputs_keep_chart(hash);
        assert!(!cache.recognized_text_path(hash).is_file());
        assert!(!cache.asr_segments_path(hash).is_file());
        assert!(!cache.timed_transcript_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn delete_transcript_variants_leaves_base_transcript_and_other_artifacts() {
        let cache = temp_cache_dir();
        let hash = "songC";
        seed_full_song(&cache, hash);

        cache.delete_transcript_variants(hash);

        assert!(!cache.variant_transcript_path(hash, 1.2).is_file());
        assert!(cache.transcript_path(hash).is_file());
        assert!(cache.vocal_chart_path(hash).is_file());
        assert!(cache.music_analysis_path(hash).is_file());
        assert!(cache.vocals_path(hash).is_file());
        assert!(cache.instrumental_path(hash).is_file());

        cache.clear_all();
    }

    // Phase 5 status note (docs/analysis-dag-redesign.md, `realign`/
    // `reanalyze_full`'s eager-delete gap): these two enumerators exist so
    // `app-core/src/analyzer.rs` can back a file up *before* deleting it
    // instead of deleting blind. Real regression protection for that use
    // case is "the enumerator returns exactly what the matching delete
    // removes" -- checked directly here rather than trusted by construction.

    #[test]
    fn transcript_variant_paths_matches_what_delete_transcript_variants_removes() {
        let cache = temp_cache_dir();
        let hash = "songVariantEnum";
        seed_full_song(&cache, hash);

        let enumerated = cache.transcript_variant_paths(hash);
        assert_eq!(enumerated, vec![cache.variant_transcript_path(hash, 1.2)]);

        cache.delete_transcript_variants(hash);
        for path in &enumerated {
            assert!(!path.is_file());
        }
        cache.clear_all();
    }

    #[test]
    fn analysis_output_paths_keep_chart_matches_what_the_delete_removes_and_excludes_the_chart() {
        let cache = temp_cache_dir();
        let hash = "songAnalysisOutputEnum";
        seed_full_song(&cache, hash);

        let enumerated = cache.analysis_output_paths_keep_chart(hash);
        assert!(!enumerated.contains(&cache.vocal_chart_path(hash)));
        assert!(enumerated.contains(&cache.transcript_path(hash)));
        assert!(enumerated.contains(&cache.variant_transcript_path(hash, 1.2)));
        assert!(enumerated.contains(&cache.lyrics_path(hash)));
        assert!(enumerated.contains(&cache.pitch_track_path(hash)));
        assert!(enumerated.contains(&cache.pitch_notes_path(hash)));
        assert!(enumerated.contains(&cache.music_analysis_path(hash)));
        assert!(enumerated.contains(&cache.vocals_path(hash)));
        assert!(enumerated.contains(&cache.instrumental_path(hash)));

        cache.delete_analysis_outputs_keep_chart(hash);
        for path in &enumerated {
            assert!(!path.is_file());
        }
        assert!(cache.vocal_chart_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn analysis_output_paths_keep_chart_only_returns_files_that_exist() {
        let cache = temp_cache_dir();
        let hash = "songAnalysisOutputEnumEmpty";
        assert!(cache.analysis_output_paths_keep_chart(hash).is_empty());
        cache.clear_all();
    }

    #[test]
    fn delete_song_cache_does_not_touch_a_different_song_in_the_same_directory() {
        // Dependency-isolation guarantee behind docs/analysis-dag-redesign.md
        // §5's claim that stems.separate and music.analysis are independent
        // per-song: a delete for one hash must never reach into another
        // hash's files even though they share one flat cache directory.
        let cache = temp_cache_dir();
        let target = "songD";
        let other = "songE";
        seed_full_song(&cache, target);
        seed_full_song(&cache, other);

        cache.delete_song_cache(target);

        assert!(!cache.vocal_chart_path(target).is_file());
        assert!(!cache.transcript_path(target).is_file());
        assert!(!cache.music_analysis_path(target).is_file());
        assert!(!cache.vocals_path(target).is_file());
        assert!(!cache.instrumental_path(target).is_file());

        assert!(cache.vocal_chart_path(other).is_file());
        assert!(cache.transcript_path(other).is_file());
        assert!(cache.variant_transcript_path(other, 1.2).is_file());
        assert!(cache.music_analysis_path(other).is_file());
        assert!(cache.vocals_path(other).is_file());
        assert!(cache.instrumental_path(other).is_file());

        cache.clear_all();
    }
}

fn stem_suffix<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let tail = name.strip_prefix(prefix)?;
    tail.strip_suffix(".flac")
        .or_else(|| tail.strip_suffix(".mp3"))
}

/// True when a transcript file was built from provided LRC without stem
/// separation (`"no_stems": true`), meaning it is playable without stems.
fn transcript_marks_no_stems(path: &Path) -> bool {
    #[derive(serde::Deserialize)]
    struct NoStemsProbe {
        #[serde(default)]
        no_stems: bool,
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str::<NoStemsProbe>(&data).ok())
        .map(|probe| probe.no_stems)
        .unwrap_or(false)
}

fn is_variant_transcript_file(name: &str, hash: &str) -> bool {
    name.starts_with(&format!("{hash}_transcript_")) && name.ends_with(".json")
}

pub fn sanitize_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for ch in key.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '#' || ch == 'b' {
            out.push(ch);
        } else if ch == ' ' || ch == '-' || ch == '_' {
            out.push('_');
        }
    }
    let cleaned = out.trim_matches('_').replace("__", "_");
    if cleaned.is_empty() {
        "Unknown".to_string()
    } else {
        cleaned
    }
}

pub fn normalize_tempo(tempo: f64) -> f64 {
    if !tempo.is_finite() || tempo <= 0.0 {
        1.0
    } else {
        (tempo * 10.0).round() / 10.0
    }
}

pub fn format_tempo(tempo: f64) -> String {
    format!("{:.1}", normalize_tempo(tempo))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, TS)]
#[ts(export)]
pub struct CacheStats {
    pub songs_bytes: u64,
    pub models_bytes: u64,
    pub other_bytes: u64,
}

impl CacheStats {
    pub fn calculate() -> Self {
        std::panic::catch_unwind(|| {
            let base = uta_studio_dir();

            let songs_bytes = dir_size(&songs_cache_dir());
            let models_bytes = dir_size(&models_dir());
            let other_bytes = dir_size(&vendor_dir())
                + dir_size(&base.join("videos"))
                + dir_size(&base.join("sounds"))
                + default_uta_studio_dir()
                    .join("uta-studio.log")
                    .metadata()
                    .map(|m| m.len())
                    .unwrap_or(0)
                + config_path().metadata().map(|m| m.len()).unwrap_or(0);

            Self {
                songs_bytes,
                models_bytes,
                other_bytes,
            }
        })
        .unwrap_or_default()
    }
}

pub fn uta_studio_dir() -> PathBuf {
    configured_data_path().unwrap_or_else(default_uta_studio_dir)
}

pub fn default_uta_studio_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("UTA_STUDIO_DATA_PATH") {
        let p = PathBuf::from(path);

        if !p.as_os_str().is_empty() {
            return p;
        }
    }

    dirs::home_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir()))
        .join(".uta-studio")
}

pub fn config_path() -> PathBuf {
    default_uta_studio_dir().join("config.json")
}

pub fn songs_cache_dir() -> PathBuf {
    configured_cache_paths()
        .songs
        .unwrap_or_else(|| uta_studio_dir().join("cache"))
}

pub fn models_dir() -> PathBuf {
    configured_cache_paths()
        .models
        .unwrap_or_else(|| uta_studio_dir().join("models"))
}

pub fn vendor_dir() -> PathBuf {
    configured_cache_paths()
        .vendor
        .unwrap_or_else(|| uta_studio_dir().join("vendor"))
}

pub fn cache_roots() -> Vec<PathBuf> {
    vec![songs_cache_dir(), models_dir(), vendor_dir()]
}

pub fn dir_size(path: &Path) -> u64 {
    if !path.is_dir() {
        return 0;
    }

    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

pub fn clear_models() {
    let dir = models_dir();

    if dir.is_dir() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[derive(Debug, Deserialize)]
struct DataPathOnlyConfig {
    data_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct PathsOnlyConfig {
    cache_paths: Option<CachePaths>,
}

fn resolve_configured_path(path: PathBuf) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }

    if path.is_absolute() {
        Some(path)
    } else {
        std::env::current_dir().ok().map(|cwd| cwd.join(path))
    }
}

fn configured_data_path() -> Option<PathBuf> {
    let path = config_path();
    let content = std::fs::read_to_string(path).ok()?;
    let configured = serde_json::from_str::<DataPathOnlyConfig>(&content)
        .ok()
        .and_then(|cfg| cfg.data_path)?;
    resolve_configured_path(configured)
}

fn configured_cache_paths() -> CachePaths {
    let path = config_path();
    let Some(content) = std::fs::read_to_string(path).ok() else {
        return CachePaths::default();
    };
    let Some(paths) = serde_json::from_str::<PathsOnlyConfig>(&content)
        .ok()
        .and_then(|cfg| cfg.cache_paths)
    else {
        return CachePaths::default();
    };

    CachePaths {
        songs: paths.songs.and_then(resolve_configured_path),
        models: paths.models.and_then(resolve_configured_path),
        vendor: paths.vendor.and_then(resolve_configured_path),
    }
}

pub fn normalized_target_path(path: PathBuf) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("data_path cannot be empty".to_string());
    }

    if path.is_absolute() {
        Ok(path)
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|e| format!("failed to resolve relative data_path: {e}"))
    }
}

pub fn same_path(lhs: &Path, rhs: &Path) -> bool {
    match (
        std::fs::canonicalize(lhs).ok(),
        std::fs::canonicalize(rhs).ok(),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => lhs == rhs,
    }
}

fn copy_path_entry(src: &Path, dst: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(src)
        .map_err(|e| format!("failed reading metadata for {:?}: {e}", src))?;
    let file_type = metadata.file_type();

    if file_type.is_dir() {
        std::fs::create_dir_all(dst)
            .map_err(|e| format!("failed creating destination directory {:?}: {e}", dst))?;
        for child in std::fs::read_dir(src)
            .map_err(|e| format!("failed reading directory {:?}: {e}", src))?
        {
            let child = child.map_err(|e| format!("failed reading directory entry: {e}"))?;
            let child_src = child.path();
            let child_dst = dst.join(child.file_name());
            copy_path_entry(&child_src, &child_dst)?;
        }
        return Ok(());
    }

    if file_type.is_symlink() {
        if dst.exists() {
            if dst.is_dir() {
                std::fs::remove_dir_all(dst)
                    .map_err(|e| format!("failed clearing destination {:?}: {e}", dst))?;
            } else {
                std::fs::remove_file(dst)
                    .map_err(|e| format!("failed clearing destination {:?}: {e}", dst))?;
            }
        } else if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed creating destination parent {:?}: {e}", parent))?;
        }

        let link_target = std::fs::read_link(src)
            .map_err(|e| format!("failed reading symlink {:?}: {e}", src))?;
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&link_target, dst)
                .map_err(|e| format!("failed creating symlink {:?}: {e}", dst))?;
        }
        #[cfg(windows)]
        {
            let target_is_dir = src.is_dir();
            if target_is_dir {
                std::os::windows::fs::symlink_dir(&link_target, dst)
                    .map_err(|e| format!("failed creating symlink dir {:?}: {e}", dst))?;
            } else {
                std::os::windows::fs::symlink_file(&link_target, dst)
                    .map_err(|e| format!("failed creating symlink file {:?}: {e}", dst))?;
            }
        }
        return Ok(());
    }

    if dst.exists() {
        if dst.is_dir() {
            std::fs::remove_dir_all(dst)
                .map_err(|e| format!("failed clearing destination {:?}: {e}", dst))?;
        } else {
            std::fs::remove_file(dst)
                .map_err(|e| format!("failed clearing destination {:?}: {e}", dst))?;
        }
    } else if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed creating destination parent {:?}: {e}", parent))?;
    }

    std::fs::copy(src, dst).map_err(|e| format!("failed copying {:?} -> {:?}: {e}", src, dst))?;
    Ok(())
}

fn copy_data_entries_with<F>(
    source_root: &Path,
    destination_root: &Path,
    copy_entry: F,
) -> Result<Vec<std::ffi::OsString>, String>
where
    F: Fn(&Path, &Path) -> Result<(), String>,
{
    let mut copied = Vec::new();
    if !source_root.is_dir() {
        return Ok(copied);
    }

    for entry in std::fs::read_dir(source_root)
        .map_err(|e| format!("failed reading current data path {:?}: {e}", source_root))?
    {
        let entry = entry.map_err(|e| format!("failed reading data path entry: {e}"))?;
        let name = entry.file_name();
        let entry_name = name.to_string_lossy();
        if entry_name == "config.json" || entry_name == "uta-studio.log" {
            continue;
        }

        let src = entry.path();
        let dst = destination_root.join(&name);
        copy_entry(&src, &dst)?;
        copied.push(name);
    }

    Ok(copied)
}

fn cleanup_source_entries(source_root: &Path, copied: &[std::ffi::OsString]) {
    for name in copied {
        let src = source_root.join(name);
        if src.is_dir() {
            let _ = std::fs::remove_dir_all(&src);
        } else if src.exists() {
            let _ = std::fs::remove_file(&src);
        }
    }
}

pub fn relocate_directory_contents(
    source_root: &Path,
    destination_root: &Path,
) -> Result<(), String> {
    if same_path(source_root, destination_root) || !source_root.is_dir() {
        std::fs::create_dir_all(destination_root)
            .map_err(|e| format!("failed creating cache path {:?}: {e}", destination_root))?;
        return Ok(());
    }

    if destination_root.starts_with(source_root) {
        return Err("new cache path cannot be inside current cache path".to_string());
    }

    std::fs::create_dir_all(destination_root)
        .map_err(|e| format!("failed creating cache path {:?}: {e}", destination_root))?;
    let copied = copy_data_entries_with(source_root, destination_root, |src, dst| {
        copy_path_entry(src, dst)
    })?;
    cleanup_source_entries(source_root, &copied);

    Ok(())
}

pub fn relocate_app_data_path(new_path: PathBuf) -> Result<PathBuf, String> {
    let source_root = uta_studio_dir();
    let destination_root = normalized_target_path(new_path)?;

    if same_path(&source_root, &destination_root) {
        let default_root = default_uta_studio_dir();
        if !same_path(&destination_root, &default_root) {
            crate::library_db::rebase_song_album_art_paths(&default_root, &destination_root)?;
        }
        let mut cfg = crate::config::AppConfig::load();
        cfg.data_path = Some(destination_root.clone());
        cfg.save()?;
        crate::library_db::reconnect_library_at_root(&destination_root)?;
        return Ok(destination_root);
    }

    if destination_root.starts_with(&source_root) {
        return Err("new data_path cannot be inside current data path".to_string());
    }

    std::fs::create_dir_all(&destination_root)
        .map_err(|e| format!("failed creating new data path {:?}: {e}", destination_root))?;
    let copied = copy_data_entries_with(&source_root, &destination_root, |src, dst| {
        copy_path_entry(src, dst)
    })?;

    crate::library_db::rebase_song_album_art_paths(&source_root, &destination_root)?;
    crate::library_db::reconnect_library_at_root(&destination_root)?;

    let mut cfg = crate::config::AppConfig::load();
    cfg.data_path = Some(destination_root.clone());
    cfg.save()?;
    cleanup_source_entries(&source_root, &copied);

    Ok(destination_root)
}
