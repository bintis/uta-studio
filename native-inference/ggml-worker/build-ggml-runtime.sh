#!/usr/bin/env bash
set -euo pipefail

# Explicit local source build. This script never downloads GGML, tools, or models.
readonly GGML_COMMIT="8c63e70982c95ceb862e3a1073a2c1beef75d60a"
readonly RECIPE_DIGEST="4c2784c0e58358f852ed9ee95cd7a5b99e4e6c226f72a4790e7beeb42f7d631a"
readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly ROFORMER_DIR="${SCRIPT_DIR}/../roformer"

: "${UTA_GGML_SOURCE_DIR:?set UTA_GGML_SOURCE_DIR to the pinned GGML checkout}"
ggml_source="${UTA_GGML_SOURCE_DIR}"
build="${UTA_GGML_BUILD_DIR:-${HOME}/.cache/uta-studio/native-runtime/build/ggml-vulkan-v1}"
destination="${UTA_GGML_RUNTIME_DIR:-${HOME}/.local/share/uta-studio/runtime/ggml-vulkan-v1}"
jobs="${UTA_GGML_BUILD_JOBS:-2}"

for tool in git cmake sha256sum patchelf; do
    command -v "${tool}" >/dev/null || { printf 'missing build tool: %s\n' "${tool}" >&2; exit 2; }
done
actual_commit="$(git -C "${ggml_source}" rev-parse HEAD 2>/dev/null || true)"
[[ "${actual_commit}" == "${GGML_COMMIT}" ]] || {
    printf 'GGML source identity mismatch: %s\n' "${actual_commit}" >&2
    exit 3
}
git -C "${ggml_source}" diff --quiet --ignore-submodules -- || {
    printf 'GGML source checkout has uncommitted runtime changes\n' >&2
    exit 3
}

rm -rf "${build}" "${destination}.staging"
cmake -S "${ROFORMER_DIR}" -B "${build}" \
    -DUTA_GGML_SOURCE_DIR="${ggml_source}" \
    -DGGML_VULKAN=ON -DGGML_CUDA=OFF -DGGML_SYCL=OFF -DGGML_NATIVE=OFF \
    -DCMAKE_BUILD_TYPE=Release
cmake --build "${build}" --target uta-roformer-runtime -j"${jobs}"

staging="${destination}.staging"
mkdir -p "${staging}/bin" "${staging}/lib"
cp -- "${build}/uta-roformer-runtime" "${staging}/bin/uta-roformer-runtime"
for library in libggml.so.0 libggml-base.so.0 libggml-cpu.so.0; do
    source_path="$(find "${build}/ggml" -type f -name "${library}*" | sort | tail -1)"
    [[ -n "${source_path}" ]] || { printf 'missing GGML library: %s\n' "${library}" >&2; exit 4; }
    cp -L -- "${source_path}" "${staging}/lib/${library}"
done
vulkan_path="$(find "${build}/ggml" -type f -name 'libggml-vulkan.so.0*' | sort | tail -1)"
[[ -n "${vulkan_path}" ]] || { printf 'missing GGML Vulkan library\n' >&2; exit 4; }
cp -L -- "${vulkan_path}" "${staging}/lib/libggml-vulkan.so.0"
patchelf --set-rpath '$ORIGIN/../lib' "${staging}/bin/uta-roformer-runtime"
for library in "${staging}"/lib/*.so.0; do
    patchelf --set-rpath '$ORIGIN' "${library}"
done

engine_sha="$(sha256sum "${staging}/bin/uta-roformer-runtime" | cut -d' ' -f1)"
{
    printf '{\n  "schema_version": 1,\n'
    printf '  "recipe_digest": "%s",\n' "${RECIPE_DIGEST}"
    printf '  "ggml_commit": "%s",\n' "${GGML_COMMIT}"
    printf '  "engine": {"path":"bin/uta-roformer-runtime","sha256":"%s"},\n' "${engine_sha}"
    printf '  "libraries": {\n'
    first=1
    for library in "${staging}"/lib/*.so.0; do
        name="$(basename "${library}")"
        digest="$(sha256sum "${library}" | cut -d' ' -f1)"
        (( first )) || printf ',\n'
        printf '    "lib/%s": "%s"' "${name}" "${digest}"
        first=0
    done
    printf '\n  }\n}\n'
} > "${staging}/runtime-manifest.json"

rm -rf "${destination}"
mv -- "${staging}" "${destination}"
printf 'Pinned GGML Vulkan runtime built at %s\n' "${destination}"
