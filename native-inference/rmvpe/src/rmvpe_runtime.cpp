#include "uta_studio/rmvpe_runtime.h"
#include "uta_studio/diagnostics.h"
#include "graph.h"
#include "mel.h"
#include "decode.h"

#include <algorithm>
#include <cerrno>
#include <cstdlib>
#include <limits>
#include <stdexcept>

#include <ggml.h>
#include <ggml-alloc.h>
#include <ggml-backend.h>

namespace {

int DivCeil(int value, int step) { return (value + step - 1) / step; }

// One [start,len) slice of a window's timeline, len<=kGruChunkFrames. Every
// sub-chunk graph in this file is built, allocated, computed, and torn down
// fresh because this recurrent/view/concat-heavy graph shape cannot safely
// reuse one allocation across compute calls.
constexpr int kGruChunkFrames = 128;

struct ChunkSpan { int start; int len; };

std::vector<ChunkSpan> PlanChunks(int total) {
    std::vector<ChunkSpan> spans;
    for (int start = 0; start < total; start += kGruChunkFrames) {
        spans.push_back({start, std::min(kGruChunkFrames, total - start)});
    }
    return spans;
}

// Runs BuildCnnHead alone as one fresh graph for the whole window (safe for
// any T -- no per-timestep unrolling in the U-Net or head conv).
std::vector<float> RunCnnHead(rmvpe::UtaRmvpeGraph& graph, const std::vector<float>& mel_flat, int T) {
    const size_t graph_capacity = (size_t)T * 30 + 20000;
    struct ggml_init_params params = {
        (graph_capacity + 5000) * ggml_tensor_overhead() + ggml_graph_overhead_custom(graph_capacity, false) + (16u << 20),
        nullptr, true,
    };
    ggml_context* ctx = ggml_init(params);
    if (!ctx) throw std::runtime_error("RMVPE CNN head: failed to allocate ggml context");
    ggml_cgraph* gf = ggml_new_graph_custom(ctx, graph_capacity, false);

    ggml_tensor* mel_input = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, T, rmvpe::kMelBins);
    ggml_set_input(mel_input);

    ggml_tensor* gru_input = graph.BuildCnnHead(ctx, gf, mel_input, T, nullptr);

    ggml_gallocr_t allocr = ggml_gallocr_new(ggml_backend_get_default_buffer_type(graph.GetBackend()));
    if (!ggml_gallocr_reserve(allocr, gf) || !ggml_gallocr_alloc_graph(allocr, gf)) {
        ggml_gallocr_free(allocr);
        ggml_free(ctx);
        throw std::runtime_error("RMVPE CNN head: graph allocation failed");
    }

    ggml_backend_tensor_set(mel_input, mel_flat.data(), 0, mel_flat.size() * sizeof(float));
    enum ggml_status status = ggml_backend_graph_compute(graph.GetBackend(), gf);
    if (status != GGML_STATUS_SUCCESS) {
        ggml_gallocr_free(allocr);
        ggml_free(ctx);
        throw std::runtime_error("RMVPE CNN head: compute failed");
    }

    std::vector<float> out(ggml_nelements(gru_input));
    ggml_backend_tensor_get(gru_input, out.data(), 0, out.size() * sizeof(float));

    ggml_gallocr_free(allocr);
    ggml_free(ctx);
    return out;
}

// Runs one direction's GRU over one sub-chunk as a fresh graph. Returns this
// chunk's [256,chunk_len] hidden-state sequence and its final cell state
// (256 floats) to carry into the next chunk in this direction's order.
struct GruChunkOutput { std::vector<float> out; std::vector<float> h_final; };

