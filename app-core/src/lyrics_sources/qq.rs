use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::parser::{decrypt_qrc_cloud, parse_qrc};
use super::{
    LyricsCandidate, LyricsCandidateLocator, LyricsFetchResult, LyricsProvider,
    LyricsProviderError, LyricsSearchQuery, merge_auxiliary, rank_candidates,
};

struct QqClient {
    http: Client,
    comm: Value,
}

impl QqClient {
    fn new() -> Result<Self, LyricsProviderError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("okhttp/3.14.9")
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    reqwest::header::HeaderValue::from_static("application/json"),
                );
                headers.insert(
                    reqwest::header::COOKIE,
                    reqwest::header::HeaderValue::from_static("tmeLoginType=-1;"),
                );
                headers
            })
            .build()?;
        let mut client = Self {
            http,
            comm: json!({
                "ct": 11,
                "cv": "1003006",
                "v": "1003006",
                "os_ver": "15",
                "phonetype": "24122RKC7C",
                "rom": "Redmi/miro/miro:15/AE3A.240806.005/OS2.0.105.0.VOMCNXM:user/release-keys",
                "tmeAppID": "qqmusiclight",
                "nettype": "NETWORK_WIFI",
                "udid": "0"
            }),
        };
        let data = client.request_raw(
            "GetSession",
            "music.getSession.session",
            json!({"caller":0,"uid":"0","vkey":0}),
        )?;
        let session = data.get("session").ok_or_else(|| {
            LyricsProviderError::InvalidData("QQ Music session response has no session".into())
        })?;
        let comm = client.comm.as_object_mut().ok_or_else(|| {
            LyricsProviderError::InvalidData("QQ Music comm is not an object".into())
        })?;
        for key in ["uid", "sid", "userip"] {
            if let Some(value) = session.get(key) {
                comm.insert(key.to_owned(), value.clone());
            }
        }
        Ok(client)
    }

    fn request(
        &self,
        method: &str,
        module: &str,
        param: Value,
    ) -> Result<Value, LyricsProviderError> {
        self.request_raw(method, module, param)
    }

    fn request_raw(
        &self,
        method: &str,
        module: &str,
        param: Value,
    ) -> Result<Value, LyricsProviderError> {
        let response = self
            .http
            .post("https://u.y.qq.com/cgi-bin/musicu.fcg")
            .json(&json!({
                "comm": self.comm,
                "request": {"method": method, "module": module, "param": param}
            }))
            .send()?
            .error_for_status()?
            .json::<Value>()?;
        let root_code = response.get("code").and_then(Value::as_i64).unwrap_or(-1);
        let request = response.get("request").ok_or_else(|| {
            LyricsProviderError::InvalidData("QQ Music response has no request object".into())
        })?;
        let request_code = request.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if root_code != 0 || request_code != 0 {
            return Err(LyricsProviderError::Api(format!(
                "QQ Music API error: root={root_code}, request={request_code}"
            )));
        }
        request
            .get("data")
            .cloned()
            .ok_or_else(|| LyricsProviderError::InvalidData("QQ Music response has no data".into()))
    }
}

