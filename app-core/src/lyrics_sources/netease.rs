// SPDX-FileCopyrightText: Copyright (C) 2024-2025 沉默の金 <cmzj@cmzj.org>
// SPDX-License-Identifier: GPL-3.0-only
//
// Rust adaptation of LDDC's NetEase Cloud Music EAPI lyric provider.

use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aes::Aes128;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use md5::{Digest, Md5};
use reqwest::blocking::Client;
use serde_json::{Map, Value, json};

use super::parser::parse_yrc;
use super::{
    LyricsCandidate, LyricsCandidateLocator, LyricsFetchResult, LyricsProvider,
    LyricsProviderError, LyricsSearchQuery, ProviderLyricDocument, merge_auxiliary,
    parse_standard_lrc, plain_document, rank_candidates,
};

const EAPI_KEY: &[u8; 16] = b"e82ckenh8dichen8";
const DEVICE_ID_XOR_KEY: &str = "3go8&$8*3*3h0k(2)2";
const DEVICE_ID: &str = "AA9955F5FE37BA7EAF48F8EF0C9966B28293CC8D6415CCD93549";

struct NeteaseClient {
    http: Client,
    cookies: BTreeMap<String, String>,
}

impl NeteaseClient {
    fn new() -> Result<Self, LyricsProviderError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/537.36 Safari/537.36 Chrome/91.0.4472.164 NeteaseMusicDesktop/3.1.3.203419",
            )
            .build()?;
        let now_ms = now_millis();
        let seed =
            blake3::hash(format!("rain-net-ease:{now_ms}:{}", std::process::id()).as_bytes())
                .to_hex()
                .to_string();
        let client_sign = format!(
            "AA:BB:CC:DD:EE:FF@@@RAINNE01@@@@@@{}",
            &seed[..64.min(seed.len())]
        );
        let mut cookies = BTreeMap::from([
            ("WEVNSM".into(), "1.0.0".into()),
            ("os".into(), "pc".into()),
            ("deviceId".into(), DEVICE_ID.into()),
            (
                "osver".into(),
                "Microsoft-Windows-10--build-22600-64bit".into(),
            ),
            ("clientSign".into(), client_sign),
            ("channel".into(), "netease".into()),
            ("mode".into(), "MS-iCraft B760M WIFI".into()),
            ("appver".into(), "3.1.3.203419".into()),
            ("WNMCID".into(), format!("rainne.{now_ms}.01.0")),
        ]);
        let mut client = Self {
            http,
            cookies: cookies.clone(),
        };
        let path = "/eapi/register/anonimous";
        let params = json!({
            "username": anonymous_username(DEVICE_ID),
            "e_r": true,
            "header": header_json(&cookies)
        });
        let response = client.request_with_cookies(path, params, &cookies)?;
        if response.get("code").and_then(Value::as_i64).unwrap_or(200) != 200 {
            return Err(LyricsProviderError::Api(format!(
                "NetEase anonymous login failed: {}",
                response
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
            )));
        }
        client.cookies.append(&mut cookies);
        Ok(client)
    }

    fn request(&self, path: &str, mut params: Value) -> Result<Value, LyricsProviderError> {
        let object = params.as_object_mut().ok_or_else(|| {
            LyricsProviderError::InvalidData("NetEase params are not an object".into())
        })?;
        object.insert("e_r".into(), Value::Bool(true));
        object.insert("header".into(), Value::String(header_json(&self.cookies)));
        let response = self.request_with_cookies(path, params, &self.cookies)?;
        let code = response.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if code != 200 {
            return Err(LyricsProviderError::Api(format!(
                "NetEase API error {code}: {}",
                response
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
            )));
        }
        Ok(response)
    }

    fn request_with_cookies(
        &self,
        path: &str,
        params: Value,
        cookies: &BTreeMap<String, String>,
    ) -> Result<Value, LyricsProviderError> {
        let api_path = path.replace("eapi", "api");
        let body = eapi_encrypt(api_path.as_bytes(), &params)?;
        let response = self
            .http
            .post(format!("https://interface.music.163.com{path}"))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .header(reqwest::header::ACCEPT, "*/*")
            .header(reqwest::header::COOKIE, cookie_header(cookies))
            .header(
                "mconfig-info",
                r#"{"IuRPVVmc3WWul9fT":{"version":733184,"appver":"3.1.3.203419"}}"#,
            )
            .header(reqwest::header::ORIGIN, "orpheus://orpheus")
            .body(body)
            .send()?
            .error_for_status()?;
        let bytes = response.bytes()?;
        let decrypted = aes_ecb_decrypt(&bytes, EAPI_KEY)?;
        serde_json::from_slice(&decrypted).map_err(|error| {
            LyricsProviderError::InvalidData(format!("invalid NetEase JSON: {error}"))
        })
    }
}

