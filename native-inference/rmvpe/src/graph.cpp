#include "graph.h"

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#include <vector>

#ifdef HAVE_GGML_VULKAN
#include "ggml-vulkan.h"
#endif

namespace rmvpe {

UtaRmvpeGraph::~UtaRmvpeGraph() {
    if (buffer_weights_) ggml_backend_buffer_free(buffer_weights_);
    if (ctx_weights_) ggml_free(ctx_weights_);
    if (backend_) ggml_backend_free(backend_);
}

void UtaRmvpeGraph::Initialize(int device) {
    if (backend_) throw std::runtime_error("RMVPE backend is already initialized");
    if (device < 0) {
        backend_ = ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_CPU, nullptr);
    } else {
#ifdef HAVE_GGML_VULKAN
        const int device_count = ggml_backend_vk_get_device_count();
        if (device >= device_count) {
            throw std::runtime_error(
                "GGML Vulkan device " + std::to_string(device) + " is unavailable (device count " +
                std::to_string(device_count) + "); refusing an implicit CPU fallback");
        }
        backend_ = ggml_backend_vk_init(static_cast<size_t>(device));
#else
        throw std::runtime_error("built without Vulkan support");
#endif
    }
    if (!backend_) {
        throw std::runtime_error(device < 0
            ? "failed to initialize the diagnostic GGML CPU backend"
            : "failed to initialize the requested GGML Vulkan device; refusing an implicit CPU fallback");
    }
}

