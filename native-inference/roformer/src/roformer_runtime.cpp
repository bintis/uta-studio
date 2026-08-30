#include "uta_studio/roformer_runtime.h"
#include "uta_studio/diagnostics.h"
#include "graph.h"
#include "utils.h"
#include "stft.h"
#include <iostream>
#include <complex>
#include <algorithm>
#include <cstring>
#include <ggml.h>
#include <ggml-alloc.h>
#include <ggml-backend.h>
#include <queue>
#include <thread>
#include <mutex>
#include <condition_variable>
#include <atomic>
#include <exception>
#include <stdexcept>
#include <chrono>
#include <sstream>

using Complex = std::complex<float>;
static constexpr const char* kInferenceCancelledMessage = "RoformerRuntime cancelled";

namespace {
constexpr size_t kGraphContextSize = 1024ull * 1024 * 1024;
constexpr int kGraphCapacity = 65536;

std::string DurationMs(std::chrono::steady_clock::time_point start) {
    std::ostringstream stream;
    stream << "duration_ms=" << std::chrono::duration<double, std::milli>(
        std::chrono::steady_clock::now() - start).count();
    return stream.str();
}

std::string ChunkDetails(int sequence, int total_chunks, int offset) {
    return "chunk=" + std::to_string(sequence + 1) +
           " total_chunks=" + std::to_string(total_chunks) +
           " offset_frames=" + std::to_string(offset);
}
}

struct RoformerRuntime::CpuScratch {
    // Rejected: sharing one RoformerRuntime-wide scratch across the pipeline caused
    // cross-thread vector reuse and a post-PASSED 0xc0000409 test_inference crash.
    // Keep scratch ownership local to each serial call or pipeline stage thread.
    int hann_win_length = -1;
    std::vector<float> hann_window;
    std::vector<float> channel_audio;
    std::vector<float> istft_in;
    std::vector<std::vector<float>> output_channels;

    void EnsureHannWindow(int win_length) {
        if (hann_win_length == win_length && hann_window.size() == static_cast<size_t>(win_length)) {
            return;
        }
        hann_window.resize(win_length);
        stft::hann_window(hann_window.data(), win_length);
        hann_win_length = win_length;
    }
};

// Helper forward decl
std::vector<float> GetWindow(int size, int fade_size);

std::vector<float> GetWindow(int size, int fade_size) {
    std::vector<float> window(size, 1.0f);
    // Use inclusive linear fade-in and fade-out ramps.
    // linspace includes both endpoints, so we divide by (fade_size - 1)
    for (int i = 0; i < fade_size; ++i) {
        // fadein[i] = i / (fade_size - 1), ranges from 0.0 to 1.0 inclusive
        // fadeout[i] = 1 - i / (fade_size - 1), ranges from 1.0 to 0.0 inclusive
        float fadein = (fade_size > 1) ? (float)i / (fade_size - 1) : 1.0f;
        float fadeout = (fade_size > 1) ? 1.0f - (float)i / (fade_size - 1) : 1.0f;
        window[i] *= fadein;                     // Start of window: fade in
        window[size - fade_size + i] *= fadeout; // End of window: fade out
    }
    return window;
}


RoformerRuntime::RoformerRuntime(const std::string& model_path) {
    uta_diagnostics::Log("inference", "constructor.begin", "model=" + model_path);
    model_ = std::make_unique<UtaRoformerGraph>();
    model_->Initialize(model_path);
    ggml_backend_t backend = model_->GetBackend();
    ggml_backend_dev_t device = ggml_backend_get_device(backend);
    struct ggml_backend_dev_props props{};
    ggml_backend_dev_get_props(device, &props);
    uta_diagnostics::Log("inference", "constructor.end",
        std::string("backend=") + ggml_backend_name(backend) +
        " device=" + (props.description ? props.description : "<unknown>") +
        " device_id=" + (props.device_id ? props.device_id : "<unknown>") +
        " memory_free=" + std::to_string(props.memory_free) +
        " memory_total=" + std::to_string(props.memory_total));
}

int RoformerRuntime::GetDefaultChunkSize() const {
    return model_->GetDefaultChunkSize();
}

int RoformerRuntime::GetDefaultNumOverlap() const {
    return model_->GetDefaultNumOverlap();
}

int RoformerRuntime::GetSampleRate() const {
    return model_->GetSampleRate();
}

int RoformerRuntime::GetNumStems() const {
    return model_->GetNumStems();
}

RoformerRuntime::~RoformerRuntime() {
    ClearGraphCache();
}

void RoformerRuntime::ReleaseGraphState(GraphState& graph) {
    if (graph.allocr) {
        ggml_gallocr_free(graph.allocr);
        graph.allocr = nullptr;
    }
    if (graph.ctx) {
        ggml_free(graph.ctx);
        graph.ctx = nullptr;
    }
    graph.gf = nullptr;
    graph.input_tensor = nullptr;
    graph.pos_time = nullptr;
    graph.pos_freq = nullptr;
    graph.mask_out_tensor = nullptr;
}

void RoformerRuntime::ClearGraphCache() {
    for (auto& entry : graph_cache_) {
        ReleaseGraphState(*entry.second);
    }
    graph_cache_.clear();
}

