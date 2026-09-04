use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::gguf::GGUFFile;

const SAMPLE_RATE: usize = 44_100;
const HOP: usize = 1_024;
const BINS: usize = 384;
const CHANNELS: usize = 6;
const ONSET_THRESHOLD: f32 = 0.32;
const OFFSET_THRESHOLD: f32 = 0.70;
const MIDI_FMIN: f32 = 24.0;

#[derive(Serialize)]
pub struct Range {
    pub start: u64,
    pub end: u64,
}

#[derive(Serialize)]
pub struct Note {
    pub range: Range,
    pub midi: u8,
    pub onset_score: Option<f32>,
    pub offset_score: Option<f32>,
    pub pitch_score: Option<f32>,
}

#[derive(Serialize)]
pub struct Evidence<'a> {
    pub schema_version: u32,
    pub model_id: &'a str,
    pub upstream_revision: &'a str,
    pub checkpoint_identity: &'a str,
    pub config_identity: &'a str,
    pub conversion_identity: &'a str,
    pub model_generation: &'a str,
    pub runtime_identity: &'a str,
    pub backend: &'a str,
    pub source_start: u64,
    pub source_duration: u64,
    pub mix_audio_identity: &'a str,
    pub vocal_audio_identity: &'a str,
    pub separator_model_generation: &'a str,
    pub vocal_preparation_generation: &'a str,
    pub frontend_profile: &'a str,
    pub decode_profile: &'a str,
    pub onset_threshold: f32,
    pub offset_threshold: f32,
    pub notes: Vec<Note>,
}

fn config_text<'a>(config: &'a serde_json::Value, key: &str, fallback: &'a str) -> &'a str {
    config
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
}

fn reflected(audio: &[f32], index: isize) -> f32 {
    if audio.len() <= 1 {
        return audio.first().copied().unwrap_or(0.0);
    }
    let period = 2 * (audio.len() - 1) as isize;
    let folded = index.rem_euclid(period);
    audio[if folded < audio.len() as isize {
        folded as usize
    } else {
        (period - folded) as usize
    }]
}

pub fn append_signal_features(
    audio: &[f32],
    frames: usize,
    output: &mut [f32],
    base_channel: usize,
) {
    let fft_sizes = [8_192_usize, 16_384, 32_768];
    let mut planner = FftPlanner::<f32>::new();
    for (scale_index, fft_size) in fft_sizes.into_iter().enumerate() {
        let fft = planner.plan_fft_forward(fft_size);
        let mut buffer = vec![Complex32::new(0.0, 0.0); fft_size];
        for frame in 0..frames {
            let center = frame * HOP;
            for (index, value) in buffer.iter_mut().enumerate() {
                let source = center as isize + index as isize - (fft_size / 2) as isize;
                let window =
                    0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / fft_size as f32).cos();
                *value = Complex32::new(reflected(audio, source) * window, 0.0);
            }
            fft.process(&mut buffer);
            for bin in 0..BINS {
                let midi = MIDI_FMIN + bin as f32 / 4.0;
                let hz = 440.0 * 2.0_f32.powf((midi - 69.0) / 12.0);
                let exact = hz * fft_size as f32 / SAMPLE_RATE as f32;
                let lower = exact.floor() as usize;
                let fraction = exact - lower as f32;
                let left = buffer[lower.min(fft_size / 2)].norm();
                let right = buffer[(lower + 1).min(fft_size / 2)].norm();
                let magnitude = (left + (right - left) * fraction).ln_1p();
                let channel = base_channel + scale_index;
                output[((channel * frames + frame) * BINS) + bin] = magnitude;
            }
        }
    }
}

