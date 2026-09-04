#pragma once

#include <functional>
#include <memory>
#include <string>
#include <vector>

#include "decode.h"

namespace rmvpe {
class UtaRmvpeGraph;
}

// Drives the full RMVPE pipeline: log-mel frontend -> windowed CNN+GRU
// inference (chunked for ggml-alloc bounds; see rmvpe_runtime.cpp) ->
// per-frame pitch decode. It preserves the validated 1024-frame window,
// 128-frame overlap, 896-frame stride, and 32-frame bucket contract so native
// evidence remains frame-for-frame comparable across backend revisions.
class RmvpeRuntime {
public:
    explicit RmvpeRuntime(const std::string& gguf_path);
    ~RmvpeRuntime();

    // audio: mono float32 PCM at rmvpe::kSampleRate (16 kHz). progress_callback
    // reports (windows_completed, total_windows).
    std::vector<rmvpe::PitchFrame> Process(const std::vector<float>& audio,
                                            std::function<void(int, int)> progress_callback = nullptr);

    const char* BackendName() const;

private:
    std::unique_ptr<rmvpe::UtaRmvpeGraph> model_;
};
