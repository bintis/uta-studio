// Rotates the [start, start+dims) sub-range of each head_dim-wide row of a
// [num_heads, seq_len, head_dim] contiguous tensor, using per-token angles
// `position * inv_freqs[pair_index]`. `rope` dispatches this once
// (start=0, dims=rope_dims, positions=global positions); `region_rope`
// dispatches it twice with disjoint [start, dims) halves and two different
// position sources — see `crate::tensor::cpu::rope::apply_rope_chunk`, which
// this mirrors exactly.
struct Params {
    num_heads: u32,
    seq_len: u32,
    head_dim: u32,
    start: u32,
    dims: u32,
    width: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> inv_freqs: array<f32>;
@group(0) @binding(2) var<storage, read> positions: array<f32>;
@group(0) @binding(3) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.y * params.width + gid.x;
    let pairs = params.dims / 2u;
    let total = params.num_heads * params.seq_len * pairs;
    if (idx >= total || pairs == 0u) {
        return;
    }

    let pair_index = idx % pairs;
    let token = (idx / pairs) % params.seq_len;
    let head = idx / (pairs * params.seq_len);

    let base = (head * params.seq_len + token) * params.head_dim + params.start + pair_index * 2u;
    let angle = positions[token] * inv_freqs[pair_index];
    let s = sin(angle);
    let c = cos(angle);
    let x0 = data[base];
    let x1 = data[base + 1u];
    data[base] = x0 * c - x1 * s;
    data[base + 1u] = x0 * s + x1 * c;
}
