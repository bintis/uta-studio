// Last-axis softmax over a [outer, axis_len] contiguous view, in place.
// One thread per row; rows are the model's key/note counts (small), so a
// sequential per-row reduction is simple and correct rather than optimal.
// The running max seeds from the row's own first element (not a sentinel
// constant), so a row containing a real -inf mask value is handled exactly
// like `crate::tensor::cpu::elementwise::apply_softmax_inplace`.
struct Params {
    outer: u32,
    axis_len: u32,
    width: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.y * params.width + gid.x;
    if (row >= params.outer) {
        return;
    }
    let base = row * params.axis_len;

    var max_value = data[base];
    for (var i: u32 = 1u; i < params.axis_len; i = i + 1u) {
        let v = data[base + i];
        if (v > max_value) {
            max_value = v;
        }
    }

    var sum: f32 = 0.0;
    for (var i: u32 = 0u; i < params.axis_len; i = i + 1u) {
        let e = exp(data[base + i] - max_value);
        data[base + i] = e;
        sum = sum + e;
    }

    if (sum > 0.0) {
        for (var i: u32 = 0u; i < params.axis_len; i = i + 1u) {
            data[base + i] = data[base + i] / sum;
        }
    }
}
