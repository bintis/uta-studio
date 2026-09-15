#pragma once
#include <ATen/ATen.h>
#include <cstdint>
#include <fstream>
#include <functional>
#include <map>
#include <string>
#include <variant>
#include <vector>

namespace uta::torch_native {
struct Meta {
    using Array = std::vector<Meta>;
    std::variant<uint64_t, int64_t, double, bool, std::string, Array> value;
    int64_t integer() const;
    double number() const;
    const std::string& text() const;
    const Array& array() const;
    std::string json() const;
};
std::string json_string(const std::string& value);

class Gguf {
public:
    explicit Gguf(const std::string& path);
    const std::map<std::string, Meta>& metadata() const { return metadata_; }
    const Meta& meta(const std::string& name) const;
    bool has_meta(const std::string& name) const;
    int64_t integer(const std::string& name, int64_t default_value) const;
    double number(const std::string& name, double default_value) const;
    std::string text(const std::string& name, const std::string& default_value = "") const;
    std::vector<int64_t> integers(const std::string& name) const;
    std::string metadata_json() const;
    std::vector<std::string> tensor_names() const;
    std::vector<int64_t> shape(const std::string& name) const;
    at::Tensor read_tensor(const std::string& name);
private:
    struct TensorInfo { std::vector<int64_t> shape; uint32_t kind; uint64_t offset; uint64_t bytes; };
    std::ifstream stream_;
    std::map<std::string, Meta> metadata_;
    std::map<std::string, TensorInfo> tensors_;
    uint64_t file_bytes_ = 0;
    uint64_t data_offset_ = 0;
    uint64_t remaining_budget_ = 256 * 1024 * 1024;
    void read(void* target, uint64_t bytes);
    template<typename Type> Type scalar() { Type result{}; read(&result, sizeof(result)); return result; }
    std::string string();
    Meta read_meta(uint32_t kind, unsigned depth = 0);
};

class Weights {
public:
    Weights(const std::string& path, const at::Device& device, const std::function<void()>& complete);
    bool has(const std::string& name) const;
    const at::Tensor& get(const std::string& name) const;
    at::Tensor optional(const std::string& name) const;
    at::Tensor linear(const at::Tensor& input, const std::string& prefix) const;
    at::Tensor norm(const at::Tensor& input, const std::string& prefix, double epsilon = 1e-5) const;
    at::Tensor rms_norm(const at::Tensor& input, const std::string& weight, double epsilon = 1e-5) const;
    Gguf container;
    at::Device device;
private:
    std::map<std::string, at::Tensor> tensors_;
};
} // namespace uta::torch_native
