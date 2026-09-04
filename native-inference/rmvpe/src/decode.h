#pragma once
// RMVPE pitch decode math (local averaging, activation layout, and windowing
// constants), retained term-for-term from the earlier validated reference so
// GGML/Vulkan evidence remains directly comparable across backend revisions.

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>

namespace rmvpe {

constexpr int kMinInputFrames = 32;
constexpr int kMaxInputFrames = 1'024;
constexpr int kFrameStep = 32;
constexpr int kOverlapFrames = 128;
constexpr int kStrideFrames = kMaxInputFrames - kOverlapFrames; // 896
constexpr int kPitchClasses = 360;
constexpr float kCentsOffset = 1'997.379'4f;
constexpr float kVoicedThreshold = 0.03f;
constexpr double kTimelineStepSeconds = 0.01;

struct PitchFrame {
    double time;
    float hz;
    float confidence;
    bool voiced;
};

// argmax over 360 salience classes -> +/-4-bin (20-cent) salience-weighted
// average -> cents-to-Hz. Matches rmvpe.rs::local_average_hz exactly,
// including its unweighted-fallback path when all salience in the window is
// ~zero (weight <= f32::EPSILON).
inline std::pair<float, float> LocalAverageHz(const float* activation, int n) {
    if (activation == nullptr || n != kPitchClasses) {
        throw std::runtime_error("RMVPE activation frame has an invalid shape");
    }
    for (int i = 0; i < n; ++i) {
        if (!std::isfinite(activation[i]) || activation[i] < 0.0f || activation[i] > 1.0f) {
            throw std::runtime_error("RMVPE activation frame contains an invalid value");
        }
    }
    int center = 0;
    float confidence = activation[0];
    for (int i = 1; i < n; ++i) {
        if (activation[i] > confidence) {
            confidence = activation[i];
            center = i;
        }
    }
    const int start = std::max(center - 4, 0);
    const int end = std::min(center + 4, kPitchClasses - 1);
    double weighted_cents = 0.0;
    double weight = 0.0;
    for (int cls = start; cls <= end; ++cls) {
        const float salience = activation[cls];
        weighted_cents += static_cast<double>(salience) * (20.0 * cls + kCentsOffset);
        weight += salience;
    }
    double cents;
    if (weight > std::numeric_limits<float>::epsilon()) {
        cents = weighted_cents / weight;
    } else {
        cents = 20.0 * center + kCentsOffset;
    }
    const float hz = static_cast<float>(10.0 * std::pow(2.0, cents / 1'200.0));
    return {hz, std::clamp(confidence, 0.0f, 1.0f)};
}

// Extracts one frame's 360-class activation from the model's raw output,
// accepting either [1,frames,360] or [1,360,frames] layout (the production
// worker is defensive about this since it never inspects the graph itself --
// same here, since the GGML graph's declared output layout is a choice made
// once in graph.cpp and this decode path should not silently assume it).
inline const float* ActivationFrame(const float* data, const int64_t* dims, int padded_frames,
                                     int frame, std::vector<float>* scratch) {
    if (dims[1] == padded_frames && dims[2] == kPitchClasses) {
        return data + static_cast<int64_t>(frame) * kPitchClasses;
    }
    if (dims[1] == kPitchClasses && dims[2] == padded_frames) {
        scratch->resize(kPitchClasses);
        for (int cls = 0; cls < kPitchClasses; ++cls) {
            (*scratch)[cls] = data[static_cast<int64_t>(cls) * padded_frames + frame];
        }
        return scratch->data();
    }
    throw std::runtime_error("unexpected RMVPE output shape");
}

} // namespace rmvpe
