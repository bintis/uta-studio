#pragma once
#include <ATen/ATen.h>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <stdexcept>
#include <utility>

namespace uta::torch_native {
// Schedule scratch, not context: even the smallest tile retains a complete
// softmax row. GQA shares each physical KV head instead of repeating the cache.
inline int64_t qwen_strict_query_tile(int64_t heads_per_key, int64_t keys) {
    if (heads_per_key <= 0 || keys <= 0)
        throw std::invalid_argument("Qwen strict attention requires positive groups and key rows");
    constexpr int64_t score_elements = 4 * 1024 * 1024;
    return std::max<int64_t>(1, std::min<int64_t>(64, score_elements / heads_per_key / keys));
}

// The XPU strict route deliberately uses two-dimensional FP32 mm, not a
// broadcasted four-dimensional matmul or fused SDPA. Pack one physical KV head,
// flatten its independent query heads into rows, and retain every visible key.
// Each operation's result stays alive until Complete returns. Check/Begin/
// Complete are also used by the CPU-only production-helper oracle.
template <class Check, class Begin, class Complete>
at::Tensor qwen_strict_attention(const at::Tensor& query, const at::Tensor& key,
                                const at::Tensor& value, const at::Tensor& mask,
                                Check check, Begin begin_step, Complete complete_step) {
    if (query.dim() != 4 || key.dim() != 4 || value.dim() != 4 ||
        query.size(0) != key.size(0) || key.size(0) != value.size(0) ||
        key.size(1) != value.size(1) || query.size(3) != key.size(3) ||
        key.size(2) != value.size(2) ||
        query.device() != key.device() || key.device() != value.device() ||
        query.scalar_type() != at::kFloat || key.scalar_type() != at::kFloat || value.scalar_type() != at::kFloat)
        throw std::invalid_argument("Qwen strict attention requires compatible FP32 [batch,head,row,channel] tensors");
    for (const auto& tensor : {query, key, value})
        for (auto dimension : tensor.sizes())
            if (dimension <= 0) throw std::invalid_argument("Qwen strict attention dimensions must be positive");
    if (query.size(1) % key.size(1))
        throw std::invalid_argument("Qwen query heads must be divisible by KV heads");
    const auto batches = query.size(0), heads = query.size(1), rows = query.size(2), keys = key.size(2);
    const auto heads_per_key = heads / key.size(1);
    const auto tile = qwen_strict_query_tile(heads_per_key, keys);
    const auto scale = 1.0 / std::sqrt(static_cast<double>(query.size(3)));
    // Expansion is a read-only view. Only the current group's query rows are
    // applied below; never allocate a repeated KV tensor or a global mask copy.
    auto expanded_mask = mask.defined() ? mask.expand({batches, heads, rows, keys}) : at::Tensor();
    const auto run = [&](const char* stage, int64_t batch, int64_t head,
                         int64_t row, int64_t count, auto operation) {
        check();
        begin_step(stage, batch, head, row, count);
        auto result = operation();
        complete_step(stage, batch, head, row, count);
        return result;
    };
    auto output = run("output_allocate", -1, -1, 0, rows, [&] {
        return at::empty({batches, heads, rows, value.size(3)}, query.options());
    });
    for (int64_t batch = 0; batch < batches; ++batch) {
        for (int64_t head = 0; head < key.size(1); ++head) {
            const auto first_head = head * heads_per_key;
            auto packed = run("kv_pack", batch, head, 0, keys, [&] {
                return std::make_pair(key.select(0, batch).select(0, head).contiguous(),
                                      value.select(0, batch).select(0, head).contiguous());
            });
            for (int64_t row = 0; row < rows;) {
                const auto count = std::min(tile, rows - row);
                auto queries = run("query_pack", batch, head, row, count, [&] {
                    return query.select(0, batch).narrow(0, first_head, heads_per_key)
                        .narrow(1, row, count).contiguous().view({heads_per_key * count, query.size(3)});
                });
                auto scores = run("scores", batch, head, row, count, [&] {
                    auto product = at::mm(queries, packed.first.transpose(0, 1))
                        .view({heads_per_key, count, keys});
                    product.mul_(scale);
                    return product;
                });
                auto probabilities = run("softmax", batch, head, row, count, [&] {
                    if (expanded_mask.defined()) {
                        auto piece = expanded_mask.select(0, batch).narrow(0, first_head, heads_per_key)
                            .narrow(1, row, count);
                        if (piece.scalar_type() == at::kBool)
                            scores.masked_fill_(piece.logical_not(), -std::numeric_limits<float>::infinity());
                        else scores.add_(piece.to(at::kFloat));
                    }
                    auto normalized = at::softmax(scores, -1);
                    // Preserve strict attention's all-masked-row zero result;
                    // do not hide NaNs or infinities in other arithmetic.
                    return at::where(at::isneginf(scores).all(-1, true), at::zeros_like(normalized), normalized);
                });
                auto attended = run("values", batch, head, row, count, [&] {
                    auto product = at::mm(probabilities.reshape({heads_per_key * count, keys}), packed.second)
                        .view({heads_per_key, count, value.size(3)});
                    output.select(0, batch).narrow(0, first_head, heads_per_key).narrow(1, row, count).copy_(product);
                    return product;
                });
                // Keep the final producer through its completion, including an
                // aliased input pack and the final single-token decode tile.
                (void)attended;
                row += count;
            }
        }
    }
    return output;
}
} // namespace uta::torch_native
