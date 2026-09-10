#include "note_network.hpp"
#include <cmath>
#include <stdexcept>

namespace uta::torch_native {
at::Tensor NoteNetwork::conv(const at::Tensor& input, const std::string& prefix, int64_t padding, int64_t groups) const {
    const auto& kernel = weights.get(prefix + ".weight");
    if (padding < 0) padding = kernel.size(-1) / 2;
    return convolution(weights, input.transpose(0, 1).unsqueeze(0), prefix, {1}, {padding}, groups).squeeze(0).transpose(0, 1);
}
at::Tensor NoteNetwork::linear(const at::Tensor& input, const std::string& prefix, bool bias) const {
    const auto& kernel = weights.get(prefix + ".weight");
    return at::linear(input, kernel.dim() == 3 ? kernel.squeeze(-1) : kernel, bias ? weights.optional(prefix + ".bias") : at::Tensor());
}
at::Tensor NoteNetwork::residual(const at::Tensor& input, const std::string& prefix, bool silu) const {
    auto branch = conv(weights.norm(input, prefix + ".0"), prefix + ".1", 1) * static_cast<float>(1.0 / std::sqrt(3.0));
    branch = silu ? at::silu(branch) : at::leaky_relu(branch, 0.01);
    return input + conv(branch, prefix + ".4", 0);
}
at::Tensor NoteNetwork::blocks(at::Tensor input, const std::string& prefix, int64_t count, bool silu) const {
    for (int64_t block = 0; block < count; ++block) input = residual(input, prefix + ".res_blocks.0.blocks." + std::to_string(block), silu);
    return conv(weights.norm(input, prefix + ".last_norm"), prefix + ".post_net1", 1);
}
at::Tensor NoteNetwork::relative_positions(int64_t frames) {
    const auto found = position_cache.find(frames);
    if (found != position_cache.end()) return found->second;
    std::vector<float> values(frames * 256);
    for (int64_t row = 0; row < frames; ++row)
        for (int64_t channel = 0; channel < 128; ++channel) {
            const float angle = static_cast<float>(4999 - row) * std::exp(-static_cast<float>(2 * channel) * std::log(10000.0f) / 256.0f);
            values[row * 256 + channel * 2] = std::sin(angle);
            values[row * 256 + channel * 2 + 1] = std::cos(angle);
        }
    auto tensor = at::from_blob(values.data(), {frames, 256}, at::kFloat).to(owner.runtime->device, at::kFloat, false, true);
    // Model buckets are bounded; retaining all arbitrary lengths is not a cache.
    if (position_cache.size() == 4) position_cache.erase(position_cache.begin());
    position_cache.emplace(frames, tensor);
    return tensor;
}
at::Tensor NoteNetwork::absolute_positions(int64_t valid, int64_t frames) {
    std::vector<float> values(frames * 256, 0.0f);
    const float scale = std::log(10000.0f) / 127.0f;
    for (int64_t row = 0; row < valid; ++row)
        for (int64_t channel = 0; channel < 128; ++channel) {
            const float angle = static_cast<float>(row + 1) * std::exp(-static_cast<float>(channel) * scale);
            values[row * 256 + channel] = std::sin(angle);
            values[row * 256 + channel + 128] = std::cos(angle);
        }
    return at::from_blob(values.data(), {frames, 256}, at::kFloat).to(owner.runtime->device, at::kFloat, false, true);
}
at::Tensor NoteNetwork::feed_forward(const at::Tensor& input, const std::string& prefix, bool experts) const {
    if (!experts) return linear(at::relu(linear(input, prefix + ".w_1")), prefix + ".w_2");
    std::vector<at::Tensor> parts;
    for (int64_t expert = 0; expert < 4; ++expert) {
        const auto name = prefix + ".freq_experts." + std::to_string(expert);
        parts.push_back(linear(at::relu(linear(input.narrow(-1, expert * 64, 64), name + ".w_1")), name + ".w_2"));
    }
    return at::cat(parts, -1);
}
at::Tensor NoteNetwork::relative_attention(const at::Tensor& input, const at::Tensor& positions, const std::string& prefix) const {
    const auto frames = input.size(0);
    const auto layout = [&](const at::Tensor& tensor) { return tensor.reshape({1, frames, 4, 64}).transpose(1, 2); };
    auto query = layout(linear(input, prefix + ".linear_q"));
    auto key = layout(linear(input, prefix + ".linear_k"));
    auto value = layout(linear(input, prefix + ".linear_v"));
    auto position = layout(linear(positions, prefix + ".linear_pos", false));
    auto query_content = query + weights.get(prefix + ".pos_bias_u").reshape({1, 4, 1, 64});
    auto query_position = query + weights.get(prefix + ".pos_bias_v").reshape({1, 4, 1, 64});
    auto content = at::matmul(query_content, key.transpose(-1, -2));
    auto relative = at::matmul(query_position, position.transpose(-1, -2));
    relative = at::constant_pad_nd(relative, {1, 0}, 0).reshape({1, 4, frames + 1, frames}).narrow(2, 1, frames);
    auto probabilities = at::softmax((content + relative) / 8.0, -1);
    auto output = at::matmul(probabilities, value).transpose(1, 2).reshape({frames, 256});
    return linear(output, prefix + ".linear_out");
}
at::Tensor NoteNetwork::convolution_module(const at::Tensor& input, const std::string& prefix) const {
    auto halves = conv(input, prefix + ".pointwise_conv1", 0).chunk(2, -1);
    auto gated = halves[0] * at::sigmoid(halves[1]);
    auto depthwise = conv(gated, prefix + ".depthwise_conv", 4, gated.size(-1));
    auto normalized = batch_norm(weights, depthwise.transpose(0, 1).unsqueeze(0), prefix + ".norm").squeeze(0).transpose(0, 1);
    return conv(at::silu(normalized), prefix + ".pointwise_conv2", 0);
}
at::Tensor NoteNetwork::conformer(at::Tensor value, const std::string& prefix, int64_t layers, bool experts) {
    auto positions = relative_positions(value.size(0));
    value = value * 16.0;
    for (int64_t layer = 0; layer < layers; ++layer) {
        owner.check_cancel();
        const auto name = prefix + ".encoder_layers." + std::to_string(layer);
        value = value + feed_forward(weights.norm(value, name + ".norm_ff_macaron"), name + ".feed_forward_macaron", experts) * 0.5;
        value = value + relative_attention(weights.norm(value, name + ".norm_mha"), positions, name + ".self_attn");
        value = value + convolution_module(weights.norm(value, name + ".norm_conv"), name + ".conv_module");
        value = value + feed_forward(weights.norm(value, name + ".norm_ff"), name + ".feed_forward", experts) * 0.5;
        value = weights.norm(value, name + ".norm_final");
    }
    return weights.norm(value, prefix + ".layer_norm");
}
at::Tensor NoteNetwork::cmu(at::Tensor value, const std::string& prefix, const at::Tensor& initial_mask, int64_t layers, bool experts) {
    if (value.dim() != 2 || value.size(1) != 256 || value.size(0) % 16) throw std::invalid_argument("native CMU requires [frames_multiple_of_16, 256]");
    std::vector<at::Tensor> skips;
    for (int64_t stage = 0; stage < 4; ++stage) {
        owner.check_cancel();
        const auto name = prefix + ".down.layers." + std::to_string(stage);
        value = residual(value, name + ".0.blocks.0");
        if (stage == 0 && initial_mask.defined()) value = value * initial_mask;
        value = residual(conv(value, name + ".1", 1), name + ".2.blocks.0");
        skips.push_back(value);
        value = at::avg_pool1d(value.transpose(0, 1).unsqueeze(0), {2}, {2}).squeeze(0).transpose(0, 1);
    }
    value = conv(weights.norm(value, prefix + ".down.last_norm"), prefix + ".down.post_net", 1);
    value = conv(value, prefix + ".mid.pre", 1);
    value = conv(conformer(value, prefix + ".mid.net", layers, experts), prefix + ".mid.post", 1);
    for (int64_t stage = 0; stage < 4; ++stage) {
        owner.check_cancel();
        const auto up = prefix + ".up.ups." + std::to_string(stage);
        auto output = at::conv_transpose1d(value.transpose(0, 1).unsqueeze(0), weights.get(up + ".0.weight"),
                                           weights.get(up + ".0.bias"), {2}, {1}, {1});
        value = at::leaky_relu(weights.norm(output.squeeze(0).transpose(0, 1), up + ".1"), 0.01);
        const auto name = prefix + ".up.layers." + std::to_string(stage);
        value = residual(conv(at::cat({value, skips[3 - stage]}, -1), name + ".0", 1), name + ".1.blocks.0");
    }
    return conv(weights.norm(value, prefix + ".up.last_norm"), prefix + ".up.post_net", 1);
}
} // namespace uta::torch_native
