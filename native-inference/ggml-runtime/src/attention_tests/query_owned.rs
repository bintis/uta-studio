//! Opt-in regression cases that actually enter the query-owned (>32-query) path.
//! These also run against the control library; enablement and pipeline selection
//! are recorded by the backend, not inferred merely from successful output.

use super::{Shape, backend, run_shape};

#[test]
#[ignore = "explicit B580 query-owned attention regression; requires UTA_TEST_GGML_RUNTIME_DIR"]
fn query_owned_tail_mask_and_stride() {
    let backend = backend();
    for (queries, keys, padding, masked) in [
        (33, 1, 0, false),
        (33, 15, 0, false),
        (33, 16, 0, false),
        (33, 17, 0, false),
        (33, 31, 0, false),
        (64, 32, 0, false),
        (65, 33, 0, false),
        (33, 64, 0, false),
        (65, 65, 0, false),
        (65, 90, 4, false),
        (65, 33, 0, true),
        (65, 1722, 0, false),
        (33, 1722, 0, true),
    ] {
        run_shape(
            &backend,
            Shape {
                d: 64,
                queries,
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
#[ignore = "explicit B580 grouped-query and KV broadcast regression"]
fn grouped_query_and_key_head_broadcast() {
    let backend = super::backend();
    for (queries, heads, key_heads, keys, padding, masked) in [
        (1, 8, 1, 33, 0, false),
        (8, 16, 1, 65, 0, false),
        (1, 32, 1, 33, 0, false),
        (8, 32, 1, 65, 0, true),
        (17, 8, 2, 33, 4, false),
        (33, 8, 2, 65, 0, false),
        (65, 8, 2, 90, 4, true),
    ] {
        super::run_shape_with_key_heads(
            &backend,
            super::Shape {
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
        );
    }
}