RoformerRuntime::GraphState* RoformerRuntime::EnsureGraph(int n_frames, int batch_size) {
    const std::uint64_t cache_key =
        (static_cast<std::uint64_t>(static_cast<std::uint32_t>(n_frames)) << 32) |
        static_cast<std::uint32_t>(batch_size);
    auto cached = graph_cache_.find(cache_key);
    if (cached != graph_cache_.end()) {
        uta_diagnostics::Log("inference", "graph.cache.hit",
                             "n_frames=" + std::to_string(n_frames) +
                             " batch_size=" + std::to_string(batch_size));
        return cached->second.get();
    }

    if (!graph_cache_.empty()) {
        ClearGraphCache();
    }

    std::cout << "[RoformerRuntime] Building graph for n_frames=" << n_frames
              << ", batch_size=" << batch_size << std::endl;
    const auto graph_start = std::chrono::steady_clock::now();
    uta_diagnostics::Log("inference", "graph.build.begin",
                         "n_frames=" + std::to_string(n_frames) +
                         " batch_size=" + std::to_string(batch_size));

    auto graph = std::make_unique<GraphState>();
    graph->n_frames = n_frames;
    graph->batch_size = batch_size;

    struct ggml_init_params ctx_params = { kGraphContextSize, nullptr, true };
    graph->ctx = ggml_init(ctx_params);
    if (!graph->ctx) return nullptr;

    graph->gf = ggml_new_graph_custom(graph->ctx, kGraphCapacity, false);

    int total_dim_input = model_->GetTotalDimInput();

    graph->input_tensor = ggml_new_tensor_3d(graph->ctx, GGML_TYPE_F32, total_dim_input, n_frames, batch_size);
    ggml_set_name(graph->input_tensor, "in.model");
    ggml_set_input(graph->input_tensor);

    // BandSplit -> Transformers -> MaskEstimator
    ggml_tensor* band_out = model_->BuildBandSplitGraph(graph->ctx, graph->input_tensor, graph->gf, n_frames, batch_size);
    if (!band_out) {
        ReleaseGraphState(*graph);
        return nullptr;
    }

    int n_bands = model_->GetNumBands();
    graph->pos_time = ggml_new_tensor_1d(graph->ctx, GGML_TYPE_I32, n_frames * n_bands * batch_size);
    graph->pos_freq = ggml_new_tensor_1d(graph->ctx, GGML_TYPE_I32, n_bands * n_frames * batch_size);
    ggml_set_name(graph->pos_time, "pos.time");
    ggml_set_name(graph->pos_freq, "pos.freq");
    ggml_set_input(graph->pos_time);
    ggml_set_input(graph->pos_freq);

    ggml_tensor* trans_out = model_->BuildTransformersGraph(graph->ctx, band_out, graph->gf, graph->pos_time, graph->pos_freq, n_frames, batch_size);
    if (!trans_out) {
        ReleaseGraphState(*graph);
        return nullptr;
    }
    graph->mask_out_tensor = model_->BuildMaskEstimatorGraph(graph->ctx, trans_out, graph->gf, n_frames, batch_size);
    if (!graph->mask_out_tensor) {
        ReleaseGraphState(*graph);
        return nullptr;
    }
    // Allocate compute buffer (VRAM)
    graph->allocr = ggml_gallocr_new(ggml_backend_get_default_buffer_type(model_->GetBackend()));
    if (!ggml_gallocr_reserve(graph->allocr, graph->gf)) {
        std::cerr << "[RoformerRuntime] Warning: failed to reserve graph VRAM; trying direct graph allocation" << std::endl;
    }
    if (!ggml_gallocr_alloc_graph(graph->allocr, graph->gf)) {
        std::cerr << "[RoformerRuntime] Failed to allocate graph VRAM" << std::endl;
        ReleaseGraphState(*graph);
        return nullptr;
    }

    graph->graph_nodes = ggml_graph_n_nodes(graph->gf);
    graph->compute_buffer_size = ggml_gallocr_get_buffer_size(graph->allocr, 0);
    std::cout << "[RoformerRuntime] Graph ready: n_frames=" << n_frames
              << ", batch_size=" << batch_size
              << ", nodes=" << graph->graph_nodes
              << ", compute_buffer=" << (graph->compute_buffer_size / (1024.0 * 1024.0)) << " MiB"
              << std::endl;
    uta_diagnostics::Log("inference", "graph.build.end",
        "n_frames=" + std::to_string(n_frames) +
        " batch_size=" + std::to_string(batch_size) +
        " nodes=" + std::to_string(graph->graph_nodes) +
        " compute_buffer_bytes=" + std::to_string(graph->compute_buffer_size) +
        " " + DurationMs(graph_start));

    GraphState* graph_ptr = graph.get();
    graph_cache_.emplace(cache_key, std::move(graph));
    return graph_ptr;
}

void RoformerRuntime::ComputeSTFT(const std::vector<float>& input_audio,
                            std::vector<std::vector<float>>& stft_outputs,
                            int& n_frames,
                            CpuScratch& scratch) {
    int n_fft = model_->GetNFFT();
    int hop_length = model_->GetHopLength();
    int win_length = model_->GetWinLength();
    int n_freq = n_fft / 2 + 1;
    int channels = 2;

    scratch.EnsureHannWindow(win_length);

    stft_outputs.resize(channels);
    int n_samples = input_audio.size() / channels;

    for (int ch = 0; ch < channels; ++ch) {
        scratch.channel_audio.resize(n_samples);
        for (int i = 0; i < n_samples; ++i) {
            scratch.channel_audio[i] = input_audio[ch + i * channels];
        }

        stft_outputs[ch].resize(n_freq * (n_samples / hop_length + 5) * 2);
        stft::compute_stft(scratch.channel_audio.data(), n_samples, n_fft, hop_length, win_length,
                           scratch.hann_window.data(), true, stft_outputs[ch].data(), &n_frames);
    }
}

void RoformerRuntime::PrepareModelInput(const std::vector<std::vector<float>>& stft_outputs,
                                  int n_frames,
                                  std::vector<float>& model_input_rearranged) {
    const std::vector<int>& freq_indices = model_->GetFreqIndices();
    int num_freq_indices = freq_indices.size();
    int total_dim_input = model_->GetTotalDimInput();
    int channels = 2;

    model_input_rearranged.resize(n_frames * total_dim_input);

    #ifdef USE_OPENMP
    #pragma omp parallel for
    #endif
    for (int t = 0; t < n_frames; ++t) {
        for (int f = 0; f < num_freq_indices; ++f) {
            int idx = freq_indices[f];
            int raw_freq_idx = idx / channels;
            int ch = idx % channels;

            int in_idx_ch = (raw_freq_idx * n_frames + t) * 2;
            int out_idx = t * total_dim_input + f * 2;

            model_input_rearranged[out_idx + 0] = stft_outputs[ch][in_idx_ch + 0];
            model_input_rearranged[out_idx + 1] = stft_outputs[ch][in_idx_ch + 1];
        }
    }
}