fn conv2d(
    input: &[f32],
    c_in: usize,
    h: usize,
    w_in: usize,
    weights: &[f32],
    bias: &[f32],
    c_out: usize,
    stride_w: usize,
) -> (Vec<f32>, usize) {
    let w_out = (w_in + 2 * 4 - 9) / stride_w + 1;
    let mut out = vec![0.0_f32; c_out * h * w_out];

    out.par_chunks_mut(h * w_out)
        .enumerate()
        .for_each(|(co, co_out)| {
            let b = bias[co];
            let w_co = &weights[co * c_in * 81..(co + 1) * c_in * 81];
            for i in 0..h {
                for j in 0..w_out {
                    let mut sum = b;
                    let h_start = i as isize - 4;
                    let w_start = (j * stride_w) as isize - 4;
                    for ci in 0..c_in {
                        let w_ci = &w_co[ci * 81..(ci + 1) * 81];
                        let in_ci = &input[ci * h * w_in..(ci + 1) * h * w_in];
                        for kh in 0..9 {
                            let cur_h = h_start + kh;
                            if cur_h < 0 || cur_h >= h as isize {
                                continue;
                            }
                            let row =
                                &in_ci[(cur_h as usize) * w_in..((cur_h as usize) + 1) * w_in];
                            let w_row = &w_ci[kh as usize * 9..(kh as usize + 1) * 9];
                            for kw in 0..9 {
                                let cur_w = w_start + kw;
                                if cur_w >= 0 && cur_w < w_in as isize {
                                    sum += row[cur_w as usize] * w_row[kw as usize];
                                }
                            }
                        }
                    }
                    co_out[i * w_out + j] = sum;
                }
            }
        });

    (out, w_out)
}

fn relu_inplace(data: &mut [f32]) {
    data.par_iter_mut().for_each(|x| {
        if *x < 0.0 {
            *x = 0.0;
        }
    });
}

fn dense(input: &[f32], m: usize, k: usize, weight: &[f32], bias: &[f32], n: usize) -> Vec<f32> {
    let mut out = vec![0.0_f32; m * n];
    // input is [m, k], row-major
    // weight is [n, k], row-major (transposed)
    // out = input @ weight.T + bias -> [m, n]
    unsafe {
        gemm::gemm(
            m,
            n,
            k,
            out.as_mut_ptr(),
            1,
            n as isize,
            false,
            input.as_ptr(),
            1,
            k as isize,
            weight.as_ptr(),
            k as isize,
            1,
            0.0,
            1.0,
            false,
            false,
            false,
            gemm::Parallelism::Rayon(0),
        );
    }
    // Add bias
    out.par_chunks_mut(n).for_each(|row| {
        for j in 0..n {
            row[j] += bias[j];
        }
    });
    out
}

fn softmax(values: &[f32]) -> Vec<f32> {
    let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut result = values
        .iter()
        .map(|value| (*value - maximum).exp())
        .collect::<Vec<_>>();
    let total = result.iter().sum::<f32>().max(f32::MIN_POSITIVE);
    result.iter_mut().for_each(|value| *value /= total);
    result
}

