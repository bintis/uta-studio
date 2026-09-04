#pragma once
// RMVPE's 16 kHz / 128-bin HTK-mel log-magnitude frontend. Ported
// term-for-term from the repository's shared Rust reference (same window,
// reflect-padding, and log floor) so backend comparisons use identical input --
// verified against an identical synthetic-input dump from the Rust
// implementation before integration.

#include <cmath>
#include <cstdint>
#include <string>
#include <vector>

#include "stft.h"

namespace rmvpe {

constexpr int kSampleRate = 16'000;
constexpr int kFftSize = 1'024;
constexpr int kHopSize = 160;
constexpr int kMelBins = 128;

struct MelBand {
    std::vector<std::pair<int, float>> weights; // (fft_bin, weight)
};

inline float HzToHtkMel(float hz) {
    return 2'595.0f * std::log10(1.0f + hz / 700.0f);
}

inline float HtkMelToHz(float mel) {
    return 700.0f * (std::pow(10.0f, mel / 2'595.0f) - 1.0f);
}

inline std::vector<MelBand> BuildMelBands() {
    const float min_mel = HzToHtkMel(30.0f);
    const float max_mel = HzToHtkMel(8'000.0f);
    std::vector<float> mel_points(kMelBins + 2);
    for (int i = 0; i < kMelBins + 2; ++i) {
        const float fraction = static_cast<float>(i) / static_cast<float>(kMelBins + 1);
        mel_points[i] = HtkMelToHz(min_mel + fraction * (max_mel - min_mel));
    }
    std::vector<float> fft_frequencies(kFftSize / 2 + 1);
    for (int bin = 0; bin <= kFftSize / 2; ++bin) {
        fft_frequencies[bin] = static_cast<float>(kSampleRate) * static_cast<float>(bin) / static_cast<float>(kFftSize);
    }
    std::vector<MelBand> bands(kMelBins);
    for (int band = 0; band < kMelBins; ++band) {
        const float lower = mel_points[band];
        const float center = mel_points[band + 1];
        const float upper = mel_points[band + 2];
        const float normalization = 2.0f / (upper - lower);
        for (int bin = 0; bin <= kFftSize / 2; ++bin) {
            const float frequency = fft_frequencies[bin];
            float weight = 0.0f;
            if (frequency >= lower && frequency <= center) {
                weight = (frequency - lower) / (center - lower);
            } else if (frequency > center && frequency <= upper) {
                weight = (upper - frequency) / (upper - center);
            }
            weight *= normalization;
            if (weight > 0.0f) {
                bands[band].weights.emplace_back(bin, weight);
            }
        }
    }
    return bands;
}

inline float ReflectedSample(const float* audio, int64_t audio_len, int64_t padded_index) {
    const int64_t pad = kFftSize / 2;
    if (padded_index < pad) {
        return audio[pad - padded_index];
    }
    const int64_t audio_index = padded_index - pad;
    if (audio_index < audio_len) {
        return audio[audio_index];
    }
    const int64_t offset = audio_index - audio_len;
    return audio[audio_len - 2 - offset];
}

// Returns frame-major log-mel data with shape (frame_count, 128), matching
// mel.rs::log_mel_spectrogram exactly (natural-log floor 1e-5, magnitude not
// power spectrum, periodic Hann, reflect padding of FFT_SIZE/2 each side).
inline std::vector<float> LogMelSpectrogram(const float* audio, int64_t audio_len, int* out_frame_count) {
    if (audio_len <= kFftSize) {
        throw std::runtime_error("RMVPE requires more than 64 ms of decoded audio");
    }
    const int64_t frame_count = audio_len / kHopSize + 1;
    const int64_t padded_length = audio_len + kFftSize;
    const int64_t last_frame_end = (frame_count - 1) * kHopSize + kFftSize;
    if (last_frame_end > padded_length) {
        throw std::runtime_error("internal STFT frame calculation exceeded reflected padding");
    }

    static const std::vector<MelBand> bands = BuildMelBands();
    std::vector<float> window(kFftSize);
    stft::hann_window(window.data(), kFftSize, /*periodic=*/true);

    auto fft = stft::TableFFT::GetInstance(kFftSize);
    stft::STFTBuffer buffer;
    buffer.Resize(kFftSize);

    std::vector<float> windowed(kFftSize);
    std::vector<stft::Complex> spectrum(kFftSize / 2 + 1);
    std::vector<float> magnitudes(kFftSize / 2 + 1);
    std::vector<float> output(static_cast<size_t>(frame_count) * kMelBins);

    for (int64_t frame = 0; frame < frame_count; ++frame) {
        const int64_t start = frame * kHopSize;
        for (int i = 0; i < kFftSize; ++i) {
            windowed[i] = ReflectedSample(audio, audio_len, start + i) * window[i];
        }
        stft::rfft(windowed.data(), spectrum.data(), kFftSize, buffer, *fft);
        for (int i = 0; i <= kFftSize / 2; ++i) {
            magnitudes[i] = std::abs(spectrum[i]);
        }
        for (int band_index = 0; band_index < kMelBins; ++band_index) {
            float energy = 0.0f;
            for (const auto& [bin, weight] : bands[band_index].weights) {
                energy += magnitudes[bin] * weight;
            }
            output[frame * kMelBins + band_index] = std::log(std::max(energy, 1.0e-5f));
        }
    }
    *out_frame_count = static_cast<int>(frame_count);
    return output;
}

// Rearranges frame-major mel (frames, 128) into channel-major [128, window_frames],
// zero-padding (not log-floor-padding) beyond the real frame count -- matches
// mel.rs::to_channel_major_window exactly; this distinction is material to the
// bidirectional GRU at the final bucket boundary (see mel.rs's own comment).
inline std::vector<float> ToChannelMajorWindow(const float* frame_major, int frames, int start, int window_frames) {
    std::vector<float> channel_major(static_cast<size_t>(kMelBins) * window_frames, 0.0f);
    const int copied_frames = std::min(std::max(frames - start, 0), window_frames);
    for (int frame = 0; frame < copied_frames; ++frame) {
        for (int channel = 0; channel < kMelBins; ++channel) {
            channel_major[static_cast<size_t>(channel) * window_frames + frame] =
                frame_major[static_cast<size_t>(start + frame) * kMelBins + channel];
        }
    }
    return channel_major;
}

} // namespace rmvpe
