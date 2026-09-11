use super::{Request, audio, emit, required};
use serde_json::{Value, json};
use std::path::Path;
use uta_libtorch_runtime::{Model, rosvot, stars};
#[path = "../../../../ggml-worker/src/stars_g2p.rs"]
mod g2p;

fn progress(fraction: f32, stage: &str) {
    emit(&json!({"event":"progress", "fraction":fraction, "stage":stage}));
}

struct Word {
    id: String,
    text: String,
    start_micros: u64,
    duration_micros: u64,
}
fn rmvpe_curve(frames: &[Value]) -> Result<Vec<f32>, String> {
    frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            let hz = frame["hz"].as_f64().ok_or("RMVPE frame lacks frequency")?;
            let time = frame["time"].as_f64().ok_or("RMVPE frame lacks time")?;
            let voiced = frame["voiced"]
                .as_bool()
                .ok_or("RMVPE frame lacks voiced decision")?;
            audio::finite([time, hz])?;
            if (time - index as f64 * 0.01).abs() > 1.0e-6 {
                return Err("RMVPE frame timeline disagrees with its 10 ms cadence".into());
            }
            Ok(if voiced { hz as f32 } else { 0.0 })
        })
        .collect()
}
fn conditioning(request: &Request) -> Result<(Vec<f32>, Vec<Word>, usize), String> {
    let pitch = audio::read_json(required(&request.pitch, "real RMVPE evidence")?)?;
    let frames = pitch["frames"]
        .as_array()
        .ok_or("RMVPE evidence has no frames")?;
    let raw_pitch = rmvpe_curve(frames)?;
    audio::finite(raw_pitch.iter().map(|value| f64::from(*value)))?;
    let alignment = audio::read_json(required(
        &request.alignment,
        "real Qwen alignment evidence",
    )?)?;
    let input = alignment["words"]
        .as_array()
        .ok_or("alignment evidence has no words")?;
    let mut words = Vec::new();
    let mut unresolved = 0;
    for word in input {
        if !word["timing_issue"].is_null() {
            unresolved += 1;
            continue;
        }
        let start = word["start_seconds"]
            .as_f64()
            .ok_or("aligned word lacks start time")?;
        let end = word["end_seconds"]
            .as_f64()
            .ok_or("aligned word lacks end time")?;
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
            return Err("invalid measured aligned word interval".into());
        }
        let start_micros = (start * 1_000_000.0).round() as u64;
        let end_micros = (end * 1_000_000.0).round() as u64;
        words.push(Word {
            id: word["id"].as_str().ok_or("aligned word lacks ID")?.into(),
            text: word["text"]
                .as_str()
                .ok_or("aligned word lacks text")?
                .into(),
            start_micros,
            duration_micros: end_micros - start_micros,
        });
    }
    if words.is_empty() {
        return Err("real alignment produced no resolved words for conditioned inference".into());
    }
    Ok((raw_pitch, words, unresolved))
}
fn note(start: usize, end: usize, logits: &[f32], midi: Option<u8>) -> Result<Value, String> {
    audio::finite(logits.iter().map(|value| f64::from(*value)))?;
    Ok(json!({"start_frame":start,"end_frame":end,"pitch_logits":logits,"midi":midi}))
}
pub fn rosvot(model: Model, wav: &Path, request: &Request) -> Result<Value, String> {
    let (pitch, words, unresolved) = conditioning(request)?;
    let shared = rosvot::prepare_wav_inputs(wav, &pitch)?;
    let words = words
        .into_iter()
        .map(|word| rosvot::TranscriptWord {
            id: word.id,
            text: word.text,
            start_micros: word.start_micros,
            duration_micros: word.duration_micros,
        })
        .collect::<Vec<_>>();
    let result =
        rosvot::Rosvot::from_model(model).infer_transcript(&shared, &words, 0, progress)?;
    audio::finite(
        result
            .note_boundary_logits
            .iter()
            .map(|value| f64::from(*value)),
    )?;
    let notes = result
        .notes
        .iter()
        .map(|item| {
            note(
                item.start_frame,
                item.end_frame,
                &item.pitch_logits,
                item.midi,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(
        json!({"valid_frames":result.valid_frames,"note_boundary_logits":result.note_boundary_logits,
        "regulated_note_boundaries":result.regulated_note_boundaries,"notes":notes,
        "conditioning":{"rmvpe":request.pitch,"alignment":request.alignment,"resolved_words":words.len(),"unresolved_words_not_used":unresolved}}),
    )
}
pub fn stars(model: Model, wav: &Path, request: &Request) -> Result<Value, String> {
    let (pitch, words, unresolved) = conditioning(request)?;
    let shared = stars::prepare_wav_inputs(wav, &pitch)?;
    let words = words
        .into_iter()
        .map(|word| stars::TranscriptWord {
            id: word.id,
            text: word.text,
            start_micros: word.start_micros,
            duration_micros: word.duration_micros,
        })
        .collect::<Vec<_>>();
    let phonemizer = g2p::ChineseG2pAsset::load_embedded()?;
    let result = stars::Stars::from_model(model).infer_transcript(
        &shared,
        &words,
        0,
        true,
        |texts| {
            let phones = phonemizer.phonemize_words(texts)?;
            Ok(stars::PhonemeInput {
                phone_ids: phones.phone_ids,
                phone_to_word: phones.phone_to_word,
            })
        },
        progress,
    )?;
    audio::finite(
        result
            .note_boundary_logits
            .iter()
            .map(|value| f64::from(*value)),
    )?;
    let notes = result
        .notes
        .iter()
        .map(|item| {
            note(
                item.start_frame,
                item.end_frame,
                &item.pitch_logits,
                item.midi,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let techniques = result.techniques.ok_or("STARS did not produce requested techniques")?.into_iter().map(|item| {
        audio::finite(item.raw_logits.iter().chain(&item.source_local_scores).map(|value| f64::from(*value)))?;
        Ok(json!({"start_frame":item.start_frame,"end_frame":item.end_frame,"phoneme_id":item.phoneme_id,
            "raw_logits":item.raw_logits,"source_local_scores":item.source_local_scores}))
    }).collect::<Result<Vec<Value>,String>>()?;
    let styles = result.styles.ok_or("STARS did not produce requested styles")?.into_iter().map(|item| {
        let style = item.logits;
        let arrays = [&style.technique_group,&style.language,&style.gender,&style.emotion,&style.method,&style.pace,&style.range];
        audio::finite(arrays.iter().flat_map(|row| row.iter()).map(|value| f64::from(*value)))?;
        Ok(json!({"start_frame":item.start_frame,"end_frame":item.end_frame,"technique_group":style.technique_group,
            "language":style.language,"gender":style.gender,"emotion":style.emotion,"method":style.method,"pace":style.pace,"range":style.range}))
    }).collect::<Result<Vec<Value>,String>>()?;
    Ok(
        json!({"valid_frames":result.valid_frames,"note_boundary_logits":result.note_boundary_logits,
        "regulated_note_boundaries":result.regulated_note_boundaries,"notes":notes,"techniques":techniques,"styles":styles,
        "technique_taxonomy":stars::TECHNIQUE_TAXONOMY,
        "conditioning":{"rmvpe":request.pitch,"alignment":request.alignment,"resolved_words":words.len(),
            "unresolved_words_not_used":unresolved,"g2p":g2p::PROFILE}}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_pitch_conditioning_keeps_uv_instead_of_voicing_every_raw_estimate() {
        let frames = vec![
            json!({"time":0.0,"hz":440.,"voiced":true}),
            json!({"time":0.01,"hz":220.,"voiced":false}),
        ];
        assert_eq!(rmvpe_curve(&frames).unwrap(), [440., 0.]);
        assert!(rmvpe_curve(&[json!({"time":1.0,"hz":220.,"voiced":true})]).is_err());
    }
}