void RoformerRuntime::PostProcessAndISTFT(const std::vector<float>& mask_output,
                                    const std::vector<std::vector<float>>& stft_outputs,
                                    int n_frames,
                                    std::vector<std::vector<float>>& output_audio,
                                    CpuScratch& scratch) {
    int n_fft = model_->GetNFFT();
    int hop_length = model_->GetHopLength();
    int win_length = model_->GetWinLength();
    int n_freq = n_fft / 2 + 1;
    int channels = 2;

    const std::vector<int>& freq_indices = model_->GetFreqIndices();
    int num_freq_indices = freq_indices.size();
    int mask_features = num_freq_indices * 2;
    int num_stems = model_->GetNumStems();
    // Tensor layout: [mask_features, num_stems, n_frames, batch]
    // GGML stride for time t is: mask_features * num_stems
    int stride_time = mask_features * num_stems;

    output_audio.resize(num_stems);

    scratch.EnsureHannWindow(win_length);
    scratch.output_channels.resize(channels);

    const std::vector<int>& num_bands_per_freq = model_->GetNumBandsPerFreq();
    const int istft_input_size = n_freq * n_frames * 2;
    const int approx_len = (n_frames - 1) * hop_length + n_fft;

    // Process each stem
    for (int stem = 0; stem < num_stems; ++stem) {
        int n_samples_out = 0;

        for (int ch = 0; ch < channels; ++ch) {
            scratch.istft_in.resize(istft_input_size);
            std::fill(scratch.istft_in.begin(), scratch.istft_in.end(), 0.0f);

            // Rejected: directly combining mask scatter, normalization, and complex
            // multiply changed output numerics and triggered a test_inference stack
            // protection failure. Keep the original two-pass math order.
            for (int f = 0; f < num_freq_indices; ++f) {
                int freq_stereo_idx = freq_indices[f];
                if (freq_stereo_idx % channels != ch) {
                    continue;
                }
                int raw_freq_idx = freq_stereo_idx / channels;
                int dst_base = raw_freq_idx * n_frames * 2;

                for (int t = 0; t < n_frames; ++t) {
                    int mask_idx = t * stride_time + stem * mask_features + f * 2;
                    int dst_idx = dst_base + t * 2;
                    scratch.istft_in[dst_idx + 0] += mask_output[mask_idx + 0];
                    scratch.istft_in[dst_idx + 1] += mask_output[mask_idx + 1];
                }
            }

            #ifdef USE_OPENMP
            #pragma omp parallel for
            #endif
            for (int f = 0; f < n_freq; ++f) {
                float denom = (float)num_bands_per_freq[f];
                if (denom < 1e-8f) denom = 1e-8f;

                for (int t = 0; t < n_frames; ++t) {
                    int dst_idx = (f * n_frames + t) * 2;
                    int stft_idx = dst_idx;
                    Complex stft_val(stft_outputs[ch][stft_idx + 0], stft_outputs[ch][stft_idx + 1]);
                    Complex mask_val(scratch.istft_in[dst_idx + 0], scratch.istft_in[dst_idx + 1]);
                    mask_val /= denom;
                    Complex masked = stft_val * mask_val;
                    scratch.istft_in[dst_idx + 0] = masked.real();
                    scratch.istft_in[dst_idx + 1] = masked.imag();
                }
            }

            // Zero DC if enabled
            if (model_->GetZeroDC()) {
                for (int t = 0; t < n_frames; ++t) {
                    // f=0 is DC component
                    int dst_idx = (0 * n_frames + t) * 2;
                    scratch.istft_in[dst_idx + 0] = 0.0f;
                    scratch.istft_in[dst_idx + 1] = 0.0f;
                }
            }

            scratch.output_channels[ch].resize(approx_len + n_fft);
            stft::compute_istft(scratch.istft_in.data(), n_freq, n_frames, n_fft, hop_length, win_length,
                                scratch.hann_window.data(), true, approx_len, scratch.output_channels[ch].data());
            if (ch == 0) n_samples_out = approx_len;
            scratch.output_channels[ch].resize(n_samples_out);
        }

        output_audio[stem].resize(channels * n_samples_out);
        for (int i = 0; i < n_samples_out; ++i) {
            for (int ch = 0; ch < channels; ++ch) {
                output_audio[stem][ch + i * channels] = scratch.output_channels[ch][i];
            }
        }
    }
}

std::vector<std::vector<float>> RoformerRuntime::Process(const std::vector<float>& input_audio,
                                                   int chunk_size,
                                                   int num_overlap,
                                                   std::function<void(int, int)> progress_callback,
                                                   CancelCallback cancel_callback,
                                                   int batch_size,
                                                   bool serial_pipeline) {
    if (input_audio.empty()) return {};
    if (batch_size != 1) {
        throw std::runtime_error(
            "RoFormer batch size must be 1; larger Vulkan batches hard-reset the current host");
    }
    if (serial_pipeline) {
        uta_diagnostics::Log("inference", "pipeline.serial.enabled",
            "cpu_preprocess_gpu_compute_cpu_postprocess=true batch_size=1");
        return ProcessOverlapAdd(
            input_audio,
            chunk_size,
            num_overlap,
            [this](const std::vector<float>& chunk) { return ProcessChunk(chunk); },
            std::move(progress_callback),
            std::move(cancel_callback));
    }
    return ProcessOverlapAddPipelined(
        input_audio, chunk_size, num_overlap, progress_callback, cancel_callback, batch_size);
}

