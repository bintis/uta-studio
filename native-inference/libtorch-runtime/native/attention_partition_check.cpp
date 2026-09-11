#include "attention_partition.hpp"
#include <ATen/Context.h>
#include <ATen/Parallel.h>
#include <c10/core/InferenceMode.h>
#include <array>
#include <iomanip>
#include <iostream>
#include <vector>

namespace {
at::Tensor fixture(at::IntArrayRef shape, double phase) {
    int64_t count = 1;
    for (auto dimension : shape) count *= dimension;
    return (at::sin(at::arange(count, at::kDouble) * 0.037 + phase) * 0.5).reshape(shape).to(at::kFloat);
}
at::Tensor reference_attention(
    const at::Tensor& query, const at::Tensor& key, const at::Tensor& value,
    const at::Tensor& mask, bool causal, bool grouped, double scale) {
    const auto rounded = [](const at::Tensor& input) { return input.to(at::kHalf).to(at::kDouble); };
    auto rounded_query = rounded(query);
    auto rounded_key = rounded(key);
    auto rounded_value = rounded(value);
    if (query.size(1) != key.size(1)) {
        if (!grouped || query.size(1) % key.size(1))
            throw std::invalid_argument("invalid grouped-query reference fixture");
        rounded_key = at::repeat_interleave(rounded_key, query.size(1) / key.size(1), 1);
        rounded_value = at::repeat_interleave(rounded_value, query.size(1) / key.size(1), 1);
    }
    auto scores = at::matmul(rounded_query, rounded_key.transpose(-1, -2)) * scale;
    if (mask.defined()) {
        if (mask.scalar_type() == at::kBool)
            scores = scores.masked_fill(mask.logical_not(), -std::numeric_limits<double>::infinity());
        else
            scores = scores + mask.to(at::kDouble);
    }
    if (causal) {
        auto query_rows = at::arange(query.size(2), at::kLong).unsqueeze(-1);
        auto key_columns = at::arange(key.size(2), at::kLong).unsqueeze(0);
        scores = scores.masked_fill(key_columns > query_rows, -std::numeric_limits<double>::infinity());
    }
    auto probabilities = at::softmax(scores, -1);
    probabilities = at::where(at::isneginf(scores).all(-1, true), at::zeros_like(probabilities), probabilities);
    return at::matmul(probabilities, rounded_value).to(at::kHalf).to(at::kDouble);
}
void verify(const std::string& name, const at::Tensor& actual, const at::Tensor& expected) {
    const auto observed = actual.to(at::kCPU).to(at::kDouble);
    const auto error = observed - expected;
    const auto nmse = error.square().sum().item<double>() /
        std::max(expected.square().sum().item<double>(), 1e-30);
    const auto maximum = error.abs().max().item<double>();
    const bool passed = observed.sizes() == expected.sizes() &&
        at::isfinite(observed).all().item<bool>() && nmse <= 5e-5 && maximum <= 1e-3;
    std::cout << std::setprecision(12) << "{\"event\":\"explicit_mixed_attention_check\",\"case\":\"" << name
              << "\",\"compared_elements\":" << expected.numel() << ",\"nmse\":" << nmse
              << ",\"maximum_absolute_error\":" << maximum
              << ",\"experimental_fused_kernel\":false,\"passed\":" << (passed ? "true" : "false") << "}\n" << std::flush;
    if (!passed) throw std::runtime_error("explicit mixed attention disagrees with complete rounded-input double reference");
}
}
int main(int argc, char** argv) {
    try {
        if (argc != 2 || std::string(argv[1]) != "rocm" || !at::globalContext().hasROCM())
            throw std::invalid_argument("usage: uta-libtorch-attention-partition-check rocm");
        c10::InferenceMode inference;
        at::set_num_threads(2);
        at::globalContext().setSDPUseMath(false);
        const auto device = at::Device(at::kCUDA, 0);
        const double scale = 0.125;
        for (const auto& shape : std::vector<std::array<int64_t, 6>>{
                 {5, 4, 263, 397, 64, 64}, {1, 8, 1001, 1001, 64, 64}, {2, 4, 61, 61, 128, 64}}) {
            const auto batch = shape[0], heads = shape[1], rows = shape[2], keys = shape[3];
            const auto width = shape[4], output_width = shape[5];
            const auto query = fixture({batch, heads, rows, width}, 0.31);
            const auto key = fixture({batch, heads, keys, width}, 0.97);
            const auto value = fixture({batch, heads, keys, output_width}, 1.73);
            const auto expected = reference_attention(query, key, value, {}, false, false, scale);
            const auto actual = uta::torch_native::partitioned_mixed_attention(
                query.to(device), key.to(device), value.to(device), scale, [] {});
            verify("roformer_complete_context", actual, expected);
        }
        {
            const auto query = fixture({1, 4, 23, 64}, 0.17);
            const auto key = fixture({1, 4, 31, 64}, 0.71);
            const auto value = fixture({1, 4, 31, 64}, 1.37);
            auto rows = at::arange(23, at::kLong).unsqueeze(-1);
            auto columns = at::arange(31, at::kLong).unsqueeze(0);
            auto mask = at::where(at::remainder(rows + columns, 4) != 0, 0.0, -10000.0).to(at::kFloat);
            const auto expected = reference_attention(query, key, value, mask, false, false, scale);
            const auto actual = uta::torch_native::explicit_mixed_attention(
                query.to(device), key.to(device), value.to(device), mask.to(device), false, false, scale);
            verify("game_additive_mask", actual, expected);
        }
        {
            const auto query = fixture({1, 8, 17, 64}, 0.23);
            const auto key = fixture({1, 2, 29, 64}, 0.89);
            const auto value = fixture({1, 2, 29, 64}, 1.61);
            auto rows = at::arange(17, at::kLong).unsqueeze(-1);
            auto columns = at::arange(29, at::kLong).unsqueeze(0);
            auto mask = columns <= rows + 12;
            const auto expected = reference_attention(query, key, value, mask, false, true, scale);
            const auto actual = uta::torch_native::explicit_mixed_attention(
                query.to(device), key.to(device), value.to(device), mask.to(device), false, true, scale);
            verify("qwen_boolean_mask_grouped_query", actual, expected);
        }
        {
            const auto query = fixture({1, 2, 37, 64}, 0.41);
            const auto key = fixture({1, 2, 37, 64}, 1.03);
            const auto value = fixture({1, 2, 37, 64}, 1.91);
            const auto expected = reference_attention(query, key, value, {}, true, false, scale);
            const auto actual = uta::torch_native::explicit_mixed_attention(
                query.to(device), key.to(device), value.to(device), {}, true, false, scale);
            verify("causal_attention", actual, expected);
        }
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