pub fn decode_notes(on_off: &[f32], octave: &[f32], class: &[f32], frames: usize) -> Vec<Note> {
    let onset = (0..frames)
        .map(|frame| on_off[frame * 4 + 1])
        .collect::<Vec<_>>();
    let mut notes = Vec::new();
    let mut active: Option<(usize, f32)> = None;
    let mut pitches = Vec::<(u8, f32)>::new();
    let finish = |end: usize,
                  active: &mut Option<(usize, f32)>,
                  pitches: &mut Vec<(u8, f32)>,
                  notes: &mut Vec<Note>,
                  offset_score: Option<f32>| {
        let Some((start, onset_score)) = active.take() else {
            return;
        };
        if pitches.is_empty() || end <= start {
            pitches.clear();
            return;
        }
        let mut counts = BTreeMap::<u8, (usize, f32)>::new();
        for (midi, score) in pitches.drain(..) {
            let entry = counts.entry(midi).or_default();
            entry.0 += 1;
            entry.1 += score;
        }
        let (midi, (count, score)) = counts
            .into_iter()
            .max_by(|left, right| {
                left.1
                    .0
                    .cmp(&right.1.0)
                    .then_with(|| left.1.1.total_cmp(&right.1.1))
            })
            .unwrap();
        notes.push(Note {
            range: Range {
                start: start as u64 * HOP as u64 * 1_000_000 / SAMPLE_RATE as u64,
                end: end as u64 * HOP as u64 * 1_000_000 / SAMPLE_RATE as u64,
            },
            midi,
            onset_score: Some(onset_score),
            offset_score,
            pitch_score: Some(score / count as f32),
        });
    };
    for frame in 0..frames {
        let backward = frame.saturating_sub(3);
        let forward = (frame + 4).min(frames);
        let is_peak = onset[frame] >= ONSET_THRESHOLD
            && onset[backward..forward]
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .is_some_and(|(index, _)| backward + index == frame);
        if is_peak {
            finish(frame, &mut active, &mut pitches, &mut notes, None);
            active = Some((frame, onset[frame]));
        } else if on_off[frame * 4 + 2] >= OFFSET_THRESHOLD {
            finish(
                frame,
                &mut active,
                &mut pitches,
                &mut notes,
                Some(on_off[frame * 4 + 2]),
            );
        }
        if active.is_some() {
            let octave_probs = softmax(&octave[frame * 5..frame * 5 + 5]);
            let class_probs = softmax(&class[frame * 13..frame * 13 + 13]);
            let oct = octave_probs[..4]
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .map(|(index, _)| index)
                .unwrap_or(0);
            let chroma = class_probs[..12]
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .map(|(index, _)| index)
                .unwrap_or(0);
            pitches.push((
                (36 + oct * 12 + chroma) as u8,
                octave_probs[oct] * class_probs[chroma],
            ));
        }
    }
    finish(frames, &mut active, &mut pitches, &mut notes, None);
    notes
}

pub struct JbmModel {
    pub file: GGUFFile,
}

impl JbmModel {
    pub fn load(path: &Path) -> Result<Self> {
        let file = GGUFFile::open(path)?;
        Ok(Self { file })
    }

