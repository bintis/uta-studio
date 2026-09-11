//! Frame-domain work around the RoFormer graph.
//!
//! Everything here is host-side and independent of GGML: packing the STFT into
//! the model's input layout, applying the predicted mask and inverting it, and
//! the bounded overlap-add that turns a track into chunks and back.

use crate::stft::{Spectrogram, compute_istft};
use crate::stage_profile::StageProfile;

use super::Config;

pub(super) fn prepare_model_input(
    spectra: &[Spectrogram; 2],
    frame_count: usize,
    total_dimension: usize,
    frequency_indices: &[usize],
) -> Result<Vec<f32>, String> {
    let mut output = vec![0.0_f32; frame_count * total_dimension];
    // Transpose frequency-major spectra in bounded frame strips. Each source
    // run is contiguous; only a small set of destination rows stays active.
    const FRAME_STRIP: usize = 32;
    for begin in (0..frame_count).step_by(FRAME_STRIP) {
        let end = (begin + FRAME_STRIP).min(frame_count);
        for (frequency_position, stereo_frequency) in frequency_indices.iter().copied().enumerate()
        {
            let raw_frequency = stereo_frequency / 2;
            let channel = stereo_frequency % 2;
            if raw_frequency >= spectra[channel].n_freq {
                return Err("RoFormer frequency index exceeds the STFT".to_string());
            }
            for frame in begin..end {
                let (real, imaginary) = spectra[channel].get(raw_frequency, frame);
                let destination = frame * total_dimension + frequency_position * 2;
                output[destination] = real;
                output[destination + 1] = imaginary;
            }
        }
    }
    Ok(output)
}

fn accumulate_channel_mask(
    mask: &[f32], frequency_indices: &[usize], stem: usize,
    stride_time: usize, channel: usize, spectrum: &mut Spectrogram,
) {
    let feature_count = frequency_indices.len() * 2;
    const FRAME_STRIP: usize = 32;
    for begin in (0..spectrum.n_frames).step_by(FRAME_STRIP) {
        let end = (begin + FRAME_STRIP).min(spectrum.n_frames);
        // Different frames are independent. Within each frame/frequency the
        // original band order is retained, including overlapping mel bands.
        for (position, stereo_frequency) in frequency_indices.iter().copied().enumerate() {
            if stereo_frequency % 2 != channel { continue; }
            let frequency = stereo_frequency / 2;
            for frame in begin..end {
                let source = frame * stride_time + stem * feature_count + position * 2;
                spectrum.add(frequency, frame, mask[source], mask[source + 1]);
            }
        }
    }
}

pub(super) fn reconstruct_stems(
    mask: &[f32],
    spectra: &[Spectrogram; 2],
    output_frames: usize,
    config: &Config,
    profile: &mut StageProfile,
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
        let mark = profile.mark();
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
            accumulate_channel_mask(
                mask, &config.frequency_indices, stem, stride_time, channel, &mut channels[channel],
            );
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
        profile.record("mask_reconstruct", mark);
        let mark = profile.mark();
        let reconstructed = channels.map(|spectrum| {
            compute_istft(
                &spectrum,
                config.fft_size,
                config.hop_length,
                config.window_length,
                output_frames,
            )
        });
        profile.record("istft", mark);
        let mark = profile.mark();
        let mut interleaved = Vec::with_capacity(output_frames * 2);
        for frame in 0..output_frames {
            interleaved.push(reconstructed[0][frame]);
            interleaved.push(reconstructed[1][frame]);
        }
        outputs.push(interleaved);
        profile.record("interleave", mark);
    }
    Ok(outputs)
}

pub(super) fn process_overlap_add(
    input: &[f32],
    chunk_size: usize,
    overlap: usize,
    process: impl FnMut(&[f32]) -> Result<Vec<Vec<f32>>, String>,
    progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<Vec<f32>>, String> {
    super::overlap::process(input, chunk_size, overlap, process, progress)
}

pub(super) fn crossfade_window(size: usize, fade: usize) -> Vec<f32> {
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

pub(super) fn reflect_pad_track(input: &[f32], amount: usize) -> Vec<f32> {
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
#[path = "frontend_bench.rs"]
mod frontend_bench;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_features_preserve_every_storage_bit_across_strips_and_band_orders() {
        for frame_count in [1, 31, 32, 33, 257, 1722] {
            let mut spectra: [Spectrogram; 2] = std::array::from_fn(|_| Spectrogram {
                data: vec![0.0; 11 * frame_count * 2], n_freq: 11, n_frames: frame_count,
            });
            let encodings = [0, 0x80000000, 0x3f800001, 0x00000001, 0x7f800000, 0x7fc01234];
            for (channel, spectrum) in spectra.iter_mut().enumerate() {
                for (index, value) in spectrum.data.iter_mut().enumerate() {
                    *value = f32::from_bits(encodings[(index + channel) % encodings.len()]);
                }
            }
            let indices = [21, 0, 7, 7, 2, 1, 20];
            let width = indices.len() * 2;
            let actual = prepare_model_input(&spectra, frame_count, width, &indices).unwrap();
            let mut expected = Vec::new();
            for frame in 0..frame_count {
                for frequency in indices {
                    let (real, imaginary) = spectra[frequency % 2].get(frequency / 2, frame);
                    expected.extend([real.to_bits(), imaginary.to_bits()]);
                }
            }
            assert_eq!(actual.iter().map(|value| value.to_bits()).collect::<Vec<_>>(), expected);
        }
    }

    #[test]
    fn packed_features_report_an_out_of_range_frequency() {
        let spectra: [Spectrogram; 2] = std::array::from_fn(|_| Spectrogram {
            data: vec![0.0; 3 * 33 * 2], n_freq: 3, n_frames: 33,
        });
        assert!(prepare_model_input(&spectra, 33, 2, &[6]).unwrap_err().contains("frequency index"));
    }

    #[test]
    fn tiled_mask_accumulation_preserves_order_and_storage_bits() {
        let indices = [5, 0, 2, 5, 0, 1, 4, 0];
        let width = indices.len() * 2;
        let stems = 2;
        for frames in [1, 31, 32, 33, 257, 1722] {
            let values = [1.0e8_f32, 1.0, -1.0e8, -0.0, 1.0e-30, -1.0e-30, 0.25];
            let mask = (0..frames * width * stems).map(|index| values[index % values.len()]).collect::<Vec<_>>();
            let original = mask.iter().map(|value| value.to_bits()).collect::<Vec<_>>();
            for stem in 0..stems {
                for channel in 0..2 {
                    let mut actual = Spectrogram { data: vec![0.0; 3 * frames * 2], n_freq: 3, n_frames: frames };
                    let mut expected = vec![0.0_f32; actual.data.len()];
                    // Scalar oracle visits a complete frame at a time; every
                    // frequency keeps its original (non-associative) band sum.
                    for frame in 0..frames {
                        for (position, frequency) in indices.iter().copied().enumerate() {
                            if frequency % 2 != channel { continue; }
                            let source = frame * width * stems + stem * width + position * 2;
                            let destination = (frequency / 2 * frames + frame) * 2;
                            expected[destination] += mask[source];
                            expected[destination + 1] += mask[source + 1];
                        }
                    }
                    accumulate_channel_mask(&mask, &indices, stem, width * stems, channel, &mut actual);
                    assert_eq!(actual.data.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
                        expected.iter().map(|value| value.to_bits()).collect::<Vec<_>>());
                }
            }
            assert_eq!(mask.iter().map(|value| value.to_bits()).collect::<Vec<_>>(), original);
        }
    }

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
