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
    element_offset: u32,
    dispatch_count: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read> weight: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let local = gid.y * p.width + gid.x;
    if (local >= p.dispatch_count) { return; }
    let flat = p.element_offset + local;
    if (flat >= p.total) { return; }
    let plane = p.out_height * p.out_width;
    let oc = flat / plane;
    let rem = flat % plane;
    let oy = rem / p.out_width;
    let ox = rem % p.out_width;
    var sum = 0.0;
    for (var ky = 0u; ky < p.kernel_height; ky = ky + 1u) {
        let numerator_y = i32(oy) + i32(p.pad_height) - i32(ky);
        if (numerator_y < 0 || numerator_y % i32(p.stride_height) != 0) { continue; }
        let iy = numerator_y / i32(p.stride_height);
        if (iy >= i32(p.in_height)) { continue; }
        for (var kx = 0u; kx < p.kernel_width; kx = kx + 1u) {
            let numerator_x = i32(ox) + i32(p.pad_width) - i32(kx);
            if (numerator_x < 0 || numerator_x % i32(p.stride_width) != 0) { continue; }
            let ix = numerator_x / i32(p.stride_width);
            if (ix >= i32(p.in_width)) { continue; }
            for (var ic = 0u; ic < p.in_channels; ic = ic + 1u) {
                let input_index = (ic * p.in_height + u32(iy)) * p.in_width + u32(ix);
                let weight_index = ((ic * p.out_channels + oc) * p.kernel_height + ky) * p.kernel_width + kx;
                sum = sum + input[input_index] * weight[weight_index];
            }
        }
    }
    output[flat] = sum;
}
