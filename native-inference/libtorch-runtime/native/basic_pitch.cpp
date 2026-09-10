#include "runtime.hpp"
#include <algorithm>
#include <stdexcept>

namespace uta::torch_native {
namespace {
class BasicPitchPlan final : public Plan {
public:
    BasicPitchPlan(std::shared_ptr<Runtime> selected, std::shared_ptr<Weights> loaded)
        : Plan(std::move(selected), std::move(loaded)) {
        for (const auto* prefix : {"contour_conv1", "contour_final", "note_conv1", "note_final", "onset_conv1", "onset_final"}) {
            const auto& raw = weights->get(std::string(prefix) + ".weight");
            auto shape = raw.sizes().vec();
            std::reverse(shape.begin(), shape.end());
            // This converter recorded [O,I,H,W] directly as GGUF dimensions but
            // kept OIHW bytes. Restore the actual byte layout with a view, not
            // an incorrect permutation of the misleading stored dimensions.
            kernels.emplace(prefix, raw.reshape(shape));
        }
        real_kernel = weights->get("cqt.conv_real.weight").reshape({36, 1, 256});
        imaginary_kernel = weights->get("cqt.conv_imag.weight").reshape({36, 1, 256});
        cqt_kernel = at::cat({real_kernel, imaginary_kernel}, 0);
        lowpass = weights->get("cqt.lowpass.weight").reshape({1, 1, 256});
        const std::vector<int64_t> shifts{-36, 0, 36, 57, 72, 84, 93, 101};
        std::vector<int64_t> indices;
        std::vector<float> valid;
        for (const auto shift : shifts)
            for (int64_t bin = 0; bin < 264; ++bin) {
                const auto source = bin + shift;
                indices.push_back(std::clamp<int64_t>(source, 0, 308));
                valid.push_back(source >= 0 && source < 309 ? 1.0f : 0.0f);
            }
        harmonic_indices = at::from_blob(indices.data(), {static_cast<int64_t>(indices.size())}, at::kLong).to(runtime->device, at::kLong, false, true);
        harmonic_valid = at::from_blob(valid.data(), {1, 1, static_cast<int64_t>(valid.size())}, at::kFloat).to(runtime->device, at::kFloat, false, true);
    }
    TensorMap forward(const std::string& operation, const Inputs& inputs) override {
        check_cancel();
        at::Tensor harmonics;
        if (operation == "forward") {
            auto audio = inputs.get("audio");
            if (audio.dim() == 1) audio = audio.unsqueeze(0);
            if (audio.dim() != 2 || audio.size(-1) != 43844)
                throw std::invalid_argument("Basic Pitch native audio layout is [batch, 43844] or [43844]");
            harmonics = frontend(audio);
        } else if (operation == "activations") {
            harmonics = inputs.get("harmonics");
            if (harmonics.dim() == 3) harmonics = harmonics.unsqueeze(0);
            if (harmonics.dim() != 4 || harmonics.size(1) != 8 || harmonics.size(3) != 264)
                throw std::invalid_argument("Basic Pitch harmonic layout is [batch, 8, frames, 264]");
        } else throw std::invalid_argument("Basic Pitch operation must be forward or activations");
        auto onset_first = at::relu(convolve(harmonics, "onset_conv1", 3));
        auto contour_first = at::relu(convolve(harmonics, "contour_conv1", 1));
        auto contour = at::sigmoid(convolve(contour_first, "contour_final", 1));
        auto note_first = at::relu(convolve(contour, "note_conv1", 3));
        auto note_head = at::sigmoid(convolve(note_first, "note_final", 1));
        auto onset_head = at::sigmoid(convolve(at::cat({note_head, onset_first}, 1), "onset_final", 1));
        // Keep the existing evidence mapping explicitly: source raw output
        // labels are opposite the Studio note/onset labels.
        return {{"frame", onset_head.squeeze(1)}, {"onset", note_head.squeeze(1)}, {"contour", contour.squeeze(1)}};
    }
private:
    std::map<std::string, at::Tensor> kernels;
    at::Tensor real_kernel, imaginary_kernel, cqt_kernel, lowpass, harmonic_indices, harmonic_valid;
    at::Tensor conv1d_gemm(const at::Tensor& input, const at::Tensor& kernel, int64_t stride, int64_t padding) const {
        if (input.dim() != 3 || kernel.dim() != 3 || input.size(1) != kernel.size(1))
            throw std::invalid_argument("Basic Pitch GEMM convolution shape mismatch");
        auto padded = padding ? at::constant_pad_nd(input, {padding, padding}, 0.0) : input;
        const auto width = kernel.size(2);
        if (padded.size(2) < width) throw std::invalid_argument("Basic Pitch GEMM convolution kernel exceeds input");
        auto windows = padded.unfold(2, width, stride).permute({0, 2, 1, 3}).contiguous();
        auto matrix = windows.reshape({windows.size(0), windows.size(1), -1});
        auto weights_matrix = kernel.reshape({kernel.size(0), -1});
        return at::matmul(matrix, weights_matrix.transpose(0, 1)).transpose(1, 2).contiguous();
    }
    at::Tensor frontend(const at::Tensor& audio) {
        auto signal = audio.unsqueeze(1);
        std::vector<at::Tensor> octaves;
        int64_t hop = 256;
        for (int64_t octave = 0; octave < 9; ++octave) {
            check_cancel();
            if (octave) {
                signal = conv1d_gemm(signal, lowpass, 2, 127);
                hop /= 2;
            }
            auto padded = at::reflection_pad1d(signal, {128, 128});
            auto coefficients = conv1d_gemm(padded, cqt_kernel, hop, 0);
            auto parts = coefficients.chunk(2, 1);
            octaves.push_back(parts[0].square() + parts[1].square());
        }
        std::reverse(octaves.begin(), octaves.end());
        auto energy = at::cat(octaves, 1).narrow(1, 15, 309);
        auto lengths = weights->get("cqt.sqrt_lengths").reshape({1, 309, 1});
        // The reference scales real and imaginary values before squaring.
        // Multiplication by lengths^2 is mathematically identical; all steps
        // remain explicitly FP32 and complete activations are compared in tests.
        auto logarithm = 10.0 * at::log10(energy * lengths.square() + 1e-10);
        auto minimum = at::amin(logarithm, {1, 2}, true);
        auto maximum = at::amax(logarithm, {1, 2}, true);
        auto range = maximum - minimum;
        auto normalized = at::where(range > 0, (logarithm - minimum) / range.clamp_min(1e-30), at::zeros_like(logarithm));
        normalized = normalized * weights->get("cqt_bn.scale") + weights->get("cqt_bn.shift");
        auto gathered = normalized.transpose(1, 2).index_select(-1, harmonic_indices) * harmonic_valid;
        return gathered.reshape({audio.size(0), gathered.size(1), 8, 264}).permute({0, 2, 1, 3}).contiguous();
    }
    at::Tensor convolve(const at::Tensor& input, const std::string& prefix, int64_t frequency_stride) const {
        const auto& kernel = kernels.at(prefix);
        const int64_t frequency_output = (input.size(3) + frequency_stride - 1) / frequency_stride;
        const int64_t frequency_padding = std::max<int64_t>(0, (frequency_output - 1) * frequency_stride + kernel.size(3) - input.size(3)) / 2;
        return at::conv2d(input, kernel, weights->get(prefix + ".bias"), {1, frequency_stride}, {kernel.size(2) / 2, frequency_padding});
    }
};
}
std::unique_ptr<Plan> make_basic_pitch(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    return std::make_unique<BasicPitchPlan>(std::move(runtime), std::move(weights));
}
} // namespace uta::torch_native
