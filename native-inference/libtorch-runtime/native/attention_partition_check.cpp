#include "attention_partition.hpp"
#include <ATen/Context.h>
#include <ATen/Parallel.h>
#include <c10/core/InferenceMode.h>
#include <array>
#include <iomanip>
#include <iostream>

namespace {
at::Tensor fixture(at::IntArrayRef shape, double phase) {
    int64_t count = 1;
    for (auto dimension : shape) count *= dimension;
    return (at::sin(at::arange(count, at::kDouble) * 0.037 + phase) * 0.5).reshape(shape).to(at::kFloat);
}
}
int main(int argc, char** argv) {
    try {
        if (argc != 2 || std::string(argv[1]) != "rocm" || !at::globalContext().hasROCM())
            throw std::invalid_argument("usage: uta-libtorch-attention-partition-check rocm");
        c10::InferenceMode inference;
        at::set_num_threads(2);
        at::globalContext().setSDPUseMath(false);
        const auto device = at::Device(at::kCUDA, 0);
        for (const auto& shape : std::vector<std::array<int64_t, 6>>{{5,4,263,397,64,64}, {1,8,1001,1001,64,64}, {2,4,61,61,128,64}}) {
            const auto batch=shape[0], heads=shape[1], rows=shape[2], keys=shape[3], width=shape[4], output_width=shape[5];
            const auto query=fixture({batch,heads,rows,width},0.31);
            const auto key=fixture({batch,heads,keys,width},0.97);
            const auto value=fixture({batch,heads,keys,output_width},1.73);
            const double scale = 0.125;
            const auto rounded=[](const at::Tensor& input) { return input.to(at::kHalf).to(at::kDouble); };
            auto expected=at::matmul(at::softmax(at::matmul(rounded(query),rounded(key).transpose(-1,-2))*scale,-1),rounded(value));
            expected=expected.to(at::kHalf).to(at::kDouble);
            const auto actual=uta::torch_native::partitioned_fused_attention(query.to(device),key.to(device),value.to(device),scale,[] {}).to(at::kCPU).to(at::kDouble);
            const auto error=actual-expected;
            const auto nmse=error.square().sum().item<double>()/std::max(expected.square().sum().item<double>(),1e-30);
            const auto maximum=error.abs().max().item<double>();
            const bool passed=actual.sizes()==expected.sizes() && at::isfinite(actual).all().item<bool>() && nmse <= 5e-5 && maximum <= 1e-3;
            std::cout << std::setprecision(12) << "{\"event\":\"partitioned_attention_check\",\"backend\":\"rocm\",\"query_rows\":" << rows
                      << ",\"key_rows\":" << keys << ",\"query_width\":" << width << ",\"value_width\":" << output_width
                      << ",\"compared_elements\":" << expected.numel() << ",\"nmse\":" << nmse << ",\"maximum_absolute_error\":" << maximum
                      << ",\"math_fallback\":false,\"passed\":" << (passed ? "true" : "false") << "}\n" << std::flush;
            if (!passed) throw std::runtime_error("partitioned fused attention disagrees with complete rounded-input double reference");
        }
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
