//! Song detail route: overview, lyrics editor, and authoring jobs.

use crate::studio::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LyricsInputMode {
    Plain,
    TimedLrc,
    StructuredTimedTranscript,
}

#[cfg(test)]
mod timed_transcript_boundary_tests {
    use super::*;

    #[test]
    fn adjusts_only_the_selected_repeated_word_boundary() {
        let mut value = serde_json::json!({
            "segments": [{
                "start": 1.0, "end": 2.0, "extension": {"kept": true},
                "words": [
                    {"word": "同じ", "start": 1.0, "end": 1.4},
                    {"word": "同じ", "start": 1.4, "end": 2.0}
                ]
            }]
        });
        adjust_transcript_boundary_value(
            &mut value,
            TranscriptBoundaryTarget::Word {
                segment: 0,
                word: 1,
            },
            TranscriptBoundaryEdge::Start,
            0.025,
            &AppConfig::default(),
        )
        .unwrap();
        assert_eq!(value["segments"][0]["words"][0]["start"], 1.0);
        let adjusted = value["segments"][0]["words"][1]["start"]
            .as_f64()
            .expect("adjusted word start stays numeric");
        assert!((adjusted - 1.425).abs() < 0.000_001);
        assert_eq!(value["segments"][0]["extension"]["kept"], true);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TranscriptBoundaryTarget {
    Segment(usize),
    Word { segment: usize, word: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TranscriptBoundaryEdge {
    Start,
    End,
}

pub(crate) fn adjust_transcript_boundary_value(
    value: &mut serde_json::Value,
    target: TranscriptBoundaryTarget,
    edge: TranscriptBoundaryEdge,
    delta_seconds: f64,
    config: &AppConfig,
) -> Result<(), String> {
    let field = match edge {
        TranscriptBoundaryEdge::Start => "start",
        TranscriptBoundaryEdge::End => "end",
    };
    let boundary = match target {
        TranscriptBoundaryTarget::Segment(segment) => value
            .get_mut("segments")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|segments| segments.get_mut(segment)),
        TranscriptBoundaryTarget::Word { segment, word } => value
            .get_mut("segments")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|segments| segments.get_mut(segment))
            .and_then(|segment| segment.get_mut("words"))
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|words| words.get_mut(word)),
    }
    .ok_or_else(|| "The selected timing boundary no longer exists.".to_string())?;
    let seconds = boundary
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| {
            localized_message(config, UiMessage::TimingNotNumeric, &[("{field}", field)])
        })?;
    boundary[field] = serde_json::json!(seconds + delta_seconds);
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LyricsCandidateUseMode {
    Plain,
    TimedLrc,
    Translation,
    Romanization,
}

pub(crate) const LYRICS_CANDIDATE_SLOTS: usize = 6;

pub(crate) fn lyrics_candidate_text(
    candidate: &app_core::LyricsCandidate,
    use_mode: LyricsCandidateUseMode,
) -> Option<(LyricsInputMode, String)> {
    match use_mode {
        LyricsCandidateUseMode::Plain => {
            let text = if candidate.lines.is_empty() {
                candidate
                    .synced_lyrics
                    .as_deref()
                    .map(app_core::strip_lyrics_timing)?
            } else {
                candidate.lines.join("\n")
            };
            (!text.trim().is_empty()).then(|| {
                (
                    LyricsInputMode::Plain,
                    app_core::normalize_lyrics_text(&text),
                )
            })
        }
        LyricsCandidateUseMode::TimedLrc => candidate.synced_lyrics.as_deref().and_then(|text| {
            let text = app_core::normalize_lyrics_text(text);
            (!text.trim().is_empty()).then_some((LyricsInputMode::TimedLrc, text))
        }),
        LyricsCandidateUseMode::Translation => candidate.translation.as_deref().and_then(|text| {
            let text = app_core::normalize_lyrics_text(text);
            (!text.trim().is_empty()).then_some((LyricsInputMode::Plain, text))
        }),
        LyricsCandidateUseMode::Romanization => {
            candidate.romanization.as_deref().and_then(|text| {
                let text = app_core::normalize_lyrics_text(text);
                (!text.trim().is_empty()).then_some((LyricsInputMode::Plain, text))
            })
        }
    }
}

#[cfg(test)]
mod lyrics_candidate_text_tests {
    use super::*;

