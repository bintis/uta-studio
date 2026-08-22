//! Artifact Inventory: revisions, content signatures, and legacy import.
//!
//! Phase 2 of the analysis DAG redesign (docs/analysis-dag-redesign.md §9).
//! Upgrades "the cache file exists" into a queryable, provenance-carrying
//! `ArtifactRevision` per song/kind, persisted in `library_db`. This module
//! never deletes or rewrites a source file as a side effect of importing it
//! — legacy files are read-only inputs.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
use crate::cache::CacheDir;
use crate::library_db;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ArtifactSummary {
    pub kind: ArtifactKind,
    pub present: bool,
}

/// One concrete production of an artifact: which file, what produced it,
/// what it was built from, and whether it's the version currently in use.
/// See docs/analysis-dag-redesign.md §9 for field rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ArtifactRevision {
    /// Deterministic: `{file_hash}:{kind}:{content_hash}`. A rerun that
    /// produces byte-identical output reuses the same revision id instead
    /// of minting a new one -- this is intentional, not a collision bug.
    pub id: String,
    pub file_hash: String,
    pub kind: ArtifactKind,
    pub path: PathBuf,
    pub content_hash: String,
    pub producer_node: AnalysisNodeId,
    pub input_revisions: Vec<String>,
    pub config_hash: String,
    pub algorithm_version: String,
    pub created_at_ms: i64,
    pub byte_size: u64,
    pub active: bool,
    pub legacy: bool,
    /// Phase 6 `invalidate_analysis_artifact` / Phase 7 §7.6 "Invalidate":
    /// explicitly marked stale or wrong by a user action, independent of
    /// `active`/`legacy`. Never set by ordinary analysis runs -- only by
    /// `invalidate_artifact_revision` below.
    pub invalidated: bool,
}

/// Real, file-existence-based artifact presence for a song, independent of
/// where the pipeline's progress indicator happens to be. This is the fix
/// for the bug class docs/analysis-dag-redesign.md §2 documented in the
/// current desktop UI: "stems_ready"/"pitch_ready"/etc. there are aliases
/// of `stage_index` comparison, not real checks -- a no-stems LRC song
/// reads as "stems ready" once progress reaches 100% even though no stem
/// file exists. This checks the actual cache files. It intentionally does
/// **not** consult the Phase 2 Artifact Inventory DB yet: nothing writes a
/// live-run row to `analysis_artifacts` in real time (Phase 3 status note),
/// so a DB-only check would be blind to an in-progress run; a currently
/// live task should keep using progress-based state for the node it's
/// actively on and only trust this for nodes it has already passed.
pub fn cached_artifact_presence(cache: &CacheDir, file_hash: &str) -> Vec<ArtifactSummary> {
    // A song separated before separation was decoupled from detected
    // key/tempo has its stems on disk as `{hash}_vocals_{key}_{tempo}.ext`,
    // never the bare `{hash}_vocals.ext` `vocals_path()`/`instrumental_path()`
    // check for -- confirmed against a real analyzed song's cache directory,
    // not a hypothetical. `CacheDir::has_variant_stems` already recognizes
    // that legacy shape (same one `transcript_exists`'s `stems_exist` check
    // relies on); the bare check alone under-reports every song analyzed
    // before that change, which otherwise silently shows as missing stems
    // in the UI despite being fully separated and usable.
    let stems_present = (cache.vocals_path(file_hash).is_file()
        && cache.instrumental_path(file_hash).is_file())
        || cache.has_variant_stems(file_hash);
    vec![
        ArtifactSummary {
            kind: ArtifactKind::MusicAnalysis,
            present: cache.music_analysis_path(file_hash).is_file(),
        },
        ArtifactSummary {
            kind: ArtifactKind::VocalStem,
            present: stems_present,
        },
        ArtifactSummary {
            kind: ArtifactKind::InstrumentalStem,
            present: stems_present,
        },
        ArtifactSummary {
            kind: ArtifactKind::PitchTrack,
            present: cache.pitch_track_path(file_hash).is_file(),
        },
        ArtifactSummary {
            kind: ArtifactKind::PitchNoteCandidates,
            present: cache.pitch_notes_path(file_hash).is_file(),
        },
        ArtifactSummary {
            kind: ArtifactKind::RecognizedText,
            present: cache.recognized_text_path(file_hash).is_file(),
        },
        ArtifactSummary {
            kind: ArtifactKind::AsrSegments,
            present: cache.asr_segments_path(file_hash).is_file(),
        },
        ArtifactSummary {
            kind: ArtifactKind::TimedTranscript,
            // Prefer the dedicated §4.4 file; fall back to the
            // compatibility file for songs analyzed before it existed.
            present: cache.timed_transcript_path(file_hash).is_file()
                || cache.transcript_path(file_hash).is_file(),
        },
        ArtifactSummary {
            kind: ArtifactKind::CandidateChart,
            present: cache.candidate_chart_path(file_hash).is_file(),
        },
        ArtifactSummary {
            kind: ArtifactKind::AuthoredChart,
            present: cache.vocal_chart_path(file_hash).is_file(),
        },
    ]
}

pub fn artifact_present(summaries: &[ArtifactSummary], kind: ArtifactKind) -> bool {
    summaries.iter().any(|s| s.kind == kind && s.present)
}

/// Convenience wrapper over `cached_artifact_presence` for real callers
/// (the injectable version exists so tests never touch the real app data
/// root).
pub fn cached_artifact_presence_for_song(file_hash: &str) -> Vec<ArtifactSummary> {
    cached_artifact_presence(&CacheDir::new(), file_hash)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Same hashing convention as `song::compute_file_hash` (blake3, first 32
/// hex chars) so every content-identity hash in the app reads the same way.
pub fn hash_file_contents(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex()[..32].to_string())
}

/// Revision-specific, content-addressed storage below an authorized cache
/// root. Canonical analyzer files are mutable working materializations; a
/// revision always points at the copy committed by this store.
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    cache_root: PathBuf,
    store_root: PathBuf,
}