    fn tensor<'a>(&'a self, name: &str) -> Result<&'a [f32]> {
        self.file.tensor_data_f32(name)
    }

    pub fn forward_onset(&self, features: &[f32], frames: usize) -> Result<Vec<f32>> {
        // features: [6, frames, 384]
        // conv1: stride_w = 4
        let (mut c1, w1) = conv2d(
            features,
            6,
            frames,
            384,
            self.tensor("onset_cnn.conv1.weight")?,
            self.tensor("onset_cnn.conv1.bias")?,
            16,
            4,
        );
        relu_inplace(&mut c1);

        let (mut c2, w2) = conv2d(
            &c1,
            16,
            frames,
            w1,
            self.tensor("onset_cnn.conv2.weight")?,
            self.tensor("onset_cnn.conv2.bias")?,
            32,
            1,
        );
        relu_inplace(&mut c2);

        let (mut c3, w3) = conv2d(
            &c2,
            32,
            frames,
            w2,
            self.tensor("onset_cnn.conv3.weight")?,
            self.tensor("onset_cnn.conv3.bias")?,
            32,
            1,
        );
        relu_inplace(&mut c3);

        let (mut c4, w4) = conv2d(
            &c3,
            32,
            frames,
            w3,
            self.tensor("onset_cnn.conv4.weight")?,
            self.tensor("onset_cnn.conv4.bias")?,
            32,
            1,
        );
        relu_inplace(&mut c4);

        let (c5, w5) = conv2d(
            &c4,
            32,
            frames,
            w4,
            self.tensor("onset_cnn.conv5.weight")?,
            self.tensor("onset_cnn.conv5.bias")?,
            32,
            1,
        );
        // c5 is [32, frames, 96]
        // Transpose [0, 2, 3, 1] in ONNX: [frames, 96, 32], flatten to [frames, 3072]
        let mut flat = vec![0.0_f32; frames * 32 * w5];
        for co in 0..32 {
            for f in 0..frames {
                for w in 0..w5 {
                    flat[(f * w5 + w) * 32 + co] = c5[(co * frames + f) * w5 + w];
                }
            }
        }

        let mut fc1 = dense(
            &flat,
            frames,
            3072,
            self.tensor("onset_cnn.fc1.weight")?,
            self.tensor("onset_cnn.fc1.bias")?,
            64,
        );
        relu_inplace(&mut fc1);

        let mut fc2 = dense(
            &fc1,
            frames,
            64,
            self.tensor("onset_cnn.fc2.weight")?,
            self.tensor("onset_cnn.fc2.bias")?,
            32,
        );
        relu_inplace(&mut fc2);

        let fc3 = dense(
            &fc2,
            frames,
            32,
            self.tensor("onset_cnn.fc3.weight")?,
            self.tensor("onset_cnn.fc3.bias")?,
            4,
        );

        // Softmax over 4 classes per frame
        let mut on_off = vec![0.0_f32; frames * 4];
        for f in 0..frames {
            let s = softmax(&fc3[f * 4..(f + 1) * 4]);
            on_off[f * 4..(f + 1) * 4].copy_from_slice(&s);
        }
        Ok(on_off)
    }

    pub fn forward_pitch(&self, features: &[f32], frames: usize) -> Result<(Vec<f32>, Vec<f32>)> {
        let (mut c1, w1) = conv2d(
            features,
            6,
            frames,
            384,
            self.tensor("pitch_cnn.conv1.weight")?,
            self.tensor("pitch_cnn.conv1.bias")?,
            16,
            4,
        );
        relu_inplace(&mut c1);

        let (mut c2, w2) = conv2d(
            &c1,
            16,
            frames,
            w1,
            self.tensor("pitch_cnn.conv2.weight")?,
            self.tensor("pitch_cnn.conv2.bias")?,
            32,
            1,
        );
        relu_inplace(&mut c2);

        let (mut c3, w3) = conv2d(
            &c2,
            32,
            frames,
            w2,
            self.tensor("pitch_cnn.conv3.weight")?,
            self.tensor("pitch_cnn.conv3.bias")?,
            32,
            1,
        );
        relu_inplace(&mut c3);

        let (mut c4, w4) = conv2d(
            &c3,
            32,
            frames,
            w3,
            self.tensor("pitch_cnn.conv4.weight")?,
            self.tensor("pitch_cnn.conv4.bias")?,
            32,
            1,
        );
        relu_inplace(&mut c4);

        let (c5, w5) = conv2d(
            &c4,
            32,
            frames,
            w4,
            self.tensor("pitch_cnn.conv5.weight")?,
            self.tensor("pitch_cnn.conv5.bias")?,
            32,
            1,
        );
        let mut flat = vec![0.0_f32; frames * 32 * w5];
        for co in 0..32 {
            for f in 0..frames {
                for w in 0..w5 {
                    flat[(f * w5 + w) * 32 + co] = c5[(co * frames + f) * w5 + w];
                }
            }
        }

        let mut fc1 = dense(
            &flat,
            frames,
            3072,
            self.tensor("pitch_cnn.fc1.weight")?,
            self.tensor("pitch_cnn.fc1.bias")?,
            64,
        );
        relu_inplace(&mut fc1);

        let mut fc2 = dense(
            &fc1,
            frames,
            64,
            self.tensor("pitch_cnn.fc2.weight")?,
            self.tensor("pitch_cnn.fc2.bias")?,
            32,
        );
        relu_inplace(&mut fc2);

        let fc3 = dense(
            &fc2,
            frames,
            32,
            self.tensor("pitch_cnn.fc3.weight")?,
            self.tensor("pitch_cnn.fc3.bias")?,
            18,
        );

        let mut octave = vec![0.0_f32; frames * 5];
        let mut pitch_class = vec![0.0_f32; frames * 13];
        for f in 0..frames {
            octave[f * 5..(f + 1) * 5].copy_from_slice(&fc3[f * 18..f * 18 + 5]);
            pitch_class[f * 13..(f + 1) * 13].copy_from_slice(&fc3[f * 18 + 5..f * 18 + 18]);
        }
        Ok((octave, pitch_class))
    }
}

