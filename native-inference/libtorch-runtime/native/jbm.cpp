#include "runtime.hpp"
#include <algorithm>
#include <stdexcept>

namespace uta::torch_native {
namespace {
class JbmPlan final : public Plan {
public:
    using Plan::Plan;
    TensorMap forward(const std::string& operation, const Inputs& inputs) override {
        if (operation != "forward") throw std::invalid_argument("JBM555 native operation must be forward");
        auto features = inputs.get("features");
        if (features.dim() == 3) features = features.unsqueeze(0);
        if (features.dim() != 4 || features.size(0) != 1 || features.size(1) != 6 || features.size(3) != 384)
            throw std::invalid_argument("JBM555 features must contain mix and vocal channels in [6, frames, 384] layout");
        const int64_t frames = features.size(2);
        std::vector<at::Tensor> onset_parts, pitch_parts;
        // Keep the learned network's exact contextual neighborhood while
        // bounding intermediate CNN memory independently of track duration.
        for (int64_t owned_begin = 0; owned_begin < frames; owned_begin += 1024) {
            check_cancel();
            const int64_t owned_end = std::min<int64_t>(frames, owned_begin + 1024);
            const int64_t begin = std::max<int64_t>(0, owned_begin - 64);
            const int64_t end = std::min<int64_t>(frames, owned_end + 64);
            auto chunk = features.narrow(2, begin, end - begin);
            onset_parts.push_back(branch(chunk, "onset_cnn").narrow(0, owned_begin - begin, owned_end - owned_begin));
            pitch_parts.push_back(branch(chunk, "pitch_cnn").narrow(0, owned_begin - begin, owned_end - owned_begin));
        }
        if (onset_parts.empty()) throw std::invalid_argument("JBM555 requires nonempty frame input");
        auto on_off = at::softmax(at::cat(onset_parts, 0), -1);
        auto pitch = at::cat(pitch_parts, 0);
        return {{"on_off", on_off}, {"octave", pitch.narrow(-1, 0, 5)}, {"pitch_class", pitch.narrow(-1, 5, 13)}};
    }
private:
    at::Tensor branch(const at::Tensor& input, const std::string& prefix) {
        auto value = input;
        for (int64_t layer = 1; layer <= 5; ++layer) {
            check_cancel();
            value = convolution(*weights, value, prefix + ".conv" + std::to_string(layer), {1, layer == 1 ? 4 : 1}, {4, 4});
            if (layer < 5) value = at::relu(value);
        }
        // The source's dense row is [frequency,channel], not [channel,frequency].
        value = value.permute({0, 2, 3, 1}).contiguous().reshape({input.size(2), -1});
        for (int64_t layer = 1; layer <= 3; ++layer) {
            value = weights->linear(value, prefix + ".fc" + std::to_string(layer));
            if (layer < 3) value = at::relu(value);
        }
        return value;
    }
};
}
std::unique_ptr<Plan> make_jbm(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    return std::make_unique<JbmPlan>(std::move(runtime), std::move(weights));
}
} // namespace uta::torch_native