void UtaRmvpeGraph::LoadWeights(const std::string& path) {
    struct gguf_init_params params = { /*.no_alloc=*/true, /*.ctx=*/&ctx_weights_ };
    struct gguf_context* ctx_gguf = gguf_init_from_file(path.c_str(), params);
    if (!ctx_gguf) throw std::runtime_error("failed to load GGUF file: " + path);

    int arch_idx = gguf_find_key(ctx_gguf, "general.architecture");
    if (arch_idx < 0 || std::string(gguf_get_val_str(ctx_gguf, arch_idx)) != "rmvpe") {
        gguf_free(ctx_gguf);
        throw std::runtime_error("GGUF general.architecture is not 'rmvpe'");
    }
    const auto require_u32 = [&](const char* key, uint32_t expected) {
        const int index = gguf_find_key(ctx_gguf, key);
        if (index < 0 || gguf_get_val_u32(ctx_gguf, index) != expected) {
            throw std::runtime_error(std::string("RMVPE GGUF metadata mismatch: ") + key);
        }
    };
    const auto require_bool = [&](const char* key, bool expected) {
        const int index = gguf_find_key(ctx_gguf, key);
        if (index < 0 || gguf_get_val_bool(ctx_gguf, index) != expected) {
            throw std::runtime_error(std::string("RMVPE GGUF metadata mismatch: ") + key);
        }
    };
    try {
        require_u32("rmvpe.sample_rate", 16'000);
        require_u32("rmvpe.n_fft", 1'024);
        require_u32("rmvpe.hop_length", 160);
        require_u32("rmvpe.mel_bins", 128);
        require_u32("rmvpe.pitch_classes", 360);
        require_u32("rmvpe.gru_input_size", 384);
        require_u32("rmvpe.gru_hidden_size", 256);
        require_bool("rmvpe.gru_bidirectional", true);
        require_bool("rmvpe.gru_linear_before_reset", true);
        require_u32("rmvpe.cnn_head_out_channels", 3);
        require_u32("rmvpe.encoder_stages", 5);
        require_u32("rmvpe.bottleneck_stages", 4);
        require_u32("rmvpe.bottleneck_channels", 512);
        require_u32("rmvpe.decoder_stages", 5);
        require_u32("rmvpe.blocks_per_stage", 4);
        if (gguf_get_n_tensors(ctx_gguf) != 282) {
            throw std::runtime_error("RMVPE GGUF tensor count is not 282");
        }
    } catch (...) {
        gguf_free(ctx_gguf);
        throw;
    }

    buffer_weights_ = ggml_backend_alloc_ctx_tensors_from_buft(
        ctx_weights_, ggml_backend_get_default_buffer_type(backend_));
    if (!buffer_weights_) {
        gguf_free(ctx_gguf);
        throw std::runtime_error("failed to allocate weight buffer");
    }

    FILE* file = fopen(path.c_str(), "rb");
    if (!file) {
        gguf_free(ctx_gguf);
        throw std::runtime_error("cannot open " + path);
    }
    try {
        const size_t data_offset = gguf_get_data_offset(ctx_gguf);
        std::vector<uint8_t> read_buf;
        for (ggml_tensor* t = ggml_get_first_tensor(ctx_weights_); t;
             t = ggml_get_next_tensor(ctx_weights_, t)) {
            if (t->type != GGML_TYPE_F32) {
                throw std::runtime_error(std::string("RMVPE tensor is not F32: ") + t->name);
            }
            const int tid = gguf_find_tensor(ctx_gguf, t->name);
            if (tid < 0) {
                throw std::runtime_error(std::string("tensor missing from GGUF data: ") + t->name);
            }
            const size_t offset = data_offset + gguf_get_tensor_offset(ctx_gguf, tid);
            const size_t size = ggml_nbytes(t);
            if (read_buf.size() < size) read_buf.resize(size);
            if (fseek(file, static_cast<long>(offset), SEEK_SET) != 0 ||
                fread(read_buf.data(), 1, size, file) != size) {
                throw std::runtime_error(std::string("short read for tensor ") + t->name);
            }
            ggml_backend_tensor_set(t, read_buf.data(), 0, size);
        }
    } catch (...) {
        fclose(file);
        gguf_free(ctx_gguf);
        throw;
    }
    fclose(file);
    gguf_free(ctx_gguf);
}

ggml_tensor* UtaRmvpeGraph::GetWeight(const std::string& name) const {
    return ggml_get_tensor(ctx_weights_, name.c_str());
}

ggml_tensor* UtaRmvpeGraph::Conv(ggml_context* ctx, const std::string& weight_name, ggml_tensor* x, int stride, int pad) {
    ggml_tensor* w = GetWeight(weight_name + ".weight");
    if (!w) throw std::runtime_error("missing conv weight: " + weight_name);
    ggml_tensor* out = ggml_conv_2d(ctx, w, x, stride, stride, pad, pad, 1, 1);
    ggml_tensor* b = GetWeight(weight_name + ".bias");
    if (b) {
        ggml_tensor* bias4d = ggml_reshape_4d(ctx, b, 1, 1, b->ne[0], 1);
        out = ggml_add(ctx, out, bias4d);
    }
    return out;
}

ggml_tensor* UtaRmvpeGraph::BatchNorm(ggml_context* ctx, const std::string& prefix, ggml_tensor* x) {
    ggml_tensor* scale = GetWeight(prefix + ".weight");
    ggml_tensor* bias = GetWeight(prefix + ".bias");
    ggml_tensor* mean = GetWeight(prefix + ".running_mean");
    ggml_tensor* var = GetWeight(prefix + ".running_var");
    if (!scale || !bias || !mean || !var) throw std::runtime_error("missing batchnorm tensors: " + prefix);
    const int64_t c = scale->ne[0];
    ggml_tensor* scale4d = ggml_reshape_4d(ctx, scale, 1, 1, c, 1);
    ggml_tensor* bias4d = ggml_reshape_4d(ctx, bias, 1, 1, c, 1);
    ggml_tensor* mean4d = ggml_reshape_4d(ctx, mean, 1, 1, c, 1);
    // sqrt(var+eps) computed on the fly (ONNX BatchNormalization default eps=1e-5).
    ggml_tensor* std_dev = ggml_sqrt(ctx, ggml_scale_bias(ctx, var, 1.0f, 1.0e-5f));
    ggml_tensor* std4d = ggml_reshape_4d(ctx, std_dev, 1, 1, c, 1);
    ggml_tensor* centered = ggml_sub(ctx, x, mean4d);
    ggml_tensor* normalized = ggml_div(ctx, centered, std4d);
    ggml_tensor* scaled = ggml_mul(ctx, normalized, scale4d);
    return ggml_add(ctx, scaled, bias4d);
}

ggml_tensor* UtaRmvpeGraph::ResConvBlock(ggml_context* ctx, const std::string& prefix, ggml_tensor* x) {
    ggml_tensor* main = Conv(ctx, prefix + ".conv.conv.0", x, 1, 1);
    main = ggml_relu(ctx, main);
    main = Conv(ctx, prefix + ".conv.conv.3", main, 1, 1);
    main = ggml_relu(ctx, main);
    ggml_tensor* shortcut_w = GetWeight(prefix + ".shortcut.weight");
    ggml_tensor* residual = shortcut_w ? Conv(ctx, prefix + ".shortcut", x, 1, 0) : x;
    return ggml_add(ctx, main, residual); // no ReLU after Add -- confirmed from the ONNX graph
}

ggml_tensor* UtaRmvpeGraph::AvgPool2x2(ggml_context* ctx, ggml_tensor* x) {
    return ggml_pool_2d(ctx, x, GGML_OP_POOL_AVG, 2, 2, 2, 2, 0.0f, 0.0f);
}

ggml_tensor* UtaRmvpeGraph::ConvTransposeUpsample(ggml_context* ctx, const std::string& weight_name, ggml_tensor* x) {
    ggml_tensor* w = GetWeight(weight_name);
    if (!w) throw std::runtime_error("missing conv_transpose weight: " + weight_name);
    ggml_tensor* full = ggml_conv_transpose_2d_p0(ctx, w, x, 2);
    // Empirically verified crop recipe for ONNX ConvTranspose2d(pad=1,
    // output_padding=1,stride=2,kernel=3):
    // view starting at offset 1 on each spatial axis, length = full-1.
    const int pad = 1, output_pad = 1;
    const int64_t target_w = full->ne[0] - 2 * pad + output_pad;
    const int64_t target_h = full->ne[1] - 2 * pad + output_pad;
    const size_t offset = (size_t)pad * full->nb[0] + (size_t)pad * full->nb[1];
    ggml_tensor* cropped = ggml_view_4d(ctx, full, target_w, target_h, full->ne[2], full->ne[3],
                                        full->nb[1], full->nb[2], full->nb[3], offset);
    return ggml_cont(ctx, cropped);
}

namespace {

// Balanced-tree concat (mirrors native-inference/roformer's ConcatBalanced) --
// a sequential running-concat chain overflows ggml-alloc's fixed-size
// free-block list at real RMVPE window sizes.
ggml_tensor* ConcatBalanced(ggml_context* ctx, std::vector<ggml_tensor*> leaves, int dim) {
    while (leaves.size() > 1) {
        std::vector<ggml_tensor*> next;
        for (size_t i = 0; i < leaves.size(); i += 2) {
            if (i + 1 < leaves.size()) next.push_back(ggml_concat(ctx, leaves[i], leaves[i + 1], dim));
            else next.push_back(leaves[i]);
        }
        leaves = next;
    }
    return leaves[0];
}

struct GruDirectionWeights {
    ggml_tensor *Wz, *Wr, *Wh; // ne=[D,H]
    ggml_tensor *Rz, *Rr, *Rh; // ne=[H,H]
    ggml_tensor *bias_z, *bias_r, *Wbh, *Rbh; // ne=[H]
};

ggml_tensor* GateView2d(ggml_context* ctx, ggml_tensor* weight_ihh, int direction, int gate, int64_t d0, int64_t rows) {
    // weight_ihh: ne=[d0, 768, 2] (D or H, 3*H, num_directions).
    const size_t offset = (size_t)direction * weight_ihh->nb[2] + (size_t)gate * rows * weight_ihh->nb[1];
    return ggml_view_2d(ctx, weight_ihh, d0, rows, weight_ihh->nb[1], offset);
}

ggml_tensor* BiasView1d(ggml_context* ctx, ggml_tensor* bias, int direction, int part, int64_t hidden) {
    // bias: ne=[1536, 2] (6*H, num_directions). part 0..5 = Wbz,Wbr,Wbh,Rbz,Rbr,Rbh.
    const size_t offset = (size_t)direction * bias->nb[1] + (size_t)part * hidden * bias->nb[0];
    return ggml_view_1d(ctx, bias, hidden, offset);
}

GruDirectionWeights LoadGruDirection(ggml_context* ctx, ggml_tensor* weight_ih, ggml_tensor* weight_hh,
                                     ggml_tensor* bias, int direction, int64_t D, int64_t H) {
    GruDirectionWeights w{};
    w.Wz = GateView2d(ctx, weight_ih, direction, 0, D, H);
    w.Wr = GateView2d(ctx, weight_ih, direction, 1, D, H);
    w.Wh = GateView2d(ctx, weight_ih, direction, 2, D, H);
    w.Rz = GateView2d(ctx, weight_hh, direction, 0, H, H);
    w.Rr = GateView2d(ctx, weight_hh, direction, 1, H, H);
    w.Rh = GateView2d(ctx, weight_hh, direction, 2, H, H);
    ggml_tensor* Wbz = BiasView1d(ctx, bias, direction, 0, H);
    ggml_tensor* Wbr = BiasView1d(ctx, bias, direction, 1, H);
    w.Wbh = BiasView1d(ctx, bias, direction, 2, H);
    ggml_tensor* Rbz = BiasView1d(ctx, bias, direction, 3, H);
    ggml_tensor* Rbr = BiasView1d(ctx, bias, direction, 4, H);
    w.Rbh = BiasView1d(ctx, bias, direction, 5, H);
    w.bias_z = ggml_add(ctx, Wbz, Rbz);
    w.bias_r = ggml_add(ctx, Wbr, Rbr);
    return w;
}

// ONNX GRU semantics, gate order z,r,h, linear_before_reset=1 (confirmed from
// the real rmvpe.onnx GRU node's attributes. The equation order preserves the
// ONNX linear-before-reset definition.
ggml_tensor* GruCell(ggml_context* ctx, const GruDirectionWeights& w, ggml_tensor* x_t, ggml_tensor* h_prev) {
    ggml_tensor* z_pre = ggml_add(ctx, ggml_add(ctx, ggml_mul_mat(ctx, w.Wz, x_t), ggml_mul_mat(ctx, w.Rz, h_prev)), w.bias_z);
    ggml_tensor* z_t = ggml_sigmoid(ctx, z_pre);
    ggml_tensor* r_pre = ggml_add(ctx, ggml_add(ctx, ggml_mul_mat(ctx, w.Wr, x_t), ggml_mul_mat(ctx, w.Rr, h_prev)), w.bias_r);
    ggml_tensor* r_t = ggml_sigmoid(ctx, r_pre);
    ggml_tensor* rh_b = ggml_add(ctx, ggml_mul_mat(ctx, w.Rh, h_prev), w.Rbh);
    ggml_tensor* gated = ggml_mul(ctx, r_t, rh_b);
    ggml_tensor* h_tilde_pre = ggml_add(ctx, ggml_add(ctx, ggml_mul_mat(ctx, w.Wh, x_t), gated), w.Wbh);
    ggml_tensor* h_tilde = ggml_tanh(ctx, h_tilde_pre);
    ggml_tensor* diff = ggml_sub(ctx, h_prev, h_tilde);
    return ggml_add(ctx, h_tilde, ggml_mul(ctx, z_t, diff));
}

} // namespace

ggml_tensor* UtaRmvpeGraph::BidirectionalGru(ggml_context* ctx, ggml_tensor* x, int T) {
    // x: ne=[D=384, T].
    const int64_t D = x->ne[0];
    const int64_t H = 256;
    ggml_tensor* weight_ih = GetWeight("gru.weight_ih"); // ne=[384,768,2]
    ggml_tensor* weight_hh = GetWeight("gru.weight_hh"); // ne=[256,768,2]
    ggml_tensor* bias = GetWeight("gru.bias");           // ne=[1536,2]
    if (!weight_ih || !weight_hh || !bias) throw std::runtime_error("missing GRU weights");

    ggml_tensor* h0 = ggml_new_tensor_1d(ctx, GGML_TYPE_F32, H);
    // h0 must be zero; since this graph is rebuilt fresh per sub-chunk
    // for each compute, the caller fills it via ggml_backend_tensor_set
    // after allocation -- Build() marks it as an input for that purpose.
    ggml_set_input(h0);
    ggml_set_name(h0, "rmvpe.gru.h0");

    GruDirectionWeights fwd_w = LoadGruDirection(ctx, weight_ih, weight_hh, bias, 0, D, H);
    GruDirectionWeights bwd_w = LoadGruDirection(ctx, weight_ih, weight_hh, bias, 1, D, H);

    std::vector<ggml_tensor*> fwd_seq(T), bwd_seq(T);
    ggml_tensor* h_prev = h0;
    for (int t = 0; t < T; ++t) {
        ggml_tensor* x_t = ggml_view_1d(ctx, x, D, (size_t)t * D * ggml_element_size(x));
        ggml_tensor* h_t = GruCell(ctx, fwd_w, x_t, h_prev);
        fwd_seq[t] = ggml_reshape_2d(ctx, h_t, H, 1);
        h_prev = h_t;
    }
    h_prev = h0;
    for (int t = T - 1; t >= 0; --t) {
        ggml_tensor* x_t = ggml_view_1d(ctx, x, D, (size_t)t * D * ggml_element_size(x));
        ggml_tensor* h_t = GruCell(ctx, bwd_w, x_t, h_prev);
        bwd_seq[t] = ggml_reshape_2d(ctx, h_t, H, 1);
        h_prev = h_t;
    }
    ggml_tensor* fwd_out = ConcatBalanced(ctx, fwd_seq, 1); // ne=[H,T]
    ggml_tensor* bwd_out = ConcatBalanced(ctx, bwd_seq, 1); // ne=[H,T]
    return ggml_concat(ctx, fwd_out, bwd_out, 0); // ne=[2H,T], forward-first (ONNX direction0=forward)
}

ggml_tensor* UtaRmvpeGraph::BuildCnnHead(ggml_context* ctx, ggml_cgraph* gf, ggml_tensor* mel_input, int T,
                                         std::vector<std::pair<std::string, ggml_tensor*>>* debug_taps) {
    auto tap = [&](const char* name, ggml_tensor* t) {
        if (debug_taps) {
            ggml_set_output(t);
            ggml_build_forward_expand(gf, t);
            debug_taps->emplace_back(name, t);
        }
        return t;
    };
    // mel_input: ne=[T,128] (time fastest, matching mel.h's raw buffer layout,
    // which itself matches the ONNX graph's raw `input` [1,128,T] byte layout
    // -- see graph.h's doc comment). The ONNX graph's Transpose(perm=[0,2,1])
    // then Unsqueeze(axis=1) produces [1,1,T,128]; the equivalent ggml step
    // is swapping which axis is fastest (mel, not time) via transpose+cont.
    ggml_tensor* image = ggml_cont(ctx, ggml_transpose(ctx, mel_input)); // ne=[128,T,1,1]

    // Single 1-channel affine BatchNorm applied once before the encoder.
    ggml_tensor* x = BatchNorm(ctx, "unet.encoder.bn", image);

    ggml_tensor* skips[5];
    for (int stage = 0; stage < 5; ++stage) {
        for (int block = 0; block < 4; ++block) {
            x = ResConvBlock(ctx, "unet.encoder.layers." + std::to_string(stage) + ".conv." + std::to_string(block), x);
        }
        skips[stage] = x; // pre-pool output, needed for the matching decoder stage's skip connection
        if (stage == 4) tap("enc4_out", x);
        x = AvgPool2x2(ctx, x);
    }

    for (int stage = 0; stage < 4; ++stage) {
        for (int block = 0; block < 4; ++block) {
            x = ResConvBlock(ctx, "unet.intermediate.layers." + std::to_string(stage) + ".conv." + std::to_string(block), x);
        }
    }
    tap("bottleneck_out", x);

    for (int stage = 0; stage < 5; ++stage) {
        const std::string prefix = "unet.decoder.layers." + std::to_string(stage);
        ggml_tensor* up = ConvTransposeUpsample(ctx, prefix + ".conv1.conv1.0.weight", x);
        up = BatchNorm(ctx, prefix + ".conv1.conv1.1", up);
        up = ggml_relu(ctx, up);
        ggml_tensor* skip = skips[4 - stage]; // decoder.0<->encoder.4, decoder.1<->encoder.3, ...
        x = ggml_concat(ctx, up, skip, 2); // channel axis = ne2
        for (int block = 0; block < 4; ++block) {
            x = ResConvBlock(ctx, prefix + ".conv2." + std::to_string(block), x);
        }
        if (stage == 4) tap("dec4_out", x);
    }

    // Final 16->3 channel head conv (with bias), then reshape into the GRU's
    // [384,T] input: swap time/channel axes (matching ONNX's
    // Transpose(perm=[0,2,1,3])) then flatten (channel,mel)->384.
    ggml_tensor* head = Conv(ctx, "cnn", x, 1, 1); // ne=[128,T,3,1]
    tap("cnn_out", head);
    ggml_tensor* permuted = ggml_cont(ctx, ggml_permute(ctx, head, 0, 2, 1, 3)); // ne=[128,3,T,1]
    ggml_tensor* gru_input = ggml_reshape_2d(ctx, permuted, 384, T); // ne=[384,T]
    ggml_set_output(gru_input);
    ggml_build_forward_expand(gf, gru_input);
    if (debug_taps) debug_taps->emplace_back("gru_input", gru_input);
    return gru_input;
}

UtaRmvpeGraph::GruChunkResult UtaRmvpeGraph::BuildGruDirectionChunk(
        ggml_context* ctx, ggml_cgraph* gf, ggml_tensor* x_chunk, int T_chunk, int direction) {
    // x_chunk: ne=[D=384,T_chunk], one contiguous slice of the full window's
    // BuildCnnHead output (re-uploaded fresh for this chunk -- see the class
    // doc comment's Constraint B note on why the whole graph is rebuilt per
    // chunk rather than reusing one allocation).
    const int64_t D = x_chunk->ne[0];
    const int64_t H = 256;
    ggml_tensor* weight_ih = GetWeight("gru.weight_ih"); // ne=[384,768,2]
    ggml_tensor* weight_hh = GetWeight("gru.weight_hh"); // ne=[256,768,2]
    ggml_tensor* bias = GetWeight("gru.bias");           // ne=[1536,2]
    if (!weight_ih || !weight_hh || !bias) throw std::runtime_error("missing GRU weights");

    ggml_tensor* h_prev_in = ggml_new_tensor_1d(ctx, GGML_TYPE_F32, H);
    ggml_set_input(h_prev_in);
    ggml_set_name(h_prev_in, direction == 0 ? "rmvpe.gru.h_prev.fwd" : "rmvpe.gru.h_prev.bwd");

    GruDirectionWeights w = LoadGruDirection(ctx, weight_ih, weight_hh, bias, direction, D, H);

    std::vector<ggml_tensor*> seq(T_chunk);
    ggml_tensor* h_prev = h_prev_in;
    if (direction == 0) {
        for (int t = 0; t < T_chunk; ++t) {
            ggml_tensor* x_t = ggml_view_1d(ctx, x_chunk, D, (size_t)t * D * ggml_element_size(x_chunk));
            ggml_tensor* h_t = GruCell(ctx, w, x_t, h_prev);
            seq[t] = ggml_reshape_2d(ctx, h_t, H, 1);
            h_prev = h_t;
        }
    } else {
        for (int t = T_chunk - 1; t >= 0; --t) {
            ggml_tensor* x_t = ggml_view_1d(ctx, x_chunk, D, (size_t)t * D * ggml_element_size(x_chunk));
            ggml_tensor* h_t = GruCell(ctx, w, x_t, h_prev);
            seq[t] = ggml_reshape_2d(ctx, h_t, H, 1);
            h_prev = h_t;
        }
    }
    ggml_tensor* out = ConcatBalanced(ctx, seq, 1); // ne=[H,T_chunk]
    ggml_tensor* h_final = h_prev;
    ggml_set_name(h_final, direction == 0 ? "rmvpe.gru.h_final.fwd" : "rmvpe.gru.h_final.bwd");
    ggml_set_output(out);
    ggml_set_output(h_final);
    ggml_build_forward_expand(gf, out);
    ggml_build_forward_expand(gf, h_final);
    return { out, h_final, h_prev_in };
}

ggml_tensor* UtaRmvpeGraph::BuildOutputHead(ggml_context* ctx, ggml_cgraph* gf, ggml_tensor* gru_out_full, int T) {
    (void)T; // kept in the signature for symmetry with the other two stages
    ggml_tensor* fc_w = GetWeight("fc.1.weight"); // ne=[512,360]
    ggml_tensor* fc_b = GetWeight("fc.1.bias");   // ne=[360]
    if (!fc_w || !fc_b) throw std::runtime_error("missing fc.1 weights");
    ggml_tensor* logits = ggml_add(ctx, ggml_mul_mat(ctx, fc_w, gru_out_full), fc_b); // ne=[360,T]
    ggml_tensor* output = ggml_sigmoid(ctx, logits);
    ggml_set_output(output);
    ggml_build_forward_expand(gf, output);
    return output;
}

ggml_tensor* UtaRmvpeGraph::Build(ggml_context* ctx, ggml_cgraph* gf, ggml_tensor* mel_input, int T,
                                  std::vector<std::pair<std::string, ggml_tensor*>>* debug_taps) {
    ggml_tensor* gru_input = BuildCnnHead(ctx, gf, mel_input, T, debug_taps);

    ggml_tensor* gru_out = BidirectionalGru(ctx, gru_input, T); // ne=[512,T]
    if (debug_taps) {
        ggml_set_output(gru_out);
        ggml_build_forward_expand(gf, gru_out);
        debug_taps->emplace_back("gru_out", gru_out);
    }

    ggml_tensor* output = BuildOutputHead(ctx, gf, gru_out, T);
    if (debug_taps) debug_taps->emplace_back("final_output", output);
    return output;
}

} // namespace rmvpe
