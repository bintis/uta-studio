struct Params {
    query_frames: u32,
    key_frames: u32,
    hidden: u32,
    heads: u32,
    head_width: u32,
    total: u32,
    width: u32,
    element_offset: u32,
    dispatch_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> attention: array<f32>;
@group(0) @binding(2) var<storage, read> value: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let local = gid.y * p.width + gid.x;
    if (local >= p.dispatch_count) { return; }
    let index = p.element_offset + local;
    if (index >= p.total) { return; }

    let channel = index % p.hidden;
    let query_frame = index / p.hidden;
    let head = channel / p.head_width;
    let attention_base = (head * p.query_frames + query_frame) * p.key_frames;
    var sum = 0.0;
    for (var key_frame = 0u; key_frame < p.key_frames; key_frame = key_frame + 1u) {
        sum = sum
            + attention[attention_base + key_frame]
            * value[key_frame * p.hidden + channel];
    }
    output[index] = sum;
}
