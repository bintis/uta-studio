struct Params {
    frames: u32,
    input_channels: u32,
    output_channels: u32,
    total: u32,
    width: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let flat = gid.y * p.width + gid.x;
    if (flat >= p.total) { return; }
    let frame = flat / p.output_channels;
    let channel = flat % p.output_channels;
    let base = frame * p.input_channels;
    let a = input[base + channel];
    let b = input[base + p.output_channels + channel];
    output[flat] = a / (1.0 + exp(-b));
}
