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
        struct ProjectionShape {
            int64_t rows;
            int64_t width;
            int64_t channels;
            bool batched;
            bool split;
        };
        const std::vector<ProjectionShape> projection_shapes{
            {17, 63, 97, false, false},
            {801, 1536, 1536, false, false},
            {801, 1536, 37, false, false},
            {2051, 63, 97, false, false},
            {6408, 384, 1536, true, true},
            {60000, 384, 1536, false, false},
        };
        for (const auto& projection_shape : projection_shapes) {
            const auto rows = projection_shape.rows;
            const auto width = projection_shape.width;
            const auto channels = projection_shape.channels;
            const auto input = projection_shape.batched
                ? fixture({8, 801, width}, 0.31)
                : fixture({rows, width}, 0.31);
            const auto matrix = input.reshape({rows, width});
            const auto weight = fixture({channels, width}, 1.07);
            const auto bias = fixture({channels}, 0.73);
            const auto device_input = input.to(device), device_weight = weight.to(device), device_bias = bias.to(device);
            const auto row_tile = uta::torch_native::bounded_projection_row_tile(device_input, device_weight);
            at::Tensor device_actual;
            if (projection_shape.split) {
                const auto weight_parts = device_weight.chunk(3, 0);
                const auto bias_parts = device_bias.chunk(3, 0);
                std::vector<at::Tensor> output_parts;
                output_parts.reserve(weight_parts.size());
                for (size_t part = 0; part < weight_parts.size(); ++part)
                    output_parts.push_back(uta::torch_native::tiled_projection(
                        device_input, weight_parts[part], bias_parts[part], [] {}, row_tile));
                device_actual = at::cat(output_parts, -1);
            } else {
                device_actual = uta::torch_native::tiled_projection(
                    device_input, device_weight, device_bias, [] {}, row_tile);
            }
            auto output_shape = input.sizes().vec();
            output_shape.back() = channels;
            if (device_actual.sizes() != at::IntArrayRef(output_shape))
                throw std::runtime_error("projection output shape differs");
            const auto actual = device_actual.reshape({rows, channels}).to(at::kCPU);
            double squared_error = 0.0, reference_energy = 0.0, maximum = 0.0;
            int64_t compared = 0;
            // Compare every output row with double CPU contractions; bounded
            // host reference tiles avoid an unnecessary second full output copy.
            for (int64_t start = 0; start < rows; start += 1024) {
                const auto count = std::min<int64_t>(1024, rows - start);
                const auto expected = at::linear(matrix.narrow(0,start,count).to(at::kDouble), weight.to(at::kDouble), bias.to(at::kDouble));
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
            std::cout << std::setprecision(12) << "{\"event\":\""
                      << (projection_shape.split ? "split_qkv_projection_check" : "tiled_projection_check")
                      << "\",\"backend\":\"rocm\",\"rows\":" << rows
                      << ",\"input_channels\":" << width << ",\"output_channels\":" << channels
                      << ",\"row_tile\":" << row_tile << ",\"compared_elements\":" << compared << ",\"nmse\":" << nmse
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
            const auto device_input_weight = input_weight.to(device), device_output_weight = output_weight.to(device);
            const auto row_tile = std::min(uta::torch_native::bounded_projection_row_tile(device_input_weight),
                                           uta::torch_native::bounded_projection_row_tile(device_output_weight));
            const auto actual = uta::torch_native::tiled_feed_forward(
                input.to(device), device_input_weight, input_bias.to(device),
                device_output_weight, output_bias.to(device), [] {}, row_tile).to(at::kCPU);
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
                      << ",\"row_tile\":" << row_tile << ",\"compared_elements\":" << compared << ",\"nmse\":" << nmse
                      << ",\"maximum_absolute_error\":" << maximum << ",\"passed\":" << (passed ? "true" : "false") << "}\n" << std::flush;
            if (!passed) throw std::runtime_error("feed-forward disagrees with complete double CPU reference");
        }
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
