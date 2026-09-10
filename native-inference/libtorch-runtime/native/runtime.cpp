#include "runtime.hpp"
#include <ATen/Context.h>
#include <ATen/Parallel.h>
#include <c10/core/DeviceGuard.h>
#include <c10/core/InferenceMode.h>
#include <cmath>
#include <limits>
#include <stdexcept>
#if defined(UTA_LIBTORCH_ROCM)
#include <hip/hip_runtime_api.h>
#elif defined(UTA_LIBTORCH_XPU)
#include <torch/xpu.h>
#endif

namespace uta::torch_native {
Runtime::Runtime(const std::string& selected, int index, const std::string& arithmetic)
    : device(at::kCPU), backend(selected), precision(arithmetic) {
    if (precision != "strict" && precision != "mixed_attention")
        throw std::invalid_argument("native precision must be strict or mixed_attention");
    if (backend == "libtorch_cpu") {
        if (index != 0) throw std::invalid_argument("the explicit CPU diagnostic device has index zero");
    } else if (backend == "libtorch_rocm") {
#if defined(UTA_LIBTORCH_ROCM)
        if (!at::globalContext().hasROCM()) throw std::runtime_error("loaded LibTorch does not provide ROCm; no fallback");
        const auto status = hipSetDevice(index);
        if (status != hipSuccess) throw std::runtime_error(std::string("cannot select ROCm device: ") + hipGetErrorString(status));
        device = at::Device(at::kCUDA, index);
#else
        throw std::runtime_error("this native library was not built with ROCm; install the requested runtime");
#endif
    } else if (backend == "libtorch_xpu") {
#if defined(UTA_LIBTORCH_XPU)
        if (index < 0 || index >= static_cast<int>(torch::xpu::device_count()))
            throw std::runtime_error("requested XPU device is unavailable; no fallback");
        device = at::Device(at::kXPU, index);
#else
        throw std::runtime_error("this native library was not built with XPU; install the requested runtime");
#endif
    } else {
        throw std::invalid_argument("unknown explicit native backend: " + backend);
    }
    auto& context = at::globalContext();
    context.setFloat32Precision(at::Float32Backend::GENERIC, at::Float32Op::ALL, at::Float32Precision::IEEE);
    context.setSDPUseMath(false);
    at::set_num_threads(2);
}
void Runtime::synchronize() const {
    if (device.is_cpu()) return;
    c10::DeviceGuard guard(device);
#if defined(UTA_LIBTORCH_ROCM)
    const auto status = hipDeviceSynchronize();
    if (status != hipSuccess) throw std::runtime_error(std::string("ROCm synchronization failed: ") + hipGetErrorString(status));
#elif defined(UTA_LIBTORCH_XPU)
    torch::xpu::synchronize(device.index());
#endif
}
const at::Tensor& Inputs::get(const std::string& name) const {
    const auto found = tensors.find(name);
    if (found == tensors.end()) throw std::invalid_argument("missing native input tensor: " + name);
    return found->second;
}
at::Tensor Inputs::optional(const std::string& name) const {
    const auto found = tensors.find(name);
    return found == tensors.end() ? at::Tensor() : found->second;
}
int64_t Inputs::integer(const std::string& name, int64_t default_value) const {
    return tensors.contains(name) ? get(name).item<int64_t>() : default_value;
}
double Inputs::number(const std::string& name, double default_value) const {
    return tensors.contains(name) ? get(name).item<double>() : default_value;
}
void Plan::check_cancel() const {
    if (cancelled.load(std::memory_order_relaxed)) throw std::runtime_error("native model execution cancelled");
}

namespace {
using Factory = std::unique_ptr<Plan> (*)(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);
const std::map<std::string, Factory>& factories() {
    // Only executable, linked implementations are advertised. New families
    // enter this table together with their actual native source and tests.
    static const std::map<std::string, Factory> table{
        {"bs_roformer_leap_xe90_vocals", make_roformer},
        {"bs_roformer_leap_xe90_instrumental", make_roformer},
        {"bs_polarformer_public_instrumental", make_roformer},
        {"melband_roformer_harmony", make_roformer},
        {"melband_roformer_denoise_aufr33", make_roformer},
        {"melband_roformer_dereverb_anvuew", make_roformer},
        {"rmvpe", make_rmvpe}, {"fcpe", make_fcpe},
        {"basic_pitch", make_basic_pitch}, {"jbm555_cectc_80", make_jbm},
        {"game_1_0_3_small", make_game}, {"game_1_0_3_medium", make_game}, {"game_1_0_3_large", make_game},
        {"qwen3_asr_1_7b", make_qwen}, {"qwen3_forced_aligner_0_6b", make_qwen},
        {"firered_asr2_aed", make_firered}, {"rosvot", make_rosvot}, {"stars", make_stars},
    };
    return table;
}
}
std::vector<std::string> model_resources() {
    std::vector<std::string> result;
    for (const auto& [resource, factory] : factories()) result.push_back(resource);
    return result;
}
std::unique_ptr<Plan> make_plan(const std::string& resource, std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights) {
    const auto found = factories().find(resource);
    if (found == factories().end()) throw std::invalid_argument("no linked native LibTorch model plan for resource: " + resource);
    return found->second(std::move(runtime), std::move(weights));
}

at::Tensor fused_attention(const at::Tensor& query, const at::Tensor& key, const at::Tensor& value,
                           const at::Tensor& mask, bool causal, bool grouped, double scale) {
    auto query_half = query.to(at::kHalf).contiguous();
    auto key_half = key.to(at::kHalf).contiguous();
    auto value_half = value.to(at::kHalf).contiguous();
    std::optional<at::Tensor> attention_mask;
    if (mask.defined()) attention_mask = mask.scalar_type() == at::kBool ? mask : mask.to(at::kHalf);
    return at::scaled_dot_product_attention(query_half, key_half, value_half, attention_mask,
                                            0.0, causal, scale == 0.0 ? std::nullopt : std::optional<double>(scale), grouped).to(at::kFloat);
}

at::Tensor dense_attention(const at::Tensor& query, const at::Tensor& key, const at::Tensor& value,
                           const at::Tensor& mask, bool causal, double scale) {
    // Exact FP32 attention, tiled along queries. This keeps the score workspace
    // bounded by a query tile instead of materializing the whole NxN matrix.
    // Selecting strict arithmetic is explicit; this is not a fused-kernel fallback.
    const int64_t length = query.size(-2);
    const int64_t key_length = key.size(-2);
    if (!length || !key_length) throw std::invalid_argument("attention requires nonempty query and key sequences");
    if (scale == 0.0) scale = 1.0 / std::sqrt(static_cast<double>(query.size(-1)));
    const int64_t query_heads = query.size(-3);
    const int64_t key_heads = key.size(-3);
    if (query_heads % key_heads) throw std::invalid_argument("query heads are not divisible by KV heads");
    auto actual_key = key;
    auto actual_value = value;
    if (query_heads != key_heads) {
        actual_key = at::repeat_interleave(key, query_heads / key_heads, -3);
        actual_value = at::repeat_interleave(value, query_heads / key_heads, -3);
    }
    std::vector<at::Tensor> tiles;
    const auto key_columns = causal ? at::arange(key_length, query.options().dtype(at::kLong)).unsqueeze(0) : at::Tensor();
    for (int64_t begin = 0; begin < length; begin += 64) {
        const int64_t count = std::min<int64_t>(64, length - begin);
        auto scores = at::matmul(query.narrow(-2, begin, count).to(at::kFloat), actual_key.to(at::kFloat).transpose(-1, -2)) * scale;
        if (mask.defined()) {
            auto piece = mask.dim() >= 2 && mask.size(-2) == length ? mask.narrow(-2, begin, count) : mask;
            if (piece.scalar_type() == at::kBool) scores = scores.masked_fill(piece.logical_not(), -std::numeric_limits<float>::infinity());
            else scores = scores + piece.to(at::kFloat);
        }
        if (causal) {
            auto query_rows = at::arange(begin, begin + count, query.options().dtype(at::kLong)).unsqueeze(-1);
            scores = scores.masked_fill(key_columns > query_rows, -std::numeric_limits<float>::infinity());
        }
        auto probabilities = at::softmax(scores, -1);
        // A fully masked padded row has no valid value. Preserve the SDPA zero
        // output convention rather than propagating NaN from softmax(-inf).
        probabilities = at::where(at::isneginf(scores).all(-1, true), at::zeros_like(probabilities), probabilities);
        tiles.push_back(at::matmul(probabilities, actual_value.to(at::kFloat)));
    }
    return at::cat(tiles, -2);
}

at::Tensor rotary_interleaved(const at::Tensor& input, const at::Tensor& positions, double base) {
    const int64_t dimensions = input.size(-1);
    if (dimensions % 2) throw std::invalid_argument("interleaved rotary dimension must be even");
    auto frequency = at::exp(at::arange(0, dimensions, 2, input.options().dtype(at::kFloat)) * (-std::log(base) / dimensions));
    auto phase = positions.to(at::kFloat).unsqueeze(-1) * frequency;
    while (phase.dim() < input.dim()) phase = phase.unsqueeze(0);
    auto paired = input.reshape({input.size(0), input.size(1), input.size(2), dimensions / 2, 2});
    auto even = paired.select(-1, 0);
    auto odd = paired.select(-1, 1);
    return at::stack({even * phase.cos() - odd * phase.sin(), even * phase.sin() + odd * phase.cos()}, -1).flatten(-2);
}
at::Tensor rotary_split(const at::Tensor& input, const at::Tensor& positions, double base) {
    const int64_t dimensions = input.size(-1);
    if (dimensions % 2) throw std::invalid_argument("split rotary dimension must be even");
    auto frequency = at::exp(at::arange(0, dimensions, 2, input.options().dtype(at::kFloat)) * (-std::log(base) / dimensions));
    auto phase = positions.to(at::kFloat).unsqueeze(-1) * frequency;
    while (phase.dim() < input.dim()) phase = phase.unsqueeze(0);
    auto halves = input.chunk(2, -1);
    return at::cat({halves[0] * phase.cos() - halves[1] * phase.sin(), halves[1] * phase.cos() + halves[0] * phase.sin()}, -1);
}
at::Tensor sinusoidal(int64_t length, int64_t dimensions, const at::Device& device) {
    auto options = at::TensorOptions().dtype(at::kFloat).device(device);
    auto frequency = at::exp(at::arange(0, dimensions, 2, options) * (-std::log(10000.0) / dimensions));
    auto phase = at::arange(length, options).unsqueeze(-1) * frequency;
    return at::stack({phase.sin(), phase.cos()}, -1).flatten(-2).narrow(-1, 0, dimensions);
}
at::Tensor convolution(const Weights& weights, const at::Tensor& input, const std::string& prefix,
                       at::IntArrayRef stride, at::IntArrayRef padding, int64_t groups) {
    const auto& kernel = weights.get(prefix + ".weight");
    auto bias = weights.optional(prefix + ".bias");
    if (kernel.dim() == 3) return at::conv1d(input, kernel, bias, stride, padding, {1}, groups);
    if (kernel.dim() == 4) return at::conv2d(input, kernel, bias, stride, padding, {1, 1}, groups);
    throw std::invalid_argument("native convolution kernel must have rank three or four: " + prefix);
}
at::Tensor batch_norm(const Weights& weights, const at::Tensor& input, const std::string& prefix, double epsilon) {
    if (input.dim() < 2) throw std::invalid_argument("native batch norm input rank is invalid: " + prefix);
    const auto channels = input.size(1);
    std::vector<int64_t> shape(static_cast<std::size_t>(input.dim()), 1);
    shape[1] = channels;
    const auto mean = weights.get(prefix + ".running_mean").reshape(shape);
    const auto variance = weights.get(prefix + ".running_var").reshape(shape);
    const auto scale = weights.get(prefix + ".weight").reshape(shape);
    const auto bias = weights.get(prefix + ".bias").reshape(shape);
    return (input - mean) * at::rsqrt(variance + epsilon) * scale + bias;
}
} // namespace uta::torch_native
