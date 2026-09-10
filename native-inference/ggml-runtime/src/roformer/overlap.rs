//! Ordered overlap-add for a complete single-device model invocation.

use super::frames::{crossfade_window, reflect_pad_track};

struct Chunks {
    padded: Vec<f32>,
    input_frames: usize,
    left_pad: usize,
    size: usize,
    step: usize,
    fade: usize,
    count: usize,
    window: Vec<f32>,
}

impl Chunks {
    fn new(input: &[f32], size: usize, overlap: usize) -> Result<Self, String> {
        if input.is_empty() || input.len() % 2 != 0 || overlap == 0 {
            return Err("RoFormer overlap-add input is invalid".to_string());
        }
        let step = size / overlap;
        if step == 0 {
            return Err("RoFormer overlap-add step is zero".to_string());
        }
        let border = size - step;
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
        let count = (padded.len() / 2).div_ceil(step);
        let fade = size / 10;
        Ok(Self {
            padded,
            input_frames,
            left_pad,
            size,
            step,
            fade,
            count,
            window: crossfade_window(size, fade),
        })
    }

    fn chunk(&self, index: usize) -> Vec<f32> {
        let offset = index * self.step;
        let length = self.size.min(self.padded.len() / 2 - offset);
        let mut chunk = vec![0.0; self.size * 2];
        chunk[..length * 2].copy_from_slice(&self.padded[offset * 2..(offset + length) * 2]);
        if length < self.size && length > self.size / 2 + 1 {
            for pad in 0..self.size - length {
                let source = length.saturating_sub(2 + pad);
                chunk[(length + pad) * 2] = chunk[source * 2];
                chunk[(length + pad) * 2 + 1] = chunk[source * 2 + 1];
            }
        }
        chunk
    }
}

struct Accumulator {
    outputs: Vec<Vec<f32>>,
    counter: Vec<f32>,
}

impl Accumulator {
    fn new(chunks: &Chunks) -> Self {
        Self {
            outputs: Vec::new(),
            counter: vec![0.0; chunks.padded.len()],
        }
    }

    fn add(&mut self, chunks: &Chunks, index: usize, output: Vec<Vec<f32>>) -> Result<(), String> {
        if output.is_empty() || output.iter().any(|stem| stem.len() != chunks.size * 2) {
            return Err("RoFormer chunk output shape is invalid".to_string());
        }
        if self.outputs.is_empty() {
            self.outputs = vec![vec![0.0; chunks.padded.len()]; output.len()];
        } else if self.outputs.len() != output.len() {
            return Err("RoFormer stem count changed between chunks".to_string());
        }
        let offset = index * chunks.step;
        let length = chunks.size.min(chunks.padded.len() / 2 - offset);
        let mut window = chunks.window.clone();
        if offset == 0 {
            window[..chunks.fade].fill(1.0);
        } else if offset + chunks.step >= chunks.padded.len() / 2 {
            window[chunks.size - chunks.fade..].fill(1.0);
        }
        for frame in 0..length {
            let destination = (offset + frame) * 2;
            let source = frame * 2;
            for (stem, chunk) in self.outputs.iter_mut().zip(&output) {
                stem[destination] += chunk[source] * window[frame];
                stem[destination + 1] += chunk[source + 1] * window[frame];
            }
            self.counter[destination] += window[frame];
            self.counter[destination + 1] += window[frame];
        }
        Ok(())
    }

    fn finish(mut self, chunks: &Chunks) -> Vec<Vec<f32>> {
        for output in &mut self.outputs {
            for frame in 0..chunks.input_frames {
                let padded = (chunks.left_pad + frame) * 2;
                output[frame * 2] = output[padded] / self.counter[padded].max(1.0e-4);
                output[frame * 2 + 1] = output[padded + 1] / self.counter[padded + 1].max(1.0e-4);
            }
            output.truncate(chunks.input_frames * 2);
        }
        self.outputs
    }
}

pub(super) fn process(
    input: &[f32],
    size: usize,
    overlap: usize,
    mut compute: impl FnMut(&[f32]) -> Result<Vec<Vec<f32>>, String>,
    progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<Vec<f32>>, String> {
    let chunks = Chunks::new(input, size, overlap)?;
    let mut accumulator = Accumulator::new(&chunks);
    progress(0, chunks.count as u64);
    for index in 0..chunks.count {
        accumulator.add(&chunks, index, compute(&chunks.chunk(index))?)?;
        progress(index as u64 + 1, chunks.count as u64);
    }
    Ok(accumulator.finish(&chunks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn every_chunk_uses_the_same_caller_owned_processor() {
        let input = vec![0.25; 512];
        let owner = std::thread::current().id();
        // A non-Send owner models a backend handle without making it thread-safe.
        let calls = Rc::new(Cell::new(0));
        let mut progress = Vec::new();
        let output = process(
            &input,
            256,
            2,
            |chunk| {
                assert_eq!(std::thread::current().id(), owner);
                calls.set(calls.get() + 1);
                Ok(vec![chunk.to_vec()])
            },
            &mut |done, total| progress.push((done, total)),
        )
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(progress, [(0, 2), (1, 2), (2, 2)]);
        assert_eq!(output, [input]);
    }

    #[test]
    fn processor_failure_stops_without_reassigning_or_repeating_work() {
        let mut calls = 0;
        let mut completed = 0;
        let result = process(
            &vec![0.25; 2048],
            256,
            2,
            |chunk| {
                calls += 1;
                if calls == 2 {
                    Err("device failure".to_string())
                } else {
                    Ok(vec![chunk.to_vec()])
                }
            },
            &mut |done, _| completed = done,
        );
        assert_eq!(result.unwrap_err(), "device failure");
        assert_eq!(calls, 2);
        assert_eq!(completed, 1);
    }
}
