//! Frame-domain work around the RoFormer graph.
//!
//! Everything here is host-side and independent of GGML: packing the STFT into
//! the model's input layout, applying the predicted mask and inverting it, and
//! the bounded overlap-add that turns a track into chunks and back.

use crate::stft::{Spectrogram, compute_istft};

use super::Config;

pub(super) fn prepare_model_input(
    spectra: &[Spectrogram; 2],
    frame_count: usize,
    total_dimension: usize,
    frequency_indices: &[usize],
) -> Result<Vec<f32>, String> {
    let mut output = vec![0.0_f32; frame_count * total_dimension];
    for frame in 0..frame_count {
        for (frequency_position, stereo_frequency) in frequency_indices.iter().copied().enumerate()
        {
            let raw_frequency = stereo_frequency / 2;
            let channel = stereo_frequency % 2;
            if raw_frequency >= spectra[channel].n_freq {
                return Err("RoFormer frequency index exceeds the STFT".to_string());
            }
            let (real, imaginary) = spectra[channel].get(raw_frequency, frame);
            let destination = frame * total_dimension + frequency_position * 2;
            output[destination] = real;
            output[destination + 1] = imaginary;
        }
    }
    Ok(output)
}

pub(super) fn reconstruct_stems(
    mask: &[f32],
    spectra: &[Spectrogram; 2],
    output_frames: usize,
    config: &Config,
) -> Result<Vec<Vec<f32>>, String> {
    let frequency_count = config.fft_size / 2 + 1;
    let frame_count = spectra[0].n_frames;
    let feature_count = config.frequency_indices.len() * 2;
    let stride_time = feature_count * config.stem_count;
    if mask.len() != stride_time * frame_count {
        return Err("RoFormer mask tensor size is invalid".to_string());
    }
    let mut outputs = Vec::with_capacity(config.stem_count);
    for stem in 0..config.stem_count {
        let mut channels = [
            Spectrogram {
                data: vec![0.0; frequency_count * frame_count * 2],
                n_freq: frequency_count,
                n_frames: frame_count,
            },
            Spectrogram {
                data: vec![0.0; frequency_count * frame_count * 2],
                n_freq: frequency_count,
                n_frames: frame_count,
            },
        ];
        for channel in 0..2 {
            for (frequency_position, stereo_frequency) in
                config.frequency_indices.iter().copied().enumerate()
            {
                if stereo_frequency % 2 != channel {
                    continue;
                }
                let raw_frequency = stereo_frequency / 2;
                for frame in 0..frame_count {
                    let source =
                        frame * stride_time + stem * feature_count + frequency_position * 2;
                    channels[channel].add(raw_frequency, frame, mask[source], mask[source + 1]);
                }
            }
            for frequency in 0..frequency_count {
                let denominator = config.bands_per_frequency[frequency].max(1) as f32;
                for frame in 0..frame_count {
                    let (mask_real, mask_imaginary) = channels[channel].get(frequency, frame);
                    let (source_real, source_imaginary) = spectra[channel].get(frequency, frame);
                    channels[channel].set(
                        frequency,
                        frame,
                        (source_real * mask_real - source_imaginary * mask_imaginary) / denominator,
                        (source_real * mask_imaginary + source_imaginary * mask_real) / denominator,
                    );
                }
            }
            if config.zero_dc {
                for frame in 0..frame_count {
                    channels[channel].set(0, frame, 0.0, 0.0);
                }
            }
        }
        let reconstructed = channels.map(|spectrum| {
            compute_istft(
                &spectrum,
                config.fft_size,
                config.hop_length,
                config.window_length,
                output_frames,
            )
        });
        let mut interleaved = Vec::with_capacity(output_frames * 2);
        for frame in 0..output_frames {
            interleaved.push(reconstructed[0][frame]);
            interleaved.push(reconstructed[1][frame]);
        }
        outputs.push(interleaved);
    }
    Ok(outputs)
}

