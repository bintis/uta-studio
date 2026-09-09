use std::path::Path;

pub(crate) fn read_f32_wav(
    path: &Path,
    expected_sample_rate: u32,
    expected_channels: u16,
) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|error| format!("could not open decoded inference WAV: {error}"))?;
    let spec = reader.spec();
    if spec.sample_rate != expected_sample_rate
        || spec.channels != expected_channels
        || spec.sample_format != hound::SampleFormat::Float
        || spec.bits_per_sample != 32
    {
        return Err(format!(
            "decoded inference WAV must be {expected_sample_rate} Hz, {expected_channels} channel(s), float32"
        ));
    }
    let samples = reader
        .samples::<f32>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("could not read decoded inference WAV: {error}"))?;
    if samples.is_empty() || samples.len() % expected_channels as usize != 0 {
        return Err("decoded inference WAV has an invalid sample count".to_string());
    }
    Ok(samples)
}

pub(crate) fn write_f32_wav(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    samples: &[f32],
) -> Result<(), String> {
    if path.exists() || samples.is_empty() || samples.len() % channels as usize != 0 {
        return Err("inference WAV output path or sample count is invalid".to_string());
    }
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|error| format!("could not create inference WAV: {error}"))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|error| format!("could not write inference WAV: {error}"))?;
    }
    writer
        .finalize()
        .map_err(|error| format!("could not finalize inference WAV: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_wav_round_trip_preserves_contract() {
        let path = std::env::temp_dir().join(format!(
            "uta-ggml-wav-{}-{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let samples = [0.25, -0.5, 0.75, -1.0];
        write_f32_wav(&path, 44_100, 2, &samples).unwrap();
        assert_eq!(read_f32_wav(&path, 44_100, 2).unwrap(), samples);
        std::fs::remove_file(path).unwrap();
    }
}
