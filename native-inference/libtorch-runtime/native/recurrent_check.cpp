#include "recurrent.hpp"
#include <ATen/Context.h>
#include <ATen/Parallel.h>
#include <c10/core/InferenceMode.h>
#include <array>
#include <cmath>
#include <iomanip>
#include <iostream>
#include <string>

namespace {
at::Tensor fixture(at::IntArrayRef shape, double phase) {
    int64_t count = 1;
    for (auto dimension : shape) count *= dimension;
    return (at::sin(at::arange(count, at::kDouble) * 0.137 + phase) * 0.075).reshape(shape).to(at::kFloat);
}
}
int main(int argc, char** argv) {
    try {
        if (argc != 2) throw std::invalid_argument("usage: uta-libtorch-recurrent-check <rocm|xpu>");
        const std::string backend = argv[1];
        if (backend != "rocm" && backend != "xpu") throw std::invalid_argument("select a GPU backend explicitly");
        if (backend == "rocm" && !at::globalContext().hasROCM()) throw std::runtime_error("this LibTorch does not provide ROCm");
        const auto device = at::Device(backend == "rocm" ? at::kCUDA : at::kXPU, 0);
        c10::InferenceMode inference;
        at::set_num_threads(2);
        at::globalContext().setFloat32Precision(at::Float32Backend::GENERIC, at::Float32Op::ALL, at::Float32Precision::IEEE);
        for (const auto dimensions : std::vector<std::array<int64_t, 4>>{{1,1,5,7}, {17,2,5,7}, {64,1,384,256}}) {
            const auto frames = dimensions[0], batch = dimensions[1], width = dimensions[2], hidden = dimensions[3];
            const auto input = fixture({frames, batch, width}, 0.31);
            const auto initial = fixture({2, batch, hidden}, 0.73);
            std::vector<at::Tensor> cpu_parameters, gpu_parameters;
            for (int64_t direction = 0; direction < 2; ++direction) {
                for (auto parameter : std::vector<at::Tensor>{
                         fixture({hidden * 3, width}, direction + 1.1),
                         fixture({hidden * 3, hidden}, direction + 1.7),
                         fixture({hidden * 3}, direction + 2.3),
                         fixture({hidden * 3}, direction + 2.9)}) {
                    cpu_parameters.push_back(parameter.to(at::kDouble));
                    gpu_parameters.push_back(parameter.to(device));
                }
            }
            auto expected = std::get<0>(at::gru(input.to(at::kDouble), initial.to(at::kDouble), cpu_parameters,
                                               true, 1, 0.0, false, true, false));
            const auto device_input = input.to(device);
            const auto device_initial = initial.to(device);
            auto actual = uta::torch_native::fused_bidirectional_gru(device_input, device_initial, gpu_parameters, [] {});
            if (actual.device() != device) throw std::runtime_error("GRU changed device");
            actual = actual.to(at::kCPU).to(at::kDouble); // Synchronized complete readback.
            const auto difference = actual - expected;
            const auto nmse = difference.square().sum().item<double>() / std::max(expected.square().sum().item<double>(), 1e-30);
            const auto maximum = difference.abs().max().item<double>();
            const bool finite = at::isfinite(actual).all().item<bool>();
            const bool passed = finite && actual.sizes() == expected.sizes() && nmse <= 1e-10 && maximum <= 5e-5;
            std::cout << std::setprecision(12) << "{\"event\":\"recurrent_check\",\"backend\":\"" << backend
                      << "\",\"frames\":" << frames << ",\"batch\":" << batch << ",\"hidden\":" << hidden
                      << ",\"compared_elements\":" << expected.numel() << ",\"nmse\":" << nmse
                      << ",\"maximum_absolute_error\":" << maximum << ",\"passed\":" << (passed ? "true" : "false") << "}\n" << std::flush;
            if (!passed) throw std::runtime_error("fused GRU disagrees with the complete double CPU GRU reference");
            bool cancelled = false;
            try {
                uta::torch_native::fused_bidirectional_gru(device_input, device_initial, gpu_parameters,
                    [] { throw std::runtime_error("cancelled"); });
            } catch (const std::runtime_error& error) { cancelled = std::string(error.what()) == "cancelled"; }
            if (!cancelled) throw std::runtime_error("fused GRU cancellation was not propagated");
        }
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
