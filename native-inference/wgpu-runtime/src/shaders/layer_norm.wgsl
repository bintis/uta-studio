struct Params {
    rows: u32,
    channels: u32,
    epsilon: f32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read> weight: array<f32>;
@group(0) @binding(3) var<storage, read> bias: array<f32>;
@group(0) @binding(4) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.x;
    if (row >= p.rows) { return; }
    let base = row * p.channels;
    var mean = 0.0;
    for (var channel = 0u; channel < p.channels; channel = channel + 1u) {
        mean = mean + input[base + channel];
    }
    mean = mean / f32(p.channels);
    var variance = 0.0;
    for (var channel = 0u; channel < p.channels; channel = channel + 1u) {
        let centered = input[base + channel] - mean;
        variance = variance + centered * centered;
    }
    let inverse_std = inverseSqrt(variance / f32(p.channels) + p.epsilon);
    for (var channel = 0u; channel < p.channels; channel = channel + 1u) {
        output[base + channel] = (input[base + channel] - mean) * inverse_std * weight[channel] + bias[channel];
    }
}
