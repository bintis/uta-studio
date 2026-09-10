#pragma once
#include <ATen/ATen.h>
#include <ATen/Context.h>

namespace uta::torch_native {
// Diagnostic only, in a single-model process. This oneDNN flag is process-global,
// not thread-local: this scope is not a production concurrent precision API.
// The wheel-matched XPU Matmul.cpp reads this flag rather than the MKLDNN CPU
// precision selector. Restore it on success and on ATen exceptions.
class DiagnosticRoformerMatmulScope {
public:
    DiagnosticRoformerMatmulScope() : previous(at::globalContext().allowTF32OneDNN()) {
        at::globalContext().setAllowTF32OneDNN(true);
    }
    ~DiagnosticRoformerMatmulScope() { at::globalContext().setAllowTF32OneDNN(previous); }
    DiagnosticRoformerMatmulScope(const DiagnosticRoformerMatmulScope&) = delete;
    DiagnosticRoformerMatmulScope& operator=(const DiagnosticRoformerMatmulScope&) = delete;
private:
    bool previous;
};
inline at::Tensor diagnostic_roformer_projection(const at::Tensor& input, const at::Tensor& weight,
                                                const at::Tensor& bias = {}) {
    DiagnosticRoformerMatmulScope scope;
    return at::linear(input, weight, bias);
}
} // namespace uta::torch_native
