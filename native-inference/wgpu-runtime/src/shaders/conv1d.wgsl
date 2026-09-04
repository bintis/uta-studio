struct Params {
    frames: u32,
    in_channels: u32,
    out_channels: u32,
    kernel: u32,
    padding: u32,
    total: u32,
    width: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read> weight: array<f32>;
@group(0) @binding(3) var<storage, read> bias: array<f32>;
@group(0) @binding(4) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let flat = gid.y * p.width + gid.x;
    if (flat >= p.total) { return; }
    let frame = flat / p.out_channels;
    let oc = flat % p.out_channels;
    var sum = bias[oc];
    for (var kernel_index = 0u; kernel_index < p.kernel; kernel_index = kernel_index + 1u) {
        let source = i32(frame) + i32(kernel_index) - i32(p.padding);
        if (source < 0 || source >= i32(p.frames)) { continue; }
        for (var ic = 0u; ic < p.in_channels; ic = ic + 1u) {
            let input_index = u32(source) * p.in_channels + ic;
            let weight_index = kernel_index * p.in_channels * p.out_channels + ic * p.out_channels + oc;
            sum = sum + input[input_index] * weight[weight_index];
        }
    }
    output[flat] = sum;
}
