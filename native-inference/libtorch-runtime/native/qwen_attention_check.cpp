#include "qwen_attention.hpp"
#include "qwen_strict_attention_check.hpp"
#include <ATen/Parallel.h>
#include <c10/core/InferenceMode.h>
#include <cmath>
#include <iostream>
#include <limits>
#include <string>

namespace {
at::Tensor fixture(int64_t heads, int64_t rows, double phase) {
    // Match Qwen's transposed, noncontiguous [batch, head, row, channel] views.
    return at::sin(at::arange(2 * heads * rows * 8, at::kDouble) * 0.037 + phase)
        .to(at::kFloat).reshape({2, rows, heads, 8}).transpose(1, 2);
}
at::Tensor reference(const at::Tensor& query, const at::Tensor& key,
                     const at::Tensor& value, const at::Tensor& mask) {
    auto keys = at::repeat_interleave(key.to(at::kDouble), query.size(1) / key.size(1), 1);
    auto values = at::repeat_interleave(value.to(at::kDouble), query.size(1) / value.size(1), 1);
    auto scores = at::matmul(query.to(at::kDouble), keys.transpose(-1, -2)) /
        std::sqrt(static_cast<double>(query.size(3)));
    if (mask.defined()) scores.masked_fill_(mask.logical_not(), -std::numeric_limits<double>::infinity());
    return at::matmul(at::softmax(scores, -1), values).to(at::kFloat);
}
void require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}
void verify(const at::Tensor& actual, const at::Tensor& expected) {
    require(actual.sizes() == expected.sizes(), "Qwen attention lost output rows");
    require(at::isfinite(actual).all().item<bool>(), "Qwen attention produced nonfinite values");
    require(at::allclose(actual, expected, 1e-5, 1e-6), "Qwen tiled attention changed visible context or positions");
}
void check_windows(int64_t rows, int64_t window) {
    auto query = fixture(4, rows, 0.1), key = fixture(4, rows, 0.5), value = fixture(4, rows, 0.9);
    auto windows = at::floor_divide(at::arange(rows, at::kLong), window);
    auto expected = reference(query, key, value, windows.unsqueeze(1) == windows.unsqueeze(0));
    int64_t submitted = 0, completed = 0;
    auto actual = uta::torch_native::qwen_window_attention(query, key, value, window,
        [&](const at::Tensor& current_query, const at::Tensor& current_key,
            const at::Tensor& current_value, const at::Tensor& mask) {
            require(!mask.defined(), "encoder still allocates the global window mask");
            require(current_query.size(2) <= window && current_key.size(2) == current_query.size(2),
                    "encoder crossed an acoustic window boundary");
            ++submitted;
            return uta::torch_native::qwen_strict_checks::execute(current_query, current_key, current_value, mask);
        }, [&] { require(submitted == completed, "encoder queued a window before completing its predecessor"); },
        [&] { ++completed; });
    require(completed == (rows + window - 1) / window, "encoder did not complete every window including its tail");
    verify(actual, expected);
}
void check_causal(int64_t rows, int64_t past) {
    auto query = fixture(4, rows, 0.3), key = fixture(2, past + rows, 0.7), value = fixture(2, past + rows, 1.1);
    auto positions = at::arange(past, past + rows, at::kLong);
    auto columns = at::arange(past + rows, at::kLong);
    auto expected = reference(query, key, value, positions.unsqueeze(1) >= columns.unsqueeze(0));
    int64_t submitted = 0, completed = 0;
    auto actual = uta::torch_native::qwen_causal_attention(query, key, value, past,
        [&](const at::Tensor& current_query, const at::Tensor& current_key,
            const at::Tensor& current_value, const at::Tensor& mask) {
            require(current_query.size(2) <= 64 && mask.size(0) == current_query.size(2),
                    "decoder allocated a prompt-sized attention mask");
            require(mask.size(1) == current_key.size(2), "decoder mask does not cover its complete causal history");
            ++submitted;
            return uta::torch_native::qwen_strict_checks::execute(current_query, current_key, current_value, mask);
        }, [&] { require(submitted == completed, "decoder queued work before tile completion"); },
        [&] { ++completed; });
    require(completed == (rows + 63) / 64, "decoder did not complete every query tile");
    verify(actual, expected);
}
void check_cancellation_and_invalid_cache() {
    auto query = fixture(4, 65, 0.2), key = fixture(2, 65, 0.6), value = fixture(2, 65, 1.0);
    int completed = 0;
    bool cancelled = false;
    try {
        uta::torch_native::qwen_causal_attention(query, key, value, 0, reference,
            [&] { if (completed == 1) throw std::runtime_error("cancelled fixture"); },
            [&] { ++completed; });
    } catch (const std::runtime_error& error) {
        cancelled = std::string(error.what()) == "cancelled fixture";
    }
    require(cancelled && completed == 1, "cancellation did not stop before submitting the next query tile");
    bool rejected = false;
    try {
        uta::torch_native::qwen_causal_attention(query, key, value, 1, reference, [] {}, [] {});
    } catch (const std::invalid_argument&) { rejected = true; }
    require(rejected, "decoder accepted an inconsistent resident cache position");
}
} // namespace
int main() {
    try {
        c10::InferenceMode inference;
        at::set_num_threads(2);
        for (const int64_t rows : {1, 13, 103, 104, 105, 221}) check_windows(rows, 104);
        for (const int64_t rows : {1, 63, 64, 65, 129}) {
            check_causal(rows, 0);   // ASR prefill / aligner classification
            check_causal(rows, 37);  // resident-cache continuation, including a single token
        }
        check_cancellation_and_invalid_cache();
        uta::torch_native::qwen_strict_checks::run();
        std::cout << "Qwen production strict attention, window/cache, masks, ownership and completion checks passed (CPU oracle only)\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
