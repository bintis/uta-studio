struct Params {
    in_channels: u32,
    in_height: u32,
    in_width: u32,
    out_channels: u32,
    out_height: u32,
    out_width: u32,
    kernel_height: u32,
    kernel_width: u32,
    stride_height: u32,
    stride_width: u32,
    pad_height: u32,
    pad_width: u32,
    total: u32,
    width: u32,
    _pad0: u32,
    _pad1: u32,
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
    let plane = p.out_height * p.out_width;
    let oc = flat / plane;
    let rem = flat % plane;
    let oh = rem / p.out_width;
    let ow = rem % p.out_width;
    var sum = bias[oc];
    for (var ic = 0u; ic < p.in_channels; ic = ic + 1u) {
        for (var kh = 0u; kh < p.kernel_height; kh = kh + 1u) {
            let ih = i32(oh * p.stride_height + kh) - i32(p.pad_height);
            if (ih < 0 || ih >= i32(p.in_height)) { continue; }
            for (var kw = 0u; kw < p.kernel_width; kw = kw + 1u) {
                let iw = i32(ow * p.stride_width + kw) - i32(p.pad_width);
                if (iw < 0 || iw >= i32(p.in_width)) { continue; }
                let input_index = (ic * p.in_height + u32(ih)) * p.in_width + u32(iw);
                let weight_index = ((oc * p.in_channels + ic) * p.kernel_height + kh) * p.kernel_width + kw;
                sum = sum + input[input_index] * weight[weight_index];
            }
        }
    }
    output[flat] = sum;
}
