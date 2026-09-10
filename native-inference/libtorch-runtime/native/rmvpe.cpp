#include "runtime.hpp"
#include <stdexcept>

namespace uta::torch_native {
namespace {
class RmvpePlan final : public Plan {
public:
    RmvpePlan(std::shared_ptr<Runtime> selected, std::shared_ptr<Weights> loaded)
        : Plan(std::move(selected), std::move(loaded)) {
        const auto& input = weights->get("gru.weight_ih");
        const auto& recurrent = weights->get("gru.weight_hh");
        const auto& bias = weights->get("gru.bias");
        hidden = recurrent.size(-1);
        input_size = input.size(-1);
        // The GGUF recurrent tensors use ONNX z,r,n ordering, whereas ATen GRU
        // expects r,z,n. Repack once at model load, not once per frame/chunk.
        for (int64_t direction = 0; direction < 2; ++direction) {
            parameters.push_back(reorder(input.select(0, direction)));
            parameters.push_back(reorder(recurrent.select(0, direction)));
            parameters.push_back(reorder(bias.select(0, direction).narrow(0, 0, hidden * 3)));
            parameters.push_back(reorder(bias.select(0, direction).narrow(0, hidden * 3, hidden * 3)));
        }
    }
    TensorMap forward(const std::string& operation, const Inputs& inputs) override {
        if (operation != "forward") throw std::invalid_argument("RMVPE native operation must be forward");
        check_cancel();
        const auto& mel = inputs.get("mel");
        if (mel.dim() != 2 || mel.size(1) != 128 || mel.size(0) % 32)
            throw std::invalid_argument("RMVPE native mel layout is [padded_frames_multiple_of_32, 128]");
        auto value = batch_norm(*weights, mel.unsqueeze(0).unsqueeze(0), "unet.encoder.bn");
        std::vector<at::Tensor> skips;
        for (int64_t stage = 0; stage < 5; ++stage) {
            check_cancel();
            for (int64_t block = 0; block < 4; ++block)
                value = residual(value, "unet.encoder.layers." + std::to_string(stage) + ".conv." + std::to_string(block));
            skips.push_back(value);
            value = at::avg_pool2d(value, {2, 2}, {2, 2});
        }
        for (int64_t stage = 0; stage < 4; ++stage) {
            check_cancel();
            for (int64_t block = 0; block < 4; ++block)
                value = residual(value, "unet.intermediate.layers." + std::to_string(stage) + ".conv." + std::to_string(block));
        }
        for (int64_t stage = 0; stage < 5; ++stage) {
            check_cancel();
            const auto prefix = "unet.decoder.layers." + std::to_string(stage);
            auto up = at::conv_transpose2d(value, weights->get(prefix + ".conv1.conv1.0.weight"), {}, {2, 2}, {1, 1}, {1, 1});
            up = at::relu(batch_norm(*weights, up, prefix + ".conv1.conv1.1"));
            value = at::cat({up, skips[4 - stage]}, 1);
            for (int64_t block = 0; block < 4; ++block)
                value = residual(value, prefix + ".conv2." + std::to_string(block));
        }
        value = convolution(*weights, value, "cnn", {1, 1}, {1, 1});
        value = value.permute({0, 2, 1, 3}).contiguous().reshape({mel.size(0), 1, input_size});
        check_cancel();
        auto initial = at::zeros({2, 1, hidden}, value.options());
        // One native bidirectional sequence operation, not host-dispatched cells.
        // A ROCm/MIOpen failure propagates; never change to a CPU implementation.
        auto recurrent = std::get<0>(at::gru(value, initial, parameters, true, 1, 0.0, false, true, false));
        check_cancel();
        auto activation = at::sigmoid(weights->linear(recurrent.squeeze(1), "fc.1"));
        return {{"salience", activation}};
    }
private:
    int64_t hidden = 0, input_size = 0;
    std::vector<at::Tensor> parameters;
    at::Tensor reorder(const at::Tensor& tensor) const {
        return at::cat({tensor.narrow(0, hidden, hidden), tensor.narrow(0, 0, hidden), tensor.narrow(0, hidden * 2, hidden)}, 0).contiguous();
    }
    at::Tensor residual(const at::Tensor& input, const std::string& prefix) const {
        auto value = at::relu(convolution(*weights, input, prefix + ".conv.conv.0", {1, 1}, {1, 1}));
        value = at::relu(convolution(*weights, value, prefix + ".conv.conv.3", {1, 1}, {1, 1}));
        auto shortcut = weights->has(prefix + ".shortcut.weight")
            ? convolution(*weights, input, prefix + ".shortcut", {1, 1}, {0, 0}) : input;
        return value + shortcut;
    }
};
}
std::unique_ptr<Plan> make_rmvpe(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    return std::make_unique<RmvpePlan>(std::move(runtime), std::move(weights));
}
} // namespace uta::torch_native
