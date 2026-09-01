use serde::Deserialize;

use super::{
    LyricsCandidate, LyricsCandidateLocator, LyricsFetchResult, LyricsProvider,
    LyricsProviderError, LyricsSearchQuery, merge_auxiliary, parse_standard_lrc, plain_document,
    rank_candidates,
};

#[derive(Debug, Clone, Deserialize)]
struct LrclibCandidate {
    #[serde(default)]
    id: i64,
    #[serde(default, alias = "trackName")]
    track_name: String,
    #[serde(default, alias = "artistName")]
    artist_name: String,
    #[serde(default, alias = "albumName")]
    album_name: String,
    #[serde(default, alias = "duration")]
    duration_secs: f64,
    #[serde(default, alias = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(default, rename = "plainLyrics")]
    plain_lyrics: String,
}

pub(super) fn search(
    query: LyricsSearchQuery<'_>,
) -> Result<Vec<LyricsCandidate>, LyricsProviderError> {
    if query.title.trim().is_empty() || query.artist.trim().is_empty() {
        return Ok(Vec::new());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(format!("Uta-Studio/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let results = client
        .get("https://lrclib.net/api/search")
        .query(&[("track_name", query.title), ("artist_name", query.artist)])
        .send()?
        .error_for_status()?
        .json::<Vec<LrclibCandidate>>()?;

    let mut candidates = results
        .into_iter()
        .filter_map(|candidate| {
            let synced_lyrics = candidate
                .synced_lyrics
                .filter(|lyrics| !lyrics.trim().is_empty());
            let lines = candidate
                .plain_lyrics
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if lines.is_empty() && synced_lyrics.is_none() {
                return None;
            }
            Some(LyricsCandidate {
                provider: LyricsProvider::Lrclib,
                provider_id: Some(candidate.id.to_string()),
                track_name: candidate.track_name,
                artist_name: candidate.artist_name,
                album_name: candidate.album_name,
                duration_secs: candidate.duration_secs,
                has_timed_lyrics: synced_lyrics.is_some(),
                has_translation: false,
                has_romanization: false,
                loaded: true,
                lines,
                synced_lyrics: synced_lyrics.clone(),
                translation: None,
                romanization: None,
                locator: Some(LyricsCandidateLocator::Lrclib {
                    id: candidate.id,
                    synced_lyrics,
                    plain_lyrics: candidate.plain_lyrics,
                }),
                provider_score: None,
            })
        })
        .collect::<Vec<_>>();
    rank_candidates(&mut candidates, query);
    Ok(candidates)
}

pub(super) fn fetch(candidate: &LyricsCandidate) -> Result<LyricsFetchResult, LyricsProviderError> {
    let Some(LyricsCandidateLocator::Lrclib {
        id,
        synced_lyrics,
        plain_lyrics,
    }) = candidate.locator.as_ref()
    else {
        return Err(LyricsProviderError::InvalidData(
            "LRCLIB candidate has a non-LRCLIB locator".into(),
        ));
    };
    let document = if let Some(synced) = synced_lyrics.as_deref() {
        parse_standard_lrc(synced)?
    } else {
        plain_document(plain_lyrics)
    };
    Ok(LyricsFetchResult {
        provider: LyricsProvider::Lrclib,
        provider_id: Some(id.to_string()),
        document: merge_auxiliary(document, None, None),
    })
}
