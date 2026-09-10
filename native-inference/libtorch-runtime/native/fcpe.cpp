#include "runtime.hpp"
#include <stdexcept>

namespace uta::torch_native {
namespace {
class FcpePlan final : public Plan {
public:
    using Plan::Plan;
    TensorMap forward(const std::string& operation, const Inputs& inputs) override {
        if (operation == "constants") return {{"cents_mapping", weights->get("cents_mapping")}};
        if (operation != "forward") throw std::invalid_argument("FCPE native operation must be forward or constants");
        const auto& mel = inputs.get("mel");
        if (mel.dim() != 2 || mel.size(-1) != 128) throw std::invalid_argument("FCPE mel layout is [frames, 128]");
        check_cancel();
        auto value = convolution(*weights, mel.transpose(0, 1).unsqueeze(0), "input_stack.0", {1}, {1});
        // The catalog's 59-tensor checkpoint uses a four-group timeline norm
        // and six gated depthwise-convolution blocks. It has no learned Q/K/V
        // attention weights; adding attention would change the model.
        value = at::group_norm(value, 4, weights->get("mel_scale"), weights->get("mel_bias"), 1e-5, true);
        value = at::leaky_relu(value, 0.01);
        value = convolution(*weights, value, "input_stack.1", {1}, {1}).transpose(1, 2);
        for (int64_t layer = 0; weights->has("encoder_layers." + std::to_string(layer) + ".norm.weight"); ++layer) {
            check_cancel();
            const auto prefix = "encoder_layers." + std::to_string(layer);
            auto normalized = weights->norm(value, prefix + ".norm");
            auto expanded = pointwise(normalized, prefix + ".fc1");
            auto halves = expanded.chunk(2, -1);
            auto gated = halves[0] * at::sigmoid(halves[1]);
            const auto& kernel = weights->get(prefix + ".conv.weight");
            auto convolved = convolution(*weights, gated.transpose(1, 2), prefix + ".conv", {1}, {kernel.size(-1) / 2}, gated.size(-1));
            value = value + pointwise(at::silu(convolved).transpose(1, 2), prefix + ".fc2");
        }
        value = weights->norm(value, "norm");
        // This exported output matrix is [model_channel, pitch_class], unlike
        // the row-major [output,input] convention used by the other linears.
        value = at::matmul(value, weights->get("output_proj.weight")) + weights->get("output_proj.bias");
        return {{"salience", at::sigmoid(value).squeeze(0)}};
    }
private:
    at::Tensor pointwise(const at::Tensor& input, const std::string& prefix) const {
        const auto& kernel = weights->get(prefix + ".weight");
        return at::linear(input, kernel.reshape({kernel.size(0), -1}), weights->get(prefix + ".bias"));
    }
};
}
std::unique_ptr<Plan> make_fcpe(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    return std::make_unique<FcpePlan>(std::move(runtime), std::move(weights));
}
} // namespace uta::torch_native
