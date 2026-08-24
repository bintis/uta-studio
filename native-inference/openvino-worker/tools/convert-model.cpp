// SPDX-License-Identifier: GPL-3.0-only
#include <openvino/openvino.hpp>
#include <openvino/core/graph_util.hpp>
#include <openvino/op/constant.hpp>
#include <openvino/op/group_conv.hpp>
#include <openvino/op/parameter.hpp>
#include <openvino/op/squeeze.hpp>
#include <openvino/op/unsqueeze.hpp>
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

void expose_scalar_as_vector(const std::shared_ptr<ov::Model>& model, std::size_t index) {
    const auto original = model->get_parameters().at(index);
    const auto targets = original->output(0).get_target_inputs();
    const auto replacement = std::make_shared<ov::op::v0::Parameter>(
        original->get_element_type(), ov::PartialShape{1});
    replacement->set_friendly_name(original->get_friendly_name());
    model->replace_parameter(index, replacement);
    const auto axis = ov::op::v0::Constant::create(ov::element::i64, ov::Shape{1}, {0});
    const auto scalar = std::make_shared<ov::op::v0::Squeeze>(replacement, axis);
    for (const auto& target : targets) {
        target.replace_source_output(scalar->output(0));
    }
}

void reshape_game_model(
    const std::shared_ptr<ov::Model>& model,
    const std::string& kind,
    int64_t estimator_notes
) {
    constexpr int64_t kSamples = 1'323'000;
    constexpr int64_t kFrames = 3'000;
    constexpr int64_t kEmbedding = 256;
    std::map<std::size_t, ov::PartialShape> shapes;
    if (kind == "encoder") {
        shapes = {{0, {1, kSamples}}, {1, {1}}};
    } else if (kind == "segmenter") {
        expose_scalar_as_vector(model, 6);
        expose_scalar_as_vector(model, 7);
        shapes = {
            {0, {1, kFrames, kEmbedding}}, {1, {1}}, {2, {1, kFrames}},
            {3, {1, kFrames}}, {4, {1}}, {5, {1, kFrames}}, {6, {1}}, {7, {1}},
        };
    } else if (kind == "estimator") {
        if (estimator_notes <= 0 || estimator_notes > kFrames) {
            throw std::runtime_error("GAME estimator note bucket must be within 1..3000");
        }
        expose_scalar_as_vector(model, 4);
        shapes = {
            {0, {1, kFrames, kEmbedding}}, {1, {1, kFrames}},
            {2, {1, kFrames}}, {3, {1, estimator_notes}}, {4, {1}},
        };
    } else {
        throw std::runtime_error("GAME model kind must be encoder, segmenter, or estimator");
    }
    if (shapes.size() != model->inputs().size()) {
        throw std::runtime_error("GAME model input count does not match its pinned contract");
    }
    model->reshape(shapes);
}

// OpenVINO GPU 2026.3 rejects the ONNX frontend's rank-3 depthwise group
// convolution. Lift it to an exactly equivalent rank-4 spatial operation.
void lift_group_convolution_1d(const std::shared_ptr<ov::Model>& model) {
    for (const auto& node : model->get_ordered_ops()) {
        const auto group = std::dynamic_pointer_cast<ov::op::v1::GroupConvolution>(node);
        if (!group || group->get_input_partial_shape(0).rank().get_length() != 3) {
            continue;
        }
        const auto data_axis = ov::op::v0::Constant::create(
            ov::element::i64, ov::Shape{1}, {2});
        const auto weight_axis = ov::op::v0::Constant::create(
            ov::element::i64, ov::Shape{1}, {3});
        const auto data = std::make_shared<ov::op::v0::Unsqueeze>(
            group->input_value(0), data_axis);
        const auto weights = std::make_shared<ov::op::v0::Unsqueeze>(
            group->input_value(1), weight_axis);
        const auto strides = group->get_strides();
        const auto pads_begin = group->get_pads_begin();
        const auto pads_end = group->get_pads_end();
        const auto dilations = group->get_dilations();
        const auto lifted = std::make_shared<ov::op::v1::GroupConvolution>(
            data, weights, ov::Strides{1, strides.at(0)},
            ov::CoordinateDiff{0, pads_begin.at(0)},
            ov::CoordinateDiff{0, pads_end.at(0)},
            ov::Strides{1, dilations.at(0)}, group->get_auto_pad());
        const auto squeezed = std::make_shared<ov::op::v0::Squeeze>(lifted, data_axis);
        squeezed->set_friendly_name(group->get_friendly_name());
        ov::replace_node(group, squeezed);
    }
    model->validate_nodes_and_infer_types();
}
}  // namespace

int main(int argc, char** argv) {
    const bool inspect = argc == 3 && std::string(argv[1]) == "--inspect";
    const bool game = argc == 6 && std::string(argv[1]) == "--game-v1";
    if (!inspect && !game && argc != 5) {
        std::cerr << "usage: uta-openvino-convert --inspect INPUT\n"
                     "       uta-openvino-convert INPUT SHAPE OUTPUT.xml OUTPUT.bin\n"
                     "       uta-openvino-convert --game-v1 KIND[:NOTE_BUCKET] INPUT OUTPUT.xml OUTPUT.bin\n";
        return 2;
    }
    try {
        const std::filesystem::path input(argv[inspect ? 2 : game ? 3 : 1]);
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
        const std::filesystem::path xml(argv[game ? 4 : 3]);
        const std::filesystem::path bin(argv[game ? 5 : 4]);
        if (std::filesystem::exists(xml) || std::filesystem::exists(bin)) {
            throw std::runtime_error("refusing to overwrite an existing IR output");
        }
        if (game) {
            std::string kind(argv[2]);
            int64_t estimator_notes = 0;
            if (const auto separator = kind.find(':'); separator != std::string::npos) {
                const auto bucket = kind.substr(separator + 1);
                kind.resize(separator);
                const auto result = std::from_chars(
                    bucket.data(), bucket.data() + bucket.size(), estimator_notes);
                if (result.ec != std::errc{} || result.ptr != bucket.data() + bucket.size()) {
                    throw std::runtime_error("invalid GAME estimator note bucket");
                }
            }
            if ((kind == "estimator") != (estimator_notes > 0)) {
                throw std::runtime_error(
                    "GAME estimator requires KIND estimator:NOTE_BUCKET; other kinds take no bucket");
            }
            reshape_game_model(model, kind, estimator_notes);
            lift_group_convolution_1d(model);
        } else {
            const auto shapes = parse_shapes(argv[2]);
            if (shapes.size() != model->inputs().size()) {
                throw std::runtime_error("shape count must match the model input count");
            }
            std::map<size_t, ov::PartialShape> partial_shapes;
            for (std::size_t index = 0; index < shapes.size(); ++index) {
                partial_shapes.emplace(index, ov::PartialShape(shapes[index]));
            }
            model->reshape(partial_shapes);
        }
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
