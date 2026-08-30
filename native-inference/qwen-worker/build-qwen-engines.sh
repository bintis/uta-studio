#!/usr/bin/env bash
set -euo pipefail

# Explicit source build. Uta! Studio never invokes this during startup or
# diagnostics. No model is downloaded by this script.
readonly ASR_COMMIT="ea077b87590bcfb090d7c38c03ab36cd1c7005d3"
readonly ALIGN_COMMIT="6dcc586e5073fd6e85ee5728e75f0903d6c70c6c"
readonly GGML_COMMIT="8c63e70982c95ceb862e3a1073a2c1beef75d60a"
readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

: "${UTA_QWEN_ASR_SOURCE_DIR:?set UTA_QWEN_ASR_SOURCE_DIR to the pinned transcribe.cpp checkout}"
: "${UTA_QWEN_ALIGN_SOURCE_DIR:?set UTA_QWEN_ALIGN_SOURCE_DIR to the pinned qwen3-asr.cpp checkout}"
: "${UTA_QWEN_GGML_SOURCE_DIR:?set UTA_QWEN_GGML_SOURCE_DIR to the pinned GGML checkout}"
: "${UTA_QWEN_GGML_LIB_DIR:?set UTA_QWEN_GGML_LIB_DIR to the pinned GGML library directory}"
: "${UTA_QWEN_VULKAN_LIB_DIR:?set UTA_QWEN_VULKAN_LIB_DIR to the pinned Vulkan plugin directory}"
asr_source="${UTA_QWEN_ASR_SOURCE_DIR}"
align_source="${UTA_QWEN_ALIGN_SOURCE_DIR}"
ggml_source="${UTA_QWEN_GGML_SOURCE_DIR}"
ggml_lib="${UTA_QWEN_GGML_LIB_DIR}"
vulkan_lib="${UTA_QWEN_VULKAN_LIB_DIR}"
build="${UTA_QWEN_BUILD_DIR:-${HOME}/.cache/uta-studio/native-runtime/build/qwen-native-v1}"
jobs="${UTA_QWEN_BUILD_JOBS:-$(nproc)}"

for tool in git g++ gcc patch; do
    command -v "${tool}" >/dev/null || { printf 'missing build tool: %s\n' "${tool}" >&2; exit 2; }
done
for spec in "${asr_source}:${ASR_COMMIT}" "${align_source}:${ALIGN_COMMIT}" "${ggml_source}:${GGML_COMMIT}"; do
    source_path="${spec%:*}"; expected="${spec##*:}"
    actual="$(git -C "${source_path}" rev-parse HEAD 2>/dev/null || true)"
    [[ "${actual}" == "${expected}" ]] || {
        printf 'source identity mismatch for %s: %s\n' "${source_path}" "${actual}" >&2
        exit 3
    }
done
for library in libggml.so.0 libggml-cpu.so.0 libggml-base.so.0; do
    [[ -f "${ggml_lib}/${library}" ]] || { printf 'pinned GGML Vulkan build is incomplete\n' >&2; exit 2; }
done
[[ -f "${vulkan_lib}/libggml-vulkan.so.0" ]] || { printf 'pinned GGML Vulkan plugin is unavailable\n' >&2; exit 2; }

rm -rf "${build}"
mkdir -p "${build}/asr-obj" "${build}/align-obj" "${build}/bin"
align_overlay="${build}/predict-woo-source"
cp -a "${align_source}" "${align_overlay}"
patch -d "${align_overlay}" -p1 --forward < "${SCRIPT_DIR}/patches/predict-woo-require-gpu.patch"
patch -d "${align_overlay}" -p1 --forward < "${SCRIPT_DIR}/patches/predict-woo-fix-alignment-json-buffer-truncation.patch"

compile_parallel() {
    local standard="$1" object_dir="$2"; shift 2
    local -a common=("${@:1:$(($# - 1))}")
    local source_list="${!#}"
    local running=0
    while IFS= read -r source; do
        [[ -n "${source}" ]] || continue
        local relative="${source#/}"
        local object="${object_dir}/${relative//\//__}.o"
        if [[ "${source}" == *.c ]]; then
            gcc -std=c11 "${common[@]}" -w -c "${source}" -o "${object}" &
        else
            g++ "-std=${standard}" "${common[@]}" -c "${source}" -o "${object}" &
        fi
        ((running+=1))
        if (( running >= jobs )); then wait -n; ((running-=1)); fi
    done < "${source_list}"
    wait
}

asr_sources="${build}/asr-sources.txt"
find "${asr_source}/src" -type f \( -name '*.cpp' -o -name '*.c' \) | sort > "${asr_sources}"
printf '%s\n' "${asr_source}/examples/common/wav.cpp" "${asr_source}/examples/cli/main.cpp" >> "${asr_sources}"
asr_common=(-O3 -march=native -fPIC -pthread -I"${asr_source}/include" -I"${asr_source}/src" -I"${asr_source}/ggml/include" -I"${asr_source}/ggml/src" -I"${asr_source}/examples/common" -DTRANSCRIBE_BUILD -DTRANSCRIBE_STATIC '-DTRANSCRIBE_COMMIT="ea077b8"' -DMINIZ_NO_INFLATE_APIS -DMINIZ_NO_STDIO -DMINIZ_NO_TIME -DMINIZ_NO_ZLIB_COMPATIBLE_NAMES)
compile_parallel c++17 "${build}/asr-obj" "${asr_common[@]}" "${asr_sources}"
mapfile -t asr_objects < <(find "${build}/asr-obj" -name '*.o' | sort)
g++ -O3 -march=native -pthread -o "${build}/bin/transcribe-cli" "${asr_objects[@]}" \
    -L"${ggml_lib}" -L"${vulkan_lib}" -Wl,--no-as-needed \
    -lggml -lggml-cpu -lggml-vulkan -lggml-base -ldl -lm \
    -Wl,-rpath,"${ggml_lib}:${vulkan_lib}"

align_sources="${build}/align-sources.txt"
printf '%s\n' \
    "${align_overlay}/src/audio_encoder.cpp" "${align_overlay}/src/audio_injection.cpp" \
    "${align_overlay}/src/forced_aligner.cpp" "${align_overlay}/src/gguf_loader.cpp" \
    "${align_overlay}/src/mel_spectrogram.cpp" "${align_overlay}/src/qwen3_asr.cpp" \
    "${align_overlay}/src/text_decoder.cpp" "${align_overlay}/cli/main.cpp" > "${align_sources}"
align_common=(-O3 -march=native -fPIC -pthread -I"${ggml_source}/include" -I"${ggml_source}/src" -I"${align_overlay}/include" -I"${align_overlay}/src" -DQWEN3_ASR_TIMING)
compile_parallel c++20 "${build}/align-obj" "${align_common[@]}" "${align_sources}"
mapfile -t align_objects < <(find "${build}/align-obj" -name '*.o' | sort)
g++ -O3 -march=native -pthread -o "${build}/bin/qwen3-align-cli" "${align_objects[@]}" \
    -L"${ggml_lib}" -L"${vulkan_lib}" -Wl,--no-as-needed \
    -lggml -lggml-cpu -lggml-vulkan -lggml-base -ldl -lm \
    -Wl,-rpath,"${ggml_lib}:${vulkan_lib}"

sha256sum "${build}/bin/transcribe-cli" "${build}/bin/qwen3-align-cli"
printf 'Pinned Qwen native engines built at %s\n' "${build}/bin"
