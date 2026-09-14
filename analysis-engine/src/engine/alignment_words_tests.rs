use super::*;
use crate::artifact::{
    AlignmentArtifact, AlignmentItem, TranscriptArtifact, finalize_candidate_vocal_chart,
};
use crate::candidate_pipeline::{fuse_alignment_stage, fuse_transcript_stage};
use crate::contract::{BoundaryAuthority, BoundaryLevel};
use crate::fusion::{
    BoundaryEvidenceKind, BoundaryEvidenceSet, BoundarySegmentEvidence, HarmonyMetadata,
    LyricsAuthority, TimeRange, TranscriptTokenEvidence, build_canonical_singing_track,
    decode_candidate_graph, fuse_singing_evidence,
};

fn caller(language: &str, lines: &[(&str, u64, u64)]) -> CanonicalLyrics {
    CanonicalLyrics {
        text: lines
            .iter()
            .map(|(text, _, _)| *text)
            .collect::<Vec<_>>()
            .join("\n"),
        language: Some(language.to_string()),
        authority: LyricsAuthority::CallerCanonical,
        tokens: lines
            .iter()
            .enumerate()
            .map(|(index, (text, start, end))| TranscriptTokenEvidence {
                id: Some(format!("lrc-{index}")),
                text: text.to_string(),
                range: Some(TimeRange::new(*start, *end).unwrap()),
                confidence: None,
            })
            .collect(),
        confidence: None,
        source_experts: vec!["caller.canonical_lyrics".to_string()],
        alternatives: Vec::new(),
    }
}

