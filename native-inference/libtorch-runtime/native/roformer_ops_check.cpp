// Explicit diagnostic device; no model, fallback, timing gate or installation.
#include "roformer_ops.hpp"
#include "roformer_projection.hpp"
#include <ATen/Context.h>
#include <ATen/Parallel.h>
#include <c10/core/InferenceMode.h>
#include <c10/core/DeviceGuard.h>
#include <c10/core/impl/VirtualGuardImpl.h>
#include <chrono>
#include <cmath>
#include <iostream>
#include <stdexcept>
#include <string>

namespace {
using Clock = std::chrono::steady_clock;
void synchronize(const at::Device& device) {
    if (!device.is_cpu()) c10::impl::VirtualGuardImpl(device.type()).synchronizeDevice(device.index());
}
at::Tensor decomposed(const at::Tensor& input, const at::Tensor& cosine, const at::Tensor& sine) {
    auto shape = input.sizes().vec();
    shape.back() /= 2;
    shape.push_back(2);
    auto paired = input.reshape(shape);
    auto even = paired.select(-1, 0), odd = paired.select(-1, 1);
    return at::stack({even * cosine - odd * sine, even * sine + odd * cosine}, -1).flatten(-2);
}
void compare(const at::Tensor& actual, const at::Tensor& expected, const char* name,
             double absolute_tolerance = 2e-6, double nmse_tolerance = 1e-12) {
    auto value = actual.to(at::kCPU).to(at::kDouble);
    auto reference = expected.to(at::kCPU).to(at::kDouble);
    if (value.sizes() != reference.sizes() || !at::isfinite(value).all().item<bool>())
        throw std::runtime_error(std::string(name) + " shape/finite failure");
    auto difference = value - reference;
    const auto maximum = difference.abs().max().item<double>();
    const auto nmse = difference.square().sum().item<double>() / std::max(reference.square().sum().item<double>(), 1e-30);
    std::cout << "comparison=" << name << " elements=" << value.numel()
              << " max_abs=" << maximum << " nmse=" << nmse << std::endl;
    if (maximum > absolute_tolerance || nmse > nmse_tolerance) throw std::runtime_error(std::string(name) + " numerical mismatch");
}
void run(const at::Device& device, int64_t batch, int64_t length, int64_t width, bool packed, bool timing) {
    const int64_t heads = 8;
    auto options = at::TensorOptions().device(device).dtype(at::kFloat);
    auto storage = (at::arange(batch * length * heads * width * 3, options) * 0.013).sin()
        .reshape({batch, length, heads * width * 3});
    auto input = storage.narrow(-1, heads * width, heads * width)
        .reshape({batch, length, heads, width}).transpose(1, 2);
    if (packed) input = input.contiguous();
    auto original = input.clone();
    auto angle = at::arange(length, options).reshape({1, 1, length, 1})
        * at::exp(at::arange(0, width, 2, options) * (-std::log(10000.0) / width));
    auto cosine = angle.cos(), sine = angle.sin();
    auto phase = at::complex(cosine, sine);
    auto actual = uta::torch_native::interleaved_roformer_rotation(input, phase);
    std::cout << "shape=" << batch << ',' << heads << ',' << length << ',' << width
              << " packed=" << packed << std::endl;
    compare(actual, decomposed(input, cosine, sine), "decomposed-float");
    // Complete FP64 arithmetic over exactly the same FP32 phase/input values.
    compare(actual, decomposed(input.to(at::kCPU).to(at::kDouble),
        cosine.to(at::kCPU).to(at::kDouble), sine.to(at::kCPU).to(at::kDouble)), "double-oracle");
    if (!at::equal(input, original)) throw std::runtime_error("rotation modified its input");
    if (!timing) return;
    for (const auto& kind : {std::string("decomposed"), std::string("complex"), std::string("complex") , std::string("decomposed")}) {
        auto invoke = [&] { return kind == "complex"
            ? uta::torch_native::interleaved_roformer_rotation(input, phase)
            : decomposed(input, cosine, sine); };
        for (int warm = 0; warm < 2; ++warm) { actual = invoke(); synchronize(device); }
        for (int sample = 0; sample < 4; ++sample) {
            synchronize(device);
            const auto started = Clock::now();
            actual = invoke();
            synchronize(device);
            const auto elapsed = std::chrono::duration<double, std::milli>(Clock::now() - started).count();
            std::cout << "rotation=" << kind << " sample=" << sample << " synchronized_ms=" << elapsed << std::endl;
        }
    }
}
void normalization(const at::Device& device, int64_t rows, int64_t width, bool strided, double amplitude) {
    auto options = at::TensorOptions().device(device).dtype(at::kFloat);
    auto storage = (at::arange(rows * width * 2, options) * 0.013).sin().reshape({rows, width * 2}) * amplitude;
    auto input = storage.narrow(-1, width, width);
    if (!strided) input = input.contiguous();
    auto original = input.clone();
    auto weight = (at::arange(width, options) * 0.11).cos();
    auto actual = uta::torch_native::fused_roformer_normalization(input, weight);
    auto reference = input.to(at::kCPU).to(at::kDouble);
    reference = reference * at::rsqrt(reference.square().mean(-1, true) + 1e-12) * weight.to(at::kCPU).to(at::kDouble);
    std::cout << "normalization_shape=" << rows << ',' << width << " strided=" << strided << " amplitude=" << amplitude << std::endl;
    compare(actual, reference, "normalization-double-oracle");
    if (!at::equal(input, original)) throw std::runtime_error("normalization modified its input");
}
void projection(const at::Device& device, int64_t contraction, int64_t columns, bool strided) {
    auto options = at::TensorOptions().device(device).dtype(at::kFloat);
    auto storage = (at::arange(34 * contraction * 2, options) * 0.017).sin().reshape({2, 17, contraction * 2}) * 0.2;
    auto input = storage.narrow(-1, contraction, contraction);
    if (!strided) input = input.contiguous();
    auto original = input.clone();
    auto weight = (at::arange(columns * contraction, options) * 0.013).cos().reshape({columns, contraction}) * 0.1;
    auto bias = (at::arange(columns, options) * 0.11).sin() * 0.01;
    auto reference = at::linear(input.to(at::kCPU).to(at::kDouble), weight.to(at::kCPU).to(at::kDouble), bias.to(at::kCPU).to(at::kDouble));
    auto& context = at::globalContext();
    for (const auto initial : {false, true}) {
        context.setAllowTF32OneDNN(initial);
        auto actual = uta::torch_native::diagnostic_roformer_projection(input, weight, bias);
        synchronize(device);
        std::cout << "projection_shape=34," << contraction << ',' << columns << " strided=" << strided
                  << " initial_reduced=" << initial << std::endl;
        compare(actual, reference, "reduced-projection-double-oracle", 2e-3, 1e-5);
        if (context.allowTF32OneDNN() != initial) throw std::runtime_error("projection did not restore its math flag");
        bool caught = false;
        try { uta::torch_native::diagnostic_roformer_projection(input, weight.narrow(-1, 0, contraction - 1), bias); }
        catch (const c10::Error&) { caught = true; }
        if (!caught || context.allowTF32OneDNN() != initial) throw std::runtime_error("projection exception did not restore its math flag");
    }
    context.setAllowTF32OneDNN(false);
    if (!at::equal(input, original)) throw std::runtime_error("projection modified its input");
}
void attention(const at::Device& device, int64_t batch, int64_t length, bool timing) {
    const int64_t heads = 8, width = 64;
    auto options = at::TensorOptions().device(device).dtype(at::kFloat);
    auto storage = (at::arange(batch * length * heads * width * 3, options) * 0.017).sin()
        .reshape({batch, length, heads * width * 3});
    auto pieces = storage.chunk(3, -1);
    auto query = pieces[0].reshape({batch, length, heads, width}).transpose(1, 2);
    auto key = pieces[1].reshape({batch, length, heads, width}).transpose(1, 2);
    auto value = pieces[2].reshape({batch, length, heads, width}).transpose(1, 2);
    const double scale = 0.125;
    auto packed = [&] {
        return at::scaled_dot_product_attention(query.to(at::kHalf).contiguous(), key.to(at::kHalf).contiguous(),
            value.to(at::kHalf).contiguous(), {}, 0.0, false, scale).to(at::kFloat);
    };
    auto interleaved = [&] { return uta::torch_native::layout_preserving_roformer_attention(query, key, value, scale); };
    auto actual = interleaved();
    std::cout << "attention_shape=" << batch << ',' << heads << ',' << length << ',' << width << std::endl;
    compare(actual, packed(), "attention-packed", 2e-3, 2e-6);
    if (!timing) {
        auto rounded = [](const at::Tensor& input) { return input.to(at::kHalf).to(at::kCPU).to(at::kDouble); };
        auto reference = at::matmul(at::softmax(at::matmul(rounded(query), rounded(key).transpose(-1, -2)) * scale, -1), rounded(value));
        compare(actual, reference, "attention-double-oracle", 2e-3, 2e-6);
    }
    if (!timing) return;
    for (const auto& kind : {std::string("packed"), std::string("interleaved"), std::string("interleaved"), std::string("packed")}) {
        auto invoke = [&] { return kind == "packed" ? packed() : interleaved(); };
        for (int warm = 0; warm < 2; ++warm) { actual = invoke(); synchronize(device); }
        for (int sample = 0; sample < 4; ++sample) {
            synchronize(device);
            const auto started = Clock::now();
            actual = invoke();
            synchronize(device);
            std::cout << "attention=" << kind << " sample=" << sample << " synchronized_ms="
                      << std::chrono::duration<double, std::milli>(Clock::now() - started).count() << std::endl;
        }
    }
}
}
int main(int argc, char** argv) {
    try {
        c10::InferenceMode inference;
        at::set_num_threads(2);
        const at::Device device(argc > 1 ? argv[1] : "cpu");
        c10::DeviceGuard guard(device);
        at::globalContext().setFloat32Precision(at::Float32Backend::GENERIC, at::Float32Op::ALL, at::Float32Precision::IEEE);
        if (device.is_xpu() && argc > 2 && std::string(argv[2]) == "projection") {
            for (const auto strided : {false, true}) {
                projection(device, 256, 1536, strided);
                projection(device, 256, 1024, strided);
                projection(device, 1024, 256, strided);
            }
            std::cout << "Diagnostic RoFormer projection checks passed on " << device << std::endl;
            return 0;
        }
        const bool full = argc > 2 && std::string(argv[2]) == "full";
        for (const auto packed : {false, true}) {
            run(device, 3, 17, 64, packed, false);
            run(device, 17, 3, 64, packed, false);
            run(device, 1, 1, 2, packed, false);
            run(device, 2, 65, 128, packed, false);
        }
        for (const auto width : {8, 16, 256, 384, 516})
            for (const auto amplitude : {0.0, 1e-9, 1.0, 1e12})
                for (const auto strided : {false, true}) normalization(device, 17, width, strided, amplitude);
        if (full) {
            run(device, 90, 1722, 64, false, true);
            run(device, 1722, 90, 64, false, true);
            normalization(device, 90 * 1722, 256, false, 1.0);
            normalization(device, 60 * 801, 384, false, 1.0);
        }
        if (device.is_xpu()) {
            at::globalContext().setSDPUseMath(false);
            attention(device, 3, 17, false);
            attention(device, 1, 1, false);
            if (full) { attention(device, 90, 1722, true); attention(device, 1722, 90, true); }
        }
        synchronize(device);
        std::cout << "RoFormer primitive checks passed on " << device << std::endl;
        return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << std::endl; return 1; }
}
