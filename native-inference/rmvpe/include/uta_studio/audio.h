#pragma once
#include <vector>
#include <string>

// RMVPE's frontend requires exactly 16 kHz mono PCM (see mel.h's kSampleRate).
// Unlike RoFormer's stereo path, resampling/downmixing happens once in Rust
// (ggml-worker's audio::decode_mono_wav, mirroring openvino-worker's own
// ffmpeg-based mono decode) before this CLI ever runs -- so loading here is a
// strict validation, not a general-purpose decoder.
struct MonoAudio {
    std::vector<float> samples;
    unsigned int sample_rate;
};

class AudioFile {
public:
    // Throws std::runtime_error if the file cannot be opened, is not mono, or
    // is not 16 kHz.
    static MonoAudio LoadMono16k(const std::string& path);
};
