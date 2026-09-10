#include "convolution_projection.hpp"
#include <ATen/Context.h>
#include <ATen/Parallel.h>
#include <c10/core/InferenceMode.h>
#include <array>
#include <iomanip>
#include <iostream>

namespace {
at::Tensor fixture(at::IntArrayRef shape, double phase) {
    int64_t count = 1;
    for (auto dimension : shape) count *= dimension;
    return (at::sin(at::arange(count, at::kDouble) * 0.037 + phase) * 0.025).reshape(shape).to(at::kFloat);
}
struct Case {
    int64_t batch, channels, height, width, outputs, kernel_height, kernel_width;
    std::array<int64_t, 2> stride, padding;
    int64_t workspace;
};
}
int main(int argc, char** argv) {
    try {
        if (argc != 2 || std::string(argv[1]) != "rocm" || !at::globalContext().hasROCM())
            throw std::invalid_argument("usage: uta-libtorch-convolution-check rocm (requires native ROCm LibTorch)");
        const auto device = at::Device(at::kCUDA, 0);
        c10::InferenceMode inference;
        at::set_num_threads(2);
        at::globalContext().setFloat32Precision(at::Float32Backend::GENERIC, at::Float32Op::ALL, at::Float32Precision::IEEE);
        const std::vector<Case> cases{
            {2,3,11,13,5,3,3,{2,2},{1,1},64*1024*1024},
            {9,8,16,25,13,2,4,{1,2},{0,1},4096},
            {30,480,64,50,480,3,3,{2,2},{1,1},64*1024*1024},
        };
        for (const auto& test : cases) {
            auto input = fixture({test.batch,test.channels,test.height,test.width}, 0.31);
            auto kernel = fixture({test.outputs,test.channels,test.kernel_height,test.kernel_width}, 0.97);
            auto bias = fixture({test.outputs}, 1.37);
            auto expected = at::conv2d(input.to(at::kDouble), kernel.to(at::kDouble), bias.to(at::kDouble), test.stride, test.padding);
            auto device_input = input.to(device), device_kernel = kernel.to(device), device_bias = bias.to(device);
            auto actual = uta::torch_native::projected_convolution(device_input, device_kernel, device_bias,
                test.stride, test.padding, [] {}, test.workspace);
            if (actual.device() != device) throw std::runtime_error("convolution changed device");
            actual = actual.to(at::kCPU).to(at::kDouble);
            const auto difference = actual - expected;
            const auto nmse = difference.square().sum().item<double>() / std::max(expected.square().sum().item<double>(), 1e-30);
            const auto maximum = difference.abs().max().item<double>();
            const bool passed = at::isfinite(actual).all().item<bool>() && actual.sizes() == expected.sizes() && nmse <= 1e-10 && maximum <= 5e-5;
            std::cout << std::setprecision(12) << "{\"event\":\"projected_convolution_check\",\"backend\":\"rocm\",\"batch\":"
                      << test.batch << ",\"input_channels\":" << test.channels << ",\"compared_elements\":" << expected.numel()
                      << ",\"nmse\":" << nmse << ",\"maximum_absolute_error\":" << maximum
                      << ",\"passed\":" << (passed ? "true" : "false") << "}\n" << std::flush;
            if (!passed) throw std::runtime_error("GPU projected convolution disagrees with full double CPU convolution");
            bool cancelled = false;
            try {
                uta::torch_native::projected_convolution(device_input, device_kernel, device_bias, test.stride, test.padding,
                    [] { throw std::runtime_error("cancelled"); }, test.workspace);
            } catch (const std::runtime_error& error) { cancelled = std::string(error.what()) == "cancelled"; }
            if (!cancelled) throw std::runtime_error("projected convolution cancellation was not propagated");
        }
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
