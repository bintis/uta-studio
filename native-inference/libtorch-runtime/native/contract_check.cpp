// ABI/container fixture checks. CPU is the default; an explicit backend/device
// may be supplied to exercise the same synthetic whole-plan fixture on an accelerator.
#include "api.h"
#include <algorithm>
#include <cmath>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <numeric>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
void require(bool condition, const std::string& message) {
    if (!condition) throw std::runtime_error(message + ": " + uta_libtorch_last_error());
}
template<typename Value> void write_scalar(std::ostream& output, Value value) {
    output.write(reinterpret_cast<const char*>(&value), sizeof(value));
}
void write_string(std::ostream& output, const std::string& value) {
    write_scalar<uint64_t>(output, value.size());
    output.write(value.data(), static_cast<std::streamsize>(value.size()));
}
struct Tensor {
    std::string name;
    std::vector<int64_t> shape;
    std::vector<float> values;
    std::vector<uint16_t> half_values;
    uint64_t offset = 0;
};
void make_fixture(const std::filesystem::path& path) {
    std::vector<Tensor> tensors;
    auto add = [&](std::string name, std::vector<int64_t> shape, float value = 0.0f) {
        const auto count = std::accumulate(shape.begin(), shape.end(), int64_t{1}, std::multiplies<>());
        tensors.push_back({std::move(name), std::move(shape), std::vector<float>(count, value), {}, 0});
    };
    add("input_stack.0.weight", {4, 128, 3}); add("input_stack.0.bias", {4});
    add("input_stack.1.weight", {4, 4, 3}); add("input_stack.1.bias", {4});
    add("mel_scale", {4}, 1.0f); add("mel_bias", {4});
    add("norm.weight", {4}, 1.0f); add("norm.bias", {4});
    add("output_proj.weight", {4, 3}); add("output_proj.bias", {3});
    tensors.back().values.clear();
    tensors.back().half_values = {0xc000, 0x0000, 0x4000}; // FP16 -2,0,+2
    add("cents_mapping", {3});
    for (int64_t layer = 0; layer < 6; ++layer) {
        const auto prefix = "encoder_layers." + std::to_string(layer) + '.';
        add(prefix + "norm.weight", {4}, 1.0f); add(prefix + "norm.bias", {4});
        add(prefix + "fc1.weight", {8, 4, 1}); add(prefix + "fc1.bias", {8});
        add(prefix + "conv.weight", {4, 1, 3}); add(prefix + "conv.bias", {4});
        add(prefix + "fc2.weight", {4, 4, 1}); add(prefix + "fc2.bias", {4});
    }
    uint64_t offset = 0;
    for (auto& tensor : tensors) {
        tensor.offset = offset;
        offset += tensor.half_values.empty() ? tensor.values.size() * sizeof(float) : tensor.half_values.size() * sizeof(uint16_t);
        offset = (offset + 31) & ~uint64_t{31};
    }
    std::ofstream output(path, std::ios::binary);
    require(static_cast<bool>(output), "create isolated GGUF fixture");
    write_scalar<uint32_t>(output, 0x46554747);
    write_scalar<uint32_t>(output, 3);
    write_scalar<uint64_t>(output, tensors.size());
    write_scalar<uint64_t>(output, 2);
    write_string(output, "general.architecture"); write_scalar<uint32_t>(output, 8); write_string(output, "fcpe");
    write_string(output, "general.alignment"); write_scalar<uint32_t>(output, 4); write_scalar<uint32_t>(output, 32);
    for (const auto& tensor : tensors) {
        write_string(output, tensor.name);
        write_scalar<uint32_t>(output, tensor.shape.size());
        for (auto axis = tensor.shape.rbegin(); axis != tensor.shape.rend(); ++axis) write_scalar<uint64_t>(output, *axis);
        write_scalar<uint32_t>(output, tensor.half_values.empty() ? 0 : 1);
        write_scalar<uint64_t>(output, tensor.offset);
    }
    const auto directory = static_cast<uint64_t>(output.tellp());
    const auto data_begin = (directory + 31) & ~uint64_t{31};
    auto pad_to = [&](uint64_t position) {
        while (static_cast<uint64_t>(output.tellp()) < position) output.put('\0');
    };
    pad_to(data_begin);
    for (const auto& tensor : tensors) {
        pad_to(data_begin + tensor.offset);
        if (tensor.half_values.empty()) output.write(reinterpret_cast<const char*>(tensor.values.data()), tensor.values.size() * sizeof(float));
        else output.write(reinterpret_cast<const char*>(tensor.half_values.data()), tensor.half_values.size() * sizeof(uint16_t));
    }
    output.flush();
    require(static_cast<bool>(output), "write isolated GGUF fixture");
}
}

