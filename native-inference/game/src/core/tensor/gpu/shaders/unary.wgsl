// Elementwise unary op: scale=0, sigmoid=1, gelu=2.
struct Params {
    op_code: u32,
    total: u32,
    scale: f32,
    width: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input_data: array<f32>;
@group(0) @binding(2) var<storage, read_write> out_data: array<f32>;

// Abramowitz & Stegun 7.1.26 — must match
// `crate::tensor::cpu::elementwise::erf_approx` exactly; gelu is specified
// against this particular polynomial, not "an" erf approximation.
fn erf_approx(x_in: f32) -> f32 {
    let sign = select(1.0, -1.0, x_in < 0.0);
    let x = abs(x_in);
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let y = 1.0 - (((((1.0614054 * t - 1.4531521) * t + 1.4214138) * t - 0.28449672) * t
        + 0.2548296) * t) * exp(-x * x);
    return sign * y;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.y * params.width + gid.x;
    if (idx >= params.total) {
        return;
    }
    let v = input_data[idx];
    var result = v;
    if (params.op_code == 0u) {
        result = v * params.scale;
    } else if (params.op_code == 1u) {
        result = 1.0 / (1.0 + exp(-v));
    } else if (params.op_code == 2u) {
        result = 0.5 * v * (1.0 + erf_approx(v * 0.7071067811865476));
    }
    out_data[idx] = result;
}
