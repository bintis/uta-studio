// Native AMD-only bounded arithmetic checks; no model loading or Python runtime.
#include <ATen/ATen.h>
#include <ATen/Context.h>
#include <ATen/Parallel.h>
#include <ATen/ops/_fused_sdp_choice.h>
#include <c10/core/InferenceMode.h>
#include <hip/hip_runtime_api.h>
#include <torch/version.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <iomanip>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
using Clock = std::chrono::steady_clock;

void hip_checked(hipError_t result, const char* operation) {
    if (result != hipSuccess) throw std::runtime_error(std::string(operation) + ": " + hipGetErrorString(result));
}

std::vector<float> values(std::size_t count, double phase) {
    std::vector<float> result(count);
    for (std::size_t index = 0; index < count; ++index)
        result[index] = static_cast<float>(0.23 * std::sin(static_cast<double>(index) * 0.19 + phase));
    return result;
}

at::Tensor upload(std::vector<float>& source, at::IntArrayRef shape, const at::Device& device) {
    return at::from_blob(source.data(), shape, at::kFloat).to(device);
}

struct Fixture {
    at::Tensor output;
    std::vector<double> reference;
    double tolerance = 1e-10;
    double elapsed = 0;
    int64_t attention_backend = -1;
    bool half_attention = false;
};

template<class Function> at::Tensor timed(Function function, double& elapsed) {
    hip_checked(hipDeviceSynchronize(), "pre-compute synchronization");
    const auto begin = Clock::now();
    auto result = function();
    hip_checked(hipDeviceSynchronize(), "post-compute synchronization");
    elapsed = std::chrono::duration<double>(Clock::now() - begin).count();
    return result;
}

Fixture gemm(const at::Device& device) {
    constexpr int rows = 32, depth = 64, columns = 24;
    auto left = values(rows * depth, 0.1);
    auto right = values(columns * depth, 0.7);
    auto input = upload(left, {rows, depth}, device);
    auto weight = upload(right, {columns, depth}, device);
    Fixture fixture;
    fixture.output = timed([&] { return at::linear(input, weight); }, fixture.elapsed);
    fixture.reference.resize(rows * columns);
    for (int row = 0; row < rows; ++row)
        for (int column = 0; column < columns; ++column) {
            double total = 0;
            for (int contraction = 0; contraction < depth; ++contraction)
                total += static_cast<double>(left[row * depth + contraction]) * right[column * depth + contraction];
            fixture.reference[row * columns + column] = total;
        }
    return fixture;
}

Fixture convolution(const at::Device& device) {
    constexpr int height = 17, width = 19, channels = 2, filters = 4, kernel = 3;
    auto source = values(channels * height * width, 0.2);
    auto weights = values(filters * channels * kernel * kernel, 0.9);
    auto biases = values(filters, 1.3);
    auto input = upload(source, {1, channels, height, width}, device);
    auto weight = upload(weights, {filters, channels, kernel, kernel}, device);
    auto bias = upload(biases, {filters}, device);
    Fixture fixture;
    fixture.output = timed([&] { return at::conv2d(input, weight, bias, {1, 1}, {1, 1}); }, fixture.elapsed);
    fixture.reference.resize(filters * height * width);
    for (int filter = 0; filter < filters; ++filter)
        for (int row = 0; row < height; ++row)
            for (int column = 0; column < width; ++column) {
                double total = biases[filter];
                for (int channel = 0; channel < channels; ++channel)
                    for (int vertical = 0; vertical < kernel; ++vertical)
                        for (int horizontal = 0; horizontal < kernel; ++horizontal) {
                            const int source_row = row + vertical - 1;
                            const int source_column = column + horizontal - 1;
                            if (source_row < 0 || source_row >= height || source_column < 0 || source_column >= width) continue;
                            const auto sample = source[(channel * height + source_row) * width + source_column];
                            const auto coefficient = weights[((filter * channels + channel) * kernel + vertical) * kernel + horizontal];
                            total += static_cast<double>(sample) * coefficient;
                        }
                fixture.reference[(filter * height + row) * width + column] = total;
            }
    return fixture;
}

