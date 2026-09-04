struct Params {
    total: u32,
    width: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> left: array<f32>;
@group(0) @binding(2) var<storage, read> right: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let flat = gid.y * p.width + gid.x;
    if (flat >= p.total) { return; }
    output[flat] = left[flat] + right[flat];
}