int main(int argc, char** argv) {
    try {
        if (argc < 2 || argc > 5) throw std::invalid_argument("usage: uta-libtorch-contract-check NEW_FIXTURE_DIRECTORY [backend [device [precision]]]");
        const std::string backend = argc >= 3 ? argv[2] : "libtorch_cpu";
        const int device = argc >= 4 ? std::stoi(argv[3]) : 0;
        const std::string precision = argc >= 5 ? argv[4] : "strict";
        const std::filesystem::path root(argv[1]);
        require(std::filesystem::create_directories(root), "fixture directory must be newly created, not user input");
        const auto fixture = root / "fcpe-native-contract.gguf";
        make_fixture(fixture);
        require(uta_libtorch_tensor_layout_size() == sizeof(UtaLibtorchTensor), "C ABI tensor layout");
        const auto* info = uta_libtorch_build_info();
        require(info && std::strstr(info, "fcpe"), "read-only native capability description");
        require(!uta_libtorch_runtime_create("unknown", 0, "strict"), "unknown backend rejection");
        require(!uta_libtorch_runtime_create("libtorch_cpu", 0, "unknown"), "unknown precision rejection");
        auto* runtime = uta_libtorch_runtime_create(backend.c_str(), device, precision.c_str());
        require(runtime != nullptr, "explicit selected runtime creation");
        auto* model = uta_libtorch_model_open(runtime, "fcpe", fixture.c_str());
        require(model != nullptr, "bounded GGUF F32/F16 tensor loading");
        require(std::strstr(uta_libtorch_model_metadata(model), "general.architecture"), "immutable metadata ownership");
        uta_libtorch_runtime_free(runtime); // model retains shared native runtime
        std::vector<float> mel(5 * 128, 0.25f);
        const int64_t shape[]{5, 128};
        UtaLibtorchTensor input{"mel", mel.data(), shape, mel.size(), 2, 0};
        uta_libtorch_model_cancel(model, 1);
        require(!uta_libtorch_model_forward(model, "forward", &input, 1), "cancel before learned computation");
        require(std::strstr(uta_libtorch_last_error(), "cancelled"), "cancellation error is actionable");
        uta_libtorch_model_cancel(model, 0);
        auto* result = uta_libtorch_model_forward(model, "forward", &input, 1);
        require(result != nullptr, "complete native FCPE fixture execution");
        require(uta_libtorch_result_count(result) == 1, "native output count");
        UtaLibtorchTensor output{};
        require(uta_libtorch_result_tensor(result, 0, &output) == 0, "borrowed output view");
        require(output.rank == 2 && output.dimensions[0] == 5 && output.dimensions[1] == 3 && output.elements == 15 && output.kind == 0,
                "row-major output shape and dtype");
        const auto* values = static_cast<const float*>(output.data);
        for (uint64_t index = 0; index < output.elements; ++index) {
            const double bias = static_cast<double>(static_cast<int>(index % 3) * 2 - 2);
            const double expected = 1.0 / (1.0 + std::exp(-bias));
            require(std::isfinite(values[index]) && std::abs(values[index] - expected) < 1e-6, "complete activation vector including FP16 stored bias");
        }
        UtaLibtorchTimings timings{};
        require(uta_libtorch_result_timings(result, &timings) == 0 && timings.synchronized_compute_seconds > 0, "synchronized native timing boundaries");
        require(uta_libtorch_result_tensor(result, 1, &output) != 0, "out-of-range result is an error");
        uta_libtorch_result_free(result);
        auto invalid = input;
        invalid.elements -= 1;
        require(!uta_libtorch_model_forward(model, "forward", &invalid, 1), "input buffer and dimensions mismatch");
        require(!uta_libtorch_model_forward(model, "missing_operation", &input, 1), "unknown operation is not empty success");
        const UtaLibtorchTensor duplicate[]{input, input};
        require(!uta_libtorch_model_forward(model, "forward", duplicate, 2), "duplicate named inputs are not overwritten");
        uta_libtorch_model_free(model);
        std::cout << "{\"scope\":\"native_abi_and_synthetic_fcpe_whole_plan\",\"status\":\"passed\",\"backend\":\""
                  << backend << "\",\"device\":" << device << ",\"precision\":\"" << precision
                  << "\",\"compared_elements\":15,\"gpu_initialized\":" << (backend == "libtorch_cpu" ? "false" : "true") << "}\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "native contract check failed: " << error.what() << '\n';
        return 1;
    }
}