    fn loaded_candidate() -> app_core::LyricsCandidate {
        serde_json::from_value(serde_json::json!({
            "provider": "netease",
            "provider_id": "42",
            "track_name": "Song",
            "artist_name": "Artist",
            "album_name": "Album",
            "duration_secs": 180.0,
            "has_timed_lyrics": true,
            "has_translation": true,
            "has_romanization": true,
            "loaded": true,
            "lines": ["原文 一", "原文 二"],
            "synced_lyrics": "[00:01.00]原文 一\n[00:02.00]原文 二",
            "translation": " translated   one \n translated two ",
            "romanization": "genbun ichi\ngenbun ni"
        }))
        .unwrap()
    }

    #[test]
    fn candidate_forms_map_to_the_expected_editor_modes() {
        let candidate = loaded_candidate();
        assert_eq!(
            lyrics_candidate_text(&candidate, LyricsCandidateUseMode::Plain),
            Some((LyricsInputMode::Plain, "原文 一\n原文 二".to_string()))
        );
        assert_eq!(
            lyrics_candidate_text(&candidate, LyricsCandidateUseMode::TimedLrc),
            Some((
                LyricsInputMode::TimedLrc,
                "[00:01.00]原文 一\n[00:02.00]原文 二".to_string()
            ))
        );
        assert_eq!(
            lyrics_candidate_text(&candidate, LyricsCandidateUseMode::Translation),
            Some((
                LyricsInputMode::Plain,
                "translated one\ntranslated two".to_string()
            ))
        );
        assert_eq!(
            lyrics_candidate_text(&candidate, LyricsCandidateUseMode::Romanization),
            Some((LyricsInputMode::Plain, "genbun ichi\ngenbun ni".to_string()))
        );
    }

