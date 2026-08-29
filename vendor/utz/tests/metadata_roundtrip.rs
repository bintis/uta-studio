use serde_json::json;
use utz::{AssetRef, FORMAT_VERSION, SongMetadata, UtzManifest};

fn asset(path: &str, media_type: &str) -> AssetRef {
    AssetRef {
        path: path.into(),
        media_type: media_type.into(),
        sha256: "0000000000000000000000000000000000000000000000000000000000000000".into(),
        bytes: 1,
    }
}

#[test]
fn current_version_is_03() {
    assert_eq!(FORMAT_VERSION, "0.3.0");
}

#[test]
fn v03_metadata_round_trips() {
    let raw = json!({
        "format": "uta.song",
        "format_version": "0.3.0",
        "package_id": "org.example.roundtrip",
        "revision": 1,
        "song": {
            "title": "Demo",
            "artist": "Singer",
            "duration": 123400000,
            "metadata": {
                "album": "Album",
                "org.musicbrainz": {"recording_id": "abc"},
                "arbitrary": [1, true, null, {"nested": "value"}]
            }
        },
        "audio": {
            "assets": {
                "instrumental": {
                    "path": "audio/instrumental.ogg",
                    "media_type": "audio/ogg",
                    "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
                    "bytes": 1
                }
            }
        },
        "charts": {
            "vocal": {
                "path": "charts/vocal.json",
                "media_type": "application/vnd.uta.vocal-chart+json;version=0.3",
                "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                "bytes": 1
            }
        },
        "required_features": ["vocal-chart/0.3"]
    });

    let manifest: UtzManifest = serde_json::from_value(raw.clone()).unwrap();
    manifest.validate().unwrap();
    let encoded = serde_json::to_value(&manifest).unwrap();
    assert_eq!(encoded["song"]["metadata"], raw["song"]["metadata"]);
}

#[test]
fn pre_03_manifests_are_rejected() {
    for version in ["0.1.0", "0.2.0", "0.2.99"] {
        let manifest = UtzManifest::new(
            "org.example.reject",
            SongMetadata::new("Demo", "Singer", 10_000_000),
            utz::AudioAssets::new(utz::AssetRef::pending(
                "audio/instrumental.ogg",
                "audio/ogg",
            )),
            utz::AssetRef::pending("charts/vocal.json", utz::VOCAL_CHART_MEDIA_TYPE),
        );
        let mut raw = serde_json::to_value(manifest).unwrap();
        raw["format_version"] = json!(version);
        let parsed: UtzManifest = serde_json::from_value(raw).unwrap();
        assert!(
            parsed.validate().is_err(),
            "{version} unexpectedly accepted"
        );
    }
}

#[test]
fn v03_rejects_flat_descriptive_metadata() {
    let mut raw = serde_json::to_value(UtzManifest::new(
        "org.example.flat",
        SongMetadata::new("Demo", "Singer", 10_000_000),
        utz::AudioAssets::new(utz::AssetRef::pending(
            "audio/instrumental.ogg",
            "audio/ogg",
        )),
        utz::AssetRef::pending("charts/vocal.json", utz::VOCAL_CHART_MEDIA_TYPE),
    ))
    .unwrap();

    raw["song"]["album"] = json!("must be under metadata");
    assert!(serde_json::from_value::<UtzManifest>(raw).is_err());
}

#[test]
fn alternate_representations_round_trip_and_are_declared_assets() {
    let mut manifest = UtzManifest::new(
        "org.example.representations",
        SongMetadata::new("Demo", "Singer", 10_000_000),
        utz::AudioAssets::new(asset("audio/instrumental.ogg", "audio/ogg")),
        asset("charts/vocal.json", utz::VOCAL_CHART_MEDIA_TYPE),
    );

    manifest.representations.insert(
        "midi.quantized".into(),
        asset("representations/song.mid", "audio/midi"),
    );
    manifest.representations.insert(
        "ustx".into(),
        asset(
            "representations/song.ustx",
            "application/x-openutau-project",
        ),
    );

    assert!(manifest.validate().is_ok());
    assert_eq!(manifest.representations.len(), 2);
}
