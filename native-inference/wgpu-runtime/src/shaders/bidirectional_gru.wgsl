struct Params {
    frames: u32,
    hidden: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> projected_forward: array<f32>;
@group(0) @binding(2) var<storage, read> projected_backward: array<f32>;
@group(0) @binding(3) var<storage, read> recurrent_weight: array<f32>;
@group(0) @binding(4) var<storage, read> bias: array<f32>;
@group(0) @binding(5) var<storage, read_write> output: array<f32>;

var<workgroup> previous: array<f32, 256>;
var<workgroup> next: array<f32, 256>;

fn sigmoid(value: f32) -> f32 {
    return 1.0 / (1.0 + exp(-value));
}

@compute @workgroup_size(64)
fn main(
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
) {
    let direction = workgroup_id.x;
    let gates = 3u * p.hidden;
    let recurrent_stage = gates * p.hidden;
    let recurrent_base = direction * recurrent_stage;
    let bias_base = direction * 6u * p.hidden;

    for (var index = local_id.x; index < p.hidden; index = index + 64u) {
        previous[index] = 0.0;
    }
    workgroupBarrier();

    for (var step = 0u; step < p.frames; step = step + 1u) {
        let time = select(step, p.frames - 1u - step, direction == 1u);
        let input_base = time * gates;
        for (var row = local_id.x; row < p.hidden; row = row + 64u) {
            var recurrent_z = 0.0;
            var recurrent_r = 0.0;
            var recurrent_h = 0.0;
            for (var inner = 0u; inner < p.hidden; inner = inner + 1u) {
                let prior = previous[inner];
                recurrent_z = recurrent_z
                    + recurrent_weight[recurrent_base + row * p.hidden + inner] * prior;
                recurrent_r = recurrent_r
                    + recurrent_weight[
                        recurrent_base + (p.hidden + row) * p.hidden + inner
                    ] * prior;
                recurrent_h = recurrent_h
                    + recurrent_weight[
                        recurrent_base + (2u * p.hidden + row) * p.hidden + inner
                    ] * prior;
            }

            var input_z = projected_forward[input_base + row];
            var input_r = projected_forward[input_base + p.hidden + row];
            var input_h = projected_forward[input_base + 2u * p.hidden + row];
            if (direction == 1u) {
                input_z = projected_backward[input_base + row];
                input_r = projected_backward[input_base + p.hidden + row];
                input_h = projected_backward[input_base + 2u * p.hidden + row];
            }

            let z = sigmoid(
                input_z + recurrent_z + bias[bias_base + row]
                    + bias[bias_base + 3u * p.hidden + row]
            );
            let r = sigmoid(
                input_r + recurrent_r + bias[bias_base + p.hidden + row]
                    + bias[bias_base + 4u * p.hidden + row]
            );
            let candidate = tanh(
                input_h
                    + r * (recurrent_h + bias[bias_base + 5u * p.hidden + row])
                    + bias[bias_base + 2u * p.hidden + row]
            );
            let value = candidate + z * (previous[row] - candidate);
            next[row] = value;
            output[(direction * p.frames + time) * p.hidden + row] = value;
        }
        workgroupBarrier();
        for (var index = local_id.x; index < p.hidden; index = index + 64u) {
            previous[index] = next[index];
        }
        workgroupBarrier();
    }
}