#[test]
fn caller_japanese_lines_expand_into_words_inside_each_real_line_scope() {
    let transcript = caller(
        "ja-JP",
        &[
            ("この身が焼き尽くされようとも", 10_360_000, 15_750_000),
            ("私の愛するこの国は", 15_750_000, 21_770_000),
            ("この身が焼き尽くされようとも", 114_100_000, 117_790_000),
        ],
    );
    let original = transcript.clone();
    let words = qwen_alignment_words(&transcript, &[]).unwrap();
    let mut cursor = 0;
    for token in &transcript.tokens {
        let scope = serde_json::to_value(token.range).unwrap();
        let line_words = words[cursor..]
            .iter()
            .take_while(|word| word["audio_range"] == scope)
            .collect::<Vec<_>>();
        assert!(!line_words.is_empty());
        assert!(
            line_words.len() < token.text.chars().count(),
            "Japanese words must not become a timestamp pair per character"
        );
        assert_eq!(
            line_words
                .iter()
                .map(|word| word["text"].as_str().unwrap())
                .collect::<String>(),
            token.text
        );
        assert!(
            line_words
                .iter()
                .all(|word| word.get("start").is_none() && word.get("end").is_none()),
            "caller line scopes do not measure word or character times"
        );
        cursor += line_words.len();
    }
    assert_eq!(words.len(), cursor);
    let ids = words
        .iter()
        .map(|word| word["id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        ids.len(),
        words.len(),
        "repeated lines must not reuse word IDs"
    );
    assert_eq!(
        transcript, original,
        "canonical line text and ranges remain intact"
    );
}

#[test]
fn japanese_dictionary_boundaries_preserve_source_spelling_and_punctuation() {
    let text = "「眩しさ！」一人目覚める。 ＡＢＣ ｶﾅ e\u{301} 東京へ";
    let transcript = caller("ja", &[(text, 48_180_000, 53_700_000)]);
    let words = qwen_alignment_words(&transcript, &[]).unwrap();
    let joined = words
        .iter()
        .map(|word| word["text"].as_str().unwrap())
        .collect::<String>();
    assert_eq!(
        joined,
        text.chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    );
    assert!(
        words.iter().all(|word| word["audio_range"]
            == serde_json::json!({"start": 48_180_000, "end": 53_700_000}))
    );
    assert!(words.iter().any(|word| word["text"] == "一人"));
    assert!(words.iter().any(|word| word["text"] == "目覚める。"));
}

#[test]
fn caller_english_lines_expand_into_words_not_sentences_or_letters() {
    let transcript = caller("en", &[("sing now, sing again!", 2_000_000, 6_000_000)]);
    let words = qwen_alignment_words(&transcript, &[]).unwrap();
    let texts = words
        .iter()
        .map(|word| word["text"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(texts, ["sing", "now,", "sing", "again!"]);
    assert!(words.iter().all(|word| word["audio_range"]
        == serde_json::json!({
            "start": 2_000_000, "end": 6_000_000,
        })));
}

#[test]
fn caller_units_keep_punctuation_and_small_kana_with_their_lexical_owner() {
    let mut transcript = caller("ja", &[("「きゃ！」君。", 20_000_000, 28_000_000)]);
    let caller_words = qwen_alignment_words(&transcript, &[]).unwrap();
    let texts = caller_words
        .iter()
        .map(|word| word["text"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        [serde_json::json!("「きゃ！」"), serde_json::json!("君。")]
    );
    transcript.tokens.clear();
    transcript.authority = LyricsAuthority::Generated;
    let generated_words = qwen_alignment_words(&transcript, &[]).unwrap();
    assert_eq!(
        texts,
        generated_words
            .iter()
            .map(|word| word["text"].clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn measured_character_alignment_reaches_individual_chart_notes_without_line_lyrics() {
    let transcript = caller("zh", &[("风吹沙", 1_000_000, 2_300_000)]);
    let requests = qwen_alignment_words(&transcript, &[]).unwrap();
    assert_eq!(requests.len(), 3);
    // Explicit model-output fixture, not evenly divided caller line timing.
    let measured = [
        (1_100_000, 1_400_000),
        (1_520_000, 1_780_000),
        (1_900_000, 2_200_000),
    ];
    let alignment = AlignmentArtifact {
        contract: "uta.analysis-engine.alignment".to_string(),
        version: 1,
        transcript: transcript.text.clone(),
        language: transcript.language.clone(),
        items: requests
            .iter()
            .zip(measured)
            .map(|(request, (start, end))| AlignmentItem {
                id: request["id"].as_str().unwrap().to_string(),
                text: request["text"].as_str().unwrap().to_string(),
                level: BoundaryLevel::Word,
                start,
                duration: end - start,
                confidence: None,
                authority: BoundaryAuthority::Soft,
                timing_issue: None,
            })
            .collect(),
        source_expert: "qwen3_forced_aligner_0_6b".to_string(),
        model_sha256: "fixture".to_string(),
        runtime_manifest_sha256: "fixture".to_string(),
        backend: "ggml_cpu".to_string(),
    };
    alignment.validate(0, 3_000_000).unwrap();
    let (_, words) = fuse_alignment_stage(&transcript, &[alignment], 0, 3_000_000).unwrap();
    let boundaries = BoundaryEvidenceSet {
        source_expert: "game".to_string(),
        kind: BoundaryEvidenceKind::Game,
        model_hash: None,
        runtime_identity: None,
        segments: measured
            .into_iter()
            .zip([69.0, 71.0, 72.0])
            .map(|((start, end), midi)| BoundarySegmentEvidence {
                range: TimeRange::new(start, end).unwrap(),
                fractional_midi: Some(midi),
                boundary_decision_parameter: None,
                presence_decision_parameter: None,
            })
            .collect(),
    };
    let fusion = fuse_singing_evidence(
        &words,
        &boundaries,
        "rmvpe",
        &[],
        None,
        &[],
        None,
        None,
        None,
    )
    .unwrap();
    let selected = decode_candidate_graph(&fusion.candidates).unwrap();
    let track = build_canonical_singing_track(
        transcript,
        words,
        selected,
        Vec::new(),
        "rmvpe",
        HarmonyMetadata::default(),
        Vec::new(),
    )
    .unwrap();
    let chart = finalize_candidate_vocal_chart(&track, "word-alignment-fixture", None).unwrap();
    let notes = &chart.tracks[0].phrases[0].notes;
    assert_eq!(notes.len(), 3);
    for ((note, (start, end)), text) in notes.iter().zip(measured).zip(["风", "吹", "沙"]) {
        assert_eq!((note.start, note.duration), (start, end - start));
        assert_eq!(note.lyrics.len(), 1);
        let utz::LyricToken::Text(lyric) = &note.lyrics[0] else {
            panic!("each measured character needs text, not a whole-line continuation");
        };
        assert_eq!(lyric.text, text);
    }
}

#[test]
fn generated_sentences_reach_chart_phrases_even_when_a_sentence_end_is_unresolved() {
    let text = "光る 歌。歌う！光る 歌。";
    let generated = TranscriptArtifact {
        contract: "uta.analysis-engine.transcript".to_string(),
        version: 1,
        authority: crate::artifact::TranscriptAuthority::Generated,
        language: Some("ja".to_string()),
        text: text.to_string(),
        tokens: Vec::new(),
        audio_segments: vec![crate::artifact::TranscriptAudioSegment {
            start: 0,
            duration: 2_000_000,
            text_start: 0,
            text_end: text
                .chars()
                .filter(|character| !character.is_whitespace())
                .count(),
        }],
        confidence: None,
        source_experts: vec!["qwen3_asr_1_7b".to_string()],
        alternatives: Vec::new(),
        model_sha256: Some("fixture".to_string()),
        runtime_manifest_sha256: Some("fixture".to_string()),
        backend: "ggml_cpu".to_string(),
    };
    let (artifact, transcript) = fuse_transcript_stage(&[generated], None).unwrap();
    assert_eq!(artifact.text, text);
    assert_eq!(transcript.authority, LyricsAuthority::Generated);
    assert_eq!(transcript.tokens.len(), 3);
    assert!(transcript.tokens.iter().all(|token| token.range.is_none()));
    let requests = qwen_alignment_words(&transcript, &artifact.audio_segments).unwrap();
    assert_eq!(
        requests
            .iter()
            .map(|word| word["text"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["光る", "歌。", "歌う！", "光る", "歌。"]
    );
    assert!(
        requests
            .iter()
            .all(|word| word["audio_range"] == serde_json::json!({"start": 0, "end": 2_000_000}))
    );
    let measured = [
        (100_000, 250_000),
        (260_000, 400_000),
        (500_000, 800_000),
        (900_000, 1_050_000),
        (1_060_000, 1_300_000),
    ];
    let alignment = AlignmentArtifact {
        contract: "uta.analysis-engine.alignment".to_string(),
        version: 1,
        transcript: text.to_string(),
        language: transcript.language.clone(),
        items: requests
            .iter()
            .zip(measured)
            .enumerate()
            .map(|(index, (request, (start, end)))| AlignmentItem {
                id: request["id"].as_str().unwrap().to_string(),
                text: request["text"].as_str().unwrap().to_string(),
                level: BoundaryLevel::Word,
                // The unmeasured sentence-final word retains the actual
                // model search scope, not an invented word-sized interval.
                start: if index == 1 { 0 } else { start },
                duration: if index == 1 { 2_000_000 } else { end - start },
                confidence: None,
                authority: BoundaryAuthority::Soft,
                timing_issue: (index == 1).then(|| "collapsed_timestamp".to_string()),
            })
            .collect(),
        source_expert: "qwen3_forced_aligner_0_6b".to_string(),
        model_sha256: "fixture".to_string(),
        runtime_manifest_sha256: "fixture".to_string(),
        backend: "ggml_cpu".to_string(),
    };
    alignment.validate(0, 2_000_000).unwrap();
    let (alignment, words) = fuse_alignment_stage(&transcript, &[alignment], 0, 2_000_000).unwrap();
    assert_eq!(
        (alignment.items[1].start, alignment.items[1].duration),
        (0, 2_000_000)
    );
    assert_eq!(
        words
            .iter()
            .map(|word| word.line_id.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("lyric-line-0"),
            Some("lyric-line-1"),
            Some("lyric-line-2"),
            Some("lyric-line-2")
        ]
    );
    let boundaries = BoundaryEvidenceSet {
        source_expert: "game".to_string(),
        kind: BoundaryEvidenceKind::Game,
        model_hash: None,
        runtime_identity: None,
        segments: measured
            .into_iter()
            .map(|(start, end)| BoundarySegmentEvidence {
                range: TimeRange::new(start, end).unwrap(),
                fractional_midi: Some(69.0),
                boundary_decision_parameter: None,
                presence_decision_parameter: None,
            })
            .collect(),
    };
    let fusion = fuse_singing_evidence(
        &words,
        &boundaries,
        "rmvpe",
        &[],
        None,
        &[],
        None,
        None,
        None,
    )
    .unwrap();
    let selected = decode_candidate_graph(&fusion.candidates).unwrap();
    let mut track = build_canonical_singing_track(
        transcript,
        words,
        selected,
        Vec::new(),
        "rmvpe",
        HarmonyMetadata::default(),
        Vec::new(),
    )
    .unwrap();
    crate::candidate_pipeline::attach_alignment_lyric_units(&mut track, &alignment);
    assert_eq!(track.lyric_units.len(), requests.len());
    assert!(track.lyric_units[1].measured_range.is_none());
    assert_eq!(
        track.lyric_units[1].audition_range,
        TimeRange::new(0, 2_000_000).unwrap()
    );
    let chart =
        finalize_candidate_vocal_chart(&track, "generated-sentences-fixture", None).unwrap();
    let phrases = &chart.tracks[0].phrases;
    assert_eq!(phrases.len(), 3);
    assert_eq!(
        phrases
            .iter()
            .map(|phrase| phrase
                .notes
                .iter()
                .flat_map(|note| &note.lyrics)
                .filter_map(|token| match token {
                    utz::LyricToken::Text(token) => Some(token.text.as_str()),
                    _ => None,
                })
                .collect::<String>())
            .collect::<Vec<_>>(),
        ["光る歌。", "歌う！", "光る歌。"]
    );
    let notes = phrases
        .iter()
        .flat_map(|phrase| &phrase.notes)
        .collect::<Vec<_>>();
    let lyrics = notes
        .iter()
        .flat_map(|note| &note.lyrics)
        .filter_map(|token| match token {
            utz::LyricToken::Text(token) if !token.text.is_empty() => Some(token),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        lyrics
            .iter()
            .map(|token| token.text.as_str())
            .collect::<String>(),
        text.chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    );
    assert_eq!(lyrics.len(), requests.len());
    assert_eq!(lyrics[0].text, "光る");
    assert!(!lyrics[0].timing_unresolved);
    assert_eq!(lyrics[1].text, "歌。");
    assert!(
        lyrics[1].timing_unresolved,
        "the preserved final word has an audition scope, not a measured word time"
    );
    for (token, unit) in lyrics.iter().zip(&track.lyric_units) {
        assert_eq!(token.id, unit.id);
        assert_eq!(token.text, unit.text);
        assert_eq!(token.timing_unresolved, unit.measured_range.is_none());
        if let Some(range) = unit.measured_range {
            assert_eq!(
                token.timing,
                Some(utz::LyricTiming {
                    start: range.start,
                    duration: range.end - range.start,
                })
            );
        }
    }
    // Clip only the unresolved display scope between measured neighbours;
    // preserve its full original search scope in canonical lyric evidence.
    assert_eq!(
        lyrics[1].timing,
        Some(utz::LyricTiming {
            start: 250_000,
            duration: 250_000,
        })
    );
    assert_eq!(
        track.lyric_units[1].audition_range,
        TimeRange::new(0, 2_000_000).unwrap()
    );
    assert_eq!(notes.len(), measured.len());
    for (note, (start, end)) in notes.iter().zip(measured) {
        assert_eq!((note.start, note.duration), (start, end - start));
    }
    let encoded = serde_json::to_vec(&chart).unwrap();
    let decoded: utz::VocalChart = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, chart);
}

#[test]
#[ignore = "requires explicit caller-lyrics input and a new diagnostic output path; no inference"]
fn prepare_alignment_words_from_explicit_lyrics() {
    let input = std::env::var_os("UTA_STUDIO_ALIGNMENT_LYRICS_INPUT")
        .expect("explicit canonical lyric input is required");
    let output = std::env::var_os("UTA_STUDIO_ALIGNMENT_WORDS_OUTPUT")
        .expect("explicit new word-request output is required");
    let transcript: CanonicalLyrics =
        serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let words = qwen_alignment_words(&transcript, &[]).unwrap();
    assert_eq!(
        words
            .iter()
            .map(|word| word["text"].as_str().unwrap())
            .collect::<String>(),
        transcript
            .text
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    );
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &words).unwrap();
    file.sync_all().unwrap();
    println!(
        "caller tokens: {}; lexical alignment requests: {}; input scopes only, no measured times",
        transcript.tokens.len(),
        words.len()
    );
}
