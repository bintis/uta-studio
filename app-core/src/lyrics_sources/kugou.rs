use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use md5::{Digest, Md5};
use reqwest::blocking::Client;
use serde_json::Value;

use super::parser::{decrypt_krc, parse_krc};
use super::{
    LyricsCandidate, LyricsCandidateLocator, LyricsFetchResult, LyricsProvider,
    LyricsProviderError, LyricsSearchQuery, parse_standard_lrc, plain_document, rank_candidates,
};

const SIGN_SECRET: &str = "LnT6xpN3khm36zse0QzvmgTZ3waWdRSA";

pub(super) fn search(
    query: LyricsSearchQuery<'_>,
) -> Result<Vec<LyricsCandidate>, LyricsProviderError> {
    let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
    let params = BTreeMap::from([
        ("sorttype".into(), "0".into()),
        ("keyword".into(), query.keyword()),
        ("pagesize".into(), "20".into()),
        ("page".into(), "1".into()),
    ]);
    let data = kugou_request(
        &client,
        "https://complexsearch.kugou.com/v2/search/song",
        params,
        "SearchSong",
    )?;
    let songs = data
        .pointer("/data/lists")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut candidates = songs
        .into_iter()
        .filter_map(|song| {
            let id = value_string(song.get("ID")?)?;
            let hash = song.get("FileHash")?.as_str()?.to_owned();
            let track_name = song.get("SongName")?.as_str()?.to_owned();
            let artist_name = song
                .get("Singers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|artist| artist.get("name").and_then(Value::as_str))
                .filter(|artist| !artist.is_empty())
                .collect::<Vec<_>>()
                .join(" / ");
            let album_name = song
                .get("AlbumName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let duration_secs = song
                .get("Duration")
                .and_then(Value::as_u64)
                .unwrap_or_default() as f64;
            Some(LyricsCandidate {
                provider: LyricsProvider::Kugou,
                provider_id: Some(id.clone()),
                track_name,
                artist_name,
                album_name,
                duration_secs,
                has_timed_lyrics: true,
                has_translation: true,
                has_romanization: true,
                loaded: false,
                lines: Vec::new(),
                synced_lyrics: None,
                translation: None,
                romanization: None,
                locator: Some(LyricsCandidateLocator::Kugou { id, hash }),
                provider_score: None,
            })
        })
        .collect::<Vec<_>>();
    rank_candidates(&mut candidates, query);
    Ok(candidates)
}

pub(super) fn fetch(candidate: &LyricsCandidate) -> Result<LyricsFetchResult, LyricsProviderError> {
    let Some(LyricsCandidateLocator::Kugou { id, hash }) = candidate.locator.as_ref() else {
        return Err(LyricsProviderError::InvalidData(
            "Kugou candidate has a non-Kugou locator".into(),
        ));
    };
    let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
    let duration_ms = if candidate.duration_secs.is_finite() && candidate.duration_secs > 0.0 {
        (candidate.duration_secs * 1000.0).round() as u64
    } else {
        0
    };
    let search_params = BTreeMap::from([
        ("album_audio_id".into(), id.clone()),
        ("duration".into(), duration_ms.to_string()),
        ("hash".into(), hash.clone()),
        (
            "keyword".into(),
            format!("{} - {}", candidate.artist_name, candidate.track_name),
        ),
        ("lrctxt".into(), "1".into()),
        ("man".into(), "no".into()),
    ]);
    let list = kugou_request(
        &client,
        "https://lyrics.kugou.com/v1/search",
        search_params,
        "Lyric",
    )?;
    let lyric = list
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .ok_or_else(|| LyricsProviderError::Api("Kugou returned no lyric candidates".into()))?;
    let lyric_id = value_string(
        lyric
            .get("id")
            .ok_or_else(|| LyricsProviderError::InvalidData("Kugou lyric has no id".into()))?,
    )
    .ok_or_else(|| LyricsProviderError::InvalidData("invalid Kugou lyric id".into()))?;
    let access_key = lyric
        .get("accesskey")
        .and_then(Value::as_str)
        .ok_or_else(|| LyricsProviderError::InvalidData("Kugou lyric has no accesskey".into()))?;
    let provider_score = lyric.get("score").and_then(Value::as_f64);

    let download = kugou_request(
        &client,
        "https://lyrics.kugou.com/download",
        BTreeMap::from([
            ("accesskey".into(), access_key.to_owned()),
            ("charset".into(), "utf8".into()),
            ("client".into(), "mobi".into()),
            ("fmt".into(), "krc".into()),
            ("id".into(), lyric_id.clone()),
            ("ver".into(), "1".into()),
        ]),
        "Lyric",
    )?;
    let content = download
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            LyricsProviderError::InvalidData("Kugou lyric download has no content".into())
        })?;
    let content_type = download
        .get("contenttype")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let raw_original = if content_type == 2 {
        String::from_utf8(BASE64.decode(content).map_err(|error| {
            LyricsProviderError::Decode(format!("invalid Kugou lyric base64: {error}"))
        })?)
        .map_err(|error| {
            LyricsProviderError::Decode(format!("Kugou lyric is not UTF-8: {error}"))
        })?
    } else {
        decrypt_krc(content)?
    };
    let document = if content_type == 2 {
        if raw_original.contains('[') && raw_original.contains(']') {
            parse_standard_lrc(&raw_original).unwrap_or_else(|_| plain_document(&raw_original))
        } else {
            plain_document(&raw_original)
        }
    } else {
        parse_krc(&raw_original)?
    };

    let _ = provider_score;
    Ok(LyricsFetchResult {
        provider: LyricsProvider::Kugou,
        provider_id: Some(lyric_id),
        document,
    })
}

fn kugou_request(
    client: &Client,
    url: &str,
    caller_params: BTreeMap<String, String>,
    module: &str,
) -> Result<Value, LyricsProviderError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let mid = md5_hex(now.as_millis().to_string().as_bytes());
    let mut params = if module == "Lyric" {
        BTreeMap::from([
            ("appid".into(), "3116".into()),
            ("clientver".into(), "11070".into()),
        ])
    } else {
        BTreeMap::from([
            ("userid".into(), "0".into()),
            ("appid".into(), "3116".into()),
            ("token".into(), String::new()),
            ("clienttime".into(), now.as_secs().to_string()),
            ("iscorrection".into(), "1".into()),
            ("uuid".into(), "-".into()),
            ("mid".into(), mid.clone()),
            ("dfid".into(), "-".into()),
            ("clientver".into(), "11070".into()),
            ("platform".into(), "AndroidFilter".into()),
        ])
    };
    params.extend(caller_params);
    let signature_body = params
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<String>();
    let signature = md5_hex(format!("{SIGN_SECRET}{signature_body}{SIGN_SECRET}").as_bytes());
    params.insert("signature".into(), signature);

    let response = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            format!("Android14-1070-11070-201-0-{module}-wifi"),
        )
        .header("KG-Rec", "1")
        .header("KG-RC", "1")
        .header("KG-CLIENTTIMEMS", now.as_millis().to_string())
        .header("mid", mid)
        .query(&params)
        .send()?
        .error_for_status()?
        .json::<Value>()?;
    let code = response
        .get("error_code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if code != 0 && code != 200 {
        return Err(LyricsProviderError::Api(format!(
            "Kugou API error {code}: {}",
            response
                .get("error_msg")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        )));
    }
    Ok(response)
}

fn md5_hex(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn value_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}
