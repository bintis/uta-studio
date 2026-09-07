struct Params {
    signal_len: u32,
    out_channels: u32,
    kernel: u32,
    hop: u32,
    frames: u32,
    total: u32,
    width: u32,
    element_offset: u32,
    dispatch_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> signal: array<f32>;
@group(0) @binding(2) var<storage, read> weight: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let local = gid.y * p.width + gid.x;
    if (local >= p.dispatch_count) { return; }
    let flat = p.element_offset + local;
    if (flat >= p.total) { return; }
    let frame = flat / p.out_channels;
    let channel = flat % p.out_channels;
    let start = frame * p.hop;
    var sum = 0.0;
    for (var index = 0u; index < p.kernel; index = index + 1u) {
        let source = start + index;
        if (source < p.signal_len) {
            sum = sum + signal[source] * weight[channel * p.kernel + index];
        }
    }
    output[flat] = sum;
}
