#pragma once
// RMVPE (E2E0 U-Net + bidirectional GRU) GGML compute graph. Architecture and
// every tensor name below were confirmed node-by-node against the pinned
// rmvpe.onnx graph rather than assumed from the published paper. GGML-specific
// op conventions include the native Conv2d kernel layout, an explicit padded
// ConvTranspose2d crop, and manual BatchNorm.

#include <string>
#include <utility>
#include <vector>

#include "ggml.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "gguf.h"

namespace rmvpe {

class UtaRmvpeGraph {
public:
    ~UtaRmvpeGraph();

    // device: -1 = CPU backend, >=0 = Vulkan device index.
    void Initialize(int device);
    void LoadWeights(const std::string& gguf_path);

    ggml_backend_t GetBackend() const { return backend_; }

    // mel_input: ne=[T,128] (time fastest, matching mel.h's ToChannelMajorWindow
    // buffer layout exactly -- see graph.cpp's Build() for the derivation of
    // why this needs one transpose to reach the U-Net's expected [128,T]
    // (mel fastest) layout). Builds the full forward pass and returns the
    // final [360,T] sigmoid tensor (already registered on gf via
    // ggml_build_forward_expand).
    // debug_taps, if non-null, is filled with named intermediate checkpoint
    // tensors (all also marked ggml_set_output) for validation against the
    // ONNX Runtime reference dumps: enc4, bottleneck, dec4, cnn_head,
    // gru_input, gru_out, final -- see graph.cpp's Build().
    //
    // NOTE: this one-shot path unrolls the *entire* bidirectional GRU (both
    // directions, all T timesteps) into a single graph, so it is only safe up
    // to T~128-192. Real RMVPE windows are up
    // to 1024 frames; the runtime driver must use the three-stage
    // BuildCnnHead/BuildGruDirectionChunk/BuildOutputHead split below instead,
    // chunking the GRU into <=128-frame sub-chunks with host-side hidden-state
    // carry-over between chunks. Build() stays as-is for small-window testing.
    ggml_tensor* Build(ggml_context* ctx, ggml_cgraph* gf, ggml_tensor* mel_input, int T,
                       std::vector<std::pair<std::string, ggml_tensor*>>* debug_taps = nullptr);

    // ---- Three-stage split for windows above the GRU's per-graph timestep
    // ceiling. Stages 1 and 3 have no per-timestep
    // unrolling (conv/batchnorm/pool ops all operate on the whole T at once),
    // so they are safe for any real window size (up to 1024 frames). Only
    // stage 2 needs chunking, and every one of its
    // per-chunk graphs must get a *fresh* ggml_gallocr_alloc_graph plus full
    // weight re-upload -- no caching/reuse of one allocated graph shape across
    // calls for this recurrent graph shape.

    // Stage 1: mel input -> the U-Net's [384,T] GRU-input sequence.
    ggml_tensor* BuildCnnHead(ggml_context* ctx, ggml_cgraph* gf, ggml_tensor* mel_input, int T,
                              std::vector<std::pair<std::string, ggml_tensor*>>* debug_taps = nullptr);

    // Stage 2: one direction's GRU cell chain over ONE sub-chunk of the full
    // sequence (T_chunk must stay under the ~128-192 ceiling). direction:
    // 0=forward, 1=backward. Forward chunks must be built and computed in
    // left-to-right order across the full window; backward chunks in
    // right-to-left order -- each direction's hidden state only carries
    // within that direction's own chunk sequence, independent of the other
    // direction. The returned h_prev tensor is an input the caller fills
    // (via ggml_backend_tensor_set, after allocation) with the previous
    // chunk's h_final in this direction's order, or zeros for that
    // direction's first chunk. h_final (ne=[256], marked ggml_set_output) is
    // this chunk's last cell state in this direction's iteration order --
    // read it back and feed it as the next chunk's h_prev input data. out
    // (ne=[256,T_chunk]) holds this direction's hidden state for every
    // timestep of the chunk, indexed in the chunk's own natural (left-to-
    // right) time order regardless of direction.
    struct GruChunkResult {
        ggml_tensor* out;
        ggml_tensor* h_final;
        ggml_tensor* h_prev; // input tensor -- caller uploads data here
    };
    GruChunkResult BuildGruDirectionChunk(ggml_context* ctx, ggml_cgraph* gf, ggml_tensor* x_chunk,
                                          int T_chunk, int direction);

    // Stage 3: the full [512,T] concatenated (forward-then-backward channel
    // order, matching BidirectionalGru/ONNX direction 0=forward) GRU output
    // sequence -> fc.1 + sigmoid, ne=[360,T].
    ggml_tensor* BuildOutputHead(ggml_context* ctx, ggml_cgraph* gf, ggml_tensor* gru_out_full, int T);

private:
    ggml_backend_t backend_ = nullptr;
    ggml_context* ctx_weights_ = nullptr;
    ggml_backend_buffer_t buffer_weights_ = nullptr;

    ggml_tensor* GetWeight(const std::string& name) const;

    // x: ne=[W,H,C,N]. Returns ne=[W,H,C,N] (channel count of `weight_name`'s OC).
    ggml_tensor* Conv(ggml_context* ctx, const std::string& weight_name, ggml_tensor* x, int stride, int pad);
    ggml_tensor* BatchNorm(ggml_context* ctx, const std::string& prefix, ggml_tensor* x);
    // One ResConvBlock: Conv3x3->ReLU->Conv3x3->ReLU + (shortcut Conv1x1 if
    // `prefix.shortcut.weight` exists in the GGUF, else identity) -- no ReLU
    // after the Add (confirmed from the ONNX graph's node connectivity).
    ggml_tensor* ResConvBlock(ggml_context* ctx, const std::string& prefix, ggml_tensor* x);
    ggml_tensor* AvgPool2x2(ggml_context* ctx, ggml_tensor* x);
    // ConvTranspose2d(stride=2,pad=1,output_padding=1) via
    // ggml_conv_transpose_2d_p0 + the empirically-verified start-crop recipe
    // (see graph.cpp).
    ggml_tensor* ConvTransposeUpsample(ggml_context* ctx, const std::string& weight_name, ggml_tensor* x);

    ggml_tensor* BidirectionalGru(ggml_context* ctx, ggml_tensor* x, int T);
};

} // namespace rmvpe
