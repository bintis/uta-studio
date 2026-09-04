// Depthwise 1D conv over a [time, channels] contiguous input with a
// [channels, kernel_size] kernel, matching
// `crate::tensor::cpu::conv::conv1d_dw`'s zero-padding semantics.
struct Params {
    time: u32,
    channels: u32,
    kernel_size: u32,
    out_time: u32,
    stride: u32,
    padding: u32,
    has_bias: u32,
    width: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input_data: array<f32>;
@group(0) @binding(2) var<storage, read> kernel_data: array<f32>;
@group(0) @binding(3) var<storage, read> bias_data: array<f32>;
@group(0) @binding(4) var<storage, read_write> out_data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.y * params.width + gid.x;
    let total = params.out_time * params.channels;
    if (idx >= total) {
        return;
    }
    let out_t = idx / params.channels;
    let channel = idx % params.channels;

    var sum: f32 = 0.0;
    if (params.has_bias != 0u) {
        sum = bias_data[channel];
    }
    for (var k: u32 = 0u; k < params.kernel_size; k = k + 1u) {
        let input_index = out_t * params.stride + k;
        if (input_index < params.padding) {
            continue;
        }
        let input_t = input_index - params.padding;
        if (input_t >= params.time) {
            continue;
        }
        sum = sum + input_data[input_t * params.channels + channel]
            * kernel_data[channel * params.kernel_size + k];
    }
    out_data[idx] = sum;
}
