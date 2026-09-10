#include "api.h"
#include "runtime.hpp"
#include <c10/core/DeviceGuard.h>
#include <c10/core/InferenceMode.h>
#include <chrono>
#include <torch/version.h>
#include <limits>
#include <mutex>
#include <stdexcept>

using uta::torch_native::Inputs;
using uta::torch_native::Plan;
using uta::torch_native::Runtime;
using uta::torch_native::Weights;

struct UtaLibtorchRuntime { std::shared_ptr<Runtime> runtime; };
struct UtaLibtorchModel {
    std::unique_ptr<Plan> plan;
    std::string metadata;
    std::mutex execution;
};
struct OutputTensor {
    std::string name;
    at::Tensor tensor;
    std::vector<int64_t> dimensions;
};
struct UtaLibtorchResult {
    std::vector<OutputTensor> tensors;
    UtaLibtorchTimings timings{};
};
namespace {
thread_local std::string error_text;
using Clock = std::chrono::steady_clock;
double seconds(Clock::time_point start) { return std::chrono::duration<double>(Clock::now() - start).count(); }
void save_error() noexcept {
    try { throw; }
    catch (const std::exception& error) { try { error_text = error.what(); } catch (...) {} }
    catch (...) { try { error_text = "unknown native LibTorch exception"; } catch (...) {} }
}
const char* required(const char* value, const char* description) {
    if (!value || !*value) throw std::invalid_argument(std::string("missing ") + description);
    return value;
}
at::Tensor input_tensor(const UtaLibtorchTensor& input, const at::Device& device) {
    if (input.rank && !input.dimensions) throw std::invalid_argument("native tensor dimensions are null");
    uint64_t elements = 1;
    std::vector<int64_t> shape;
    shape.reserve(input.rank);
    for (uint32_t axis = 0; axis < input.rank; ++axis) {
        const auto dimension = input.dimensions[axis];
        if (dimension < 0 || (dimension && elements > static_cast<uint64_t>(std::numeric_limits<int64_t>::max()) / dimension))
            throw std::invalid_argument("native input tensor dimensions overflow the address range");
        elements *= static_cast<uint64_t>(dimension);
        shape.push_back(dimension);
    }
    if (elements != input.elements || (elements && !input.data)) throw std::invalid_argument("native input shape and buffer disagree");
    const auto dtype = input.kind == 0 ? at::kFloat : input.kind == 1 ? at::kLong : at::ScalarType::Undefined;
    if (dtype == at::ScalarType::Undefined) throw std::invalid_argument("native input tensor kind must be float32 or int64");
    auto options = at::TensorOptions().device(at::kCPU).dtype(dtype);
    if (!elements) return at::empty(shape, options.device(device));
    auto tensor = at::from_blob(const_cast<void*>(input.data), shape, options);
    // Explicit scalar controls are host data; learned-stage inputs upload once.
    const bool scalar_control = input.name && input.name[0] == '@';
    return scalar_control ? tensor.clone() : tensor.to(device, dtype, false, true);
}
}

