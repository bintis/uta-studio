#define DR_WAV_IMPLEMENTATION
#include "dr_libs/dr_wav.h"
#include "uta_studio/audio.h"
#include <cmath>
#include <limits>
#include <stdexcept>

namespace {
constexpr drwav_uint64 kMaxInputFrames = 4ULL * 60ULL * 60ULL * 16'000ULL;
}

MonoAudio AudioFile::LoadMono16k(const std::string& path) {
    unsigned int channels = 0;
    unsigned int sample_rate = 0;
    drwav_uint64 total_pcm_frames = 0;

    float* data = drwav_open_file_and_read_pcm_frames_f32(
        path.c_str(), &channels, &sample_rate, &total_pcm_frames, NULL);
    if (!data) {
        throw std::runtime_error("Failed to open audio file: " + path);
    }

    MonoAudio audio;
    if (channels != 1 || sample_rate != 16000) {
        drwav_free(data, NULL);
        throw std::runtime_error(
            "RMVPE input must be pre-decoded to 16 kHz mono (got " +
            std::to_string(channels) + " channel(s) at " + std::to_string(sample_rate) + " Hz): " + path);
    }

    if (total_pcm_frames == 0 || total_pcm_frames > kMaxInputFrames ||
        total_pcm_frames > static_cast<drwav_uint64>(std::numeric_limits<size_t>::max()) ||
        total_pcm_frames > audio.samples.max_size()) {
        drwav_free(data, NULL);
        throw std::runtime_error("RMVPE input frame count is invalid: " + path);
    }
    audio.samples.assign(data, data + static_cast<size_t>(total_pcm_frames));
    for (float sample : audio.samples) {
        if (!std::isfinite(sample)) {
            drwav_free(data, NULL);
            throw std::runtime_error("RMVPE input contains a non-finite sample: " + path);
        }
    }
    audio.sample_rate = sample_rate;
    drwav_free(data, NULL);
    return audio;
}
