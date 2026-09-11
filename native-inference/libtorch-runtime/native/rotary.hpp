#pragma once
#include <ATen/ATen.h>
#include <cmath>
#include <stdexcept>

namespace uta::torch_native {
// Invocation-owned constants, not a cache keyed only by length: estimator
// positions depend on the actual decoded regions and must never be stale.
struct RotaryPhase {
    at::Tensor cosine, sine;
};
inline RotaryPhase prepare_rotary_phase(int64_t dimensions, const at::Tensor& positions,
                                        const at::TensorOptions& options, double base = 10000.0) {
    if (dimensions % 2) throw std::invalid_argument("rotary dimension must be even");
    auto frequency = at::exp(at::arange(0, dimensions, 2, options.dtype(at::kFloat)) * (-std::log(base) / dimensions));
    auto phase = positions.to(at::kFloat).unsqueeze(-1) * frequency;
    return {phase.cos(), phase.sin()};
}
inline at::Tensor apply_rotary_interleaved(const at::Tensor& input, const RotaryPhase& phase) {
    auto cosine = phase.cosine, sine = phase.sine;
    while (cosine.dim() < input.dim()) {
        cosine = cosine.unsqueeze(0);
        sine = sine.unsqueeze(0);
    }
    auto shape = input.sizes().vec();
    shape.back() /= 2;
    shape.push_back(2);
    auto paired = input.reshape(shape);
    auto even = paired.select(-1, 0), odd = paired.select(-1, 1);
    return at::stack({even * cosine - odd * sine, even * sine + odd * cosine}, -1).flatten(-2);
}
inline at::Tensor apply_rotary_split(const at::Tensor& input, const RotaryPhase& phase) {
    auto cosine = phase.cosine, sine = phase.sine;
    while (cosine.dim() < input.dim()) {
        cosine = cosine.unsqueeze(0);
        sine = sine.unsqueeze(0);
    }
    auto halves = input.chunk(2, -1);
    return at::cat({halves[0] * cosine - halves[1] * sine, halves[1] * cosine + halves[0] * sine}, -1);
}
} // namespace uta::torch_native
