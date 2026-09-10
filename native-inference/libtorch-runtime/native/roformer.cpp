#include "runtime.hpp"
#include <cmath>
#include <numeric>
#include <stdexcept>

namespace uta::torch_native {
namespace {
std::vector<int64_t> integer_buffer(Gguf& container, const std::string& name) {
    auto buffer = container.read_tensor(name).to(at::kLong).contiguous();
    const auto* data = buffer.const_data_ptr<int64_t>();
    return {data, data + buffer.numel()};
}
at::Tensor device_integers(std::vector<int64_t>& values, const at::Device& device) {
    return at::from_blob(values.data(), {static_cast<int64_t>(values.size())}, at::kLong).to(device, at::kLong, false, true);
}
struct PositionCache {
    int64_t length = 0;
    at::Tensor cosine, sine, key_cosine, key_sine;
};

class RoformerPlan final : public Plan {
public:
    RoformerPlan(std::shared_ptr<Runtime> selected, std::shared_ptr<Weights> loaded)
        : Plan(std::move(selected), std::move(loaded)) {
        auto& container = weights->container;
        architecture = container.text("general.architecture");
        if (architecture == "bs" || architecture == "bs-roformer") architecture = "bs_roformer";
        if (architecture == "mel_band" || architecture == "mel-band-roformer") architecture = "mel_band_roformer";
        if (architecture != "bs_roformer" && architecture != "bs_polarformer" && architecture != "mel_band_roformer")
            throw std::invalid_argument("weight architecture is not a RoFormer family: " + architecture);
        const auto prefix = architecture + '.';
        public_names = (architecture == "bs_roformer" || architecture == "bs_polarformer") && container.has_meta(prefix + "n_bands");
        polar = architecture == "bs_polarformer";
        const auto configured = [&](const std::string& name, int64_t value) { return container.integer(prefix + name, value); };
        fft = configured(public_names ? "n_fft" : "stft_n_fft", 2048);
        dimension = configured("dim", 384);
        depth = configured("depth", 6);
        heads = configured("heads", 8);
        head_dimension = configured("dim_head", 64);
        stems = configured(public_names ? "n_stems" : "num_stems", 1);
        final_norm = configured("has_final_norm", architecture != "mel_band_roformer") != 0;
        output_norm = architecture == "mel_band_roformer";
        skips = configured("skip_connection", 0) != 0;
        zero_dc = configured("zero_dc", 0) != 0;
        if (public_names) {
            widths = container.integers(prefix + "band_widths");
            const auto features = std::accumulate(widths.begin(), widths.end(), int64_t{0});
            std::vector<int64_t> indices(static_cast<std::size_t>(features / 2));
            std::iota(indices.begin(), indices.end(), 0);
            frequency_indices = device_integers(indices, runtime->device);
            frequency_counts = at::ones({fft / 2 + 1}, at::TensorOptions().dtype(at::kFloat).device(runtime->device));
        } else {
            auto frequencies = integer_buffer(container, "buffer_num_freqs_per_band");
            for (const auto count : frequencies) widths.push_back(count * 4);
            auto indices = integer_buffer(container, "buffer_freq_indices");
            frequency_indices = device_integers(indices, runtime->device);
            auto counts = integer_buffer(container, "buffer_num_bands_per_freq");
            frequency_counts = device_integers(counts, runtime->device).to(at::kFloat).clamp_min(1.0);
        }
        if (widths.empty() || dimension <= 0 || heads <= 0 || head_dimension <= 0 || stems <= 0)
            throw std::invalid_argument("invalid RoFormer dimensions in GGUF");
        for (const auto width : widths) if (width <= 0 || width % 4) throw std::invalid_argument("RoFormer band width does not encode stereo complex bins");
        if (configured("linear_transformer_depth", 0) != 0)
            throw std::invalid_argument("this checkpoint contains a linear-attention RoFormer stage absent from the current catalog plans");
        if (!public_names) {
            while (weights->has("mask_est.0.freq.0.mlp." + std::to_string(mask_layers * 2) + ".weight")) ++mask_layers;
            if (!mask_layers) throw std::invalid_argument("RoFormer mask estimator has no layers");
        }
    }

