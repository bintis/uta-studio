#pragma once
#include "gguf.hpp"
#include <atomic>
#include <memory>
#include <optional>

namespace uta::torch_native {
using TensorMap = std::map<std::string, at::Tensor>;
struct Runtime {
    at::Device device;
    std::string backend;
    std::string precision;
    Runtime(const std::string& backend, int index, const std::string& precision);
    void synchronize() const;
    // Opt-in fault localization; never enabled for performance measurements.
    bool trace_synchronization = false;
    // Process CPU/wall attribution around existing submission/completion only.
    bool profile_submission = false;
    void checkpoint(const std::string& stage) const;
};
struct Inputs {
    TensorMap tensors;
    const at::Tensor& get(const std::string& name) const;
    at::Tensor optional(const std::string& name) const;
    int64_t integer(const std::string& name, int64_t default_value) const;
    double number(const std::string& name, double default_value) const;
};
class Plan {
public:
    Plan(std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights)
        : runtime(std::move(runtime)), weights(std::move(weights)) {}
    virtual ~Plan() = default;
    virtual TensorMap forward(const std::string& operation, const Inputs& inputs) = 0;
    void check_cancel() const;
    std::shared_ptr<Runtime> runtime;
    std::shared_ptr<Weights> weights;
    std::atomic<bool> cancelled{false};
};
std::vector<std::string> model_resources();
std::unique_ptr<Plan> make_plan(const std::string& resource, std::shared_ptr<Runtime> runtime, std::shared_ptr<Weights> weights);
std::unique_ptr<Plan> make_roformer(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);
std::unique_ptr<Plan> make_rmvpe(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);
std::unique_ptr<Plan> make_fcpe(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);
std::unique_ptr<Plan> make_basic_pitch(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);
std::unique_ptr<Plan> make_game(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);
std::unique_ptr<Plan> make_jbm(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);
std::unique_ptr<Plan> make_stars(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);
std::unique_ptr<Plan> make_rosvot(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);
std::unique_ptr<Plan> make_qwen(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);
std::unique_ptr<Plan> make_firered(std::shared_ptr<Runtime>, std::shared_ptr<Weights>);

// Explicitly mixed-precision, full-context fused attention. No math-SDPA
// fallback. Packing and FP16 input/output conversion occur inside model timing.
at::Tensor fused_attention(const at::Tensor& query, const at::Tensor& key,
                           const at::Tensor& value, const at::Tensor& mask = {},
                           bool causal = false, bool grouped = false, double scale = 0.0);
// Explicit FP32 small-context attention for correctness-sensitive models.
// This is a model-selected algorithm, never a silent SDPA fallback.
at::Tensor dense_attention(const at::Tensor& query, const at::Tensor& key,
                           const at::Tensor& value, const at::Tensor& mask = {},
                           bool causal = false, double scale = 0.0);
at::Tensor rotary_interleaved(const at::Tensor& input, const at::Tensor& positions,
                              double base = 10000.0);
at::Tensor rotary_split(const at::Tensor& input, const at::Tensor& positions,
                        double base = 10000.0);
at::Tensor sinusoidal(int64_t length, int64_t dimensions, const at::Device& device);
at::Tensor convolution(const Weights& weights, const at::Tensor& input,
                       const std::string& prefix, at::IntArrayRef stride,
                       at::IntArrayRef padding, int64_t groups = 1);
at::Tensor batch_norm(const Weights& weights, const at::Tensor& input,
                      const std::string& prefix, double epsilon = 1e-5);
} // namespace uta::torch_native
