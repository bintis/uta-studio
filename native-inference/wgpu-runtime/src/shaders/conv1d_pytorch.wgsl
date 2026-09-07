struct Params {
    frames: u32,
    in_channels: u32,
    out_channels: u32,
    kernel: u32,
    groups: u32,
    dilation: u32,
    padding: u32,
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
    let frame = flat / p.out_channels;
    let oc = flat % p.out_channels;
    let in_per_group = p.in_channels / p.groups;
    let out_per_group = p.out_channels / p.groups;
    let group = oc / out_per_group;
    var sum = bias[oc];
    for (var local_ic = 0u; local_ic < in_per_group; local_ic = local_ic + 1u) {
        let ic = group * in_per_group + local_ic;
        for (var kernel_index = 0u; kernel_index < p.kernel; kernel_index = kernel_index + 1u) {
            let source = i32(frame) + i32(kernel_index * p.dilation) - i32(p.padding);
            if (source < 0 || source >= i32(p.frames)) { continue; }
            let weight_index = (oc * in_per_group + local_ic) * p.kernel + kernel_index;
            sum = sum + input[u32(source) * p.in_channels + ic] * weight[weight_index];
        }
    }
    output[flat] = sum;
}
