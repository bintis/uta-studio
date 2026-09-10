#include "runtime.hpp"
#include <array>
#include <cmath>
#include <limits>
#include <stdexcept>

namespace uta::torch_native {
namespace {
struct DecoderCache { at::Tensor key, value; };
class QwenPlan final : public Plan {
public:
    QwenPlan(std::shared_ptr<Runtime> selected, std::shared_ptr<Weights> loaded)
        : Plan(std::move(selected), std::move(loaded)) {
        auto& metadata = weights->container;
        aligner = metadata.text("general.architecture") == "qwen3-asr";
        if (!aligner && metadata.text("general.architecture") != "qwen3_asr") throw std::invalid_argument("Qwen native weight architecture is unknown");
        const auto read = [&](const char* asr_name, const char* alignment_name) {
            return metadata.meta(aligner ? alignment_name : asr_name).integer();
        };
        encoder_layers = read("stt.qwen3_asr.encoder.n_layers", "qwen3-asr.audio.encoder.layer_count");
        encoder_dimension = read("stt.qwen3_asr.encoder.d_model", "qwen3-asr.audio.encoder.embedding_length");
        encoder_heads = read("stt.qwen3_asr.encoder.n_heads", "qwen3-asr.audio.encoder.attention.head_count");
        mel_bins = read("stt.qwen3_asr.encoder.num_mel_bins", "qwen3-asr.audio.num_mel_bins");
        chunk_frames = aligner ? 100 : metadata.meta("stt.qwen3_asr.encoder.n_window").integer() * 2;
        decoder_layers = read("stt.qwen3_asr.decoder.n_layers", "qwen3-asr.block_count");
        heads = read("stt.qwen3_asr.decoder.n_heads", "qwen3-asr.attention.head_count");
        kv_heads = read("stt.qwen3_asr.decoder.n_kv_heads", "qwen3-asr.attention.head_count_kv");
        head_dimension = read("stt.qwen3_asr.decoder.head_dim", "qwen3-asr.attention.key_length");
        epsilon = metadata.meta(aligner ? "qwen3-asr.attention.layer_norm_rms_epsilon" : "stt.qwen3_asr.decoder.rms_norm_eps").number();
        theta = metadata.meta(aligner ? "qwen3-asr.rope.freq_base" : "stt.qwen3_asr.decoder.rope_theta").number();
        encoder_prefix = aligner ? "audio.encoder" : "enc";
        embedding_name = aligner ? "token_embd.weight" : "dec.token_embd.weight";
        output_norm = aligner ? "output_norm.weight" : "dec.output_norm.weight";
        head_name = aligner ? "output.weight" : embedding_name;
        if (encoder_heads <= 0 || encoder_dimension % encoder_heads || heads <= 0 || kv_heads <= 0 || heads % kv_heads || head_dimension <= 0 || chunk_frames <= 0)
            throw std::invalid_argument("Qwen native attention or chunk dimensions are invalid");
        cache.resize(decoder_layers);
    }
    TensorMap forward(const std::string& operation, const Inputs& inputs) override {
        check_cancel();
        if (operation == "encode" || operation == "encode_outputs") {
            invalidate_session();
            audio = at::Tensor();
            audio = encode(inputs.get("mel"));
            if (operation == "encode_outputs") return {{"audio", audio}};
            return {{"rows", at::scalar_tensor(audio.size(0), at::kLong)}};
        }
        if (operation == "session") {
            invalidate_session();
            capacity = inputs.integer("@capacity", 0);
            if (capacity <= 0) throw std::invalid_argument("Qwen session capacity must be explicitly positive");
            auto options = weights->get(embedding_name).options().dtype(runtime->precision == "mixed_attention" ? at::kHalf : at::kFloat);
            try {
                for (auto& layer : cache) {
                    layer.key = at::empty({1, kv_heads, capacity, head_dimension}, options);
                    layer.value = at::empty({1, kv_heads, capacity, head_dimension}, options);
                }
                past = 0;
            } catch (...) { invalidate_session(); throw; }
            return {{"position", at::scalar_tensor(past, at::kLong)}};
        }
        if (operation == "decode" || operation == "classify" || operation == "logits") {
            const bool incremental = operation == "decode";
            try { return decode(inputs, incremental, operation == "classify"); }
            catch (...) { if (incremental) invalidate_session(); throw; }
        }
        if (operation == "clear") {
            invalidate_session();
            audio = at::Tensor();
            return {{"cleared", at::scalar_tensor(1, at::kLong)}};
        }
        throw std::invalid_argument("Qwen native operation must be encode, encode_outputs, session, decode, classify, logits or clear");
    }
private:
    bool aligner = false;
    int64_t encoder_layers = 0, encoder_dimension = 0, encoder_heads = 0, mel_bins = 0, chunk_frames = 0;
    int64_t decoder_layers = 0, heads = 0, kv_heads = 0, head_dimension = 0, capacity = 0, past = -1;
    double epsilon = 0.0, theta = 0.0;
    std::string encoder_prefix, embedding_name, output_norm, head_name;
    at::Tensor audio, encoder_positions;
    int64_t cached_position_rows = 0, cached_chunk_rows = 0;
    std::vector<DecoderCache> cache;

