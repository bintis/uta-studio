#pragma once

namespace uta::torch_native {
// The registered host callback owns persistence. Calls return only after its
// write/sync attempt; no asynchronous stderr reader has to receive the event.
void diagnostic_event(const char* phase, const char* detail) noexcept;
} // namespace uta::torch_native
