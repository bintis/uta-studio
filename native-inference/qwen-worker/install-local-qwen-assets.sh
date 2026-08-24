#!/usr/bin/env bash
set -euo pipefail

# Explicit user/developer installation action. Existing model/runtime files are
# retained; this script never replaces them.
readonly ASR_SHA256="b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e"
readonly ALIGN_SHA256="c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b"
readonly GGML_COMMIT="8c63e70982c95ceb862e3a1073a2c1beef75d60a"
readonly ASR_COMMIT="ea077b87590bcfb090d7c38c03ab36cd1c7005d3"
readonly ALIGN_COMMIT="6dcc586e5073fd6e85ee5728e75f0903d6c70c6c"
readonly ASR_RECIPE="53083b7b39dd2a805f441453ae07c797"
readonly ALIGN_RECIPE="3ec367aaf3f723079851e2fbdbd375f8"

: "${UTA_QWEN_ASR_MODEL_SOURCE:?set UTA_QWEN_ASR_MODEL_SOURCE}"
: "${UTA_QWEN_ALIGN_MODEL_SOURCE:?set UTA_QWEN_ALIGN_MODEL_SOURCE}"
: "${UTA_QWEN_ASR_ENGINE_SOURCE:?set UTA_QWEN_ASR_ENGINE_SOURCE}"
: "${UTA_QWEN_ALIGN_ENGINE_SOURCE:?set UTA_QWEN_ALIGN_ENGINE_SOURCE}"
: "${UTA_QWEN_GGML_LIB_DIR:?set UTA_QWEN_GGML_LIB_DIR}"
: "${UTA_QWEN_VULKAN_LIB_DIR:?set UTA_QWEN_VULKAN_LIB_DIR}"

models_dir="${UTA_STUDIO_MODELS_PATH:-${HOME}/.local/share/uta-studio/models}"
runtime_dir="${UTA_STUDIO_QWEN_ENGINE_RUNTIME_DIR:-${HOME}/.local/share/uta-studio/runtime/qwen-native-v1}"
patchelf_bin="${PATCHELF:-patchelf}"
command -v "${patchelf_bin}" >/dev/null || {
    printf 'patchelf is required to make the local runtime relocatable\n' >&2
    exit 2
}

require_source() {
    local path="$1"
    [[ -f "${path}" ]] || { printf 'source is unavailable: %s\n' "${path}" >&2; exit 2; }
}

copy_model() {
    local source="$1" destination="$2"
    if [[ -e "${destination}" ]]; then
        require_source "${destination}"
        return
    fi
    mkdir -p "$(dirname "${destination}")"
    local temporary="${destination}.tmp.$$"
    trap 'rm -f -- "${temporary}"' RETURN
    cp --reflink=auto "${source}" "${temporary}"
    sync "${temporary}"
    mv "${temporary}" "${destination}"
    trap - RETURN
}

require_source "${UTA_QWEN_ASR_MODEL_SOURCE}"
require_source "${UTA_QWEN_ALIGN_MODEL_SOURCE}"
copy_model "${UTA_QWEN_ASR_MODEL_SOURCE}" \
    "${models_dir}/qwen-asr/Qwen3-ASR-1.7B-Q4_K_M.gguf"
copy_model "${UTA_QWEN_ALIGN_MODEL_SOURCE}" \
    "${models_dir}/qwen-align/qwen3-forced-aligner-predict-woo-f16.gguf"

align_install_manifest="${models_dir}/qwen-align/install-manifest.json"
if [[ ! -e "${align_install_manifest}" ]]; then
    cat >"${align_install_manifest}.tmp.$$" <<EOF
{"schema_version":1,"model_id":"qwen3_forced_aligner_0_6b","file":"qwen3-forced-aligner-predict-woo-f16.gguf","sha256":"${ALIGN_SHA256}"}
EOF
    sync "${align_install_manifest}.tmp.$$"
    mv "${align_install_manifest}.tmp.$$" "${align_install_manifest}"
fi

[[ ! -e "${runtime_dir}" ]] || {
    printf 'refusing to replace existing Qwen runtime: %s\n' "${runtime_dir}" >&2
    exit 4
}
staging="${runtime_dir}.tmp.$$"
trap 'rm -rf -- "${staging}"' EXIT
mkdir -p "${staging}/bin" "${staging}/lib"
cp "${UTA_QWEN_ASR_ENGINE_SOURCE}" "${staging}/bin/transcribe-cli"
cp "${UTA_QWEN_ALIGN_ENGINE_SOURCE}" "${staging}/bin/qwen3-align-cli"
for name in libggml.so.0 libggml-cpu.so.0 libggml-base.so.0; do
    cp -L "${UTA_QWEN_GGML_LIB_DIR}/${name}" "${staging}/lib/${name}"
done
cp -L "${UTA_QWEN_VULKAN_LIB_DIR}/libggml-vulkan.so.0" \
    "${staging}/lib/libggml-vulkan.so.0"

system_rpath="$({
    "${patchelf_bin}" --print-rpath "${UTA_QWEN_ASR_ENGINE_SOURCE}"
    for source in \
        "${UTA_QWEN_GGML_LIB_DIR}/libggml.so.0" \
        "${UTA_QWEN_GGML_LIB_DIR}/libggml-cpu.so.0" \
        "${UTA_QWEN_GGML_LIB_DIR}/libggml-base.so.0" \
        "${UTA_QWEN_VULKAN_LIB_DIR}/libggml-vulkan.so.0"; do
        "${patchelf_bin}" --print-rpath "${source}"
    done
} | tr ':' '\n' | grep '^/nix/store/' | sort -u | paste -sd: -)"
for engine in "${staging}/bin/transcribe-cli" "${staging}/bin/qwen3-align-cli"; do
    "${patchelf_bin}" --set-rpath "\$ORIGIN/../lib:${system_rpath}" "${engine}"
    chmod 0755 "${engine}"
done
for library in "${staging}"/lib/*.so.0; do
    "${patchelf_bin}" --set-rpath "\$ORIGIN:${system_rpath}" "${library}"
done

digest() { sha256sum "$1" | cut -d' ' -f1; }
cat >"${staging}/runtime-manifest.json" <<EOF
{
  "schema_version": 1,
  "ggml_commit": "${GGML_COMMIT}",
  "engines": {
    "qwen3_asr_1_7b": {
      "path": "bin/transcribe-cli",
      "sha256": "$(digest "${staging}/bin/transcribe-cli")",
      "source_commit": "${ASR_COMMIT}",
      "runtime_recipe_digest": "${ASR_RECIPE}"
    },
    "qwen3_forced_aligner_0_6b": {
      "path": "bin/qwen3-align-cli",
      "sha256": "$(digest "${staging}/bin/qwen3-align-cli")",
      "source_commit": "${ALIGN_COMMIT}",
      "runtime_recipe_digest": "${ALIGN_RECIPE}"
    }
  },
  "libraries": {
    "lib/libggml.so.0": "$(digest "${staging}/lib/libggml.so.0")",
    "lib/libggml-base.so.0": "$(digest "${staging}/lib/libggml-base.so.0")",
    "lib/libggml-cpu.so.0": "$(digest "${staging}/lib/libggml-cpu.so.0")",
    "lib/libggml-vulkan.so.0": "$(digest "${staging}/lib/libggml-vulkan.so.0")"
  }
}
EOF
sync -f "${staging}"
mv "${staging}" "${runtime_dir}"
trap - EXIT
printf 'Pinned Qwen models and native runtime installed successfully.\n'