// =================================================================================================
// Pipeline Stages
// =================================================================================================

std::shared_ptr<RoformerRuntime::ChunkState> RoformerRuntime::PreProcessChunk(const std::vector<float>& chunk_audio,
                                                                  int id,
                                                                  int sequence,
                                                                  int total_chunks,
                                                                  CpuScratch& scratch) {
    const auto start = std::chrono::steady_clock::now();
    const std::string chunk = ChunkDetails(sequence, total_chunks, id);
    uta_diagnostics::Log("inference", "chunk.preprocess.begin", chunk);
    auto state = std::make_shared<ChunkState>();
    state->id = id;
    state->sequence = sequence;
    state->total_chunks = total_chunks;
    state->input_audio = chunk_audio; // Copy

    if (chunk_audio.empty()) {
        uta_diagnostics::Log("inference", "chunk.preprocess.empty", chunk);
        return state;
    }

    // 1. STFT
    ComputeSTFT(state->input_audio, state->stft_outputs, state->n_frames, scratch);

    // 2. Prepare Input
    PrepareModelInput(state->stft_outputs, state->n_frames, state->stft_flattened);

    uta_diagnostics::Log("inference", "chunk.preprocess.end",
        chunk + " n_frames=" + std::to_string(state->n_frames) +
        " input_values=" + std::to_string(state->stft_flattened.size()) +
        " " + DurationMs(start));

    return state;
}

void RoformerRuntime::RunInference(std::shared_ptr<ChunkState> state) {
    RunInferenceBatch({std::move(state)});
}

void RoformerRuntime::RunInferenceBatch(const std::vector<std::shared_ptr<ChunkState>>& states) {
    if (states.empty()) return;
    for (const auto& state : states) {
        if (!state || state->stft_flattened.empty()) return;
    }

    const int batch_size = static_cast<int>(states.size());
    const int n_frames = states.front()->n_frames;
    const size_t input_values_per_chunk = states.front()->stft_flattened.size();
    for (const auto& state : states) {
        if (state->n_frames != n_frames || state->stft_flattened.size() != input_values_per_chunk) {
            throw std::runtime_error("Cannot batch RoFormer chunks with different tensor shapes");
        }
    }

    const std::string batch =
        "batch_size=" + std::to_string(batch_size) +
        " first_chunk=" + std::to_string(states.front()->sequence + 1) +
        " last_chunk=" + std::to_string(states.back()->sequence + 1) +
        " total_chunks=" + std::to_string(states.front()->total_chunks);
    const auto inference_start = std::chrono::steady_clock::now();
    uta_diagnostics::Log("inference", "batch.inference.begin", batch);

    uta_diagnostics::Log("inference", "batch.graph.ensure.begin", batch +
                         " n_frames=" + std::to_string(n_frames));
    GraphState* graph = EnsureGraph(n_frames, batch_size);
    if (!graph) {
        uta_diagnostics::Log("inference", "batch.graph.ensure.failed", batch);
        return;
    }
    uta_diagnostics::Log("inference", "batch.graph.ensure.end", batch +
                         " nodes=" + std::to_string(graph->graph_nodes));

    int n_bands = model_->GetNumBands();

    // Prepare position data
    // Use cached vectors to avoid allocation
    int required_time_size = n_frames * n_bands * batch_size;
    if (graph->pos_time_data.size() != required_time_size) {
        graph->pos_time_data.resize(required_time_size);
        for(int i=0; i < required_time_size; ++i) graph->pos_time_data[i] = i % n_frames;
    }

    int required_freq_size = n_bands * n_frames * batch_size;
    // Note: pos_freq logic (i % n_bands) depends on n_bands (constant) and total size.
    // If n_frames changes, size changes, and values might depend on n_frames?
    // Wait, pos_freq_data[i] = i % n_bands.
    // This is valid regardless of n_frames as long as size is correct.
    // But we should regenerate if size changes.
    if (graph->pos_freq_data.size() != required_freq_size) {
        graph->pos_freq_data.resize(required_freq_size);
        for(int i=0; i < required_freq_size; ++i) graph->pos_freq_data[i] = i % n_bands;
    }

    ggml_backend_t backend = model_->GetBackend();
    const size_t input_bytes = ggml_nbytes(graph->input_tensor);
    const size_t pos_time_bytes = ggml_nbytes(graph->pos_time);
    const size_t pos_freq_bytes = ggml_nbytes(graph->pos_freq);
    const size_t output_bytes = ggml_nbytes(graph->mask_out_tensor);

    graph->input_data.resize(input_values_per_chunk * batch_size);
    for (int i = 0; i < batch_size; ++i) {
        std::copy(states[i]->stft_flattened.begin(), states[i]->stft_flattened.end(),
                  graph->input_data.begin() + static_cast<std::ptrdiff_t>(i * input_values_per_chunk));
    }

    // 4. Host -> Device
    const auto upload_start = std::chrono::steady_clock::now();
    uta_diagnostics::Log("inference", "batch.upload.begin", batch +
        " input_bytes=" + std::to_string(input_bytes) +
        " pos_time_bytes=" + std::to_string(pos_time_bytes) +
        " pos_freq_bytes=" + std::to_string(pos_freq_bytes));
    ggml_backend_tensor_set(graph->input_tensor, graph->input_data.data(), 0, input_bytes);
    ggml_backend_tensor_set(graph->pos_time, graph->pos_time_data.data(), 0, pos_time_bytes);
    ggml_backend_tensor_set(graph->pos_freq, graph->pos_freq_data.data(), 0, pos_freq_bytes);
    uta_diagnostics::Log("inference", "batch.upload.end", batch + " " + DurationMs(upload_start));

    // 5. Compute
    size_t memory_free = 0;
    size_t memory_total = 0;
    ggml_backend_dev_memory(ggml_backend_get_device(backend), &memory_free, &memory_total);
    const auto compute_start = std::chrono::steady_clock::now();
    uta_diagnostics::Log("inference", "batch.compute.begin", batch +
        " graph_nodes=" + std::to_string(graph->graph_nodes) +
        " memory_free=" + std::to_string(memory_free) +
        " memory_total=" + std::to_string(memory_total));
    enum ggml_status status = ggml_backend_graph_compute(backend, graph->gf);
    ggml_backend_dev_memory(ggml_backend_get_device(backend), &memory_free, &memory_total);
    uta_diagnostics::Log("inference", "batch.compute.end", batch +
        " status=" + std::string(ggml_status_to_string(status)) +
        " memory_free=" + std::to_string(memory_free) +
        " memory_total=" + std::to_string(memory_total) +
        " " + DurationMs(compute_start));

    // 6. Device -> Host
    const auto download_start = std::chrono::steady_clock::now();
    uta_diagnostics::Log("inference", "batch.download.begin", batch +
                         " output_bytes=" + std::to_string(output_bytes));
    graph->output_data.resize(ggml_nelements(graph->mask_out_tensor));
    ggml_backend_tensor_get(graph->mask_out_tensor, graph->output_data.data(), 0, output_bytes);
    uta_diagnostics::Log("inference", "batch.download.end", batch + " " + DurationMs(download_start));

    if (status != GGML_STATUS_SUCCESS) {
        throw std::runtime_error(std::string("GGML graph compute failed: ") + ggml_status_to_string(status));
    }

    const size_t output_values_per_chunk = graph->output_data.size() / batch_size;
    for (int i = 0; i < batch_size; ++i) {
        auto begin = graph->output_data.begin() + static_cast<std::ptrdiff_t>(i * output_values_per_chunk);
        states[i]->mask_output.assign(begin, begin + static_cast<std::ptrdiff_t>(output_values_per_chunk));
        uta_diagnostics::Log("inference", "chunk.inference.end",
            ChunkDetails(states[i]->sequence, states[i]->total_chunks, states[i]->id) +
            " batch_size=" + std::to_string(batch_size) + " " + DurationMs(inference_start));
    }
    uta_diagnostics::Log("inference", "batch.inference.end", batch + " " + DurationMs(inference_start));
}

