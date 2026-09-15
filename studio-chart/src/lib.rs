//! Uta! Studio's internal vocal chart model.
//!
//! Analysis, caches, and the editor author this model. It follows the UTZ
//! VocalChart document and adds Studio-owned lyric timing state. `.utz`
//! packages only receive its standard projection, [`VocalChart::to_utz`];
//! the `utz` crate itself is used unmodified from upstream.

use serde::{Deserialize, Serialize};

pub use utz::{
    DEFAULT_TIMEBASE, LyricJoin, MAX_EXACT_INTEGER, MAX_ID_BYTES, NoteBonus, NotePitch,
    NoteScoring, ScoringMode, UTZ_TIMEBASE, VOCAL_CHART_FORMAT, VOCAL_CHART_MEDIA_TYPE,
    VOCAL_CHART_VERSION, VocalMode, VocalTrackRole,
};

#[derive(Debug, thiserror::Error)]
pub enum ChartError {
    #[error(transparent)]
    Standard(#[from] utz::UtzError),
    #[error("invalid lyric timing: {0}")]
    LyricTiming(String),
}

pub type Result<T> = std::result::Result<T, ChartError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VocalChart {
    pub format: String,
    pub format_version: String,
    pub timebase: u64,
    #[serde(default)]
    pub language: Option<String>,
    pub tracks: Vec<VocalTrack>,
}

impl VocalChart {
    pub fn new(tracks: Vec<VocalTrack>) -> Self {
        Self {
            format: VOCAL_CHART_FORMAT.into(),
            format_version: VOCAL_CHART_VERSION.into(),
            timebase: DEFAULT_TIMEBASE,
            language: None,
            tracks,
        }
    }

