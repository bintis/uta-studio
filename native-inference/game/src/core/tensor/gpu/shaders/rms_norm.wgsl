// RMS norm over the last axis of a [rows, feature_dim] contiguous view.
struct Params {
    rows: u32,
    feature_dim: u32,
    eps: f32,
    width: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input_data: array<f32>;
@group(0) @binding(2) var<storage, read> weight: array<f32>;
@group(0) @binding(3) var<storage, read_write> out_data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.y * params.width + gid.x;
    if (row >= params.rows) {
        return;
    }
    let base = row * params.feature_dim;

    var sum_sq: f32 = 0.0;
    for (var i: u32 = 0u; i < params.feature_dim; i = i + 1u) {
        let v = input_data[base + i];
        sum_sq = sum_sq + v * v;
    }
    let mean_sq = sum_sq / f32(params.feature_dim);
    let inv_rms = inverseSqrt(mean_sq + params.eps);

    for (var i: u32 = 0u; i < params.feature_dim; i = i + 1u) {
        out_data[base + i] = input_data[base + i] * inv_rms * weight[i];
    }
}
