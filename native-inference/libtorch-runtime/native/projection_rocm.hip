#include <hip/hip_runtime.h>
#include <cstdint>
#include <stdexcept>
#include <string>

namespace {
constexpr int tile_width = 16;

__global__ void projection_kernel(
    float* output,
    const float* input,
    const float* weight,
    const float* bias,
    int64_t rows,
    int64_t input_channels,
    int64_t output_channels) {
    __shared__ float input_tile[tile_width][tile_width];
    __shared__ float weight_tile[tile_width][tile_width];

    const auto row = static_cast<int64_t>(blockIdx.y) * tile_width + threadIdx.y;
    const auto output_channel = static_cast<int64_t>(blockIdx.x) * tile_width + threadIdx.x;
    float sum = bias && output_channel < output_channels ? bias[output_channel] : 0.0F;

    for (int64_t begin = 0; begin < input_channels; begin += tile_width) {
        const auto input_channel = begin + threadIdx.x;
        input_tile[threadIdx.y][threadIdx.x] = row < rows && input_channel < input_channels
            ? input[row * input_channels + input_channel]
            : 0.0F;

        const auto weight_channel = begin + threadIdx.y;
        weight_tile[threadIdx.y][threadIdx.x] = output_channel < output_channels && weight_channel < input_channels
            ? weight[output_channel * input_channels + weight_channel]
            : 0.0F;
        __syncthreads();

#pragma unroll
        for (int channel = 0; channel < tile_width; ++channel)
            sum = fmaf(input_tile[threadIdx.y][channel], weight_tile[channel][threadIdx.x], sum);
        __syncthreads();
    }

    if (row < rows && output_channel < output_channels)
        output[row * output_channels + output_channel] = sum;
}
} // namespace

extern "C" void uta_libtorch_rocm_projection(
    float* output,
    const float* input,
    const float* weight,
    const float* bias,
    int64_t rows,
    int64_t input_channels,
    int64_t output_channels,
    void* stream_pointer) {
    const dim3 threads(tile_width, tile_width);
    const dim3 blocks(
        static_cast<unsigned int>((output_channels + tile_width - 1) / tile_width),
        static_cast<unsigned int>((rows + tile_width - 1) / tile_width));
    hipLaunchKernelGGL(
        projection_kernel,
        blocks,
        threads,
        0,
        static_cast<hipStream_t>(stream_pointer),
        output,
        input,
        weight,
        bias,
        rows,
        input_channels,
        output_channels);
    const auto status = hipGetLastError();
    if (status != hipSuccess)
        throw std::runtime_error(std::string("ROCm projection kernel launch failed: ") + hipGetErrorString(status));
}
