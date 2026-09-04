// General rank-4 broadcasting binary op (add: op_code=0, mul: op_code=1).
// Shapes/strides are left-padded to rank 4 with (dim=1, stride=0), matching
// the alignment `crate::tensor::cpu::util::broadcast_offset` uses on the CPU
// path: a dimension of size 1 never contributes to the operand offset, which
// is exactly what makes it "broadcast".
struct Params {
    lhs_shape: vec4<u32>,
    lhs_strides: vec4<u32>,
    rhs_shape: vec4<u32>,
    rhs_strides: vec4<u32>,
    out_shape: vec4<u32>,
    out_strides: vec4<u32>,
    op_code: u32,
    total: u32,
    // Threads-per-row of the (possibly 2D) dispatch grid — see
    // `gpu::dispatch_grid_1d` in mod.rs for why a flat 1D dispatch isn't
    // always possible (Vulkan caps workgroups-per-dimension at 65535,
    // which a real attention-score-sized tensor exceeds).
    width: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> lhs: array<f32>;
@group(0) @binding(2) var<storage, read> rhs: array<f32>;
@group(0) @binding(3) var<storage, read_write> out_data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.y * params.width + gid.x;
    if (idx >= params.total) {
        return;
    }

    var remaining = idx;
    var lhs_offset: u32 = 0u;
    var rhs_offset: u32 = 0u;
    for (var axis: u32 = 0u; axis < 4u; axis = axis + 1u) {
        let dim = params.out_shape[axis];
        let stride = params.out_strides[axis];
        var coord: u32 = 0u;
        if (stride > 0u) {
            coord = (remaining / stride) % dim;
        }
        if (params.lhs_shape[axis] > 1u) {
            lhs_offset = lhs_offset + coord * params.lhs_strides[axis];
        }
        if (params.rhs_shape[axis] > 1u) {
            rhs_offset = rhs_offset + coord * params.rhs_strides[axis];
        }
    }

    let a = lhs[lhs_offset];
    let b = rhs[rhs_offset];
    if (params.op_code == 0u) {
        out_data[idx] = a + b;
    } else {
        out_data[idx] = a * b;
    }
}
