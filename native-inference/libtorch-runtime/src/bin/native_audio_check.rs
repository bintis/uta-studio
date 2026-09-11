//! One explicit native model over complete real audio. This diagnostic never
//! substitutes a backend or manufactures dependent pitch/transcript evidence.
#[path = "audio_check/audio.rs"]
mod audio;
#[path = "audio_check/conditioned.rs"]
mod conditioned;
#[path = "audio_check/speech.rs"]
mod speech;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;
use uta_libtorch_runtime as native;
use uta_libtorch_runtime::{Backend, Library, Model, Precision};

#[derive(Deserialize, Serialize)]
struct Request {
    library: PathBuf,
    backend: String,
    device: u16,
    precision: String,
    resource: String,
    model_path: PathBuf,
    audio: PathBuf,
    #[serde(default)]
    mix: Option<PathBuf>,
    #[serde(default)]
    pitch: Option<PathBuf>,
    #[serde(default)]
    transcript: Option<PathBuf>,
    #[serde(default)]
    alignment: Option<PathBuf>,
    #[serde(default)]
    cmvn: Option<PathBuf>,
    #[serde(default)]
    vocabulary: Option<PathBuf>,
    #[serde(default)]
    language: Option<String>,
}
fn required<'a>(value: &'a Option<PathBuf>, description: &str) -> Result<&'a Path, String> {
    value
        .as_deref()
        .ok_or_else(|| format!("missing {description}"))
}
fn emit(value: &Value) {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, value).expect("write diagnostic record");
    writeln!(output)
        .and_then(|_| output.flush())
        .expect("flush diagnostic record");
}
fn progress(completed: u64, total: u64) {
    emit(&json!({"event":"progress","completed":completed,"total":total}));
}
fn geometry(resource: &str) -> Result<(u32, u16), String> {
    Ok(match resource {
        "bs_roformer_leap_xe90_vocals"
        | "bs_roformer_leap_xe90_instrumental"
        | "bs_polarformer_public_instrumental"
        | "melband_roformer_harmony"
        | "melband_roformer_denoise_aufr33"
        | "melband_roformer_dereverb_anvuew" => (44100, 2),
        "rmvpe" | "fcpe" | "qwen3_asr_1_7b" | "qwen3_forced_aligner_0_6b" | "firered_asr2_aed" => {
            (16000, 1)
        }
        "game_1_0_3_small" | "game_1_0_3_medium" | "game_1_0_3_large" | "jbm555_cectc_80" => {
            (44100, 1)
        }
        "basic_pitch" => (22050, 1),
        "stars" | "rosvot" => (24000, 1),
        other => return Err(format!("no native real-audio diagnostic route for {other}")),
    })
}
fn separate(model: Model, wav: &Path, root: &Path, resource: &str) -> Result<Value, String> {
    let direct = root.join("direct.wav");
    native::roformer::Roformer::from_model(model)?.process_wav(wav, &direct, &mut progress)?;
    emit(&json!({"event":"model_forward_complete","resource":resource}));
    let (spec, original) = audio::read_wav(wav)?;
    let (_, predicted) = audio::read_wav(&direct)?;
    if original.len() != predicted.len() {
        return Err("separation output does not cover every source sample".into());
    }
    let (name, residual_name) = match resource {
        "bs_roformer_leap_xe90_vocals" => ("guide-vocals", Some("instrumental")),
        "bs_roformer_leap_xe90_instrumental" | "bs_polarformer_public_instrumental" => {
            ("instrumental", Some("guide-vocals"))
        }
        "melband_roformer_harmony" => ("lead", Some("residual")),
        "melband_roformer_denoise_aufr33" => ("denoised", None),
        _ => ("dereverberated", None),
    };
    let mut outputs = vec![audio::publish_wave(root, name, &direct, spec, &predicted)?];
    let mut reconstruction_maximum = 0.0_f64;
    if let Some(name) = residual_name {
        let residual = original
            .iter()
            .zip(&predicted)
            .map(|(mix, direct)| mix - direct)
            .collect::<Vec<_>>();
        audio::finite(residual.iter().map(|value| f64::from(*value)))?;
        for ((mix, direct), residual) in original.iter().zip(&predicted).zip(&residual) {
            reconstruction_maximum = reconstruction_maximum
                .max((f64::from(*direct) + f64::from(*residual) - f64::from(*mix)).abs());
        }
        let path = root.join("residual.wav");
        audio::write_wav(&path, spec, &residual)?;
        outputs.push(audio::publish_wave(root, name, &path, spec, &residual)?);
    }
    Ok(
        json!({"outputs":outputs,"reconstruction_maximum_absolute_error":reconstruction_maximum,
        "exact_direct_waveform_scratch":direct,"samples":predicted.len()}),
    )
}
fn execute(model: Model, wav: &Path, root: &Path, request: &Request) -> Result<Value, String> {
    match request.resource.as_str() {
        "bs_roformer_leap_xe90_vocals"
        | "bs_roformer_leap_xe90_instrumental"
        | "bs_polarformer_public_instrumental"
        | "melband_roformer_harmony"
        | "melband_roformer_denoise_aufr33"
        | "melband_roformer_dereverb_anvuew" => separate(model, wav, root, &request.resource),
        "rmvpe" => {
            let frames = native::rmvpe::Rmvpe::from_model(model).process_wav(wav, progress)?;
            audio::finite(
                frames.iter().flat_map(|frame| {
                    [frame.time, f64::from(frame.hz), f64::from(frame.confidence)]
                }),
            )?;
            Ok(
                json!({"frames":frames.iter().map(|frame| json!({"time":frame.time,"hz":frame.hz,"confidence":frame.confidence,"voiced":frame.voiced})).collect::<Vec<_>>()}),
            )
        }
        "fcpe" => {
            let frames = native::fcpe::Fcpe::from_model(model)?.process_wav(wav, progress)?;
            audio::finite(
                frames
                    .iter()
                    .flat_map(|frame| [frame.time, f64::from(frame.hz.unwrap_or(0.))]),
            )?;
            Ok(
                json!({"frames":frames.iter().map(|frame| json!({"time":frame.time,"hz":frame.hz})).collect::<Vec<_>>()}),
            )
        }
        "basic_pitch" => {
            let frames =
                native::basic_pitch::BasicPitch::from_model(model).process_wav(wav, progress)?;
            audio::finite(frames.iter().flat_map(|frame| {
                [
                    frame.time,
                    f64::from(frame.note_max),
                    f64::from(frame.onset_max),
                    f64::from(frame.contour_score),
                ]
            }))?;
            Ok(
                json!({"frames":frames.iter().map(|frame| json!({"time":frame.time,"note_max":frame.note_max,"onset_max":frame.onset_max,
                "contour_class":frame.contour_class,"contour_score":frame.contour_score})).collect::<Vec<_>>()}),
            )
        }
        "game_1_0_3_small" | "game_1_0_3_medium" | "game_1_0_3_large" => {
            let result = native::game::Game::from_model(model)?.process_wav_with_progress(
                wav,
                &native::game::GameInferParams::default(),
                &mut progress,
            )?;
            audio::finite(result.notes.iter().map(|note| f64::from(note.pitch_midi)))?;
            Ok(
                json!({"num_frames":result.num_frames,"boundaries":result.boundaries,
                "notes":result.notes.iter().map(|note| json!({"offset_micros":note.offset_micros,"duration_micros":note.duration_micros,
                    "pitch_midi":note.pitch_midi,"voiced":note.voiced})).collect::<Vec<_>>()}),
            )
        }
        "jbm555_cectc_80" => {
            let mix = root.join("mix.wav");
            audio::decode(
                required(&request.mix, "real original mix for JBM555")?,
                &mix,
                44100,
                1,
            )?;
            let (notes, samples) =
                native::jbm555::Jbm555::from_model(model).process_wavs(&mix, wav, progress)?;
            audio::finite(notes.iter().flat_map(|note| {
                [
                    f64::from(note.onset_score),
                    f64::from(note.offset_score.unwrap_or(0.)),
                    f64::from(note.pitch_score),
                ]
            }))?;
            Ok(
                json!({"samples":samples,"mix_source":request.mix,"vocal_source":request.audio,
                "notes":notes.iter().map(|note| json!({"start_micros":note.range.start,"end_micros":note.range.end,"midi":note.midi,
                    "onset_score":note.onset_score,"offset_score":note.offset_score,"pitch_score":note.pitch_score})).collect::<Vec<_>>()}),
            )
        }
        "qwen3_asr_1_7b" => speech::asr(model, wav, request),
        "qwen3_forced_aligner_0_6b" => speech::align(model, wav, request),
        "firered_asr2_aed" => speech::fire(model, wav, request),
        "stars" => conditioned::stars(model, wav, request),
        "rosvot" => conditioned::rosvot(model, wav, request),
        other => Err(format!("unimplemented native audio route: {other}")),
    }
}
fn run(request_path: &Path, root: &Path) -> Result<(), String> {
    let request: Request =
        serde_json::from_slice(&std::fs::read(request_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    std::fs::create_dir(root).map_err(|error| error.to_string())?;
    audio::save_json(&root.join("request.json"), &request)?;
    let result = (|| {
        let (rate, channels) = geometry(&request.resource)?;
        let started = Instant::now();
        let wav = root.join("input.wav");
        audio::decode(&request.audio, &wav, rate, channels)?;
        let reader = hound::WavReader::open(&wav).map_err(|error| error.to_string())?;
        let samples = reader.duration();
        drop(reader);
        let preparation_seconds = started.elapsed().as_secs_f64();
        emit(
            &json!({"event":"begin","resource":request.resource,"backend":request.backend,"device":request.device,"precision":request.precision,
            "samples_per_channel":samples,"sample_rate":rate,"channels":channels,"source":request.audio}),
        );
        let started = Instant::now();
        let library = Library::load(&request.library)?;
        let runtime_seconds = started.elapsed().as_secs_f64();
        let started = Instant::now();
        let model = library.open(
            &request.resource,
            &request.model_path,
            Backend::parse(&request.backend)?,
            request.device,
            Precision::parse(&request.precision)?,
        )?;
        let model_seconds = started.elapsed().as_secs_f64();
        audio::save_json(&root.join("build.json"), library.build_info())?;
        emit(
            &json!({"event":"loaded","runtime_seconds":runtime_seconds,"model_seconds":model_seconds,"build":library.build_info()}),
        );
        let started = Instant::now();
        let evidence = execute(model, &wav, root, &request)?;
        let execution_seconds = started.elapsed().as_secs_f64();
        audio::save_json(&root.join("evidence.json"), &evidence)?;
        let summary = json!({"status":"passed","resource":request.resource,"backend":request.backend,"device":request.device,"precision":request.precision,
            "source":request.audio,"samples_per_channel":samples,"duration_seconds":f64::from(samples)/f64::from(rate),
            "preparation_seconds":preparation_seconds,"runtime_seconds":runtime_seconds,"model_seconds":model_seconds,"execution_seconds":execution_seconds,
            "timing_scope":"model adapter, postprocessing, model destruction and audio publication; excludes initial resampling and runtime/weight load",
            "evidence":"evidence.json","production_qualified":false});
        audio::save_json(&root.join("result.json"), &summary)?;
        emit(&summary);
        Ok(())
    })();
    if let Err(error) = &result {
        let failure = json!({"status":"failed","resource":request.resource,"error":error});
        audio::save_json(&root.join("failure.json"), &failure)?;
        emit(&failure);
    }
    result
}
fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    let result = if arguments.len() == 3 {
        run(Path::new(&arguments[1]), Path::new(&arguments[2]))
    } else {
        Err("usage: native_audio_check REQUEST_JSON NEW_OUTPUT_DIRECTORY".into())
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_current_model_families_have_explicit_canonical_audio() {
        assert_eq!(
            geometry("bs_polarformer_public_instrumental").unwrap(),
            (44100, 2)
        );
        assert_eq!(geometry("qwen3_forced_aligner_0_6b").unwrap(), (16000, 1));
        assert_eq!(geometry("stars").unwrap(), (24000, 1));
        assert!(geometry("auto").is_err());
    }
}
