#pragma once
#include "runtime.hpp"

namespace uta::torch_native {
// Shared learned primitives, not a backend emulation. Both families use the
// same CMU topology but have different conditioning, FFNs and output heads.
class NoteNetwork {
public:
    explicit NoteNetwork(Plan& owner) : owner(owner), weights(*owner.weights) {}
    at::Tensor conv(const at::Tensor&, const std::string&, int64_t padding = -1, int64_t groups = 1) const;
    at::Tensor linear(const at::Tensor&, const std::string&, bool bias = true) const;
    at::Tensor residual(const at::Tensor&, const std::string&, bool silu = false) const;
    at::Tensor blocks(at::Tensor, const std::string&, int64_t count = 1, bool silu = false) const;
    at::Tensor cmu(at::Tensor, const std::string&, const at::Tensor& initial_mask, int64_t layers, bool experts);
    at::Tensor relative_positions(int64_t frames);
    at::Tensor absolute_positions(int64_t valid, int64_t frames);
    Plan& owner;
    Weights& weights;
private:
    std::map<int64_t, at::Tensor> position_cache;
    at::Tensor conformer(at::Tensor, const std::string&, int64_t layers, bool experts);
    at::Tensor feed_forward(const at::Tensor&, const std::string&, bool experts) const;
    at::Tensor relative_attention(const at::Tensor&, const at::Tensor&, const std::string&) const;
    at::Tensor convolution_module(const at::Tensor&, const std::string&) const;
};
} // namespace uta::torch_native
