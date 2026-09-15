#include "weight_upload.hpp"
#include <ATen/Parallel.h>
#include <c10/core/InferenceMode.h>
#include <cstring>
#include <iostream>
#include <vector>

namespace {
void require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}
void check(const at::Tensor& stored, int64_t tile_bytes) {
    int64_t completed = 0;
    const auto dtype = stored.is_floating_point() ? at::kFloat : at::kLong;
    auto expected = stored.to(dtype);
    auto actual = uta::torch_native::upload_weight_in_tiles(stored, at::Device(at::kCPU),
        [&](int64_t copied, int64_t total) {
            require(total == stored.numel() && copied > completed && copied <= total,
                    "upload did not complete each consecutive tile");
            require((copied - completed) * expected.element_size() <= static_cast<uint64_t>(tile_bytes),
                    "upload exceeded its staging byte budget");
            completed = copied;
        }, tile_bytes);
    require(completed == stored.numel(), "upload lost the final partial tile");
    require(actual.sizes() == expected.sizes() && actual.scalar_type() == expected.scalar_type(),
            "upload changed native weight shape or dtype");
    if (stored.is_floating_point()) {
        auto equal = at::logical_or(actual == expected, at::logical_and(at::isnan(actual), at::isnan(expected)));
        require(equal.all().item<bool>(), "upload changed a floating-point weight");
        auto zero_sign = at::logical_or(expected != 0, at::signbit(actual) == at::signbit(expected));
        require(zero_sign.all().item<bool>(), "upload changed signed zero");
    } else {
        require(at::equal(actual, expected), "upload changed an integer weight");
    }
}
} // namespace
int main() {
    try {
        c10::InferenceMode inference;
        at::set_num_threads(2);
        for (int64_t count : {0, 1, 7, 8, 9, 1003}) {
            check(at::arange(count, at::kFloat) * 0.0137 + 1.00008, 32);
            check(at::arange(count, at::kFloat).to(at::kHalf), 32);
            check(at::arange(count, at::kFloat).to(at::kBFloat16), 32);
            check(at::arange(count, at::kInt), 32);
            check(at::arange(count, at::kLong) + (int64_t{1} << 40), 32);
        }
        check(at::scalar_tensor(1.00008, at::kFloat), 32);
        check(at::arange(21, at::kFloat).reshape({3, 7}), 32);
        // All half-storage patterns, including subnormals, signed zeros,
        // infinities and NaNs. A non-power-of-two tile also exercises tails.
        std::vector<uint16_t> patterns(65536);
        for (size_t index = 0; index < patterns.size(); ++index) patterns[index] = static_cast<uint16_t>(index);
        auto half = at::empty({static_cast<int64_t>(patterns.size())}, at::kHalf);
        std::memcpy(half.data_ptr(), patterns.data(), patterns.size() * sizeof(uint16_t));
        check(half, 1036);
        int calls = 0;
        bool failed = false;
        try {
            uta::torch_native::upload_weight_in_tiles(at::arange(65, at::kFloat), at::Device(at::kCPU),
                [&](int64_t, int64_t) { ++calls; throw std::runtime_error("completion failure"); }, 32);
        } catch (const std::runtime_error& error) {
            failed = std::string(error.what()) == "completion failure";
        }
        require(failed && calls == 1, "upload retried or continued after completion failed");
        std::cout << "Weight upload dtype, complete half-pattern, tile, tail and failure checks passed (CPU only)\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
