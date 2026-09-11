#pragma once
#include "mixed_attention.hpp"
#include <ATen/ATen.h>
#include <algorithm>
#include <functional>
#include <stdexcept>

namespace uta::torch_native {
// Unmasked noncausal attention for RoFormer. Only independent batches and
// query rows are partitioned; every query retains the complete K/V sequence.
// This is the selected ROCm mixed-attention path, not a CPU or fused-SDPA
// fallback.
inline at::Tensor partitioned_mixed_attention(
    const at::Tensor& query, const at::Tensor& key, const at::Tensor& value,
    double scale, const std::function<void()>& check_cancel,
    int64_t batch_tile = 4, int64_t query_tile = 64) {
    if (query.dim() != 4 || key.dim() != 4 || value.dim() != 4 || batch_tile <= 0 || query_tile <= 0 ||
        query.size(0) != key.size(0) || key.size(0) != value.size(0))
        throw std::invalid_argument("partitioned mixed attention requires compatible batches and positive tile dimensions");
    const auto batches = query.size(0);
    auto output = at::empty({batches, query.size(1), query.size(2), value.size(3)},
                            query.options().dtype(at::kFloat));
    for (int64_t begin = 0; begin < batches; begin += batch_tile) {
        check_cancel();
        const auto count = std::min<int64_t>(batch_tile, batches - begin);
        output.narrow(0, begin, count).copy_(explicit_mixed_attention(
            query.narrow(0, begin, count), key.narrow(0, begin, count),
            value.narrow(0, begin, count), {}, false, false, scale, query_tile));
    }
    return output;
}
} // namespace uta::torch_native
