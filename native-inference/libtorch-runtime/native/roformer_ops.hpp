#pragma once
#include <ATen/ATen.h>

namespace uta::torch_native {
// Keep the checkpoint's epsilon and FP32 reduction; avoid materializing square,
// scaled-input and affine intermediates for every transformer normalization.
inline at::Tensor fused_roformer_normalization(const at::Tensor& input, const at::Tensor& weight) {
    return std::get<0>(at::_fused_rms_norm(input, {input.size(-1)}, weight, 1e-12));
}

// Ordinary RoPE stores adjacent real/imaginary coordinates. Reinterpret that
// pair without packing QKV's strided view; one complex multiply replaces four
// products, two sums and a stack. Phase and arithmetic remain complex-FP32.
// PolarFormer is not RoPE and must keep its own softplus/phase transformation.
inline at::Tensor interleaved_roformer_rotation(const at::Tensor& input, const at::Tensor& phase) {
    auto paired = at::view_as_complex(input.unflatten(-1, {input.size(-1) / 2, 2}));
    return at::view_as_real(paired * phase).flatten(-2);
}
// oneDNN SDPA supports dense head-interleaved tensors. Preserve the layout
// produced by rotation/QKV conversion instead of copying every operand to BHLD;
// the returned interleaved layout also avoids repacking before the output GEMM.
template<class Checkpoint>
inline at::Tensor layout_preserving_roformer_attention(const at::Tensor& query, const at::Tensor& key,
                                                      const at::Tensor& value, double scale, Checkpoint&& checkpoint) {
    auto query_half = query.to(at::kHalf);
    checkpoint("query_conversion");
    auto key_half = key.to(at::kHalf);
    checkpoint("key_conversion");
    auto value_half = value.to(at::kHalf);
    checkpoint("value_conversion");
    auto attended = at::scaled_dot_product_attention(query_half, key_half, value_half, {}, 0.0, false, scale);
    checkpoint("sdpa");
    auto output = attended.to(at::kFloat);
    checkpoint("attention_output_conversion");
    return output;
}
inline at::Tensor layout_preserving_roformer_attention(const at::Tensor& query, const at::Tensor& key,
                                                      const at::Tensor& value, double scale) {
    return layout_preserving_roformer_attention(query, key, value, scale, [](const char*) {});
}
} // namespace uta::torch_native
