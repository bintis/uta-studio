//! Explicit device tests, never part of an ordinary CPU-only `cargo test`.
//! Each invocation loads one library set; GGML's plugin registry is process-global.

mod gemm;

use std::path::PathBuf;
use std::time::Instant;

use crate::ffi::*;
use crate::{DeviceKind, GgmlBackendHandle, GgmlRuntime};

#[derive(Clone, Copy, Debug)]
struct Shape {
    d: usize,
    queries: usize,
    keys: usize,
    heads: usize,
    batches: usize,
    padding: usize,
    masked: bool,
}

impl Shape {
    fn q_index(self, b: usize, h: usize, t: usize, d: usize) -> usize {
        ((b * self.queries + t) * self.heads + h) * (self.d + self.padding) + d
    }

    fn kv_index(self, b: usize, h: usize, t: usize, d: usize) -> usize {
        ((b * self.heads + h) * self.keys + t) * (self.d + self.padding) + d
    }

    fn o_index(self, b: usize, h: usize, t: usize, d: usize) -> usize {
        ((b * self.queries + t) * self.heads + h) * self.d + d
    }
}

// Normal binary16 values only; avoid requiring a conversion library for fixtures.
fn fixture_bits(index: usize, seed: u32) -> u16 {
    let hash = (index as u32)
        .wrapping_add(seed)
        .wrapping_mul(2_654_435_761);
    ((hash >> 16) as u16 & 0x8000) | 0x3000 | ((hash >> 3) as u16 & 0x0fff)
}

fn fixture_value(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = (u32::from(bits >> 10 & 31) + 112) << 23;
    f32::from_bits(sign | exponent | (u32::from(bits & 1023) << 13))
}

struct Storage<'a> {
    api: &'a ModelApi,
    context: ContextPtr,
    buffer: BufferPtr,
}

impl Drop for Storage<'_> {
    fn drop(&mut self) {
        // SAFETY: synchronous graph_compute/readback finished before the storage
        // leaves scope. The backend and library outlive this unique ownership.
        unsafe {
            if !self.buffer.is_null() {
                (self.api.ggml_backend_buffer_free)(self.buffer);
            }
            if !self.context.is_null() {
                (self.api.ggml_free)(self.context);
            }
        }
    }
}

fn backend() -> GgmlBackendHandle {
    let path = PathBuf::from(
        std::env::var_os("UTA_TEST_GGML_RUNTIME_DIR")
            .expect("set UTA_TEST_GGML_RUNTIME_DIR to the selected library directory"),
    );
    let runtime = GgmlRuntime::load(&path).expect("load the recorded GGML runtime");
    let device = runtime
        .devices()
        .unwrap()
        .into_iter()
        .find(|device| {
            device.kind == DeviceKind::DiscreteGpu && device.description.contains("B580")
        })
        .expect("this explicit test requires Intel B580; no other-device fallback");
    eprintln!(
        "attention test device: {:?}; libraries: {}",
        device,
        path.display()
    );
    runtime.create_backend(&device).unwrap()
}