    TensorMap forward(const std::string& operation, const Inputs& inputs) override {
        check_cancel();
        if (operation == "constants") return {{"frequency_indices", frequency_indices}, {"bands_per_frequency", frequency_counts.to(at::kLong)}};
        const bool prepared = operation == "mask";
        if (!prepared && operation != "forward") throw std::invalid_argument("RoFormer operation must be forward or mask");
        auto spectrum = prepared ? at::Tensor() : inputs.get("spectrum");
        at::Tensor features;
        if (prepared) {
            features = inputs.get("features");
        } else {
            if (spectrum.dim() != 4 || spectrum.size(1) != fft / 2 + 1 || spectrum.size(2) != 2 || spectrum.size(3) != 2)
                throw std::invalid_argument("RoFormer spectrum layout is [frames, fft/2+1, stereo, complex]");
            features = spectrum.reshape({spectrum.size(0), -1, 2}).index_select(1, frequency_indices).flatten(1);
        }
        const auto frames = features.size(0);
        const auto total = std::accumulate(widths.begin(), widths.end(), int64_t{0});
        if (features.dim() != 2 || features.size(1) != total) throw std::invalid_argument("RoFormer prepared band feature shape mismatch");
        std::vector<at::Tensor> bands;
        int64_t offset = 0;
        for (std::size_t band = 0; band < widths.size(); ++band) {
            const auto prefix = "band_split." + std::to_string(band) + '.';
            auto input = features.narrow(1, offset, widths[band]);
            auto normalized = weights->rms_norm(input, prefix + (public_names ? "norm" : "norm.weight"), 1e-12);
            bands.push_back(at::linear(normalized, weights->get(prefix + (public_names ? "w" : "linear.weight")),
                                      weights->get(prefix + (public_names ? "b" : "linear.bias"))));
            offset += widths[band];
        }
        auto value = at::stack(bands, 1); // [time, band, model channel]
        runtime->checkpoint("roformer.band_split");
        std::vector<at::Tensor> previous;
        for (int64_t layer = 0; layer < depth; ++layer) {
            check_cancel();
            if (skips) for (const auto& skip : previous) value = value + skip;
            const auto prefix = "blk." + std::to_string(layer) + '.';
            auto time = value.transpose(0, 1).contiguous(); // [band, time, channel]
            time = attend(time, prefix + (public_names ? "time" : "time_attn"), true);
            time = feed_forward(time, prefix + (public_names ? "time" : "time_ff"));
            runtime->checkpoint(prefix + "time_feed_forward");
            if (output_norm) time = weights->rms_norm(time, prefix + "time_norm.weight", 1e-12);
            value = time.transpose(0, 1).contiguous();
            value = attend(value, prefix + (public_names ? "freq" : "freq_attn"), false);
            value = feed_forward(value, prefix + (public_names ? "freq" : "freq_ff"));
            runtime->checkpoint(prefix + "frequency_feed_forward");
            if (output_norm) value = weights->rms_norm(value, prefix + "freq_norm.weight", 1e-12);
            if (skips) previous.push_back(value);
        }
        if (final_norm) value = weights->rms_norm(value, public_names ? "final_norm" : "final_norm.weight", 1e-12);
        std::vector<at::Tensor> predicted;
        for (int64_t stem = 0; stem < stems; ++stem) {
            check_cancel();
            bands.clear();
            for (std::size_t band = 0; band < widths.size(); ++band) {
                auto current = value.select(1, static_cast<int64_t>(band));
                if (public_names) {
                    const auto prefix = "mask." + std::to_string(stem) + '.' + std::to_string(band) + '.';
                    current = at::tanh(at::linear(current, weights->get(prefix + "w1"), weights->get(prefix + "b1")));
                    current = at::linear(current, weights->get(prefix + "w2"), weights->get(prefix + "b2"));
                } else {
                    const auto prefix = "mask_est." + std::to_string(stem) + ".freq." + std::to_string(band) + ".mlp.";
                    for (int64_t layer = 0; layer < mask_layers; ++layer) {
                        current = weights->linear(current, prefix + std::to_string(layer * 2));
                        if (layer + 1 < mask_layers) current = at::tanh(current);
                    }
                }
                auto gated = current.chunk(2, -1);
                bands.push_back(gated[0] * at::sigmoid(gated[1]));
            }
            predicted.push_back(at::cat(bands, -1));
        }
        auto mask = at::stack(predicted, 1); // [time, stem, gathered stereo-complex feature]
        if (prepared) return {{"mask", mask}};
        std::vector<at::Tensor> separated;
        for (int64_t stem = 0; stem < stems; ++stem) {
            auto accumulated = at::zeros({frames, (fft / 2 + 1) * 2, 2}, spectrum.options());
            accumulated.index_add_(1, frequency_indices, mask.select(1, stem).reshape({frames, -1, 2}));
            accumulated = accumulated.reshape_as(spectrum) / frequency_counts.reshape({1, -1, 1, 1});
            auto real = spectrum.select(-1, 0) * accumulated.select(-1, 0) - spectrum.select(-1, 1) * accumulated.select(-1, 1);
            auto imaginary = spectrum.select(-1, 0) * accumulated.select(-1, 1) + spectrum.select(-1, 1) * accumulated.select(-1, 0);
            auto result = at::stack({real, imaginary}, -1);
            if (zero_dc) result.select(1, 0).zero_();
            separated.push_back(result);
        }
        return {{"spectrum", stems == 1 ? separated.front() : at::stack(separated, 0)}};
    }
private:
    std::string architecture;
    bool public_names = false, polar = false, final_norm = false, output_norm = false, skips = false, zero_dc = false;
    int64_t fft = 0, dimension = 0, depth = 0, heads = 0, head_dimension = 0, stems = 0, mask_layers = 0;
    std::vector<int64_t> widths;
    at::Tensor frequency_indices, frequency_counts;
    PositionCache time_position, frequency_position;