extern "C" {
const char* uta_libtorch_build_info(void) noexcept {
    try {
        static const std::string description = [] {
#if defined(UTA_LIBTORCH_ROCM)
            const char* compiled = "libtorch_rocm";
#elif defined(UTA_LIBTORCH_XPU)
            const char* compiled = "libtorch_xpu";
#else
            const char* compiled = "libtorch_cpu";
#endif
            std::string result = "{\"torch_version\":" + uta::torch_native::json_string(TORCH_VERSION)
                + ",\"compiled_backend\":" + uta::torch_native::json_string(compiled) + ",\"models\":[";
            bool first = true;
            for (const auto& resource : uta::torch_native::model_resources()) {
                if (!first) result += ',';
                result += uta::torch_native::json_string(resource);
                first = false;
            }
            result += "],\"roformer_projection_math\":\"ieee\"";
            return result + ",\"qualification\":\"not_asserted\"}";
        }();
        return description.c_str();
    } catch (...) { save_error(); return nullptr; }
}
size_t uta_libtorch_tensor_layout_size(void) noexcept { return sizeof(UtaLibtorchTensor); }
const char* uta_libtorch_last_error(void) noexcept { return error_text.c_str(); }
UtaLibtorchRuntime* uta_libtorch_runtime_create(const char* backend, int device, const char* precision) noexcept {
    try {
        error_text.clear();
        c10::InferenceMode inference;
        auto result = std::make_unique<UtaLibtorchRuntime>();
        result->runtime = std::make_shared<Runtime>(required(backend, "native backend"), device, required(precision, "native precision"));
        return result.release();
    } catch (...) { save_error(); return nullptr; }
}
void uta_libtorch_runtime_free(UtaLibtorchRuntime* runtime) noexcept {
    try { delete runtime; } catch (...) { save_error(); }
}
UtaLibtorchModel* uta_libtorch_model_open(UtaLibtorchRuntime* runtime, const char* resource, const char* path) noexcept {
    try {
        error_text.clear();
        if (!runtime || !runtime->runtime) throw std::invalid_argument("native runtime handle is null");
        const std::string model_resource = required(resource, "model resource");
        const std::string model_path = required(path, "GGUF path");
        c10::InferenceMode inference;
        c10::DeviceGuard guard(runtime->runtime->device);
        auto weights = std::make_shared<Weights>(model_path, runtime->runtime->device);
        auto result = std::make_unique<UtaLibtorchModel>();
        result->metadata = weights->container.metadata_json();
        result->plan = uta::torch_native::make_plan(model_resource, runtime->runtime, std::move(weights));
        runtime->runtime->synchronize();
        return result.release();
    } catch (...) { save_error(); return nullptr; }
}
void uta_libtorch_model_free(UtaLibtorchModel* model) noexcept {
    try { delete model; } catch (...) { save_error(); }
}
const char* uta_libtorch_model_metadata(const UtaLibtorchModel* model) noexcept {
    if (!model) return nullptr;
    return model->metadata.c_str();
}
void uta_libtorch_model_cancel(UtaLibtorchModel* model, int cancelled) noexcept {
    if (model && model->plan) model->plan->cancelled.store(cancelled != 0, std::memory_order_relaxed);
}
UtaLibtorchResult* uta_libtorch_model_forward(UtaLibtorchModel* model, const char* operation,
                                            const UtaLibtorchTensor* inputs, size_t input_count) noexcept {
    try {
        error_text.clear();
        if (!model || !model->plan) throw std::invalid_argument("native model handle is null");
        if (input_count && !inputs) throw std::invalid_argument("native input array is null");
        required(operation, "native model operation");
        std::lock_guard lock(model->execution);
        auto& plan = *model->plan;
        c10::InferenceMode inference;
        c10::DeviceGuard guard(plan.runtime->device);
        plan.check_cancel();
        auto result = std::make_unique<UtaLibtorchResult>();
        plan.runtime->synchronize();
        auto begin = Clock::now();
        Inputs prepared;
        for (size_t index = 0; index < input_count; ++index) {
            const std::string name = required(inputs[index].name, "native tensor name");
            if (!prepared.tensors.emplace(name, input_tensor(inputs[index], plan.runtime->device)).second)
                throw std::invalid_argument("duplicate native tensor input: " + name);
        }
        plan.runtime->synchronize();
        result->timings.upload_seconds = seconds(begin);
        begin = Clock::now();
        auto output = plan.forward(operation, prepared);
        plan.runtime->synchronize();
        result->timings.synchronized_compute_seconds = seconds(begin);
        begin = Clock::now();
        for (auto& [name, tensor] : output) {
            if (!tensor.defined()) throw std::runtime_error("native model returned an undefined tensor: " + name);
            auto copied = tensor.to(at::TensorOptions().device(at::kCPU).dtype(tensor.is_floating_point() ? at::kFloat : at::kLong)).contiguous();
            result->tensors.push_back({name, copied, copied.sizes().vec()});
        }
        result->timings.readback_seconds = seconds(begin);
        return result.release();
    } catch (...) { save_error(); return nullptr; }
}
size_t uta_libtorch_result_count(const UtaLibtorchResult* result) noexcept { return result ? result->tensors.size() : 0; }
int uta_libtorch_result_tensor(const UtaLibtorchResult* result, size_t index, UtaLibtorchTensor* output) noexcept {
    try {
        error_text.clear();
        if (!result || !output || index >= result->tensors.size()) throw std::invalid_argument("native result tensor index is invalid");
        const auto& entry = result->tensors[index];
        *output = {entry.name.c_str(), entry.tensor.const_data_ptr(), entry.dimensions.data(),
                   static_cast<uint64_t>(entry.tensor.numel()), static_cast<uint32_t>(entry.dimensions.size()),
                   entry.tensor.scalar_type() == at::kFloat ? uint32_t{0} : uint32_t{1}};
        return 0;
    } catch (...) { save_error(); return -1; }
}
int uta_libtorch_result_timings(const UtaLibtorchResult* result, UtaLibtorchTimings* output) noexcept {
    if (!result || !output) return -1;
    *output = result->timings;
    return 0;
}
void uta_libtorch_result_free(UtaLibtorchResult* result) noexcept {
    try { delete result; } catch (...) { save_error(); }
}
} // extern C
