#pragma once

#include <vector>
#include <string>
#include <memory>
#include <functional>
#include <unordered_map>
#include <cstddef>
#include <cstdint>
// Forward declaration
class UtaRoformerGraph;

struct ggml_context;
struct ggml_cgraph;
struct ggml_gallocr;
struct ggml_tensor;

class RoformerRuntime {
public:
    using CancelCallback = std::function<bool()>;

    RoformerRuntime(const std::string& model_path);
    ~RoformerRuntime();

    // Process a full audio track (interleaved stereo float32)
    // Uses overlap-add chunking to handle long files
    // Process a full audio track (interleaved stereo float32)
    // Returns a vector of stems, where each stem is an interleaved stereo float vector
    std::vector<std::vector<float>> Process(const std::vector<float>& input_audio,
                               int chunk_size = 352800,
                               int num_overlap = 2,
                               std::function<void(int, int)> progress_callback = nullptr,
                               CancelCallback cancel_callback = nullptr,
                               int batch_size = 1,
                               bool serial_pipeline = false);

    // Low-level chunk processing (public for testing)
    std::vector<std::vector<float>> ProcessChunk(const std::vector<float>& chunk_audio);

    // Get model's recommended inference defaults
    int GetDefaultChunkSize() const;
    int GetDefaultNumOverlap() const;
    int GetSampleRate() const;
    int GetNumStems() const;

    // Static helper for the pinned overlap-add contract.
    // model_func: input [samples], output [stems][samples] (interleaved stereo)
    using ModelCallback = std::function<std::vector<std::vector<float>>(const std::vector<float>&)>;
    static std::vector<std::vector<float>> ProcessOverlapAdd(const std::vector<float>& input_audio,
                                                int chunk_size,
                                                int num_overlap,
                                                ModelCallback model_func,
                                                std::function<void(int, int)> progress_callback = nullptr,
                                                CancelCallback cancel_callback = nullptr);

private:
    // Pipelined Overlap-Add
    std::vector<std::vector<float>> ProcessOverlapAddPipelined(const std::vector<float>& input_audio,
                                                  int chunk_size,
                                                  int num_overlap,
                                                  std::function<void(int, int)> progress_callback,
                                                  CancelCallback cancel_callback,
                                                  int batch_size);

private:
    std::unique_ptr<UtaRoformerGraph> model_;
    struct CpuScratch;

    struct GraphState {
        int n_frames = -1;
        int batch_size = -1;
        ggml_context* ctx = nullptr;
        ggml_cgraph* gf = nullptr;
        ggml_gallocr* allocr = nullptr;
        ggml_tensor* input_tensor = nullptr;
        ggml_tensor* pos_time = nullptr;
        ggml_tensor* pos_freq = nullptr;
        ggml_tensor* mask_out_tensor = nullptr;
        std::vector<int32_t> pos_time_data;
        std::vector<int32_t> pos_freq_data;
        std::vector<float> input_data;
        std::vector<float> output_data;
        size_t compute_buffer_size = 0;
        int graph_nodes = 0;
    };

    std::unordered_map<std::uint64_t, std::unique_ptr<GraphState>> graph_cache_;

    // Pipelined State Data
    struct ChunkState {
        int id = -1;
        int sequence = -1;
        int total_chunks = -1;
        std::vector<float> input_audio;       // Original chunk audio
        std::vector<float> stft_flattened;    // [Prepared Input for GPU]
        std::vector<std::vector<float>> stft_outputs; // Kept for reconstruction
        int n_frames = 0;

        std::vector<float> mask_output;       // Output from GPU
        std::vector<std::vector<float>> final_audio;       // Result after ISTFT [stems][samples]
    };

    // Helper to ensure graph is built for specific n_frames
    GraphState* EnsureGraph(int n_frames, int batch_size);
    void ReleaseGraphState(GraphState& graph);
    void ClearGraphCache();

    void ComputeSTFT(const std::vector<float>& input_audio,
                     std::vector<std::vector<float>>& stft_outputs,
                     int& n_frames,
                     CpuScratch& scratch);

    void PrepareModelInput(const std::vector<std::vector<float>>& stft_outputs,
                           int n_frames,
                           std::vector<float>& model_input_rearranged);

    void PostProcessAndISTFT(const std::vector<float>& mask_output,
                             const std::vector<std::vector<float>>& stft_outputs,
                             int n_frames,
                             std::vector<std::vector<float>>& output_audio,
                             CpuScratch& scratch);

    // Pipeline Steps
    std::shared_ptr<ChunkState> PreProcessChunk(const std::vector<float>& chunk_audio,
                                                int id,
                                                int sequence,
                                                int total_chunks,
                                                CpuScratch& scratch);
    void RunInference(std::shared_ptr<ChunkState> state);
    void RunInferenceBatch(const std::vector<std::shared_ptr<ChunkState>>& states);
    void PostProcessChunk(std::shared_ptr<ChunkState> state, CpuScratch& scratch);
};
