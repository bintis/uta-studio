use super::*;
use crate::artifact::{AlignmentArtifact, AlignmentItem, finalize_candidate_vocal_chart};
use crate::candidate_pipeline::fuse_alignment_stage;
use crate::contract::{BoundaryAuthority, BoundaryLevel};
use crate::fusion::{
    BoundaryEvidenceKind, BoundaryEvidenceSet, BoundarySegmentEvidence, HarmonyMetadata,
    LyricsAuthority, TimeRange, TranscriptTokenEvidence, build_canonical_singing_track,
    decode_candidate_graph, fuse_singing_evidence,
};

fn caller(language: &str, lines: &[(&str, u64, u64)]) -> CanonicalLyrics {
    CanonicalLyrics {
        text: lines.iter().map(|(text, _, _)| *text).collect::<Vec<_>>().join("\n"),
        language: Some(language.to_string()),
        authority: LyricsAuthority::CallerCanonical,
        tokens: lines.iter().enumerate().map(|(index, (text, start, end))| {
            TranscriptTokenEvidence {
                id: Some(format!("lrc-{index}")),
                text: text.to_string(),
                range: Some(TimeRange::new(*start, *end).unwrap()),
                confidence: None,
            }
        }).collect(),
        confidence: None,
        source_experts: vec!["caller.canonical_lyrics".to_string()],
        alternatives: Vec::new(),
    }
}

#[test]
fn caller_japanese_lines_expand_into_characters_inside_each_real_line_scope() {
    let transcript = caller("ja-JP", &[
        ("この身が焼き尽くされようとも", 10_360_000, 15_750_000),
        ("私の愛するこの国は", 15_750_000, 21_770_000),
        ("この身が焼き尽くされようとも", 114_100_000, 117_790_000),
    ]);
    let original = transcript.clone();
    let words = qwen_alignment_words(&transcript, &[]).unwrap();
    let mut cursor = 0;
    for token in &transcript.tokens {
        for character in token.text.chars() {
            assert_eq!(words[cursor]["text"], character.to_string());
            assert_eq!(words[cursor]["audio_range"], serde_json::to_value(token.range).unwrap());
            assert!(words[cursor].get("start").is_none(), "a scope is not a measured word time");
            cursor += 1;
        }
    }
    assert_eq!(words.len(), cursor);
    let ids = words.iter().map(|word| word["id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), words.len(), "repeated lines must not reuse word IDs");
    assert_eq!(transcript, original, "canonical line text and ranges remain intact");
}

#[test]
fn caller_english_lines_expand_into_words_not_sentences_or_letters() {
    let transcript = caller("en", &[("sing now, sing again!", 2_000_000, 6_000_000)]);
    let words = qwen_alignment_words(&transcript, &[]).unwrap();
    let texts = words.iter().map(|word| word["text"].as_str().unwrap()).collect::<Vec<_>>();
    assert_eq!(texts, ["sing", "now,", "sing", "again!"]);
    assert!(words.iter().all(|word| word["audio_range"] == serde_json::json!({
        "start": 2_000_000, "end": 6_000_000,
    })));
}

#[test]
fn caller_units_keep_punctuation_and_small_kana_with_their_lexical_owner() {
    let mut transcript = caller("ja", &[("「きゃ！」君。", 20_000_000, 28_000_000)]);
    let caller_words = qwen_alignment_words(&transcript, &[]).unwrap();
    let texts = caller_words.iter().map(|word| word["text"].clone()).collect::<Vec<_>>();
    assert_eq!(texts, [serde_json::json!("「きゃ！」"), serde_json::json!("君。")]);
    transcript.tokens.clear();
    transcript.authority = LyricsAuthority::Generated;
    let generated_words = qwen_alignment_words(&transcript, &[]).unwrap();
    assert_eq!(texts, generated_words.iter().map(|word| word["text"].clone()).collect::<Vec<_>>());
}

#[test]
fn measured_character_alignment_reaches_individual_chart_notes_without_line_lyrics() {
    let transcript = caller("zh", &[("风吹沙", 1_000_000, 2_300_000)]);
    let requests = qwen_alignment_words(&transcript, &[]).unwrap();
    assert_eq!(requests.len(), 3);
    // Explicit model-output fixture, not evenly divided caller line timing.
    let measured = [(1_100_000, 1_400_000), (1_520_000, 1_780_000), (1_900_000, 2_200_000)];
    let alignment = AlignmentArtifact {
        contract: "uta.analysis-engine.alignment".to_string(),
        version: 1,
        transcript: transcript.text.clone(),
        language: transcript.language.clone(),
        items: requests.iter().zip(measured).map(|(request, (start, end))| AlignmentItem {
            id: request["id"].as_str().unwrap().to_string(),
            text: request["text"].as_str().unwrap().to_string(),
            level: BoundaryLevel::Word,
            start,
            duration: end - start,
            confidence: None,
            authority: BoundaryAuthority::Soft,
            timing_issue: None,
        }).collect(),
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
        segments: measured.into_iter().zip([69.0, 71.0, 72.0])
            .map(|((start, end), midi)| BoundarySegmentEvidence {
                range: TimeRange::new(start, end).unwrap(),
                fractional_midi: Some(midi),
                boundary_decision_parameter: None,
                presence_decision_parameter: None,
            }).collect(),
    };
    let fusion = fuse_singing_evidence(
        &words, &boundaries, "rmvpe", &[], None, &[], None, None, None,
    ).unwrap();
    let selected = decode_candidate_graph(&fusion.candidates).unwrap();
    let track = build_canonical_singing_track(
        transcript, words, selected, Vec::new(), "rmvpe", HarmonyMetadata::default(), Vec::new(),
    ).unwrap();
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
#[ignore = "requires explicit caller-lyrics input and a new diagnostic output path; no inference"]
fn prepare_alignment_words_from_explicit_lyrics() {
    let input = std::env::var_os("UTA_STUDIO_ALIGNMENT_LYRICS_INPUT")
        .expect("explicit canonical lyric input is required");
    let output = std::env::var_os("UTA_STUDIO_ALIGNMENT_WORDS_OUTPUT")
        .expect("explicit new word-request output is required");
    let transcript: CanonicalLyrics =
        serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let words = qwen_alignment_words(&transcript, &[]).unwrap();
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(output).unwrap();
    serde_json::to_writer_pretty(&mut file, &words).unwrap();
    file.sync_all().unwrap();
    println!("caller tokens: {}; lexical alignment requests: {}", transcript.tokens.len(), words.len());
}