    /// Checks the standard UTZ rules on the projection, then the Studio-only
    /// lyric timing that the projection drops.
    pub fn validate(&self) -> Result<()> {
        self.to_utz().validate()?;
        for track in &self.tracks {
            for phrase in &track.phrases {
                for note in &phrase.notes {
                    for token in &note.lyrics {
                        if let LyricToken::Text(token) = token {
                            validate_lyric_timing(token)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The standard UTZ document for this chart. Independent and unresolved
    /// lyric timing is internal, so every lyric follows its hosting note.
    pub fn to_utz(&self) -> utz::VocalChart {
        utz::VocalChart {
            format: self.format.clone(),
            format_version: self.format_version.clone(),
            timebase: self.timebase,
            language: self.language.clone(),
            tracks: self.tracks.iter().map(standard_track).collect(),
        }
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
    /// Absolute lyric interval in the chart timebase, independent of its
    /// hosting note. None binds the lyric to the note's timing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<LyricTiming>,
    /// Original text whose token timing has not been independently resolved.
    #[serde(default, skip_serializing_if = "is_false")]
    pub timing_unresolved: bool,
    #[serde(default)]
    pub reading: Option<String>,
    #[serde(default)]
    pub phonemes: Option<String>,
}

/// An independent lyric interval in absolute chart-timebase units.
/// It may extend beyond the hosting note or overlap other lyric intervals.
/// An unresolved token may have zero duration: this is a placement marker,
/// not a measured interval or a zero-length note.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LyricTiming {
    pub start: u64,
    pub duration: u64,
}

fn validate_lyric_timing(token: &LyricTextToken) -> Result<()> {
    let Some(timing) = token.timing else {
        return Ok(());
    };
    // A point marks an unresolved word when adjacent measured words leave no
    // positive interval.
    if timing.duration == 0 && !token.timing_unresolved {
        return Err(ChartError::LyricTiming(format!(
            "lyric token {} has no duration",
            token.id
        )));
    }
    if timing
        .start
        .checked_add(timing.duration)
        .is_none_or(|end| end > MAX_EXACT_INTEGER)
    {
        return Err(ChartError::LyricTiming(format!(
            "lyric token {} is outside the supported integer range",
            token.id
        )));
    }
    Ok(())
}

fn standard_track(track: &VocalTrack) -> utz::VocalTrack {
    utz::VocalTrack {
        id: track.id.clone(),
        role: track.role,
        part: track.part,
        singer: track.singer.clone(),
        scoring_enabled: track.scoring_enabled,
        phrases: track
            .phrases
            .iter()
            .map(|phrase| utz::VocalPhrase {
                id: phrase.id.clone(),
                notes: phrase.notes.iter().map(standard_note).collect(),
            })
            .collect(),
    }
}

fn standard_note(note: &VocalNote) -> utz::VocalNote {
    utz::VocalNote {
        id: note.id.clone(),
        start: note.start,
        duration: note.duration,
        pitch: note.pitch,
        vocal_mode: note.vocal_mode,
        bonus: note.bonus,
        scoring: note.scoring.clone(),
        lyrics: note
            .lyrics
            .iter()
            .map(|token| match token {
                LyricToken::Text(token) => utz::LyricToken::Text(utz::LyricTextToken {
                    id: token.id.clone(),
                    text: token.text.clone(),
                    join_before: token.join_before,
                    reading: token.reading.clone(),
                    phonemes: token.phonemes.clone(),
                }),
                LyricToken::Continuation { continuation_of } => utz::LyricToken::Continuation {
                    continuation_of: continuation_of.clone(),
                },
            })
            .collect(),
    }
}

fn default_true() -> bool {
    true
}

fn is_false(value: &bool) -> bool {
    !value
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

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
                timing: None,
                timing_unresolved: false,
                reading: None,
                phonemes: None,
            })],
        }
    }

    fn chart() -> VocalChart {
        VocalChart::new(vec![VocalTrack {
            id: "lead".into(),
            role: VocalTrackRole::Lead,
            part: None,
            singer: None,
            scoring_enabled: true,
            phrases: vec![VocalPhrase {
                id: "phrase".into(),
                notes: vec![note("note", 0, "lyric", "歌")],
            }],
        }])
    }

    fn first_token(chart: &mut VocalChart) -> &mut LyricTextToken {
        let LyricToken::Text(token) = &mut chart.tracks[0].phrases[0].notes[0].lyrics[0] else {
            panic!("expected text token");
        };
        token
    }

    fn timed_chart() -> VocalChart {
        let mut chart = chart();
        chart.tracks[0].phrases[0].notes.push(note(
            "unmeasured",
            1_000_000,
            "unmeasured-lyric",
            "切に",
        ));
        let token = first_token(&mut chart);
        token.text = "切".into();
        token.timing = Some(LyricTiming {
            start: 125_000,
            duration: 600_000,
        });
        chart.tracks[0].phrases[0].notes[0]
            .lyrics
            .push(LyricToken::Text(LyricTextToken {
                id: "next-word".into(),
                text: "に".into(),
                join_before: LyricJoin::None,
                timing: Some(LyricTiming {
                    start: 725_000,
                    duration: 250_000,
                }),
                timing_unresolved: false,
                reading: None,
                phonemes: None,
            }));
        let LyricToken::Text(unmeasured) = &mut chart.tracks[0].phrases[0].notes[1].lyrics[0]
        else {
            panic!("expected text token");
        };
        unmeasured.timing_unresolved = true;
        unmeasured.timing = Some(LyricTiming {
            start: 1_250_000,
            duration: 0,
        });
        chart
    }

    #[test]
    fn studio_lyric_timing_round_trips_through_internal_json() {
        let chart = timed_chart();
        chart.validate().unwrap();
        let decoded: VocalChart =
            serde_json::from_slice(&serde_json::to_vec(&chart).unwrap()).unwrap();
        assert_eq!(decoded, chart);

        let bound = serde_json::to_value(self::chart()).unwrap();
        let token = &bound["tracks"][0]["phrases"][0]["notes"][0]["lyrics"][0];
        assert!(token.get("timing").is_none());
        assert!(token.get("timing_unresolved").is_none());
    }

    #[test]
    fn standard_projection_drops_studio_timing_and_keeps_note_geometry() {
        let chart = timed_chart();
        let standard = chart.to_utz();
        standard.validate().unwrap();
        let encoded = serde_json::to_value(&standard).unwrap();
        for note in encoded["tracks"][0]["phrases"][0]["notes"]
            .as_array()
            .unwrap()
        {
            for token in note["lyrics"].as_array().unwrap() {
                assert!(token.get("timing").is_none(), "{token}");
                assert!(token.get("timing_unresolved").is_none(), "{token}");
            }
        }
        let notes = &standard.tracks[0].phrases[0].notes;
        assert_eq!(
            notes
                .iter()
                .map(|note| (note.start, note.duration, note.lyrics.len()))
                .collect::<Vec<_>>(),
            [(0, 500_000, 2), (1_000_000, 500_000, 1)]
        );

        let manifest = utz::UtzManifest::new(
            "org.uta.example",
            utz::SongMetadata::new("Example", "Uta", 12_000_000),
            utz::AudioAssets::new(utz::AssetRef::pending(
                "audio/instrumental.mp3",
                "audio/mpeg",
            )),
            utz::AssetRef::pending("charts/vocal.json", VOCAL_CHART_MEDIA_TYPE),
        );
        let files = BTreeMap::from([
            ("audio/instrumental.mp3".into(), b"audio".to_vec()),
            (
                "charts/vocal.json".into(),
                serde_json::to_vec(&standard).unwrap(),
            ),
        ]);
        let package = utz::UtzPackage::build(manifest, files).unwrap();
        let decoded = utz::UtzPackage::from_bytes(&package.to_bytes().unwrap()).unwrap();
        assert_eq!(decoded.vocal_chart().unwrap(), standard);
    }

    #[test]
    fn lyric_timing_uses_exact_time_and_overflow_rules() {
        for timing in [
            LyricTiming {
                start: 0,
                duration: 0,
            },
            LyricTiming {
                start: MAX_EXACT_INTEGER + 1,
                duration: 1,
            },
            LyricTiming {
                start: 0,
                duration: MAX_EXACT_INTEGER + 1,
            },
            LyricTiming {
                start: MAX_EXACT_INTEGER,
                duration: 1,
            },
            LyricTiming {
                start: u64::MAX,
                duration: u64::MAX,
            },
        ] {
            let mut chart = chart();
            first_token(&mut chart).timing = Some(timing);
            assert!(chart.validate().is_err(), "{timing:?}");
        }
        let mut chart = chart();
        let token = first_token(&mut chart);
        token.timing = Some(LyricTiming {
            start: MAX_EXACT_INTEGER - 1,
            duration: 1,
        });
        // A lyric's interval need not be contained in its hosting note.
        chart.validate().unwrap();
        let token = first_token(&mut chart);
        token.timing = Some(LyricTiming {
            start: 250_000,
            duration: 0,
        });
        assert!(chart.validate().is_err());
        first_token(&mut chart).timing_unresolved = true;
        chart.validate().unwrap();
    }

    #[test]
    fn standard_rules_still_apply() {
        let mut chart = chart();
        chart.tracks[0].phrases[0].notes[0].lyrics.clear();
        assert!(matches!(chart.validate(), Err(ChartError::Standard(_))));
    }
}