pub fn infer(
    mix: &[f32],
    vocal: &[f32],
    model_path: &Path,
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &'static str),
) -> Result<PathBuf> {
    if mix.is_empty() || vocal.is_empty() {
        return Err(Error::message(
            "JBM555 requires non-empty mix and vocal inputs",
        ));
    }
    let samples = mix.len().min(vocal.len());
    let frames = samples.div_ceil(HOP).max(1);
    let mut features = vec![0.0_f32; CHANNELS * frames * BINS];

    progress(0.05, "Computing native dual-input JBM555 spectral features");
    append_signal_features(&mix[..samples], frames, &mut features, 0);
    append_signal_features(&vocal[..samples], frames, &mut features, 3);

    progress(0.40, "Loading JBM555 native GGUF weights");
    let model = JbmModel::load(model_path)?;

    progress(0.50, "Running JBM555 Onset CNN");
    let on_off = model.forward_onset(&features, frames)?;

    progress(0.75, "Running JBM555 Pitch CNN");
    let (octave, class) = model.forward_pitch(&features, frames)?;

    progress(0.90, "Decoding JBM555 notes and peak tracking");
    let mut notes = decode_notes(&on_off, &octave, &class, frames);

    let source_start = config
        .get("source_start")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    for note in &mut notes {
        note.range.start = note.range.start.saturating_add(source_start);
        note.range.end = note.range.end.saturating_add(source_start);
    }
    let source_duration = config
        .get("source_duration")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(samples as u64 * 1_000_000 / SAMPLE_RATE as u64);
    let source_end = source_start.saturating_add(source_duration);
    for note in &mut notes {
        note.range.end = note.range.end.min(source_end);
    }
    notes.retain(|note| note.range.start < note.range.end && note.range.start < source_end);

    let destination = output_dir.join("jbm555-note-evidence.json");
    let temporary = output_dir.join("jbm555-note-evidence.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(Error::Io)?;
    serde_json::to_writer(
        &mut file,
        &Evidence {
            schema_version: 1,
            model_id: "jbm555_cectc_80",
            upstream_revision: config_text(config, "upstream_revision", "jbm555-public"),
            checkpoint_identity: config_text(config, "checkpoint_identity", "jbm555_80"),
            config_identity: config_text(config, "config_identity", "cectc80-public"),
            conversion_identity: config_text(config, "conversion_identity", "gguf-f32-v1"),
            model_generation: config_text(config, "model_generation", "runtime-managed"),
            runtime_identity: "jbm555_native_v1",
            backend: "jbm555_native",
            source_start,
            source_duration,
            mix_audio_identity: config_text(config, "mix_audio_identity", "task-mix"),
            vocal_audio_identity: config_text(config, "vocal_audio_identity", "task-vocal"),
            separator_model_generation: config_text(
                config,
                "separator_model_generation",
                "leap-xe90",
            ),
            vocal_preparation_generation: config_text(
                config,
                "vocal_preparation_generation",
                "native-44k1",
            ),
            frontend_profile: "jbm555-native-logfft-44k1-hop1024-midi24-384x48-scales0.5-1-2-v1",
            decode_profile: "jbm555-cectc-onset0.32-offset0.70-v1",
            onset_threshold: ONSET_THRESHOLD,
            offset_threshold: OFFSET_THRESHOLD,
            notes,
        },
    )
    .map_err(Error::Json)?;
    file.write_all(b"\n").map_err(Error::Io)?;
    file.sync_all().map_err(Error::Io)?;
    std::fs::rename(&temporary, &destination).map_err(Error::Io)?;
    Ok(destination)
}
