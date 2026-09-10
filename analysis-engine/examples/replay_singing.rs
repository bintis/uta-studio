//! Read-only evidence replay for diagnosing note segmentation. No model or
//! audio execution is performed. All published artifacts go to a NEW directory.
//! Usage: replay_singing EVIDENCE_ROOT OUTPUT_ROOT TRANSCRIPT ALIGNMENT DURATION_MICROS

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use uta_analysis_engine::artifact::{
    AcousticEvidence, PitchEvidence, finalize_candidate_vocal_chart, parse_alignment_artifact,
    parse_basic_pitch_evidence, parse_fcpe_pitch, parse_game_evidence, parse_transcript_artifact,
    write_json_artifact,
};
use uta_analysis_engine::candidate_pipeline::{
    execute_candidate_graph_stage, execute_singing_fusion_stage_with_timed_notes,
    fuse_alignment_stage, fuse_transcript_stage,
};
use uta_analysis_engine::candidate_pipeline::FusionDecisionMode;

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 5 {
        return Err(
            "usage: replay_singing EVIDENCE_ROOT OUTPUT_ROOT TRANSCRIPT ALIGNMENT DURATION_MICROS"
                .into(),
        );
    }
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let duration: u64 = args[4].to_str().ok_or("duration must be UTF-8")?.parse()?;
    let pitch: PitchEvidence = read_json(&input.join("pitch/pitch-evidence.json"))?;
    let start = pitch.start;
    let transcript = parse_transcript_artifact(&PathBuf::from(&args[2]))?;
    let alignment = parse_alignment_artifact(&PathBuf::from(&args[3]), start, duration)?;
    let (transcript, canonical) = fuse_transcript_stage(&[transcript], None)?;
    let (alignment, words) = fuse_alignment_stage(&canonical, &[alignment], start, duration)?;
    let acoustic: AcousticEvidence = read_json(&input.join("evidence/acoustic-evidence.json"))?;
    let game = parse_game_evidence(
        &input.join("worker/game/game-note-evidence.json"),
        start,
        duration,
    )?;
    let secondary_path = input.join("worker/fcpe/fcpe-pitch-evidence.json");
    let secondary = secondary_path
        .is_file()
        .then(|| parse_fcpe_pitch(&secondary_path, start, duration))
        .transpose()?;
    let activation_path = input.join("worker/basic-pitch/basic-pitch-activation-evidence.json");
    let activation = activation_path
        .is_file()
        .then(|| parse_basic_pitch_evidence(&activation_path, start, duration))
        .transpose()?;
    let fusion = execute_singing_fusion_stage_with_timed_notes(
        &transcript,
        &alignment,
        &words,
        Some(&pitch),
        secondary.as_ref(),
        activation.as_ref(),
        Some(&game),
        Some(&acoustic),
        &[],
        &[],
        &[],
        &[],
        start,
        duration,
        "rmvpe",
    )?;
    let singing =
        execute_candidate_graph_stage(canonical, words, fusion, FusionDecisionMode::Algorithm)?;
    let chart = finalize_candidate_vocal_chart(&singing.track, "local-evidence-replay", None)?;
    let mut pool_sources = BTreeMap::<String, usize>::new();
    let mut chosen_sources = BTreeMap::<String, usize>::new();
    for candidate in &singing.fusion.candidates {
        *pool_sources
            .entry(candidate.boundary_source.clone())
            .or_default() += 1;
    }
    for note in &singing.track.notes {
        *chosen_sources
            .entry(note.evidence.boundary_source.clone())
            .or_default() += 1;
    }
    let notes = chart
        .tracks
        .iter()
        .flat_map(|track| &track.phrases)
        .flat_map(|phrase| &phrase.notes)
        .collect::<Vec<_>>();
    let pitched = notes
        .iter()
        .copied()
        .filter(|note| note.pitch.is_some())
        .collect::<Vec<_>>();
    let short = [30_000, 50_000, 80_000, 100_000, 150_000]
        .into_iter()
        .map(|threshold| {
            (
                threshold.to_string(),
                pitched
                    .iter()
                    .filter(|note| note.duration < threshold)
                    .count(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let same_pitch_pairs = pitched
        .windows(2)
        .filter(|pair| {
            pair[0].pitch.unwrap().midi == pair[1].pitch.unwrap().midi
                && pair[1].start >= pair[0].start + pair[0].duration
                && pair[1].start - pair[0].start - pair[0].duration <= 20_000
        })
        .count();
    let summary = serde_json::json!({
        "scope": "cached independent experts only; no new model execution, no STARS/ROSVOT conditioned on stale alignment",
        "evidence_root": input, "source_start": start, "source_duration": duration,
        "transcript_path": PathBuf::from(&args[2]), "alignment_path": PathBuf::from(&args[3]),
        "alignment_items": alignment.items.len(), "measured_words": alignment.measured_items().count(),
        "unresolved_words": alignment.items.iter().filter(|item| item.timing_issue.is_some()).count(),
        "candidate_count": singing.fusion.candidates.len(), "candidate_sources": pool_sources,
        "selected_sources": chosen_sources,
        "selected_canonical_notes": singing.track.notes.len(), "chart_objects": notes.len(),
        "pitched_notes": pitched.len(), "pitched_duration_micros": pitched.iter().map(|note| note.duration).sum::<u64>(),
        "short_pitched_counts_below_micros": short, "same_pitch_pairs_gap_at_most_twenty_ms": same_pitch_pairs,
        "unassigned_lyric_notes": singing.track.notes.iter().filter(|note| note.word_id.is_none()).count(),
        "review_regions": singing.review_regions.len(),
        "warning": "Counts are diagnostic signals, not measured transcription accuracy or listening qualification."
    });
    // create_dir intentionally refuses an existing destination. Source files
    // are never modified, and artifact writes use the Engine's atomic publisher.
    std::fs::create_dir(&output)?;
    write_json_artifact(
        &output,
        Path::new("summary.json"),
        "application/json",
        &summary,
    )?;
    write_json_artifact(
        &output,
        Path::new("canonical.json"),
        "application/json",
        &singing.track,
    )?;
    write_json_artifact(
        &output,
        Path::new("vocal-chart.json"),
        utz::VOCAL_CHART_MEDIA_TYPE,
        &chart,
    )?;
    write_json_artifact(
        &output,
        Path::new("alignment.json"),
        "application/json",
        &alignment,
    )?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
