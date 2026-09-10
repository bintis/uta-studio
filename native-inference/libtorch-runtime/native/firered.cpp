#include "runtime.hpp"
#include <cmath>
#include <stdexcept>

namespace uta::torch_native {
namespace {
struct FireRedCache { at::Tensor key, value, cross_key, cross_value; };
class FireRedPlan final : public Plan {
public:
    FireRedPlan(std::shared_ptr<Runtime> selected, std::shared_ptr<Weights> loaded)
        : Plan(std::move(selected), std::move(loaded)) {
        dimension = weights->get("decoder.tgt_word_emb.weight").size(1);
        const auto& bias = weights->get("encoder.layer_stack.0.mhsa.pos_bias_u");
        heads = bias.size(0);
        head_dimension = dimension / heads;
        while (weights->has("encoder.layer_stack." + std::to_string(encoder_layers) + ".layer_norm.weight")) ++encoder_layers;
        while (weights->has("decoder.layer_stack." + std::to_string(decoder_layers) + ".self_attn_norm.weight")) ++decoder_layers;
        cache.resize(decoder_layers);
        if (!encoder_layers || !decoder_layers || dimension % heads) throw std::invalid_argument("invalid FireRed native architecture dimensions");
    }
    TensorMap forward(const std::string& operation, const Inputs& inputs) override {
        check_cancel();
        if (operation == "encode" || operation == "encode_outputs") {
            clear();
            audio = encode(inputs.get("features"));
            prepare_cross_attention();
            if (operation == "encode_outputs") return {{"audio", audio}};
            return {{"rows", at::scalar_tensor(audio.size(0), at::kLong)}};
        }
        if (operation == "session") {
            if (!audio.defined()) throw std::invalid_argument("FireRed session requires resident encoder output");
            past = -1;
            capacity = inputs.integer("@capacity", 0);
            if (capacity <= 0) throw std::invalid_argument("FireRed session capacity must be explicitly positive");
            for (auto& layer : cache) {
                layer.key = at::empty({1, heads, capacity, head_dimension}, audio.options());
                layer.value = at::empty({1, heads, capacity, head_dimension}, audio.options());
            }
            past = 0;
            return {{"position", at::scalar_tensor(past, at::kLong)}};
        }
        if (operation == "decode" || operation == "logits" || operation == "legacy_prefix_logits") {
            const bool incremental = operation == "decode";
            try { return decode(inputs, incremental, operation != "legacy_prefix_logits"); }
            catch (...) { if (incremental) past = -1; throw; }
        }
        if (operation == "clear") { clear(); return {{"cleared", at::scalar_tensor(1, at::kLong)}}; }
        throw std::invalid_argument("unknown FireRed native operation");
    }
private:
    int64_t dimension = 0, heads = 0, head_dimension = 0, encoder_layers = 0, decoder_layers = 0;
    int64_t capacity = 0, past = -1;
    at::Tensor audio;
    std::vector<FireRedCache> cache;
    void clear() {
        audio = at::Tensor();
        for (auto& layer : cache) layer = {};
        past = -1;
        capacity = 0;
    }
    at::Tensor linear(const at::Tensor& input, const std::string& prefix, bool bias = true) const {
        const auto& raw = weights->get(prefix + ".weight");
        auto kernel = raw.dim() == 3 && raw.size(-1) == 1 ? raw.squeeze(-1) : raw;
        return at::linear(input, kernel, bias ? weights->optional(prefix + ".bias") : at::Tensor());
    }
    at::Tensor layout(const at::Tensor& value) const {
        return value.reshape({1, value.size(0), heads, head_dimension}).transpose(1, 2);
    }
    at::Tensor attention_output(const at::Tensor& query, const at::Tensor& key, const at::Tensor& value,
                                const std::string& prefix, const at::Tensor& mask = {}) const {
        // FireRed deliberately remains FP32 even when another family in the
        // same worker selected mixed_attention. Its declared plan is strict.
        auto result = dense_attention(query, key, value, mask).transpose(1, 2).reshape({query.size(2), dimension});
        return linear(result, prefix + ".fc");
    }
    at::Tensor feed_forward(const at::Tensor& input, const std::string& prefix) const {
        auto normalized = weights->norm(input, prefix + ".net.0");
        auto branch = linear(at::silu(linear(normalized, prefix + ".net.1")), prefix + ".net.4");
        return input + branch * 0.5;
    }
    at::Tensor relative_attention(const at::Tensor& input, const at::Tensor& position,
                                  const at::Tensor& relative_indices, const std::string& prefix) const {
        auto query = layout(linear(weights->norm(input, prefix + ".layer_norm_q"), prefix + ".w_qs"));
        auto key = layout(linear(weights->norm(input, prefix + ".layer_norm_k"), prefix + ".w_ks"));
        auto value = layout(linear(weights->norm(input, prefix + ".layer_norm_v"), prefix + ".w_vs"));
        auto projected_position = layout(linear(position, prefix + ".linear_pos"));
        auto query_content = query + weights->get(prefix + ".pos_bias_u").reshape({1, heads, 1, head_dimension});
        auto query_position = query + weights->get(prefix + ".pos_bias_v").reshape({1, heads, 1, head_dimension});
        auto content_scores = at::matmul(query_content, key.transpose(-1, -2));
        auto position_scores = at::matmul(query_position, projected_position.transpose(-1, -2));
        auto shifted = position_scores.gather(-1, relative_indices.expand({1, heads, input.size(0), input.size(0)}));
        auto probabilities = at::softmax((content_scores + shifted) / std::sqrt(static_cast<double>(head_dimension)), -1);
        auto attended = at::matmul(probabilities, value).transpose(1, 2).reshape({input.size(0), dimension});
        return input + linear(attended, prefix + ".fc");
    }
    at::Tensor conformer_convolution(const at::Tensor& input, const std::string& prefix) const {
        auto parts = linear(weights->norm(input, prefix + ".pre_layer_norm"), prefix + ".pointwise_conv1").chunk(2, -1);
        auto gated = parts[0] * at::sigmoid(parts[1]);
        const auto& kernel = weights->get(prefix + ".depthwise_conv.weight");
        // The exported depthwise stage has no learned bias. Do not invent one
        // or confuse its following LayerNorm (named batch_norm) with BatchNorm.
        auto convolved = at::conv1d(gated.transpose(0, 1).unsqueeze(0), kernel, {}, {1}, {kernel.size(-1) / 2}, {1}, gated.size(-1));
        convolved = convolved.squeeze(0).transpose(0, 1);
        return input + linear(at::silu(weights->norm(convolved, prefix + ".batch_norm")), prefix + ".pointwise_conv2");
    }
    at::Tensor encode(const at::Tensor& features) {
        if (features.dim() != 2 || features.size(0) != 230 || features.size(1) != 80)
            throw std::invalid_argument("FireRed frontend contract is [230, 80] CMVN feature frames per current audio window");
        auto value = at::constant_pad_nd(features, {0, 0, 0, 6}, 0).unsqueeze(0).unsqueeze(0);
        value = at::relu(convolution(*weights, value, "encoder.input_preprocessor.conv.0", {2, 2}, {0, 0}));
        value = at::relu(convolution(*weights, value, "encoder.input_preprocessor.conv.2", {2, 2}, {0, 0}));
        const auto frames = value.size(2);
        value = value.permute({0, 2, 1, 3}).contiguous().reshape({frames, -1});
        value = linear(value, "encoder.input_preprocessor.out");
        const auto& position_weights = weights->get("encoder.positional_encoding.pe");
        auto positions = position_weights.reshape({-1, dimension});
        auto position = positions.narrow(0, positions.size(0) / 2 - frames + 1, 2 * frames - 1);
        auto query = at::arange(frames, value.options().dtype(at::kLong)).unsqueeze(1);
        auto key = at::arange(frames, value.options().dtype(at::kLong)).unsqueeze(0);
        auto relative_indices = (key + frames - 1 - query).reshape({1, 1, frames, frames});
        for (int64_t layer = 0; layer < encoder_layers; ++layer) {
            check_cancel();
            const auto prefix = "encoder.layer_stack." + std::to_string(layer);
            value = feed_forward(value, prefix + ".ffn1");
            value = relative_attention(value, position, relative_indices, prefix + ".mhsa");
            value = conformer_convolution(value, prefix + ".conv");
            value = feed_forward(value, prefix + ".ffn2");
            value = weights->norm(value, prefix + ".layer_norm");
        }
        return value;
    }
    void prepare_cross_attention() {
        for (int64_t layer = 0; layer < decoder_layers; ++layer) {
            check_cancel();
            const auto prefix = "decoder.layer_stack." + std::to_string(layer) + ".cross_attn";
            cache[layer].cross_key = layout(linear(audio, prefix + ".w_ks", false)).contiguous();
            cache[layer].cross_value = layout(linear(audio, prefix + ".w_vs")).contiguous();
        }
    }
    TensorMap decode(const Inputs& inputs, bool incremental, bool causal) {
        if (!audio.defined()) throw std::invalid_argument("FireRed decoder requires a successfully encoded audio window");
        auto tokens = inputs.get("tokens").to(at::kLong);
        if (tokens.dim() != 1 || !tokens.numel()) throw std::invalid_argument("FireRed decoding requires a nonempty token vector");
        const auto rows = tokens.numel();
        const int64_t start = incremental ? past : 0;
        if (incremental && (start < 0 || rows > capacity - start)) throw std::invalid_argument("FireRed incremental session is unavailable or full");
        if (incremental && inputs.integer("@expected_position", start) != start)
            throw std::invalid_argument("FireRed requested position disagrees with resident KV state");
        auto value = at::embedding(weights->get("decoder.tgt_word_emb.weight"), tokens) * std::sqrt(static_cast<double>(dimension));
        auto positional = weights->get("decoder.positional_encoding.pe").reshape({-1, dimension});
        value = value + positional.narrow(0, start, rows);
        at::Tensor mask;
        if (causal) mask = at::arange(start, start + rows, tokens.options()).unsqueeze(1) >= at::arange(start + rows, tokens.options()).unsqueeze(0);
        for (int64_t layer = 0; layer < decoder_layers; ++layer) {
            check_cancel();
            const auto prefix = "decoder.layer_stack." + std::to_string(layer);
            auto normalized = weights->norm(value, prefix + ".self_attn_norm");
            auto query = layout(linear(normalized, prefix + ".self_attn.w_qs"));
            auto key = layout(linear(normalized, prefix + ".self_attn.w_ks", false));
            auto projected_value = layout(linear(normalized, prefix + ".self_attn.w_vs"));
            if (incremental) {
                cache[layer].key.narrow(2, start, rows).copy_(key);
                cache[layer].value.narrow(2, start, rows).copy_(projected_value);
                key = cache[layer].key.narrow(2, 0, start + rows);
                projected_value = cache[layer].value.narrow(2, 0, start + rows);
            }
            value = value + attention_output(query, key, projected_value, prefix + ".self_attn", mask);
            query = layout(linear(weights->norm(value, prefix + ".cross_attn_norm"), prefix + ".cross_attn.w_qs"));
            value = value + attention_output(query, cache[layer].cross_key, cache[layer].cross_value, prefix + ".cross_attn");
            normalized = weights->norm(value, prefix + ".mlp_norm");
            value = value + linear(at::gelu(linear(normalized, prefix + ".mlp.w_1"), "none"), prefix + ".mlp.w_2");
        }
        value = weights->norm(value, "decoder.layer_norm_out");
        auto selected = inputs.optional("selected_rows");
        value = selected.defined() ? value.index_select(0, selected.to(at::kLong)) : value.narrow(0, rows - 1, 1);
        auto logits = linear(value, "decoder.tgt_word_prj", false);
        if (incremental) past += rows;
        return {{"logits", logits}, {"position", at::scalar_tensor(incremental ? past : rows, at::kLong)}};
    }
};
}
std::unique_ptr<Plan> make_firered(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    return std::make_unique<FireRedPlan>(std::move(runtime), std::move(weights));
}
} // namespace uta::torch_native
