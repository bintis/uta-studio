// Batched matmul: lhs [batch, m, k] @ rhs [batch, k, n] -> out [batch, m, n].
// A 2D matmul is dispatched with batch=1.
struct Params {
    batch: u32,
    m: u32,
    k: u32,
    n: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> lhs: array<f32>;
@group(0) @binding(2) var<storage, read> rhs: array<f32>;
@group(0) @binding(3) var<storage, read_write> out_data: array<f32>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let col = gid.x;
    let row = gid.y;
    let b = gid.z;
    if (b >= params.batch || row >= params.m || col >= params.n) {
        return;
    }

    let lhs_base = b * params.m * params.k + row * params.k;
    let rhs_base = b * params.k * params.n;
    var acc: f32 = 0.0;
    for (var i: u32 = 0u; i < params.k; i = i + 1u) {
        acc = acc + lhs[lhs_base + i] * rhs[rhs_base + i * params.n + col];
    }
    out_data[b * params.m * params.n + row * params.n + col] = acc;
}
