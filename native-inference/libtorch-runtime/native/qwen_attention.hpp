#pragma once
#include <ATen/ATen.h>
#include <algorithm>
#include <stdexcept>

namespace uta::torch_native {
// Qwen's encoder mask is block diagonal. Execute those exact acoustic windows
// rather than asking a fused kernel to process the complete masked timeline.
// No audio rows, positions, or visible keys are removed, including the last
// partial window. Completion happens while the tile and its output are alive.
template <class Attend, class Check, class Complete>
at::Tensor qwen_window_attention(const at::Tensor& query, const at::Tensor& key,
                                 const at::Tensor& value, int64_t window_rows,
                                 Attend attend, Check check, Complete complete) {
    if (query.dim() != 4 || key.dim() != 4 || value.dim() != 4 || window_rows <= 0 ||
        query.size(2) <= 0 || query.size(2) != key.size(2) || key.size(2) != value.size(2))
        throw std::invalid_argument("Qwen encoder attention requires matching nonempty timelines and a positive acoustic window");
    const auto rows = query.size(2);
    auto output = at::empty({query.size(0), query.size(1), rows, value.size(3)}, query.options().dtype(at::kFloat));
    for (int64_t begin = 0; begin < rows;) {
        check();
        const auto count = std::min(window_rows, rows - begin);
        auto attended = attend(query.narrow(2, begin, count), key.narrow(2, begin, count),
                               value.narrow(2, begin, count), at::Tensor());
        output.narrow(2, begin, count).copy_(attended);
        complete();
        begin += count;
    }
    return output;
}

// Tile only the decoder's query dimension. Every query retains all of its
// causal K/V history; the mask uses the absolute resident-cache offset, not
// tile-local positions. Never materialize a prompt_rows x cache_rows mask.
template <class Attend, class Check, class Complete>
at::Tensor qwen_causal_attention(const at::Tensor& query, const at::Tensor& key,
                                 const at::Tensor& value, int64_t past,
                                 Attend attend, Check check, Complete complete) {
    if (query.dim() != 4 || key.dim() != 4 || value.dim() != 4 || past < 0 ||
        query.size(2) <= 0 || key.size(2) < query.size(2) ||
        key.size(2) - query.size(2) != past || key.size(2) != value.size(2))
        throw std::invalid_argument("Qwen decoder attention timeline disagrees with its resident cache position");
    constexpr int64_t query_tile = 64;
    const auto rows = query.size(2);
    auto output = at::empty({query.size(0), query.size(1), rows, value.size(3)}, query.options().dtype(at::kFloat));
    for (int64_t begin = 0; begin < rows;) {
        check();
        const auto count = std::min(query_tile, rows - begin);
        const auto visible = past + begin + count;
        auto positions = at::arange(past + begin, visible, query.options().dtype(at::kLong));
        auto columns = at::arange(visible, query.options().dtype(at::kLong));
        auto mask = positions.unsqueeze(1) >= columns.unsqueeze(0);
        auto attended = attend(query.narrow(2, begin, count), key.narrow(2, 0, visible),
                               value.narrow(2, 0, visible), mask);
        output.narrow(2, begin, count).copy_(attended);
        complete();
        begin += count;
    }
    return output;
}
} // namespace uta::torch_native
