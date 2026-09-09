//! Opt-in primitive controls for the two XE90 attention sequence lengths.
//! These are diagnostics, not an alternative production attention graph. The
//! long-sequence case microbatches independent bands only: every head still
//! sees all 1722 keys. Unlike a 4096-square GEMM, QK has reduction depth 64.

use std::path::Path;
use std::ptr;
use std::time::Instant;

use crate::ffi::{
    self, GGML_PREC_F32, GGML_STATUS_SUCCESS, GGML_TYPE_F16, GGML_TYPE_F32, GgmlInitParams,
};
use crate::{DeviceKind, GgmlBackendHandle, GgmlRuntime};

const HEADS: usize = 8;
const WARMUPS: usize = 16;
const SAMPLES: usize = 8;

#[derive(Clone, Copy, Debug)]
enum Primitive {
    QueryKey,
    ProbabilityValue,
    Softmax,
}

fn bits(index: usize, salt: u32) -> u16 {
    let h = (index as u32).wrapping_add(salt).wrapping_mul(0x9e37_79b9);
    let h = (h ^ (h >> 16)).wrapping_mul(0x85eb_ca6b);
    ((h >> 16) as u16 & 0x8000) | ((12 + (h >> 10) % 3) as u16) << 10 | (h as u16 & 1023)
}

fn value(bits: u16) -> f32 {
    // Fixtures contain finite normal binary16 values only, with exponent 12..14.
    let sign = (u32::from(bits) & 0x8000) << 16;
    let exponent = ((u32::from(bits) >> 10) & 31) + 112;
    f32::from_bits(sign | (exponent << 23) | ((u32::from(bits) & 1023) << 13))
}

fn rhs_offset(
    k: usize,
    n: usize,
    reduction: usize,
    column: usize,
    head_batch: usize,
    interleaved: bool,
) -> usize {
    if interleaved {
        reduction + k * (head_batch % HEADS + HEADS * (column + n * (head_batch / HEADS)))
    } else {
        reduction + k * (column + n * head_batch)
    }
}

struct Arena<'a> {
    api: &'a ffi::ModelApi,
    ctx: ffi::ContextPtr,
    buffer: ffi::BufferPtr,
}

impl Drop for Arena<'_> {
    fn drop(&mut self) {
        // SAFETY: both allocations belong to this arena and the library handle
        // outlives it. Views never own a second copy of the underlying storage.
        unsafe {
            if !self.buffer.is_null() {
                (self.api.ggml_backend_buffer_free)(self.buffer);
            }
            (self.api.ggml_free)(self.ctx);
        }
    }
}

