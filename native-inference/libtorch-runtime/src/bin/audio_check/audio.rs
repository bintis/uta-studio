use serde::Serialize;
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::Command;

pub fn save_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())
}
pub fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}
pub fn finite(values: impl IntoIterator<Item = f64>) -> Result<(), String> {
    for (index, value) in values.into_iter().enumerate() {
        if !value.is_finite() {
            return Err(format!("nonfinite output at value {index}"));
        }
    }
    Ok(())
}
fn ffmpeg() -> Command {
    let mut command =
        Command::new(std::env::var_os("UTA_STUDIO_FFMPEG_PATH").unwrap_or_else(|| "ffmpeg".into()));
    command.args(["-nostdin", "-v", "error", "-n"]);
    command
}
pub fn decode(source: &Path, destination: &Path, rate: u32, channels: u16) -> Result<(), String> {
    let output = ffmpeg()
        .arg("-i")
        .arg(source)
        .args([
            "-vn",
            "-ar",
            &rate.to_string(),
            "-ac",
            &channels.to_string(),
            "-c:a",
            "pcm_f32le",
        ])
        .arg(destination)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "audio decoding failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}
pub fn read_wav(path: &Path) -> Result<(hound::WavSpec, Vec<f32>), String> {
    let mut reader = hound::WavReader::open(path).map_err(|error| error.to_string())?;
    let spec = reader.spec();
    let values = reader
        .samples::<f32>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    finite(values.iter().map(|value| f64::from(*value)))?;
    Ok((spec, values))
}
pub fn write_wav(path: &Path, spec: hound::WavSpec, samples: &[f32]) -> Result<(), String> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    let mut writer =
        hound::WavWriter::new(BufWriter::new(file), spec).map_err(|error| error.to_string())?;
    for &sample in samples {
        writer
            .write_sample(sample)
            .map_err(|error| error.to_string())?;
    }
    writer.finalize().map_err(|error| error.to_string())
}
pub fn publish_wave(
    root: &Path,
    name: &str,
    source: &Path,
    spec: hound::WavSpec,
    samples: &[f32],
) -> Result<Value, String> {
    finite(samples.iter().map(|value| f64::from(*value)))?;
    let raw = root.join(format!("{name}.f32"));
    let mut file = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&raw)
            .map_err(|error| error.to_string())?,
    );
    for sample in samples {
        file.write_all(&sample.to_le_bytes())
            .map_err(|error| error.to_string())?;
    }
    file.flush()
        .and_then(|_| file.get_ref().sync_all())
        .map_err(|error| error.to_string())?;
    let peak = samples.iter().fold(0.0_f32, |peak, value| peak.max(value.abs()));
    let gain = pcm_gain(peak);
    let flac = root.join(format!("{name}.flac"));
    let output = ffmpeg()
        .arg("-i")
        .arg(source)
        .args(["-af", &format!("volume={gain:.17}:precision=double")])
        .args([
            "-c:a",
            "flac",
            "-sample_fmt",
            "s32",
            "-bits_per_raw_sample",
            "32",
        ])
        .arg(&flac)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "FLAC publication failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let energy = samples
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>();
    Ok(
        json!({"flac":flac,"complete_float_tensor":raw,"samples":samples.len(),"sample_rate":spec.sample_rate,
        "channels":spec.channels,"duration_seconds":samples.len() as f64 / f64::from(spec.channels) / f64::from(spec.sample_rate),
        "peak":peak,"rms":(energy / samples.len().max(1) as f64).sqrt(),
        "pcm_encode_gain":gain,"unscaled_out_of_range_samples":samples.iter().filter(|value| **value < -1.0 || **value >= 1.0).count(),
        "integer_audio_clipped_samples":samples.iter().filter(|value| (f64::from(**value) * gain).abs() >= 1.0).count(),
        "gain_scope":"FLAC representation only; exact float tensor and inference inputs are unchanged",
        "finite":true,"float_tensor_is_exact":true,"flac_representation":"signed_32_bit_pcm"}),
    )
}

fn pcm_gain(peak: f32) -> f64 {
    if peak >= 1.0 { f64::from(1.0 - f32::EPSILON) / f64::from(peak) } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm_gain_preserves_quiet_audio_and_prevents_both_polarities_clipping() {
        assert_eq!(pcm_gain(0.0), 1.0);
        assert_eq!(pcm_gain(0.9), 1.0);
        for peak in [1.0, 1.3913388, 3.0] {
            assert!(f64::from(peak) * pcm_gain(peak) < 1.0);
            assert!(-f64::from(peak) * pcm_gain(peak) > -1.0);
        }
    }
    #[test]
    fn finite_check_checks_every_value() {
        assert!(finite([0.0, 1.0, f64::NAN]).is_err());
        assert!(finite([f64::INFINITY]).is_err());
        assert!(finite([0.0, -1.0]).is_ok());
    }
}
