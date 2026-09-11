//! Explicit CPU-only traversal diagnostic, not a throughput acceptance test.
use super::*;
use std::hint::black_box;
use std::time::Instant;

fn scalar_pack(spectra: &[Spectrogram; 2], frames: usize, indices: &[usize]) -> Vec<f32> {
    let width = indices.len() * 2;
    let mut output = vec![0.0; frames * width];
    for frame in 0..frames {
        for (position, frequency) in indices.iter().copied().enumerate() {
            let (real, imaginary) = spectra[frequency % 2].get(frequency / 2, frame);
            output[frame * width + position * 2] = real;
            output[frame * width + position * 2 + 1] = imaginary;
        }
    }
    output
}

fn accumulate(
    mask: &[f32],
    frames: usize,
    frequencies: usize,
    indices: &[usize],
    tiled: bool,
) -> Vec<f32> {
    let width = indices.len() * 2;
    let mut output = Vec::new();
    for channel in 0..2 {
        let mut spectrum = Spectrogram {
            data: vec![0.0; frequencies * frames * 2],
            n_freq: frequencies,
            n_frames: frames,
        };
        if tiled {
            // Diagnostic-only rejected candidate; not model-routed.
            for begin in (0..frames).step_by(32) {
                for (position, frequency) in indices.iter().copied().enumerate() {
                    if frequency % 2 != channel {
                        continue;
                    }
                    for frame in begin..(begin + 32).min(frames) {
                        let source = frame * width + position * 2;
                        spectrum.add(frequency / 2, frame, mask[source], mask[source + 1]);
                    }
                }
            }
        } else {
            for (position, frequency) in indices.iter().copied().enumerate() {
                if frequency % 2 != channel {
                    continue;
                }
                for frame in 0..frames {
                    let source = frame * width + position * 2;
                    spectrum.add(frequency / 2, frame, mask[source], mask[source + 1]);
                }
            }
        }
        output.extend(spectrum.data);
    }
    output
}

#[test]
#[ignore = "explicit CPU-only full-axis numerical and fixed ABBA timing diagnostic"]
fn full_shape_frontend_traversals() {
    let frames = 1722;
    let frequencies = 1025;
    let spectra: [Spectrogram; 2] = std::array::from_fn(|channel| Spectrogram {
        data: (0..frequencies * frames * 2)
            .map(|index| ((index + channel) % 257) as f32 * 0.001 - 0.1)
            .collect(),
        n_freq: frequencies,
        n_frames: frames,
    });
    let indices = (0..frequencies * 2).collect::<Vec<_>>();
    let reference = scalar_pack(&spectra, frames, &indices);
    let packed = prepare_model_input(&spectra, frames, indices.len() * 2, &indices).unwrap();
    assert!(
        reference
            .iter()
            .zip(&packed)
            .all(|(left, right)| left.to_bits() == right.to_bits())
    );
    let expected = accumulate(&reference, frames, frequencies, &indices, false);
    let actual = accumulate(&reference, frames, frequencies, &indices, true);
    assert!(
        expected
            .iter()
            .zip(&actual)
            .all(|(left, right)| left.to_bits() == right.to_bits())
    );
    println!(
        "frontend_full_shape_storage_bits_equal=true samples={}",
        packed.len()
    );
    for operation in ["packing", "mask"] {
        for tiled in [false, true, true, false] {
            let invoke = || match (operation, tiled) {
                ("packing", false) => scalar_pack(&spectra, frames, &indices),
                ("packing", true) => {
                    prepare_model_input(&spectra, frames, indices.len() * 2, &indices).unwrap()
                }
                _ => accumulate(&reference, frames, frequencies, &indices, tiled),
            };
            black_box(invoke());
            for sample in 0..4 {
                let started = Instant::now();
                let output = invoke();
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                black_box(&output);
                println!(
                    "operation={operation} tiled={tiled} sample={sample} milliseconds={elapsed:.6}"
                );
            }
        }
    }
}
