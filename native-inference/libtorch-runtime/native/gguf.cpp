#include "gguf.hpp"
#include <algorithm>
#include <bit>
#include <cmath>
#include <iomanip>
#include <limits>
#include <sstream>
#include <stdexcept>

namespace uta::torch_native {
namespace {
struct Storage { at::ScalarType dtype; uint64_t bytes; };
Storage storage(uint32_t kind) {
    switch (kind) {
        case 0: return {at::kFloat, 4};
        case 1: return {at::kHalf, 2};
        case 24: return {at::kChar, 1};
        case 25: return {at::kShort, 2};
        case 26: return {at::kInt, 4};
        case 27: return {at::kLong, 8};
        case 28: return {at::kDouble, 8};
        case 30: return {at::kBFloat16, 2};
        default: throw std::runtime_error("LibTorch GGUF reader does not support tensor storage type " + std::to_string(kind));
    }
}
uint64_t multiply(uint64_t left, uint64_t right) {
    if (right && left > static_cast<uint64_t>(std::numeric_limits<int64_t>::max()) / right)
        throw std::runtime_error("GGUF tensor size overflows supported address range");
    return left * right;
}
}

std::string json_string(const std::string& value) {
    std::ostringstream output;
    output << '"';
    for (const unsigned char character : value) {
        switch (character) {
            case '"': output << "\\\""; break;
            case '\\': output << "\\\\"; break;
            case '\n': output << "\\n"; break;
            case '\r': output << "\\r"; break;
            case '\t': output << "\\t"; break;
            default:
                if (character < 32) output << "\\u" << std::hex << std::setw(4) << std::setfill('0') << static_cast<unsigned>(character) << std::dec;
                else output << character;
        }
    }
    output << '"';
    return output.str();
}

int64_t Meta::integer() const {
    if (const auto* item = std::get_if<int64_t>(&value)) return *item;
    if (const auto* item = std::get_if<uint64_t>(&value)) {
        if (*item > static_cast<uint64_t>(std::numeric_limits<int64_t>::max())) throw std::runtime_error("GGUF integer exceeds int64");
        return static_cast<int64_t>(*item);
    }
    if (const auto* item = std::get_if<bool>(&value)) return *item;
    throw std::runtime_error("GGUF metadata value is not an integer");
}
double Meta::number() const {
    if (const auto* item = std::get_if<double>(&value)) return *item;
    return static_cast<double>(integer());
}
const std::string& Meta::text() const {
    if (const auto* item = std::get_if<std::string>(&value)) return *item;
    throw std::runtime_error("GGUF metadata value is not a string");
}
const Meta::Array& Meta::array() const {
    if (const auto* item = std::get_if<Array>(&value)) return *item;
    throw std::runtime_error("GGUF metadata value is not an array");
}
std::string Meta::json() const {
    return std::visit([](const auto& item) -> std::string {
        using Type = std::decay_t<decltype(item)>;
        if constexpr (std::is_same_v<Type, std::string>) return json_string(item);
        else if constexpr (std::is_same_v<Type, bool>) return item ? "true" : "false";
        else if constexpr (std::is_same_v<Type, Array>) {
            std::string result = "[";
            for (const auto& child : item) { if (result.size() > 1) result += ','; result += child.json(); }
            return result + ']';
        } else {
            std::ostringstream output;
            output << std::setprecision(17) << item;
            return output.str();
        }
    }, value);
}

void Gguf::read(void* target, uint64_t bytes) {
    const auto position = stream_.tellg();
    if (position < 0 || static_cast<uint64_t>(position) > file_bytes_ || bytes > file_bytes_ - static_cast<uint64_t>(position))
        throw std::runtime_error("truncated GGUF input");
    stream_.read(static_cast<char*>(target), static_cast<std::streamsize>(bytes));
    if (!stream_ || stream_.gcount() != static_cast<std::streamsize>(bytes)) throw std::runtime_error("GGUF read failed");
}
std::string Gguf::string() {
    const auto count = scalar<uint64_t>();
    if (count > remaining_budget_) throw std::runtime_error("GGUF metadata exceeds bounded reader budget");
    remaining_budget_ -= count;
    std::string result(static_cast<std::size_t>(count), '\0');
    if (count) read(result.data(), count);
    return result;
}
Meta Gguf::read_meta(uint32_t kind, unsigned depth) {
    if (depth > 8 || remaining_budget_ < sizeof(Meta)) throw std::runtime_error("GGUF metadata nesting or size exceeds bounded reader budget");
    remaining_budget_ -= sizeof(Meta);
    switch (kind) {
        case 0: return Meta{static_cast<uint64_t>(scalar<uint8_t>())};
        case 1: return Meta{static_cast<int64_t>(scalar<int8_t>())};
        case 2: return Meta{static_cast<uint64_t>(scalar<uint16_t>())};
        case 3: return Meta{static_cast<int64_t>(scalar<int16_t>())};
        case 4: return Meta{static_cast<uint64_t>(scalar<uint32_t>())};
        case 5: return Meta{static_cast<int64_t>(scalar<int32_t>())};
        case 6: {
            const auto value = scalar<float>();
            if (!std::isfinite(value)) throw std::runtime_error("GGUF metadata contains a nonfinite float");
            return Meta{static_cast<double>(value)};
        }
        case 7: {
            const auto value = scalar<uint8_t>();
            if (value > 1) throw std::runtime_error("GGUF metadata contains an invalid boolean");
            return Meta{value != 0};
        }
        case 8: return Meta{string()};
        case 9: {
            const auto child_kind = scalar<uint32_t>();
            const auto count = scalar<uint64_t>();
            if (count > remaining_budget_ / sizeof(Meta)) throw std::runtime_error("GGUF metadata array exceeds bounded reader budget");
            Meta::Array result;
            result.reserve(static_cast<std::size_t>(count));
            for (uint64_t index = 0; index < count; ++index) result.push_back(read_meta(child_kind, depth + 1));
            return Meta{std::move(result)};
        }
        case 10: return Meta{scalar<uint64_t>()};
        case 11: return Meta{scalar<int64_t>()};
        case 12: {
            const auto value = scalar<double>();
            if (!std::isfinite(value)) throw std::runtime_error("GGUF metadata contains a nonfinite double");
            return Meta{value};
        }
        default: throw std::runtime_error("unsupported GGUF metadata type " + std::to_string(kind));
    }
}

Gguf::Gguf(const std::string& path) : stream_(path, std::ios::binary) {
    if constexpr (std::endian::native != std::endian::little) throw std::runtime_error("native GGUF reader requires a little-endian host");
    if (!stream_) throw std::runtime_error("cannot open GGUF weights read-only: " + path);
    stream_.seekg(0, std::ios::end);
    const auto end = stream_.tellg();
    if (end < 24) throw std::runtime_error("GGUF header is missing or truncated");
    file_bytes_ = static_cast<uint64_t>(end);
    stream_.seekg(0);
    if (scalar<uint32_t>() != 0x46554747) throw std::runtime_error("weights do not have GGUF magic");
    const auto version = scalar<uint32_t>();
    if (version != 2 && version != 3) throw std::runtime_error("GGUF reader supports versions two and three only");
    const auto tensor_count = scalar<uint64_t>();
    const auto metadata_count = scalar<uint64_t>();
    if (tensor_count > 100000 || metadata_count > 1000000) throw std::runtime_error("GGUF directory exceeds bounded reader limits");
    for (uint64_t index = 0; index < metadata_count; ++index) {
        const auto name = string();
        const auto kind = scalar<uint32_t>();
        if (!metadata_.emplace(name, read_meta(kind)).second) throw std::runtime_error("duplicate GGUF metadata key: " + name);
    }
    for (uint64_t index = 0; index < tensor_count; ++index) {
        const auto name = string();
        const auto rank = scalar<uint32_t>();
        if (!rank || rank > 4) throw std::runtime_error("unsupported GGUF rank for " + name);
        TensorInfo info;
        uint64_t elements = 1;
        for (uint32_t axis = 0; axis < rank; ++axis) {
            const auto dimension = scalar<uint64_t>();
            if (!dimension) throw std::runtime_error("zero GGUF dimension for " + name);
            elements = multiply(elements, dimension);
            info.shape.push_back(static_cast<int64_t>(dimension));
        }
        // GGUF lists the innermost contiguous axis first; ATen is row-major.
        std::reverse(info.shape.begin(), info.shape.end());
        info.kind = scalar<uint32_t>();
        info.offset = scalar<uint64_t>();
        info.bytes = multiply(elements, storage(info.kind).bytes);
        if (!tensors_.emplace(name, std::move(info)).second) throw std::runtime_error("duplicate GGUF tensor: " + name);
    }
    const auto alignment = integer("general.alignment", 32);
    if (alignment < 1 || alignment > 1048576 || (alignment & (alignment - 1))) throw std::runtime_error("invalid GGUF data alignment");
    const auto directory_end = static_cast<uint64_t>(stream_.tellg());
    data_offset_ = (directory_end + alignment - 1) & ~(static_cast<uint64_t>(alignment) - 1);
    if (data_offset_ > file_bytes_) throw std::runtime_error("GGUF data section is truncated");
    for (const auto& [name, info] : tensors_)
        if (info.offset % alignment || info.offset > file_bytes_ - data_offset_ || info.bytes > file_bytes_ - data_offset_ - info.offset)
            throw std::runtime_error("invalid or truncated GGUF tensor range: " + name);
}

const Meta& Gguf::meta(const std::string& name) const {
    const auto found = metadata_.find(name);
    if (found == metadata_.end()) throw std::runtime_error("missing GGUF metadata: " + name);
    return found->second;
}
bool Gguf::has_meta(const std::string& name) const { return metadata_.contains(name); }
int64_t Gguf::integer(const std::string& name, int64_t default_value) const { return has_meta(name) ? meta(name).integer() : default_value; }
double Gguf::number(const std::string& name, double default_value) const { return has_meta(name) ? meta(name).number() : default_value; }
std::string Gguf::text(const std::string& name, const std::string& default_value) const { return has_meta(name) ? meta(name).text() : default_value; }
std::vector<int64_t> Gguf::integers(const std::string& name) const {
    std::vector<int64_t> result;
    for (const auto& child : meta(name).array()) result.push_back(child.integer());
    return result;
}
std::string Gguf::metadata_json() const {
    std::string result = "{";
    for (const auto& [name, value] : metadata_) { if (result.size() > 1) result += ','; result += json_string(name) + ':' + value.json(); }
    return result + '}';
}
std::vector<std::string> Gguf::tensor_names() const {
    std::vector<std::string> result;
    for (const auto& [name, info] : tensors_) result.push_back(name);
    return result;
}
std::vector<int64_t> Gguf::shape(const std::string& name) const { return tensors_.at(name).shape; }
at::Tensor Gguf::read_tensor(const std::string& name) {
    const auto found = tensors_.find(name);
    if (found == tensors_.end()) throw std::runtime_error("missing GGUF tensor: " + name);
    const auto& info = found->second;
    auto result = at::empty(info.shape, at::TensorOptions().device(at::kCPU).dtype(storage(info.kind).dtype));
    stream_.clear();
    stream_.seekg(static_cast<std::streamoff>(data_offset_ + info.offset));
    read(result.data_ptr(), info.bytes);
    return result;
}

Weights::Weights(const std::string& path, const at::Device& selected) : container(path), device(selected) {
    for (const auto& name : container.tensor_names()) {
        auto tensor = container.read_tensor(name);
        // One explicit conversion at load, not a conversion at every layer call.
        tensor = tensor.to(at::TensorOptions().device(device).dtype(tensor.is_floating_point() ? at::kFloat : at::kLong));
        tensors_.emplace(name, std::move(tensor));
    }
}
bool Weights::has(const std::string& name) const { return tensors_.contains(name); }
const at::Tensor& Weights::get(const std::string& name) const {
    const auto found = tensors_.find(name);
    if (found == tensors_.end()) throw std::runtime_error("missing native model weight: " + name);
    return found->second;
}
at::Tensor Weights::optional(const std::string& name) const { return has(name) ? get(name) : at::Tensor(); }
at::Tensor Weights::linear(const at::Tensor& input, const std::string& prefix) const {
    return at::linear(input, get(prefix + ".weight"), optional(prefix + ".bias"));
}
at::Tensor Weights::norm(const at::Tensor& input, const std::string& prefix, double epsilon) const {
    auto value = input.to(at::kFloat);
    return at::layer_norm(value, {value.size(-1)}, get(prefix + ".weight"), optional(prefix + ".bias"), epsilon);
}
at::Tensor Weights::rms_norm(const at::Tensor& input, const std::string& name, double epsilon) const {
    auto value = input.to(at::kFloat);
    auto normalized = value * at::rsqrt(value.square().mean(-1, true) + epsilon);
    return name.empty() ? normalized : normalized * get(name);
}
} // namespace uta::torch_native