fn run_shape(backend: &GgmlBackendHandle, s: Shape, timed_calls: usize) {
    let api = &backend.runtime.model_api;
    // SAFETY: all shapes below are positive bounded fixtures. Every tensor and
    // graph belongs to this live metadata arena; the allocated backend buffer is
    // retained through all uploads, synchronous computations and readbacks.
    unsafe {
        let context = (api.ggml_init)(GgmlInitParams {
            mem_size: 8 * 1024 * 1024,
            mem_buffer: std::ptr::null_mut(),
            no_alloc: true,
        });
        assert!(!context.is_null());
        let mut storage = Storage {
            api,
            context,
            buffer: std::ptr::null_mut(),
        };
        let row = s.d + s.padding;
        let q_parent = (api.ggml_new_tensor_4d)(
            context,
            GGML_TYPE_F32,
            row as i64,
            s.heads as i64,
            s.queries as i64,
            s.batches as i64,
        );
        let q = (api.ggml_view_4d)(
            context,
            q_parent,
            s.d as i64,
            s.queries as i64,
            s.heads as i64,
            s.batches as i64,
            row * s.heads * 4,
            row * 4,
            row * s.heads * s.queries * 4,
            0,
        );
        let k_parent = (api.ggml_new_tensor_4d)(
            context,
            GGML_TYPE_F16,
            row as i64,
            s.keys as i64,
            s.heads as i64,
            s.batches as i64,
        );
        let v_parent = (api.ggml_new_tensor_4d)(
            context,
            GGML_TYPE_F16,
            row as i64,
            s.keys as i64,
            s.heads as i64,
            s.batches as i64,
        );
        let kv_view = |parent| {
            (api.ggml_view_4d)(
                context,
                parent,
                s.d as i64,
                s.keys as i64,
                s.heads as i64,
                s.batches as i64,
                row * 2,
                row * s.keys * 2,
                row * s.keys * s.heads * 2,
                0,
            )
        };
        let k = kv_view(k_parent);
        let v = kv_view(v_parent);
        let mask_rows = s.queries.div_ceil(16) * 16;
        let mask = if s.masked {
            (api.ggml_new_tensor_4d)(
                context,
                GGML_TYPE_F16,
                s.keys as i64,
                mask_rows as i64,
                1,
                1,
            )
        } else {
            std::ptr::null_mut()
        };
        let scale = 1.0 / (s.d as f32).sqrt();
        let output = (api.ggml_flash_attn_ext)(context, q, k, v, mask, scale, 0.0, 0.0);
        assert!(!output.is_null());
        (api.ggml_flash_attn_ext_set_prec)(output, GGML_PREC_F32);
        (api.ggml_set_output)(output);
        let graph = (api.ggml_new_graph_custom)(context, 64, false);
        assert!(!graph.is_null());
        (api.ggml_build_forward_expand)(graph, output);
        storage.buffer = (api.ggml_backend_alloc_ctx_tensors_from_buft)(
            context,
            (api.ggml_backend_get_default_buffer_type)(backend.raw),
        );
        assert!(!storage.buffer.is_null());

        // Poison row padding. Incorrect unguarded vector/tile reads must not be
        // hidden by friendly zero-filled storage.
        let mut queries = vec![f32::NAN; row * s.heads * s.queries * s.batches];
        let mut keys = vec![0x7e00_u16; row * s.keys * s.heads * s.batches];
        let mut values = keys.clone();
        for b in 0..s.batches {
            for h in 0..s.heads {
                for t in 0..s.queries {
                    for d in 0..s.d {
                        let index = s.q_index(b, h, t, d);
                        queries[index] = fixture_value(fixture_bits(index, 13));
                    }
                }
                for t in 0..s.keys {
                    for d in 0..s.d {
                        let index = s.kv_index(b, h, t, d);
                        keys[index] = fixture_bits(index, 71);
                        values[index] = fixture_bits(index, 191);
                    }
                }
            }
        }
        (api.ggml_backend_tensor_set)(q_parent, queries.as_ptr().cast(), 0, queries.len() * 4);
        (api.ggml_backend_tensor_set)(k_parent, keys.as_ptr().cast(), 0, keys.len() * 2);
        (api.ggml_backend_tensor_set)(v_parent, values.as_ptr().cast(), 0, values.len() * 2);
        if s.masked {
            let masks: Vec<u16> = (0..mask_rows * s.keys)
                .map(|i| if i % s.keys > i / s.keys { 0xfc00 } else { 0 })
                .collect();
            (api.ggml_backend_tensor_set)(mask, masks.as_ptr().cast(), 0, masks.len() * 2);
        }

        // Fixed warmups avoid confusing device clock/cache ramp-up with kernel
        // throughput. The GPU logger prints every call; discard these first.
        // Upload, readback and the f64 oracle remain outside all compute timings.
        let warmup_calls = if timed_calls > 1 { 8 } else { 1 };
        for _ in 0..warmup_calls {
            assert_eq!(
                (api.ggml_backend_graph_compute)(backend.raw, graph),
                GGML_STATUS_SUCCESS
            );
        }
        let mut seconds = Vec::new();
        for _ in 0..timed_calls {
            let start = Instant::now();
            let status = (api.ggml_backend_graph_compute)(backend.raw, graph);
            seconds.push(start.elapsed().as_secs_f64());
            assert_eq!(status, GGML_STATUS_SUCCESS);
        }
        let mut actual = vec![0.0_f32; s.d * s.queries * s.heads * s.batches];
        (api.ggml_backend_tensor_get)(output, actual.as_mut_ptr().cast(), 0, actual.len() * 4);
        assert!(
            actual.iter().all(|v| v.is_finite()),
            "non-finite output for {s:?}"
        );
        let mut max_error = 0.0_f64;
        let mut squared_error = 0.0;
        let mut reference_energy = 0.0;
        let mut checked = 0;
        for b in [0, s.batches - 1] {
            for h in [0, s.heads - 1] {
                for t in [0, s.queries / 2, s.queries - 1] {
                    let scores: Vec<f64> = (0..s.keys)
                        .map(|key| {
                            if s.masked && key > t {
                                return f64::NEG_INFINITY;
                            }
                            (0..s.d)
                                .map(|d| {
                                    f64::from(queries[s.q_index(b, h, t, d)])
                                        * f64::from(fixture_value(keys[s.kv_index(b, h, key, d)]))
                                })
                                .sum::<f64>()
                                * f64::from(scale)
                        })
                        .collect();
                    let maximum = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    let probabilities: Vec<f64> =
                        scores.iter().map(|v| (v - maximum).exp()).collect();
                    let sum: f64 = probabilities.iter().sum();
                    for d in 0..s.d {
                        let reference = probabilities
                            .iter()
                            .enumerate()
                            .map(|(key, p)| {
                                p * f64::from(fixture_value(values[s.kv_index(b, h, key, d)]))
                            })
                            .sum::<f64>()
                            / sum;
                        let error = f64::from(actual[s.o_index(b, h, t, d)]) - reference;
                        max_error = max_error.max(error.abs());
                        squared_error += error * error;
                        reference_energy += reference * reference;
                        checked += 1;
                    }
                }
            }
        }
        // The kernel uses half operands/probabilities and F32 accumulators, not
        // all-F64 math. Match the upstream FLASH_ATTN_EXT NMSE tolerance.
        let nmse = squared_error / reference_energy.max(f64::MIN_POSITIVE);
        eprintln!(
            "ATTENTION_RESULT {}",
            serde_json::json!({
                "d": s.d, "queries": s.queries, "keys": s.keys,
                "heads": s.heads, "batches": s.batches, "padding": s.padding,
                "masked": s.masked, "warmup_calls": warmup_calls, "host_compute_seconds": seconds,
                "matmul_flops": 4_u64 * s.queries as u64 * s.keys as u64 * s.d as u64
                    * s.heads as u64 * s.batches as u64,
                "finite_outputs": actual.len(), "reference_values": checked,
                "max_abs_error": max_error, "nmse": nmse,
            })
        );
        assert!(nmse < 5e-4, "attention NMSE {nmse} for {s:?}");
    }
}

