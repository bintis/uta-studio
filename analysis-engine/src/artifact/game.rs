use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::contract::{CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult};
use crate::fusion::TimeRange;

const MAX_EVIDENCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_NOTES: usize = 1_000_000;
#[cfg(test)]
const GAME_SOURCE_ASSET_SHA256: &str =
    "5b7a21e64c6310efac399f5d12838fffa70565be162436b5a4a65f290721e7d8";
const GAME_SOURCE_COMMIT: &str = "475a8ee781fe8cca980b3b12fbe6c80c768a813a";
#[cfg(test)]
const GAME_MANIFEST_SHA256: &str =
    "aa9f3a4c2d107527913ef3947f337b41bff7b6de39de6c91ce46b82ced15ac87";
const ESTIMATOR_NOTE_BUCKETS: [usize; 6] = [32, 64, 128, 256, 512, 1_024];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameNoteEvidenceV1 {
    pub range: TimeRange,
    /// Fractional GAME MIDI estimate. This is not a finalized target note.
    pub midi: f32,
    /// Worker decision configuration, not observed/calibrated confidence.
    pub boundary_decision_threshold: f32,
    /// Worker decision configuration, not observed/calibrated confidence.
    pub presence_decision_threshold: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameEvidenceV1 {
    pub schema_version: u32,
    pub model_id: String,
    pub variant: String,
    pub source_asset_sha256: String,
    pub source_commit: String,
    pub model_manifest_sha256: String,
    pub runtime_manifest_sha256: String,
    pub backend: String,
    pub sample_rate: usize,
    pub timestep_ms: u32,
    pub d3pm_steps: usize,
    pub estimator_note_buckets: Vec<usize>,
    pub notes: Vec<GameNoteEvidenceV1>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerGameEvidence {
    schema_version: u32,
    model_id: String,
    variant: String,
    source_asset_sha256: String,
    source_commit: String,
    model_manifest_sha256: String,
    runtime_manifest_sha256: String,
    backend: String,
    sample_rate: usize,
    timestep_ms: u32,
    d3pm_steps: usize,
    estimator_note_buckets: Vec<usize>,
    boundary_decision_threshold: f32,
    presence_decision_threshold: f32,
    notes: Vec<WorkerGameNote>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerGameNote {
    start: f64,
    duration: f64,
    midi: f32,
    voiced: bool,
}

pub fn parse_game_evidence(
    path: &Path,
    source_start: u64,
    source_duration: u64,
) -> EngineResult<GameEvidenceV1> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("GAME evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(invalid("GAME evidence size is invalid"));
    }
    let raw: WorkerGameEvidence = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read GAME evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("GAME evidence JSON is invalid: {error}")))?;
    if raw.schema_version != 1
        || raw.model_id != "game"
        || raw.variant != "GAME-1.0.3-medium-onnx"
        || raw.source_commit != GAME_SOURCE_COMMIT
        || !matches!(raw.backend.as_str(), "openvino_gpu" | "openvino_cpu")
        || raw.sample_rate != 44_100
        || raw.timestep_ms != 10
        || raw.d3pm_steps != 8
        || raw.estimator_note_buckets != ESTIMATOR_NOTE_BUCKETS
        || !valid_threshold(raw.boundary_decision_threshold)
        || !valid_threshold(raw.presence_decision_threshold)
        || raw.notes.is_empty()
        || raw.notes.len() > MAX_NOTES
    {
        return Err(invalid("GAME evidence identity or shape is invalid"));
    }

    let source_end = source_start
        .checked_add(source_duration)
        .ok_or_else(|| invalid("GAME source timeline overflows"))?;
    let mut previous_end = source_start;
    let mut notes = Vec::with_capacity(raw.notes.len());
    for note in raw.notes {
        if !note.voiced
            || !note.start.is_finite()
            || !note.duration.is_finite()
            || note.start < 0.0
            || note.duration <= 0.0
            || !note.midi.is_finite()
            || !(0.0..=128.0).contains(&note.midi)
        {
            return Err(invalid("GAME note evidence contains invalid values"));
        }
        let local_start = seconds_to_canonical(note.start)?;
        let local_duration = seconds_to_canonical(note.duration)?;
        let start = source_start
            .checked_add(local_start)
            .ok_or_else(|| invalid("GAME note start overflows"))?;
        let end = start
            .checked_add(local_duration)
            .ok_or_else(|| invalid("GAME note end overflows"))?;
        if start < previous_end || end <= start || end > source_end.saturating_add(1) {
            return Err(invalid(
                "GAME notes overlap or exceed the decoded source timeline",
            ));
        }
        previous_end = end;
        notes.push(GameNoteEvidenceV1 {
            range: TimeRange { start, end },
            midi: note.midi,
            boundary_decision_threshold: raw.boundary_decision_threshold,
            presence_decision_threshold: raw.presence_decision_threshold,
        });
    }

    Ok(GameEvidenceV1 {
        schema_version: raw.schema_version,
        model_id: raw.model_id,
        variant: raw.variant,
        source_asset_sha256: raw.source_asset_sha256,
        source_commit: raw.source_commit,
        model_manifest_sha256: raw.model_manifest_sha256,
        runtime_manifest_sha256: raw.runtime_manifest_sha256,
        backend: raw.backend,
        sample_rate: raw.sample_rate,
        timestep_ms: raw.timestep_ms,
        d3pm_steps: raw.d3pm_steps,
        estimator_note_buckets: raw.estimator_note_buckets,
        notes,
    })
}

fn seconds_to_canonical(seconds: f64) -> EngineResult<u64> {
    let units = seconds * f64::from(CANONICAL_TIMEBASE);
    if !units.is_finite() || units < 0.0 || units > u64::MAX as f64 {
        return Err(invalid("GAME evidence time is invalid"));
    }
    Ok(units.round() as u64)
}

fn valid_threshold(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_finite_ordered_game_notes_without_rounding_midi() {
        let path = std::env::temp_dir().join(format!("uta-game-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "model_id": "game",
                "variant": "GAME-1.0.3-medium-onnx",
                "source_asset_sha256": GAME_SOURCE_ASSET_SHA256,
                "source_commit": GAME_SOURCE_COMMIT,
                "model_manifest_sha256": GAME_MANIFEST_SHA256,
                "runtime_manifest_sha256": "d".repeat(64),
                "backend": "openvino_gpu",
                "sample_rate": 44100,
                "timestep_ms": 10,
                "d3pm_steps": 8,
                "estimator_note_buckets": ESTIMATOR_NOTE_BUCKETS,
                "boundary_decision_threshold": 0.2,
                "presence_decision_threshold": 0.2,
                "notes": [
                    {"start":0.1,"duration":0.2,"midi":69.25,"voiced":true},
                    {"start":0.4,"duration":0.1,"midi":71.75,"voiced":true}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let evidence = parse_game_evidence(&path, 2_000_000, 500_000).unwrap();
        assert_eq!(evidence.notes[0].range.start, 2_100_000);
        assert_eq!(evidence.notes[0].midi, 69.25);
        assert_eq!(evidence.notes[1].range.end, 2_500_000);
        std::fs::remove_file(path).unwrap();
    }
}