pub(super) fn search(
    query: LyricsSearchQuery<'_>,
) -> Result<Vec<LyricsCandidate>, LyricsProviderError> {
    let client = NeteaseClient::new()?;
    let data = client.request(
        "/eapi/search/song/list/page",
        json!({
            "limit": "20",
            "offset": "0",
            "keyword": query.keyword(),
            "scene": "NORMAL",
            "needCorrect": "true"
        }),
    )?;
    let songs = data
        .pointer("/data/resources")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut candidates = songs
        .into_iter()
        .filter_map(|resource| {
            let song = resource.pointer("/baseInfo/simpleSongData")?;
            let id = value_string(song.get("id")?)?;
            let track_name = song.get("name")?.as_str()?.to_owned();
            let artist_name = song
                .get("ar")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|artist| artist.get("name").and_then(Value::as_str))
                .filter(|artist| !artist.is_empty())
                .collect::<Vec<_>>()
                .join(" / ");
            let album_name = song
                .pointer("/al/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let duration_secs = song
                .get("dt")
                .and_then(Value::as_u64)
                .map(|milliseconds| milliseconds as f64 / 1000.0)
                .unwrap_or_default();
            Some(LyricsCandidate {
                provider: LyricsProvider::Netease,
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
                locator: Some(LyricsCandidateLocator::Netease { id }),
                provider_score: None,
            })
        })
        .collect::<Vec<_>>();
    rank_candidates(&mut candidates, query);
    Ok(candidates)
}

pub(super) fn fetch(candidate: &LyricsCandidate) -> Result<LyricsFetchResult, LyricsProviderError> {
    let Some(LyricsCandidateLocator::Netease { id }) = candidate.locator.as_ref() else {
        return Err(LyricsProviderError::InvalidData(
            "NetEase candidate has a non-NetEase locator".into(),
        ));
    };
    let numeric_id = id.parse::<u64>().map_err(|error| {
        LyricsProviderError::InvalidData(format!("invalid NetEase id {id}: {error}"))
    })?;
    let client = NeteaseClient::new()?;
    let data = client.request(
        "/eapi/song/lyric/v1",
        json!({"id": numeric_id, "lv":"-1", "tv":"-1", "rv":"-1", "yv":"-1"}),
    )?;

    let yrc = lyric_field(&data, "yrc");
    let lrc = lyric_field(&data, "lrc");
    let raw_original = yrc
        .clone()
        .into_iter()
        .chain(lrc.clone())
        .find(|value| !value.trim().is_empty())
        .ok_or_else(|| LyricsProviderError::Api("NetEase returned no lyrics".into()))?;
    let original = if yrc.as_deref().is_some_and(|value| !value.trim().is_empty()) {
        parse_yrc(&raw_original)?
    } else if raw_original.contains('[') && raw_original.contains(']') {
        parse_standard_lrc(&raw_original)?
    } else {
        plain_document(&raw_original)
    };
    let raw_translation = lyric_field(&data, "tlyric").filter(|value| !value.trim().is_empty());
    let raw_romanization = lyric_field(&data, "romalrc").filter(|value| !value.trim().is_empty());
    let translation = raw_translation
        .as_deref()
        .map(parse_auxiliary)
        .transpose()?;
    let romanization = raw_romanization
        .as_deref()
        .map(parse_auxiliary)
        .transpose()?;
    let document = merge_auxiliary(original, translation.as_ref(), romanization.as_ref());

    Ok(LyricsFetchResult {
        provider: LyricsProvider::Netease,
        provider_id: Some(id.clone()),
        document,
    })
}

