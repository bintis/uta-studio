#include <hip/hip_runtime_api.h>
#include <hip/hiprtc.h>
#include <algorithm>
#include <cstdint>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
constexpr unsigned int tile_width = 16;
constexpr const char* projection_source = R"(
extern "C" __global__ void uta_projection_kernel(
    float* output,
    const float* input,
    const float* weight,
    const float* bias,
    long rows,
    long input_channels,
    long output_channels) {
    constexpr int tile_width = 16;
    __shared__ float input_tile[tile_width][tile_width];
    __shared__ float weight_tile[tile_width][tile_width];

    const long row = (long)blockIdx.y * tile_width + threadIdx.y;
    const long output_channel = (long)blockIdx.x * tile_width + threadIdx.x;
    float sum = bias && output_channel < output_channels ? bias[output_channel] : 0.0F;

    for (long begin = 0; begin < input_channels; begin += tile_width) {
        const long input_channel = begin + threadIdx.x;
        input_tile[threadIdx.y][threadIdx.x] = row < rows && input_channel < input_channels
            ? input[row * input_channels + input_channel]
            : 0.0F;

        const long weight_channel = begin + threadIdx.y;
        weight_tile[threadIdx.y][threadIdx.x] = output_channel < output_channels && weight_channel < input_channels
            ? weight[output_channel * input_channels + weight_channel]
            : 0.0F;
        __syncthreads();

#pragma unroll
        for (int channel = 0; channel < tile_width; ++channel)
            sum = __builtin_fmaf(input_tile[threadIdx.y][channel], weight_tile[channel][threadIdx.x], sum);
        __syncthreads();
    }

    if (row < rows && output_channel < output_channels)
        output[row * output_channels + output_channel] = sum;
}
)";

void check_hip(hipError_t status, const char* operation) {
    if (status != hipSuccess)
        throw std::runtime_error(std::string(operation) + ": " + hipGetErrorString(status));
}

struct ProjectionModule {
    hipModule_t module{};
    hipFunction_t function{};

    ProjectionModule() {
        int device = 0;
        check_hip(hipGetDevice(&device), "cannot query ROCm projection device");
        hipDeviceProp_t properties{};
        check_hip(hipGetDeviceProperties(&properties, device), "cannot query ROCm projection architecture");
        std::string architecture = properties.gcnArchName;
        if (const auto suffix = architecture.find(':'); suffix != std::string::npos)
            architecture.erase(suffix);
        const std::string architecture_option = "--gpu-architecture=" + architecture;
        const char* options[]{architecture_option.c_str()};

        hiprtcProgram program{};
        auto status = hiprtcCreateProgram(&program, projection_source, "uta_projection.hip", 0, nullptr, nullptr);
        if (status != HIPRTC_SUCCESS)
            throw std::runtime_error(std::string("cannot create ROCm projection program: ") + hiprtcGetErrorString(status));
        status = hiprtcCompileProgram(program, 1, options);
        if (status != HIPRTC_SUCCESS) {
            size_t log_size = 0;
            hiprtcGetProgramLogSize(program, &log_size);
            std::string log(log_size, '\0');
            if (log_size) hiprtcGetProgramLog(program, log.data());
            hiprtcDestroyProgram(&program);
            throw std::runtime_error(std::string("cannot compile ROCm projection program: ") +
                                     hiprtcGetErrorString(status) + "\n" + log);
        }
        size_t code_size = 0;
        if (hiprtcGetCodeSize(program, &code_size) != HIPRTC_SUCCESS) {
            hiprtcDestroyProgram(&program);
            throw std::runtime_error("cannot measure compiled ROCm projection program");
        }
        std::vector<char> code(code_size);
        if (hiprtcGetCode(program, code.data()) != HIPRTC_SUCCESS) {
            hiprtcDestroyProgram(&program);
            throw std::runtime_error("cannot read compiled ROCm projection program");
        }
        hiprtcDestroyProgram(&program);
        check_hip(hipModuleLoadData(&module, code.data()), "cannot load ROCm projection module");
        check_hip(hipModuleGetFunction(&function, module, "uta_projection_kernel"),
                  "cannot locate ROCm projection kernel");
    }
};

const ProjectionModule& projection_module() {
    static const ProjectionModule value;
    return value;
}
} // namespace

extern "C" __attribute__((visibility("default"))) void uta_libtorch_rocm_projection(
    float* output,
    const float* input,
    const float* weight,
    const float* bias,
    int64_t rows,
    int64_t input_channels,
    int64_t output_channels,
    void* stream_pointer) {
    const auto& kernel = projection_module();
    const auto block_columns = static_cast<unsigned int>((output_channels + tile_width - 1) / tile_width);
    const auto block_rows = static_cast<unsigned int>((rows + tile_width - 1) / tile_width);
    void* arguments[]{&output, &input, &weight, &bias, &rows, &input_channels, &output_channels};
    check_hip(hipModuleLaunchKernel(
        kernel.function,
        block_columns, block_rows, 1,
        tile_width, tile_width, 1,
        0, static_cast<hipStream_t>(stream_pointer), arguments, nullptr),
        "ROCm projection kernel launch failed");
}
