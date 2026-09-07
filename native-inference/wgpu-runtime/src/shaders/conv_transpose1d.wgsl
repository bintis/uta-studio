struct Params {
    input_frames: u32,
    in_channels: u32,
    out_channels: u32,
    kernel: u32,
    stride: u32,
    padding: u32,
    output_frames: u32,
    total: u32,
    width: u32,
    element_offset: u32,
    dispatch_count: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read> weight: array<f32>;
@group(0) @binding(3) var<storage, read> bias: array<f32>;
@group(0) @binding(4) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let local = gid.y * p.width + gid.x;
    if (local >= p.dispatch_count) { return; }
    let flat = p.element_offset + local;
    if (flat >= p.total) { return; }
    let output_frame = flat / p.out_channels;
    let oc = flat % p.out_channels;
    var sum = bias[oc];
    for (var kernel_index = 0u; kernel_index < p.kernel; kernel_index = kernel_index + 1u) {
        let numerator = i32(output_frame) + i32(p.padding) - i32(kernel_index);
        if (numerator < 0 || numerator % i32(p.stride) != 0) { continue; }
        let input_frame = numerator / i32(p.stride);
        if (input_frame >= i32(p.input_frames)) { continue; }
        for (var ic = 0u; ic < p.in_channels; ic = ic + 1u) {
            let weight_index = (ic * p.out_channels + oc) * p.kernel + kernel_index;
            sum = sum + input[u32(input_frame) * p.in_channels + ic] * weight[weight_index];
        }
    }
    output[flat] = sum;
}