fn run(backend: &GgmlBackendHandle, sequence: usize, batches: usize, primitive: Primitive) {
    let api = &backend.runtime.model_api;
    let (m, n, k) = match primitive {
        Primitive::QueryKey => (sequence, sequence, 64),
        Primitive::ProbabilityValue => (64, sequence, sequence),
        Primitive::Softmax => (sequence, sequence, 0),
    };
    let interleaved = matches!(primitive, Primitive::QueryKey);
    // SAFETY: dimensions and byte extents below are derived from positive,
    // bounded fixtures. Every allocation is checked before it is dereferenced.
    unsafe {
        let ctx = (api.ggml_init)(GgmlInitParams {
            mem_size: 4 * 1024 * 1024,
            mem_buffer: ptr::null_mut(),
            no_alloc: true,
        });
        assert!(!ctx.is_null(), "GGML context allocation failed");
        let mut arena = Arena {
            api,
            ctx,
            buffer: ptr::null_mut(),
        };
        let (lhs, rhs_storage, output) = if matches!(primitive, Primitive::Softmax) {
            let input = (api.ggml_new_tensor_4d)(
                ctx,
                GGML_TYPE_F32,
                m as i64,
                n as i64,
                HEADS as i64,
                batches as i64,
            );
            assert!(!input.is_null());
            (ptr::null_mut(), input, (api.ggml_soft_max)(ctx, input))
        } else {
            let a = (api.ggml_new_tensor_4d)(
                ctx,
                GGML_TYPE_F16,
                k as i64,
                m as i64,
                HEADS as i64,
                batches as i64,
            );
            assert!(!a.is_null());
            let b_parent = if interleaved {
                (api.ggml_new_tensor_4d)(
                    ctx,
                    GGML_TYPE_F32,
                    k as i64,
                    HEADS as i64,
                    n as i64,
                    batches as i64,
                )
            } else {
                (api.ggml_new_tensor_4d)(
                    ctx,
                    GGML_TYPE_F32,
                    k as i64,
                    n as i64,
                    HEADS as i64,
                    batches as i64,
                )
            };
            assert!(!b_parent.is_null());
            let b = if interleaved {
                (api.ggml_view_4d)(
                    ctx,
                    b_parent,
                    k as i64,
                    n as i64,
                    HEADS as i64,
                    batches as i64,
                    k * HEADS * 4,
                    k * 4,
                    k * HEADS * n * 4,
                    0,
                )
            } else {
                b_parent
            };
            assert!(!b.is_null());
            let out = (api.ggml_mul_mat)(ctx, a, b);
            assert!(!out.is_null());
            (api.ggml_mul_mat_set_prec)(out, GGML_PREC_F32);
            (a, b_parent, out)
        };
        assert!(!output.is_null());
        let graph = (api.ggml_new_graph_custom)(ctx, 64, false);
        assert!(!graph.is_null());
        (api.ggml_build_forward_expand)(graph, output);
        arena.buffer = (api.ggml_backend_alloc_ctx_tensors_from_buft)(
            ctx,
            (api.ggml_backend_get_default_buffer_type)(backend.raw),
        );
        assert!(!arena.buffer.is_null(), "GPU fixture allocation failed");
        let a: Vec<u16> = (0..k * m * HEADS * batches).map(|i| bits(i, 113)).collect();
        if !lhs.is_null() {
            (api.ggml_backend_tensor_set)(lhs, a.as_ptr().cast(), 0, a.len() * 2);
        }
        let rhs_len = if k == 0 {
            m * n * HEADS * batches
        } else {
            k * n * HEADS * batches
        };
        let b: Vec<f32> = (0..rhs_len).map(|i| value(bits(i, 977))).collect();
        (api.ggml_backend_tensor_set)(rhs_storage, b.as_ptr().cast(), 0, b.len() * 4);
        for _ in 0..WARMUPS {
            assert_eq!(
                (api.ggml_backend_graph_compute)(backend.raw, graph),
                GGML_STATUS_SUCCESS
            );
        }
        let mut seconds = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let start = Instant::now();
            let status = (api.ggml_backend_graph_compute)(backend.raw, graph);
            seconds.push(start.elapsed().as_secs_f64());
            assert_eq!(status, GGML_STATUS_SUCCESS);
        }
        let mut actual = vec![0.0_f32; m * n * HEADS * batches];
        (api.ggml_backend_tensor_get)(output, actual.as_mut_ptr().cast(), 0, actual.len() * 4);
        assert!(
            actual.iter().all(|x| x.is_finite()),
            "non-finite primitive output"
        );
        let mut max_error = 0.0_f64;
        let mut error_energy = 0.0_f64;
        let mut reference_energy = 0.0_f64;
        let mut checked = 0;
        for head_batch in [0, HEADS * batches - 1] {
            for column in [0, n / 2, n - 1] {
                let base = m * (column + n * head_batch);
                let (row_max, row_sum) = if k == 0 {
                    let max = b[base..base + m]
                        .iter()
                        .copied()
                        .fold(f32::NEG_INFINITY, f32::max) as f64;
                    let sum: f64 = b[base..base + m]
                        .iter()
                        .map(|&x| (f64::from(x) - max).exp())
                        .sum();
                    (max, sum)
                } else {
                    (0.0, 0.0)
                };
                for row in [0, m / 2, m - 1] {
                    let expected = if k == 0 {
                        (f64::from(b[base + row]) - row_max).exp() / row_sum
                    } else {
                        (0..k)
                            .map(|r| {
                                f64::from(value(a[r + k * (row + m * head_batch)]))
                                    * f64::from(
                                        b[rhs_offset(k, n, r, column, head_batch, interleaved)],
                                    )
                            })
                            .sum()
                    };
                    let error = f64::from(actual[base + row]) - expected;
                    max_error = max_error.max(error.abs());
                    error_energy += error * error;
                    reference_energy += expected * expected;
                    checked += 1;
                }
            }
        }
        let nmse = error_energy / reference_energy.max(f64::MIN_POSITIVE);
        assert!(nmse < 5e-4, "primitive reference mismatch: {nmse:e}");
        eprintln!(
            "ATTENTION_PRIMITIVE_RESULT {}",
            serde_json::json!({
                "primitive": format!("{primitive:?}"), "sequence": sequence,
                "m": m, "n": n, "k": k, "heads": HEADS, "independent_batches": batches,
                "rhs_interleaved": interleaved, "warmup_calls": WARMUPS,
                "host_compute_seconds": seconds,
                "matmul_flops": 2_u64 * m as u64 * n as u64 * k as u64 * HEADS as u64 * batches as u64,
                "softmax_minimum_io_bytes": if k == 0 { 8_u64 * actual.len() as u64 } else { 0 },
                "finite_outputs": actual.len(), "reference_values": checked,
                "max_abs_error": max_error, "nmse": nmse,
                "scope": "standalone primitive, not fused attention or end-to-end model throughput"
            })
        );
    }
}

#[test]
fn fixture_rhs_layout_and_half_values() {
    assert_eq!(value(0x3800), 0.5);
    assert_eq!(value(0xb800), -0.5);
    assert_eq!(rhs_offset(64, 1722, 63, 1721, 15, true), 64 * 1722 * 16 - 1);
    assert_eq!(
        rhs_offset(64, 1722, 63, 1721, 15, false),
        64 * 1722 * 16 - 1
    );
    assert_eq!(rhs_offset(64, 1722, 0, 1, 0, true), 64 * HEADS);
    assert_eq!(rhs_offset(64, 1722, 0, 1, 0, false), 64);
}

#[test]
#[ignore = "explicit B580 diagnostic; set UTA_TEST_GGML_RUNTIME_DIR and serialize GPU tests"]
fn b580_attention_primitive_shapes() {
    let path = std::env::var("UTA_TEST_GGML_RUNTIME_DIR")
        .expect("explicit runtime library directory required");
    let runtime = GgmlRuntime::load(Path::new(&path)).expect("GGML library load failed");
    let device = runtime
        .devices()
        .expect("GGML device enumeration failed")
        .into_iter()
        .find(|d| d.kind == DeviceKind::DiscreteGpu && d.description.contains("B580"))
        .expect("selected B580 is not available; no fallback");
    eprintln!("primitive test device: {device:?}; libraries: {path}");
    let backend = runtime
        .create_backend(&device)
        .expect("B580 initialization failed");
    // The time case needs quadratic scratch; eight independent bands keep each
    // standalone primitive below 2 GiB. Context is not shortened. The frequency
    // case retains all 1722 independent frames, as in the model.
    for (sequence, batches) in [(1722, 8), (90, 1722)] {
        for primitive in [
            Primitive::QueryKey,
            Primitive::Softmax,
            Primitive::ProbabilityValue,
        ] {
            run(&backend, sequence, batches, primitive);
        }
    }
}
