//! Streaming, ordered overlap-add shared by ordinary and dual-GPU execution.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Instant;

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

#[derive(Debug, Default)]
pub struct DualChunkStats {
    pub primary_chunks: usize,
    pub secondary_chunks: usize,
}

fn worthwhile_secondary(remaining: usize, primary_ns: u64, secondary_ns: u64) -> bool {
    primary_ns == 0 || secondary_ns <= primary_ns.saturating_mul(remaining as u64)
}

/// The primary processor stays on the caller's thread. The secondary factory
/// constructs and destroys its processor on the secondary thread: GPU handles
/// need neither an unsafe Send implementation nor concurrent shared access.
pub(super) fn process_dual<Processor>(
    input: &[f32],
    size: usize,
    overlap: usize,
    mut primary: impl FnMut(&[f32]) -> Result<Vec<Vec<f32>>, String>,
    initialize_secondary: impl FnOnce() -> Result<Processor, String> + Send,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(Vec<Vec<f32>>, DualChunkStats), String>
where
    Processor: FnMut(&[f32]) -> Result<Vec<Vec<f32>>, String>,
{
    let chunks = Chunks::new(input, size, overlap)?;
    // A final lone chunk cannot amortize constructing a cold second model.
    // Keep it on the owned primary; longer passes can measure both processors
    // and decide subsequent assignments from their actual chunk durations.
    if chunks.count <= 2 {
        return process(input, size, overlap, primary, progress).map(|output| {
            (
                output,
                DualChunkStats {
                    primary_chunks: chunks.count,
                    secondary_chunks: 0,
                },
            )
        });
    }
    let next = AtomicUsize::new(2);
    let primary_ns = AtomicU64::new(0);
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::sync_channel(1);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = {
            let chunks = &chunks;
            let next = &next;
            let primary_ns = &primary_ns;
            let stop = &stop;
            scope.spawn(move || {
                let mut secondary = match initialize_secondary() {
                    Ok(processor) => processor,
                    Err(error) => {
                        let _ = ready_sender.send(Err(error));
                        return;
                    }
                };
                // Initialize devices serially, then permit concurrent computation.
                if ready_sender.send(Ok(())).is_err() {
                    return;
                }
                let mut index = 1;
                loop {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let started = Instant::now();
                    let output = secondary(&chunks.chunk(index));
                    let failed = output.is_err();
                    let elapsed = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
                    if sender.send((index, output)).is_err() || failed {
                        break;
                    }
                    let remaining = chunks.count.saturating_sub(next.load(Ordering::Acquire));
                    if !worthwhile_secondary(remaining, primary_ns.load(Ordering::Acquire), elapsed)
                    {
                        break;
                    }
                    index = next.fetch_add(1, Ordering::AcqRel);
                    if index >= chunks.count {
                        break;
                    }
                }
            })
        };
        let result = (|| {
            ready_receiver
                .recv()
                .map_err(|_| "secondary GPU initialization stopped".to_string())??;
            let mut accumulator = Accumulator::new(&chunks);
            let mut pending = BTreeMap::new();
            let mut committed = 0;
            let mut stats = DualChunkStats::default();
            progress(0, chunks.count as u64);
            let mut accept =
                |index, output: Result<Vec<Vec<f32>>, String>| -> Result<usize, String> {
                    pending.insert(index, output?);
                    while let Some(output) = pending.remove(&committed) {
                        accumulator.add(&chunks, committed, output)?;
                        committed += 1;
                    }
                    Ok(pending.len())
                };
            let mut index = 0;
            loop {
                let started = Instant::now();
                let output = primary(&chunks.chunk(index));
                primary_ns.store(
                    started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                    Ordering::Release,
                );
                let mut buffered = accept(index, output)?;
                stats.primary_chunks += 1;
                for (index, output) in receiver.try_iter() {
                    buffered = accept(index, output)?;
                    stats.secondary_chunks += 1;
                }
                // A bounded reorder window prevents a delayed device from
                // retaining an entire song of completed chunks in host RAM.
                while buffered >= 32 {
                    let (index, output) = receiver.recv().map_err(|_| {
                        "secondary GPU stopped with an incomplete ordered chunk".to_string()
                    })?;
                    buffered = accept(index, output)?;
                    stats.secondary_chunks += 1;
                }
                progress(
                    (stats.primary_chunks + stats.secondary_chunks) as u64,
                    chunks.count as u64,
                );
                index = next.fetch_add(1, Ordering::AcqRel);
                if index >= chunks.count {
                    break;
                }
            }
            // Drop the closure's borrows before draining the final ordered tail.
            drop(accept);
            for (index, output) in &receiver {
                pending.insert(index, output?);
                stats.secondary_chunks += 1;
                while let Some(output) = pending.remove(&committed) {
                    accumulator.add(&chunks, committed, output)?;
                    committed += 1;
                }
                progress(
                    (stats.primary_chunks + stats.secondary_chunks) as u64,
                    chunks.count as u64,
                );
            }
            if committed != chunks.count {
                return Err("dual-GPU chunk execution ended before completion".to_string());
            }
            Ok((accumulator.finish(&chunks), stats))
        })();
        stop.store(true, Ordering::Release);
        // Unblock a sender even if the primary failed while a result was queued.
        drop(receiver);
        worker
            .join()
            .map_err(|_| "secondary GPU worker panicked".to_string())?;
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn uneven_workers_preserve_ordered_overlap_and_use_both_processors() {
        let input = (0..8192)
            .map(|index| (index as f32 * 0.01).sin())
            .collect::<Vec<_>>();
        let reference = process(
            &input,
            512,
            2,
            |chunk| Ok(vec![chunk.to_vec()]),
            &mut |_, _| {},
        )
        .unwrap();
        let mut progress = Vec::new();
        let (actual, stats) = process_dual(
            &input,
            512,
            2,
            |chunk| {
                std::thread::sleep(Duration::from_millis(1));
                Ok(vec![chunk.to_vec()])
            },
            || {
                // A non-Send owner is created and stays on its own thread.
                let owner = std::rc::Rc::new(());
                Ok(move |chunk: &[f32]| {
                    let _ = &owner;
                    std::thread::sleep(Duration::from_millis(4));
                    Ok(vec![chunk.to_vec()])
                })
            },
            &mut |done, total| progress.push((done, total)),
        )
        .unwrap();
        assert_eq!(actual, reference);
        assert!(stats.primary_chunks > 0);
        assert!(stats.secondary_chunks > 0);
        assert!(progress.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(progress.last().unwrap().0, progress.last().unwrap().1);
    }

    #[test]
    fn primary_failure_unblocks_secondary_result_transport() {
        let error = process_dual(
            &vec![0.0; 4096],
            256,
            2,
            |_| Err("primary failed".to_string()),
            || Ok(|chunk: &[f32]| Ok(vec![chunk.to_vec()])),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert_eq!(error, "primary failed");
    }

    #[test]
    fn secondary_failure_is_not_replaced_with_primary_or_cpu_work() {
        let error = process_dual(
            &vec![0.0; 4096],
            256,
            2,
            |chunk| Ok(vec![chunk.to_vec()]),
            || Ok(|_: &[f32]| Err("secondary failed".to_string())),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert_eq!(error, "secondary failed");
    }

    #[test]
    fn one_chunk_does_not_initialize_an_unused_secondary_model() {
        let input = vec![0.5; 16];
        let (output, stats) = process_dual(
            &input,
            512,
            1,
            |chunk| Ok(vec![chunk.to_vec()]),
            || -> Result<fn(&[f32]) -> Result<Vec<Vec<f32>>, String>, String> {
                panic!("secondary must not initialize for one chunk")
            },
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(output, [input]);
        assert_eq!(stats.secondary_chunks, 0);
    }

    #[test]
    fn a_lone_remaining_chunk_does_not_construct_a_cold_secondary() {
        let input = vec![0.25; 512];
        let (output, stats) = process_dual(
            &input,
            256,
            2,
            |chunk| Ok(vec![chunk.to_vec()]),
            || -> Result<fn(&[f32]) -> Result<Vec<Vec<f32>>, String>, String> {
                panic!("a cold secondary cannot amortize its setup on the final chunk")
            },
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(output, [input]);
        assert_eq!(stats.primary_chunks, 2);
        assert_eq!(stats.secondary_chunks, 0);
    }

    #[test]
    fn slow_secondary_does_not_take_a_tail_that_the_primary_can_finish_earlier() {
        assert!(!worthwhile_secondary(2, 10, 30));
        assert!(worthwhile_secondary(4, 10, 30));
    }
}
