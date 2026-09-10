#pragma once
#include <ATen/ATen.h>
#include <ATen/core/dispatch/Dispatcher.h>

namespace uta::torch_native {
// Native oneDNN matmul post-op, with the same FP32 operands and erf GELU.
// Unlike _addmm_activation on XPU, this avoids a separate activation pass.
// No process-global precision change or alternate execution backend.
inline at::Tensor fused_roformer_projection_gelu(const at::Tensor& input, const at::Tensor& weight,
                                                const at::Tensor& bias) {
    using Projection = at::Tensor(const at::Tensor&, const at::Tensor&, const std::optional<at::Tensor>&,
        std::string_view, c10::List<std::optional<at::Scalar>>, std::optional<std::string_view>);
    static const auto operation = c10::Dispatcher::singleton()
        .findSchemaOrThrow("mkldnn::_linear_pointwise", "").typed<Projection>();
    return operation.call(input, weight, bias, "gelu",
        c10::List<std::optional<at::Scalar>>{}, std::string_view("none"));
}
} // namespace uta::torch_native
