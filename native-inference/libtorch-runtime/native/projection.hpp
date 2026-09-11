#pragma once
#include <ATen/ATen.h>
#include <algorithm>
#include <functional>
#include <stdexcept>
#include <vector>

namespace uta::torch_native {
// Same shared-weight linear map, with bounded GEMM row count on the selected
// GPU. Leading batch/sequence axes are flattened, never split semantically.
// The output is allocated once; all row tiles including the tail remain on GPU.
inline at::Tensor tiled_projection(const at::Tensor& input, const at::Tensor& weight,
                                  const at::Tensor& bias, const std::function<void()>& check_cancel,
                                  int64_t row_tile = 1024) {
    if (input.dim() < 2 || weight.dim() != 2 || input.size(-1) <= 0 || weight.size(0) <= 0 ||
        input.size(-1) != weight.size(1) || input.scalar_type() != at::kFloat ||
        weight.scalar_type() != input.scalar_type() || weight.device() != input.device() ||
        (!input.is_cuda() && !input.is_xpu()) || row_tile <= 0)
        throw std::invalid_argument("tiled projection requires compatible GPU F32 input and matrix weights");
    if (bias.defined() && (bias.sizes() != at::IntArrayRef({weight.size(0)}) ||
        bias.device() != input.device() || bias.scalar_type() != input.scalar_type()))
        throw std::invalid_argument("tiled projection bias shape, device or dtype mismatch");
    const auto width = input.size(-1), rows = input.numel() / width;
    auto shape = input.sizes().vec();
    shape.back() = weight.size(0);
    auto matrix = input.reshape({rows, width});
    auto output = at::empty({rows, weight.size(0)}, input.options());
    for (int64_t start = 0; start < rows; start += row_tile) {
        check_cancel();
        const auto count = std::min<int64_t>(row_tile, rows - start);
        output.narrow(0, start, count).copy_(at::linear(matrix.narrow(0, start, count), weight, bias));
    }
    return output.reshape(shape);
}

// RoFormer feed-forward projections can have a much wider hidden dimension
// than their resident sequence. Keep only one hidden row tile live while the
// complete output remains on the selected GPU.
inline at::Tensor tiled_feed_forward(
    const at::Tensor& input, const at::Tensor& input_weight, const at::Tensor& input_bias,
    const at::Tensor& output_weight, const at::Tensor& output_bias,
    const std::function<void()>& check_cancel, int64_t row_tile = 1024) {
    if (input.dim() < 2 || input_weight.dim() != 2 || output_weight.dim() != 2 ||
        input.size(-1) <= 0 || input_weight.size(0) <= 0 || output_weight.size(0) <= 0 ||
        input.size(-1) != input_weight.size(1) || input_weight.size(0) != output_weight.size(1) ||
        input.scalar_type() != at::kFloat || input_weight.scalar_type() != input.scalar_type() ||
        output_weight.scalar_type() != input.scalar_type() || input_weight.device() != input.device() ||
        output_weight.device() != input.device() || (!input.is_cuda() && !input.is_xpu()) || row_tile <= 0)
        throw std::invalid_argument("tiled feed-forward requires compatible GPU F32 input and matrix weights");
    const auto compatible_bias = [&](const at::Tensor& bias, int64_t channels) {
        return !bias.defined() || (bias.sizes() == at::IntArrayRef({channels}) &&
            bias.device() == input.device() && bias.scalar_type() == input.scalar_type());
    };
    if (!compatible_bias(input_bias, input_weight.size(0)) || !compatible_bias(output_bias, output_weight.size(0)))
        throw std::invalid_argument("tiled feed-forward bias shape, device or dtype mismatch");
    const auto width = input.size(-1), rows = input.numel() / width;
    auto shape = input.sizes().vec();
    shape.back() = output_weight.size(0);
    auto matrix = input.reshape({rows, width});
    auto output = at::empty({rows, output_weight.size(0)}, input.options());
    for (int64_t start = 0; start < rows; start += row_tile) {
        check_cancel();
        const auto count = std::min<int64_t>(row_tile, rows - start);
        auto hidden = at::gelu(at::linear(matrix.narrow(0, start, count), input_weight, input_bias), "none");
        output.narrow(0, start, count).copy_(at::linear(hidden, output_weight, output_bias));
    }
    return output.reshape(shape);
}
} // namespace uta::torch_native
