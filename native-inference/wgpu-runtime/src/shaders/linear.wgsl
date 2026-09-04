struct Params {
    m: u32,
    k: u32,
    n: u32,
    weight_transposed: u32,
    has_bias: u32,
    total: u32,
    width: u32,
    _pad: u32,
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
    let row = flat / p.n;
    let col = flat % p.n;
    var sum = 0.0;
    if (p.has_bias != 0u) { sum = bias[col]; }
    for (var inner = 0u; inner < p.k; inner = inner + 1u) {
        let wi = select(inner * p.n + col, col * p.k + inner, p.weight_transposed != 0u);
        sum = sum + input[row * p.k + inner] * weight[wi];
    }
    output[flat] = sum;
}