void RoformerRuntime::PostProcessChunk(std::shared_ptr<ChunkState> state, CpuScratch& scratch) {
    if (!state || state->mask_output.empty()) return;

    const auto start = std::chrono::steady_clock::now();
    const std::string chunk = ChunkDetails(state->sequence, state->total_chunks, state->id);
    uta_diagnostics::Log("inference", "chunk.postprocess.begin", chunk);

    // 7. Post-Process & ISTFT
    PostProcessAndISTFT(state->mask_output, state->stft_outputs, state->n_frames, state->final_audio, scratch);

    // 8. Trim
    for (auto& stem_audio : state->final_audio) {
        if (stem_audio.size() > state->input_audio.size()) {
           stem_audio.resize(state->input_audio.size());
        } else if (stem_audio.size() < state->input_audio.size()) {
           stem_audio.resize(state->input_audio.size(), 0.0f);
        }
    }
    uta_diagnostics::Log("inference", "chunk.postprocess.end", chunk +
        " stems=" + std::to_string(state->final_audio.size()) +
        " " + DurationMs(start));
}

std::vector<std::vector<float>> RoformerRuntime::ProcessChunk(const std::vector<float>& chunk_audio) {
    // Serial fallback
    CpuScratch preprocess_scratch;
    CpuScratch postprocess_scratch;
    auto state = PreProcessChunk(chunk_audio, 0, 0, 1, preprocess_scratch);
    RunInference(state);
    PostProcessChunk(state, postprocess_scratch);
    return state->final_audio;
}

// =================================================================================================
// Pipelined Overlap-Add Logic
// =================================================================================================

// =================================================================================================
// Thread Safe Queue
// =================================================================================================

template <typename T>
class ThreadSafeQueue {
public:
    ThreadSafeQueue(size_t max_size) : max_size_(max_size), shutdown_(false) {}

    ~ThreadSafeQueue() {
        Shutdown();
    }

    void Push(T item) {
        std::unique_lock<std::mutex> lock(mutex_);
        cv_push_.wait(lock, [this] { return queue_.size() < max_size_ || shutdown_; });
        if (shutdown_) return;
        queue_.push(std::move(item));
        cv_pop_.notify_one();
    }

    bool Pop(T& item) {
        std::unique_lock<std::mutex> lock(mutex_);
        cv_pop_.wait(lock, [this] { return !queue_.empty() || shutdown_; });
        if (queue_.empty() && shutdown_) return false;
        item = std::move(queue_.front());
        queue_.pop();
        cv_push_.notify_one();
        return true;
    }

    void Shutdown() {
        {
            std::lock_guard<std::mutex> lock(mutex_);
            shutdown_ = true;
        }
        cv_push_.notify_all();
        cv_pop_.notify_all();
    }

private:
    std::queue<T> queue_;
    size_t max_size_;
    bool shutdown_;
    std::mutex mutex_;
    std::condition_variable cv_push_;
    std::condition_variable cv_pop_;
};

// =================================================================================================
// Pipelined Overlap-Add Logic
// =================================================================================================

// =================================================================================================
// Pipelined Overlap-Add Logic (Optimized 3-Stage)
// =================================================================================================

