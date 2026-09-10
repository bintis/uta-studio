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
