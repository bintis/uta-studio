//! Explicit coverage for model-independent F32/F16 K/V storage dispatch.

use super::{KvStorage, Shape, backend, reference_storage, run_shape_with_storage};

#[test]
fn floating_fixture_preserves_poison_and_exercises_rounding() {
    let bits = [0x3800, 0xbc00, 0x7e00];
    let half = reference_storage(&bits, false);
    let float = reference_storage(&bits, true);
    assert_eq!(&half[..2], &[0.5, -1.0]);
    assert!(half[2].is_nan() && float[2].is_nan());
    assert_ne!(half[0], float[0]);
    assert_ne!(half[1], float[1]);
    assert!((float[0] - half[0]).abs() < 0.001);
}

#[test]
#[ignore = "explicit B580 floating-storage numeric regression; requires UTA_TEST_GGML_RUNTIME_DIR"]
fn floating_kv_numeric() {
    let backend = backend();
    for (key_float, value_float) in [(true, true), (true, false), (false, true)] {
        let storage = KvStorage {
            key_float,
            value_float,
        };
        // Both sides of the query-owned threshold; complete blocks, tails,
        // poisoned row padding, masks, GQA and unequal query/key lengths.
        for (queries, keys, padding, masked, heads, key_heads) in [
            (1, 33, 0, false, 32, 1),
            (32, 32, 0, false, 4, 2),
            (33, 31, 0, false, 2, 2),
            (63, 64, 4, false, 4, 2),
            (65, 97, 4, true, 4, 2),
            (33, 257, 4, false, 2, 2),
        ] {
            run_shape_with_storage(
                &backend,
                Shape {
                    d: 64,
                    queries,
                    keys,
                    heads,
                    batches: 2,
                    padding,
                    masked,
                },
                key_heads,
                1,
                1.0,
                storage,
            );
        }
        // Large score changes exercise repeated online output rescaling.
        run_shape_with_storage(
            &backend,
            Shape {
                d: 64,
                queries: 65,
                keys: 1057,
                heads: 2,
                batches: 2,
                padding: 4,
                masked: false,
            },
            2,
            1,
            16.0,
            storage,
        );
        // A different head size must remain on the existing implementation.
        run_shape_with_storage(
            &backend,
            Shape {
                d: 128,
                queries: 33,
                keys: 65,
                heads: 2,
                batches: 2,
                padding: 0,
                masked: false,
            },
            2,
            1,
            1.0,
            storage,
        );
    }
}

#[test]
#[ignore = "explicit B580 MelBand-geometry timing; requires UTA_TEST_GGML_RUNTIME_DIR"]
fn floating_kv_model_shapes() {
    let backend = backend();
    for (queries, keys, batches) in [(801, 801, 60), (60, 60, 801)] {
        run_shape_with_storage(
            &backend,
            Shape {
                d: 64,
                queries,
                keys,
                heads: 8,
                batches,
                padding: 0,
                masked: false,
            },
            8,
            8,
            1.0,
            KvStorage {
                key_float: true,
                value_float: true,
            },
        );
    }
}