pub(super) fn process_overlap_add(
    input: &[f32],
    chunk_size: usize,
    overlap: usize,
    mut process: impl FnMut(&[f32]) -> Result<Vec<Vec<f32>>, String>,
    progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<Vec<f32>>, String> {
    if input.is_empty() || input.len() % 2 != 0 || overlap == 0 {
        return Err("RoFormer overlap-add input is invalid".to_string());
    }
    let step = chunk_size / overlap;
    if step == 0 {
        return Err("RoFormer overlap-add step is zero".to_string());
    }
    let fade = chunk_size / 10;
    let border = chunk_size - step;
    let input_frames = input.len() / 2;
    let padded = if input_frames > 2 * border && border > 0 {
        reflect_pad_track(input, border)
    } else {
        input.to_vec()
    };
    let left_pad = if padded.len() == input.len() {
        0
    } else {
        border
    };
    let padded_frames = padded.len() / 2;
    let total_chunks = padded_frames.div_ceil(step) as u64;
    let base_window = crossfade_window(chunk_size, fade);
    let mut counter = vec![0.0_f32; padded.len()];
    let mut outputs: Vec<Vec<f32>> = Vec::new();
    let mut offset = 0_usize;
    let mut completed = 0_u64;
    while offset < padded_frames {
        let part_length = chunk_size.min(padded_frames - offset);
        let mut chunk = vec![0.0_f32; chunk_size * 2];
        chunk[..part_length * 2].copy_from_slice(&padded[offset * 2..(offset + part_length) * 2]);
        if part_length < chunk_size && part_length > chunk_size / 2 + 1 {
            for pad in 0..chunk_size - part_length {
                let source = part_length.saturating_sub(2 + pad);
                chunk[(part_length + pad) * 2] = chunk[source * 2];
                chunk[(part_length + pad) * 2 + 1] = chunk[source * 2 + 1];
            }
        }
        let chunk_outputs = process(&chunk)?;
        if chunk_outputs.is_empty()
            || chunk_outputs
                .iter()
                .any(|output| output.len() != chunk_size * 2)
        {
            return Err("RoFormer chunk output shape is invalid".to_string());
        }
        if outputs.is_empty() {
            outputs = vec![vec![0.0_f32; padded.len()]; chunk_outputs.len()];
        } else if outputs.len() != chunk_outputs.len() {
            return Err("RoFormer stem count changed between chunks".to_string());
        }
        let mut window = base_window.clone();
        if offset == 0 {
            window[..fade].fill(1.0);
        } else if offset + step >= padded_frames {
            window[chunk_size - fade..].fill(1.0);
        }
        for frame in 0..part_length {
            let weight = window[frame];
            let destination = (offset + frame) * 2;
            let source = frame * 2;
            for (output, chunk_output) in outputs.iter_mut().zip(&chunk_outputs) {
                output[destination] += chunk_output[source] * weight;
                output[destination + 1] += chunk_output[source + 1] * weight;
            }
            counter[destination] += weight;
            counter[destination + 1] += weight;
        }
        offset += step;
        completed += 1;
        progress(completed, total_chunks);
    }
    for output in &mut outputs {
        for frame in 0..input_frames {
            let padded_index = (left_pad + frame) * 2;
            let destination = frame * 2;
            output[destination] = output[padded_index] / counter[padded_index].max(1.0e-4);
            output[destination + 1] =
                output[padded_index + 1] / counter[padded_index + 1].max(1.0e-4);
        }
        output.truncate(input.len());
    }
    Ok(outputs)
}

fn crossfade_window(size: usize, fade: usize) -> Vec<f32> {
    let mut window = vec![1.0_f32; size];
    for index in 0..fade {
        let fade_in = if fade > 1 {
            index as f32 / (fade - 1) as f32
        } else {
            1.0
        };
        window[index] *= fade_in;
        window[size - fade + index] *= 1.0 - fade_in;
    }
    window
}

fn reflect_pad_track(input: &[f32], amount: usize) -> Vec<f32> {
    let frames = input.len() / 2;
    let mut output = vec![0.0_f32; (frames + amount * 2) * 2];
    output[amount * 2..(amount + frames) * 2].copy_from_slice(input);
    for index in 0..amount {
        let left_source = (1 + index).min(frames - 1);
        let left_destination = amount - 1 - index;
        let right_source = frames.saturating_sub(2 + index);
        let right_destination = amount + frames + index;
        output[left_destination * 2..left_destination * 2 + 2]
            .copy_from_slice(&input[left_source * 2..left_source * 2 + 2]);
        output[right_destination * 2..right_destination * 2 + 2]
            .copy_from_slice(&input[right_source * 2..right_source * 2 + 2]);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_add_preserves_identity_chunks() {
        let frames = 4096;
        let input = (0..frames * 2)
            .map(|index| (index as f32 * 0.01).sin())
            .collect::<Vec<_>>();
        let output = process_overlap_add(
            &input,
            1024,
            2,
            |chunk| Ok(vec![chunk.to_vec()]),
            &mut |_, _| {},
        )
        .unwrap();
        let maximum = input
            .iter()
            .zip(&output[0])
            .map(|(left, right)| (left - right).abs())
            .fold(0.0_f32, f32::max);
        assert!(maximum < 1.0e-5, "maximum difference {maximum}");
    }
}