static ARTIFACT_STORE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl ArtifactStore {
    pub const STORAGE_VERSION: i32 = 1;

    pub fn new(cache_root: impl Into<PathBuf>) -> Result<Self, String> {
        let cache_root = cache_root.into();
        std::fs::create_dir_all(&cache_root).map_err(|e| e.to_string())?;
        let cache_root = cache_root.canonicalize().map_err(|e| e.to_string())?;
        let store_root = cache_root.join("artifact-store");
        std::fs::create_dir_all(&store_root).map_err(|e| e.to_string())?;
        Ok(Self {
            cache_root,
            store_root,
        })
    }

    pub fn root(&self) -> &Path {
        &self.store_root
    }

    /// Copies mutable cache output into an immutable destination. A hard
    /// link is intentionally not used: overwriting a canonical file in
    /// place could otherwise mutate every revision sharing its inode.
    pub fn capture(
        &self,
        file_hash: &str,
        kind: ArtifactKind,
        source: &Path,
    ) -> Result<(PathBuf, String, u64), String> {
        ensure_within_root(&self.cache_root, source)?;
        if !source.is_file() {
            return Err(format!(
                "artifact source is not a file: {}",
                source.display()
            ));
        }
        let content_hash = hash_file_contents(source).map_err(|e| e.to_string())?;
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("bin");
        let kind_dir = artifact_kind_to_str(kind).trim_matches('"').replace(
            |ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_',
            "_",
        );
        let destination_dir = self.store_root.join(file_hash).join(kind_dir);
        std::fs::create_dir_all(&destination_dir).map_err(|e| e.to_string())?;
        let destination = destination_dir.join(format!("{content_hash}.{extension}"));
        if destination.is_file() {
            let existing_hash = hash_file_contents(&destination).map_err(|e| e.to_string())?;
            if existing_hash != content_hash {
                return Err(format!(
                    "artifact store corruption at {}: expected {}, found {}",
                    destination.display(),
                    content_hash,
                    existing_hash
                ));
            }
            let byte_size = destination.metadata().map_err(|e| e.to_string())?.len();
            return Ok((destination, content_hash, byte_size));
        }

        let temp = destination_dir.join(format!(
            ".{content_hash}.{}.{}.tmp",
            std::process::id(),
            ARTIFACT_STORE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<(), String> {
            let mut input = std::fs::File::open(source).map_err(|e| e.to_string())?;
            let mut output = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
            output.sync_all().map_err(|e| e.to_string())?;
            let copied_hash = hash_file_contents(&temp).map_err(|e| e.to_string())?;
            if copied_hash != content_hash {
                return Err(format!(
                    "artifact changed while being captured: expected {content_hash}, copied {copied_hash}"
                ));
            }
            match std::fs::rename(&temp, &destination) {
                Ok(()) => Ok(()),
                Err(error) if destination.is_file() => {
                    let existing_hash =
                        hash_file_contents(&destination).map_err(|e| e.to_string())?;
                    if existing_hash == content_hash {
                        Ok(())
                    } else {
                        Err(format!("could not atomically commit artifact: {error}"))
                    }
                }
                Err(error) => Err(error.to_string()),
            }
        })();
        if temp.exists() {
            let _ = std::fs::remove_file(&temp);
        }
        result?;
        let stored_hash = hash_file_contents(&destination).map_err(|e| e.to_string())?;
        if stored_hash != content_hash {
            return Err(format!(
                "artifact store verification failed at {}",
                destination.display()
            ));
        }
        let byte_size = destination.metadata().map_err(|e| e.to_string())?.len();
        Ok((destination, content_hash, byte_size))
    }

    pub fn verify_revision(&self, revision: &ArtifactRevision) -> Result<(), String> {
        ensure_within_root(&self.store_root, &revision.path)?;
        let actual = hash_file_contents(&revision.path).map_err(|e| e.to_string())?;
        if actual == revision.content_hash {
            Ok(())
        } else {
            Err(format!(
                "artifact revision {} is corrupt: expected {}, found {}",
                revision.id, revision.content_hash, actual
            ))
        }
    }

    /// Repairs only from a byte-identical authorized canonical file. It
    /// never substitutes a newer file merely because its name matches.
    pub fn repair_revision(
        &self,
        revision: &ArtifactRevision,
        canonical: &Path,
    ) -> Result<PathBuf, String> {
        ensure_within_root(&self.cache_root, canonical)?;
        let hash = hash_file_contents(canonical).map_err(|e| e.to_string())?;
        if hash != revision.content_hash {
            return Err("canonical file does not match the missing revision hash".to_string());
        }
        let (path, _, _) = self.capture(&revision.file_hash, revision.kind, canonical)?;
        Ok(path)
    }
}

/// Generalizes the stem-separation signature pattern
/// (`pipeline.py::_cached_separator_matches`) to every node: identity is the
/// producing node, its algorithm version, its own normalized parameters,
/// the content hashes of its actual inputs, and the model in use -- and
/// nothing else. In particular this never takes a sibling artifact's value
/// (e.g. detected key/BPM) as an input, which is the property
/// docs/analysis-dag-redesign.md §8 requires generalized from the stems
/// case to every node.
pub fn compute_config_hash(
    node_id: &AnalysisNodeId,
    algorithm_version: &str,
    normalized_parameters_json: &str,
    input_content_hashes: &[&str],
    model_digest: Option<&str>,
) -> String {
    compute_native_config_hash(
        node_id,
        algorithm_version,
        normalized_parameters_json,
        input_content_hashes,
        model_digest,
        None,
    )
}

pub fn compute_native_config_hash(
    node_id: &AnalysisNodeId,
    algorithm_version: &str,
    normalized_parameters_json: &str,
    input_content_hashes: &[&str],
    model_digest: Option<&str>,
    runtime_recipe_digest: Option<&str>,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(node_id.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(algorithm_version.as_bytes());
    hasher.update(b"\0");
    hasher.update(normalized_parameters_json.as_bytes());
    hasher.update(b"\0");
    for input_hash in input_content_hashes {
        hasher.update(input_hash.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\0");
    hasher.update(model_digest.unwrap_or("").as_bytes());
    hasher.update(b"\0");
    hasher.update(runtime_recipe_digest.unwrap_or("").as_bytes());
    hasher.finalize().to_hex()[..32].to_string()
}

fn artifact_kind_to_str(kind: ArtifactKind) -> String {
    serde_json::to_string(&kind).unwrap_or_default()
}

fn artifact_kind_from_str(value: &str) -> Option<ArtifactKind> {
    serde_json::from_str(value).ok()
}

fn revision_from_row(row: library_db::AnalysisArtifactRow) -> Option<ArtifactRevision> {
    Some(ArtifactRevision {
        id: row.id,
        file_hash: row.file_hash,
        kind: artifact_kind_from_str(&row.kind)?,
        path: PathBuf::from(row.path),
        content_hash: row.content_hash,
        producer_node: AnalysisNodeId::new(row.producer_node),
        input_revisions: serde_json::from_str(&row.input_revisions).unwrap_or_default(),
        config_hash: row.config_hash,
        algorithm_version: row.algorithm_version,
        created_at_ms: row.created_at_ms,
        byte_size: row.byte_size as u64,
        active: row.active,
        legacy: row.legacy,
        invalidated: row.invalidated,
    })
}

pub(crate) fn revision_to_row(revision: &ArtifactRevision) -> library_db::AnalysisArtifactRow {
    library_db::AnalysisArtifactRow {
        id: revision.id.clone(),
        file_hash: revision.file_hash.clone(),
        kind: artifact_kind_to_str(revision.kind),
        path: revision.path.to_string_lossy().to_string(),
        content_hash: revision.content_hash.clone(),
        producer_node: revision.producer_node.as_str().to_string(),
        input_revisions: serde_json::to_string(&revision.input_revisions).unwrap_or_default(),
        config_hash: revision.config_hash.clone(),
        algorithm_version: revision.algorithm_version.clone(),
        created_at_ms: revision.created_at_ms,
        byte_size: revision.byte_size as i64,
        active: revision.active,
        legacy: revision.legacy,
        invalidated: revision.invalidated,
    }
}

/// Records a new revision as the given artifact's only or newest version.
/// Does not touch `active` for existing revisions of the same kind -- call
/// `set_active_artifact_revision` explicitly once the new output is
/// verified, so a failed/unverified run can never silently replace the
/// last good Active Revision (docs/analysis-dag-redesign.md §2.5 / phase
/// plan §2.6 test list).
pub fn record_artifact_revision(revision: &ArtifactRevision) -> Result<(), String> {
    library_db::analysis_artifact_upsert(&revision_to_row(revision)).map_err(|e| e.to_string())
}

pub fn load_analysis_artifacts(file_hash: &str) -> Vec<ArtifactRevision> {
    library_db::analysis_artifacts_for_song(file_hash)
        .unwrap_or_default()
        .into_iter()
        .filter_map(revision_from_row)
        .collect()
}

pub fn load_artifact_revisions(file_hash: &str, kind: ArtifactKind) -> Vec<ArtifactRevision> {
    library_db::analysis_artifacts_for_kind(file_hash, &artifact_kind_to_str(kind))
        .unwrap_or_default()
        .into_iter()
        .filter_map(revision_from_row)
        .collect()
}

pub fn load_active_artifact(file_hash: &str, kind: ArtifactKind) -> Option<ArtifactRevision> {
    library_db::analysis_active_artifact(file_hash, &artifact_kind_to_str(kind))
        .ok()
        .flatten()
        .and_then(revision_from_row)
}

/// Moves pre-store inventory rows onto immutable backing files without
/// deleting their canonical compatibility files. Safe to call repeatedly.
pub fn migrate_artifact_revisions_to_store(
    cache: &CacheDir,
    file_hash: &str,
) -> Result<usize, String> {
    let store = ArtifactStore::new(&cache.path)?;
    let mut migrated = 0;
    for mut revision in load_analysis_artifacts(file_hash) {
        if store.verify_revision(&revision).is_ok() {
            continue;
        }
        ensure_within_root(&cache.path, &revision.path)?;
        let source_hash = hash_file_contents(&revision.path).map_err(|e| e.to_string())?;
        if source_hash != revision.content_hash {
            return Err(format!(
                "cannot migrate revision {}: backing file hash no longer matches",
                revision.id
            ));
        }
        let (immutable_path, content_hash, byte_size) =
            store.capture(file_hash, revision.kind, &revision.path)?;
        revision.path = immutable_path;
        revision.content_hash = content_hash;
        revision.byte_size = byte_size;
        record_artifact_revision(&revision)?;
        migrated += 1;
    }
    Ok(migrated)
}

/// §7.6 "Compare revisions": which fields differ between two revisions of
/// the *same* artifact kind, and whether they're byte-identical
/// (`content_hash` equal) despite that -- a real, useful distinction a
/// naive "different config_hash means different output" assumption would
/// miss (e.g. two separator configs that happen to produce the same audio).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ArtifactRevisionComparison {
    pub revision_a: ArtifactRevision,
    pub revision_b: ArtifactRevision,
    pub same_content: bool,
    pub changed_fields: Vec<&'static str>,
}

fn artifact_revision_changed_fields(
    a: &ArtifactRevision,
    b: &ArtifactRevision,
) -> Vec<&'static str> {
    let mut changed = Vec::new();
    if a.content_hash != b.content_hash {
        changed.push("content_hash");
    }
    if a.config_hash != b.config_hash {
        changed.push("config_hash");
    }
    if a.algorithm_version != b.algorithm_version {
        changed.push("algorithm_version");
    }
    if a.producer_node != b.producer_node {
        changed.push("producer_node");
    }
    if a.byte_size != b.byte_size {
        changed.push("byte_size");
    }
    changed
}

/// Compares two revisions of the same song+kind. Rejects mismatched
/// file_hash or kind (and an unknown revision id) the same way
/// `compare_analysis_runs` rejects comparing runs from two different
/// songs -- there's no meaningful diff between unrelated artifacts.
pub fn compare_artifact_revisions(
    file_hash: &str,
    kind: ArtifactKind,
    revision_id_a: &str,
    revision_id_b: &str,
) -> Result<ArtifactRevisionComparison, String> {
    let revisions = load_artifact_revisions(file_hash, kind);
    let revision_a = revisions
        .iter()
        .find(|r| r.id == revision_id_a)
        .cloned()
        .ok_or_else(|| format!("no such artifact revision: {revision_id_a}"))?;
    let revision_b = revisions
        .iter()
        .find(|r| r.id == revision_id_b)
        .cloned()
        .ok_or_else(|| format!("no such artifact revision: {revision_id_b}"))?;
    let same_content = revision_a.content_hash == revision_b.content_hash;
    let changed_fields = artifact_revision_changed_fields(&revision_a, &revision_b);
    Ok(ArtifactRevisionComparison {
        revision_a,
        revision_b,
        same_content,
        changed_fields,
    })
}

/// Marks `revision_id` as the Active Revision for its song+kind and
/// deactivates every sibling revision of that song+kind. The revision must
/// already exist (via `record_artifact_revision`) and its `path` must
/// resolve inside `cache_root` -- this is the boundary check phase plan
/// §6.3 requires of every artifact-facing API, applied here at the point
/// a revision becomes selectable rather than trusting callers everywhere.
pub fn set_active_artifact_revision(
    cache_root: &Path,
    file_hash: &str,
    kind: ArtifactKind,
    revision_id: &str,
) -> Result<(), String> {
    let revisions = load_artifact_revisions(file_hash, kind);
    let target = revisions
        .iter()
        .find(|r| r.id == revision_id)
        .ok_or_else(|| format!("no such artifact revision: {revision_id}"))?;
    if target.invalidated {
        return Err(
            "this revision has been invalidated and cannot be set active; \
             produce a fresh revision instead"
                .to_string(),
        );
    }
    ensure_within_root(cache_root, &target.path)?;
    let cache = CacheDir {
        path: cache_root.to_path_buf(),
    };
    let staged = stage_compatibility_materializations(&cache, target)?;
    let previous = load_active_artifact(file_hash, kind).map(|revision| revision.id);
    if let Err(error) = library_db::analysis_artifact_set_active(
        file_hash,
        &artifact_kind_to_str(kind),
        revision_id,
    ) {
        for (temporary, _) in staged {
            let _ = std::fs::remove_file(temporary);
        }
        return Err(error.to_string());
    }
    for (temporary, destination) in staged {
        if let Err(error) = atomic_replace_file(&temporary, &destination) {
            if let Some(previous) = previous.as_deref() {
                let _ = library_db::analysis_artifact_set_active(
                    file_hash,
                    &artifact_kind_to_str(kind),
                    previous,
                );
            } else {
                let _ = library_db::analysis_artifact_clear_active(
                    file_hash,
                    &artifact_kind_to_str(kind),
                );
            }
            let _ = std::fs::remove_file(temporary);
            return Err(format!(
                "Active selection was rolled back because the compatibility file could not be updated: {error}"
            ));
        }
    }
    Ok(())
}

fn compatibility_paths(cache: &CacheDir, revision: &ArtifactRevision) -> Vec<PathBuf> {
    let hash = &revision.file_hash;
    match revision.kind {
        ArtifactKind::MusicAnalysis => vec![cache.music_analysis_path(hash)],
        ArtifactKind::VocalStem
        | ArtifactKind::InstrumentalStem
        | ArtifactKind::RawVocalStem
        | ArtifactKind::DenoisedVocalStem
        | ArtifactKind::DereverbedVocalStem
        | ArtifactKind::AnalysisVocalStem
        | ArtifactKind::HighQualityInstrumentalStem
        | ArtifactKind::DenoisedInstrumentalStem
        | ArtifactKind::DereverbedInstrumentalStem
        | ArtifactKind::KaraokeInstrumentalStem
        | ArtifactKind::DrumStem
        | ArtifactKind::BassStem
        | ArtifactKind::GuitarStem
        | ArtifactKind::PianoStem
        | ArtifactKind::OtherStem => {
            let role = match revision.kind {
                ArtifactKind::VocalStem | ArtifactKind::AnalysisVocalStem => "vocals",
                ArtifactKind::RawVocalStem => "vocals_raw",
                ArtifactKind::DenoisedVocalStem => "vocals_denoised",
                ArtifactKind::DereverbedVocalStem => "vocals_dry",
                ArtifactKind::HighQualityInstrumentalStem => "instrumental_hq",
                ArtifactKind::DenoisedInstrumentalStem => "instrumental_denoised",
                ArtifactKind::DereverbedInstrumentalStem => "instrumental_dry",
                ArtifactKind::KaraokeInstrumentalStem => "instrumental_karaoke",
                ArtifactKind::DrumStem => "drums",
                ArtifactKind::BassStem => "bass",
                ArtifactKind::GuitarStem => "guitar",
                ArtifactKind::PianoStem => "piano",
                ArtifactKind::OtherStem => "other",
                _ => "instrumental",
            };
            let extension = revision
                .path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("bin");
            vec![cache.path.join(format!("{hash}_{role}.{extension}"))]
        }
        ArtifactKind::PitchTrack => vec![cache.pitch_track_path(hash)],
        ArtifactKind::PitchNoteCandidates => vec![cache.pitch_notes_path(hash)],
        ArtifactKind::LyricsInput => vec![cache.lyrics_path(hash)],
        ArtifactKind::RecognizedText => vec![cache.recognized_text_path(hash)],
        ArtifactKind::AsrSegments => vec![cache.asr_segments_path(hash)],
        ArtifactKind::TimedTranscript => vec![
            cache.timed_transcript_path(hash),
            cache.transcript_path(hash),
        ],
        ArtifactKind::CandidateChart => vec![cache.candidate_chart_path(hash)],
        ArtifactKind::AuthoredChart => vec![cache.vocal_chart_path(hash)],
        ArtifactKind::SourceMedia
        | ArtifactKind::KeyAnalysis
        | ArtifactKind::RhythmAnalysis
        | ArtifactKind::AudioDescriptors
        | ArtifactKind::PreprocessedAudio
        | ArtifactKind::AudioStem
        | ArtifactKind::PitchEvidence
        | ArtifactKind::BoundaryEvidence
        | ArtifactKind::TechniqueEvidence
        | ArtifactKind::AcousticEvidence
        | ArtifactKind::CanonicalLyrics
        | ArtifactKind::TranscriptEvidence
        | ArtifactKind::AlignmentEvidence
        | ArtifactKind::EvidenceBundle
        | ArtifactKind::CandidateGraph
        | ArtifactKind::CanonicalSingingTrack
        | ArtifactKind::HumanCorrectionSet => Vec::new(),
    }
}

fn stage_compatibility_materializations(
    cache: &CacheDir,
    revision: &ArtifactRevision,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut staged = Vec::new();
    for destination in compatibility_paths(cache, revision) {
        let parent = destination
            .parent()
            .ok_or_else(|| "compatibility path has no parent".to_string())?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = parent.join(format!(
            ".active-{}-{}.tmp",
            std::process::id(),
            ARTIFACT_STORE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<(), String> {
            let mut source = std::fs::File::open(&revision.path).map_err(|e| e.to_string())?;
            let mut output = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut source, &mut output).map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
            output.sync_all().map_err(|e| e.to_string())?;
            let hash = hash_file_contents(&temporary).map_err(|e| e.to_string())?;
            if hash != revision.content_hash {
                return Err("staged Active materialization failed hash verification".to_string());
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_file(&temporary);
            for (temporary, _) in staged {
                let _ = std::fs::remove_file(temporary);
            }
            return Err(error);
        }
        staged.push((temporary, destination));
    }
    Ok(staged)
}

#[cfg(not(target_os = "windows"))]
fn atomic_replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(target_os = "windows")]
fn atomic_replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
    }
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Phase 6 `invalidate_analysis_artifact` / Phase 7 §7.6 "Invalidate": a
/// destructive-*classified* but non-deleting action -- the file and its DB
/// row both survive (unlike `delete_artifact_revision`), only marked as no
/// longer trustworthy. Also drops it from being the Active Revision (see
/// `library_db::analysis_artifact_set_invalidated`), so nothing downstream
/// keeps silently relying on output the user just said is wrong.
pub fn invalidate_artifact_revision(
    cache_root: &Path,
    file_hash: &str,
    kind: ArtifactKind,
    revision_id: &str,
) -> Result<(), String> {
    let revisions = load_artifact_revisions(file_hash, kind);
    let target = revisions
        .iter()
        .find(|r| r.id == revision_id)
        .ok_or_else(|| format!("no such artifact revision: {revision_id}"))?;
    ensure_within_root(cache_root, &target.path)?;
    library_db::analysis_artifact_set_invalidated(revision_id, true).map_err(|e| e.to_string())
}

fn ensure_within_root(root: &Path, candidate: &Path) -> Result<(), String> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let candidate_parent = candidate.parent().unwrap_or(candidate);
    let resolved = candidate_parent
        .canonicalize()
        .unwrap_or_else(|_| candidate_parent.to_path_buf());
    if resolved.starts_with(&root) {
        Ok(())
    } else {
        Err(format!(
            "artifact path {} escapes authorized cache root {}",
            candidate.display(),
            root.display()
        ))
    }
}

/// Deletes one revision's DB row and its backing file, refusing to touch
/// anything outside `cache_root`. Never deletes source media -- this
/// function only ever operates on rows this module itself created, all of
/// which point into the cache directory by construction.
pub fn delete_artifact_revision(
    cache_root: &Path,
    revision: &ArtifactRevision,
) -> Result<(), String> {
    ensure_within_root(cache_root, &revision.path)?;
    if library_db::analysis_artifact_is_pinned(&revision.id).map_err(|e| e.to_string())? {
        return Err("pinned artifact revisions must be unpinned before deletion".to_string());
    }
    if revision.active {
        return Err("the Active revision must be replaced before deletion".to_string());
    }
    let uses = library_db::analysis_artifact_usage_count(&revision.id)
        .map_err(|error| error.to_string())?;
    if uses > 0 {
        return Err(format!(
            "this revision is consumed by {uses} recorded run binding(s) and cannot be deleted"
        ));
    }
    let staged = revision.path.with_extension(format!(
        "delete-pending-{}-{}",
        std::process::id(),
        ARTIFACT_STORE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    if revision.path.is_file() {
        std::fs::rename(&revision.path, &staged).map_err(|e| e.to_string())?;
    }
    if let Err(error) = library_db::analysis_artifact_delete(&revision.id) {
        if staged.is_file() {
            let _ = std::fs::rename(&staged, &revision.path);
        }
        return Err(error.to_string());
    }
    if staged.is_file() {
        std::fs::remove_file(&staged).map_err(|error| error.to_string())?;
    }
    Ok(())
}

struct LegacyCandidate {
    kind: ArtifactKind,
    path: PathBuf,
}

fn legacy_candidates(cache: &CacheDir, hash: &str) -> Vec<LegacyCandidate> {
    vec![
        LegacyCandidate {
            kind: ArtifactKind::MusicAnalysis,
            path: cache.music_analysis_path(hash),
        },
        LegacyCandidate {
            kind: ArtifactKind::VocalStem,
            path: cache.vocals_path(hash),
        },
        LegacyCandidate {
            kind: ArtifactKind::InstrumentalStem,
            path: cache.instrumental_path(hash),
        },
        LegacyCandidate {
            kind: ArtifactKind::PitchTrack,
            path: cache.pitch_track_path(hash),
        },
        LegacyCandidate {
            kind: ArtifactKind::PitchNoteCandidates,
            path: cache.pitch_notes_path(hash),
        },
        LegacyCandidate {
            kind: ArtifactKind::LyricsInput,
            path: cache.lyrics_path(hash),
        },
        // §4.4: recognized_text/asr_segments are only ever present for
        // songs analyzed after the split shipped -- correctly absent for
        // known-lyrics/Timed-LRC/pre-split songs, so nothing gets imported
        // for those. TimedTranscript keeps pointing at the compatibility
        // `transcript.json` (not the new dedicated file): it's guaranteed
        // present for every analyzed song, old or new, and when both files
        // exist their content -- and therefore this scanner's
        // content-hash-keyed revision id -- is identical, so a second
        // candidate for the dedicated file would be redundant, not missed.
        LegacyCandidate {
            kind: ArtifactKind::RecognizedText,
            path: cache.recognized_text_path(hash),
        },
        LegacyCandidate {
            kind: ArtifactKind::AsrSegments,
            path: cache.asr_segments_path(hash),
        },
        LegacyCandidate {
            kind: ArtifactKind::TimedTranscript,
            path: cache.transcript_path(hash),
        },
        LegacyCandidate {
            kind: ArtifactKind::AuthoredChart,
            path: cache.vocal_chart_path(hash),
        },
    ]
}

/// Scans the current cache layout for `hash` and records a `legacy = true`
/// revision for every artifact kind that has a file on disk but no DB row
/// yet. Idempotent (content-hash-keyed ids), read-only toward the
/// filesystem, and never activates over an existing Active Revision for the
/// same song+kind -- a prior real run's provenance always wins over a
/// legacy guess. Phase plan §2.4.
pub fn import_legacy_artifacts(cache: &CacheDir, file_hash: &str) -> Vec<ArtifactRevision> {
    let mut imported = Vec::new();
    let Ok(store) = ArtifactStore::new(&cache.path) else {
        return imported;
    };
    for candidate in legacy_candidates(cache, file_hash) {
        if !candidate.path.is_file() {
            continue;
        }
        let source_path = candidate.path;
        let Ok((immutable_path, content_hash, byte_size)) =
            store.capture(file_hash, candidate.kind, &source_path)
        else {
            continue;
        };
        let id = format!(
            "{file_hash}:{}:{content_hash}",
            artifact_kind_to_str(candidate.kind)
        );
        if load_artifact_revisions(file_hash, candidate.kind)
            .iter()
            .any(|r| r.id == id)
        {
            continue;
        }
        let has_active = load_active_artifact(file_hash, candidate.kind).is_some();
        let revision = ArtifactRevision {
            id,
            file_hash: file_hash.to_string(),
            kind: candidate.kind,
            path: immutable_path,
            content_hash,
            producer_node: AnalysisNodeId::new("legacy.import"),
            input_revisions: Vec::new(),
            config_hash: "legacy_unknown".to_string(),
            algorithm_version: "legacy_unknown".to_string(),
            created_at_ms: now_ms(),
            byte_size,
            active: !has_active,
            legacy: true,
            invalidated: false,
        };
        if record_artifact_revision(&revision).is_ok() {
            imported.push(revision);
        }
    }
    imported
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = COUNTER.fetch_add(1, Ordering::SeqCst);
        let started_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-artifact-test-{label}-{}-{started_at}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    /// See `library_db::reconnect_for_test` -- shared across every test
    /// module in the crate that needs real SQL, so this can't race with
    /// e.g. `analysis_profile`'s DB-backed tests.
    fn isolated_test_db(label: &str) -> MutexGuard<'static, ()> {
        crate::library_db::reconnect_for_test(&temp_dir(&format!("db-{label}")))
    }

    #[test]
    fn cached_artifact_presence_reflects_real_files_not_a_guess() {
        let dir = temp_dir("presence");
        let cache = CacheDir { path: dir };
        let hash = "songPresence";
        // Deliberately a no-stems LRC-style song: transcript + chart exist,
        // but there are no stem files at all.
        std::fs::write(cache.transcript_path(hash), b"{}").unwrap();
        std::fs::write(cache.vocal_chart_path(hash), b"{}").unwrap();

        let presence = cached_artifact_presence(&cache, hash);

        // §4.4: only the compatibility `transcript.json` exists here (a
        // song analyzed before the split) -- TimedTranscript must still
        // report present via the fallback, while the two dedicated-file
        // kinds correctly report absent.
        assert!(artifact_present(&presence, ArtifactKind::TimedTranscript));
        assert!(!artifact_present(&presence, ArtifactKind::RecognizedText));
        assert!(!artifact_present(&presence, ArtifactKind::AsrSegments));
        assert!(artifact_present(&presence, ArtifactKind::AuthoredChart));
        assert!(!artifact_present(&presence, ArtifactKind::VocalStem));
        assert!(!artifact_present(&presence, ArtifactKind::InstrumentalStem));
        assert!(!artifact_present(&presence, ArtifactKind::PitchTrack));

        cache.clear_all();
    }

    #[test]
    fn cached_artifact_presence_reports_the_split_transcript_artifacts_when_present() {
        let dir = temp_dir("presence-split");
        let cache = CacheDir { path: dir };
        let hash = "songSplitPresence";
        std::fs::write(cache.recognized_text_path(hash), b"{}").unwrap();
        std::fs::write(cache.asr_segments_path(hash), b"{}").unwrap();
        std::fs::write(cache.timed_transcript_path(hash), b"{}").unwrap();

        let presence = cached_artifact_presence(&cache, hash);

        assert!(artifact_present(&presence, ArtifactKind::RecognizedText));
        assert!(artifact_present(&presence, ArtifactKind::AsrSegments));
        assert!(artifact_present(&presence, ArtifactKind::TimedTranscript));

        cache.clear_all();
    }

    /// Regression test for a real gap this session's own live-app
    /// verification caught: a song separated before separation was
    /// decoupled from detected key/tempo has its stems on disk as
    /// `{hash}_vocals_{key}_{tempo}.flac`, never the bare
    /// `{hash}_vocals.flac` the naive check looks for.
    #[test]
    fn cached_artifact_presence_recognizes_legacy_key_tempo_suffixed_stems() {
        let dir = temp_dir("presence-legacy-stems");
        let cache = CacheDir { path: dir };
        let hash = "songLegacyStems";
        std::fs::write(cache.path.join(format!("{hash}_vocals_Dm_1.0.flac")), b"x").unwrap();
        std::fs::write(
            cache.path.join(format!("{hash}_instrumental_Dm_1.0.flac")),
            b"x",
        )
        .unwrap();

        let presence = cached_artifact_presence(&cache, hash);

        assert!(artifact_present(&presence, ArtifactKind::VocalStem));
        assert!(artifact_present(&presence, ArtifactKind::InstrumentalStem));

        cache.clear_all();
    }

    #[test]
    fn cached_artifact_presence_is_empty_for_an_unanalyzed_song() {
        let dir = temp_dir("presence-empty");
        let cache = CacheDir { path: dir };
        let presence = cached_artifact_presence(&cache, "songNeverAnalyzed");
        assert!(presence.iter().all(|s| !s.present));
        cache.clear_all();
    }

    #[test]
    fn same_inputs_and_config_produce_a_stable_signature() {
        let node = AnalysisNodeId::new("stems.separate");
        let a = compute_config_hash(&node, "1", r#"{"separator":"karaoke"}"#, &["abc123"], None);
        let b = compute_config_hash(&node, "1", r#"{"separator":"karaoke"}"#, &["abc123"], None);
        assert_eq!(a, b);
    }

    #[test]
    fn different_parameters_produce_different_signatures() {
        let node = AnalysisNodeId::new("stems.separate");
        let a = compute_config_hash(&node, "1", r#"{"separator":"karaoke"}"#, &["abc123"], None);
        let b = compute_config_hash(
            &node,
            "1",
            r#"{"separator":"native_workflow"}"#,
            &["abc123"],
            None,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn artifact_store_keeps_earlier_revision_bytes_after_canonical_overwrite() {
        let dir = temp_dir("immutable-store");
        let canonical = dir.join("pitch_track.json");
        std::fs::write(&canonical, b"revision-a").unwrap();
        let store = ArtifactStore::new(&dir).unwrap();
        let (path_a, hash_a, _) = store
            .capture("songImmutable", ArtifactKind::PitchTrack, &canonical)
            .unwrap();

        std::fs::write(&canonical, b"revision-b").unwrap();
        let (path_b, hash_b, _) = store
            .capture("songImmutable", ArtifactKind::PitchTrack, &canonical)
            .unwrap();

        assert_ne!(path_a, path_b);
        assert_ne!(hash_a, hash_b);
        assert_eq!(std::fs::read(&path_a).unwrap(), b"revision-a");
        assert_eq!(hash_file_contents(&path_a).unwrap(), hash_a);
        assert_eq!(std::fs::read(&path_b).unwrap(), b"revision-b");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn artifact_store_reuses_identical_content_and_rejects_path_escape() {
        let dir = temp_dir("store-deduplicate");
        let canonical = dir.join("timed_transcript.json");
        std::fs::write(&canonical, b"same").unwrap();
        let store = ArtifactStore::new(&dir).unwrap();
        let first = store
            .capture("songSame", ArtifactKind::TimedTranscript, &canonical)
            .unwrap();
        let second = store
            .capture("songSame", ArtifactKind::TimedTranscript, &canonical)
            .unwrap();
        assert_eq!(first.0, second.0);

        let outside = temp_dir("store-outside").join("outside.json");
        std::fs::write(&outside, b"outside").unwrap();
        assert!(
            store
                .capture("songSame", ArtifactKind::TimedTranscript, &outside)
                .unwrap_err()
                .contains("escapes authorized cache root")
        );
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(outside.parent().unwrap()).unwrap();
    }

    #[test]
    fn setting_active_atomically_updates_compatibility_bytes_without_mutating_history() {
        let _guard = isolated_test_db("active-materialization");
        let dir = temp_dir("active-materialization");
        let cache = CacheDir { path: dir };
        let canonical = cache.pitch_track_path("songActiveMaterialize");
        std::fs::write(&canonical, b"revision-a").unwrap();
        let store = ArtifactStore::new(&cache.path).unwrap();
        let (path_a, hash_a, size_a) = store
            .capture(
                "songActiveMaterialize",
                ArtifactKind::PitchTrack,
                &canonical,
            )
            .unwrap();
        std::fs::write(&canonical, b"revision-b").unwrap();
        let (path_b, hash_b, size_b) = store
            .capture(
                "songActiveMaterialize",
                ArtifactKind::PitchTrack,
                &canonical,
            )
            .unwrap();
        let make_revision =
            |id: &str, path: PathBuf, hash: String, size, active| ArtifactRevision {
                id: id.to_string(),
                file_hash: "songActiveMaterialize".to_string(),
                kind: ArtifactKind::PitchTrack,
                path,
                content_hash: hash,
                producer_node: AnalysisNodeId::new("pitch.extract"),
                input_revisions: Vec::new(),
                config_hash: "test".to_string(),
                algorithm_version: "1".to_string(),
                created_at_ms: now_ms(),
                byte_size: size,
                active,
                legacy: false,
                invalidated: false,
            };
        let revision_a = make_revision("revision-a", path_a.clone(), hash_a, size_a, true);
        let revision_b = make_revision("revision-b", path_b, hash_b, size_b, false);
        record_artifact_revision(&revision_a).unwrap();
        record_artifact_revision(&revision_b).unwrap();

        set_active_artifact_revision(
            &cache.path,
            "songActiveMaterialize",
            ArtifactKind::PitchTrack,
            &revision_b.id,
        )
        .unwrap();

        assert_eq!(std::fs::read(&canonical).unwrap(), b"revision-b");
        assert_eq!(std::fs::read(path_a).unwrap(), b"revision-a");
        assert_eq!(
            load_active_artifact("songActiveMaterialize", ArtifactKind::PitchTrack)
                .unwrap()
                .id,
            revision_b.id
        );
        cache.clear_all();
    }

    #[test]
    fn stem_signature_excludes_key_and_bpm_by_construction() {
        // The signature function has no key/tempo/bpm parameter at all, so
        // a music-analysis algorithm change has nothing to plumb through
        // here -- mirrors the native-worker guarantee tested in
        // the frozen legacy cache-signature fixture.
        let node = AnalysisNodeId::new("stems.separate");
        let signature_with_context = |extra: &str| {
            compute_config_hash(
                &node,
                "1",
                &format!(r#"{{"separator":"karaoke","context":"{extra}"}}"#),
                &["abc123"],
                None,
            )
        };
        // Changing an unrelated context string (standing in for "detected
        // key changed") does change the signature only because we chose to
        // put it in the parameters JSON -- proving the caller, not this
        // function, is responsible for keeping key/bpm out of the stem
        // node's normalized_parameters. The real guarantee lives in
        // pipeline.py never constructing that JSON with key/bpm in it.
        assert_ne!(
            signature_with_context("Cmaj"),
            signature_with_context("Dmin")
        );
    }

    #[test]
    fn legacy_import_creates_revisions_without_modifying_files() {
        let _guard = isolated_test_db("legacy-import");
        let dir = temp_dir("legacy-import");
        let cache = CacheDir { path: dir };
        let hash = "songLegacy";
        std::fs::write(cache.music_analysis_path(hash), b"{\"version\":1}").unwrap();
        std::fs::write(cache.vocal_chart_path(hash), b"{\"chart\":true}").unwrap();
        let before_music = std::fs::read(cache.music_analysis_path(hash)).unwrap();
        let before_chart = std::fs::read(cache.vocal_chart_path(hash)).unwrap();

        let imported = import_legacy_artifacts(&cache, hash);

        assert!(
            imported
                .iter()
                .any(|r| r.kind == ArtifactKind::MusicAnalysis && r.legacy)
        );
        assert!(
            imported
                .iter()
                .any(|r| r.kind == ArtifactKind::AuthoredChart && r.legacy)
        );
        assert_eq!(
            std::fs::read(cache.music_analysis_path(hash)).unwrap(),
            before_music
        );
        assert_eq!(
            std::fs::read(cache.vocal_chart_path(hash)).unwrap(),
            before_chart
        );

        cache.clear_all();
    }

    #[test]
    fn legacy_import_picks_up_the_split_recognized_text_and_asr_segments_files() {
        let _guard = isolated_test_db("legacy-split");
        let dir = temp_dir("legacy-split");
        let cache = CacheDir { path: dir };
        let hash = "songLegacySplit";
        std::fs::write(cache.recognized_text_path(hash), b"{\"segments\":[]}").unwrap();
        std::fs::write(cache.asr_segments_path(hash), b"{\"segments\":[]}").unwrap();

        let imported = import_legacy_artifacts(&cache, hash);

        assert!(
            imported
                .iter()
                .any(|r| r.kind == ArtifactKind::RecognizedText && r.legacy)
        );
        assert!(
            imported
                .iter()
                .any(|r| r.kind == ArtifactKind::AsrSegments && r.legacy)
        );

        cache.clear_all();
    }

    #[test]
    fn legacy_import_is_idempotent() {
        let _guard = isolated_test_db("legacy-idempotent");
        let dir = temp_dir("legacy-idempotent");
        let cache = CacheDir { path: dir };
        let hash = "songIdempotent";
        std::fs::write(cache.music_analysis_path(hash), b"{\"version\":1}").unwrap();

        let first = import_legacy_artifacts(&cache, hash);
        let second = import_legacy_artifacts(&cache, hash);

        assert_eq!(first.len(), 1);
        assert_eq!(
            second.len(),
            0,
            "re-scanning must not duplicate an already-imported revision"
        );
        assert_eq!(load_analysis_artifacts(hash).len(), 1);

        cache.clear_all();
    }

    #[test]
    fn recording_a_new_revision_never_moves_active_until_explicitly_set() {
        let _guard = isolated_test_db("active-protection");
        let dir = temp_dir("active-protection");
        let cache = CacheDir { path: dir };
        let hash = "songActiveProtect";
        std::fs::write(cache.pitch_track_path(hash), b"old").unwrap();
        let legacy = import_legacy_artifacts(&cache, hash);
        let old_active = legacy
            .into_iter()
            .find(|r| r.kind == ArtifactKind::PitchTrack)
            .unwrap();
        assert!(old_active.active);

        let new_source = cache.path.join("new-pitch-track.json");
        std::fs::write(&new_source, b"new").unwrap();
        let (new_path, new_content_hash, new_byte_size) = ArtifactStore::new(&cache.path)
            .unwrap()
            .capture(hash, ArtifactKind::PitchTrack, &new_source)
            .unwrap();
        let new_revision = ArtifactRevision {
            id: "songActiveProtect:new".to_string(),
            file_hash: hash.to_string(),
            kind: ArtifactKind::PitchTrack,
            path: new_path,
            content_hash: new_content_hash,
            producer_node: AnalysisNodeId::new("pitch.extract"),
            input_revisions: vec![],
            config_hash: "cfg".to_string(),
            algorithm_version: "1".to_string(),
            created_at_ms: now_ms(),
            byte_size: new_byte_size,
            active: false,
            legacy: false,
            invalidated: false,
        };
        record_artifact_revision(&new_revision).unwrap();

        // Old revision must still be the Active one -- a recorded-but-
        // unverified new revision must never silently replace it.
        let still_active = load_active_artifact(hash, ArtifactKind::PitchTrack).unwrap();
        assert_eq!(still_active.id, old_active.id);

        set_active_artifact_revision(
            &cache.path.clone(),
            hash,
            ArtifactKind::PitchTrack,
            &new_revision.id,
        )
        .unwrap();
        let now_active = load_active_artifact(hash, ArtifactKind::PitchTrack).unwrap();
        assert_eq!(now_active.id, new_revision.id);

        cache.clear_all();
    }

    #[test]
    fn comparing_a_revision_to_itself_reports_no_changed_fields() {
        let _guard = isolated_test_db("compare-self");
        let dir = temp_dir("compare-self");
        let cache = CacheDir { path: dir };
        let hash = "songCompareSelf";
        std::fs::write(cache.pitch_track_path(hash), b"data").unwrap();
        let revision = import_legacy_artifacts(&cache, hash)
            .into_iter()
            .find(|r| r.kind == ArtifactKind::PitchTrack)
            .unwrap();

        let comparison =
            compare_artifact_revisions(hash, ArtifactKind::PitchTrack, &revision.id, &revision.id)
                .unwrap();
        assert!(comparison.same_content);
        assert!(comparison.changed_fields.is_empty());

        cache.clear_all();
    }

    #[test]
    fn comparing_two_different_revisions_reports_real_differences() {
        let _guard = isolated_test_db("compare-different");
        let dir = temp_dir("compare-different");
        let cache = CacheDir { path: dir };
        let hash = "songCompareDifferent";
        std::fs::write(cache.pitch_track_path(hash), b"old").unwrap();
        let old = import_legacy_artifacts(&cache, hash)
            .into_iter()
            .find(|r| r.kind == ArtifactKind::PitchTrack)
            .unwrap();

        let new_revision = ArtifactRevision {
            id: "songCompareDifferent:new".to_string(),
            file_hash: hash.to_string(),
            kind: ArtifactKind::PitchTrack,
            path: cache.pitch_track_path(hash),
            content_hash: "differentcontenthash".to_string(),
            producer_node: AnalysisNodeId::new("pitch.extract"),
            input_revisions: vec![],
            config_hash: "differentconfig".to_string(),
            algorithm_version: "2".to_string(),
            created_at_ms: now_ms(),
            byte_size: 999,
            active: false,
            legacy: false,
            invalidated: false,
        };
        record_artifact_revision(&new_revision).unwrap();

        let comparison =
            compare_artifact_revisions(hash, ArtifactKind::PitchTrack, &old.id, &new_revision.id)
                .unwrap();
        assert!(!comparison.same_content);
        assert!(comparison.changed_fields.contains(&"content_hash"));
        assert!(comparison.changed_fields.contains(&"config_hash"));
        assert!(comparison.changed_fields.contains(&"algorithm_version"));
        assert!(comparison.changed_fields.contains(&"byte_size"));

        cache.clear_all();
    }

    #[test]
    fn comparing_against_an_unknown_revision_id_is_rejected() {
        let _guard = isolated_test_db("compare-unknown");
        let dir = temp_dir("compare-unknown");
        let cache = CacheDir { path: dir };
        let hash = "songCompareUnknown";
        std::fs::write(cache.pitch_track_path(hash), b"data").unwrap();
        let revision = import_legacy_artifacts(&cache, hash)
            .into_iter()
            .find(|r| r.kind == ArtifactKind::PitchTrack)
            .unwrap();

        let result = compare_artifact_revisions(
            hash,
            ArtifactKind::PitchTrack,
            &revision.id,
            "does-not-exist",
        );
        assert!(result.is_err());

        cache.clear_all();
    }

    #[test]
    fn invalidating_the_active_revision_clears_active_and_keeps_the_file() {
        let _guard = isolated_test_db("invalidate-active");
        let dir = temp_dir("invalidate-active");
        let cache = CacheDir { path: dir };
        let hash = "songInvalidateActive";
        std::fs::write(cache.pitch_track_path(hash), b"data").unwrap();
        let revision = import_legacy_artifacts(&cache, hash)
            .into_iter()
            .find(|r| r.kind == ArtifactKind::PitchTrack)
            .unwrap();
        assert!(revision.active);

        invalidate_artifact_revision(&cache.path, hash, ArtifactKind::PitchTrack, &revision.id)
            .unwrap();

        assert!(load_active_artifact(hash, ArtifactKind::PitchTrack).is_none());
        let reloaded = load_artifact_revisions(hash, ArtifactKind::PitchTrack)
            .into_iter()
            .find(|r| r.id == revision.id)
            .unwrap();
        assert!(reloaded.invalidated);
        assert!(!reloaded.active);
        assert!(
            cache.pitch_track_path(hash).is_file(),
            "invalidate must not delete the backing file -- that's Delete's job"
        );

        cache.clear_all();
    }

    #[test]
    fn an_invalidated_revision_cannot_be_set_active() {
        let _guard = isolated_test_db("invalidate-reject-active");
        let dir = temp_dir("invalidate-reject-active");
        let cache = CacheDir { path: dir };
        let hash = "songInvalidateRejectActive";
        std::fs::write(cache.pitch_track_path(hash), b"data").unwrap();
        let revision = import_legacy_artifacts(&cache, hash)
            .into_iter()
            .find(|r| r.kind == ArtifactKind::PitchTrack)
            .unwrap();
        invalidate_artifact_revision(&cache.path, hash, ArtifactKind::PitchTrack, &revision.id)
            .unwrap();

        let result =
            set_active_artifact_revision(&cache.path, hash, ArtifactKind::PitchTrack, &revision.id);
        assert!(result.is_err());

        cache.clear_all();
    }

    #[test]
    fn invalidate_rejects_a_path_outside_the_cache_root() {
        let _guard = isolated_test_db("invalidate-path-escape");
        let dir = temp_dir("invalidate-path-escape");
        let cache = CacheDir { path: dir.clone() };
        let hash = "songInvalidateEscape";
        let outside = std::env::temp_dir().join("uta-studio-invalidate-outside.json");
        std::fs::write(&outside, b"nope").unwrap();

        let escaping = ArtifactRevision {
            id: "songInvalidateEscape:escaping".to_string(),
            file_hash: hash.to_string(),
            kind: ArtifactKind::AuthoredChart,
            path: outside.clone(),
            content_hash: "hash".to_string(),
            producer_node: AnalysisNodeId::new("legacy.import"),
            input_revisions: vec![],
            config_hash: "legacy_unknown".to_string(),
            algorithm_version: "legacy_unknown".to_string(),
            created_at_ms: now_ms(),
            byte_size: 4,
            active: false,
            legacy: true,
            invalidated: false,
        };
        record_artifact_revision(&escaping).unwrap();

        let result =
            invalidate_artifact_revision(&dir, hash, ArtifactKind::AuthoredChart, &escaping.id);
        assert!(result.is_err());

        let _ = std::fs::remove_file(&outside);
        cache.clear_all();
    }

    #[test]
    fn set_active_rejects_a_path_outside_the_cache_root() {
        let _guard = isolated_test_db("path-escape");
        let dir = temp_dir("path-escape");
        let cache = CacheDir { path: dir.clone() };
        let hash = "songEscape";
        let outside = std::env::temp_dir().join("uta-studio-outside-cache-root.json");
        std::fs::write(&outside, b"nope").unwrap();

        let escaping = ArtifactRevision {
            id: "songEscape:escaping".to_string(),
            file_hash: hash.to_string(),
            kind: ArtifactKind::AuthoredChart,
            path: outside.clone(),
            content_hash: "x".to_string(),
            producer_node: AnalysisNodeId::new("chart.build_candidate"),
            input_revisions: vec![],
            config_hash: "cfg".to_string(),
            algorithm_version: "1".to_string(),
            created_at_ms: now_ms(),
            byte_size: 4,
            active: false,
            legacy: false,
            invalidated: false,
        };
        record_artifact_revision(&escaping).unwrap();

        let result =
            set_active_artifact_revision(&dir, hash, ArtifactKind::AuthoredChart, &escaping.id);
        assert!(result.is_err());

        let _ = std::fs::remove_file(&outside);
        cache.clear_all();
    }

    /// Phase 9 §9.2 Artifact acceptance item: "Delete Revision 不删除源媒体"
    /// (deleting a revision must never delete source media). The revision
    /// system only ever records paths under the cache root in practice,
    /// but `delete_artifact_revision` must independently refuse to touch
    /// anything outside it even if a row somehow pointed elsewhere --
    /// `set_active_rejects_a_path_outside_the_cache_root` above covers the
    /// same guard for `set_active_artifact_revision`; this is delete's own
    /// path, not previously exercised directly.
    #[test]
    fn delete_rejects_a_path_outside_the_cache_root_and_leaves_it_on_disk() {
        let _guard = isolated_test_db("delete-path-escape");
        let dir = temp_dir("delete-path-escape");
        let cache = CacheDir { path: dir.clone() };
        let hash = "songDeleteEscape";
        let outside = std::env::temp_dir().join("uta-studio-outside-cache-root-delete.json");
        std::fs::write(&outside, b"do not delete me").unwrap();

        let escaping = ArtifactRevision {
            id: "songDeleteEscape:escaping".to_string(),
            file_hash: hash.to_string(),
            kind: ArtifactKind::AuthoredChart,
            path: outside.clone(),
            content_hash: "x".to_string(),
            producer_node: AnalysisNodeId::new("chart.build_candidate"),
            input_revisions: vec![],
            config_hash: "cfg".to_string(),
            algorithm_version: "1".to_string(),
            created_at_ms: now_ms(),
            byte_size: 4,
            active: false,
            legacy: false,
            invalidated: false,
        };
        record_artifact_revision(&escaping).unwrap();

        let result = delete_artifact_revision(&dir, &escaping);

        assert!(result.is_err());
        assert!(
            outside.is_file(),
            "file outside the cache root must survive a rejected delete"
        );

        let _ = std::fs::remove_file(&outside);
        cache.clear_all();
    }
}
