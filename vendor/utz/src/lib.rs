//! Independent `.utz` package model and safe ZIP reader/writer.

use std::{
    collections::{BTreeMap, HashSet},
    io::{Cursor, Read, Seek, Write},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const FORMAT_ID: &str = "uta.song";
pub const FORMAT_VERSION: &str = "0.1.0";
pub const MANIFEST_PATH: &str = "manifest.json";
pub const MAX_FILES: usize = 128;
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum UtzError {
    #[error("invalid ZIP package: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("could not read or write package: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid manifest JSON: {0}")]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UtzManifest {
    pub format: String,
    pub format_version: String,
    pub package_id: String,
    pub revision: u32,
    pub song: SongMetadata,
    pub audio: AudioAssets,
    pub charts: ChartAssets,
    #[serde(default)]
    pub visuals: VisualAssets,
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub provenance: Provenance,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioAssets {
    pub instrumental: AssetRef,
    #[serde(default)]
    pub guide_vocals: Option<AssetRef>,
    #[serde(default)]
    pub original: Option<AssetRef>,
    #[serde(default)]
    pub audio_offset_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChartAssets {
    pub transcript: AssetRef,
    pub pitch_track: AssetRef,
    pub pitch_notes: AssetRef,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VisualAssets {
    #[serde(default)]
    pub cover: Option<AssetRef>,
    #[serde(default)]
    pub video: Option<AssetRef>,
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

impl UtzManifest {
    pub fn new(
        package_id: impl Into<String>,
        song: SongMetadata,
        audio: AudioAssets,
        charts: ChartAssets,
    ) -> Self {
        Self {
            format: FORMAT_ID.into(),
            format_version: FORMAT_VERSION.into(),
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

    pub fn assets(&self) -> Vec<&AssetRef> {
        let mut assets = vec![
            &self.audio.instrumental,
            &self.charts.transcript,
            &self.charts.pitch_track,
            &self.charts.pitch_notes,
        ];
        assets.extend(self.audio.guide_vocals.iter());
        assets.extend(self.audio.original.iter());
        assets.extend(self.visuals.cover.iter());
        assets.extend(self.visuals.video.iter());
        assets
    }

    fn update_asset_metadata(&mut self, contents: &BTreeMap<String, Vec<u8>>) -> Result<()> {
        fn update(asset: &mut AssetRef, contents: &BTreeMap<String, Vec<u8>>) -> Result<()> {
            validate_package_path(&asset.path)?;
            let bytes = contents.get(&asset.path).ok_or_else(|| {
                UtzError::Invalid(format!("manifest asset is missing: {}", asset.path))
            })?;
            asset.bytes = bytes.len() as u64;
            asset.sha256 = sha256_hex(bytes);
            Ok(())
        }

        update(&mut self.audio.instrumental, contents)?;
        if let Some(asset) = &mut self.audio.guide_vocals {
            update(asset, contents)?;
        }
        if let Some(asset) = &mut self.audio.original {
            update(asset, contents)?;
        }
        update(&mut self.charts.transcript, contents)?;
        update(&mut self.charts.pitch_track, contents)?;
        update(&mut self.charts.pitch_notes, contents)?;
        if let Some(asset) = &mut self.visuals.cover {
            update(asset, contents)?;
        }
        if let Some(asset) = &mut self.visuals.video {
            update(asset, contents)?;
        }
        Ok(())
    }

    fn update_asset_metadata_from(
        &mut self,
        metadata: &BTreeMap<String, (u64, String)>,
    ) -> Result<()> {
        fn update(asset: &mut AssetRef, metadata: &BTreeMap<String, (u64, String)>) -> Result<()> {
            validate_package_path(&asset.path)?;
            let (bytes, sha256) = metadata.get(&asset.path).ok_or_else(|| {
                UtzError::Invalid(format!("manifest asset is missing: {}", asset.path))
            })?;
            asset.bytes = *bytes;
            asset.sha256.clone_from(sha256);
            Ok(())
        }

        update(&mut self.audio.instrumental, metadata)?;
        if let Some(asset) = &mut self.audio.guide_vocals {
            update(asset, metadata)?;
        }
        if let Some(asset) = &mut self.audio.original {
            update(asset, metadata)?;
        }
        update(&mut self.charts.transcript, metadata)?;
        update(&mut self.charts.pitch_track, metadata)?;
        update(&mut self.charts.pitch_notes, metadata)?;
        if let Some(asset) = &mut self.visuals.cover {
            update(asset, metadata)?;
        }
        if let Some(asset) = &mut self.visuals.video {
            update(asset, metadata)?;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self.format != FORMAT_ID {
            return Err(UtzError::Invalid(format!(
                "unsupported format {:?}",
                self.format
            )));
        }
        if self.format_version != FORMAT_VERSION {
            return Err(UtzError::Invalid(format!(
                "unsupported format version {:?}; this reader supports {FORMAT_VERSION}",
                self.format_version
            )));
        }
        if self.package_id.trim().is_empty()
            || self.song.title.trim().is_empty()
            || self.song.artist.trim().is_empty()
        {
            return Err(UtzError::Invalid(
                "package_id, song title, and artist are required".into(),
            ));
        }
        if self.revision == 0
            || !self.song.duration_seconds.is_finite()
            || self.song.duration_seconds < 0.0
        {
            return Err(UtzError::Invalid(
                "revision and song duration are invalid".into(),
            ));
        }
        if self.scoring.engine != "uta.pitch" || self.scoring.version != 1 {
            return Err(UtzError::Invalid(format!(
                "unsupported scoring engine {} version {}",
                self.scoring.engine, self.scoring.version
            )));
        }
        let mut paths = HashSet::new();
        for asset in self.assets() {
            validate_package_path(&asset.path)?;
            if asset.path == MANIFEST_PATH {
                return Err(UtzError::Invalid(
                    "manifest.json cannot be used as an asset".into(),
                ));
            }
            if asset.media_type.trim().is_empty() {
                return Err(UtzError::Invalid(format!(
                    "asset {} has no media type",
                    asset.path
                )));
            }
            if !paths.insert(asset.path.as_str()) {
                return Err(UtzError::Invalid(format!(
                    "asset path is used more than once: {}",
                    asset.path
                )));
            }
            if asset.sha256.len() != 64
                || !asset
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err(UtzError::Invalid(format!(
                    "asset {} has an invalid SHA-256",
                    asset.path
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct UtzPackage {
    manifest: UtzManifest,
    files: BTreeMap<String, Vec<u8>>,
}

impl UtzPackage {
    pub fn build(mut manifest: UtzManifest, files: BTreeMap<String, Vec<u8>>) -> Result<Self> {
        if files.len() > MAX_FILES.saturating_sub(1) {
            return Err(UtzError::Invalid(format!(
                "package has more than {MAX_FILES} files"
            )));
        }
        let mut total = 0u64;
        for (path, bytes) in &files {
            validate_package_path(path)?;
            if path == MANIFEST_PATH {
                return Err(UtzError::Invalid(
                    "files map must not contain manifest.json".into(),
                ));
            }
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(UtzError::Invalid(format!("asset is too large: {path}")));
            }
            total = total.saturating_add(bytes.len() as u64);
        }
        if total > MAX_TOTAL_BYTES {
            return Err(UtzError::Invalid(
                "package expands beyond the size limit".into(),
            ));
        }
        manifest.update_asset_metadata(&files)?;
        manifest.validate()?;
        Ok(Self { manifest, files })
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut archive = ZipArchive::new(Cursor::new(bytes))?;
        if archive.len() > MAX_FILES {
            return Err(UtzError::Invalid(format!(
                "package has more than {MAX_FILES} files"
            )));
        }
        let mut files = BTreeMap::new();
        let mut total = 0u64;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_owned();
            validate_package_path(&name)?;
            if entry.size() > MAX_FILE_BYTES {
                return Err(UtzError::Invalid(format!("asset is too large: {name}")));
            }
            total = total.saturating_add(entry.size());
            if total > MAX_TOTAL_BYTES {
                return Err(UtzError::Invalid(
                    "package expands beyond the size limit".into(),
                ));
            }
            let mut content = Vec::with_capacity(entry.size().min(usize::MAX as u64) as usize);
            entry.read_to_end(&mut content)?;
            if files.insert(name.clone(), content).is_some() {
                return Err(UtzError::Invalid(format!("duplicate archive path: {name}")));
            }
        }
        let manifest_bytes = files
            .remove(MANIFEST_PATH)
            .ok_or_else(|| UtzError::Invalid("manifest.json is missing".into()))?;
        let manifest: UtzManifest = serde_json::from_slice(&manifest_bytes)?;
        manifest.validate()?;
        for asset in manifest.assets() {
            let content = files.get(&asset.path).ok_or_else(|| {
                UtzError::Invalid(format!("manifest asset is missing: {}", asset.path))
            })?;
            if content.len() as u64 != asset.bytes {
                return Err(UtzError::Invalid(format!(
                    "asset byte count differs: {}",
                    asset.path
                )));
            }
            if sha256_hex(content) != asset.sha256 {
                return Err(UtzError::Invalid(format!(
                    "asset checksum differs: {}",
                    asset.path
                )));
            }
        }
        Ok(Self { manifest, files })
    }

    /// Write a package directly to a seekable destination. Large media files
    /// are hashed and copied in fixed-size chunks, so peak memory stays nearly
    /// constant even when a package includes video.
    pub fn write_streaming<W, F>(
        mut manifest: UtzManifest,
        sources: BTreeMap<String, AssetSource>,
        output: W,
        mut progress: F,
    ) -> Result<W>
    where
        W: Write + Seek,
        F: FnMut(&str, u64, u64),
    {
        if sources.len() > MAX_FILES.saturating_sub(1) {
            return Err(UtzError::Invalid(format!(
                "package has more than {MAX_FILES} files"
            )));
        }

        let declared: BTreeMap<String, String> = manifest
            .assets()
            .into_iter()
            .map(|asset| (asset.path.clone(), asset.media_type.clone()))
            .collect();
        if declared.len() != manifest.assets().len() {
            return Err(UtzError::Invalid(
                "asset path is used more than once".into(),
            ));
        }
        if sources.keys().ne(declared.keys()) {
            return Err(UtzError::Invalid(
                "package sources must exactly match the manifest assets".into(),
            ));
        }

        let mut total = 0u64;
        for (path, source) in &sources {
            validate_package_path(path)?;
            if path == MANIFEST_PATH {
                return Err(UtzError::Invalid(
                    "manifest.json cannot be used as an asset".into(),
                ));
            }
            let bytes = match source {
                AssetSource::Bytes(bytes) => bytes.len() as u64,
                AssetSource::File(path) => std::fs::metadata(path)?.len(),
            };
            if bytes > MAX_FILE_BYTES {
                return Err(UtzError::Invalid(format!("asset is too large: {path}")));
            }
            total = total.saturating_add(bytes);
            if total > MAX_TOTAL_BYTES {
                return Err(UtzError::Invalid(
                    "package expands beyond the size limit".into(),
                ));
            }
        }

        let mut writer = ZipWriter::new(output);
        let base_options = SimpleFileOptions::default().unix_permissions(0o644);
        let mut metadata = BTreeMap::new();
        let mut completed = 0u64;
        let mut buffer = vec![0u8; 128 * 1024];

        for (path, source) in sources {
            let media_type = declared.get(&path).expect("source keys were checked");
            let compression = if media_type == "application/json" || media_type.starts_with("text/")
            {
                CompressionMethod::Deflated
            } else {
                // Audio, artwork and video are already compressed. Storing
                // them avoids wasted CPU and makes export much faster.
                CompressionMethod::Stored
            };
            writer.start_file(&path, base_options.compression_method(compression))?;
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
            base_options.compression_method(CompressionMethod::Deflated),
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
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }
    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
    }
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
        return Err(UtzError::Invalid(format!("unsafe package path: {path:?}")));
    }
    let parsed = Path::new(path);
    if parsed
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(UtzError::Invalid(format!("unsafe package path: {path:?}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (UtzManifest, BTreeMap<String, Vec<u8>>) {
        let manifest = UtzManifest::new(
            "org.uta.example",
            SongMetadata {
                title: "Example".into(),
                artist: "Uta".into(),
                album: None,
                language: Some("ja".into()),
                duration_seconds: 12.0,
                bpm: Some(120.0),
                key: Some("C".into()),
            },
            AudioAssets {
                instrumental: AssetRef::pending("audio/instrumental.mp3", "audio/mpeg"),
                guide_vocals: None,
                original: None,
                audio_offset_seconds: 0.0,
            },
            ChartAssets {
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
    fn package_round_trip_verifies_assets() {
        let (manifest, files) = sample();
        let package = UtzPackage::build(manifest, files).unwrap();
        let encoded = package.to_bytes().unwrap();
        let decoded = UtzPackage::from_bytes(&encoded).unwrap();
        assert_eq!(decoded.manifest().package_id, "org.uta.example");
        assert_eq!(
            decoded.file("audio/instrumental.mp3"),
            Some(b"audio".as_slice())
        );
    }

    #[test]
    fn streaming_writer_round_trip_verifies_assets() {
        let (manifest, files) = sample();
        let sources = files
            .into_iter()
            .map(|(path, bytes)| (path, AssetSource::Bytes(bytes)))
            .collect();
        let cursor =
            UtzPackage::write_streaming(manifest, sources, Cursor::new(Vec::new()), |_, _, _| {})
                .unwrap();
        let decoded = UtzPackage::from_bytes(&cursor.into_inner()).unwrap();
        assert_eq!(decoded.manifest().song.title, "Example");
        assert_eq!(
            decoded.file("audio/instrumental.mp3"),
            Some(b"audio".as_slice())
        );
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
    fn unsupported_versions_are_rejected() {
        let (mut manifest, files) = sample();
        manifest.format_version = "1.0.0".into();
        assert!(UtzPackage::build(manifest, files).is_err());
    }
}
