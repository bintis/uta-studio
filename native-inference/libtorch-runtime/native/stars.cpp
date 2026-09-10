#include "note_network.hpp"
#include <array>
#include <stdexcept>

namespace uta::torch_native {
namespace {
class StarsPlan final : public Plan {
public:
    StarsPlan(std::shared_ptr<Runtime> selected, std::shared_ptr<Weights> loaded)
        : Plan(std::move(selected), std::move(loaded)), net(*this) {}
    TensorMap forward(const std::string& operation, const Inputs& inputs) override {
        check_cancel();
        if (operation == "utterance" || operation == "utterance_outputs") {
            clear();
            const auto& mel = inputs.get("mel");
            if (mel.dim() != 2 || mel.size(1) != 80 || mel.size(0) <= 0 || mel.size(0) % 16)
                throw std::invalid_argument("STARS mel layout is [padded_frames_multiple_of_16, 80]");
            frames = mel.size(0);
            valid = inputs.integer("@valid_frames", frames);
            if (valid <= 0 || valid > frames) throw std::invalid_argument("STARS valid-frame interval is outside the padded input");
            mask = (at::arange(frames, mel.options().dtype(at::kLong)) < valid).unsqueeze(-1).to(at::kFloat);
            auto projected = net.conv(mel, "mel_proj", 1);
            auto encoded = net.blocks(projected, "mel_encoder", 2);
            auto pitch = at::embedding(weights->get("pitch_embed.weight"), inputs.get("pitch").to(at::kLong));
            auto uv = at::embedding(weights->get("uv_embed.weight"), inputs.get("uv").to(at::kLong));
            embedded = encoded * mask + (pitch + uv) * mask;
            auto local = net.cmu(embedded, "prosody_extractor_utter.cmuencoder.net", mask, 2, true);
            local = net.blocks(local, "prosody_extractor_utter.encoder");
            utterance = net.linear(at::cat({local, net.absolute_positions(valid, frames)}, -1), "l1_utter");
            auto logits = net.linear(utterance, "ph_frame_predictor.ph_head");
            TensorMap output{{"boundary_probabilities", at::sigmoid(logits.select(-1, 0).clamp(-16.0, 16.0))},
                             {"phoneme_logits", logits.narrow(-1, 1, 61)}};
            if (operation == "utterance_outputs") {
                output.emplace("projected", projected); output.emplace("encoded", encoded);
                output.emplace("embedded", embedded); output.emplace("features", utterance);
            }
            return output;
        }
        if (operation == "rhythm" || operation == "rhythm_outputs") {
            require(utterance, "utterance");
            rhythm = pitch_features = sentence = attention = weighted = at::Tensor();
            auto phoneme = grouped_prosody(inputs.get("phoneme_ids"), inputs.integer("@phoneme_count", 0), "prosody_extractor_ph", "l1_ph");
            auto word = grouped_prosody(inputs.get("word_ids"), inputs.integer("@word_count", 0), "prosody_extractor_word", "l1_word");
            rhythm = utterance + phoneme + word;
            auto logits = net.linear(rhythm, "note_frame_predictor.note_head");
            TensorMap output{{"boundary_logits", (logits.select(-1, 0) / 0.2).clamp(-16.0, 16.0)}};
            if (operation == "rhythm_outputs") output.emplace("features", rhythm);
            return output;
        }
        if (operation == "pitch" || operation == "pitch_outputs") {
            require(rhythm, "rhythm");
            pitch_features = sentence = attention = weighted = at::Tensor();
            const auto count = inputs.integer("@note_count", 0);
            auto note = grouped_prosody(inputs.get("note_ids"), count, "prosody_extractor_note", "l1_note");
            pitch_features = rhythm + note;
            auto probabilities = at::sigmoid(net.linear(pitch_features, "pitch_decoder.multihead_dot_attn")).mean(-1);
            // Pitch aggregation includes note zero and padded frames, unlike
            // the preceding 1-indexed VQ grouping. Keep these maps distinct.
            auto boundaries = inputs.get("boundaries").to(at::kLong);
            if (boundaries.dim() != 1 || boundaries.size(0) != frames) throw std::invalid_argument("STARS pitch boundary map must cover padded frames");
            auto aggregated = aggregate(pitch_features * probabilities.unsqueeze(-1), probabilities, boundaries.cumsum(0), count);
            auto post = masked_blocks(aggregated, "pitch_decoder.post");
            TensorMap output{{"logits", net.linear(post, "pitch_decoder.pitch_out") / 0.01}};
            if (operation == "pitch_outputs") output.emplace("features", pitch_features);
            return output;
        }
        if (operation == "sentence" || operation == "sentence_outputs") {
            require(pitch_features, "pitch");
            sentence = attention = weighted = at::Tensor();
            auto prosody = net.cmu(embedded, "prosody_extractor_sentence.cmuencoder.net", mask, 1, true);
            prosody = net.blocks(prosody, "prosody_extractor_sentence.encoder");
            sentence = pitch_features + prosody.mean(0, true); // deliberately includes padded frames
            auto tokens = align_sentence(sentence);
            const std::array<std::string, 7> names{"tech", "lan", "gen", "emo", "meth", "pace", "range"};
            const std::array<std::string, 7> output_names{"technique_group", "language", "gender", "emotion", "method", "pace", "range"};
            TensorMap output;
            for (std::size_t token = 0; token < names.size(); ++token) {
                auto normalized = weights->norm(tokens.select(0, token), "style_predict." + names[token] + "_norm");
                output.emplace(output_names[token], net.linear(normalized, "style_predict." + names[token] + "_head"));
            }
            attention = at::sigmoid(net.linear(sentence, "tech_predictor.multihead_tech_attn")).mean(-1);
            weighted = sentence * attention.unsqueeze(-1);
            if (operation == "sentence_outputs") {
                output.emplace("features", sentence); output.emplace("attention", attention); output.emplace("weighted_features", weighted);
            }
            return output;
        }
        if (operation == "techniques" || operation == "technique_features") {
            at::Tensor aggregated;
            if (operation == "technique_features") aggregated = inputs.get("aggregated");
            else {
                require(weighted, "sentence");
                const auto& intervals = inputs.get("@intervals");
                if (!intervals.device().is_cpu() || intervals.scalar_type() != at::kLong || intervals.dim() != 2 || intervals.size(1) != 2 || !intervals.size(0))
                    throw std::invalid_argument("STARS @intervals must be host int64 [phonemes, 2] decoded ranges");
                const auto contiguous = intervals.contiguous();
                const auto* ranges = contiguous.const_data_ptr<int64_t>();
                std::vector<at::Tensor> values;
                for (int64_t row = 0; row < intervals.size(0); ++row) {
                    const int64_t begin = ranges[row * 2], end = ranges[row * 2 + 1];
                    if (begin < 0 || end <= begin || end > frames) throw std::invalid_argument("STARS technique interval exceeds frame features");
                    values.push_back(weighted.narrow(0, begin, end - begin).sum(0) / (attention.narrow(0, begin, end - begin).sum() + 1e-5));
                }
                aggregated = at::stack(values, 0);
            }
            auto post = masked_blocks(aggregated, "tech_predictor.tech_post", true);
            return {{"logits", net.linear(post, "tech_predictor.binary_tech_out")}};
        }
        if (operation == "clear") { clear(); return {{"cleared", at::scalar_tensor(1, at::kLong)}}; }
        throw std::invalid_argument("unknown STARS native stage; valid stages are utterance, rhythm, pitch, sentence and techniques");
    }
private:
    NoteNetwork net;
    int64_t frames = 0, valid = 0;
    at::Tensor embedded, mask, utterance, rhythm, pitch_features, sentence, attention, weighted;
    void clear() {
        embedded = mask = utterance = rhythm = pitch_features = sentence = attention = weighted = at::Tensor();
        frames = valid = 0;
    }
    void require(const at::Tensor& value, const std::string& stage) const {
        if (!value.defined()) throw std::invalid_argument("STARS stage requires successful resident " + stage + " features");
    }
    at::Tensor masked_blocks(const at::Tensor& value, const std::string& prefix, bool silu = false) const {
        auto nonzero = value.abs().gt(0).any(-1, true).to(at::kFloat);
        auto output = net.residual(value, prefix + ".res_blocks.0.blocks.0", silu) * nonzero;
        output = weights->norm(output, prefix + ".last_norm") * nonzero;
        return net.conv(output, prefix + ".post_net1", 1) * nonzero;
    }
    at::Tensor aggregate(const at::Tensor& values, const at::Tensor& probabilities, const at::Tensor& ids, int64_t count) const {
        if (count <= 0) throw std::invalid_argument("STARS aggregation count must come from decoded nonempty intervals");
        auto sums = at::zeros({count, 256}, values.options());
        auto denominator = at::zeros({count}, probabilities.options());
        sums.index_add_(0, ids, values);
        denominator.index_add_(0, ids, probabilities);
        return sums / (denominator + 1e-5).unsqueeze(-1);
    }
    at::Tensor grouped_prosody(const at::Tensor& supplied_ids, int64_t count, const std::string& adaptor, const std::string& projection) {
        if (count <= 0 || supplied_ids.dim() != 1 || supplied_ids.size(0) != frames) throw std::invalid_argument("STARS prosody needs a complete decoded segment map and positive count");
        auto ids = supplied_ids.to(at::kLong);
        auto local = net.cmu(embedded, adaptor + ".cmuencoder.net", mask, 2, true);
        auto sums = at::zeros({count + 1, 256}, local.options());
        auto counts = at::zeros({count + 1}, local.options());
        sums.index_add_(0, ids, local);
        counts.index_add_(0, ids, at::ones({frames}, local.options()));
        auto grouped = sums.narrow(0, 1, count) / counts.narrow(0, 1, count).clamp_min(1).unsqueeze(-1);
        auto encoded = masked_blocks(grouped, adaptor + ".encoder");
        auto codebook = weights->get(adaptor + ".vqvae.embedding").reshape({48, 256});
        // Direct squared differences preserve the source's distance formula;
        // do not replace it with a cancellation-prone norm/dot identity.
        auto distances = (encoded.unsqueeze(1) - codebook.unsqueeze(0)).square().sum(-1);
        auto quantized = codebook.index_select(0, distances.argmin(-1));
        auto projected = net.linear(at::cat({quantized, net.absolute_positions(count, count)}, -1), projection);
        auto padded = at::cat({at::zeros({1, 256}, projected.options()), projected}, 0);
        return padded.index_select(0, ids);
    }
    at::Tensor align_sentence(const at::Tensor& features) {
        auto tokens = weights->get("cls_tokens").reshape({16, 256});
        auto keep = at::arange(frames, features.options().dtype(at::kLong)) < valid;
        auto attention_mask = at::where(keep, 0.0, -1e8).to(at::kFloat).unsqueeze(0);
        const auto layout = [](const at::Tensor& input) { return input.reshape({1, input.size(0), 2, 128}).transpose(1, 2); };
        for (int64_t layer = 0; layer < 2; ++layer) {
            check_cancel();
            const auto prefix = "align_sentence.layers." + std::to_string(layer);
            const auto attention_prefix = prefix + ".multihead_attn";
            const auto& weight = weights->get(attention_prefix + ".in_proj_weight");
            const auto& bias = weights->get(attention_prefix + ".in_proj_bias");
            auto query = at::linear(tokens, weight.narrow(0, 0, 256), bias.narrow(0, 0, 256));
            auto key_value = at::linear(features, weight.narrow(0, 256, 512), bias.narrow(0, 256, 512)).chunk(2, -1);
            // This small 16-query stage remains exact FP32, preserving finite
            // -1e8 padding independently from a model-wide SDPA setting.
            auto attended = dense_attention(layout(query), layout(key_value[0]), layout(key_value[1]), attention_mask)
                .transpose(1, 2).reshape({16, 256});
            tokens = weights->norm(tokens + net.linear(attended, attention_prefix + ".out_proj"), prefix + ".norm1");
            tokens = weights->norm(tokens + net.linear(at::relu(net.linear(tokens, prefix + ".linear1")), prefix + ".linear2"), prefix + ".norm2");
        }
        return tokens;
    }
};
}
std::unique_ptr<Plan> make_stars(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    return std::make_unique<StarsPlan>(std::move(runtime), std::move(weights));
}
} // namespace uta::torch_native
