// SPDX-License-Identifier: GPL-3.0-only
#include <openvino/openvino.hpp>
#include <openvino/pass/serialize.hpp>

#include <charconv>
#include <filesystem>
#include <iostream>
#include <memory>
#include <string>
#include <vector>

namespace {
std::vector<int64_t> parse_shape(const std::string& text) {
    std::vector<int64_t> shape;
    std::size_t start = 0;
    while (start < text.size()) {
        const auto end = text.find(',', start);
        const auto token = text.substr(start, end - start);
        int64_t dimension = 0;
        const auto result = std::from_chars(token.data(), token.data() + token.size(), dimension);
        if (result.ec != std::errc{} || result.ptr != token.data() + token.size() || dimension < 0) {
            throw std::runtime_error("shape must contain non-negative comma-separated dimensions");
        }
        shape.push_back(dimension);
        if (end == std::string::npos) {
            break;
        }
        start = end + 1;
    }
    if (shape.empty()) {
        throw std::runtime_error("shape is empty");
    }
    return shape;
}

std::vector<std::vector<int64_t>> parse_shapes(const std::string& text) {
    std::vector<std::vector<int64_t>> shapes;
    std::size_t start = 0;
    while (start < text.size()) {
        const auto end = text.find(';', start);
        shapes.push_back(parse_shape(text.substr(start, end - start)));
        if (end == std::string::npos) {
            break;
        }
        start = end + 1;
    }
    return shapes;
}
}  // namespace

int main(int argc, char** argv) {
    const bool inspect = argc == 3 && std::string(argv[1]) == "--inspect";
    if (!inspect && argc != 5) {
        std::cerr << "usage: uta-openvino-convert --inspect INPUT\n"
                     "       uta-openvino-convert INPUT SHAPE OUTPUT.xml OUTPUT.bin\n";
        return 2;
    }
    try {
        const std::filesystem::path input(argv[inspect ? 2 : 1]);
        if (!std::filesystem::is_regular_file(input)) {
            throw std::runtime_error("input model is unavailable");
        }
        ov::Core core;
        auto model = core.read_model(input);
        if (inspect) {
            std::cout << "inputs\t" << model->inputs().size() << "\noutputs\t"
                      << model->outputs().size() << '\n';
            std::size_t index = 0;
            for (const auto& port : model->inputs()) {
                std::cout << "input\t" << index++ << '\t'
                          << port.get_element_type().get_type_name() << '\t'
                          << port.get_partial_shape().to_string() << '\n';
            }
            index = 0;
            for (const auto& port : model->outputs()) {
                std::cout << "output\t" << index++ << '\t'
                          << port.get_element_type().get_type_name() << '\t'
                          << port.get_partial_shape().to_string() << '\n';
            }
            return 0;
        }
        const std::filesystem::path xml(argv[3]);
        const std::filesystem::path bin(argv[4]);
        if (std::filesystem::exists(xml) || std::filesystem::exists(bin)) {
            throw std::runtime_error("refusing to overwrite an existing IR output");
        }
        const auto shapes = parse_shapes(argv[2]);
        if (shapes.size() != model->inputs().size()) {
            throw std::runtime_error("shape count must match the model input count");
        }
        std::map<size_t, ov::PartialShape> partial_shapes;
        for (std::size_t index = 0; index < shapes.size(); ++index) {
            partial_shapes.emplace(index, ov::PartialShape(shapes[index]));
        }
        model->reshape(partial_shapes);
        ov::pass::Serialize serialize(xml, bin, ov::pass::Serialize::Version::IR_V11);
        serialize.run_on_model(model);
        if (!std::filesystem::is_regular_file(xml) || !std::filesystem::is_regular_file(bin)) {
            throw std::runtime_error("OpenVINO IR serialization produced incomplete output");
        }
    } catch (const std::exception& error) {
        std::cerr << "uta-openvino-convert: " << error.what() << '\n';
        return 1;
    }
    return 0;
}
