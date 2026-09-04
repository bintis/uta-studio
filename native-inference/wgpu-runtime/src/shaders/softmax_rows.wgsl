struct Params {
    rows: u32,
    channels: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.x;
    if (row >= p.rows) { return; }
    let base = row * p.channels;
    var maximum = -3.402823466e+38;
    for (var channel = 0u; channel < p.channels; channel = channel + 1u) {
        maximum = max(maximum, input[base + channel]);
    }
    var total = 0.0;
    for (var channel = 0u; channel < p.channels; channel = channel + 1u) {
        total = total + exp(input[base + channel] - maximum);
    }
    let denominator = max(total, 1.175494351e-38);
    for (var channel = 0u; channel < p.channels; channel = channel + 1u) {
        output[base + channel] = exp(input[base + channel] - maximum) / denominator;
    }
}
