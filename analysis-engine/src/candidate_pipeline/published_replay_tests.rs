//! Explicit read-only diagnostic over a published candidate pool and alignment.
//! No model, worker, audio decode or user-library mutation is performed.

use super::*;
use crate::artifact::{SingingAnalysis, TranscriptToken, parse_alignment_artifact};
use std::path::{Path, PathBuf};

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    serde_json::from_slice(&std::fs::read(path).expect("read explicit snapshot"))
        .expect("current artifact shape")
}

fn compact(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

#[test]
#[ignore = "explicit published-artifact snapshot and fresh output directory required"]
fn replay_published_lyrics_and_candidate_pool() {
    let input =
        PathBuf::from(std::env::var_os("UTA_STUDIO_PUBLISHED_REPLAY_INPUT").expect("input"));
    let output =
        PathBuf::from(std::env::var_os("UTA_STUDIO_PUBLISHED_REPLAY_OUTPUT").expect("output"));
    let caller: Lyrics = read_json(&input.join("caller-lyrics.json"));
    let retained_pitch: utz::PitchEvidence = read_json(&input.join("PitchEvidence.json"));
    let duration = retained_pitch.hop * retained_pitch.frequency_hz.len() as u64;
    let alignment = parse_alignment_artifact(&input.join("AlignmentEvidence.json"), 0, duration)
        .expect("published alignment");
    let bundle: SingingAnalysis = read_json(&input.join("EvidenceBundle.json"));
    let artifact = TranscriptArtifact {
        contract: "uta.analysis-engine.transcript".to_string(),
        version: 1,
        authority: TranscriptAuthority::CallerCanonical,
        language: caller.language.clone(),
        text: alignment.transcript.clone(),
        tokens: caller
            .tokens
            .iter()
            .map(|token| TranscriptToken {
                id: token.id.clone(),
                text: token.text.clone(),
                confidence: None,
            })
            .collect(),
        audio_segments: Vec::new(),
        confidence: None,
        source_experts: vec!["caller.canonical".to_string()],
        alternatives: Vec::new(),
        model_sha256: None,
        runtime_manifest_sha256: None,
        backend: "caller".to_string(),
    };
    let (_, mut transcript) = fuse_transcript_stage(&[artifact], None).expect("caller transcript");
    attach_caller_lyric_ranges(&mut transcript, &caller);
    let (alignment, words) = fuse_alignment_stage(&transcript, &[alignment], 0, duration)
        .expect("retained measured alignment");
    let curve = retained_pitch
        .frequency_hz
        .iter()
        .enumerate()
        .filter_map(|(index, hz)| {
            hz.map(|hz| F0Point {
                time: retained_pitch.start + index as u64 * retained_pitch.hop,
                hz: hz as f32,
                confidence: Some(retained_pitch.confidence[index] as f32),
            })
        })
        .collect();
    let fusion = SingingFusionStageOutput {
        fusion: SingingFusionEvidence {
            schema_version: 1,
            candidates: bundle.candidate_evidence,
            hard_boundaries: bundle.candidate_hard_boundaries,
        },
        alignment,
        f0_curve: curve,
        continuous_f0_source: "rmvpe".to_string(),
        provenance: Vec::new(),
    };
    let singing =
        execute_candidate_graph_stage(transcript, words, fusion, FusionDecisionMode::Algorithm)
            .expect("decode retained pool");
    let chart = crate::artifact::finalize_candidate_vocal_chart(
        &singing.track,
        "published-lyric-replay",
        None,
    )
    .expect("finalize current chart");
    chart.validate().expect("valid UTZ chart");
    let notes = chart
        .tracks
        .iter()
        .flat_map(|track| &track.phrases)
        .flat_map(|phrase| &phrase.notes)
        .collect::<Vec<_>>();
    let tokens = notes
        .iter()
        .flat_map(|note| &note.lyrics)
        .filter_map(|token| match token {
            uta_studio_chart::LyricToken::Text(token) => Some(token),
            uta_studio_chart::LyricToken::Continuation { .. } => None,
        })
        .collect::<Vec<_>>();
    let text = tokens
        .iter()
        .map(|token| token.text.as_str())
        .collect::<String>();
    let reported_words =
        notes
            .iter()
            .flat_map(|note| {
                note.lyrics.iter().filter_map(move |token| {
                    let uta_studio_chart::LyricToken::Text(token) = token else {
                        return None;
                    };
                    (note.start >= 48_000_000 && note.start < 54_000_000
            || note.start >= 103_000_000 && note.start < 107_000_000)
            .then(|| serde_json::json!({
                "id": token.id, "text": token.text, "start": note.start,
                "duration": note.duration, "timing_unresolved": token.timing_unresolved,
            }))
                })
            })
            .collect::<Vec<_>>();
    let summary = serde_json::json!({
        "reported_words": reported_words,
        "scope": "existing candidate pool decoded and chart reprojected; no new candidate construction, alignment or model execution",
        "caller_nonspace_chars": compact(&singing.track.transcript.text).chars().count(),
        "chart_nonspace_chars": compact(&text).chars().count(),
        "all_caller_text_preserved_in_order": compact(&text) == compact(&singing.track.transcript.text),
        "chart_notes": notes.len(),
        "pitched_notes": notes.iter().filter(|note| note.pitch.is_some()).count(),
        "pitched_under_100ms": notes.iter().filter(|note| note.pitch.is_some() && note.duration < 100_000).count(),
        "unresolved_text_groups": tokens.iter().filter(|token| token.timing_unresolved).count(),
        "multi_text_notes": notes.iter().filter(|note| note.lyrics.iter().filter(|token|
            matches!(token, uta_studio_chart::LyricToken::Text(text) if !text.text.is_empty())).count() > 1).count(),
    });
    std::fs::create_dir(&output).expect("unique output directory");
    for (name, value) in [
        ("vocal-chart.json", serde_json::to_value(&chart).unwrap()),
        (
            "canonical.json",
            serde_json::to_value(&singing.track).unwrap(),
        ),
        ("summary.json", summary),
    ] {
        std::fs::write(
            output.join(name),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .expect("write isolated output");
    }
    assert_eq!(
        compact(&text),
        compact(&singing.track.transcript.text),
        "all imported text must survive in original order"
    );
    let word_start = |id: &str| {
        notes.iter().find_map(|note| {
            note.lyrics
                .iter()
                .any(|token| {
                    matches!(token,
            uta_studio_chart::LyricToken::Text(text) if text.id == id)
                })
                .then_some(note.start)
        })
    };
    assert_eq!(
        word_start("aligned-word-89"),
        Some(104_640_000),
        "ni must retain its measured onset instead of sharing the preceding cut"
    );
    assert!(word_start("aligned-word-87").unwrap() < word_start("aligned-word-89").unwrap());
    assert!(
        !notes.iter().any(|note| note.start == 103_110_000
            && note.duration == 170_000
            && note
                .lyrics
                .iter()
                .all(|token| matches!(token, uta_studio_chart::LyricToken::Text(text) if text.text.is_empty()))),
        "caller line scope must not create the reported empty prefix"
    );
}
