// Standalone synthetic LibTorch XPU operator benchmark. No model loading.
#include "common.hpp"
#include <ATen/ATen.h>
#include <ATen/Context.h>
#include <ATen/Parallel.h>
#include <ATen/ops/_fused_sdp_choice.h>
#include <c10/core/InferenceMode.h>
#include <torch/version.h>
#include <torch/xpu.h>

int main(int argc, char** argv) {
    try {
        std::cout.setf(std::ios::unitbuf);
        if (argc < 3) throw std::invalid_argument("usage: torch-probe CASE f32|f16|bf16 [warmup=8] [repeats=8]");
        const auto current = probe::shape(argv[1]);
        const std::string precision = argv[2];
        const int warmup = argc > 3 ? std::stoi(argv[3]) : 8;
        const int repeats = argc > 4 ? std::stoi(argv[4]) : 8;
        if (warmup < 0 || repeats < 1) throw std::invalid_argument("invalid iteration count");
        const auto dtype = precision == "f32" ? at::kFloat : precision == "f16" ? at::kHalf : at::kBFloat16;
        probe::rounded(0.0f, precision);
        at::set_num_threads(2);
        c10::InferenceMode inference;
        auto& context = at::globalContext();
        context.setFloat32Precision(at::Float32Backend::GENERIC, at::Float32Op::ALL, at::Float32Precision::IEEE);
        context.setFloat32Precision(at::Float32Backend::MKLDNN, at::Float32Op::MATMUL, at::Float32Precision::IEEE);
        context.setSDPUseMath(false); // Never materialize a multi-gigabyte fallback score tensor silently.
        if (!torch::xpu::is_available()) throw std::runtime_error("LibTorch XPU unavailable; no CPU fallback");
        const auto device = at::Device(at::kXPU, 0);
        std::cout << "PROBE_ENV {\"torch_version\":" << probe::quote(TORCH_VERSION)
                  << ",\"xpu_devices\":" << torch::xpu::device_count()
                  << ",\"device\":\"xpu:0\",\"onednn_tf32_allowed\":" << (context.allowTF32OneDNN() ? "true" : "false")
                  << ",\"math_sdpa_enabled\":false,\"output_dtype\":\"float32\"}\n";
        const auto preparation_start = std::chrono::steady_clock::now();
        auto left_data = probe::data(current.input_count(), 101, precision);
        auto right_data = probe::data(current.attention ? current.input_count() : current.columns * current.depth, 202, precision);
        std::vector<float> value_data;
        auto upload = [&](std::vector<float>& values) {
            return at::from_blob(values.data(), {static_cast<int64_t>(values.size())}, at::kFloat)
                .to(at::TensorOptions().device(device).dtype(dtype), false, true);
        };
        auto left_storage = upload(left_data);
        auto right_storage = upload(right_data);
        at::Tensor left, right, values, output;
        int64_t sdpa_choice = -1;
        std::string note;
        if (current.attention) {
            value_data = probe::data(current.input_count(), 303, precision);
            auto value_storage = upload(value_data);
            const std::vector<int64_t> base_shape{current.batch, current.rows, current.heads, current.depth};
            left = left_storage.reshape(base_shape).permute({0, 2, 1, 3});
            right = right_storage.reshape(base_shape).permute({0, 2, 1, 3});
            values = value_storage.reshape(base_shape).permute({0, 2, 1, 3});
            sdpa_choice = at::_fused_sdp_choice(left, right, values, {}, 0.0, false);
            note = "fused SDPA choice=" + std::to_string(sdpa_choice) + "; input layout [batch,row,head,depth] viewed as BHND; output cast to float32 included; math fallback disabled";
        } else {
            const std::vector<int64_t> dimensions{current.batch, current.rows, current.depth};
            const std::vector<int64_t> strides = current.frequency
                ? std::vector<int64_t>{current.depth, current.batch * current.depth, 1}
                : std::vector<int64_t>{current.rows * current.depth, current.depth, 1};
            left = left_storage.as_strided(dimensions, strides);
            right = right_storage.reshape({current.columns, current.depth}).transpose(0, 1);
            note = "shared weight matmul; native broadcasting/flattening/reordering allowed and included; output cast to float32 included; ";
            note += current.frequency ? "frequency-transposed input view" : "contiguous time input";
        }
        torch::xpu::synchronize(0);
        const double preparation = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - preparation_start).count();
        std::cout << "PROBE_READY " << probe::quote(current.name + ":" + precision) << " sdpa_choice=" << sdpa_choice << '\n';
        const auto timing = probe::measure([&] {
            output = current.attention
                ? at::scaled_dot_product_attention(left, right, values, {}, 0.0, false).to(at::kFloat)
                : at::matmul(left, right).to(at::kFloat);
            torch::xpu::synchronize(0);
        }, warmup, repeats);
        auto host = (current.attention ? output.permute({0, 2, 1, 3}) : output).contiguous().to(at::kCPU);
        const auto accuracy = probe::validate(current, precision, host.const_data_ptr<float>());
        probe::report("libtorch_xpu", current, precision, timing, accuracy, note, preparation);
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "PROBE_ERROR " << probe::quote(error.what()) << '\n';
        return 2;
    }
}
