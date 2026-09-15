#pragma once
#include <ATen/ATen.h>
#include <algorithm>
#include <cstdint>
#include <stdexcept>

namespace uta::torch_native {
// Host-side GGUF decoding is not model inference. Convert only a bounded host
// slice to the unchanged native FP32/INT64 storage type, then copy matching
// dtypes. This avoids handing a whole vocabulary tensor to a combined device
// transfer/conversion and keeps every staging slice alive through completion.
// The complete callback must wait on the selected device before returning.
template <class Complete>
at::Tensor upload_weight_in_tiles(const at::Tensor& stored, const at::Device& device,
                                   Complete complete, int64_t tile_bytes = 4 * 1024 * 1024) {
    if (!stored.device().is_cpu() || !stored.is_contiguous() || tile_bytes <= 0)
        throw std::invalid_argument("weight upload requires contiguous host storage and a positive tile size");
    const auto dtype = stored.is_floating_point() ? at::kFloat : at::kLong;
    const int64_t element_bytes = dtype == at::kFloat ? sizeof(float) : sizeof(int64_t);
    if (tile_bytes < element_bytes)
        throw std::invalid_argument("weight upload tile cannot hold one native element");
    auto result = at::empty(stored.sizes(), stored.options().device(device).dtype(dtype));
    auto source = stored.reshape({-1});
    auto destination = result.reshape({-1});
    const auto elements = stored.numel();
    const int64_t tile_elements = tile_bytes / element_bytes;
    for (int64_t begin = 0; begin < elements;) {
        const auto count = std::min(tile_elements, elements - begin);
        // No device change here: FP16/BF16 expansion happens on the CPU.
        auto staging = source.narrow(0, begin, count).to(dtype);
        destination.narrow(0, begin, count).copy_(staging, false);
        complete(begin + count, elements);
        begin += count;
    }
    return result;
}
} // namespace uta::torch_native
