#include "diagnostics.hpp"
#include "api.h"
#include <atomic>

namespace {
std::atomic<UtaLibtorchDiagnosticCallback> diagnostic_callback{nullptr};
}
extern "C" void uta_libtorch_set_diagnostic_callback(UtaLibtorchDiagnosticCallback callback) noexcept {
    diagnostic_callback.store(callback, std::memory_order_release);
}
namespace uta::torch_native {
void diagnostic_event(const char* phase, const char* detail) noexcept {
    const auto callback = diagnostic_callback.load(std::memory_order_acquire);
    if (callback) callback(phase, detail);
}
} // namespace uta::torch_native
