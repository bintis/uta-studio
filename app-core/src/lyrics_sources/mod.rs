use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

use crate::lrc;
use crate::song::Song;

mod kugou;
mod lrclib;
mod netease;
mod parser;
mod qq;
mod tripledes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LyricsProvider {
    Lrclib,
    QqMusic,
    Kugou,
    Netease,
}

impl LyricsProvider {
    pub const ALL: [Self; 4] = [Self::Lrclib, Self::QqMusic, Self::Kugou, Self::Netease];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Lrclib => "LRCLIB",
            Self::QqMusic => "QQ Music",
            Self::Kugou => "Kugou",
            Self::Netease => "NetEase",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum LyricsCandidateLocator {
    Lrclib {
        id: i64,
        synced_lyrics: Option<String>,
        plain_lyrics: String,
    },
    QqMusic {
        id: String,
        mid: Option<String>,
    },
    Kugou {
        id: String,
        hash: String,
    },
    Netease {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LyricsCandidate {
    pub provider: LyricsProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub track_name: String,
    pub artist_name: String,
    pub album_name: String,
    pub duration_secs: f64,
    pub has_timed_lyrics: bool,
    pub has_translation: bool,
    pub has_romanization: bool,
    pub loaded: bool,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced_lyrics: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub romanization: Option<String>,
    #[serde(skip)]
    #[ts(skip)]
    locator: Option<LyricsCandidateLocator>,
    #[serde(skip)]
    #[ts(skip)]
    provider_score: Option<f64>,
}

impl LyricsCandidate {
    fn artist_display(&self) -> &str {
        &self.artist_name
    }
}

/// Compatibility alias for older callers. New code should use [`LyricsCandidate`].
pub type LrclibCandidate = LyricsCandidate;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LyricsProviderFailure {
    pub provider: LyricsProvider,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LyricsSearchResult {
    pub candidates: Vec<LyricsCandidate>,
    pub provider_errors: Vec<LyricsProviderFailure>,
}

#[derive(Debug, Clone, PartialEq)]
struct ProviderLyricLine {
    start: Duration,
    end: Duration,
    text: String,
    translation: Option<String>,
    romanization: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct ProviderLyricDocument {
    lines: Vec<ProviderLyricLine>,
}

impl ProviderLyricDocument {
    fn plain_lines(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(|line| line.text.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect()
    }

    fn to_lrc(&self) -> String {
        self.lines
            .iter()
            .filter_map(|line| {
                let text = line.text.replace(['\r', '\n'], " ").trim().to_string();
                (!text.is_empty()).then(|| format!("[{}]{text}", format_lrc_timestamp(line.start)))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn translation_text(&self) -> Option<String> {
        joined_auxiliary(&self.lines, |line| line.translation.as_deref())
    }

    fn romanization_text(&self) -> Option<String> {
        joined_auxiliary(&self.lines, |line| line.romanization.as_deref())
    }
}

fn joined_auxiliary(
    lines: &[ProviderLyricLine],
    select: impl Fn(&ProviderLyricLine) -> Option<&str>,
) -> Option<String> {
    let text = lines
        .iter()
        .filter_map(select)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn format_lrc_timestamp(value: Duration) -> String {
    let centiseconds = value.as_millis().saturating_add(5) / 10;
    format!(
        "{:02}:{:02}.{:02}",
        centiseconds / 6_000,
        centiseconds / 100 % 60,
        centiseconds % 100
    )
}

fn parse_standard_lrc(text: &str) -> Result<ProviderLyricDocument, LyricsProviderError> {
    let parsed = lrc::parse_lrc(text).map_err(LyricsProviderError::Parse)?;
    Ok(ProviderLyricDocument {
        lines: parsed
            .segments
            .into_iter()
            .map(|segment| ProviderLyricLine {
                start: Duration::from_secs_f64(segment.start.max(0.0)),
                end: Duration::from_secs_f64(segment.end.max(segment.start)),
                text: segment.text,
                translation: None,
                romanization: None,
            })
            .collect(),
    })
}

fn plain_document(text: &str) -> ProviderLyricDocument {
    ProviderLyricDocument {
        lines: text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .enumerate()
            .map(|(index, text)| {
                let start = Duration::from_secs(index as u64 * 4);
                ProviderLyricLine {
                    start,
                    end: start + Duration::from_secs(4),
                    text: text.to_string(),
                    translation: None,
                    romanization: None,
                }
            })
            .collect(),
    }
}

fn merge_auxiliary(
    mut original: ProviderLyricDocument,
    translation: Option<&ProviderLyricDocument>,
    romanization: Option<&ProviderLyricDocument>,
) -> ProviderLyricDocument {
    for line in &mut original.lines {
        line.translation = translation.and_then(|document| nearest_line_text(document, line.start));
        line.romanization =
            romanization.and_then(|document| nearest_line_text(document, line.start));
    }
    original
}

fn nearest_line_text(document: &ProviderLyricDocument, start: Duration) -> Option<String> {
    document
        .lines
        .iter()
        .min_by_key(|line| line.start.abs_diff(start))
        .filter(|line| line.start.abs_diff(start) <= Duration::from_millis(350))
        .map(|line| line.text.clone())
}

#[derive(Debug, Error)]
enum LyricsProviderError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider returned invalid data: {0}")]
    InvalidData(String),
    #[error("provider request failed: {0}")]
    Api(String),
    #[error("lyrics decode failed: {0}")]
    Decode(String),
    #[error("lyrics parse failed: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Copy)]
struct LyricsSearchQuery<'a> {
    title: &'a str,
    artist: &'a str,
    album: Option<&'a str>,
    duration: Option<Duration>,
}

impl LyricsSearchQuery<'_> {
    fn keyword(&self) -> String {
        if self.artist.trim().is_empty() {
            self.title.trim().to_owned()
        } else {
            format!("{} - {}", self.artist.trim(), self.title.trim())
        }
    }
}

#[derive(Debug)]
struct LyricsFetchResult {
    provider: LyricsProvider,
    provider_id: Option<String>,
    document: ProviderLyricDocument,
}

fn search_provider(
    provider: LyricsProvider,
    query: LyricsSearchQuery<'_>,
) -> Result<Vec<LyricsCandidate>, LyricsProviderError> {
    match provider {
        LyricsProvider::Lrclib => lrclib::search(query),
        LyricsProvider::QqMusic => qq::search(query),
        LyricsProvider::Kugou => kugou::search(query),
        LyricsProvider::Netease => netease::search(query),
    }
}

fn fetch_candidate(candidate: &LyricsCandidate) -> Result<LyricsFetchResult, LyricsProviderError> {
    match candidate.provider {
        LyricsProvider::Lrclib => lrclib::fetch(candidate),
        LyricsProvider::QqMusic => qq::fetch(candidate),
        LyricsProvider::Kugou => kugou::fetch(candidate),
        LyricsProvider::Netease => netease::fetch(candidate),
    }
}

pub fn lyrics_candidates(song: &Song) -> LyricsSearchResult {
    let title = song.title.trim();
    let artist = song.artist.trim();
    if title.is_empty() || artist.is_empty() || artist == "Unknown Artist" {
        return LyricsSearchResult::default();
    }
    let query = LyricsSearchQuery {
        title,
        artist,
        album: (!song.album.trim().is_empty()).then_some(song.album.as_str()),
        duration: (song.duration_secs.is_finite() && song.duration_secs > 0.0)
            .then(|| Duration::from_secs_f64(song.duration_secs)),
    };

    let results = thread::scope(|scope| {
        LyricsProvider::ALL
            .into_iter()
            .map(|provider| (provider, scope.spawn(move || search_provider(provider, query))))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(provider, handle)| {
                let result = handle.join().unwrap_or_else(|_| {
                    Err(LyricsProviderError::Api(
                        "provider worker panicked".to_string(),
                    ))
                });
                (provider, result)
            })
            .collect::<Vec<_>>()
    });

    let mut candidates = Vec::new();
    let mut provider_errors = Vec::new();
    for (provider, result) in results {
        match result {
            Ok(mut provider_candidates) => candidates.append(&mut provider_candidates),
            Err(error) => provider_errors.push(LyricsProviderFailure {
                provider,
                message: error.to_string(),
            }),
        }
    }
    rank_candidates(&mut candidates, query);
    candidates.truncate(40);
    LyricsSearchResult {
        candidates,
        provider_errors,
    }
}

pub fn fetch_lyrics_candidate(candidate: &LyricsCandidate) -> Result<LyricsCandidate, String> {
    let fetched = fetch_candidate(candidate).map_err(|error| error.to_string())?;
    let mut loaded = candidate.clone();
    loaded.provider = fetched.provider;
    loaded.provider_id = fetched.provider_id;
    loaded.lines = fetched.document.plain_lines();
    loaded.synced_lyrics = if loaded.has_timed_lyrics {
        let value = fetched.document.to_lrc();
        (!value.is_empty()).then_some(value)
    } else {
        None
    };
    loaded.translation = fetched.document.translation_text();
    loaded.romanization = fetched.document.romanization_text();
    loaded.has_translation = loaded.translation.is_some();
    loaded.has_romanization = loaded.romanization.is_some();
    loaded.loaded = !loaded.lines.is_empty() || loaded.synced_lyrics.is_some();
    if !loaded.loaded {
        return Err(format!(
            "{} returned no usable lyrics",
            candidate.provider.display_name()
        ));
    }
    Ok(loaded)
}

pub fn lrclib_candidates(song: &Song) -> Vec<LyricsCandidate> {
    let query = LyricsSearchQuery {
        title: &song.title,
        artist: &song.artist,
        album: (!song.album.trim().is_empty()).then_some(song.album.as_str()),
        duration: (song.duration_secs.is_finite() && song.duration_secs > 0.0)
            .then(|| Duration::from_secs_f64(song.duration_secs)),
    };
    lrclib::search(query).unwrap_or_default()
}

fn rank_candidates(candidates: &mut [LyricsCandidate], query: LyricsSearchQuery<'_>) {
    let title = normalize(query.title);
    let artist = normalize(query.artist);
    let album = query.album.map(normalize);
    let expected_duration = query.duration.map(|duration| duration.as_millis() as i128);
    candidates.sort_by(|left, right| {
        candidate_score(right, &title, &artist, album.as_deref(), expected_duration).total_cmp(
            &candidate_score(left, &title, &artist, album.as_deref(), expected_duration),
        )
    });
}

fn candidate_score(
    candidate: &LyricsCandidate,
    title: &str,
    artist: &str,
    album: Option<&str>,
    expected_duration_ms: Option<i128>,
) -> f64 {
    let title_score = text_similarity(title, &normalize(&candidate.track_name));
    let artist_score = if artist.is_empty() {
        1.0
    } else {
        text_similarity(artist, &normalize(candidate.artist_display()))
    };
    let album_score = album.map_or(1.0, |album| {
        if candidate.album_name.is_empty() {
            0.6
        } else {
            text_similarity(album, &normalize(&candidate.album_name))
        }
    });
    let duration_score = match (expected_duration_ms, candidate_duration_ms(candidate)) {
        (Some(expected), Some(actual)) => {
            let delta = (expected - i128::from(actual)).unsigned_abs() as f64;
            (1.0 - delta / 15_000.0).clamp(0.0, 1.0)
        }
        _ => 0.7,
    };
    let timed_bonus = if candidate.has_timed_lyrics { 0.08 } else { 0.0 };
    let provider_bonus = candidate.provider_score.unwrap_or_default().clamp(0.0, 100.0) / 1000.0;
    title_score * 0.45
        + artist_score * 0.25
        + album_score * 0.12
        + duration_score * 0.18
        + timed_bonus
        + provider_bonus
}

fn candidate_duration_ms(candidate: &LyricsCandidate) -> Option<u64> {
    (candidate.duration_secs.is_finite() && candidate.duration_secs > 0.0)
        .then(|| (candidate.duration_secs * 1000.0).round() as u64)
}

fn normalize(value: &str) -> String {
    value
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .map(|ch| match ch {
            '（' => '(',
            '）' => ')',
            '：' => ':',
            '！' => '!',
            '？' => '?',
            '／' => '/',
            '＆' => '&',
            '－' => '-',
            '＜' => '<',
            '＞' => '>',
            '［' => '[',
            '］' => ']',
            other => other,
        })
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

fn text_similarity(left: &str, right: &str) -> f64 {
    if left == right {
        return 1.0;
    }
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0_usize; right.len() + 1];
    for (left_index, left_char) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right.iter().enumerate() {
            current[right_index + 1] = if left_char == right_char {
                previous[right_index]
            } else {
                1 + previous[right_index]
                    .min(previous[right_index + 1])
                    .min(current[right_index])
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    1.0 - previous[right.len()] as f64 / left.len().max(right.len()) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(title: &str, artist: &str, duration_secs: f64) -> LyricsCandidate {
        LyricsCandidate {
            provider: LyricsProvider::Lrclib,
            provider_id: Some("1".into()),
            track_name: title.into(),
            artist_name: artist.into(),
            album_name: "Album".into(),
            duration_secs,
            has_timed_lyrics: true,
            has_translation: false,
            has_romanization: false,
            loaded: true,
            lines: vec!["test".into()],
            synced_lyrics: Some("[00:01.00]test".into()),
            translation: None,
            romanization: None,
            locator: None,
            provider_score: None,
        }
    }

    #[test]
    fn ranking_prefers_metadata_and_duration_match() {
        let query = LyricsSearchQuery {
            title: "Song",
            artist: "Artist",
            album: Some("Album"),
            duration: Some(Duration::from_secs(200)),
        };
        let mut candidates = vec![
            candidate("Wrong", "Artist", 200.0),
            candidate("Song", "Artist", 260.0),
            candidate("Song", "Artist", 201.0),
        ];
        rank_candidates(&mut candidates, query);
        assert_eq!(candidates[0].duration_secs, 201.0);
    }

    #[test]
    fn lrc_serialization_keeps_line_timing() {
        let document = ProviderLyricDocument {
            lines: vec![ProviderLyricLine {
                start: Duration::from_millis(61_230),
                end: Duration::from_millis(65_000),
                text: "hello".into(),
                translation: None,
                romanization: None,
            }],
        };
        assert_eq!(document.to_lrc(), "[01:01.23]hello");
    }
}
