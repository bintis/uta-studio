#include "roformer_bounded.hpp"
#include <ATen/Parallel.h>
#include <c10/core/InferenceMode.h>
#include <functional>
#include <iostream>
#include <limits>
#include <string>
#include <vector>

namespace {
void require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}
at::Tensor fixture(at::IntArrayRef shape, double phase) {
    int64_t count = 1;
    for (auto size : shape) count *= size;
    return (at::sin(at::arange(count, at::kDouble) * 0.037 + phase) * 0.3).reshape(shape).to(at::kFloat);
}
void compare(const at::Tensor& actual, const at::Tensor& expected) {
    require(actual.sizes() == expected.sizes(), "bounded RoFormer changed output shape");
    require(at::isfinite(actual).all().item<bool>(), "bounded RoFormer produced nonfinite values");
    auto error = actual.to(at::kDouble) - expected.to(at::kDouble);
    require(error.abs().max().item<double>() < 2e-5, "bounded RoFormer differs from complete double oracle");
}
struct Completion {
    int64_t rows = 0, calls = 0;
    void operator()(const char*, int64_t start, int64_t count) {
        require(start == rows && count > 0, "tile skips, repeats, or reorders rows");
        rows += count;
        ++calls;
    }
};

void projection(bool strided, bool bias_present) {
    auto input = fixture({3, 771, 34}, 0.3);
    input = strided ? input.slice(-1, 0, 34, 2) : input.narrow(-1, 0, 17).contiguous();
    auto weight = fixture({17, 31}, 0.7).transpose(0, 1);
    auto bias = bias_present ? fixture({31}, 1.1) : at::Tensor();
    auto before = input.clone();
    auto expected = at::matmul(input.to(at::kDouble), weight.to(at::kDouble).transpose(0, 1));
    if (bias.defined()) expected = expected + bias.to(at::kDouble);
    Completion finished;
    auto actual = uta::torch_native::bounded_roformer_linear(input, weight, bias, [] {}, std::ref(finished));
    compare(actual, expected);
    require(finished.rows == 2313 && finished.calls > 1, "projection did not complete its non-divisible tail");
    require(at::equal(input, before), "projection changed caller input");
}

void band_destination() {
    auto features = fixture({2067, 51}, 0.4);
    auto weights = fixture({3, 11, 17}, 0.8);
    auto biases = fixture({3, 11}, 1.2);
    auto projected = at::empty({3, 2067, 11}, features.options());
    std::vector<at::Tensor> expected;
    for (int64_t band = 0; band < 3; ++band) {
        auto input = features.narrow(1, band * 17, 17);
        uta::torch_native::bounded_roformer_linear_into(projected.select(0, band), input,
            weights.select(0, band), biases.select(0, band), [] {}, [](const char*, int64_t, int64_t) {});
        expected.push_back(at::linear(input.to(at::kDouble), weights.select(0, band).to(at::kDouble),
                                       biases.select(0, band).to(at::kDouble)));
    }
    compare(projected.transpose(0, 1), at::stack(expected, 1));
}

void feed_forward() {
    auto input = fixture({3, 771, 34}, 0.5).slice(-1, 0, 34, 2);
    auto before = input.clone();
    auto norm_weight = fixture({17}, 0.8);
    auto input_weight = fixture({131, 17}, 1.2), input_bias = fixture({131}, 1.4);
    auto output_weight = fixture({17, 131}, 1.6), output_bias = fixture({17}, 1.8);
    auto expected_input = input.to(at::kDouble);
    auto normalized = expected_input * at::rsqrt(expected_input.square().mean(-1, true) + 1e-12) * norm_weight.to(at::kDouble);
    auto expected = at::linear(at::gelu(at::linear(normalized, input_weight.to(at::kDouble), input_bias.to(at::kDouble)), "none"),
                               output_weight.to(at::kDouble), output_bias.to(at::kDouble));
    int64_t normalized_rows = 0;
    Completion finished;
    auto actual = uta::torch_native::bounded_roformer_feed_forward(input,
        [&](const at::Tensor& rows) {
            require(rows.dim() == 2 && rows.size(0) <= 2048, "normalization allocated the full feed-forward sequence");
            require(normalized_rows == finished.rows, "next hidden tile began before previous completion");
            normalized_rows += rows.size(0);
            return rows * at::rsqrt(rows.square().mean(-1, true) + 1e-12) * norm_weight;
        }, input_weight, input_bias, output_weight, output_bias, [] {}, std::ref(finished));
    compare(actual, expected);
    require(finished.rows == 2313 && finished.calls > 1, "feed-forward lost the final row tile");
    require(at::equal(input, before), "feed-forward changed its residual input");
}