pub(super) fn search(
    query: LyricsSearchQuery<'_>,
) -> Result<Vec<LyricsCandidate>, LyricsProviderError> {
    let client = QqClient::new()?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    let search_id =
        18_014_398_509_481_984_u64 + ((now_ms % 4_194_304) << 32) + now_ms % 86_400_000;
    let data = client.request(
        "DoSearchForQQMusicLite",
        "music.search.SearchCgiService",
        json!({
            "search_id": search_id.to_string(),
            "remoteplace": "search.android.keyboard",
            "query": query.keyword(),
            "search_type": 0,
            "num_per_page": 20,
            "page_num": 1,
            "highlight": 0,
            "nqc_flag": 0,
            "page_id": 1,
            "grp": 1
        }),
    )?;
    let songs = data
        .pointer("/body/item_song")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut candidates = songs
        .into_iter()
        .filter_map(|song| {
            let id = value_string(song.get("id")?)?;
            let title = song.get("title")?.as_str()?.to_owned();
            let artists = song
                .get("singer")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|artist| artist.get("name").and_then(Value::as_str))
                .filter(|artist| !artist.is_empty())
                .collect::<Vec<_>>()
                .join(" / ");
            let album = song
                .pointer("/album/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let duration_secs = song
                .get("interval")
                .and_then(Value::as_u64)
                .unwrap_or_default() as f64;
            Some(LyricsCandidate {
                provider: LyricsProvider::QqMusic,
                provider_id: Some(id.clone()),
                track_name: title,
                artist_name: artists,
                album_name: album,
                duration_secs,
                has_timed_lyrics: true,
                has_translation: true,
                has_romanization: true,
                loaded: false,
                lines: Vec::new(),
                synced_lyrics: None,
                translation: None,
                romanization: None,
                locator: Some(LyricsCandidateLocator::QqMusic {
                    id,
                    mid: song.get("mid").and_then(Value::as_str).map(str::to_owned),
                }),
                provider_score: None,
            })
        })
        .collect::<Vec<_>>();
    rank_candidates(&mut candidates, query);
    Ok(candidates)
}

pub(super) fn fetch(candidate: &LyricsCandidate) -> Result<LyricsFetchResult, LyricsProviderError> {
    let Some(LyricsCandidateLocator::QqMusic { id, mid }) = candidate.locator.as_ref() else {
        return Err(LyricsProviderError::InvalidData(
            "QQ Music candidate has a non-QQ locator".into(),
        ));
    };
    let _ = mid;
    let song_id = id.parse::<u64>().map_err(|error| {
        LyricsProviderError::InvalidData(format!("invalid QQ song id {id}: {error}"))
    })?;
    let client = QqClient::new()?;
    let title = BASE64.encode(candidate.track_name.as_bytes());
    let album = BASE64.encode(candidate.album_name.as_bytes());
    let artist = BASE64.encode(candidate.artist_name.as_bytes());
    let data = client.request(
        "GetPlayLyricInfo",
        "music.musichallSong.PlayLyricInfo",
        json!({
            "albumName": album,
            "crypt": 1,
            "ct": 19,
            "cv": 2111,
            "interval": candidate.duration_secs.max(0.0).round() as u64,
            "lrc_t": 0,
            "qrc": 1,
            "qrc_t": 0,
            "roma": 1,
            "roma_t": 0,
            "singerName": artist,
            "songID": song_id,
            "songName": title,
            "trans": 1,
            "trans_t": 0,
            "type": 0
        }),
    )?;

    let raw_original = decode_qq_field(&data, "lyric", true)?
        .ok_or_else(|| LyricsProviderError::Api("QQ Music returned no lyrics".into()))?;
    let original = parse_qrc(&raw_original)?;
    let raw_translation = decode_qq_field(&data, "trans", false)?;
    let raw_romanization = decode_qq_field(&data, "roma", false)?;
    let translation = raw_translation.as_deref().map(parse_qrc).transpose()?;
    let romanization = raw_romanization.as_deref().map(parse_qrc).transpose()?;
    let document = merge_auxiliary(original, translation.as_ref(), romanization.as_ref());

    Ok(LyricsFetchResult {
        provider: LyricsProvider::QqMusic,
        provider_id: Some(id.clone()),
        document,
    })
}

fn decode_qq_field(
    data: &Value,
    field: &str,
    original: bool,
) -> Result<Option<String>, LyricsProviderError> {
    let encrypted = data.get(field).and_then(Value::as_str).unwrap_or_default();
    let timing = if original {
        data.get("qrc_t")
            .and_then(Value::as_i64)
            .filter(|value| *value != 0)
            .or_else(|| data.get("lrc_t").and_then(Value::as_i64))
    } else {
        data.get(format!("{field}_t")).and_then(Value::as_i64)
    };
    if encrypted.is_empty() || timing.unwrap_or_default() == 0 {
        return Ok(None);
    }
    decrypt_qrc_cloud(encrypted).map(Some)
}

fn value_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}
