#include "runtime.hpp"
#include <stdexcept>

namespace uta::torch_native {
namespace {
class GamePlan final : public Plan {
public:
    GamePlan(std::shared_ptr<Runtime> selected, std::shared_ptr<Weights> loaded)
        : Plan(std::move(selected), std::move(loaded)) {
        auto& metadata = weights->container;
        heads = metadata.meta("game.encoder.num_heads").integer();
        head_dimension = metadata.meta("game.encoder.head_dim").integer();
        cycle = weights->get("region_embedding.embedding.weight").size(0);
    }
    TensorMap forward(const std::string& operation, const Inputs& inputs) override {
        check_cancel();
        if (operation == "encode" || operation == "encode_outputs") {
            // Invalidate stale conditioning before computation so a failed new
            // window cannot accidentally reuse the previous window's encoding.
            segmenter = estimator = at::Tensor();
            auto value = linear(linear(inputs.get("mel"), "spectrogram_projection"), "encoder.input_proj");
            auto positions = at::arange(value.size(0), value.options().dtype(at::kLong));
            value = encoder(value, "encoder", positions);
            auto parts = linear(norm(value, "encoder.output_norm.weight"), "encoder.output_proj").chunk(2, -1);
            segmenter = parts[0].contiguous();
            estimator = parts[1].contiguous();
            frame_positions = positions;
            if (operation == "encode_outputs") return {{"segmenter", segmenter}, {"estimator", estimator}};
            return {{"frames", at::scalar_tensor(segmenter.size(0), at::TensorOptions().dtype(at::kLong))}};
        }
        if (operation == "segment") {
            auto embedding = inputs.optional("segmenter");
            if (!embedding.defined()) embedding = segmenter;
            if (!embedding.defined()) throw std::invalid_argument("GAME segmenter requires a successfully encoded window");
            auto noise = inputs.get("noise").to(at::kLong);
            if (embedding.dim() != 2 || noise.dim() != 1 || noise.size(0) != embedding.size(0))
                throw std::invalid_argument("GAME segmenter embedding and noise timelines disagree");
            auto value = embedding + at::embedding(weights->get("noise_embedding.embedding.weight"), noise);
            auto time = inputs.get("time").reshape({1, 1});
            time = linear(at::gelu(linear(time, "time_embedding.0"), "none"), "time_embedding.2");
            value = value + time;
            value = value + at::embedding(weights->get("language_embedding.weight"), inputs.get("language").to(at::kLong).reshape({1}));
            value = linear(value, "segmenter.input_proj");
            auto positions = frame_positions.defined() && frame_positions.numel() == value.size(0)
                ? frame_positions : at::arange(value.size(0), value.options().dtype(at::kLong));
            value = encoder(value, "segmenter", positions);
            return {{"logits", linear(norm(value, "segmenter.output_norm.weight"), "segmenter.output_proj").squeeze(-1)}};
        }
        if (operation == "estimate") return estimate(inputs);
        if (operation == "clear") {
            segmenter = estimator = frame_positions = at::Tensor();
            return {{"cleared", at::scalar_tensor(1, at::kLong)}};
        }
        throw std::invalid_argument("GAME native operation must be encode, encode_outputs, segment, estimate or clear");
    }
private:
    int64_t heads = 0, head_dimension = 0, cycle = 0;
    at::Tensor segmenter, estimator, frame_positions;
    at::Tensor linear(const at::Tensor& input, const std::string& prefix) const {
        const auto& raw = weights->get(prefix + ".weight");
        auto kernel = raw.dim() == 3 && raw.size(-1) == 1 ? raw.squeeze(-1) : raw;
        return at::linear(input, kernel, weights->get(prefix + ".bias"));
    }
    at::Tensor norm(const at::Tensor& input, const std::string& weight) const {
        return weights->rms_norm(input, weight, 1e-6);
    }
    at::Tensor depthwise(const at::Tensor& input, const std::string& prefix, bool gelu) const {
        const auto& kernel = weights->get(prefix + ".weight");
        auto result = convolution(*weights, input.transpose(0, 1).unsqueeze(0), prefix,
                                  {1}, {kernel.size(-1) / 2}, input.size(-1)).squeeze(0).transpose(0, 1);
        return gelu ? at::gelu(result, "none") : result;
    }
    at::Tensor residual_glu(const at::Tensor& input, const std::string& norm_name,
                            const std::string& prefix, const std::string& scale_name, double scale) const {
        auto parts = linear(norm(input, norm_name), prefix + ".ln1").chunk(2, -1);
        auto branch = linear(at::gelu(parts[0], "none") * parts[1], prefix + ".ln2");
        return input + branch * weights->get(scale_name) * scale;
    }
    at::Tensor heads_layout(const at::Tensor& input) const {
        return input.reshape({1, input.size(0), heads, head_dimension}).transpose(1, 2);
    }
    at::Tensor attend(const at::Tensor& query, const at::Tensor& key, const at::Tensor& value,
                      const at::Tensor& mask = {}) const {
        auto output = runtime->precision == "mixed_attention" ? fused_attention(query, key, value, mask)
                                                               : dense_attention(query, key, value, mask);
        return output.transpose(1, 2).reshape({query.size(-2), heads * head_dimension});
    }
    at::Tensor attention(const at::Tensor& input, const std::string& prefix, const at::Tensor& positions) const {
        auto query = rotary_interleaved(heads_layout(linear(input, prefix + ".q_linear")), positions);
        auto parts = linear(input, prefix + ".kv_linear").chunk(2, -1);
        auto key = rotary_interleaved(heads_layout(parts[0]), positions);
        return linear(attend(query, key, heads_layout(parts[1])), prefix + ".out_linear");
    }
    at::Tensor cgmlp(const at::Tensor& input, const std::string& prefix) const {
        auto parts = at::gelu(linear(input, prefix + ".pw1"), "none").chunk(2, -1);
        auto convolved = depthwise(norm(parts[1], prefix + ".norm.weight"), prefix + ".dw", true);
        return linear(parts[0] * convolved, prefix + ".pw2");
    }
    at::Tensor merge(const at::Tensor& attention_value, const at::Tensor& convolution_value,
                     const std::string& depthwise_name, const std::string& linear_name) const {
        auto joined = at::cat({attention_value, convolution_value}, -1);
        return linear(joined + depthwise(joined, depthwise_name, false), linear_name);
    }
    at::Tensor encoder(at::Tensor input, const std::string& module, const at::Tensor& positions) {
        const auto layers = weights->container.meta("game." + module + ".num_layers").integer();
        for (int64_t layer = 0; layer < layers; ++layer) {
            check_cancel();
            const auto prefix = module + ".layers." + std::to_string(layer);
            input = residual_glu(input, prefix + ".norm1.weight", prefix + ".ffn1", prefix + ".lay_scale1.scale", 0.5);
            const auto attention_prefix = prefix + ".attn";
            auto attended = attention(norm(input, attention_prefix + ".a_norm.weight"), attention_prefix + ".attn", positions);
            auto convolved = cgmlp(norm(input, attention_prefix + ".c_norm.weight"), attention_prefix + ".c");
            auto branch = merge(attended, convolved, attention_prefix + ".merge_dw_conv", attention_prefix + ".merge_linear");
            input = input + branch * weights->get(prefix + ".lay_scale2.scale");
            input = residual_glu(input, prefix + ".norm2.weight", prefix + ".ffn2", prefix + ".lay_scale3.scale", 0.5);
        }
        return input;
    }
    at::Tensor mixed_positions(const at::Tensor& input, const at::Tensor& global, const at::Tensor& local) const {
        auto halves = input.chunk(2, -1);
        return at::cat({rotary_interleaved(halves[0], global), rotary_interleaved(halves[1], local)}, -1);
    }
    std::pair<at::Tensor, at::Tensor> joint_attention(const at::Tensor& pool, const at::Tensor& frames,
        const std::string& prefix, const at::Tensor& global, const at::Tensor& local, const at::Tensor& mask) const {
        auto pool_parts = linear(norm(pool, prefix + ".pool_norm.weight"), prefix + ".pool_qkv").chunk(3, -1);
        auto frame_parts = linear(norm(frames, prefix + ".x_norm.weight"), prefix + ".x_qkv").chunk(3, -1);
        auto pool_query = norm(heads_layout(pool_parts[0]), prefix + ".pool_q_norm.weight");
        auto pool_key = norm(heads_layout(pool_parts[1]), prefix + ".pool_k_norm.weight");
        auto frame_query = norm(heads_layout(frame_parts[0]), prefix + ".x_q_norm.weight");
        auto frame_key = norm(heads_layout(frame_parts[1]), prefix + ".x_k_norm.weight");
        auto query = mixed_positions(at::cat({pool_query, frame_query}, -2), global, local);
        auto key = mixed_positions(at::cat({pool_key, frame_key}, -2), global, local);
        auto value = at::cat({heads_layout(pool_parts[2]), heads_layout(frame_parts[2])}, -2);
        auto result = attend(query, key, value, mask);
        return {linear(result.narrow(0, 0, pool.size(0)), prefix + ".pool_out"),
                linear(result.narrow(0, pool.size(0), frames.size(0)), prefix + ".x_out")};
    }
    TensorMap estimate(const Inputs& inputs) {
        auto embedding = inputs.optional("estimator");
        if (!embedding.defined()) embedding = estimator;
        if (!embedding.defined()) throw std::invalid_argument("GAME estimator requires a successfully encoded window");
        auto regions = inputs.get("regions").to(at::kLong);
        if (regions.dim() != 1 || regions.size(0) != embedding.size(0)) throw std::invalid_argument("GAME estimator region timeline disagrees with encoder");
        const int64_t count = inputs.integer("@region_count", -1);
        if (count < 0) throw std::invalid_argument("GAME estimator requires the host-decoded @region_count");
        if (!count) return {{"pool_logits", at::empty({0, weights->get("estimator.output_proj_pool.bias").numel()}, embedding.options())}};
        auto frames = linear(embedding + at::embedding(weights->get("region_embedding.embedding.weight"), at::remainder(regions, cycle)), "estimator.input_proj");
        auto pool = weights->get("estimator.pool_token_gen.emb").reshape({1, -1}).expand({count, -1});
        auto options = regions.options();
        auto pool_ids = at::arange(1, count + 1, options);
        auto frame_indices = at::arange(regions.size(0), options);
        auto global = at::cat({pool_ids - 1, frame_indices}, 0);
        auto previous = at::cat({at::zeros({1}, options), regions.narrow(0, 0, regions.size(0) - 1)}, 0);
        auto starts = at::where(regions != previous, frame_indices, at::zeros_like(frame_indices));
        auto last_start = std::get<0>(at::cummax(starts, 0));
        auto local = at::cat({at::zeros({count}, options), at::where(regions > 0, frame_indices - last_start + 1, at::zeros_like(frame_indices))}, 0);
        auto identifiers = at::cat({pool_ids, regions}, 0);
        auto in_pool = at::arange(count + regions.size(0), options) < count;
        auto valid = identifiers != 0;
        auto allowed = valid.unsqueeze(1) & valid.unsqueeze(0)
            & ((in_pool.unsqueeze(1) == in_pool.unsqueeze(0)) | (identifiers.unsqueeze(1) == identifiers.unsqueeze(0)));
        // The checkpoint uses finite -10000 masking, including padded region
        // zero rows. Preserve that numerical convention instead of substituting
        // -infinity or zeroing rows differently from the existing source graph.
        auto mask = at::where(allowed, 0.0, -10000.0).to(at::kFloat);
        const auto layers = weights->container.meta("game.estimator.num_layers").integer();
        for (int64_t layer = 0; layer < layers; ++layer) {
            check_cancel();
            const auto prefix = "estimator.layers." + std::to_string(layer);
            frames = residual_glu(frames, prefix + ".norm_ffn1_x.weight", prefix + ".ffn1_x", prefix + ".lay_scale_ffn1_x.scale", 1.0);
            pool = residual_glu(pool, prefix + ".norm_ffn1_pool.weight", prefix + ".ffn1_pool", prefix + ".lay_scale_ffn1_pool.scale", 1.0);
            const auto attention_prefix = prefix + ".attn";
            auto [pool_attention, frame_attention] = joint_attention(pool, frames, attention_prefix + ".jattn", global, local, mask);
            auto pool_convolution = cgmlp(norm(pool, attention_prefix + ".c_norm_pool.weight"), attention_prefix + ".c_pool");
            auto frame_convolution = cgmlp(norm(frames, attention_prefix + ".c_norm_x.weight"), attention_prefix + ".c_x");
            pool = pool + merge(pool_attention, pool_convolution, attention_prefix + ".merge_dw_conv_pool", attention_prefix + ".merge_linear_pool")
                * weights->get(prefix + ".lay_scale_jpac_pool.scale");
            frames = frames + merge(frame_attention, frame_convolution, attention_prefix + ".merge_dw_conv_x", attention_prefix + ".merge_linear_x")
                * weights->get(prefix + ".lay_scale_jpac_x.scale");
            frames = residual_glu(frames, prefix + ".norm_ffn2_x.weight", prefix + ".ffn2_x", prefix + ".lay_scale_ffn2_x.scale", 1.0);
            pool = residual_glu(pool, prefix + ".norm_ffn2_pool.weight", prefix + ".ffn2_pool", prefix + ".lay_scale_ffn2_pool.scale", 1.0);
        }
        return {{"pool_logits", linear(norm(pool, "estimator.output_norm_pool.weight"), "estimator.output_proj_pool")}};
    }
};
}
std::unique_ptr<Plan> make_game(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    return std::make_unique<GamePlan>(std::move(runtime), std::move(weights));
}
} // namespace uta::torch_native
