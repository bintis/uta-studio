#pragma once
#include "qwen_strict_attention.hpp"
#include <string>
#include <tuple>
#include <vector>

namespace uta::torch_native::qwen_strict_checks {
inline void require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}
inline at::Tensor fixture(int64_t batches, int64_t heads, int64_t rows, int64_t width, double phase) {
    return at::sin(at::arange(batches * heads * rows * width, at::kDouble) * 0.037 + phase)
        .to(at::kFloat).reshape({batches, rows, heads, width}).transpose(1, 2);
}
inline at::Tensor reference(const at::Tensor& query, const at::Tensor& key,
                            const at::Tensor& value, const at::Tensor& mask = {}) {
    // Independent complete-context double oracle: deliberately use the former
    // repeated-head mathematical expression, not the production packing code.
    auto keys = at::repeat_interleave(key.to(at::kDouble), query.size(1) / key.size(1), 1);
    auto values = at::repeat_interleave(value.to(at::kDouble), query.size(1) / value.size(1), 1);
    auto scores = at::matmul(query.to(at::kDouble), keys.transpose(-1, -2)) /
        std::sqrt(static_cast<double>(query.size(3)));
    if (mask.defined()) {
        if (mask.scalar_type() == at::kBool)
            scores.masked_fill_(mask.logical_not(), -std::numeric_limits<double>::infinity());
        else scores.add_(mask.to(at::kDouble));
    }
    auto probabilities = at::softmax(scores, -1);
    probabilities = at::where(at::isneginf(scores).all(-1, true), at::zeros_like(probabilities), probabilities);
    return at::matmul(probabilities, values).to(at::kFloat);
}
inline void verify(const at::Tensor& actual, const at::Tensor& expected) {
    require(actual.sizes() == expected.sizes(), "strict head-group attention changed output geometry");
    require(at::isfinite(actual).all().item<bool>(), "strict head-group attention produced nonfinite output");
    require(at::allclose(actual, expected, 1e-5, 1e-6), "strict head-group attention disagrees with the complete double oracle");
}
inline at::Tensor execute(const at::Tensor& query, const at::Tensor& key,
                           const at::Tensor& value, const at::Tensor& mask = {}) {
    int64_t checks = 0, started = 0, completed = 0;
    std::tuple<std::string, int64_t, int64_t, int64_t, int64_t> active;
    const auto result = qwen_strict_attention(query, key, value, mask,
        [&] {
            require(started == completed, "strict attention advanced before completing the previous operator");
            ++checks;
        }, [&](const char* stage, int64_t batch, int64_t head, int64_t row, int64_t count) {
            require(checks == started + 1, "strict attention submitted an unchecked operator");
            active = {stage, batch, head, row, count};
            if (std::string(stage) == "scores") {
                const auto group = query.size(1) / key.size(1);
                require(count == 1 || count <= (4 * 1024 * 1024) / group / key.size(2),
                        "strict attention exceeded its score scratch schedule");
            }
            ++started;
        }, [&](const char* stage, int64_t batch, int64_t head, int64_t row, int64_t count) {
            require(active == std::make_tuple(std::string(stage), batch, head, row, count),
                    "strict attention completed a different operator");
            require(started == completed + 1, "strict attention completed an operator twice");
            ++completed;
        });
    const auto tile = qwen_strict_query_tile(query.size(1) / key.size(1), key.size(2));
    const auto expected = 1 + query.size(0) * key.size(1) * (1 + 4 * ((query.size(2) + tile - 1) / tile));
    require(checks == expected && started == expected && completed == expected,
            "strict attention lost an operator or a final partial tile");
    require(!result.is_alias_of(query) && !result.is_alias_of(key) && !result.is_alias_of(value),
            "strict attention returned input-owned storage");
    return result;
}
inline void check_cache_and_masks() {
    const int64_t batches = 2, heads = 6, key_heads = 2, rows = 129, keys = 173, width = 8;
    auto query = fixture(batches, heads, rows, width, 0.1);
    // Real session caches have capacity beyond visible rows. Poison the spare
    // capacity, and also exercise nonunit channel strides and a nonzero offset.
    auto key_storage = at::full({batches, key_heads, keys + 23, width * 2},
                                std::numeric_limits<float>::quiet_NaN(), at::kFloat);
    auto value_storage = at::full_like(key_storage, std::numeric_limits<float>::quiet_NaN());
    auto key = key_storage.narrow(2, 3, keys).slice(3, 0, width * 2, 2);
    auto value = value_storage.narrow(2, 3, keys).slice(3, 0, width * 2, 2);
    key.copy_(fixture(batches, key_heads, keys, width, 0.7));
    value.copy_(fixture(batches, key_heads, keys, width, 1.3));
    const auto original_query = query.clone(), original_key = key.clone(), original_value = value.clone();
    auto positions = at::arange(keys - rows, keys, at::kLong);
    auto columns = at::arange(keys, at::kLong);
    auto causal = positions.unsqueeze(1) >= columns.unsqueeze(0);
    causal.select(0, 7).fill_(false);
    auto additive = at::zeros({batches, heads, rows, keys}, at::kFloat);
    additive.masked_fill_(causal.logical_not(), -std::numeric_limits<float>::infinity());
    // Different masks for each batch and query head detect incorrect GQA mask
    // sharing; -infinity rows must stay zero, not become NaN.
    additive.select(0, 1).select(0, 4).narrow(1, 0, 5).fill_(-std::numeric_limits<float>::infinity());
    auto head_mask = at::ones({heads, rows, keys}, at::kBool);
    head_mask.select(0, 2).narrow(1, 0, 9).fill_(false);
    const std::vector<at::Tensor> masks{
        at::Tensor(), columns < keys - 1, causal, head_mask, additive,
        at::zeros({1, keys}, at::kBool), at::zeros({1, 1, rows, 1}, at::kFloat)
    };
    for (const auto& mask : masks) verify(execute(query, key, value, mask), reference(query, key, value, mask));
    require(at::equal(query, original_query) && at::equal(key, original_key) && at::equal(value, original_value),
            "strict attention modified Q/K/V inputs or the resident cache");
    require(at::isnan(key_storage.narrow(2, keys + 3, 20)).all().item<bool>(),
            "strict attention wrote into unused cache capacity");
}
inline void check_geometry_and_ownership() {
    struct Geometry { int64_t batches, heads, key_heads, rows, keys, width, value_width; };
    const Geometry geometries[] = {
        {1, 16, 8, 64, 64, 128, 128},  // representative ASR first prefill tile
        {1, 16, 8, 1, 293, 128, 128},  // single-token session continuation
        {2, 4, 1, 65, 91, 8, 11},     // grouped heads, unequal value width, tail
        {2, 4, 4, 105, 105, 8, 8},    // non-grouped encoder window
        {1, 1, 1, 1, 1, 8, 8}
    };
    for (const auto& shape : geometries) {
        auto query = fixture(shape.batches, shape.heads, shape.rows, shape.width, 0.2);
        auto key = fixture(shape.batches, shape.key_heads, shape.keys, shape.width, 0.8);
        auto value = fixture(shape.batches, shape.key_heads, shape.keys, shape.value_width, 1.4);
        verify(execute(query, key, value), reference(query, key, value));
    }
    auto input = fixture(2, 3, 65, 8, 0.4);
    const auto original = input.clone();
    auto expected = reference(input, input, input);
    auto first = execute(input, input, input);
    for (int repeat = 0; repeat < 3; ++repeat) {
        auto next = execute(input, input, input);
        verify(next, expected);
        verify(first, expected);
        next.zero_();
        require(at::equal(input, original), "strict attention output aliases its shared input");
    }
    verify(first, expected);
}
inline void check_stopping_and_budget() {
    require(qwen_strict_query_tile(2, 64) == 64, "strict attention changed the ordinary query schedule");
    require(qwen_strict_query_tile(16, 8193) == 31, "strict attention ignored the physical head-group scratch size");
    require(qwen_strict_query_tile(16, std::numeric_limits<int64_t>::max()) == 1,
            "strict attention truncated or overflowed a complete key row");
    auto query = fixture(1, 4, 65, 8, 0.2), key = fixture(1, 2, 65, 8, 0.6), value = fixture(1, 2, 65, 8, 1.0);
    for (const auto& phase : {"output_allocate", "kv_pack", "query_pack", "scores", "softmax", "values"}) {
        std::vector<std::string> started;
        bool failed = false;
        try {
            qwen_strict_attention(query, key, value, {}, [] {},
                [&](const char* stage, int64_t, int64_t, int64_t, int64_t) { started.emplace_back(stage); },
                [&](const char* stage, int64_t, int64_t, int64_t, int64_t) {
                    if (std::string(stage) == phase) throw std::runtime_error("completion failure fixture");
                });
        } catch (const std::runtime_error& error) { failed = std::string(error.what()) == "completion failure fixture"; }
        require(failed && !started.empty() && started.back() == phase,
                "strict attention retried or continued after a failed completion");
    }
    int64_t started = 0, completed = 0;
    bool cancelled = false;
    try {
        qwen_strict_attention(query, key, value, {},
            [&] { if (completed == 4) throw std::runtime_error("cancelled fixture"); },
            [&](const char*, int64_t, int64_t, int64_t, int64_t) { ++started; },
            [&](const char*, int64_t, int64_t, int64_t, int64_t) { ++completed; });
    } catch (const std::runtime_error& error) { cancelled = std::string(error.what()) == "cancelled fixture"; }
    require(cancelled && started == 4 && completed == 4, "strict attention submitted another operator after cancellation");
    bool rejected = false;
    try { execute(query, key.narrow(1, 0, 0), value.narrow(1, 0, 0)); }
    catch (const std::invalid_argument&) { rejected = true; }
    require(rejected, "strict attention accepted an empty KV head axis");
}
inline void run() {
    check_cache_and_masks();
    check_geometry_and_ownership();
    check_stopping_and_budget();
}
} // namespace uta::torch_native::qwen_strict_checks
