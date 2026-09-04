struct Params {
    channels: u32,
    height: u32,
    width_in: u32,
    total: u32,
    dispatch_width: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let flat = gid.y * p.dispatch_width + gid.x;
    if (flat >= p.total) { return; }
    let channel = flat % p.channels;
    let spatial = flat / p.channels;
    let height_index = spatial / p.width_in;
    let width_index = spatial % p.width_in;
    let source = (channel * p.height + height_index) * p.width_in + width_index;
    output[flat] = input[source];
}