Fixture recurrent(const at::Device& device) {
    constexpr int sequence = 17, inputs = 5, hidden = 8, directions = 2;
    auto source = values(sequence * inputs, 0.3);
    auto input = upload(source, {sequence, 1, inputs}, device);
    std::vector<std::vector<float>> storage;
    std::vector<at::Tensor> parameters;
    storage.reserve(directions * 4);
    parameters.reserve(directions * 4);
    for (int direction = 0; direction < directions; ++direction) {
        storage.push_back(values(3 * hidden * inputs, 0.6 + direction));
        parameters.push_back(upload(storage.back(), {3 * hidden, inputs}, device));
        storage.push_back(values(3 * hidden * hidden, 1.0 + direction));
        parameters.push_back(upload(storage.back(), {3 * hidden, hidden}, device));
        storage.push_back(values(3 * hidden, 1.4 + direction));
        parameters.push_back(upload(storage.back(), {3 * hidden}, device));
        storage.push_back(values(3 * hidden, 1.8 + direction));
        parameters.push_back(upload(storage.back(), {3 * hidden}, device));
    }
    auto initial = at::zeros({directions, 1, hidden}, input.options());
    Fixture fixture;
    fixture.output = timed([&] {
        return std::get<0>(at::gru(input, initial, parameters, true, 1, 0.0, false, true, false));
    }, fixture.elapsed);
    fixture.reference.resize(sequence * hidden * directions);
    const auto sigmoid = [](double value) { return 1.0 / (1.0 + std::exp(-value)); };
    for (int direction = 0; direction < directions; ++direction) {
        std::vector<double> previous(hidden, 0.0), next(hidden);
        for (int step = 0; step < sequence; ++step) {
            const int position = direction ? sequence - step - 1 : step;
            for (int unit = 0; unit < hidden; ++unit) {
                double feed[3], recurrent_feed[3];
                for (int gate = 0; gate < 3; ++gate) {
                    const int row = gate * hidden + unit;
                    feed[gate] = storage[direction * 4 + 2][row];
                    recurrent_feed[gate] = storage[direction * 4 + 3][row];
                    for (int channel = 0; channel < inputs; ++channel)
                        feed[gate] += static_cast<double>(source[position * inputs + channel]) * storage[direction * 4][row * inputs + channel];
                    for (int channel = 0; channel < hidden; ++channel)
                        recurrent_feed[gate] += previous[channel] * storage[direction * 4 + 1][row * hidden + channel];
                }
                const double reset = sigmoid(feed[0] + recurrent_feed[0]);
                const double update = sigmoid(feed[1] + recurrent_feed[1]);
                const double proposal = std::tanh(feed[2] + reset * recurrent_feed[2]);
                next[unit] = (1.0 - update) * proposal + update * previous[unit];
                fixture.reference[position * directions * hidden + direction * hidden + unit] = next[unit];
            }
            previous = next;
        }
    }
    return fixture;
}

Fixture attention(const at::Device& device) {
    constexpr int heads = 2, sequence = 65, depth = 64;
    auto query_values = values(heads * sequence * depth, 0.2);
    auto key_values = values(heads * sequence * depth, 1.0);
    auto value_values = values(heads * sequence * depth, 1.9);
    // Explicit input rounding. The complete reference uses these same FP16 values.
    for (auto* source : {&query_values, &key_values, &value_values}) {
        auto rounded = at::from_blob(source->data(), {static_cast<int64_t>(source->size())}, at::kFloat).to(at::kHalf).to(at::kFloat);
        std::copy_n(rounded.const_data_ptr<float>(), source->size(), source->begin());
    }
    auto query = upload(query_values, {1, heads, sequence, depth}, device).to(at::kHalf);
    auto key = upload(key_values, {1, heads, sequence, depth}, device).to(at::kHalf);
    auto value = upload(value_values, {1, heads, sequence, depth}, device).to(at::kHalf);
    Fixture fixture;
    fixture.half_attention = true;
    fixture.tolerance = 5e-4;
    fixture.attention_backend = at::_fused_sdp_choice(query, key, value, {}, 0.0, false);
    if (fixture.attention_backend <= 0) throw std::runtime_error("fused SDPA unavailable; mathematical fallback is disabled");
    fixture.output = timed([&] { return at::scaled_dot_product_attention(query, key, value).to(at::kFloat); }, fixture.elapsed);
    fixture.reference.resize(heads * sequence * depth);
    for (int head = 0; head < heads; ++head)
        for (int row = 0; row < sequence; ++row) {
            std::vector<double> scores(sequence);
            for (int column = 0; column < sequence; ++column) {
                double score = 0;
                for (int feature = 0; feature < depth; ++feature)
                    score += static_cast<double>(query_values[(head * sequence + row) * depth + feature]) * key_values[(head * sequence + column) * depth + feature];
                scores[column] = score / std::sqrt(static_cast<double>(depth));
            }
            const double maximum = *std::max_element(scores.begin(), scores.end());
            double denominator = 0;
            for (auto& score : scores) { score = std::exp(score - maximum); denominator += score; }
            for (int feature = 0; feature < depth; ++feature) {
                double total = 0;
                for (int column = 0; column < sequence; ++column)
                    total += scores[column] / denominator * value_values[(head * sequence + column) * depth + feature];
                fixture.reference[(head * sequence + row) * depth + feature] = total;
            }
        }
    return fixture;
}

