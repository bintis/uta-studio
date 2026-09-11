#pragma once
#include <ATen/ATen.h>
#include <c10/core/Event.h>
#include <c10/core/impl/VirtualGuardImpl.h>
#include <chrono>
#include <thread>

namespace uta::torch_native {
// Wait only for already-submitted work. The API's full-device synchronization
// must still run afterwards for all streams and asynchronous error propagation.
// This is not a load/idle gate and never resubmits work on an error.
template<class Ready, class Pause>
inline void await_roformer_completion(Ready&& ready, Pause&& pause) {
    while (!ready()) pause();
}
inline void wait_for_roformer_work(const at::Device& device) {
    if (!device.is_xpu()) return;
    c10::impl::VirtualGuardImpl guard(device.type());
    c10::Event completion(device.type(), c10::EventFlag::PYTORCH_DEFAULT);
    completion.record(guard.getStream(device));
    await_roformer_completion([&] { return completion.query(); }, [] {
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    });
}
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
// Same complex-FP32 arithmetic, but store directly through the FP16 rounding
// already required by mixed attention. This removes an FP32 output tensor and
// a separate conversion pass; neither input nor the FP32 phase is downcast.
inline at::Tensor interleaved_roformer_rotation_half(const at::Tensor& input, const at::Tensor& phase) {
    auto paired = at::view_as_complex(input.unflatten(-1, {input.size(-1) / 2, 2}));
    auto output = at::empty_like(input.transpose(-3, -2), input.options().dtype(at::kHalf),
        at::MemoryFormat::Contiguous).transpose(-3, -2);
    auto complex_output = at::view_as_complex(output.unflatten(-1, {input.size(-1) / 2, 2}));
    at::mul_out(complex_output, paired, phase);
    return output;
}
// Diagnostic-only copy candidate: exact bits and a local gain did not establish
// an incremental whole-model win. No complex arithmetic; use scalar conversion
// when a complex view cannot represent the tensor. Not routed by the model.
inline at::Tensor paired_roformer_half(const at::Tensor& input) {
    if (input.scalar_type() != at::kFloat || input.dim() == 0 || input.numel() == 0
        || input.size(-1) % 2 != 0 || input.stride(-1) != 1 || input.storage_offset() % 2 != 0)
        return input.to(at::kHalf);
    for (int64_t axis = 0; axis + 1 < input.dim(); ++axis)
        if (input.stride(axis) % 2 != 0) return input.to(at::kHalf);
    auto paired = at::view_as_complex(input.unflatten(-1, {input.size(-1) / 2, 2}));
    return at::view_as_real(paired.to(at::kComplexHalf)).flatten(-2);
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
    return attended;
}
inline at::Tensor layout_preserving_roformer_attention(const at::Tensor& query, const at::Tensor& key,
                                                      const at::Tensor& value, double scale) {
    return layout_preserving_roformer_attention(query, key, value, scale, [](const char*) {});
}
// Gates are FP32, so TensorIterator promotes half attention values to FP32
// inside this multiply, exactly as an explicit conversion followed by gating.
inline at::Tensor gated_roformer_attention(const at::Tensor& attended, const at::Tensor& gates) {
    return attended.transpose(1, 2) * gates.unsqueeze(-1);
}
} // namespace uta::torch_native