    void invalidate_session() {
        for (auto& layer : cache) { layer.key = at::Tensor(); layer.value = at::Tensor(); }
        past = -1;
        capacity = 0;
    }
    at::Tensor attention(const at::Tensor& query, const at::Tensor& key, const at::Tensor& value, const at::Tensor& mask = {}, bool grouped = false) const {
        return runtime->precision == "mixed_attention" ? fused_attention(query, key, value, mask, false, grouped)
                                                       : dense_attention(query, key, value, mask);
    }
    at::Tensor position_encoding(int64_t rows, int64_t per_chunk) {
        if (cached_position_rows == rows && cached_chunk_rows == per_chunk) return encoder_positions;
        // The source computes sin/cos in double and then rounds to F32. Cache
        // this small deterministic host constant, not intermediate audio data.
        std::vector<float> values(rows * encoder_dimension);
        const auto half = encoder_dimension / 2;
        const auto scale = std::log(10000.0) / (half - 1);
        for (int64_t row = 0; row < rows; ++row)
            for (int64_t channel = 0; channel < half; ++channel) {
                const auto angle = static_cast<double>(row % per_chunk) * std::exp(-scale * channel);
                values[row * encoder_dimension + channel] = static_cast<float>(std::sin(angle));
                values[row * encoder_dimension + channel + half] = static_cast<float>(std::cos(angle));
            }
        encoder_positions = at::from_blob(values.data(), {rows, encoder_dimension}, at::kFloat).to(runtime->device, at::kFloat, false, true);
        cached_position_rows = rows;
        cached_chunk_rows = per_chunk;
        return encoder_positions;
    }
    at::Tensor encode(const at::Tensor& mel) {
        if (mel.dim() != 2 || mel.size(0) != mel_bins || mel.size(1) <= 0)
            throw std::invalid_argument("Qwen mel layout is [mel_bins, frames] with a nonempty timeline");
        const auto frames = mel.size(1);
        const auto chunk = std::min<int64_t>(frames, chunk_frames);
        const auto chunks = (frames + chunk - 1) / chunk;
        const auto final_frames = (frames - 1) % chunk + 1;
        const auto downsample = [](int64_t count) { return (count + 7) / 8; };
        const auto chunk_rows = downsample(chunk);
        const auto valid_rows = (chunks - 1) * chunk_rows + downsample(final_frames);
        auto padded = at::constant_pad_nd(mel, {0, chunks * chunk - frames}, 0);
        auto value = padded.reshape({mel_bins, chunks, chunk}).permute({1, 0, 2}).unsqueeze(1).contiguous();
        for (int64_t layer = 0; layer < 3; ++layer) {
            check_cancel();
            const auto prefix = aligner ? "audio.encoder.conv" + std::to_string(layer + 1) : "enc.conv." + std::to_string(layer);
            value = at::gelu(convolution(*weights, value, prefix, {2, 2}, {1, 1}), "none");
        }
        value = value.permute({0, 3, 1, 2}).contiguous().reshape({chunks * chunk_rows, -1}).narrow(0, 0, valid_rows);
        value = at::linear(value, weights->get(encoder_prefix + ".conv_out.weight")) + position_encoding(valid_rows, chunk_rows);
        const std::array<std::string, 8> names = aligner
            ? std::array<std::string, 8>{"attn_norm", "attn_q", "attn_k", "attn_v", "attn_out", "ffn_norm", "ffn_up", "ffn_down"}
            : std::array<std::string, 8>{"norm_attn", "attn.q", "attn.k", "attn.v", "attn.out", "norm_ffn", "ffn.fc1", "ffn.fc2"};
        const auto layout = [&](const at::Tensor& projected) {
            return projected.reshape({1, valid_rows, encoder_heads, encoder_dimension / encoder_heads}).transpose(1, 2);
        };
        for (int64_t layer = 0; layer < encoder_layers; ++layer) {
            check_cancel();
            const auto prefix = aligner ? "audio.encoder.blk." + std::to_string(layer) + '.' : "enc.blocks." + std::to_string(layer) + '.';
            auto normalized = weights->norm(value, prefix + names[0]);
            auto query = layout(weights->linear(normalized, prefix + names[1]));
            auto key = layout(weights->linear(normalized, prefix + names[2]));
            auto values = layout(weights->linear(normalized, prefix + names[3]));
            auto attended = attention(query, key, values).transpose(1, 2).reshape({valid_rows, encoder_dimension});
            value = value + weights->linear(attended, prefix + names[4]);
            normalized = weights->norm(value, prefix + names[5]);
            value = value + weights->linear(at::gelu(weights->linear(normalized, prefix + names[6]), "none"), prefix + names[7]);
        }
        value = weights->norm(value, encoder_prefix + ".ln_post");
        return weights->linear(at::gelu(weights->linear(value, encoder_prefix + ".proj1"), "none"), encoder_prefix + ".proj2");
    }
    at::Tensor decoder_norm(const at::Tensor& value, const std::string& name) const {
        return weights->rms_norm(value, name, epsilon);
    }
    TensorMap decode(const Inputs& inputs, bool incremental, bool classification) {
        auto tokens = inputs.get("tokens").to(at::kLong);
        if (tokens.dim() != 1 || !tokens.numel()) throw std::invalid_argument("Qwen decoding requires a nonempty one-dimensional token array");
        const auto rows = tokens.numel();
        const int64_t start = incremental ? past : 0;
        if (incremental && (start < 0 || rows > capacity - start)) throw std::invalid_argument("Qwen decoder session is unavailable or its explicit capacity is exhausted");
        if (incremental && inputs.integer("@expected_position", start) != start)
            throw std::invalid_argument("Qwen decoder request position disagrees with resident KV state");
        if (classification && !aligner) throw std::invalid_argument("timestamp classification requires the aligner model, not ASR");
        auto value = at::embedding(weights->get(embedding_name), tokens);
        auto audio_indices = inputs.optional("audio_indices");
        if (audio_indices.defined()) {
            if (start != 0 || !audio.defined()) throw std::invalid_argument("Qwen audio injection requires resident audio and initial prefill");
            if (audio_indices.dim() != 1 || audio_indices.numel() != rows) throw std::invalid_argument("Qwen audio index map must cover every prompt token");
            auto indices = audio_indices.to(at::kLong);
            auto injected = audio.index_select(0, indices.clamp_min(0));
            value = at::where((indices >= 0).unsqueeze(-1), injected, value);
        }
        auto positions = at::arange(start, start + rows, tokens.options());
        auto keys = at::arange(start + rows, tokens.options());
        auto mask = positions.unsqueeze(1) >= keys.unsqueeze(0);
        const std::array<std::string, 11> names = aligner
            ? std::array<std::string, 11>{"attn_norm", "ffn_norm", "attn_q_norm", "attn_k_norm", "attn_q", "attn_k", "attn_v", "attn_output", "ffn_gate", "ffn_up", "ffn_down"}
            : std::array<std::string, 11>{"norm_attn", "norm_ffn", "attn.q_norm", "attn.k_norm", "attn.q", "attn.k", "attn.v", "attn.o", "ffn.gate", "ffn.up", "ffn.down"};
        const auto layout = [&](const at::Tensor& projected, int64_t count) {
            return projected.reshape({1, rows, count, head_dimension}).transpose(1, 2);
        };
        for (int64_t layer = 0; layer < decoder_layers; ++layer) {
            check_cancel();
            const auto prefix = (aligner ? "blk." : "dec.blocks.") + std::to_string(layer) + '.';
            const auto linear = [&](const at::Tensor& input, size_t item) { return at::linear(input, weights->get(prefix + names[item] + ".weight")); };
            auto normalized = decoder_norm(value, prefix + names[0] + ".weight");
            auto query = rotary_split(decoder_norm(layout(linear(normalized, 4), heads), prefix + names[2] + ".weight"), positions, theta);
            auto key = rotary_split(decoder_norm(layout(linear(normalized, 5), kv_heads), prefix + names[3] + ".weight"), positions, theta);
            auto projected_value = layout(linear(normalized, 6), kv_heads);
            if (incremental) {
                cache[layer].key.narrow(2, start, rows).copy_(key);
                cache[layer].value.narrow(2, start, rows).copy_(projected_value);
                key = cache[layer].key.narrow(2, 0, start + rows);
                projected_value = cache[layer].value.narrow(2, 0, start + rows);
            }
            auto attended = attention(query, key, projected_value, mask, heads != kv_heads).transpose(1, 2).reshape({rows, heads * head_dimension});
            value = value + linear(attended, 7);
            normalized = decoder_norm(value, prefix + names[1] + ".weight");
            value = value + linear(at::silu(linear(normalized, 8)) * linear(normalized, 9), 10);
        }
        value = decoder_norm(value, output_norm);
        auto selected = inputs.optional("selected_rows");
        if (selected.defined()) value = value.index_select(0, selected.to(at::kLong));
        else value = value.narrow(0, rows - 1, 1);
        auto logits = at::linear(value, weights->get(head_name));
        if (incremental) past += rows;
        return {{"logits", logits}, {"position", at::scalar_tensor(incremental ? past : rows, at::kLong)}};
    }
};
}
std::unique_ptr<Plan> make_qwen(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    return std::make_unique<QwenPlan>(std::move(runtime), std::move(weights));
}
} // namespace uta::torch_native
