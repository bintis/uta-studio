#include "note_network.hpp"
#include <stdexcept>

namespace uta::torch_native {
namespace {
class RosvotPlan final : public Plan {
public:
    RosvotPlan(std::shared_ptr<Runtime> selected, std::shared_ptr<Weights> loaded)
        : Plan(std::move(selected), std::move(loaded)), net(*this) {}
    TensorMap forward(const std::string& operation, const Inputs& inputs) override {
        check_cancel();
        if (operation == "frames" || operation == "frame_outputs") {
            features = attention = weighted = at::Tensor();
            auto mel = inputs.get("mel");
            if (mel.dim() != 2 || mel.size(1) != 40 || mel.size(0) <= 0 || mel.size(0) % 16)
                throw std::invalid_argument("ROSVOT mel layout is [padded_frames_multiple_of_16, 40]");
            valid = inputs.integer("@valid_frames", mel.size(0));
            if (valid <= 0 || valid > mel.size(0)) throw std::invalid_argument("ROSVOT valid frames are outside its padded input");
            auto projected = net.conv(mel, "mel_proj", 1);
            auto encoded = net.blocks(projected, "mel_encoder", 2);
            auto embedded = encoded + at::embedding(weights->get("pitch_embed.weight"), inputs.get("pitch").to(at::kLong))
                + at::embedding(weights->get("uv_embed.weight"), inputs.get("uv").to(at::kLong))
                + at::embedding(weights->get("word_bd_embed.weight"), inputs.get("word_boundaries").to(at::kLong));
            auto conditioned = net.blocks(embedded, "cond_encoder");
            features = net.cmu(conditioned, "net.net", {}, 2, false);
            auto boundary = (net.linear(features, "note_bd_out") / 0.2).clamp(-16.0, 16.0).squeeze(-1);
            attention = at::sigmoid(net.linear(features, "pitch_decoder.multihead_dot_attn")).mean(-1);
            weighted = features * attention.unsqueeze(-1);
            if (operation == "frame_outputs") return {{"projected", projected}, {"encoded", encoded}, {"embedded", embedded},
                {"conditioned", conditioned}, {"features", features}, {"boundary_logits", boundary}, {"attention", attention}, {"weighted_features", weighted}};
            // Boundary regulation requires the host's real timed transcript.
            // Only this small vector crosses the native boundary; frame
            // features and their attention weights stay on the selected device.
            return {{"boundary_logits", boundary}};
        }
        if (operation == "pitch" || operation == "pitch_features") {
            at::Tensor aggregated;
            if (operation == "pitch_features") aggregated = inputs.get("note_features");
            else {
                if (!weighted.defined()) throw std::invalid_argument("ROSVOT pitch aggregation requires a successful frame stage");
                const int64_t count = inputs.integer("@note_count", 0);
                if (count <= 0) throw std::invalid_argument("ROSVOT pitch aggregation needs the host-regulated note count");
                auto boundaries = inputs.get("boundaries").to(at::kLong);
                if (boundaries.dim() != 1 || boundaries.size(0) < valid) throw std::invalid_argument("ROSVOT boundary timeline is too short");
                auto indices = boundaries.narrow(0, 0, valid).cumsum(0);
                auto numerator = at::zeros({count, 256}, weighted.options());
                auto denominator = at::zeros({count}, attention.options());
                numerator.index_add_(0, indices, weighted.narrow(0, 0, valid));
                denominator.index_add_(0, indices, attention.narrow(0, 0, valid));
                aggregated = numerator / (denominator + 1e-5).unsqueeze(-1);
            }
            check_cancel();
            auto note_features = net.blocks(aggregated, "pitch_decoder.post");
            return {{"logits", net.linear(note_features, "pitch_decoder.pitch_out") / 0.01}};
        }
        if (operation == "clear") {
            features = attention = weighted = at::Tensor();
            valid = 0;
            return {{"cleared", at::scalar_tensor(1, at::kLong)}};
        }
        throw std::invalid_argument("ROSVOT native operation must be frames, frame_outputs, pitch, pitch_features or clear");
    }
private:
    NoteNetwork net;
    at::Tensor features, attention, weighted;
    int64_t valid = 0;
};
}
std::unique_ptr<Plan> make_rosvot(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    return std::make_unique<RosvotPlan>(std::move(runtime), std::move(weights));
}
} // namespace uta::torch_native