GruChunkOutput RunGruChunk(rmvpe::UtaRmvpeGraph& graph, int direction, const ChunkSpan& span,
                           const std::vector<float>& gru_input_flat, const std::vector<float>& h_prev, int64_t D) {
    const int64_t H = 256;
    const size_t graph_capacity = (size_t)span.len * 40 + 2000;
    struct ggml_init_params params = {
        (graph_capacity + 2000) * ggml_tensor_overhead() + ggml_graph_overhead_custom(graph_capacity, false) + (4u << 20),
        nullptr, true,
    };
    ggml_context* ctx = ggml_init(params);
    if (!ctx) throw std::runtime_error("RMVPE GRU chunk: failed to allocate ggml context");
    ggml_cgraph* gf = ggml_new_graph_custom(ctx, graph_capacity, false);

    ggml_tensor* x_chunk = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, D, span.len);
    ggml_set_input(x_chunk);

    auto result = graph.BuildGruDirectionChunk(ctx, gf, x_chunk, span.len, direction);

    ggml_gallocr_t allocr = ggml_gallocr_new(ggml_backend_get_default_buffer_type(graph.GetBackend()));
    if (!ggml_gallocr_reserve(allocr, gf) || !ggml_gallocr_alloc_graph(allocr, gf)) {
        ggml_gallocr_free(allocr);
        ggml_free(ctx);
        throw std::runtime_error("RMVPE GRU chunk: graph allocation failed");
    }

    ggml_backend_tensor_set(x_chunk, gru_input_flat.data() + (size_t)span.start * D, 0,
                            (size_t)span.len * D * sizeof(float));
    ggml_backend_tensor_set(result.h_prev, h_prev.data(), 0, h_prev.size() * sizeof(float));

    enum ggml_status status = ggml_backend_graph_compute(graph.GetBackend(), gf);
    if (status != GGML_STATUS_SUCCESS) {
        ggml_gallocr_free(allocr);
        ggml_free(ctx);
        throw std::runtime_error("RMVPE GRU chunk: compute failed");
    }

    GruChunkOutput out;
    out.out.resize(ggml_nelements(result.out));
    ggml_backend_tensor_get(result.out, out.out.data(), 0, out.out.size() * sizeof(float));
    out.h_final.resize(H);
    ggml_backend_tensor_get(result.h_final, out.h_final.data(), 0, out.h_final.size() * sizeof(float));

    ggml_gallocr_free(allocr);
    ggml_free(ctx);
    return out;
}

// Runs fc.1+sigmoid alone as one fresh graph for the whole window (safe for
// any T -- no per-timestep unrolling).
std::vector<float> RunOutputHead(rmvpe::UtaRmvpeGraph& graph, const std::vector<float>& gru_out_flat, int T) {
    const size_t graph_capacity = 200;
    struct ggml_init_params params = {
        (graph_capacity + 200) * ggml_tensor_overhead() + ggml_graph_overhead_custom(graph_capacity, false) + (4u << 20),
        nullptr, true,
    };
    ggml_context* ctx = ggml_init(params);
    if (!ctx) throw std::runtime_error("RMVPE output head: failed to allocate ggml context");
    ggml_cgraph* gf = ggml_new_graph_custom(ctx, graph_capacity, false);

    ggml_tensor* gru_out_full = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, 512, T);
    ggml_set_input(gru_out_full);
    ggml_tensor* output = graph.BuildOutputHead(ctx, gf, gru_out_full, T);

    ggml_gallocr_t allocr = ggml_gallocr_new(ggml_backend_get_default_buffer_type(graph.GetBackend()));
    if (!ggml_gallocr_reserve(allocr, gf) || !ggml_gallocr_alloc_graph(allocr, gf)) {
        ggml_gallocr_free(allocr);
        ggml_free(ctx);
        throw std::runtime_error("RMVPE output head: graph allocation failed");
    }

    ggml_backend_tensor_set(gru_out_full, gru_out_flat.data(), 0, gru_out_flat.size() * sizeof(float));
    enum ggml_status status = ggml_backend_graph_compute(graph.GetBackend(), gf);
    if (status != GGML_STATUS_SUCCESS) {
        ggml_gallocr_free(allocr);
        ggml_free(ctx);
        throw std::runtime_error("RMVPE output head: compute failed");
    }

    std::vector<float> out(ggml_nelements(output));
    ggml_backend_tensor_get(output, out.data(), 0, out.size() * sizeof(float));

    ggml_gallocr_free(allocr);
    ggml_free(ctx);
    return out;
}

// Runs the full CNN+GRU+head pipeline for one window (T frames, already
// rounded up to a multiple of 32 -- see Process()'s bucketing) and returns
// the [T,360] sigmoid activations, flattened frame-major.
std::vector<float> RunWindow(rmvpe::UtaRmvpeGraph& graph, const std::vector<float>& mel_flat, int T) {
    constexpr int64_t D = 384;
    constexpr int64_t H = 256;

    std::vector<float> gru_input = RunCnnHead(graph, mel_flat, T);

    auto chunks = PlanChunks(T);
    std::vector<float> fwd_full((size_t)H * T), bwd_full((size_t)H * T);

    std::vector<float> h_prev(H, 0.0f);
    for (const auto& span : chunks) {
        auto result = RunGruChunk(graph, /*direction=*/0, span, gru_input, h_prev, D);
        std::copy(result.out.begin(), result.out.end(), fwd_full.begin() + (size_t)span.start * H);
        h_prev = result.h_final;
    }
    std::fill(h_prev.begin(), h_prev.end(), 0.0f);
    for (auto it = chunks.rbegin(); it != chunks.rend(); ++it) {
        auto result = RunGruChunk(graph, /*direction=*/1, *it, gru_input, h_prev, D);
        std::copy(result.out.begin(), result.out.end(), bwd_full.begin() + (size_t)it->start * H);
        h_prev = result.h_final;
    }

    std::vector<float> gru_out((size_t)2 * H * T);
    for (int t = 0; t < T; ++t) {
        std::copy(fwd_full.begin() + (size_t)t * H, fwd_full.begin() + (size_t)(t + 1) * H,
                  gru_out.begin() + (size_t)t * 2 * H);
        std::copy(bwd_full.begin() + (size_t)t * H, bwd_full.begin() + (size_t)(t + 1) * H,
                  gru_out.begin() + (size_t)t * 2 * H + H);
    }

    return RunOutputHead(graph, gru_out, T);
}

} // namespace