std::vector<std::vector<float>> RoformerRuntime::ProcessOverlapAddPipelined(const std::vector<float>& input_audio,
                                                         int chunk_size,
                                                         int num_overlap,
                                                         std::function<void(int, int)> progress_callback,
                                                         CancelCallback cancel_callback,
                                                         int batch_size) {
    if (input_audio.empty()) return {};
    if (input_audio.size() % 2 != 0) {
        throw std::runtime_error("Error: Input audio must be interleaved stereo (even number of samples).");
    }

    // Parameters match the pinned demix contract.
    int channels = 2;
    int C = chunk_size;

    int step = chunk_size / num_overlap;
    int fade_size = chunk_size / 10;
    int border = chunk_size - step;

    int n_input_samples = input_audio.size() / channels;

    // 1. Pad Input
    bool do_pad = (n_input_samples > 2 * border) && (border > 0);
    int pad_l = do_pad ? border : 0;
    int pad_r = do_pad ? border : 0;
    int n_padded_samples = n_input_samples + pad_l + pad_r;
    const int total_chunks = (n_padded_samples + step - 1) / step;
    uta_diagnostics::Log("inference", "pipeline.begin",
        "input_frames=" + std::to_string(n_input_samples) +
        " padded_frames=" + std::to_string(n_padded_samples) +
        " chunk_size=" + std::to_string(chunk_size) +
        " step=" + std::to_string(step) +
        " overlap=" + std::to_string(num_overlap) +
        " batch_size=" + std::to_string(batch_size) +
        " total_chunks=" + std::to_string(total_chunks));

    std::vector<float> padded_input;

    if (do_pad) {
        padded_input.resize(n_padded_samples * channels);
        // Copy center
        for (int i = 0; i < n_input_samples; ++i) {
            padded_input[(pad_l + i) * channels + 0] = input_audio[i * channels + 0];
            padded_input[(pad_l + i) * channels + 1] = input_audio[i * channels + 1];
        }
        // Reflect Left
        for (int i = 0; i < pad_l; ++i) {
            int src_idx = 1 + i;
            if (src_idx >= n_input_samples) src_idx = n_input_samples - 1;
            int dst_idx = pad_l - 1 - i;
            padded_input[dst_idx * channels + 0] = input_audio[src_idx * channels + 0];
            padded_input[dst_idx * channels + 1] = input_audio[src_idx * channels + 1];
        }
        // Reflect Right
        for (int i = 0; i < pad_r; ++i) {
            int src_idx = n_input_samples - 2 - i;
            if (src_idx < 0) src_idx = 0;
            int dst_idx = pad_l + n_input_samples + i;
            padded_input[dst_idx * channels + 0] = input_audio[src_idx * channels + 0];
            padded_input[dst_idx * channels + 1] = input_audio[src_idx * channels + 1];
        }
    } else {
        padded_input = input_audio;
    }

    std::vector<std::vector<float>> result; // [stems][samples]
    std::vector<float> counter(n_padded_samples * channels, 0.0f);
    std::vector<float> window_base = GetWindow(chunk_size, fade_size);
    std::mutex result_mutex; // Protects 'result' and 'counter'
    std::atomic<bool> cancel_requested{false};

    // lambda to extract chunk 'i'
    auto extract_chunk = [&](int i) -> std::vector<float> {
        if (i >= n_padded_samples) return {};

        int remaining = n_padded_samples - i;
        int part_len = std::min(C, remaining);

        std::vector<float> chunk_in(C * channels, 0.0f);

        // Copy part
        for (int k = 0; k < part_len; ++k) {
            chunk_in[k * channels + 0] = padded_input[(i + k) * channels + 0];
            chunk_in[k * channels + 1] = padded_input[(i + k) * channels + 1];
        }

        // Pad short chunk if needed
        if (part_len < C) {
             int pad_amount = C - part_len;
             if (part_len > C / 2 + 1) {
                 // Reflect pad right
                 for(int k=0; k<pad_amount; ++k) {
                     int src_idx = part_len - 2 - k;
                     if(src_idx < 0) src_idx = 0;
                     chunk_in[(part_len + k)*2+0] = chunk_in[src_idx*2+0];
                     chunk_in[(part_len + k)*2+1] = chunk_in[src_idx*2+1];
                 }
             }
        }
        return chunk_in;
    };

    // lambda to accumulate result 'state' at offset 'i'
    // Now protected by mutex
    auto accumulate_result = [&](std::shared_ptr<ChunkState> state, int i) {
        if (!state) return;
        const std::vector<std::vector<float>>& chunk_out_stems = state->final_audio;
        if (chunk_out_stems.empty()) return;

        std::lock_guard<std::mutex> lock(result_mutex);

        // Lazy Initialize result
        if (result.empty()) {
            int num_stems = chunk_out_stems.size();
            result.resize(num_stems, std::vector<float>(n_padded_samples * channels, 0.0f));
        }

        int remaining = n_padded_samples - i;
        int part_len = std::min(C, remaining);

        std::vector<float> window = window_base; // Copy
        if (i == 0) {
            for(int k=0; k<fade_size; ++k) window[k] = 1.0f;
        } else if (i + step >= n_padded_samples) {
            for(int k=0; k<fade_size; ++k) window[C - 1 - k] = 1.0f;
        }

        int num_stems = result.size();
        for (int k = 0; k < part_len; ++k) {
            float w = window[k];
            int res_idx = (i + k) * channels;
            int chk_idx = k * channels;

            for (int s = 0; s < num_stems; ++s) {
                 if (s >= chunk_out_stems.size()) continue;
                 // result[s] is huge, but we access linearly in this block
                 result[s][res_idx + 0] += chunk_out_stems[s][chk_idx + 0] * w;
                 result[s][res_idx + 1] += chunk_out_stems[s][chk_idx + 1] * w;
            }

            // Counter is same for all stems, just update once
            counter[res_idx + 0] += w;
            counter[res_idx + 1] += w;
        }
    };

    // =================================================================================================
    // 3-Stage Pipeline
    // =================================================================================================

    // Queues
    // Bounded size to prevents running out of memory
    // 3 items buffer is enough to keep GPU busy
    ThreadSafeQueue<std::shared_ptr<ChunkState>> input_queue(3);
    ThreadSafeQueue<std::shared_ptr<ChunkState>> output_queue(3);
    std::mutex exception_mutex;
    std::exception_ptr pipeline_exception = nullptr;

    auto set_pipeline_exception = [&](std::exception_ptr eptr) {
        {
            std::lock_guard<std::mutex> lock(exception_mutex);
            if (!pipeline_exception) {
                pipeline_exception = eptr;
            }
        }
        cancel_requested.store(true, std::memory_order_release);
        input_queue.Shutdown();
        output_queue.Shutdown();
    };

    // Structure to hold chunk metadata together
    struct ChunkTask {
        int offset;
        std::shared_ptr<ChunkState> state;
    };

    // 1. Preprocessor Thread
    auto preproccessor = std::thread([&]() {
        try {
            CpuScratch preprocess_scratch;
            int current_offset = 0;
            int current_sequence = 0;
            while (current_offset < n_padded_samples && !cancel_requested.load(std::memory_order_acquire)) {
                std::vector<float> chunk = extract_chunk(current_offset);

                auto state = PreProcessChunk(chunk, current_offset, current_sequence, total_chunks, preprocess_scratch);

                input_queue.Push(state);
                if (cancel_requested.load(std::memory_order_acquire)) {
                    break;
                }
                current_offset += step;
                current_sequence += 1;
            }
        } catch (...) {
            set_pipeline_exception(std::current_exception());
        }
        input_queue.Shutdown();
    });

    // 3. Postprocessor Thread
    auto postprocessor = std::thread([&]() {
        try {
            CpuScratch postprocess_scratch;
            std::shared_ptr<ChunkState> state;
            while (!cancel_requested.load(std::memory_order_acquire) && output_queue.Pop(state)) {
                // This does ISTFT (CPU intensive)
                PostProcessChunk(state, postprocess_scratch);
                if (cancel_requested.load(std::memory_order_acquire)) {
                    break;
                }

                // Accumulate (Memory bandwidth intensive + Mutex)
                accumulate_result(state, state->id); // state->id holds offset
                uta_diagnostics::Log("inference", "chunk.accumulate.end",
                    ChunkDetails(state->sequence, state->total_chunks, state->id));

                if (!cancel_requested.load(std::memory_order_acquire) && progress_callback) {
                    progress_callback(state->sequence + 1, state->total_chunks);
                }
            }
        } catch (...) {
            set_pipeline_exception(std::current_exception());
        }
    });

    auto poll_cancel_requested = [&]() -> bool {
        if (cancel_requested.load(std::memory_order_acquire)) {
            return true;
        }
        if (cancel_callback && cancel_callback()) {
            cancel_requested.store(true, std::memory_order_release);
            return true;
        }
        return false;
    };

    // 2. Main Thread (RoformerRuntime Loop)
    bool cancelled = false;
    std::shared_ptr<ChunkState> state;
    try {
        bool input_exhausted = false;
        std::vector<std::shared_ptr<ChunkState>> batch_states;
        batch_states.reserve(batch_size);
        while (true) {
            if (poll_cancel_requested()) {
                cancelled = true;
                break;
            }

            bool ok = input_queue.Pop(state);
            if (!ok) break; // Input queue shutdown and empty

            batch_states.clear();
            batch_states.push_back(state);
            while (static_cast<int>(batch_states.size()) < batch_size) {
                if (poll_cancel_requested()) {
                    cancelled = true;
                    break;
                }
                std::shared_ptr<ChunkState> next_state;
                if (!input_queue.Pop(next_state)) {
                    input_exhausted = true;
                    break;
                }
                batch_states.push_back(std::move(next_state));
            }

            if (poll_cancel_requested()) {
                cancelled = true;
                break;
            }

            // This does GGML RoformerRuntime (GPU intensive, Blocking)
            RunInferenceBatch(batch_states);

            if (poll_cancel_requested()) {
                cancelled = true;
                break;
            }

            for (const auto& completed_state : batch_states) {
                output_queue.Push(completed_state);
            }
            if (input_exhausted) {
                break;
            }
        }
    } catch (...) {
        set_pipeline_exception(std::current_exception());
    }

    if (cancelled) {
        cancel_requested.store(true, std::memory_order_release);
        input_queue.Shutdown();
    }

    // Wait for threads
    output_queue.Shutdown();
    if (preproccessor.joinable()) preproccessor.join();
    if (postprocessor.joinable()) postprocessor.join();

    if (pipeline_exception) {
        std::rethrow_exception(pipeline_exception);
    }

    if (cancel_requested.load(std::memory_order_acquire)) {
        throw std::runtime_error(kInferenceCancelledMessage);
    }

    // Normalize and Crop
    // result is [stems][samples]
    if (result.empty()) return {};

    int num_stems = result.size();
    const size_t output_values = static_cast<size_t>(n_input_samples) * channels;
    for (int s = 0; s < num_stems; ++s) {
        for (int k = 0; k < n_input_samples; ++k) {
            int padded_idx = (pad_l + k) * channels;
            int final_idx = k * channels;

            float w0 = counter[padded_idx + 0];
            float w1 = counter[padded_idx + 1];

            if (w0 < 1e-4f) w0 = 1.0f;
            if (w1 < 1e-4f) w1 = 1.0f;

            result[s][final_idx + 0] = result[s][padded_idx + 0] / w0;
            result[s][final_idx + 1] = result[s][padded_idx + 1] / w1;
        }
        result[s].resize(output_values);
    }
    uta_diagnostics::Log("inference", "pipeline.end",
        "total_chunks=" + std::to_string(total_chunks) +
        " batch_size=" + std::to_string(batch_size) +
        " stems=" + std::to_string(result.size()));

    return result;
}

