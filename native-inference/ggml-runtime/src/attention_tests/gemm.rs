use std::time::Instant;

use super::{Storage, backend, fixture_bits, fixture_value};
use crate::ffi::*;

/// A large dense GEMM ceiling, NOT an equivalent attention workload.
#[test]
#[ignore = "explicit bounded B580 dense GEMM; requires UTA_TEST_GGML_RUNTIME_DIR"]
fn b580_dense_matmul_ceiling() {
    let backend = backend();
    let api = &backend.runtime.model_api;
    const DIM: usize = 4096;
    // SAFETY: fixed positive shapes fit the retained metadata arena. The buffer
    // is uniquely owned and outlives every synchronous call and readback.
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
        let a = (api.ggml_new_tensor_2d)(context, GGML_TYPE_F16, DIM as i64, DIM as i64);
        let b = (api.ggml_new_tensor_2d)(context, GGML_TYPE_F16, DIM as i64, DIM as i64);
        let output = (api.ggml_mul_mat)(context, a, b);
        assert!(!output.is_null());
        (api.ggml_mul_mat_set_prec)(output, GGML_PREC_F32);
        (api.ggml_set_output)(output);
        let graph = (api.ggml_new_graph_custom)(context, 16, false);
        assert!(!graph.is_null());
        (api.ggml_build_forward_expand)(graph, output);
        storage.buffer = (api.ggml_backend_alloc_ctx_tensors_from_buft)(
            context,
            (api.ggml_backend_get_default_buffer_type)(backend.raw),
        );
        assert!(!storage.buffer.is_null());
        let lhs: Vec<u16> = (0..DIM * DIM).map(|i| fixture_bits(i, 137)).collect();
        let rhs: Vec<u16> = (0..DIM * DIM).map(|i| fixture_bits(i, 997)).collect();
        (api.ggml_backend_tensor_set)(a, lhs.as_ptr().cast(), 0, lhs.len() * 2);
        (api.ggml_backend_tensor_set)(b, rhs.as_ptr().cast(), 0, rhs.len() * 2);
        for _ in 0..32 {
            assert_eq!(
                (api.ggml_backend_graph_compute)(backend.raw, graph),
                GGML_STATUS_SUCCESS
            );
        }
        let mut seconds = Vec::new();
        for _ in 0..12 {
            let start = Instant::now();
            let status = (api.ggml_backend_graph_compute)(backend.raw, graph);
            seconds.push(start.elapsed().as_secs_f64());
            assert_eq!(status, GGML_STATUS_SUCCESS);
        }
        let mut actual = vec![0.0_f32; DIM * DIM];
        (api.ggml_backend_tensor_get)(output, actual.as_mut_ptr().cast(), 0, actual.len() * 4);
        assert!(actual.iter().all(|v| v.is_finite()));
        let mut squared_error = 0.0;
        let mut energy = 0.0;
        let mut max_error = 0.0_f64;
        for row in [0, DIM / 2, DIM - 1] {
            for col in [0, DIM / 2, DIM - 1] {
                let reference: f64 = (0..DIM)
                    .map(|k| {
                        f64::from(fixture_value(lhs[row * DIM + k]))
                            * f64::from(fixture_value(rhs[col * DIM + k]))
                    })
                    .sum();
                let error = f64::from(actual[col * DIM + row]) - reference;
                squared_error += error * error;
                energy += reference * reference;
                max_error = max_error.max(error.abs());
            }
        }
        let nmse = squared_error / energy.max(f64::MIN_POSITIVE);
        eprintln!(
            "MUL_MAT_RESULT {}",
            serde_json::json!({
                "m": DIM, "n": DIM, "k": DIM, "input_type": "f16",
                "accumulator": "f32", "warmup_calls": 32,
                "host_compute_seconds": seconds,
                "matmul_flops": 2_u64 * DIM as u64 * DIM as u64 * DIM as u64,
                "max_abs_error": max_error, "nmse": nmse,
                "finite_outputs": actual.len(), "reference_values": 9,
            })
        );
        assert!(nmse < 5e-4, "dense GEMM NMSE: {nmse}");
    }
}
