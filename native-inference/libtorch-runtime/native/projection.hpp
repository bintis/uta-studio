#pragma once
#include <ATen/ATen.h>
#include <algorithm>
#include <functional>
#include <stdexcept>
#include <vector>
#if defined(UTA_LIBTORCH_ROCM)
#include <c10/hip/HIPStream.h>
extern "C" void uta_libtorch_rocm_projection(
    float* output, int64_t output_row_stride, int64_t output_channel_stride,
    const float* input, int64_t input_row_stride, int64_t input_channel_stride,
    const float* weight, int64_t weight_row_stride, int64_t weight_channel_stride,
    const float* bias, int64_t bias_stride,
    int64_t rows, int64_t input_channels, int64_t output_channels, void* stream_pointer);
extern "C" void uta_libtorch_rocm_batched_projection(
    float* output, int64_t output_group_stride, int64_t output_row_stride, int64_t output_channel_stride,
    const float* input, int64_t input_group_stride, int64_t input_row_stride, int64_t input_channel_stride,
    const float* weight, int64_t weight_group_stride, int64_t weight_row_stride, int64_t weight_channel_stride,
    int64_t groups, int64_t rows, int64_t input_channels, int64_t output_channels, void* stream_pointer);
#endif

namespace uta::torch_native {
#if defined(UTA_LIBTORCH_ROCM)
inline void rocm_batched_projection_out(
    at::Tensor& output, const at::Tensor& input, const at::Tensor& weight,
    int64_t weight_row_stride, int64_t weight_channel_stride) {
    if (output.dim() != 4 || input.dim() != 4 || weight.dim() != 4 ||
        output.size(0) != input.size(0) || input.size(0) != weight.size(0) ||
        output.size(1) != input.size(1) || input.size(1) != weight.size(1) ||
        output.size(2) != input.size(2) || output.scalar_type() != at::kFloat ||
        input.scalar_type() != at::kFloat || weight.scalar_type() != at::kFloat ||
        output.device() != input.device() || input.device() != weight.device() || !input.is_cuda() ||
        (input.size(0) > 1 && input.stride(0) != input.size(1) * input.stride(1)) ||
        (weight.size(0) > 1 && weight.stride(0) != weight.size(1) * weight.stride(1)) ||
        (output.size(0) > 1 && output.stride(0) != output.size(1) * output.stride(1)))
        throw std::invalid_argument("ROCm batched projection requires compatible regular GPU F32 groups");
    const auto stream = c10::cuda::getCurrentCUDAStream(input.get_device()).stream();
    uta_libtorch_rocm_batched_projection(
        output.data_ptr<float>(), output.stride(1), output.stride(2), output.stride(3),
        input.const_data_ptr<float>(), input.stride(1), input.stride(2), input.stride(3),
        weight.const_data_ptr<float>(), weight.stride(1), weight_row_stride, weight_channel_stride,
        input.size(0) * input.size(1), input.size(2), input.size(3), output.size(3),
        reinterpret_cast<void*>(stream));
}
#endif

// Cap the contraction work submitted by one GEMM as well as its row count.
// This scheduling bound is derived from matrix dimensions and does not alter
// the shared weights or split a row's contraction.
inline int64_t bounded_projection_row_tile(const at::Tensor& weight) {
    if (weight.dim() != 2 || weight.size(0) <= 0 || weight.size(1) <= 0)
        throw std::invalid_argument("bounded projection requires a nonempty matrix weight");
    // Real gfx1103 execution requires small, synchronized dispatches even
    // when total contraction work would permit a larger GEMM. Every row and
    // every reduction channel remains present in the resulting tiles.
    constexpr int64_t maximum_rows = 256;
    constexpr int64_t maximum_multiply_accumulates = 256LL * 384 * 1536;
    if (weight.size(0) > maximum_multiply_accumulates / weight.size(1)) return 1;
    const auto work_per_row = weight.size(0) * weight.size(1);
    return std::max<int64_t>(1, std::min<int64_t>(maximum_rows, maximum_multiply_accumulates / work_per_row));
}

inline int64_t bounded_projection_row_tile(const at::Tensor& input, const at::Tensor& weight) {
    const auto row_tile = bounded_projection_row_tile(weight);
    if (input.dim() <= 2 || input.size(-1) <= 0 || input.size(-2) <= 0) return row_tile;
    const auto rows = input.numel() / input.size(-1);
    const auto sequence_rows = input.size(-2);
    // Keep a complete independent sequence together when it fits in the work
    // bound instead of making a flattened tile straddle adjacent batches.
    return rows > row_tile && sequence_rows <= row_tile ? sequence_rows : row_tile;
}

// Write directly into an already allocated contiguous row slice. Avoiding an
// asynchronous temporary linear result plus copy keeps its allocator lifetime
// out of the queued ROCm projection path.
inline void projection_out(at::Tensor& output, const at::Tensor& input,
                           const at::Tensor& weight, const at::Tensor& bias) {
#if defined(UTA_LIBTORCH_ROCM)
    const auto stream = c10::cuda::getCurrentCUDAStream(input.get_device()).stream();
    uta_libtorch_rocm_projection(
        output.data_ptr<float>(), output.stride(0), output.stride(1),
        input.const_data_ptr<float>(), input.stride(0), input.stride(1),
        weight.const_data_ptr<float>(), weight.stride(0), weight.stride(1),
        bias.defined() ? bias.const_data_ptr<float>() : nullptr, bias.defined() ? bias.stride(0) : 0,
        input.size(0), input.size(1), weight.size(0), reinterpret_cast<void*>(stream));
#else
    constexpr int64_t reduction_tile = 128;
    const auto width = input.size(1);
    if (width <= reduction_tile) {
        if (bias.defined())
            at::addmm_out(output, bias, input, weight.transpose(0, 1));
        else
            at::mm_out(output, input, weight.transpose(0, 1));
        return;
    }
    for (int64_t begin = 0; begin < width; begin += reduction_tile) {
        const auto count = std::min<int64_t>(reduction_tile, width - begin);
        const auto input_columns = input.narrow(1, begin, count);
        const auto weight_columns = weight.narrow(1, begin, count).transpose(0, 1);
        if (begin == 0) {
            if (bias.defined())
                at::addmm_out(output, bias, input_columns, weight_columns);
            else
                at::mm_out(output, input_columns, weight_columns);
        } else {
            at::addmm_out(output, output, input_columns, weight_columns);
        }
    }
#endif
}

// Same shared-weight linear map, writing into caller-owned contiguous storage.
// Leading batch/sequence axes are flattened, never split semantically.
inline void tiled_projection_into(
    at::Tensor output, const at::Tensor& input, const at::Tensor& weight,
    const at::Tensor& bias, const std::function<void()>& check_cancel,
    int64_t row_tile = 1024,
    const std::function<void(int64_t, int64_t)>& checkpoint_tile = {}) {
    if (input.dim() < 2 || weight.dim() != 2 || input.size(-1) <= 0 || weight.size(0) <= 0 ||
        input.size(-1) != weight.size(1) || input.scalar_type() != at::kFloat ||
        weight.scalar_type() != input.scalar_type() || weight.device() != input.device() ||
        (!input.is_cuda() && !input.is_xpu()) || row_tile <= 0)
        throw std::invalid_argument("tiled projection requires compatible GPU F32 input and matrix weights");
    if (bias.defined() && (bias.sizes() != at::IntArrayRef({weight.size(0)}) ||
        bias.device() != input.device() || bias.scalar_type() != input.scalar_type()))
        throw std::invalid_argument("tiled projection bias shape, device or dtype mismatch");
    auto shape = input.sizes().vec();
    shape.back() = weight.size(0);
    if (output.sizes() != at::IntArrayRef(shape) || output.device() != input.device() ||
        output.scalar_type() != input.scalar_type() || !output.is_contiguous())
        throw std::invalid_argument("tiled projection output must be a compatible contiguous destination");
    const auto width = input.size(-1), rows = input.numel() / width;
    auto matrix = input.reshape({rows, width});
    auto output_matrix = output.reshape({rows, weight.size(0)});
    for (int64_t start = 0; start < rows; start += row_tile) {
        check_cancel();
        const auto count = std::min<int64_t>(row_tile, rows - start);
        auto output_rows = output_matrix.narrow(0, start, count);
        projection_out(output_rows, matrix.narrow(0, start, count), weight, bias);
        if (checkpoint_tile) checkpoint_tile(start, count);
    }
}

// Allocating convenience wrapper for callers that consume the complete output.
inline at::Tensor tiled_projection(const at::Tensor& input, const at::Tensor& weight,
                                  const at::Tensor& bias, const std::function<void()>& check_cancel,
                                  int64_t row_tile = 1024,
                                  const std::function<void(int64_t, int64_t)>& checkpoint_tile = {}) {
    if (input.dim() < 2 || weight.dim() != 2)
        throw std::invalid_argument("tiled projection requires matrix-shaped input and weight");
    auto shape = input.sizes().vec();
    shape.back() = weight.size(0);
    auto output = at::empty(shape, input.options());
    tiled_projection_into(output, input, weight, bias, check_cancel, row_tile, checkpoint_tile);
    return output;
}

// RoFormer feed-forward projections can have a much wider hidden dimension
// than their resident sequence. Keep only one hidden row tile live while the
// complete output remains on the selected GPU.
inline at::Tensor tiled_feed_forward(
    const at::Tensor& input, const at::Tensor& input_weight, const at::Tensor& input_bias,
    const at::Tensor& output_weight, const at::Tensor& output_bias,
    const std::function<void()>& check_cancel, int64_t row_tile = 1024,
    const std::function<void(int64_t, int64_t)>& checkpoint_tile = {}) {
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
        const auto input_rows = matrix.narrow(0, start, count);
        auto hidden = at::empty({count, input_weight.size(0)}, input.options());
        projection_out(hidden, input_rows, input_weight, input_bias);
        hidden = at::gelu(hidden, "none");
        auto output_rows = output.narrow(0, start, count);
        projection_out(output_rows, hidden, output_weight, output_bias);
        if (checkpoint_tile) checkpoint_tile(start, count);
    }
    return output.reshape(shape);
}
} // namespace uta::torch_native
