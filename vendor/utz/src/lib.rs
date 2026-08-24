//! Independent `.utz` package model and safe ZIP reader/writer.

use std::{
    collections::{BTreeMap, HashSet},
    io::{Cursor, Read, Seek, Write},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const FORMAT_ID: &str = "uta.song";
pub const FORMAT_VERSION: &str = "0.3.0";
pub const LEGACY_FORMAT_VERSION: &str = "0.1.0";
pub const VOCAL_CHART_FORMAT: &str = "uta.vocal-chart";
pub const VOCAL_CHART_VERSION: &str = "1.1.0";
pub const VOCAL_CHART_MEDIA_TYPE: &str = "application/vnd.uta.vocal-chart+json;version=1";
pub const PITCH_EVIDENCE_FORMAT: &str = "uta.pitch-evidence";
pub const PITCH_EVIDENCE_VERSION: &str = "1.0.0";
pub const PITCH_EVIDENCE_MEDIA_TYPE: &str = "application/vnd.uta.pitch-evidence+json;version=1";
pub const DEFAULT_TIMEBASE: u64 = 1_000_000;
pub const MAX_EXACT_INTEGER: u64 = 9_007_199_254_740_991;
pub const MANIFEST_PATH: &str = "manifest.json";
pub const MAX_FILES: usize = 128;
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_DUET_PARTS: u32 = 9;
pub const MAX_ID_BYTES: usize = 64;

#[derive(Debug, Error)]
pub enum UtzError {
    #[error("invalid ZIP package: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("could not read or write package: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid manifest or chart JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid .utz package: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, UtzError>;

/// A package asset that can be copied without loading large media into RAM.
#[derive(Debug, Clone)]
pub enum AssetSource {
    Bytes(Vec<u8>),
    File(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetRef {
    pub path: String,
    pub media_type: String,
    pub sha256: String,
    pub bytes: u64,
}

impl AssetRef {
    pub fn pending(path: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            media_type: media_type.into(),
            sha256: String::new(),
            bytes: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SongMetadata {
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    pub duration_seconds: f64,
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_sort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist_sort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_start_seconds: Option<f64>,
}

/// UTZ 0.1 audio map. The chart-to-audio offset only exists in 0.1; UTZ 0.2
/// defines chart time zero as instrumental time zero instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioAssetsV01 {
    pub instrumental: AssetRef,
    #[serde(default)]
    pub guide_vocals: Option<AssetRef>,
    #[serde(default)]
    pub original: Option<AssetRef>,
    #[serde(default)]
    pub audio_offset_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioAssets {
    pub instrumental: AssetRef,
    #[serde(default)]
    pub guide_vocals: Option<AssetRef>,
    #[serde(default)]
    pub original: Option<AssetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loudness: Option<AudioLoudness>,
}

/// Advisory integrated loudness (EBU R 128 LUFS) per audio stem.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AudioLoudness {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrumental_lufs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guide_vocals_lufs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_lufs: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VisualAssets {
    #[serde(default)]
    pub cover: Option<AssetRef>,
    #[serde(default)]
    pub video: Option<AssetRef>,
    #[serde(default)]
    pub video_offset_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoringConfig {
    pub engine: String,
    pub version: u32,
    #[serde(default)]
    pub octave_tolerance: bool,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            engine: "uta.pitch".into(),
            version: 1,
            octave_tolerance: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Provenance {
    #[serde(default)]
    pub generator: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub rights: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChartAssetsV01 {
    pub transcript: AssetRef,
    pub pitch_track: AssetRef,
    pub pitch_notes: AssetRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestV01 {
    pub format: String,
    pub format_version: String,
    pub package_id: String,
    pub revision: u32,
    pub song: SongMetadata,
    pub audio: AudioAssetsV01,
    pub charts: ChartAssetsV01,
    #[serde(default)]
    pub visuals: VisualAssets,
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub provenance: Provenance,
}

impl ManifestV01 {
    pub fn new(
        package_id: impl Into<String>,
        song: SongMetadata,
        audio: AudioAssetsV01,
        charts: ChartAssetsV01,
    ) -> Self {
        Self {
            format: FORMAT_ID.into(),
            format_version: LEGACY_FORMAT_VERSION.into(),
            package_id: package_id.into(),
            revision: 1,
            song,
            audio,
            charts,
            visuals: VisualAssets::default(),
            scoring: ScoringConfig::default(),
            provenance: Provenance::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChartAssetsV02 {
    pub vocal: AssetRef,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AnalysisAssetsV02 {
    #[serde(default)]
    pub pitch_evidence: Option<AssetRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestV02 {
    pub format: String,
    pub format_version: String,
    pub package_id: String,
    pub revision: u32,
    pub song: SongMetadata,
    pub audio: AudioAssets,
    pub charts: ChartAssetsV02,
    #[serde(default)]
    pub analysis: AnalysisAssetsV02,
    #[serde(default)]
    pub visuals: VisualAssets,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scoring: Option<ScoringConfig>,
    pub required_features: Vec<String>,
    #[serde(default)]
    pub optional_features: Vec<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, AssetRef>,
    #[serde(default)]
    pub provenance: Provenance,
}

impl ManifestV02 {
    pub fn new(
        package_id: impl Into<String>,
        song: SongMetadata,
        audio: AudioAssets,
        vocal_chart: AssetRef,
    ) -> Self {
        Self {
            format: FORMAT_ID.into(),
            format_version: FORMAT_VERSION.into(),
            package_id: package_id.into(),
            revision: 1,
            song,
            audio,
            charts: ChartAssetsV02 { vocal: vocal_chart },
            analysis: AnalysisAssetsV02::default(),
            visuals: VisualAssets::default(),
            scoring: None,
            required_features: vec!["vocal-chart/1".into()],
            optional_features: Vec::new(),
            extensions: BTreeMap::new(),
            provenance: Provenance::default(),
        }
    }

    pub fn unsupported_required_features<'a>(&'a self, supported: &HashSet<&str>) -> Vec<&'a str> {
        self.required_features
            .iter()
            .map(String::as_str)
            .filter(|feature| !supported.contains(feature))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum UtzManifest {
    V0_2(ManifestV02),
    V0_1(ManifestV01),
}

impl From<ManifestV01> for UtzManifest {
    fn from(value: ManifestV01) -> Self {
        Self::V0_1(value)
    }
}

impl From<ManifestV02> for UtzManifest {
    fn from(value: ManifestV02) -> Self {
        Self::V0_2(value)
    }
}

impl UtzManifest {
    pub fn format_version(&self) -> &str {
        match self {
            Self::V0_1(value) => &value.format_version,
            Self::V0_2(value) => &value.format_version,
        }
    }

    pub fn package_id(&self) -> &str {
        match self {
            Self::V0_1(value) => &value.package_id,
            Self::V0_2(value) => &value.package_id,
        }
    }

    pub fn song(&self) -> &SongMetadata {
        match self {
            Self::V0_1(value) => &value.song,
            Self::V0_2(value) => &value.song,
        }
    }

    pub fn as_v0_1(&self) -> Option<&ManifestV01> {
        match self {
            Self::V0_1(value) => Some(value),
            Self::V0_2(_) => None,
        }
    }

    pub fn as_v0_2(&self) -> Option<&ManifestV02> {
        match self {
            Self::V0_2(value) => Some(value),
            Self::V0_1(_) => None,
        }
    }

    pub fn assets(&self) -> Vec<&AssetRef> {
        let mut assets = Vec::new();
        let visuals = match self {
            Self::V0_1(value) => {
                assets.push(&value.audio.instrumental);
                assets.extend(value.audio.guide_vocals.iter());
                assets.extend(value.audio.original.iter());
                assets.extend([
                    &value.charts.transcript,
                    &value.charts.pitch_track,
                    &value.charts.pitch_notes,
                ]);
                &value.visuals
            }
            Self::V0_2(value) => {
                assets.push(&value.audio.instrumental);
                assets.extend(value.audio.guide_vocals.iter());
                assets.extend(value.audio.original.iter());
                assets.push(&value.charts.vocal);
                assets.extend(value.analysis.pitch_evidence.iter());
                assets.extend(value.extensions.values());
                &value.visuals
            }
        };
        assets.extend(visuals.cover.iter());
        assets.extend(visuals.video.iter());
        assets
    }

    fn assets_mut(&mut self) -> Vec<&mut AssetRef> {
        match self {
            Self::V0_1(value) => {
                let mut assets = vec![
                    &mut value.audio.instrumental,
                    &mut value.charts.transcript,
                    &mut value.charts.pitch_track,
                    &mut value.charts.pitch_notes,
                ];
                assets.extend(value.audio.guide_vocals.iter_mut());
                assets.extend(value.audio.original.iter_mut());
                assets.extend(value.visuals.cover.iter_mut());
                assets.extend(value.visuals.video.iter_mut());
                assets
            }
            Self::V0_2(value) => {
                let mut assets = vec![&mut value.audio.instrumental, &mut value.charts.vocal];
                assets.extend(value.audio.guide_vocals.iter_mut());
                assets.extend(value.audio.original.iter_mut());
                assets.extend(value.analysis.pitch_evidence.iter_mut());
                assets.extend(value.visuals.cover.iter_mut());
                assets.extend(value.visuals.video.iter_mut());
                assets.extend(value.extensions.values_mut());
                assets
            }
        }
    }

    fn update_asset_metadata(&mut self, contents: &BTreeMap<String, Vec<u8>>) -> Result<()> {
        for asset in self.assets_mut() {
            validate_package_path(&asset.path)?;
            let bytes = contents.get(&asset.path).ok_or_else(|| {
                UtzError::Invalid(format!("manifest asset is missing: {}", asset.path))
            })?;
            asset.bytes = bytes.len() as u64;
            asset.sha256 = sha256_hex(bytes);
        }
        Ok(())
    }

    fn update_asset_metadata_from(
        &mut self,
        metadata: &BTreeMap<String, (u64, String)>,
    ) -> Result<()> {
        for asset in self.assets_mut() {
            validate_package_path(&asset.path)?;
            let (bytes, sha256) = metadata.get(&asset.path).ok_or_else(|| {
                UtzError::Invalid(format!("manifest asset is missing: {}", asset.path))
            })?;
            asset.bytes = *bytes;
            asset.sha256.clone_from(sha256);
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let (format, version, package_id, revision, song) = match self {
            Self::V0_1(value) => (
                &value.format,
                &value.format_version,
                &value.package_id,
                value.revision,
                &value.song,
            ),
            Self::V0_2(value) => (
                &value.format,
                &value.format_version,
                &value.package_id,
                value.revision,
                &value.song,
            ),
        };
        if format != FORMAT_ID {
            return invalid(format!("unsupported format {format:?}"));
        }
        // 0.3 only adds optional, defaulted fields to the 0.2 manifest shape,
        // so a reader for one reads the other losslessly.
        let supported = match self {
            Self::V0_1(_) => version.starts_with("0.1."),
            Self::V0_2(_) => version.starts_with("0.2.") || version.starts_with("0.3."),
        };
        if !supported {
            return invalid(format!("unsupported format version {version:?}"));
        }
        if package_id.trim().is_empty()
            || song.title.trim().is_empty()
            || song.artist.trim().is_empty()
        {
            return invalid("package_id, song title, and artist are required");
        }
        if revision == 0
            || !song.duration_seconds.is_finite()
            || song.duration_seconds < 0.0
            || song
                .bpm
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
        {
            return invalid("revision, song timing, or BPM is invalid");
        }
        if song
            .preview_start_seconds
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            return invalid("preview_start_seconds must be a finite non-negative number");
        }
        if song.tags.iter().any(|tag| tag.trim().is_empty()) {
            return invalid("song tags must be non-empty strings");
        }
        match self {
            Self::V0_1(value) => {
                if value.scoring.engine != "uta.pitch" || value.scoring.version != 1 {
                    return invalid(format!(
                        "unsupported 0.1 scoring engine {} version {}",
                        value.scoring.engine, value.scoring.version
                    ));
                }
                if !value.audio.audio_offset_seconds.is_finite() {
                    return invalid("audio_offset_seconds must be finite");
                }
            }
            Self::V0_2(value) => validate_v02_manifest(value)?,
        }

        let visuals = match self {
            Self::V0_1(value) => &value.visuals,
            Self::V0_2(value) => &value.visuals,
        };
        if !visuals.video_offset_seconds.is_finite() {
            return invalid("video_offset_seconds must be finite");
        }

        let mut paths = HashSet::new();
        for asset in self.assets() {
            validate_package_path(&asset.path)?;
            let folded = asset.path.to_lowercase();
            if folded == MANIFEST_PATH {
                return invalid("manifest.json cannot be used as an asset");
            }
            if asset.media_type.trim().is_empty() {
                return invalid(format!("asset {} has no media type", asset.path));
            }
            // Case-insensitive uniqueness protects extraction onto Windows
            // and macOS file systems.
            if !paths.insert(folded) {
                return invalid(format!("asset path is used more than once: {}", asset.path));
            }
        }
        Ok(())
    }
}

fn validate_v02_manifest(value: &ManifestV02) -> Result<()> {
    if let Some(scoring) = &value.scoring
        && (scoring.engine.trim().is_empty() || scoring.version == 0)
    {
        return invalid("scoring hints need a non-empty engine and version");
    }
    if let Some(loudness) = &value.audio.loudness {
        for lufs in [
            loudness.instrumental_lufs,
            loudness.guide_vocals_lufs,
            loudness.original_lufs,
        ]
        .into_iter()
        .flatten()
        {
            if !lufs.is_finite() || !(-70.0..=0.0).contains(&lufs) {
                return invalid("loudness values must be LUFS between -70 and 0");
            }
        }
    }
    if value.charts.vocal.media_type != VOCAL_CHART_MEDIA_TYPE {
        return invalid("vocal chart has the wrong media type");
    }
    if value
        .analysis
        .pitch_evidence
        .as_ref()
        .is_some_and(|asset| asset.media_type != PITCH_EVIDENCE_MEDIA_TYPE)
    {
        return invalid("pitch evidence has the wrong media type");
    }
    let required: HashSet<_> = value.required_features.iter().map(String::as_str).collect();
    if required.len() != value.required_features.len() || !required.contains("vocal-chart/1") {
        return invalid("required_features must uniquely include vocal-chart/1");
    }
    let optional: HashSet<_> = value.optional_features.iter().map(String::as_str).collect();
    if optional.len() != value.optional_features.len()
        || required.iter().any(|feature| optional.contains(feature))
    {
        return invalid("feature lists must be unique and disjoint");
    }
    for feature in required.iter().chain(optional.iter()) {
        validate_feature(feature)?;
    }
    if value.analysis.pitch_evidence.is_some() && !optional.contains("pitch-evidence/1") {
        return invalid("pitch evidence must declare optional feature pitch-evidence/1");
    }
    for feature in value.extensions.keys() {
        validate_feature(feature)?;
        if required.contains(feature.as_str()) || optional.contains(feature.as_str()) {
            return invalid(format!("extension feature is declared twice: {feature}"));
        }
    }
    Ok(())
}

fn validate_feature(value: &str) -> Result<()> {
    let Some((name, version)) = value.rsplit_once('/') else {
        return invalid(format!("invalid feature identifier {value:?}"));
    };
    if name.is_empty()
        || !name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && b"._-".contains(&byte))
        })
        || version.is_empty()
        || version.starts_with('0')
        || !version.bytes().all(|byte| byte.is_ascii_digit())
    {
        return invalid(format!("invalid feature identifier {value:?}"));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VocalChartV1 {
    pub format: String,
    pub format_version: String,
    pub timebase: u64,
    #[serde(default)]
    pub language: Option<String>,
    pub tracks: Vec<VocalTrack>,
}

impl VocalChartV1 {
    pub fn new(tracks: Vec<VocalTrack>) -> Self {
        Self {
            format: VOCAL_CHART_FORMAT.into(),
            format_version: VOCAL_CHART_VERSION.into(),
            timebase: DEFAULT_TIMEBASE,
            language: None,
            tracks,
        }
    }

    pub fn validate(&self) -> Result<()> {
        // Readers accept any 1.x chart; fields introduced by a newer minor
        // version are ignorable by design.
        if self.format != VOCAL_CHART_FORMAT || !self.format_version.starts_with("1.") {
            return invalid("unsupported vocal chart format or version");
        }
        validate_time_value(self.timebase, "vocal chart timebase")?;
        if self.tracks.is_empty() {
            return invalid("vocal chart has no tracks");
        }
        let mut track_ids = HashSet::new();
        let mut phrase_ids = HashSet::new();
        let mut note_ids = HashSet::new();
        let mut parts = std::collections::BTreeSet::new();
        for track in &self.tracks {
            validate_id(&track.id, "track")?;
            if !track_ids.insert(track.id.as_str()) {
                return invalid(format!("duplicate track id {}", track.id));
            }
            if let Some(part) = track.part {
                if !(1..=MAX_DUET_PARTS).contains(&part) {
                    return invalid(format!(
                        "track {} part must be between 1 and {MAX_DUET_PARTS}",
                        track.id
                    ));
                }
                parts.insert(part);
            }
            if track.phrases.is_empty() {
                return invalid(format!("track {} has no phrases", track.id));
            }
            let mut previous_phrase_end = 0;
            let mut lyric_ids = HashSet::new();
            let mut continuations = Vec::new();
            for phrase in &track.phrases {
                validate_id(&phrase.id, "phrase")?;
                if !phrase_ids.insert(phrase.id.as_str()) {
                    return invalid(format!("duplicate phrase id {}", phrase.id));
                }
                if phrase.notes.is_empty() {
                    return invalid(format!("phrase {} has no notes", phrase.id));
                }
                let mut previous_note_end = 0;
                for (index, note) in phrase.notes.iter().enumerate() {
                    validate_id(&note.id, "note")?;
                    if !note_ids.insert(note.id.as_str()) {
                        return invalid(format!("duplicate note id {}", note.id));
                    }
                    validate_exact_value(note.start, "note start")?;
                    validate_time_value(note.duration, "note duration")?;
                    let end = note
                        .start
                        .checked_add(note.duration)
                        .filter(|value| *value <= MAX_EXACT_INTEGER)
                        .ok_or_else(|| {
                            UtzError::Invalid(format!("note {} end overflows", note.id))
                        })?;
                    if index > 0 && note.start < previous_note_end {
                        return invalid(format!(
                            "notes overlap or are unordered in phrase {}",
                            phrase.id
                        ));
                    }
                    previous_note_end = end;
                    if (note.vocal_mode == VocalMode::Pitched
                        || note.scoring.mode == ScoringMode::Pitch)
                        && note.pitch.is_none()
                    {
                        return invalid(format!("note {} requires a pitch target", note.id));
                    }
                    if note
                        .pitch
                        .is_some_and(|pitch| pitch.midi > 127 || !(-99..=99).contains(&pitch.cents))
                    {
                        return invalid(format!("note {} has an invalid pitch target", note.id));
                    }
                    if !note.scoring.weight.is_finite() || note.scoring.weight < 0.0 {
                        return invalid(format!("note {} has an invalid scoring weight", note.id));
                    }
                    for token in &note.lyrics {
                        match token {
                            LyricToken::Text(token) => {
                                validate_id(&token.id, "lyric token")?;
                                if !lyric_ids.insert(token.id.as_str()) {
                                    return invalid(format!(
                                        "duplicate lyric token id {}",
                                        token.id
                                    ));
                                }
                            }
                            LyricToken::Continuation { continuation_of } => {
                                validate_id(continuation_of, "lyric continuation")?;
                                continuations.push(continuation_of.as_str());
                            }
                        }
                    }
                }
                let phrase_start = phrase.notes[0].start;
                if phrase_start < previous_phrase_end {
                    return invalid(format!(
                        "phrases overlap or are unordered in track {}",
                        track.id
                    ));
                }
                previous_phrase_end = previous_note_end;
            }
            for continuation in continuations {
                if !lyric_ids.contains(continuation) {
                    return invalid(format!(
                        "lyric continuation {continuation} does not resolve in track {}",
                        track.id
                    ));
                }
            }
        }
        // Assigned duet parts must be exactly 1..=N with no gaps, mirroring
        // the UltraStar voice numbering rule.
        if let Some(max) = parts.last().copied()
            && parts.len() as u32 != max
        {
            return invalid("duet parts must be contiguous starting at 1");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VocalTrack {
    pub id: String,
    pub role: VocalTrackRole,
    /// Duet part this track belongs to, counted from 1 (UltraStar P1/P2).
    /// `None` means the track is not assigned to a specific player.
    #[serde(default)]
    pub part: Option<u32>,
    #[serde(default)]
    pub singer: Option<String>,
    #[serde(default = "default_true")]
    pub scoring_enabled: bool,
    pub phrases: Vec<VocalPhrase>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VocalTrackRole {
    Lead,
    Harmony,
    Backing,
    Adlib,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VocalPhrase {
    pub id: String,
    pub notes: Vec<VocalNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VocalNote {
    pub id: String,
    pub start: u64,
    pub duration: u64,
    pub pitch: Option<NotePitch>,
    pub vocal_mode: VocalMode,
    pub bonus: NoteBonus,
    pub scoring: NoteScoring,
    pub lyrics: Vec<LyricToken>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotePitch {
    pub midi: u8,
    #[serde(default)]
    pub cents: i8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VocalMode {
    Pitched,
    Rap,
    Spoken,
    Freestyle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoteBonus {
    Normal,
    Golden,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoteScoring {
    pub mode: ScoringMode,
    #[serde(default = "default_weight")]
    pub weight: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScoringMode {
    Pitch,
    Rhythm,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum LyricToken {
    Text(LyricTextToken),
    Continuation { continuation_of: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LyricTextToken {
    pub id: String,
    pub text: String,
    pub join_before: LyricJoin,
    #[serde(default)]
    pub reading: Option<String>,
    #[serde(default)]
    pub phonemes: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LyricJoin {
    None,
    Space,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PitchEvidenceV1 {
    pub format: String,
    pub format_version: String,
    pub timebase: u64,
    pub start: u64,
    pub hop: u64,
    pub frequency_hz: Vec<Option<f64>>,
    pub confidence: Vec<f64>,
    #[serde(default)]
    pub model: Option<serde_json::Map<String, serde_json::Value>>,
}

impl PitchEvidenceV1 {
    pub fn validate(&self) -> Result<()> {
        if self.format != PITCH_EVIDENCE_FORMAT || !self.format_version.starts_with("1.0.") {
            return invalid("unsupported pitch evidence format or version");
        }
        validate_time_value(self.timebase, "pitch evidence timebase")?;
        if self.start > MAX_EXACT_INTEGER {
            return invalid("pitch evidence start exceeds exact integer range");
        }
        validate_time_value(self.hop, "pitch evidence hop")?;
        if self.frequency_hz.len() != self.confidence.len() {
            return invalid("pitch evidence frequency and confidence lengths differ");
        }
        if self
            .frequency_hz
            .iter()
            .flatten()
            .any(|value| !value.is_finite() || *value <= 0.0)
            || self
                .confidence
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        {
            return invalid("pitch evidence contains an invalid sample");
        }
        let frame_count = u64::try_from(self.frequency_hz.len())
            .map_err(|_| UtzError::Invalid("pitch evidence has too many frames".into()))?;
        self.hop
            .checked_mul(frame_count)
            .and_then(|span| self.start.checked_add(span))
            .filter(|end| *end <= MAX_EXACT_INTEGER)
            .ok_or_else(|| UtzError::Invalid("pitch evidence time range overflows".into()))?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct UtzPackage {
    manifest: UtzManifest,
    files: BTreeMap<String, Vec<u8>>,
}

impl UtzPackage {
    pub fn build<M: Into<UtzManifest>>(
        manifest: M,
        files: BTreeMap<String, Vec<u8>>,
    ) -> Result<Self> {
        let mut manifest = manifest.into();
        validate_file_map(&files)?;
        manifest.update_asset_metadata(&files)?;
        manifest.validate()?;
        validate_declared_files(&manifest, &files)?;
        validate_semantic_assets(&manifest, |path| {
            files
                .get(path)
                .map(Vec::as_slice)
                .ok_or_else(|| UtzError::Invalid(format!("manifest asset is missing: {path}")))
        })?;
        Ok(Self { manifest, files })
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut archive = ZipArchive::new(Cursor::new(bytes))?;
        if archive.len() > MAX_FILES {
            return invalid(format!("package has more than {MAX_FILES} files"));
        }
        let mut files = BTreeMap::new();
        let mut folded_names = HashSet::new();
        let mut total = 0u64;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_owned();
            validate_package_path(&name)?;
            if entry.size() > MAX_FILE_BYTES {
                return invalid(format!("asset is too large: {name}"));
            }
            total = total.saturating_add(entry.size());
            if total > MAX_TOTAL_BYTES {
                return invalid("package expands beyond the size limit");
            }
            let mut content = Vec::with_capacity(entry.size().min(usize::MAX as u64) as usize);
            entry.read_to_end(&mut content)?;
            if !folded_names.insert(name.to_lowercase()) {
                return invalid(format!("duplicate archive path: {name}"));
            }
            files.insert(name, content);
        }
        let manifest_bytes = files
            .remove(MANIFEST_PATH)
            .ok_or_else(|| UtzError::Invalid("manifest.json is missing".into()))?;
        let manifest: UtzManifest = serde_json::from_slice(&manifest_bytes)?;
        manifest.validate()?;
        validate_declared_files(&manifest, &files)?;
        for asset in manifest.assets() {
            let content = files.get(&asset.path).ok_or_else(|| {
                UtzError::Invalid(format!("manifest asset is missing: {}", asset.path))
            })?;
            if content.len() as u64 != asset.bytes {
                return invalid(format!("asset byte count differs: {}", asset.path));
            }
        }
        validate_semantic_assets(&manifest, |path| {
            files
                .get(path)
                .map(Vec::as_slice)
                .ok_or_else(|| UtzError::Invalid(format!("manifest asset is missing: {path}")))
        })?;
        Ok(Self { manifest, files })
    }

    /// Write a package directly to a seekable destination. Large media files
    /// are copied in fixed-size chunks, so peak memory stays nearly
    /// constant even when a package includes video.
    pub fn write_streaming<M, W, F>(
        manifest: M,
        sources: BTreeMap<String, AssetSource>,
        output: W,
        mut progress: F,
    ) -> Result<W>
    where
        M: Into<UtzManifest>,
        W: Write + Seek,
        F: FnMut(&str, u64, u64),
    {
        let mut manifest = manifest.into();
        let declared: BTreeMap<String, String> = manifest
            .assets()
            .into_iter()
            .map(|asset| (asset.path.clone(), asset.media_type.clone()))
            .collect();
        if declared.len() != manifest.assets().len() || sources.keys().ne(declared.keys()) {
            return invalid("package sources must exactly match the manifest assets");
        }
        let mut total = 0u64;
        for (path, source) in &sources {
            validate_package_path(path)?;
            let bytes = source_size(source)?;
            if bytes > MAX_FILE_BYTES {
                return invalid(format!("asset is too large: {path}"));
            }
            total = total.saturating_add(bytes);
            if total > MAX_TOTAL_BYTES {
                return invalid("package expands beyond the size limit");
            }
        }
        validate_semantic_sources(&manifest, &sources)?;

        let mut writer = ZipWriter::new(output);
        let options = SimpleFileOptions::default().unix_permissions(0o644);
        let mut metadata = BTreeMap::new();
        let mut completed = 0u64;
        let mut buffer = vec![0u8; 128 * 1024];
        for (path, source) in sources {
            let media_type = declared.get(&path).expect("source keys were checked");
            let compression = if media_type.contains("json") || media_type.starts_with("text/") {
                CompressionMethod::Deflated
            } else {
                CompressionMethod::Stored
            };
            writer.start_file(&path, options.compression_method(compression))?;
            let mut hasher = Sha256::new();
            let bytes = match source {
                AssetSource::Bytes(bytes) => {
                    writer.write_all(&bytes)?;
                    hasher.update(&bytes);
                    completed = completed.saturating_add(bytes.len() as u64);
                    progress(&path, completed, total);
                    bytes.len() as u64
                }
                AssetSource::File(source_path) => {
                    let mut source_file = std::fs::File::open(source_path)?;
                    let mut written = 0u64;
                    loop {
                        let count = source_file.read(&mut buffer)?;
                        if count == 0 {
                            break;
                        }
                        writer.write_all(&buffer[..count])?;
                        hasher.update(&buffer[..count]);
                        written = written.saturating_add(count as u64);
                        completed = completed.saturating_add(count as u64);
                        progress(&path, completed, total);
                    }
                    written
                }
            };
            metadata.insert(path, (bytes, format!("{:x}", hasher.finalize())));
        }
        manifest.update_asset_metadata_from(&metadata)?;
        manifest.validate()?;
        writer.start_file(
            MANIFEST_PATH,
            options.compression_method(CompressionMethod::Deflated),
        )?;
        writer.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        Ok(writer.finish()?)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.manifest.validate()?;
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        writer.start_file(MANIFEST_PATH, options)?;
        writer.write_all(&serde_json::to_vec_pretty(&self.manifest)?)?;
        for (path, bytes) in &self.files {
            writer.start_file(path, options)?;
            writer.write_all(bytes)?;
        }
        Ok(writer.finish()?.into_inner())
    }

    pub fn manifest(&self) -> &UtzManifest {
        &self.manifest
    }

    pub fn vocal_chart(&self) -> Result<Option<VocalChartV1>> {
        let Some(manifest) = self.manifest.as_v0_2() else {
            return Ok(None);
        };
        Ok(Some(parse_validated(
            self.file(&manifest.charts.vocal.path)
                .ok_or_else(|| UtzError::Invalid("vocal chart is missing".into()))?,
            VocalChartV1::validate,
        )?))
    }

    pub fn pitch_evidence(&self) -> Result<Option<PitchEvidenceV1>> {
        let Some(asset) = self
            .manifest
            .as_v0_2()
            .and_then(|manifest| manifest.analysis.pitch_evidence.as_ref())
        else {
            return Ok(None);
        };
        Ok(Some(parse_validated(
            self.file(&asset.path)
                .ok_or_else(|| UtzError::Invalid("pitch evidence is missing".into()))?,
            PitchEvidenceV1::validate,
        )?))
    }

    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
    }
}

fn validate_file_map(files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    if files.len() > MAX_FILES.saturating_sub(1) {
        return invalid(format!("package has more than {MAX_FILES} files"));
    }
    let mut total = 0u64;
    let mut folded_names = HashSet::new();
    for (path, bytes) in files {
        validate_package_path(path)?;
        let folded = path.to_lowercase();
        if folded == MANIFEST_PATH {
            return invalid("files map must not contain manifest.json");
        }
        if !folded_names.insert(folded) {
            return invalid(format!("duplicate archive path: {path}"));
        }
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return invalid(format!("asset is too large: {path}"));
        }
        total = total.saturating_add(bytes.len() as u64);
    }
    if total > MAX_TOTAL_BYTES {
        return invalid("package expands beyond the size limit");
    }
    Ok(())
}

fn validate_declared_files(
    manifest: &UtzManifest,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let declared: HashSet<_> = manifest
        .assets()
        .iter()
        .map(|asset| asset.path.as_str())
        .collect();
    if let Some(path) = declared.iter().find(|path| !files.contains_key(**path)) {
        return invalid(format!("manifest asset is missing: {path}"));
    }
    if matches!(manifest, UtzManifest::V0_2(_))
        && let Some(path) = files.keys().find(|path| !declared.contains(path.as_str()))
    {
        return invalid(format!("UTZ 0.2 contains undeclared asset: {path}"));
    }
    Ok(())
}

fn validate_semantic_assets<'a, F>(manifest: &UtzManifest, mut read: F) -> Result<()>
where
    F: FnMut(&str) -> Result<&'a [u8]>,
{
    if let UtzManifest::V0_2(value) = manifest {
        let _: VocalChartV1 =
            parse_validated(read(&value.charts.vocal.path)?, VocalChartV1::validate)?;
        if let Some(asset) = &value.analysis.pitch_evidence {
            let _: PitchEvidenceV1 =
                parse_validated(read(&asset.path)?, PitchEvidenceV1::validate)?;
        }
    }
    Ok(())
}

fn validate_semantic_sources(
    manifest: &UtzManifest,
    sources: &BTreeMap<String, AssetSource>,
) -> Result<()> {
    if let UtzManifest::V0_2(value) = manifest {
        let bytes = source_bytes(
            sources
                .get(&value.charts.vocal.path)
                .ok_or_else(|| UtzError::Invalid("vocal chart source is missing".into()))?,
        )?;
        let _: VocalChartV1 = parse_validated(&bytes, VocalChartV1::validate)?;
        if let Some(asset) = &value.analysis.pitch_evidence {
            let bytes =
                source_bytes(sources.get(&asset.path).ok_or_else(|| {
                    UtzError::Invalid("pitch evidence source is missing".into())
                })?)?;
            let _: PitchEvidenceV1 = parse_validated(&bytes, PitchEvidenceV1::validate)?;
        }
    }
    Ok(())
}

fn parse_validated<T: DeserializeOwned>(bytes: &[u8], validate: fn(&T) -> Result<()>) -> Result<T> {
    let value = serde_json::from_slice(bytes)?;
    validate(&value)?;
    Ok(value)
}

fn source_size(source: &AssetSource) -> Result<u64> {
    Ok(match source {
        AssetSource::Bytes(bytes) => bytes.len() as u64,
        AssetSource::File(path) => std::fs::metadata(path)?.len(),
    })
}

fn source_bytes(source: &AssetSource) -> Result<Vec<u8>> {
    Ok(match source {
        AssetSource::Bytes(bytes) => bytes.clone(),
        AssetSource::File(path) => std::fs::read(path)?,
    })
}

fn validate_time_value(value: u64, role: &str) -> Result<()> {
    if value == 0 || value > MAX_EXACT_INTEGER {
        return invalid(format!("{role} is outside the supported integer range"));
    }
    Ok(())
}

fn validate_exact_value(value: u64, role: &str) -> Result<()> {
    if value > MAX_EXACT_INTEGER {
        return invalid(format!("{role} is outside the supported integer range"));
    }
    Ok(())
}

fn validate_id(value: &str, role: &str) -> Result<()> {
    if value.trim().is_empty() {
        return invalid(format!("{role} id is empty"));
    }
    if value.len() > MAX_ID_BYTES {
        return invalid(format!("{role} id is longer than {MAX_ID_BYTES} bytes"));
    }
    Ok(())
}

fn default_true() -> bool {
    true
}

fn default_weight() -> f64 {
    1.0
}

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(UtzError::Invalid(message.into()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn validate_package_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains('\\')
        || path.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':'))
    {
        return invalid(format!("unsafe package path: {path:?}"));
    }
    // Names that break extraction on Windows file systems.
    for part in path.split('/') {
        if part.ends_with('.') || part.ends_with(' ') {
            return invalid(format!("unsafe package path: {path:?}"));
        }
        let base = part.split('.').next().unwrap_or_default();
        if matches!(
            base.to_ascii_lowercase().as_str(),
            "con" | "prn" | "aux" | "nul"
        ) || (base.len() == 4
            && matches!(&base.to_ascii_lowercase()[..3], "com" | "lpt")
            && base.as_bytes()[3].is_ascii_digit())
        {
            return invalid(format!("unsafe package path: {path:?}"));
        }
    }
    if Path::new(path)
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return invalid(format!("unsafe package path: {path:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> SongMetadata {
        SongMetadata {
            title: "Example".into(),
            artist: "Uta".into(),
            album: None,
            language: Some("ja".into()),
            duration_seconds: 12.0,
            bpm: Some(120.0),
            key: Some("C".into()),
            title_sort: None,
            artist_sort: None,
            genre: None,
            year: None,
            creator: None,
            composer: None,
            country: None,
            tags: Vec::new(),
            preview_start_seconds: Some(4.0),
        }
    }

    fn audio() -> AudioAssets {
        AudioAssets {
            instrumental: AssetRef::pending("audio/instrumental.mp3", "audio/mpeg"),
            guide_vocals: None,
            original: None,
            loudness: None,
        }
    }

    fn audio_v01() -> AudioAssetsV01 {
        AudioAssetsV01 {
            instrumental: AssetRef::pending("audio/instrumental.mp3", "audio/mpeg"),
            guide_vocals: None,
            original: None,
            audio_offset_seconds: 0.0,
        }
    }

    fn note(id: &str, start: u64, lyric_id: &str, text: &str) -> VocalNote {
        VocalNote {
            id: id.into(),
            start,
            duration: 500_000,
            pitch: Some(NotePitch { midi: 69, cents: 0 }),
            vocal_mode: VocalMode::Pitched,
            bonus: NoteBonus::Normal,
            scoring: NoteScoring {
                mode: ScoringMode::Pitch,
                weight: 1.0,
            },
            lyrics: vec![LyricToken::Text(LyricTextToken {
                id: lyric_id.into(),
                text: text.into(),
                join_before: LyricJoin::None,
                reading: None,
                phonemes: None,
            })],
        }
    }

    fn chart() -> VocalChartV1 {
        VocalChartV1::new(vec![VocalTrack {
            id: "lead".into(),
            role: VocalTrackRole::Lead,
            part: None,
            singer: None,
            scoring_enabled: true,
            phrases: vec![VocalPhrase {
                id: "phrase-1".into(),
                notes: vec![note("note-1", 0, "lyric-1", "歌")],
            }],
        }])
    }

    fn duet_track(id: &str, part: u32, note_id: &str, lyric_id: &str) -> VocalTrack {
        VocalTrack {
            id: id.into(),
            role: VocalTrackRole::Lead,
            part: Some(part),
            singer: None,
            scoring_enabled: true,
            phrases: vec![VocalPhrase {
                id: format!("{id}-phrase"),
                notes: vec![note(note_id, 0, lyric_id, "歌")],
            }],
        }
    }

    fn sample_v02() -> (ManifestV02, BTreeMap<String, Vec<u8>>) {
        let manifest = ManifestV02::new(
            "org.uta.example",
            song(),
            audio(),
            AssetRef::pending("charts/vocal.json", VOCAL_CHART_MEDIA_TYPE),
        );
        let files = BTreeMap::from([
            ("audio/instrumental.mp3".into(), b"audio".to_vec()),
            (
                "charts/vocal.json".into(),
                serde_json::to_vec(&chart()).unwrap(),
            ),
        ]);
        (manifest, files)
    }

    fn sample_v01() -> (ManifestV01, BTreeMap<String, Vec<u8>>) {
        let manifest = ManifestV01::new(
            "org.uta.legacy",
            song(),
            audio_v01(),
            ChartAssetsV01 {
                transcript: AssetRef::pending("charts/transcript.json", "application/json"),
                pitch_track: AssetRef::pending("charts/pitch-track.json", "application/json"),
                pitch_notes: AssetRef::pending("charts/pitch-notes.json", "application/json"),
            },
        );
        let files = BTreeMap::from([
            ("audio/instrumental.mp3".into(), b"audio".to_vec()),
            (
                "charts/transcript.json".into(),
                b"{\"segments\":[]}".to_vec(),
            ),
            (
                "charts/pitch-track.json".into(),
                b"{\"frames\":[]}".to_vec(),
            ),
            ("charts/pitch-notes.json".into(), b"{\"notes\":[]}".to_vec()),
        ]);
        (manifest, files)
    }

    #[test]
    fn v02_round_trip_exposes_note_owned_lyrics() {
        let (manifest, files) = sample_v02();
        let package = UtzPackage::build(manifest, files).unwrap();
        let decoded = UtzPackage::from_bytes(&package.to_bytes().unwrap()).unwrap();
        assert_eq!(decoded.manifest().format_version(), FORMAT_VERSION);
        assert_eq!(decoded.vocal_chart().unwrap().unwrap(), chart());
    }

    #[test]
    fn v01_packages_remain_readable() {
        let (manifest, files) = sample_v01();
        let package = UtzPackage::build(manifest, files).unwrap();
        let decoded = UtzPackage::from_bytes(&package.to_bytes().unwrap()).unwrap();
        assert_eq!(decoded.manifest().format_version(), LEGACY_FORMAT_VERSION);
        assert!(decoded.vocal_chart().unwrap().is_none());
    }

    #[test]
    fn streaming_writer_round_trip_verifies_assets_and_progress() {
        let (manifest, files) = sample_v02();
        let total: u64 = files.values().map(|value| value.len() as u64).sum();
        let sources = files
            .into_iter()
            .map(|(path, bytes)| (path, AssetSource::Bytes(bytes)))
            .collect();
        let mut final_progress = 0;
        let cursor = UtzPackage::write_streaming(
            manifest,
            sources,
            Cursor::new(Vec::new()),
            |_, completed, reported_total| {
                assert_eq!(reported_total, total);
                final_progress = completed;
            },
        )
        .unwrap();
        assert_eq!(final_progress, total);
        UtzPackage::from_bytes(&cursor.into_inner()).unwrap();
    }

    #[test]
    fn vocal_chart_rejects_missing_pitch_overlap_and_broken_continuation() {
        let mut missing_pitch = chart();
        missing_pitch.tracks[0].phrases[0].notes[0].pitch = None;
        assert!(missing_pitch.validate().is_err());

        let mut overlap = chart();
        overlap.tracks[0].phrases[0]
            .notes
            .push(note("note-2", 200_000, "lyric-2", "詞"));
        assert!(overlap.validate().is_err());

        let mut continuation = chart();
        continuation.tracks[0].phrases[0].notes[0].lyrics = vec![LyricToken::Continuation {
            continuation_of: "missing".into(),
        }];
        assert!(continuation.validate().is_err());
    }

    #[test]
    fn rhythm_note_can_be_unpitched() {
        let mut value = chart();
        let note = &mut value.tracks[0].phrases[0].notes[0];
        note.pitch = None;
        note.vocal_mode = VocalMode::Rap;
        note.scoring.mode = ScoringMode::Rhythm;
        assert!(value.validate().is_ok());
    }

    #[test]
    fn unknown_required_feature_is_reported() {
        let (mut manifest, _) = sample_v02();
        manifest.required_features.push("future-notes/1".into());
        let supported = HashSet::from(["vocal-chart/1"]);
        assert_eq!(
            manifest.unsupported_required_features(&supported),
            vec!["future-notes/1"]
        );
    }

    #[test]
    fn v02_rejects_undeclared_assets() {
        let (manifest, mut files) = sample_v02();
        files.insert("extra.bin".into(), Vec::new());
        assert!(UtzPackage::build(manifest, files).is_err());
    }

    #[test]
    fn traversal_paths_are_rejected() {
        assert!(validate_package_path("../outside").is_err());
        assert!(validate_package_path("/absolute").is_err());
        assert!(validate_package_path("audio\\track.mp3").is_err());
        assert!(validate_package_path("C:/track.mp3").is_err());
        assert!(validate_package_path("audio//track.mp3").is_err());
        assert!(validate_package_path("audio/track.mp3").is_ok());
    }

    #[test]
    fn windows_hostile_paths_are_rejected() {
        assert!(validate_package_path("aux.mp3").is_err());
        assert!(validate_package_path("audio/NUL.txt").is_err());
        assert!(validate_package_path("audio/COM1").is_err());
        assert!(validate_package_path("audio/track.mp3.").is_err());
        assert!(validate_package_path("audio /track.mp3").is_err());
        assert!(validate_package_path("audio/com10.mp3").is_ok());
        assert!(validate_package_path("audio/console.mp3").is_ok());
    }

    #[test]
    fn case_insensitive_duplicate_paths_are_rejected() {
        let (mut manifest, mut files) = sample_v02();
        files.insert("Audio/Instrumental.mp3".into(), b"copy".to_vec());
        manifest.audio.original = Some(AssetRef::pending("Audio/Instrumental.mp3", "audio/mpeg"));
        assert!(UtzPackage::build(manifest, files).is_err());
    }

    #[test]
    fn duet_parts_must_be_contiguous() {
        let mut valid = VocalChartV1::new(vec![
            duet_track("p1", 1, "note-1", "lyric-1"),
            duet_track("p2", 2, "note-2", "lyric-2"),
        ]);
        assert!(valid.validate().is_ok());
        valid.tracks[0].part = Some(2);
        valid.tracks[1].part = Some(3);
        assert!(valid.validate().is_err());
        valid.tracks[0].part = Some(1);
        valid.tracks[1].part = Some(1);
        assert!(valid.validate().is_ok());
        valid.tracks[1].part = Some(0);
        assert!(valid.validate().is_err());
    }

    #[test]
    fn a_note_with_no_lyric_tokens_yet_is_still_valid() {
        let mut value = chart();
        value.tracks[0].phrases[0].notes[0].lyrics.clear();
        assert!(value.validate().is_ok());
    }

    #[test]
    fn preview_start_must_be_non_negative() {
        let (mut manifest, files) = sample_v02();
        manifest.song.preview_start_seconds = Some(-1.0);
        assert!(UtzPackage::build(manifest, files).is_err());
    }

    #[test]
    fn loudness_must_be_plausible_lufs() {
        let (mut manifest, files) = sample_v02();
        manifest.audio.loudness = Some(AudioLoudness {
            instrumental_lufs: Some(3.0),
            ..AudioLoudness::default()
        });
        assert!(UtzPackage::build(manifest, files).is_err());
    }
}