void report(const Fixture& fixture, const std::string& name) {
    auto output = fixture.output.to(at::kCPU).to(at::kFloat).contiguous();
    if (output.numel() != static_cast<int64_t>(fixture.reference.size())) throw std::runtime_error("output element count mismatch");
    const auto* actual = output.const_data_ptr<float>();
    double error_energy = 0, reference_energy = 0, maximum = 0;
    std::size_t nonfinite = 0;
    for (std::size_t index = 0; index < fixture.reference.size(); ++index) {
        if (!std::isfinite(actual[index])) { ++nonfinite; continue; }
        const double difference = actual[index] - fixture.reference[index];
        error_energy += difference * difference;
        reference_energy += fixture.reference[index] * fixture.reference[index];
        maximum = std::max(maximum, std::abs(difference));
    }
    if (nonfinite) throw std::runtime_error("native output contains " + std::to_string(nonfinite) + " nonfinite elements");
    if (!(reference_energy > 0)) throw std::runtime_error("diagnostic reference has no signal energy");
    const double nmse = error_energy / reference_energy;
    const bool passed = nmse <= fixture.tolerance;
    std::cout << std::setprecision(12)
              << "{\"event\":\"result\",\"case\":" << std::quoted(name)
              << ",\"backend\":\"libtorch_rocm\",\"scope\":\"bounded_operator_not_model_acceptance\""
              << ",\"elements\":" << fixture.reference.size() << ",\"compared_elements\":" << fixture.reference.size()
              << ",\"all_finite\":true,\"reference\":\"complete_double_contraction\",\"nmse\":" << nmse
              << ",\"max_absolute_error\":" << maximum << ",\"nmse_limit\":" << fixture.tolerance
              << ",\"synchronized_cold_compute_seconds\":" << fixture.elapsed
              << ",\"attention_backend\":" << fixture.attention_backend
              << ",\"attention_input_and_output_rounding\":" << std::quoted(fixture.half_attention ? "float16" : "not_applicable")
              << ",\"passed\":" << (passed ? "true" : "false") << "}\n";
    if (!passed) throw std::runtime_error("complete-output NMSE exceeds the declared diagnostic limit");
}
} // namespace

int main(int argc, char** argv) {
    std::cout.setf(std::ios::unitbuf);
    try {
        if (argc != 2) throw std::invalid_argument("usage: uta-libtorch-device-check gemm|convolution|gru|attention");
        const std::string name = argv[1];
        if (name != "gemm" && name != "convolution" && name != "gru" && name != "attention")
            throw std::invalid_argument("unknown native diagnostic case");
        if (const char* override_architecture = std::getenv("HSA_OVERRIDE_GFX_VERSION"); override_architecture && *override_architecture)
            throw std::runtime_error("remove HSA_OVERRIDE_GFX_VERSION for native gfx1103 verification");
        at::set_num_threads(2);
        c10::InferenceMode inference;
        auto& context = at::globalContext();
        if (!context.hasROCM()) throw std::runtime_error("the loaded LibTorch is not a ROCm build; no fallback");
        context.setFloat32Precision(at::Float32Backend::GENERIC, at::Float32Op::ALL, at::Float32Precision::IEEE);
        context.setAllowFP16ReductionCuBLAS(false, false);
        context.setSDPUseMath(false);
        hip_checked(hipSetDevice(0), "select AMD device zero");
        hipDeviceProp_t properties{};
        hip_checked(hipGetDeviceProperties(&properties, 0), "query selected AMD device");
        if (std::string(properties.gcnArchName).substr(0, 7) != "gfx1103")
            throw std::runtime_error("selected AMD device is not gfx1103; refusing a different-device test");
        char pci[64]{};
        hip_checked(hipDeviceGetPCIBusId(pci, sizeof(pci), 0), "query selected AMD PCI address");
        std::cout << "{\"event\":\"environment\",\"torch_version\":" << std::quoted(TORCH_VERSION)
                  << ",\"backend\":\"libtorch_rocm\",\"device\":\"cuda:0\",\"name\":" << std::quoted(properties.name)
                  << ",\"architecture\":" << std::quoted(properties.gcnArchName) << ",\"pci\":" << std::quoted(pci)
                  << ",\"math_attention_fallback\":false,\"xpu_initialized\":false}\n";
        const at::Device device(at::kCUDA, 0);
        const Fixture fixture = name == "gemm" ? gemm(device) : name == "convolution" ? convolution(device)
                              : name == "gru" ? recurrent(device) : attention(device);
        report(fixture, name);
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "native AMD check failed: " << error.what() << '\n';
        return 1;
    }
}
