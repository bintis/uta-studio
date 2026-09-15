#pragma once
#include <cstdint>
#include <ATen/ATen.h>
#include <algorithm>
#include <cmath>
#include <stdexcept>

namespace uta::torch_native {
// Scheduling helpers for the XPU RoFormer plan. They use ordinary FP32 ATen
// operations; CPU tensors are accepted only so the same code has a host oracle.
// The caller must complete submitted device work in `complete` before it
// returns. Temporary tensors deliberately remain in scope through that call.
// No retries, reduced precision, reduced contraction, or shortened context.
// Source review of the 2026-09-15 producer trace found that Strict XPU bypassed
// the ROCm-only batch/FFN bounds. These bounds describe temporary storage, not
// a power-limit workaround or a claim that the host-reset cause is established.
inline int64_t roformer_projection_rows(int64_t input_width, int64_t output_width) {
    if (input_width <= 0 || output_width <= 0)
        throw std::invalid_argument("RoFormer projection widths must be positive");
    constexpr int64_t workspace_elements = 4 * 1024 * 1024;
    return std::max<int64_t>(1, std::min<int64_t>(2048, workspace_elements / std::max(input_width, output_width)));
}
inline int64_t roformer_batch_tile(int64_t length, int64_t projected_width) {
    if (length <= 0 || projected_width <= 0)
        throw std::invalid_argument("RoFormer sequence and projected widths must be positive");
    constexpr int64_t workspace_elements = 4 * 1024 * 1024;
    // Short frequency sequences can share a batch; long time sequences cannot.
    // Unlike the earlier ROCm workaround, do not multiply tiny dispatches by
    // blindly applying one fixed small batch/row count to both axes.
    return std::max<int64_t>(1, std::min<int64_t>(64, workspace_elements / projected_width / length));
}
inline void check_roformer_linear(const at::Tensor& input, const at::Tensor& weight,
                                   const at::Tensor& bias) {
    if (input.dim() < 2 || weight.dim() != 2 || input.size(-1) <= 0 || weight.size(0) <= 0 ||
        input.size(-1) != weight.size(1) || input.scalar_type() != at::kFloat ||
        weight.scalar_type() != at::kFloat || input.device() != weight.device())
        throw std::invalid_argument("RoFormer projection requires compatible FP32 inputs and weights");
    if (bias.defined() && (bias.dim() != 1 || bias.size(0) != weight.size(0) ||
        bias.scalar_type() != at::kFloat || bias.device() != input.device()))
        throw std::invalid_argument("RoFormer projection bias does not match its output");
}

// Linear and RMS/FFN operations are independent per row. For transposed input,
// flattening first with reshape can copy the entire chunk before the first tile.
// Visit narrow views instead. Only contiguous input uses a zero-copy flat view;
// noncontiguous leading axes are enumerated without packing their storage.
template<class Apply>
void for_roformer_row_tiles(const at::Tensor& input, int64_t tile_rows, Apply apply) {
    if (input.dim() < 2 || input.size(-1) <= 0 || tile_rows <= 0)
        throw std::invalid_argument("RoFormer row traversal requires a matrix layout and positive tile size");
    const auto rows = input.numel() / input.size(-1);
    if (rows == 0) return;
    const auto visit = [&](const at::Tensor& matrix, int64_t base) {
        for (int64_t start = 0; start < matrix.size(0);) {
            const auto count = std::min(tile_rows, matrix.size(0) - start);
            apply(matrix.narrow(0, start, count), base + start, count);
            start += count;
        }
    };
    if (input.dim() == 2) {
        visit(input, 0);
    } else if (input.is_contiguous()) {
        visit(input.view({rows, input.size(-1)}), 0);
    } else {
        const auto matrix_rows = input.size(-2);
        for (int64_t block = 0; block < rows / matrix_rows; ++block) {
            auto matrix = input;
            auto remaining = block;
            for (int64_t axis = input.dim() - 3; axis >= 0; --axis) {
                matrix = matrix.select(axis, remaining % input.size(axis));
                remaining /= input.size(axis);
            }
            visit(matrix, block * matrix_rows);
        }
    }
}

template<class Check, class Complete>
void bounded_roformer_linear_into(at::Tensor output, const at::Tensor& input,
                                  const at::Tensor& weight, const at::Tensor& bias,
                                  Check check, Complete complete) {
    check_roformer_linear(input, weight, bias);
    auto shape = input.sizes().vec();
    shape.back() = weight.size(0);
    if (output.sizes() != at::IntArrayRef(shape) || !output.is_contiguous() ||
        output.device() != input.device() || output.scalar_type() != at::kFloat)
        throw std::invalid_argument("RoFormer projection destination does not match its input");
    const auto rows = input.numel() / input.size(-1);
    auto destination = output.reshape({rows, weight.size(0)});
    const auto tile = roformer_projection_rows(input.size(-1), weight.size(0));
    for_roformer_row_tiles(input, tile, [&](const at::Tensor& row_view, int64_t start, int64_t count) {
        check();
        // Only this row view can require internal packing by ATen. The full
        // contraction width and FP32 arithmetic remain unchanged.
        auto projected = at::linear(row_view, weight, bias);
        destination.narrow(0, start, count).copy_(projected);
        complete("projection", start, count);
    });
}

template<class Check, class Complete>
at::Tensor bounded_roformer_linear(const at::Tensor& input, const at::Tensor& weight,
                                   const at::Tensor& bias, Check check, Complete complete) {
    check_roformer_linear(input, weight, bias);
    auto shape = input.sizes().vec();
    shape.back() = weight.size(0);
    auto output = at::empty(shape, input.options());
    bounded_roformer_linear_into(output, input, weight, bias, check, complete);
    return output;
}

template<class Normalize, class Check, class Begin, class Complete>
at::Tensor bounded_roformer_feed_forward(const at::Tensor& input, Normalize normalize,
                                         const at::Tensor& input_weight, const at::Tensor& input_bias,
                                         const at::Tensor& output_weight, const at::Tensor& output_bias,
                                         Check check, Begin begin, Complete complete) {
    check_roformer_linear(input, input_weight, input_bias);
    if (output_weight.dim() != 2 || output_weight.size(1) != input_weight.size(0) ||
        output_weight.size(0) <= 0 || output_weight.scalar_type() != at::kFloat ||
        output_weight.device() != input.device())
        throw std::invalid_argument("RoFormer feed-forward hidden/output weights disagree");
    if (output_bias.defined() && (output_bias.dim() != 1 || output_bias.size(0) != output_weight.size(0) ||
        output_bias.scalar_type() != at::kFloat || output_bias.device() != input.device()))
        throw std::invalid_argument("RoFormer feed-forward output bias is incompatible");
    const auto rows = input.numel() / input.size(-1);
    check();
    begin("allocation", 0, rows);
    auto output = at::empty({rows, output_weight.size(0)}, input.options());
    const auto tile = std::min(roformer_projection_rows(input.size(-1), input_weight.size(0)),
                               roformer_projection_rows(output_weight.size(1), output_weight.size(0)));
    for_roformer_row_tiles(input, tile, [&](const at::Tensor& row_view, int64_t start, int64_t count) {
        check();
        // RMS normalization is per row. Normalize inside the tile as well;
        // retaining a whole-chunk normalized/hidden tensor defeats the bound.
        // These markers describe host submission intent, not device completion.
        // Keep the existing end-of-tile wait; do not add per-operator GPU fences.
        begin("normalization", start, count);
        auto normalized = normalize(row_view);
        begin("input_projection", start, count);
        auto hidden = at::linear(normalized, input_weight, input_bias);
        begin("gelu", start, count);
        auto activated = at::gelu(hidden, "none");
        begin("output_projection", start, count);
        auto projected = at::linear(activated, output_weight, output_bias);
        begin("output_copy", start, count);
        output.narrow(0, start, count).copy_(projected);
        complete("feed_forward", start, count);
    });
    auto shape = input.sizes().vec();
    shape.back() = output_weight.size(0);
    return output.reshape(shape);
}

// Partition only independent sequences, never their time/frequency context.
// This covers normalization, projections, RoPE, attention, gates and residuals,
// not just the score matrix at the center of the attention block.
template<class Attend, class Check, class Complete>
at::Tensor bounded_roformer_batches(const at::Tensor& sequence, int64_t projected_width, Attend attend,
                                    Check check, Complete complete) {
    if (sequence.dim() != 3 || sequence.size(1) <= 0 || sequence.size(2) <= 0)
        throw std::invalid_argument("RoFormer attention requires [batch,sequence,channel] input");
    const auto batch_tile = roformer_batch_tile(sequence.size(1), std::max(projected_width, sequence.size(2)));
    auto output = at::empty(sequence.sizes(), sequence.options());
    for (int64_t start = 0; start < sequence.size(0);) {
        check();
        const auto count = std::min(batch_tile, sequence.size(0) - start);
        auto attended = attend(sequence.narrow(0, start, count));
        output.narrow(0, start, count).copy_(attended);
        complete("batch", start, count);
        start += count;
    }
    return output;
}

inline int64_t roformer_query_tile(int64_t groups, int64_t keys) {
    if (groups <= 0 || keys <= 0) throw std::invalid_argument("RoFormer attention requires nonempty groups and keys");
    constexpr int64_t score_elements = 4 * 1024 * 1024; // 16 MiB for one FP32 score tile
    // Always retain at least one complete softmax row, even for an unusually
    // large model. This bounds scratch scheduling, not the accepted context.
    return std::max<int64_t>(1, std::min<int64_t>(64, score_elements / groups / keys));
}

template<class Check, class Complete>
at::Tensor bounded_roformer_strict_attention(const at::Tensor& query, const at::Tensor& key,
                                             const at::Tensor& value, double scale,
                                             Check check, Complete complete) {
    if (query.dim() != 4 || key.dim() != 4 || value.dim() != 4 ||
        query.size(0) != key.size(0) || key.size(0) != value.size(0) ||
        query.size(1) != key.size(1) || key.size(1) != value.size(1) ||
        query.size(3) != key.size(3) || key.size(2) != value.size(2) ||
        query.device() != key.device() || key.device() != value.device() ||
        query.scalar_type() != at::kFloat || key.scalar_type() != at::kFloat || value.scalar_type() != at::kFloat)
        throw std::invalid_argument("RoFormer strict attention requires compatible FP32 [batch,head,row,channel] tensors");
    for (const auto& tensor : {query, key, value})
        for (auto dimension : tensor.sizes())
            if (dimension <= 0) throw std::invalid_argument("RoFormer attention dimensions must be positive");
    const auto groups = query.size(0) * query.size(1), rows = query.size(2), keys = key.size(2);
    const auto tile = roformer_query_tile(groups, keys);
    check();
    // Head-interleaved Q/K/V cannot in general flatten batch+head as a view.
    // Pack once per bounded batch, rather than letting each matmul's broadcast
    // reshape copy the complete K/V again for every query tile.
    auto queries = query.contiguous().view({groups, rows, query.size(3)});
    auto key_rows = key.contiguous().view({groups, keys, key.size(3)});
    auto values = value.contiguous().view({groups, keys, value.size(3)});
    auto output = at::empty({groups, rows, value.size(3)}, query.options());
    complete("attention_pack", 0, groups);
    for (int64_t start = 0; start < rows;) {
        check();
        const auto count = std::min(tile, rows - start);
        auto scores = at::bmm(queries.narrow(1, start, count), key_rows.transpose(1, 2));
        scores.mul_(scale);
        // RoFormer is unmasked/noncausal, including PolarFormer's unequal Q/V
        // widths. No half rounding, fused-SDPA substitution or K/V truncation.
        auto probabilities = at::softmax(scores, -1);
        // Preserve the prior strict path's zero convention for an all -inf
        // score row; workspace tiling must not remove its numerical guard.
        probabilities = at::where(at::isneginf(scores).all(-1, true), at::zeros_like(probabilities), probabilities);
        auto attended = at::bmm(probabilities, values);
        output.narrow(1, start, count).copy_(attended);
        complete("attention_query", start, count);
        start += count;
    }
    return output.view({query.size(0), query.size(1), rows, value.size(3)});
}
} // namespace uta::torch_native
