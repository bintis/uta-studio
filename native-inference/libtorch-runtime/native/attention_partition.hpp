#pragma once
#include <ATen/ATen.h>
#include <algorithm>
#include <functional>
#include <stdexcept>
#include <vector>

namespace uta::torch_native {
// Unmasked noncausal attention for RoFormer. Only independent batches and
// query rows are partitioned. Each query still attends to the complete K/V
// sequence. Small contiguous FP16 packs bound individual GPU launch shapes.
// The caller explicitly chooses mixed attention and disables math SDPA.
inline at::Tensor partitioned_fused_attention(
    const at::Tensor& query, const at::Tensor& key, const at::Tensor& value,
    double scale, const std::function<void()>& check_cancel,
    int64_t batch_tile = 4, int64_t query_tile = 128) {
    if (query.dim() != 4 || key.dim() != 4 || value.dim() != 4 || batch_tile <= 0 || query_tile <= 0 ||
        query.size(0) != key.size(0) || key.size(0) != value.size(0) ||
        query.size(1) != key.size(1) || key.size(1) != value.size(1) ||
        query.size(3) != key.size(3) || key.size(2) != value.size(2) ||
        query.device() != key.device() || key.device() != value.device() ||
        (!query.is_cuda() && !query.is_xpu()))
        throw std::invalid_argument("partitioned fused attention requires compatible GPU [batch,head,row,channel] tensors");
    for (const auto& tensor : {query, key, value})
        for (auto dimension : tensor.sizes())
            if (dimension <= 0) throw std::invalid_argument("partitioned attention dimensions must be positive");
    const auto batches = query.size(0), rows = query.size(2);
    auto output = at::empty({batches, query.size(1), rows, value.size(3)}, query.options().dtype(at::kFloat));
    for (int64_t begin = 0; begin < batches; begin += batch_tile) {
        check_cancel();
        const auto count = std::min<int64_t>(batch_tile, batches - begin);
        auto keys = key.narrow(0, begin, count).to(at::kHalf).contiguous();
        auto values = value.narrow(0, begin, count).to(at::kHalf).contiguous();
        for (int64_t start = 0; start < rows; start += query_tile) {
            check_cancel();
            const auto length = std::min<int64_t>(query_tile, rows - start);
            auto queries = query.narrow(0, begin, count).narrow(2, start, length).to(at::kHalf).contiguous();
            auto attended = at::scaled_dot_product_attention(queries, keys, values, {}, 0.0, false, scale, false);
            output.narrow(0, begin, count).narrow(2, start, length).copy_(attended.to(at::kFloat));
        }
    }
    return output;
}
} // namespace uta::torch_native
