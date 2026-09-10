#pragma once
// Synthetic operator diagnostics only; never loads or executes a model.
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <functional>
#include <iomanip>
#include <iostream>
#include <numeric>
#include <stdexcept>
#include <string>
#include <vector>

namespace probe {
struct Shape {
    std::string name;
    bool attention = false;
    bool frequency = false;
    int64_t batch = 1, rows = 1, columns = 1, depth = 1, heads = 1;
    int64_t input_count() const { return batch * rows * heads * depth; }
    int64_t output_count() const { return batch * rows * heads * (attention ? depth : columns); }
    double flops() const {
        return attention ? 4.0 * batch * heads * rows * rows * depth
                         : 2.0 * batch * rows * columns * depth;
    }
    int64_t input_offset(int64_t batch_index, int64_t row, int64_t head, int64_t depth_index) const {
        if (attention) return ((batch_index * rows + row) * heads + head) * depth + depth_index;
        return frequency ? (row * batch + batch_index) * depth + depth_index
                         : (batch_index * rows + row) * depth + depth_index;
    }
};
inline Shape shape(const std::string& name) {
    if (name == "smoke-gemm") return {name, false, false, 2, 33, 64, 64, 1};
    if (name == "smoke-attention") return {name, true, false, 2, 65, 0, 64, 2};
    if (name == "qkv-time") return {name, false, false, 90, 1722, 1536, 256, 1};
    if (name == "qkv-frequency") return {name, false, true, 1722, 90, 1536, 256, 1};
    if (name == "ffn-time") return {name, false, false, 90, 1722, 1024, 256, 1};
    if (name == "ffn-frequency") return {name, false, true, 1722, 90, 1024, 256, 1};
    if (name == "down-time") return {name, false, false, 90, 1722, 256, 1024, 1};
    if (name == "attention-time") return {name, true, false, 90, 1722, 0, 64, 8};
    if (name == "attention-frequency") return {name, true, true, 1722, 90, 0, 64, 8};
    throw std::invalid_argument("unknown case: " + name);
}
inline uint64_t mix(uint64_t value) {
    value += UINT64_C(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)) * UINT64_C(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)) * UINT64_C(0x94d049bb133111eb);
    return value ^ (value >> 31);
}
inline float value(int64_t index, uint64_t salt) {
    return (static_cast<float>(mix(static_cast<uint64_t>(index) ^ salt) >> 40) * 0x1p-23f - 1.0f) * 0.5f;
}
inline float rounded(float input, const std::string& precision) {
    if (precision == "f32") return input;
    if (precision == "f16") return static_cast<float>(static_cast<_Float16>(input));
    if (precision == "bf16") {
        uint32_t bits;
        std::memcpy(&bits, &input, sizeof(bits));
        bits += 0x7fff + ((bits >> 16) & 1);
        bits &= 0xffff0000;
        std::memcpy(&input, &bits, sizeof(bits));
        return input;
    }
    throw std::invalid_argument("precision must be f32, f16, or bf16");
}
inline std::vector<float> data(int64_t count, uint64_t salt, const std::string& precision) {
    std::vector<float> result(static_cast<size_t>(count));
    for (int64_t index = 0; index < count; ++index) result[index] = rounded(value(index, salt), precision);
    return result;
}
inline std::string quote(const std::string& text) {
    std::string result = "\"";
    for (unsigned char character : text) {
        if (character == '"' || character == '\\') { result += '\\'; result += character; }
        else if (character == '\n') result += "\\n";
        else if (character == '\r') result += "\\r";
        else if (character == '\t') result += "\\t";
        else if (character >= 32) result += character;
    }
    return result + '"';
}
inline void array(const std::vector<double>& values) {
    std::cout << '[';
    for (size_t index = 0; index < values.size(); ++index) {
        if (index) std::cout << ',';
        std::cout << values[index];
    }
    std::cout << ']';
}
struct Timing {
    std::vector<double> warmup, measured;
};
inline Timing measure(const std::function<void()>& compute, int warmup, int repeats) {
    Timing result;
    for (int index = 0; index < warmup + repeats; ++index) {
        const auto begin = std::chrono::steady_clock::now();
        compute(); // Includes completion synchronization; excludes preparation and reference work.
        const double elapsed = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - begin).count();
        (index < warmup ? result.warmup : result.measured).push_back(elapsed);
    }
    return result;
}
inline double reference(const Shape& current, int64_t output_index, const std::string& precision) {
    if (!current.attention) {
        const int64_t column = output_index % current.columns;
        const int64_t row = (output_index / current.columns) % current.rows;
        const int64_t batch = output_index / (current.columns * current.rows);
        double sum = 0;
        for (int64_t depth = 0; depth < current.depth; ++depth) {
            const float left = rounded(value(current.input_offset(batch, row, 0, depth), 101), precision);
            const float right = rounded(value(column * current.depth + depth, 202), precision);
            sum += static_cast<double>(left) * right;
        }
        return sum;
    }
    const int64_t depth = output_index % current.depth;
    const int64_t head = (output_index / current.depth) % current.heads;
    const int64_t row = (output_index / (current.depth * current.heads)) % current.rows;
    const int64_t batch = output_index / (current.depth * current.heads * current.rows);
    std::vector<double> scores(current.rows);
    for (int64_t key = 0; key < current.rows; ++key) {
        double dot = 0;
        for (int64_t feature = 0; feature < current.depth; ++feature) {
            const float query = rounded(value(current.input_offset(batch, row, head, feature), 101), precision);
            const float operand = rounded(value(current.input_offset(batch, key, head, feature), 202), precision);
            dot += static_cast<double>(query) * operand;
        }
        scores[key] = dot / std::sqrt(static_cast<double>(current.depth));
    }
    const double maximum = *std::max_element(scores.begin(), scores.end());
    double total = 0, weighted = 0;
    for (int64_t key = 0; key < current.rows; ++key) {
        const double probability = std::exp(scores[key] - maximum);
        total += probability;
        weighted += probability * rounded(value(current.input_offset(batch, key, head, depth), 303), precision);
    }
    return weighted / total;
}
struct Accuracy {
    int64_t count = 0, finite = 0;
    int samples = 0;
    double nmse = 0, original_nmse = 0, max_abs = 0;
};
inline Accuracy validate(const Shape& current, const std::string& precision, const float* output) {
    Accuracy result;
    result.count = current.output_count();
    for (int64_t index = 0; index < result.count; ++index) result.finite += std::isfinite(output[index]);
    double squared = 0, energy = 0, original_squared = 0, original_energy = 0;
    const int samples = static_cast<int>(std::min<int64_t>(128, result.count));
    for (int index = 0; index < samples; ++index) {
        const int64_t position = index < 2 ? (index == 0 ? 0 : result.count - 1)
            : static_cast<int64_t>(mix(index + 404) % static_cast<uint64_t>(result.count));
        const double expected = reference(current, position, precision);
        const double original = precision == "f32" ? expected : reference(current, position, "f32");
        const double error = output[position] - expected;
        const double original_error = output[position] - original;
        squared += error * error; energy += expected * expected;
        original_squared += original_error * original_error; original_energy += original * original;
        result.max_abs = std::max(result.max_abs, std::abs(error));
    }
    result.samples = samples;
    result.nmse = squared / std::max(energy, 1e-30);
    result.original_nmse = original_squared / std::max(original_energy, 1e-30);
    return result;
}
inline void report(const std::string& backend, const Shape& current, const std::string& precision,
                   const Timing& timing, const Accuracy& accuracy, const std::string& note, double preparation) {
    const double mean = std::accumulate(timing.measured.begin(), timing.measured.end(), 0.0) / timing.measured.size();
    double squares = 0;
    for (double value : timing.measured) squares += (value - mean) * (value - mean);
    const double deviation = timing.measured.size() > 1 ? std::sqrt(squares / (timing.measured.size() - 1)) : 0;
    const bool passed = accuracy.finite == accuracy.count && std::isfinite(accuracy.nmse) && accuracy.nmse < 5e-4;
    std::cout << std::setprecision(12) << "PROBE_RESULT {\"backend\":" << quote(backend)
        << ",\"case\":" << quote(current.name) << ",\"precision\":" << quote(precision)
        << ",\"batch\":" << current.batch << ",\"rows\":" << current.rows
        << ",\"columns\":" << current.columns << ",\"depth\":" << current.depth << ",\"heads\":" << current.heads
        << ",\"flops\":" << current.flops() << ",\"mean_ms\":" << mean << ",\"sd_ms\":" << deviation
        << ",\"effective_tflops\":" << current.flops() / (mean * 1e9)
        << ",\"prepare_ms\":" << preparation << ",\"finite_values\":" << accuracy.finite
        << ",\"output_values\":" << accuracy.count << ",\"reference_samples\":" << accuracy.samples
        << ",\"nmse_quantized_input_reference\":" << accuracy.nmse
        << ",\"nmse_original_float_input_reference\":" << accuracy.original_nmse
        << ",\"max_abs_quantized_input_reference\":" << accuracy.max_abs
        << ",\"numeric_pass\":" << (passed ? "true" : "false")
        << ",\"note\":" << quote(note)
        << ",\"timing_scope\":\"synchronized host compute including dispatch; excludes preparation, upload, download, and validation\""
        << ",\"warmup_ms\":"; array(timing.warmup);
    std::cout << ",\"measured_ms\":"; array(timing.measured); std::cout << "}\n" << std::flush;
    if (!passed) throw std::runtime_error("numerical validation failed");
}
} // namespace probe
