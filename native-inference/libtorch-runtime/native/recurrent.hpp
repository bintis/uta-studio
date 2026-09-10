#pragma once
#include <ATen/ATen.h>
#include <ATen/ops/_thnn_fused_gru_cell.h>
#include <functional>
#include <stdexcept>
#include <vector>

namespace uta::torch_native {
// Explicit native GPU execution, not a runtime fallback. Input projection is
// batched over the entire sequence; only the recurrent dependency is serial.
// Parameters use ATen r,z,n gate order, with distinct input/recurrent biases.
// The fused cell preserves n=tanh(i_n+r*(h_n+b_hn)), including reverse direction.
inline at::Tensor fused_bidirectional_gru(
    const at::Tensor& input, const at::Tensor& initial,
    const std::vector<at::Tensor>& parameters,
    const std::function<void()>& check_cancel) {
    if ((!input.is_cuda() && !input.is_xpu()) || input.scalar_type() != at::kFloat ||
        input.dim() != 3 || input.size(0) <= 0 || input.size(1) <= 0 || parameters.size() != 8 ||
        initial.dim() != 3 || initial.size(0) != 2 || initial.size(1) != input.size(1) ||
        initial.device() != input.device() || initial.scalar_type() != input.scalar_type())
        throw std::invalid_argument("fused bidirectional GRU requires device F32 [time,batch,input] and [2,batch,hidden]");
    const auto frames = input.size(0), batch = input.size(1), hidden = initial.size(2);
    if (hidden <= 0) throw std::invalid_argument("fused GRU hidden size must be positive");
    std::vector<at::Tensor> directions;
    for (int64_t direction = 0; direction < 2; ++direction) {
        const auto offset = static_cast<std::size_t>(direction * 4);
        const auto& input_weight = parameters[offset];
        const auto& hidden_weight = parameters[offset + 1];
        const auto& input_bias = parameters[offset + 2];
        const auto& hidden_bias = parameters[offset + 3];
        if (input_weight.sizes() != at::IntArrayRef({hidden * 3, input.size(2)}) ||
            hidden_weight.sizes() != at::IntArrayRef({hidden * 3, hidden}) ||
            input_bias.sizes() != at::IntArrayRef({hidden * 3}) ||
            hidden_bias.sizes() != at::IntArrayRef({hidden * 3}))
            throw std::invalid_argument("fused GRU parameter shape mismatch");
        for (std::size_t item = offset; item < offset + 4; ++item)
            if (parameters[item].device() != input.device() || parameters[item].scalar_type() != input.scalar_type())
                throw std::invalid_argument("fused GRU parameters must remain on the input device and dtype");
        check_cancel();
        auto projected = at::linear(input.reshape({frames * batch, input.size(2)}), input_weight)
                             .reshape({frames, batch, hidden * 3});
        auto state = initial.select(0, direction).contiguous();
        std::vector<at::Tensor> timeline(static_cast<std::size_t>(frames));
        for (int64_t step = 0; step < frames; ++step) {
            check_cancel();
            const auto frame = direction == 0 ? step : frames - 1 - step;
            auto recurrent = at::linear(state, hidden_weight);
            state = std::get<0>(at::_thnn_fused_gru_cell(
                projected.select(0, frame), recurrent, state, input_bias, hidden_bias));
            timeline[static_cast<std::size_t>(frame)] = state;
        }
        directions.push_back(at::stack(timeline, 0));
    }
    return at::cat(directions, -1);
}
} // namespace uta::torch_native
