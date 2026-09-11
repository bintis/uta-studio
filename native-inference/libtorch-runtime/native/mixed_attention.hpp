#pragma once
#include <ATen/ATen.h>
#include <algorithm>
#include <cmath>
#include <limits>
#include <stdexcept>

namespace uta::torch_native {
// Explicit GPU mixed attention for ROCm devices where packaged fused SDPA is
// experimental. Q/K/V are rounded through FP16, contractions and softmax stay
// FP32, and each output tile is rounded through FP16. Query tiling bounds the
// score workspace without reducing any key/value context.
inline at::Tensor explicit_mixed_attention(
    const at::Tensor& query, const at::Tensor& key, const at::Tensor& value,
    const at::Tensor& mask = {}, bool causal = false, bool grouped = false,
    double scale = 0.0, int64_t query_tile = 64) {
    if (query.dim() != 4 || key.dim() != 4 || value.dim() != 4 || query_tile <= 0 ||
        query.size(0) != key.size(0) || key.size(0) != value.size(0) ||
        query.size(3) != key.size(3) || key.size(2) != value.size(2) ||
        query.device() != key.device() || key.device() != value.device() ||
        (!query.is_cuda() && !query.is_xpu()))
        throw std::invalid_argument("explicit mixed attention requires compatible GPU [batch,head,row,channel] tensors");
    for (const auto& tensor : {query, key, value})
        for (auto dimension : tensor.sizes())
            if (dimension <= 0) throw std::invalid_argument("explicit mixed attention dimensions must be positive");
    if (mask.defined() && mask.device() != query.device())
        throw std::invalid_argument("explicit mixed attention mask must use the selected GPU");
    const auto query_heads = query.size(1), key_heads = key.size(1);
    if (query_heads % key_heads || (query_heads != key_heads && !grouped))
        throw std::invalid_argument("explicit mixed attention grouped-query heads are incompatible");
    if (scale == 0.0) scale = 1.0 / std::sqrt(static_cast<double>(query.size(3)));

    auto rounded_key = key.to(at::kHalf).to(at::kFloat).contiguous();
    auto rounded_value = value.to(at::kHalf).to(at::kFloat).contiguous();
    if (query_heads != key_heads) {
        rounded_key = at::repeat_interleave(rounded_key, query_heads / key_heads, 1);
        rounded_value = at::repeat_interleave(rounded_value, query_heads / key_heads, 1);
    }
    const auto rows = query.size(2), keys = key.size(2);
    auto output = at::empty({query.size(0), query_heads, rows, value.size(3)}, query.options().dtype(at::kFloat));
    const auto key_columns = causal
        ? at::arange(keys, query.options().dtype(at::kLong)).unsqueeze(0)
        : at::Tensor();
    for (int64_t begin = 0; begin < rows; begin += query_tile) {
        const auto count = std::min<int64_t>(query_tile, rows - begin);
        auto rounded_query = query.narrow(2, begin, count).to(at::kHalf).to(at::kFloat).contiguous();
        auto scores = at::matmul(rounded_query, rounded_key.transpose(-1, -2)) * scale;
        if (mask.defined()) {
            auto piece = mask.dim() >= 2 && mask.size(-2) == rows ? mask.narrow(-2, begin, count) : mask;
            if (piece.scalar_type() == at::kBool)
                scores = scores.masked_fill(piece.logical_not(), -std::numeric_limits<float>::infinity());
            else
                scores = scores + piece.to(at::kFloat);
        }
        if (causal) {
            auto query_rows = at::arange(begin, begin + count, query.options().dtype(at::kLong)).unsqueeze(-1);
            scores = scores.masked_fill(key_columns > query_rows, -std::numeric_limits<float>::infinity());
        }
        auto probabilities = at::softmax(scores, -1);
        probabilities = at::where(at::isneginf(scores).all(-1, true), at::zeros_like(probabilities), probabilities);
        auto attended = at::matmul(probabilities, rounded_value).to(at::kHalf).to(at::kFloat);
        output.narrow(2, begin, count).copy_(attended);
    }
    return output;
}
} // namespace uta::torch_native
