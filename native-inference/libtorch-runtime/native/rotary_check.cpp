// CPU-only exact comparison; never creates an accelerator runtime or device.
#include "rotary.hpp"
#include <c10/core/InferenceMode.h>
#include <iostream>

namespace {
at::Tensor reference(const at::Tensor& input, const at::Tensor& positions, double base, bool split) {
    const auto dimensions = input.size(-1);
    auto frequency = at::exp(at::arange(0, dimensions, 2, input.options().dtype(at::kFloat)) * (-std::log(base) / dimensions));
    auto phase = positions.to(at::kFloat).unsqueeze(-1) * frequency;
    while (phase.dim() < input.dim()) phase = phase.unsqueeze(0);
    if (split) {
        auto halves = input.chunk(2, -1);
        return at::cat({halves[0] * phase.cos() - halves[1] * phase.sin(), halves[1] * phase.cos() + halves[0] * phase.sin()}, -1);
    }
    auto paired = input.reshape({input.size(0), input.size(1), input.size(2), dimensions / 2, 2});
    auto even = paired.select(-1, 0), odd = paired.select(-1, 1);
    return at::stack({even * phase.cos() - odd * phase.sin(), even * phase.sin() + odd * phase.cos()}, -1).flatten(-2);
}
}
int main() {
    try {
        c10::InferenceMode inference;
        for (const int64_t length : {1, 63, 65, 257}) {
            for (const int64_t width : {32, 64}) {
                auto storage = at::sin(at::arange(2 * length * 3 * width, at::kFloat) * 0.017).reshape({2, length, 3, width});
                auto input = storage.transpose(1, 2); // original noncontiguous head layout
                for (const double base : {10000.0, 1000000.0}) {
                    for (const int64_t offset : {0, 17}) {
                        auto positions = at::arange(length, at::kLong) + offset;
                        const auto phase = uta::torch_native::prepare_rotary_phase(width, positions, input.options(), base);
                        for (int pass = 0; pass < 3; ++pass) {
                            auto current = input + static_cast<float>(pass) * 0.125f;
                            auto interleaved = uta::torch_native::apply_rotary_interleaved(current, phase);
                            auto split = uta::torch_native::apply_rotary_split(current, phase);
                            if (!at::equal(interleaved, reference(current, positions, base, false)) ||
                                !at::equal(split, reference(current, positions, base, true)))
                                throw std::runtime_error("reused rotary phases changed CPU output");
                        }
                    }
                }
            }
        }
        std::cout << "CPU rotary phase reuse matches recomputation exactly\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