void independent_batches() {
    auto input = fixture({129, 65, 16}, 0.4).transpose(0, 1).contiguous().transpose(0, 1);
    auto original = input.clone();
    Completion finished;
    int64_t invoked = 0;
    // The mean depends on every sequence row; splitting its context changes
    // this oracle, whereas partitioning independent batches does not.
    auto actual = uta::torch_native::bounded_roformer_batches(input, 16,
        [&](const at::Tensor& tile) {
            require(tile.size(0) <= 64 && tile.size(1) == 65, "attention shortened context or kept all independent batches");
            require(invoked == finished.calls, "next attention batch started before completion");
            ++invoked;
            return tile + tile.mean(1, true);
        }, [] {}, std::ref(finished));
    compare(actual, input.to(at::kDouble) + input.to(at::kDouble).mean(1, true));
    require(finished.rows == 129 && finished.calls == 3, "attention dropped its final partial batch");
    require(at::equal(input, original), "attention changed caller residual storage");
}

void attention(int64_t batch, int64_t rows, int64_t keys, bool polar) {
    const int64_t heads = 3, value_width = 8, query_width = polar ? 16 : 8;
    auto query_storage = fixture({batch, rows, heads * query_width * 3}, 0.3);
    auto key_storage = fixture({batch, keys, heads * query_width * 3}, 0.9);
    auto value_storage = fixture({batch, keys, heads * value_width * 3}, 1.5);
    // Views of head-interleaved QKV storage, including nonzero offsets. These
    // cannot flatten batch and head as a view when there is more than one batch.
    auto query = query_storage.narrow(-1, heads * query_width, heads * query_width)
        .reshape({batch, rows, heads, query_width}).transpose(1, 2);
    auto key = key_storage.narrow(-1, heads * query_width, heads * query_width)
        .reshape({batch, keys, heads, query_width}).transpose(1, 2);
    auto value = value_storage.narrow(-1, heads * value_width * 2, heads * value_width)
        .reshape({batch, keys, heads, value_width}).transpose(1, 2);
    auto original_query = query.clone(), original_key = key.clone(), original_value = value.clone();
    const double scale = 1.0 / std::sqrt(static_cast<double>(value_width));
    auto expected = at::matmul(at::softmax(at::matmul(query.to(at::kDouble), key.to(at::kDouble).transpose(-1, -2)) * scale, -1),
                               value.to(at::kDouble));
    int64_t packed = 0, completed_rows = 0, checks = 0, tiles = 0;
    auto actual = uta::torch_native::bounded_roformer_strict_attention(query, key, value, scale,
        [&] { require(checks == packed + tiles, "attention submitted another tile before completing the previous work"); ++checks; },
        [&](const char* phase, int64_t start, int64_t count) {
            if (std::string(phase) == "attention_pack") {
                require(packed == 0 && start == 0 && count == batch * heads, "attention repacked operands inside its query loop");
                ++packed;
            } else {
                require(packed == 1 && start == completed_rows, "attention changed query order");
                require(count <= uta::torch_native::roformer_query_tile(batch * heads, keys), "attention exceeded score workspace");
                completed_rows += count;
                ++tiles;
            }
        });
    compare(actual, expected);
    require(completed_rows == rows && packed == 1 && checks == 1 + tiles, "attention did not complete every output row");
    require(at::equal(query, original_query) && at::equal(key, original_key) && at::equal(value, original_value),
            "strict attention modified Q/K/V inputs");
}

void negative_infinity_row_keeps_zero_output() {
    auto options = at::TensorOptions().dtype(at::kFloat).device(at::kCPU);
    auto query = at::full({1, 1, 1, 1}, -std::numeric_limits<float>::max(), options);
    auto key = at::full({1, 1, 2, 1}, std::numeric_limits<float>::max(), options);
    auto value = at::ones({1, 1, 2, 1}, options);
    auto actual = uta::torch_native::bounded_roformer_strict_attention(query, key, value, 1.0,
        [] {}, [](const char*, int64_t, int64_t) {});
    require(at::equal(actual, at::zeros_like(actual)), "strict attention lost its all-negative-infinity row guard");
}

void failures_stop_submission() {
    auto input = fixture({4099, 17}, 0.5), weight = fixture({31, 17}, 0.9);
    auto output = at::full({4099, 31}, std::numeric_limits<float>::quiet_NaN(), input.options());
    int completed = 0;
    bool failed = false;
    try {
        uta::torch_native::bounded_roformer_linear_into(output, input, weight, {}, [] {},
            [&](const char*, int64_t, int64_t) { ++completed; throw std::runtime_error("completion failure"); });
    } catch (const std::runtime_error& error) { failed = std::string(error.what()) == "completion failure"; }
    require(failed && completed == 1, "completion failure retried or fell back");
    require(at::isnan(output.narrow(0, 2048, 2051)).all().item<bool>(), "another projection ran after completion failure");
    completed = 0;
    bool cancelled = false;
    try {
        uta::torch_native::bounded_roformer_linear(input, weight, {},
            [&] { if (completed) throw std::runtime_error("cancelled"); },
            [&](const char*, int64_t, int64_t) { ++completed; });
    } catch (const std::runtime_error& error) { cancelled = std::string(error.what()) == "cancelled"; }
    require(cancelled && completed == 1, "cancellation did not stop between tiles");
}

