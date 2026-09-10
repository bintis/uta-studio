// Independent synthetic operators only. No GGML, model, Python interpreter, or user data.
#include "common.hpp"
#include <ATen/ATen.h>
#include <ATen/Context.h>
#include <ATen/Parallel.h>
#include <ATen/ops/_fused_sdp_choice.h>
#include <c10/core/Event.h>
#include <c10/core/InferenceMode.h>
#include <c10/core/impl/VirtualGuardImpl.h>
#include <torch/version.h>
#include <torch/xpu.h>

static double mean(const std::vector<double>& values) {
    return std::accumulate(values.begin(), values.end(), 0.0) / values.size();
}
static double deviation(const std::vector<double>& values) {
    double total = 0;
    for (double value : values) total += (value - mean(values)) * (value - mean(values));
    return values.size() > 1 ? std::sqrt(total / (values.size() - 1)) : 0;
}
int main(int argc, char** argv) {
    try {
        std::cout.setf(std::ios::unitbuf);
        std::cout << std::setprecision(12);
        if (argc < 5) throw std::invalid_argument("usage: benchmark CASE f32|f16|bf16 SIZE interleaved|contiguous|bridge|mixed [warmup=8] [repeats=12] [inner=1]");
        const std::string name = argv[1], precision = argv[2], layout = argv[4];
        const int64_t size = std::stoll(argv[3]);
        const int warmup = argc > 5 ? std::stoi(argv[5]) : 8;
        const int repeats = argc > 6 ? std::stoi(argv[6]) : 12;
        const int inner = argc > 7 ? std::stoi(argv[7]) : 1;
        if (warmup < 0 || repeats < 1 || inner < 1 || size < 1) throw std::invalid_argument("invalid dimensions/iterations");
        if (layout != "interleaved" && layout != "contiguous" && layout != "bridge" && layout != "mixed") throw std::invalid_argument("unknown layout");
        probe::rounded(0.0f, precision);
        const bool attention = name.find("attention") != std::string::npos;
        const bool elementwise = name == "copy" || name == "exp" || name == "vector" || name == "softmax";
        if (!attention && !elementwise && name != "gemm") throw std::invalid_argument("unknown case");
        const bool bridge = layout == "bridge", mixed = layout == "mixed";
        probe::Shape current = attention ? probe::shape(name) : probe::Shape{name, false, false, 1, size, size, size, 1};
        const auto dtype = precision == "f32" ? at::kFloat : precision == "f16" ? at::kHalf : at::kBFloat16;
        at::set_num_threads(2);
        c10::InferenceMode inference;
        auto& context = at::globalContext();
        context.setFloat32Precision(at::Float32Backend::GENERIC, at::Float32Op::ALL, at::Float32Precision::IEEE);
        context.setFloat32Precision(at::Float32Backend::MKLDNN, at::Float32Op::MATMUL, at::Float32Precision::IEEE);
        context.setSDPUseMath(false);
        if (!torch::xpu::is_available()) throw std::runtime_error("XPU unavailable; no CPU fallback");
        const auto device = at::Device(at::kXPU, 0);
        c10::impl::VirtualGuardImpl guard(at::kXPU);
        const auto stream = guard.getStream(device);
        const auto options = at::TensorOptions().dtype(dtype).device(device);
        std::cout << "CAPACITY_ENV {\"version\":" << probe::quote(TORCH_VERSION)
            << ",\"device_count\":" << torch::xpu::device_count()
            << ",\"device\":\"xpu:0\",\"math_fallback\":false,\"fp32_precision\":\"ieee\"}\n";
        const auto preparation_begin = std::chrono::steady_clock::now();
        at::Tensor left, right, values, output;
        double flops = elementwise ? (name == "vector" ? 2.0 * size : 0.0) : current.flops();
        double bytes = 0;
        int64_t choice = -1;
        auto upload = [&](int64_t count, uint64_t salt, const std::string& data_precision, at::ScalarType data_type) {
            auto host = probe::data(count, salt, data_precision);
            return at::from_blob(host.data(), {count}, at::kFloat).to(at::TensorOptions().device(device).dtype(data_type), false, true);
        };
        if (elementwise) {
            left = upload(size, 101, precision, dtype);
            right = at::full_like(left, 0.25);
            values = at::full_like(left, 0.5);
            if (name == "softmax") {
                if (size % 1722) throw std::invalid_argument("softmax element count must be divisible by 1722");
                left = left.reshape({size / 1722, 1722});
            }
            output = at::empty_like(left);
            bytes = static_cast<double>(size) * left.element_size() * (name == "vector" ? 4 : 2);
        } else {
            left = upload(current.input_count(), 101, bridge || mixed ? "f32" : precision, bridge || mixed ? at::kFloat : dtype);
            right = upload(attention ? current.input_count() : size * size, 202, precision, dtype);
            if (attention) {
                values = upload(current.input_count(), 303, precision, dtype);
                std::vector<int64_t> shape{current.batch, current.rows, current.heads, current.depth};
                left = left.reshape(shape).permute({0, 2, 1, 3});
                right = right.reshape(shape).permute({0, 2, 1, 3});
                values = values.reshape(shape).permute({0, 2, 1, 3});
                if (layout == "contiguous") { left = left.contiguous(); right = right.contiguous(); values = values.contiguous(); }
                choice = at::_fused_sdp_choice(bridge ? left.to(dtype) : left, right, values, {}, 0.0, false);
            } else {
                left = left.reshape({size, size});
                right = right.reshape({size, size}).transpose(0, 1);
                output = at::empty({size, size}, options);
            }
        }
        auto compute = [&] {
            if (attention) {
                output = at::scaled_dot_product_attention(bridge ? left.to(dtype) : left, right, values, {}, 0.0, false);
                if (bridge) output = output.permute({0, 2, 1, 3}).contiguous().to(at::kFloat);
            } else if (name == "gemm") at::mm_out(output, left, right);
            else if (name == "copy") output.copy_(left);
            else if (name == "exp") at::exp_out(output, left);
            else if (name == "vector") at::addcmul_out(output, left, right, values, 1.0);
            else output = at::softmax(left, -1);
        };
        torch::xpu::synchronize(0);
        const double preparation = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - preparation_begin).count();
        std::cout << "CAPACITY_READY {\"case\":" << probe::quote(name) << ",\"precision\":" << probe::quote(precision)
            << ",\"layout\":" << probe::quote(layout) << ",\"size\":" << size << ",\"sdpa_choice\":" << choice << "}\n";
        for (int index = 0; index < warmup; ++index) { compute(); torch::xpu::synchronize(0); }
        std::vector<double> host_samples, device_samples;
        for (int iteration = 0; iteration < repeats; ++iteration) {
            c10::Event begin(at::kXPU, c10::EventFlag::BACKEND_DEFAULT), end(at::kXPU, c10::EventFlag::BACKEND_DEFAULT);
            torch::xpu::synchronize(0);
            const auto started = std::chrono::steady_clock::now();
            begin.record(stream);
            for (int index = 0; index < inner; ++index) compute();
            end.record(stream);
            end.synchronize();
            host_samples.push_back(std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - started).count() / inner);
            device_samples.push_back(begin.elapsedTime(end) / inner);
        }
        const bool finite = at::isfinite(output).all().item<bool>();
        double nmse = 0, original_nmse = 0, max_abs = 0;
        int reference_samples = 0;
        bool numeric = finite;
        if (!elementwise) {
            auto host = (attention && !bridge ? output.permute({0, 2, 1, 3}) : output).contiguous().to(at::kFloat).to(at::kCPU);
            const auto accuracy = probe::validate(current, precision, host.const_data_ptr<float>());
            nmse = accuracy.nmse; original_nmse = accuracy.original_nmse; max_abs = accuracy.max_abs; reference_samples = accuracy.samples;
            numeric = finite && std::isfinite(nmse) && nmse < (precision == "f32" && !attention ? 1e-10 : 5e-4);
        } else {
            if (name == "copy") numeric = numeric && at::equal(output, left);
            else if (name == "softmax") {
                max_abs = (output.to(at::kFloat).sum(-1) - 1).abs().max().item<double>();
                numeric = numeric && max_abs < 0.01;
            } else {
                auto host = output.flatten().to(at::kFloat).to(at::kCPU);
                for (int index = 0; index < 128; ++index) {
                    int64_t position = (index * (size - 1)) / 127;
                    double input = probe::rounded(probe::value(position, 101), precision);
                    double expected = name == "exp" ? std::exp(input) : input + 0.125;
                    max_abs = std::max(max_abs, std::abs(host.const_data_ptr<float>()[position] - expected));
                }
                reference_samples = 128;
                numeric = numeric && max_abs < (precision == "f32" ? 1e-5 : 0.02);
            }
        }
        std::cout << "CAPACITY_RESULT {\"case\":" << probe::quote(name) << ",\"precision\":" << probe::quote(precision)
            << ",\"layout\":" << probe::quote(layout) << ",\"size\":" << size << ",\"warmup\":" << warmup
            << ",\"repeats\":" << repeats << ",\"inner\":" << inner << ",\"sdpa_choice\":" << choice
            << ",\"flops\":" << flops << ",\"algorithmic_bytes\":" << bytes
            << ",\"event_mean_ms\":" << mean(device_samples) << ",\"event_sd_ms\":" << deviation(device_samples)
            << ",\"host_mean_ms\":" << mean(host_samples) << ",\"effective_tflops\":" << flops / (mean(device_samples) * 1e9)
            << ",\"effective_gbs\":" << bytes / (mean(device_samples) * 1e6)
            << ",\"prepare_ms\":" << preparation << ",\"finite\":" << (finite ? "true" : "false")
            << ",\"nmse\":" << nmse << ",\"original_float_nmse\":" << original_nmse << ",\"max_abs\":" << max_abs
            << ",\"reference_samples\":" << reference_samples << ",\"numeric_pass\":" << (numeric ? "true" : "false")
            << ",\"event_ms\":"; probe::array(device_samples);
        std::cout << ",\"host_ms\":"; probe::array(host_samples);
        std::cout << ",\"timing_scope\":\"XPU event interval includes device work and queue gaps; host synchronization reported separately; transfers and validation excluded\"}\n";
        return numeric ? 0 : 3;
    } catch (const std::exception& error) {
        std::cerr << "CAPACITY_ERROR " << probe::quote(error.what()) << '\n';
        return 2;
    }
}
