#pragma once
#include <ATen/ATen.h>
#include <ATen/ops/im2col.h>
#include <algorithm>
#include <functional>
#include <limits>
#include <stdexcept>
#include <vector>

namespace uta::torch_native {
// NCHW, unit dilation and one group. Explicit GPU im2col/shared-weight GEMM
// avoids MIOpen runtime compilation for the Qwen acoustic frontend. Bound the
// batch tile's column workspace; a single example is the minimum viable tile.
inline at::Tensor projected_convolution(
    const at::Tensor& input, const at::Tensor& kernel, const at::Tensor& bias,
    at::IntArrayRef stride, at::IntArrayRef padding,
    const std::function<void()>& check_cancel,
    int64_t workspace_bytes = 64 * 1024 * 1024) {
    if ((!input.is_cuda() && !input.is_xpu()) || input.dim() != 4 || kernel.dim() != 4 ||
        input.scalar_type() != at::kFloat || kernel.scalar_type() != at::kFloat ||
        input.device() != kernel.device() || input.size(1) != kernel.size(1) ||
        stride.size() != 2 || padding.size() != 2 || stride[0] <= 0 || stride[1] <= 0 ||
        padding[0] < 0 || padding[1] < 0 || workspace_bytes <= 0)
        throw std::invalid_argument("projected convolution requires compatible device F32 NCHW input and OIHW kernel");
    for (auto dimension : input.sizes())
        if (dimension <= 0) throw std::invalid_argument("projected convolution input dimensions must be positive");
    for (auto dimension : kernel.sizes())
        if (dimension <= 0) throw std::invalid_argument("projected convolution kernel dimensions must be positive");
    if (bias.defined() && (bias.sizes() != at::IntArrayRef({kernel.size(0)}) ||
        bias.device() != input.device() || bias.scalar_type() != input.scalar_type()))
        throw std::invalid_argument("projected convolution bias shape, device or dtype mismatch");
    const auto vertical = input.size(2) + 2 * padding[0] - kernel.size(2);
    const auto horizontal = input.size(3) + 2 * padding[1] - kernel.size(3);
    if (vertical < 0 || horizontal < 0) throw std::invalid_argument("projected convolution kernel exceeds padded input");
    const auto height = vertical / stride[0] + 1, width = horizontal / stride[1] + 1;
    const auto channels = kernel.size(0), features = kernel.numel() / channels;
    const auto checked_product = [](int64_t left, int64_t right) {
        if (left > std::numeric_limits<int64_t>::max() / right)
            throw std::invalid_argument("projected convolution workspace dimensions overflow");
        return left * right;
    };
    const auto locations = checked_product(height, width);
    const auto per_example = checked_product(checked_product(features, locations), input.element_size());
    const auto tile = std::max<int64_t>(1, std::min<int64_t>(input.size(0), workspace_bytes / per_example));
    auto weights = kernel.reshape({channels, features});
    std::vector<at::Tensor> outputs;
    for (int64_t begin = 0; begin < input.size(0); begin += tile) {
        check_cancel();
        const auto batch = std::min<int64_t>(tile, input.size(0) - begin);
        auto columns = at::im2col(input.narrow(0, begin, batch),
            {kernel.size(2), kernel.size(3)}, {1, 1}, padding, stride);
        auto matrix = columns.transpose(1, 2).contiguous().reshape({batch * locations, features});
        auto projected = at::linear(matrix, weights, bias);
        outputs.push_back(projected.reshape({batch, height, width, channels}).permute({0, 3, 1, 2}).contiguous());
    }
    return outputs.size() == 1 ? outputs.front() : at::cat(outputs, 0);
}
} // namespace uta::torch_native