RmvpeRuntime::RmvpeRuntime(const std::string& gguf_path) {
    uta_diagnostics::Log("inference", "constructor.begin", "model=" + gguf_path);
    model_ = std::make_unique<rmvpe::UtaRmvpeGraph>();
    int device = -1;
#ifdef HAVE_GGML_VULKAN
    if (std::getenv("UTA_STUDIO_RMVPE_FORCE_CPU") == nullptr) {
        const char* device_value = std::getenv("UTA_STUDIO_VULKAN_DEVICE");
        if (device_value == nullptr) {
            device = 0;
        } else {
            errno = 0;
            char* end = nullptr;
            const long parsed = std::strtol(device_value, &end, 10);
            if (errno != 0 || end == device_value || *end != '\0' || parsed < 0 ||
                parsed > std::numeric_limits<int>::max()) {
                throw std::runtime_error(
                    "invalid UTA_STUDIO_VULKAN_DEVICE value: " + std::string(device_value));
            }
            device = static_cast<int>(parsed);
        }
    }
#endif
    model_->Initialize(device);
    model_->LoadWeights(gguf_path);
    uta_diagnostics::Log("inference", "constructor.end", std::string("backend=") + BackendName());
}

RmvpeRuntime::~RmvpeRuntime() = default;

const char* RmvpeRuntime::BackendName() const {
    return ggml_backend_name(model_->GetBackend());
}

std::vector<rmvpe::PitchFrame> RmvpeRuntime::Process(const std::vector<float>& audio,
                                                      std::function<void(int, int)> progress_callback) {
    int frames = 0;
    std::vector<float> frame_major = rmvpe::LogMelSpectrogram(audio.data(), (int64_t)audio.size(), &frames);
    uta_diagnostics::Log("inference", "mel.end", "frames=" + std::to_string(frames));

    const int window_count = frames <= rmvpe::kMaxInputFrames
        ? 1
        : DivCeil(frames - rmvpe::kMaxInputFrames, rmvpe::kStrideFrames) + 1;

    std::vector<rmvpe::PitchFrame> evidence;
    evidence.reserve(frames);
    std::vector<float> scratch;
    int start = 0;
    for (int window = 0; window < window_count; ++window) {
        const int remaining = frames - start;
        const bool final_window = remaining <= rmvpe::kMaxInputFrames;
        const int clamped = std::min(std::max(remaining, rmvpe::kMinInputFrames), rmvpe::kMaxInputFrames);
        const int input_frames = DivCeil(clamped, rmvpe::kFrameStep) * rmvpe::kFrameStep;

        std::vector<float> mel_window = rmvpe::ToChannelMajorWindow(frame_major.data(), frames, start, input_frames);
        std::vector<float> activations = RunWindow(*model_, mel_window, input_frames);
        if (progress_callback) progress_callback(window + 1, window_count);

        const int64_t dims[3] = {1, input_frames, rmvpe::kPitchClasses};
        const int keep_start = start == 0 ? 0 : rmvpe::kOverlapFrames / 2;
        const int keep_end = final_window ? remaining : rmvpe::kMaxInputFrames - rmvpe::kOverlapFrames / 2;
        for (int local_frame = keep_start; local_frame < keep_end; ++local_frame) {
            const float* activation = rmvpe::ActivationFrame(activations.data(), dims, input_frames, local_frame, &scratch);
            auto [hz, confidence] = rmvpe::LocalAverageHz(activation, rmvpe::kPitchClasses);
            const int frame = start + local_frame;
            evidence.push_back(rmvpe::PitchFrame{
                frame * rmvpe::kTimelineStepSeconds, hz, confidence, confidence >= rmvpe::kVoicedThreshold});
        }
        if (final_window) break;
        start += rmvpe::kStrideFrames;
    }

    if ((int)evidence.size() != frames) {
        throw std::runtime_error("RMVPE overlap stitching did not preserve the evidence timeline");
    }
    return evidence;
}