std::vector<std::vector<float>> RoformerRuntime::ProcessOverlapAdd(const std::vector<float>& input_audio,
                                                int chunk_size,
                                                int num_overlap,
                                                ModelCallback model_func,
                                                std::function<void(int, int)> progress_callback,
                                                CancelCallback cancel_callback) {
    if (input_audio.empty()) return {};
    if (input_audio.size() % 2 != 0) {
        throw std::runtime_error("Error: Input audio must be interleaved stereo (even number of samples).");
    }

    // Parameters match the pinned demix contract.
    int channels = 2;
    int C = chunk_size;

    int step = chunk_size / num_overlap;
    int fade_size = chunk_size / 10;
    int border = chunk_size - step;

    int n_input_samples = input_audio.size() / channels;

    // 1. Pad Input
    bool do_pad = (n_input_samples > 2 * border) && (border > 0);
    int pad_l = do_pad ? border : 0;
    int pad_r = do_pad ? border : 0;
    int n_padded_samples = n_input_samples + pad_l + pad_r;

    std::vector<float> padded_input;

    if (do_pad) {
        padded_input.resize(n_padded_samples * channels);

        // Copy center
        for (int i = 0; i < n_input_samples; ++i) {
            padded_input[(pad_l + i) * channels + 0] = input_audio[i * channels + 0];
            padded_input[(pad_l + i) * channels + 1] = input_audio[i * channels + 1];
        }
        // Reflect Left
        for (int i = 0; i < pad_l; ++i) {
            int src_idx = 1 + i;
            if (src_idx >= n_input_samples) src_idx = n_input_samples - 1;
            int dst_idx = pad_l - 1 - i;
            padded_input[dst_idx * channels + 0] = input_audio[src_idx * channels + 0];
            padded_input[dst_idx * channels + 1] = input_audio[src_idx * channels + 1];
        }
        // Reflect Right
        for (int i = 0; i < pad_r; ++i) {
            int src_idx = n_input_samples - 2 - i;
            if (src_idx < 0) src_idx = 0;
            int dst_idx = pad_l + n_input_samples + i;
            padded_input[dst_idx * channels + 0] = input_audio[src_idx * channels + 0];
            padded_input[dst_idx * channels + 1] = input_audio[src_idx * channels + 1];
        }
    } else {
        padded_input = input_audio;
    }

    std::vector<std::vector<float>> result; // [stems][samples]
    std::vector<float> counter(n_padded_samples * channels, 0.0f);
    std::vector<float> window_base = GetWindow(chunk_size, fade_size);

    int i = 0;
    int total_length = n_padded_samples;
    const int total_chunks = (total_length + step - 1) / step;
    int completed_chunks = 0;

    while (i < total_length) {
        if (cancel_callback && cancel_callback()) {
            throw std::runtime_error(kInferenceCancelledMessage);
        }

        int remaining = total_length - i;
        int part_len = std::min(C, remaining); // Clamp the final chunk to the remaining samples.

        std::vector<float> chunk_in(C * channels, 0.0f);

        // Copy part
        for (int k = 0; k < part_len; ++k) {
            chunk_in[k * channels + 0] = padded_input[(i + k) * channels + 0];
            chunk_in[k * channels + 1] = padded_input[(i + k) * channels + 1];
        }

        // Pad short chunk if needed
        if (part_len < C) {
             int pad_amount = C - part_len;
             if (part_len > C / 2 + 1) {
                 // Reflect pad right
                 for(int k=0; k<pad_amount; ++k) {
                     int src_idx = part_len - 2 - k;
                     if(src_idx < 0) src_idx = 0;
                     chunk_in[(part_len + k)*2+0] = chunk_in[src_idx*2+0];
                     chunk_in[(part_len + k)*2+1] = chunk_in[src_idx*2+1];
                 }
             }
        }

        std::vector<std::vector<float>> chunk_out_stems = model_func(chunk_in);
        if (chunk_out_stems.empty()) {
             // ?
        }

        // Lazy Initialize result
        if (result.empty()) {
            int num_stems = chunk_out_stems.size();
            result.resize(num_stems, std::vector<float>(n_padded_samples * channels, 0.0f));
        }

        // Window Adjustment
        std::vector<float> window = window_base; // Copy
        if (i == 0) {
            for(int k=0; k<fade_size; ++k) window[k] = 1.0f;
        } else if (i + step >= total_length) {
            for(int k=0; k<fade_size; ++k) window[C - 1 - k] = 1.0f;
        }

        // Accumulate
        int num_stems = result.size();
        for (int k = 0; k < part_len; ++k) {
            float w = window[k];
            int res_idx = (i + k) * channels;
            int chk_idx = k * channels;

            for (int s = 0; s < num_stems; ++s) {
                 if (s >= chunk_out_stems.size()) continue;
                 const auto& stem_chunk = chunk_out_stems[s];
                 result[s][res_idx + 0] += stem_chunk[chk_idx + 0] * w;
                 result[s][res_idx + 1] += stem_chunk[chk_idx + 1] * w;
            }

            counter[res_idx + 0] += w;
            counter[res_idx + 1] += w;
        }

        i += step;
        ++completed_chunks;
        if (progress_callback) {
             progress_callback(completed_chunks, total_chunks);
        }
    }

    // Normalize and Crop
    if (result.empty()) return {};

    int num_stems = result.size();
    const size_t output_values = static_cast<size_t>(n_input_samples) * channels;
    for (int s = 0; s < num_stems; ++s) {
        for (int k = 0; k < n_input_samples; ++k) {
            int padded_idx = (pad_l + k) * channels;
            int final_idx = k * channels;

            float w0 = counter[padded_idx + 0];
            float w1 = counter[padded_idx + 1];

            if (w0 < 1e-4f) w0 = 1.0f;
            if (w1 < 1e-4f) w1 = 1.0f;

            result[s][final_idx + 0] = result[s][padded_idx + 0] / w0;
            result[s][final_idx + 1] = result[s][padded_idx + 1] / w1;
        }
        result[s].resize(output_values);
    }

    return result;
}
