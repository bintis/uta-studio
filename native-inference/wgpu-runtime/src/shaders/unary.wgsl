struct Params {
    total: u32,
    operation: u32,
    width: u32,
    element_offset: u32,
    dispatch_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

fn sigmoid(value: f32) -> f32 {
    return 1.0 / (1.0 + exp(-value));
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let local = gid.y * p.width + gid.x;
    if (local >= p.dispatch_count) { return; }
    let flat = p.element_offset + local;
    if (flat >= p.total) { return; }
    let value = input[flat];
    if (p.operation == 0u) {
        output[flat] = max(value, 0.0);
    } else if (p.operation == 1u) {
        output[flat] = value * sigmoid(value);
    } else {
        output[flat] = sigmoid(value);
    }
}
