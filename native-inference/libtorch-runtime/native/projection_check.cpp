#include "projection.hpp"
#include <ATen/Context.h>
#include <ATen/Parallel.h>
#include <c10/core/InferenceMode.h>
#include <iomanip>
#include <iostream>

namespace {
at::Tensor fixture(at::IntArrayRef shape, double phase) {
    int64_t count = 1;
    for (auto dimension : shape) count *= dimension;
    return (at::sin(at::arange(count, at::kDouble) * 0.017 + phase) * 0.125).reshape(shape).to(at::kFloat);
}
}
int main(int argc, char** argv) {
    try {
        if (argc != 2 || std::string(argv[1]) != "rocm" || !at::globalContext().hasROCM())
            throw std::invalid_argument("usage: uta-libtorch-projection-check rocm");
        c10::InferenceMode inference;
        at::set_num_threads(2);
        at::globalContext().setFloat32Precision(at::Float32Backend::GENERIC, at::Float32Op::ALL, at::Float32Precision::IEEE);
        const auto device = at::Device(at::kCUDA, 0);
        for (const auto rows : {17, 2051, 60000}) {
            const int64_t width = rows == 60000 ? 384 : 63;
            const int64_t channels = rows == 60000 ? 1536 : 97;
            const auto input = fixture({rows, width}, 0.31);
            const auto weight = fixture({channels, width}, 1.07);
            const auto bias = fixture({channels}, 0.73);
            const auto device_input = input.to(device), device_weight = weight.to(device), device_bias = bias.to(device);
            const auto actual = uta::torch_native::tiled_projection(device_input, device_weight, device_bias, [] {}).to(at::kCPU);
            if (actual.sizes() != at::IntArrayRef({rows, channels})) throw std::runtime_error("projection output shape differs");
            double squared_error = 0.0, reference_energy = 0.0, maximum = 0.0;
            int64_t compared = 0;
            // Compare every output row with double CPU contractions; bounded
            // host reference tiles avoid an unnecessary second full output copy.
            for (int64_t start = 0; start < rows; start += 1024) {
                const auto count = std::min<int64_t>(1024, rows - start);
                const auto expected = at::linear(input.narrow(0,start,count).to(at::kDouble), weight.to(at::kDouble), bias.to(at::kDouble));
                const auto observed = actual.narrow(0,start,count).to(at::kDouble);
                if (!at::isfinite(observed).all().item<bool>()) throw std::runtime_error("nonfinite GPU projection output");
                const auto difference = observed - expected;
                squared_error += difference.square().sum().item<double>();
                reference_energy += expected.square().sum().item<double>();
                maximum = std::max(maximum, difference.abs().max().item<double>());
                compared += expected.numel();
            }
            const auto nmse = squared_error / std::max(reference_energy, 1e-30);
            const bool passed = nmse <= 1e-10 && maximum <= 5e-5;
            std::cout << std::setprecision(12) << "{\"event\":\"tiled_projection_check\",\"backend\":\"rocm\",\"rows\":" << rows
                      << ",\"compared_elements\":" << compared << ",\"nmse\":" << nmse
                      << ",\"maximum_absolute_error\":" << maximum << ",\"passed\":" << (passed ? "true" : "false") << "}\n" << std::flush;
            if (!passed) throw std::runtime_error("projection disagrees with complete double CPU reference");
        }
        for (const auto rows : {19, 2051, 60000}) {
            const int64_t width = rows == 60000 ? 384 : 61;
            const int64_t hidden_channels = rows == 60000 ? 1536 : 113;
            const int64_t output_channels = rows == 60000 ? 384 : 47;
            const auto input = fixture({rows, width}, 0.19);
            const auto input_weight = fixture({hidden_channels, width}, 0.83);
            const auto input_bias = fixture({hidden_channels}, 1.31);
            const auto output_weight = fixture({output_channels, hidden_channels}, 1.79);
            const auto output_bias = fixture({output_channels}, 2.23);
            const auto actual = uta::torch_native::tiled_feed_forward(
                input.to(device), input_weight.to(device), input_bias.to(device),
                output_weight.to(device), output_bias.to(device), [] {}).to(at::kCPU);
            if (actual.sizes() != at::IntArrayRef({rows, output_channels}))
                throw std::runtime_error("feed-forward output shape differs");
            double squared_error = 0.0, reference_energy = 0.0, maximum = 0.0;
            int64_t compared = 0;
            for (int64_t start = 0; start < rows; start += 1024) {
                const auto count = std::min<int64_t>(1024, rows - start);
                auto expected = at::linear(input.narrow(0, start, count).to(at::kDouble),
                    input_weight.to(at::kDouble), input_bias.to(at::kDouble));
                expected = at::gelu(expected, "none");
                expected = at::linear(expected, output_weight.to(at::kDouble), output_bias.to(at::kDouble));
                const auto observed = actual.narrow(0, start, count).to(at::kDouble);
                if (!at::isfinite(observed).all().item<bool>())
                    throw std::runtime_error("nonfinite GPU feed-forward output");
                const auto difference = observed - expected;
                squared_error += difference.square().sum().item<double>();
                reference_energy += expected.square().sum().item<double>();
                maximum = std::max(maximum, difference.abs().max().item<double>());
                compared += expected.numel();
            }
            const auto nmse = squared_error / std::max(reference_energy, 1e-30);
            const bool passed = nmse <= 1e-9 && maximum <= 1e-3;
            std::cout << std::setprecision(12) << "{\"event\":\"tiled_feed_forward_check\",\"backend\":\"rocm\",\"rows\":" << rows
                      << ",\"compared_elements\":" << compared << ",\"nmse\":" << nmse
                      << ",\"maximum_absolute_error\":" << maximum << ",\"passed\":" << (passed ? "true" : "false") << "}\n" << std::flush;
            if (!passed) throw std::runtime_error("feed-forward disagrees with complete double CPU reference");
        }
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