#[test]
fn fixture_layout_and_binary16_values() {
    let s = Shape {
        d: 64,
        queries: 17,
        keys: 33,
        heads: 2,
        batches: 2,
        padding: 4,
        masked: false,
    };
    assert_eq!(s.q_index(1, 1, 16, 63), 17 * 2 * 2 * 68 - 5);
    assert_eq!(s.kv_index(1, 1, 32, 63), 33 * 2 * 2 * 68 - 5);
    assert_eq!(s.o_index(1, 1, 16, 63), 17 * 2 * 2 * 64 - 1);
    assert_eq!(fixture_value(0x3800), 0.5);
    assert_eq!(fixture_value(0xbc00), -1.0);
}

#[test]
#[ignore = "explicit B580 numerical attention regression; requires UTA_TEST_GGML_RUNTIME_DIR"]
fn b580_attention_tail_numeric() {
    let backend = backend();
    for (d, keys, padding, masked) in [
        (64, 31, 0, false),
        (64, 32, 0, false),
        (64, 33, 0, false),
        (64, 90, 4, false),
        (72, 90, 0, false),
        (64, 33, 0, true),
        (64, 1722, 0, false),
    ] {
        run_shape(
            &backend,
            Shape {
                d,
                queries: 17,
                keys,
                heads: 2,
                batches: 2,
                padding,
                masked,
            },
            1,
        );
    }
}

#[test]
#[ignore = "explicit bounded B580 XE90-shape benchmark; requires UTA_TEST_GGML_RUNTIME_DIR"]
fn b580_attention_real_shapes() {
    let backend = backend();
    for (sequence, batches) in [(1722, 90), (90, 1722)] {
        run_shape(
            &backend,
            Shape {
                d: 64,
                queries: sequence,
                keys: sequence,
                heads: 8,
                batches,
                padding: 0,
                masked: false,
            },
            8,
        );
    }
}