    const PositionCache& positions(int64_t length, bool time) {
        auto& cache = time ? time_position : frequency_position;
        if (cache.length == length) return cache;
        auto options = at::TensorOptions().device(runtime->device).dtype(at::kFloat);
        auto position = at::arange(length, options).reshape({1, 1, length, 1});
        if (polar) {
            auto phase = position * weights->get("pope.inv_freqs").reshape({1, 1, 1, head_dimension});
            cache.cosine = phase.cos();
            cache.sine = phase.sin();
            auto bias = weights->get(time ? "pope.time_k_phase_bias" : "pope.freq_k_phase_bias").reshape({1, heads, 1, head_dimension});
            cache.key_cosine = (phase + bias).cos();
            cache.key_sine = (phase + bias).sin();
        } else {
            auto inverse = at::exp(at::arange(0, head_dimension, 2, options) * (-std::log(10000.0) / head_dimension));
            auto phase = position * inverse;
            cache.cosine = phase.cos();
            cache.sine = phase.sin();
        }
        cache.length = length;
        return cache;
    }
    at::Tensor rotate(const at::Tensor& value, const at::Tensor& cosine, const at::Tensor& sine) const {
        if (polar) {
            auto magnitude = at::softplus(value);
            return at::stack({magnitude * cosine, magnitude * sine}, -1).flatten(-2);
        }
        auto pairs = value.reshape({value.size(0), heads, value.size(2), head_dimension / 2, 2});
        auto even = pairs.select(-1, 0), odd = pairs.select(-1, 1);
        return at::stack({even * cosine - odd * sine, even * sine + odd * cosine}, -1).flatten(-2);
    }
    std::string name(const std::string& prefix, const std::string& public_suffix, const std::string& private_suffix) const {
        return prefix + (public_names ? '.' + public_suffix : '_' + private_suffix);
    }
    at::Tensor attend(const at::Tensor& sequence, const std::string& prefix, bool time) {
        auto normalized = weights->rms_norm(sequence, name(prefix, "attn_norm", "norm.weight"), 1e-12);
        runtime->checkpoint(prefix + ".normalization");
        const auto batch = sequence.size(0), length = sequence.size(1);
        auto qkv = at::linear(normalized, weights->get(name(prefix, "qkv", "qkv.weight"))).chunk(3, -1);
        runtime->checkpoint(prefix + ".qkv");
        auto query = qkv[0].reshape({batch, length, heads, head_dimension}).transpose(1, 2);
        auto key = qkv[1].reshape({batch, length, heads, head_dimension}).transpose(1, 2);
        auto value = qkv[2].reshape({batch, length, heads, head_dimension}).transpose(1, 2);
        const auto& cache = positions(length, time);
        query = rotate(query, cache.cosine, cache.sine);
        key = rotate(key, polar ? cache.key_cosine : cache.cosine, polar ? cache.key_sine : cache.sine);
        runtime->checkpoint(prefix + ".rotary");
        const double scale = 1.0 / std::sqrt(static_cast<double>(head_dimension));
        auto attended = runtime->precision == "mixed_attention"
            ? fused_attention(query, key, value, {}, false, false, scale)
            : dense_attention(query, key, value, {}, false, scale);
        runtime->checkpoint(prefix + ".attention");
        auto gates = at::sigmoid(at::linear(normalized, weights->get(name(prefix, "gates_w", "gate.weight")),
                                            weights->get(name(prefix, "gates_b", "gate.bias"))));
        attended = attended.transpose(1, 2) * gates.unsqueeze(-1);
        attended = attended.reshape({batch, length, heads * head_dimension});
        return sequence + at::linear(attended, weights->get(name(prefix, "out", "out.weight")));
    }
    at::Tensor feed_forward(const at::Tensor& sequence, const std::string& prefix) const {
        auto current = weights->rms_norm(sequence, name(prefix, "ff_norm", "norm.weight"), 1e-12);
        current = at::linear(current, weights->get(name(prefix, "ff1_w", "in.weight")), weights->get(name(prefix, "ff1_b", "in.bias")));
        runtime->checkpoint(prefix + ".feed_forward_projection");
        current = at::gelu(current, "none");
        return sequence + at::linear(current, weights->get(name(prefix, "ff2_w", "out.weight")), weights->get(name(prefix, "ff2_b", "out.bias")));
    }
};
} // namespace
std::unique_ptr<Plan> make_roformer(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    return std::make_unique<RoformerPlan>(std::move(runtime), std::move(weights));
}
} // namespace uta::torch_native