    #[test]
    fn plain_form_can_be_derived_from_a_timed_only_candidate() {
        let mut candidate = loaded_candidate();
        candidate.lines.clear();
        assert_eq!(
            lyrics_candidate_text(&candidate, LyricsCandidateUseMode::Plain),
            Some((LyricsInputMode::Plain, "原文 一\n原文 二".to_string()))
        );
    }
}

pub(crate) struct NativeLyricsEditor {
    pub(crate) file_hash: String,
    pub(crate) return_route: StudioRoute,
    pub(crate) search_title: String,
    pub(crate) mode: LyricsInputMode,
    pub(crate) initial_text: String,
    pub(crate) candidates: Vec<app_core::LyricsCandidate>,
    pub(crate) candidate_page: usize,
    pub(crate) searching: bool,
    pub(crate) fetching_candidate: Option<usize>,
    pub(crate) provider_errors: Vec<app_core::LyricsProviderFailure>,
    pub(crate) artifact_draft: Option<app_core::ArtifactEditDraft>,
    pub(crate) waveform: app_core::ChartWaveform,
}

pub(crate) struct NativeLanguageEditor {
    pub(crate) file_hash: String,
    pub(crate) initial_language: String,
    pub(crate) force_transcribe: bool,
    pub(crate) picker_open: bool,
}

pub(crate) const ANALYSIS_LANGUAGE_OPTIONS: &[(&str, &str)] = &[
    ("auto", "Automatic detection"),
    ("ja", "Japanese"),
    ("en", "English"),
    ("zh", "Chinese"),
    ("ko", "Korean"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("id", "Indonesian"),
    ("vi", "Vietnamese"),
    ("th", "Thai"),
    ("tr", "Turkish"),
    ("pl", "Polish"),
    ("uk", "Ukrainian"),
    ("nl", "Dutch"),
    ("sv", "Swedish"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
];

pub(crate) fn canonical_analysis_language(language: &str) -> String {
    match language
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .as_str()
    {
        "jp" | "jpn" => "ja".into(),
        "eng" => "en".into(),
        "kor" => "ko".into(),
        "chi" | "zho" | "cn" | "zh-cn" | "zh-tw" => "zh".into(),
        language
            if ANALYSIS_LANGUAGE_OPTIONS
                .iter()
                .any(|(code, _)| *code == language) =>
        {
            language.to_string()
        }
        _ => "auto".into(),
    }
}

pub(crate) fn analysis_language_label(language: &str) -> &'static str {
    ANALYSIS_LANGUAGE_OPTIONS
        .iter()
        .find_map(|(code, label)| (*code == language).then_some(*label))
        .unwrap_or("Automatic detection")
}

#[derive(Resource, Default)]
pub(crate) struct NativeAuthoringJob {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<AuthoringEvent>>>,
}

#[derive(Default)]
pub(crate) struct NativeLyricsSearchJob {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<(String, app_core::LyricsSearchResult)>>>,
}

#[derive(Default)]
pub(crate) struct NativeLyricsFetchJob {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<LyricsCandidateFetchResult>>>,
}

pub(crate) type LyricsCandidateFetchResult =
    (String, usize, Result<app_core::LyricsCandidate, String>);

#[derive(Default)]
pub(crate) struct NativeLyricsWaveformJob {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<LyricsWaveformResult>>>,
}

type LyricsWaveformResult = (String, Result<app_core::ChartWaveform, String>);

pub(crate) struct AuthoringEvent {
    pub(crate) result: Result<app_core::ShiftResult, String>,
    pub(crate) kind: &'static str,
}

#[derive(Component)]
pub(crate) struct SongDetailContent;

#[derive(Component)]
pub(crate) struct LyricsEditorInput;

#[derive(Component)]
pub(crate) struct LyricsSearchTitleInput;

#[derive(Component)]
pub(crate) struct LyricsWorkbenchContent;

#[derive(Component)]
pub(crate) struct LanguageEditorInput;

pub(crate) fn lyrics_text(file_hash: &str, mode: LyricsInputMode) -> String {
    if let Some(file) = app_core::load_lyrics_file(file_hash) {
        if mode == LyricsInputMode::TimedLrc
            && let Some(timed_lrc) = file.timed_lrc
        {
            return timed_lrc;
        }
        if mode == LyricsInputMode::Plain {
            return file.lines.join("\n");
        }
    }
    // A Timed LRC import (`provide_lrc`/`apply_timed_lyrics`) overwrites the
    // transcript but deliberately leaves any existing Authored/Candidate
    // Chart alone (the immutable artifact contract §6/Phase 5 protects chart
    // edits from being silently discarded). For an already-analyzed song
    // that chart predates the new lyrics -- it was built from whatever the
    // old transcript was -- so its phrase count and order can disagree with
    // what was just saved. The saved LRC transcript is the authoritative
    // source for this mode; read it before ever consulting a chart that may
    // be stale.
    if mode == LyricsInputMode::TimedLrc {
        let lines = app_core::lrc_transcript_line_segments(&app_core::CacheDir::new(), file_hash)
            .into_iter()
            .map(|(start, _end, text)| format!("[{}]{text}", format_lrc_timestamp(start)))
            .collect::<Vec<_>>();
        if !lines.is_empty() {
            return lines.join("\n");
        }
    }
    if let Ok(chart) = app_core::load_chart(file_hash) {
        let document = app_core::EditorDocument::new(chart.vocal_chart);
        let text = (0..document.phrase_count())
            .filter_map(|phrase| {
                let text = document.phrase_text(phrase);
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                if mode == LyricsInputMode::TimedLrc {
                    let start = document
                        .lyric(app_core::LyricAddress {
                            segment: phrase,
                            word: 0,
                        })
                        .map(|(_, start, _)| start)
                        .unwrap_or(0.0);
                    Some(format!("[{}]{text}", format_lrc_timestamp(start)))
                } else {
                    Some(text.to_string())
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return text;
        }
    }
    String::new()
}

pub(crate) fn format_lrc_timestamp(seconds: f64) -> String {
    let centiseconds = (seconds.max(0.0) * 100.0).round() as u64;
    format!(
        "{:02}:{:02}.{:02}",
        centiseconds / 6000,
        centiseconds / 100 % 60,
        centiseconds % 100
    )
}
