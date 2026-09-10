#pragma once
#include <ATen/ATen.h>

namespace uta::torch_native {
// Ordinary RoPE stores adjacent real/imaginary coordinates. Reinterpret that
// pair without packing QKV's strided view; one complex multiply replaces four
// products, two sums and a stack. Phase and arithmetic remain complex-FP32.
// PolarFormer is not RoPE and must keep its own softplus/phase transformation.
inline at::Tensor interleaved_roformer_rotation(const at::Tensor& input, const at::Tensor& phase) {
    auto shape = input.sizes().vec();
    shape.back() /= 2;
    shape.push_back(2);
    auto paired = at::view_as_complex(input.reshape(shape));
    return at::view_as_real(paired * phase).flatten(-2);
}
// oneDNN SDPA supports dense head-interleaved tensors. Preserve the layout
// produced by rotation/QKV conversion instead of copying every operand to BHLD;
// the returned interleaved layout also avoids repacking before the output GEMM.
inline at::Tensor layout_preserving_roformer_attention(const at::Tensor& query, const at::Tensor& key,
                                                      const at::Tensor& value, double scale) {
    return at::scaled_dot_product_attention(query.to(at::kHalf), key.to(at::kHalf), value.to(at::kHalf),
        {}, 0.0, false, scale).to(at::kFloat);
}
} // namespace uta::torch_native
