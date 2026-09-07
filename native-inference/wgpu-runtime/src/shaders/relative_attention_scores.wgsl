struct Params {
    frames: u32,
    position_frames: u32,
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
@group(0) @binding(3) var<storage, read> position: array<f32>;
@group(0) @binding(4) var<storage, read> bias_u: array<f32>;
@group(0) @binding(5) var<storage, read> bias_v: array<f32>;
@group(0) @binding(6) var<storage, read> nonpadding: array<f32>;
@group(0) @binding(7) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let local = gid.y * p.width + gid.x;
    if (local >= p.dispatch_count) { return; }
    let index = p.element_offset + local;
    if (index >= p.total) { return; }

    let key_frame = index % p.frames;
    let row_index = index / p.frames;
    let query_frame = row_index % p.frames;
    let head = row_index / p.frames;
    if (head >= p.heads || nonpadding[key_frame] <= 0.5) {
        output[index] = bitcast<f32>(0xff800000u);
        return;
    }

    let channel_base = head * p.head_width;
    var content_score = 0.0;
    for (var d = 0u; d < p.head_width; d = d + 1u) {
        let channel = channel_base + d;
        content_score = content_score
            + (query[query_frame * p.hidden + channel] + bias_u[channel])
            * key[key_frame * p.hidden + channel];
    }

    // Literal pad -> reshape -> drop-first-row -> reshape indexing used by
    // both ESPnet-style and Transformer-XL relative shifts. position_frames
    // is T for STARS/ROSVOT and 2*T-1 for FireRed.
    let padded_index = p.frames + query_frame * p.position_frames + key_frame;
    let padded_width = p.position_frames + 1u;
    let raw_query = padded_index / padded_width;
    let padded_column = padded_index % padded_width;
    var position_score = 0.0;
    if (padded_column != 0u && raw_query < p.frames) {
        let position_frame = padded_column - 1u;
        for (var d = 0u; d < p.head_width; d = d + 1u) {
            let channel = channel_base + d;
            position_score = position_score
                + (query[raw_query * p.hidden + channel] + bias_v[channel])
                * position[position_frame * p.hidden + channel];
        }
    }

    output[index] = (content_score + position_score) * inverseSqrt(f32(p.head_width));
}