void row_tiles_are_views_not_full_chunk_copies() {
    auto base = fixture({2, 3, 7, 34}, 0.4);
    for (const auto& input : std::vector<at::Tensor>{
             base.select(0, 0).select(0, 0).slice(-1, 0, 34, 2),
             base.select(0, 0).transpose(0, 1).slice(-1, 0, 34, 2),
             base.transpose(0, 2).slice(-1, 0, 34, 2),
             base.contiguous(), base.narrow(1, 0, 0)}) {
        const auto rows = input.numel() / input.size(-1);
        // The independent oracle may pack; the production traversal must not.
        auto expected = input.reshape({rows, input.size(-1)});
        int64_t completed = 0;
        uta::torch_native::for_roformer_row_tiles(input, 5,
            [&](const at::Tensor& tile, int64_t start, int64_t count) {
                require(tile.is_alias_of(input), "row traversal copied storage before tiling");
                require(start == completed && count > 0 && count <= 5, "row traversal lost its bound or order");
                require(at::equal(tile, expected.narrow(0, start, count)), "transposed row traversal changed logical order");
                completed += count;
            });
        require(completed == rows, "row traversal lost a leading axis or empty/tail case");
    }
}

void repeated_attention_preserves_prior_results() {
    // Exercise the fourth call and return to an earlier shape. Retain all
    // outputs so accidental scratch reuse would corrupt a previous result.
    // This is an operator ownership oracle, not a simulated GPU power loss.
    std::vector<at::Tensor> results, expected_results;
    int64_t pass = 0;
    for (const int64_t rows : {65, 65, 129, 63, 65}) {
        auto query = fixture({2, rows, 3, 8}, 0.3 + pass).transpose(1, 2);
        auto key = fixture({2, rows, 3, 8}, 0.7 + pass).transpose(1, 2);
        auto value = fixture({2, rows, 3, 8}, 1.1 + pass).transpose(1, 2);
        const double scale = 1.0 / std::sqrt(8.0);
        auto expected = at::matmul(at::softmax(
            at::matmul(query.to(at::kDouble), key.to(at::kDouble).transpose(-1, -2)) * scale, -1),
            value.to(at::kDouble));
        auto actual = uta::torch_native::bounded_roformer_strict_attention(query, key, value, scale,
            [] {}, [](const char*, int64_t, int64_t) {});
        require(!actual.is_alias_of(query) && !actual.is_alias_of(key) && !actual.is_alias_of(value),
                "attention output aliases a caller input");
        results.push_back(actual);
        expected_results.push_back(expected);
        for (std::size_t index = 0; index < results.size(); ++index) compare(results[index], expected_results[index]);
        ++pass;
    }
}

void production_workspace_geometry() {
    constexpr int64_t maximum_scores = 4 * 1024 * 1024;
    // Check the full production axis sizes without allocating/running a full
    // model. The numeric attention fixtures above exercise the real helper.
    for (const int64_t keys : {90, 801, 1722}) {
        const auto batches = uta::torch_native::roformer_batch_tile(keys, 512);
        const auto groups = batches * 8;
        require(batches * keys * 512 <= maximum_scores, "complete attention block exceeded projection workspace");
        const auto rows = uta::torch_native::roformer_query_tile(groups, keys);
        require(rows > 0 && rows <= 64 && groups * rows * keys <= maximum_scores,
                "RoFormer score workspace still scales with all bands/frames at once");
    }
}
} // namespace
int main() {
    try {
        c10::InferenceMode inference;
        at::set_num_threads(2);
        for (bool strided : {false, true}) for (bool bias : {false, true}) projection(strided, bias);
        band_destination();
        feed_forward();
        independent_batches();
        for (const int64_t rows : {1, 63, 64, 65, 129}) {
            attention(1, rows, rows, false);
            attention(3, rows, rows + 7, false);
            attention(3, rows, rows + 7, true);
        }
        negative_infinity_row_keeps_zero_output();
        failures_stop_submission();
        row_tiles_are_views_not_full_chunk_copies();
        repeated_attention_preserves_prior_results();
        production_workspace_geometry();
        std::cout << "Bounded RoFormer FP32, layout, complete-context, tail and completion checks passed (CPU oracle only)\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
