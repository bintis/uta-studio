//! Read-only replay of explicitly listed current worker evidence. No models or
//! audio are executed. Usage: replay_singing INPUTS_JSON NEW_OUTPUT_DIRECTORY
//! The caller supplies matched source timelines and conditioned expert inputs;
//! this diagnostic does not resolve models or assert production input binding.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use uta_analysis_engine::artifact::{
    AcousticEvidence, Jbm555Evidence, Jbm555ExpectedInputs, finalize_candidate_vocal_chart,
    parse_advanced_note_evidence, parse_alignment_artifact, parse_basic_pitch_evidence,
    parse_fcpe_pitch, parse_game_evidence, parse_rmvpe_pitch, parse_transcript_artifact,
    write_json_artifact,
};
use uta_analysis_engine::candidate_pipeline::{
    FusionDecisionMode, execute_candidate_graph_stage,
    execute_singing_fusion_stage_with_timed_notes, fuse_alignment_stage, fuse_transcript_stage,
};

#[derive(Deserialize)]
struct ReplayInputs {
    source_start: u64,
    source_duration: u64,
    transcript: PathBuf,
    alignment: PathBuf,
    pitch: PathBuf,
    secondary_pitch: Option<PathBuf>,
    acoustic: Option<PathBuf>,
    basic_pitch: Option<PathBuf>,
    game: Option<PathBuf>,
    jbm: Option<PathBuf>,
    #[serde(default)]
    note_experts: BTreeMap<String, PathBuf>,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: replay_singing INPUTS_JSON NEW_OUTPUT_DIRECTORY".into());
    }
    let input_path = PathBuf::from(&args[0]);
    let input: ReplayInputs = read_json(&input_path)?;
    let output = PathBuf::from(&args[1]);
    let start = input.source_start;
    let duration = input.source_duration;
    let pitch = parse_rmvpe_pitch(&input.pitch, start, duration)?;
    let transcript = parse_transcript_artifact(&input.transcript)?;
    let alignment = parse_alignment_artifact(&input.alignment, start, duration)?;
    let (transcript, canonical) = fuse_transcript_stage(&[transcript], None)?;
    let (alignment, words) = fuse_alignment_stage(&canonical, &[alignment], start, duration)?;
    let acoustic: Option<AcousticEvidence> =
        input.acoustic.as_deref().map(read_json).transpose()?;
    let game = input
        .game
        .as_deref()
        .map(|path| parse_game_evidence(path, start, duration))
        .transpose()?;
    let secondary = input
        .secondary_pitch
        .as_deref()
        .map(|path| parse_fcpe_pitch(path, start, duration))
        .transpose()?;
    let activation = input
        .basic_pitch
        .as_deref()
        .map(|path| parse_basic_pitch_evidence(path, start, duration))
        .transpose()?;
    let advanced = input
        .note_experts
        .iter()
        .map(|(model, path)| parse_advanced_note_evidence(path, model))
        .collect::<Result<Vec<_>, _>>()?;
    let mut timed = Vec::new();
    if let Some(path) = &input.jbm {
        let evidence: Jbm555Evidence = read_json(path)?;
        timed.push(evidence.timed_note_evidence(Jbm555ExpectedInputs {
            source_start: start,
            source_duration: duration,
            mix_audio_identity: &evidence.mix_audio_identity,
            vocal_audio_identity: &evidence.vocal_audio_identity,
            separator_model_generation: &evidence.separator_model_generation,
            vocal_preparation_generation: &evidence.vocal_preparation_generation,
        })?);
    }
    let fusion = execute_singing_fusion_stage_with_timed_notes(
        &transcript,
        &alignment,
        &words,
        Some(&pitch),
        secondary.as_ref(),
        activation.as_ref(),
        game.as_ref(),
        acoustic.as_ref(),
        &advanced,
        &timed,
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
    let mut proposals_by_duration = BTreeMap::<(&str, u64, u64), usize>::new();
    for candidate in &singing.fusion.candidates {
        *proposals_by_duration
            .entry((
                candidate.boundary_source.as_str(),
                candidate.range.start,
                candidate.range.end,
            ))
            .or_default() += 1;
    }
    let summary = serde_json::json!({
        "scope": "explicit cached evidence only; no model/audio execution or production input-binding claim",
        "inputs": input_path, "source_start": start, "source_duration": duration,
        "raw_game_notes": game.as_ref().map(|evidence| evidence.notes.len()),
        "alignment_items": alignment.items.len(), "measured_words": alignment.measured_items().count(),
        "unresolved_words": alignment.items.iter().filter(|item| item.timing_issue.is_some()).count(),
        "candidate_count": singing.fusion.candidates.len(), "candidate_sources": pool_sources,
        "maximum_pitch_states_per_duration": proposals_by_duration.values().max(),
        "selected_sources": chosen_sources,
        "selected_canonical_notes": singing.track.notes.len(), "chart_objects": notes.len(),
        "pitched_notes": pitched.len(), "pitched_duration_micros": pitched.iter().map(|note| note.duration).sum::<u64>(),
        "short_pitched_counts_below_micros": short, "same_pitch_pairs_gap_at_most_twenty_ms": same_pitch_pairs,
        "unassigned_lyric_notes": singing.track.notes.iter().filter(|note| note.word_id.is_none()).count(),
        "review_regions": singing.review_regions.len(),
        "warning": "Counts are diagnostic signals, not transcription accuracy or listening qualification."
    });
    // Refuse an existing destination. Inputs are never modified; all writes use
    // the Engine's atomic publisher and retain the complete candidate evidence.
    std::fs::create_dir(&output)?;
    write_json_artifact(
        &output,
        Path::new("summary.json"),
        "application/json",
        &summary,
    )?;
    write_json_artifact(
        &output,
        Path::new("fusion.json"),
        "application/json",
        &singing.fusion,
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
