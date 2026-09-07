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
@group(0) @binding(1) var<storage, read> query: array<f32>;
@group(0) @binding(2) var<storage, read> key: array<f32>;
@group(0) @binding(3) var<storage, read> nonpadding: array<f32>;
@group(0) @binding(4) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let local = gid.y * p.width + gid.x;
    if (local >= p.dispatch_count) { return; }
    let index = p.element_offset + local;
    if (index >= p.total) { return; }

    let key_frame = index % p.key_frames;
    let row_index = index / p.key_frames;
    let query_frame = row_index % p.query_frames;
    let head = row_index / p.query_frames;
    if (head >= p.heads || nonpadding[key_frame] <= 0.5) {
        output[index] = bitcast<f32>(0xff800000u);
        return;
    }

    let channel_base = head * p.head_width;
    var score = 0.0;
    for (var d = 0u; d < p.head_width; d = d + 1u) {
        let channel = channel_base + d;
        score = score
            + query[query_frame * p.hidden + channel]
            * key[key_frame * p.hidden + channel];
    }
    output[index] = score * inverseSqrt(f32(p.head_width));
}
