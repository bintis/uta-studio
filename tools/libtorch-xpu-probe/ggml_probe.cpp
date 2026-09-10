// Matched synthetic GGML Vulkan operator probe. No model or runtime installation.
#include "common.hpp"
#include <ggml.h>
#include <ggml-alloc.h>
#include <ggml-backend.h>
#include <cstdlib>

struct Resources {
    ggml_context* context = nullptr;
    ggml_backend_t backend = nullptr;
    ggml_backend_buffer_t buffer = nullptr;
    ~Resources() {
        if (buffer) ggml_backend_buffer_free(buffer);
        if (context) ggml_free(context);
        if (backend) ggml_backend_free(backend);
    }
};
static void upload(ggml_tensor* tensor, const std::vector<float>& values) {
    if (tensor->type == GGML_TYPE_F32) {
        ggml_backend_tensor_set(tensor, values.data(), 0, values.size() * sizeof(float));
    } else {
        std::vector<uint16_t> packed(values.size());
        if (tensor->type == GGML_TYPE_F16) {
            for (size_t index = 0; index < values.size(); ++index) packed[index] = ggml_fp32_to_fp16(values[index]);
        } else {
            for (size_t index = 0; index < values.size(); ++index) {
                uint32_t bits; std::memcpy(&bits, &values[index], sizeof(bits));
                packed[index] = static_cast<uint16_t>(bits >> 16); // Common fixture already rounds to BF16.
            }
        }
        ggml_backend_tensor_set(tensor, packed.data(), 0, packed.size() * sizeof(uint16_t));
    }
}
int main(int argc, char** argv) {
    try {
        std::cout.setf(std::ios::unitbuf);
        if (argc < 3) throw std::invalid_argument("usage: ggml-probe CASE f32|f16|bf16 [warmup=8] [repeats=8]");
        const auto current = probe::shape(argv[1]);
        const std::string precision = argv[2];
        probe::rounded(0, precision);
        const int warmup = argc > 3 ? std::stoi(argv[3]) : 8;
        const int repeats = argc > 4 ? std::stoi(argv[4]) : 8;
        if (warmup < 0 || repeats < 1) throw std::invalid_argument("invalid iteration count");
        const char* directory = std::getenv("UTA_TEST_GGML_RUNTIME_DIR");
        if (!directory) throw std::runtime_error("set UTA_TEST_GGML_RUNTIME_DIR explicitly");
        ggml_backend_load_all_from_path(directory);
        Resources resources;
        for (size_t index = 0; index < ggml_backend_dev_count(); ++index) {
            auto device = ggml_backend_dev_get(index);
            const std::string description = ggml_backend_dev_description(device);
            if (description.find("B580") != std::string::npos) {
                resources.backend = ggml_backend_dev_init(device, nullptr);
                std::cout << "PROBE_ENV {\"backend\":\"ggml_vulkan\",\"device\":" << probe::quote(description)
                          << ",\"library_directory\":" << probe::quote(directory) << "}\n";
                break;
            }
        }
        if (!resources.backend) throw std::runtime_error("B580 unavailable; no CPU fallback");
        resources.context = ggml_init({64 * 1024 * 1024, nullptr, true});
        if (!resources.context) throw std::runtime_error("GGML context allocation failed");
        auto* context = resources.context;
        const auto dtype = precision == "f32" ? GGML_TYPE_F32 : precision == "f16" ? GGML_TYPE_F16 : GGML_TYPE_BF16;
        const auto preparation_start = std::chrono::steady_clock::now();
        auto left_data = probe::data(current.input_count(), 101, precision);
        auto right_data = probe::data(current.attention ? current.input_count() : current.columns * current.depth, 202, precision);
        std::vector<float> value_data;
        ggml_tensor *left_storage, *right_storage, *value_storage = nullptr, *output;
        std::string note;
        if (current.attention) {
            left_storage = ggml_new_tensor_4d(context, GGML_TYPE_F32, current.depth, current.heads, current.rows, current.batch);
            right_storage = ggml_new_tensor_4d(context, dtype, current.depth, current.heads, current.rows, current.batch);
            value_storage = ggml_new_tensor_4d(context, dtype, current.depth, current.heads, current.rows, current.batch);
            auto* query = ggml_permute(context, left_storage, 0, 2, 1, 3);
            auto* key = ggml_permute(context, right_storage, 0, 2, 1, 3);
            auto* value = ggml_permute(context, value_storage, 0, 2, 1, 3);
            output = ggml_flash_attn_ext(context, query, key, value, nullptr, 1.0f / std::sqrt(static_cast<float>(current.depth)), 0.0f, 0.0f);
            ggml_flash_attn_ext_set_prec(output, GGML_PREC_F32);
            value_data = probe::data(current.input_count(), 303, precision);
            note = "GGML fused attention; Q stored F32; K/V in requested storage; F32 output and accumulator; existing internal operand/probability rounding retained";
        } else {
            // BF16 weights use F32 activation storage because the native backend's mixed decoder requires it.
            const auto activation_type = dtype == GGML_TYPE_BF16 ? GGML_TYPE_F32 : dtype;
            left_storage = ggml_new_tensor_3d(context, activation_type, current.depth,
                current.frequency ? current.batch : current.rows, current.frequency ? current.rows : current.batch);
            auto* left = current.frequency ? ggml_permute(context, left_storage, 0, 2, 1, 3) : left_storage;
            const char* requested_layout = std::getenv("UTA_PROBE_GEMM_LAYOUT");
            const std::string layout = requested_layout ? requested_layout : "native";
            if (layout == "packed" || layout == "flat") {
                if (!ggml_is_contiguous(left)) left = ggml_cont(context, left);
                if (layout == "flat") left = ggml_reshape_2d(context, left, current.depth, current.rows * current.batch);
            } else if (layout != "native") {
                throw std::invalid_argument("GGML GEMM layout must be native, packed, or flat");
            }
            right_storage = ggml_new_tensor_2d(context, dtype, current.depth, current.columns);
            output = ggml_mul_mat(context, right_storage, left);
            ggml_mul_mat_set_prec(output, GGML_PREC_F32);
            note = "shared weights, matched physical input layout; graph dispatch and synchronization included; F32 output; ";
            note += dtype == GGML_TYPE_BF16 ? "BF16 weights with BF16-rounded values stored in F32 activations" : "both operands in requested storage";
            note += "; GEMM layout=" + layout + "; optional packing is included in timed graph";
        }
        if (!ggml_backend_supports_op(resources.backend, output)) throw std::runtime_error("selected GGML backend does not support the requested operator");
        ggml_set_name(output, "probe_output");
        auto* graph = ggml_new_graph_custom(context, 128, false);
        ggml_build_forward_expand(graph, output);
        resources.buffer = ggml_backend_alloc_ctx_tensors(context, resources.backend);
        if (!resources.buffer) throw std::runtime_error("GGML device buffer allocation failed");
        upload(left_storage, left_data); upload(right_storage, right_data);
        if (value_storage) upload(value_storage, value_data);
        ggml_backend_synchronize(resources.backend);
        const double preparation = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - preparation_start).count();
        std::cout << "PROBE_READY " << probe::quote(current.name + ":" + precision) << '\n';
        const auto timing = probe::measure([&] {
            if (ggml_backend_graph_compute(resources.backend, graph) != GGML_STATUS_SUCCESS) throw std::runtime_error("GGML graph compute failed");
            ggml_backend_synchronize(resources.backend);
        }, warmup, repeats);
        std::vector<float> host(current.output_count());
        ggml_backend_tensor_get(output, host.data(), 0, host.size() * sizeof(float));
        const auto accuracy = probe::validate(current, precision, host.data());
        probe::report("ggml_vulkan", current, precision, timing, accuracy, note, preparation);
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "PROBE_ERROR " << probe::quote(error.what()) << '\n';
        return 2;
    }
}