fn parse_auxiliary(text: &str) -> Result<ProviderLyricDocument, LyricsProviderError> {
    if text.contains('[') && text.contains(']') {
        parse_standard_lrc(text)
    } else {
        Ok(plain_document(text))
    }
}

fn lyric_field(data: &Value, key: &str) -> Option<String> {
    data.get(key)?.get("lyric")?.as_str().map(str::to_owned)
}

fn eapi_encrypt(path: &[u8], params: &Value) -> Result<String, LyricsProviderError> {
    let params_bytes = serde_json::to_vec(params).map_err(|error| {
        LyricsProviderError::InvalidData(format!("NetEase params encode failed: {error}"))
    })?;
    let mut sign_source = Vec::new();
    sign_source.extend_from_slice(b"nobody");
    sign_source.extend_from_slice(path);
    sign_source.extend_from_slice(b"use");
    sign_source.extend_from_slice(&params_bytes);
    sign_source.extend_from_slice(b"md5forencrypt");
    let sign = md5_hex(&sign_source);

    let mut source = Vec::new();
    source.extend_from_slice(path);
    source.extend_from_slice(b"-36cd479b6b5-");
    source.extend_from_slice(&params_bytes);
    source.extend_from_slice(b"-36cd479b6b5-");
    source.extend_from_slice(sign.as_bytes());
    Ok(format!(
        "params={}",
        hex::encode_upper(aes_ecb_encrypt(&source, EAPI_KEY)?)
    ))
}

fn aes_ecb_encrypt(data: &[u8], key: &[u8; 16]) -> Result<Vec<u8>, LyricsProviderError> {
    let cipher = Aes128::new_from_slice(key)
        .map_err(|_| LyricsProviderError::Decode("invalid AES key".into()))?;
    let pad = 16 - data.len() % 16;
    let mut padded = data.to_vec();
    padded.extend(std::iter::repeat_n(pad as u8, pad));
    for chunk in padded.as_chunks_mut::<16>().0 {
        let block = cipher::Block::<Aes128>::from_mut_slice(chunk);
        cipher.encrypt_block(block);
    }
    Ok(padded)
}

fn aes_ecb_decrypt(data: &[u8], key: &[u8; 16]) -> Result<Vec<u8>, LyricsProviderError> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err(LyricsProviderError::Decode(
            "NetEase AES response has invalid length".into(),
        ));
    }
    let cipher = Aes128::new_from_slice(key)
        .map_err(|_| LyricsProviderError::Decode("invalid AES key".into()))?;
    let mut output = data.to_vec();
    for chunk in output.as_chunks_mut::<16>().0 {
        let block = cipher::Block::<Aes128>::from_mut_slice(chunk);
        cipher.decrypt_block(block);
    }
    let pad = *output.last().unwrap_or(&0) as usize;
    if !(1..=16).contains(&pad) || pad > output.len() {
        return Err(LyricsProviderError::Decode(
            "NetEase AES response has invalid padding".into(),
        ));
    }
    output.truncate(output.len() - pad);
    Ok(output)
}

fn header_json(cookies: &BTreeMap<String, String>) -> String {
    let mut header = Map::new();
    for key in ["clientSign", "os", "appver", "deviceId", "osver"] {
        if let Some(value) = cookies.get(key) {
            header.insert(key.to_owned(), Value::String(value.clone()));
        }
    }
    header.insert("requestId".into(), Value::from(0));
    Value::Object(header).to_string()
}

fn cookie_header(cookies: &BTreeMap<String, String>) -> String {
    cookies
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn anonymous_username(device_id: &str) -> String {
    let key = DEVICE_ID_XOR_KEY.as_bytes();
    let xored = device_id
        .as_bytes()
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect::<Vec<_>>();
    let digest = Md5::digest(&xored);
    BASE64.encode(format!("{device_id} {}", BASE64.encode(digest)).as_bytes())
}

fn md5_hex(data: &[u8]) -> String {
    let digest = Md5::digest(data);
    format!("{digest:x}")
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn value_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}
