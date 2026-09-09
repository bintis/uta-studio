#!/usr/bin/env bash
set -euo pipefail

# Explicit local source build. The product runtime contains only upstream GGML
# shared libraries; every model graph and invocation is owned by Rust.
readonly GGML_COMMIT="8c63e70982c95ceb862e3a1073a2c1beef75d60a"

: "${UTA_GGML_SOURCE_DIR:?set UTA_GGML_SOURCE_DIR to the pinned GGML checkout}"
ggml_source="${UTA_GGML_SOURCE_DIR}"
build="${UTA_GGML_BUILD_DIR:-${HOME}/.cache/uta-studio/native-runtime/build/ggml-vulkan}"
destination="${UTA_GGML_RUNTIME_DIR:-${HOME}/.local/share/uta-studio/runtime/ggml-vulkan}"
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

# Local patches. The runtime is upstream GGML plus exactly the patches this
# recipe declares; each one is applied to the verified checkout after the
# identity check above. The resulting runtime is accepted by its stable
# library roles and required ABI symbols rather than a mutable recipe version.
readonly patch_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/patches"
if [[ -d "${patch_dir}" ]]; then
    shopt -s nullglob
    for patch in "${patch_dir}"/*.patch; do
        printf 'applying local GGML patch: %s\n' "$(basename "${patch}")"
        git -C "${ggml_source}" apply --whitespace=nowarn -- "${patch}" || {
            printf 'could not apply local GGML patch: %s\n' "${patch}" >&2
            exit 3
        }
    done
    shopt -u nullglob
fi
restore_ggml_source() {
    git -C "${ggml_source}" checkout -- . 2>/dev/null || true
}
trap restore_ggml_source EXIT

rm -rf "${build}" "${destination}.staging"
# Keep GGML's embedded source provenance consistent across local builds.
GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.abbrev GIT_CONFIG_VALUE_0=12 \
cmake -S "${ggml_source}" -B "${build}" \
    -DBUILD_SHARED_LIBS=ON \
    -DGGML_BACKEND_DL=ON \
    -DGGML_CPU=ON \
    -DGGML_VULKAN=ON \
    -DGGML_CUDA=OFF \
    -DGGML_SYCL=OFF \
    -DGGML_NATIVE=OFF \
    -DCMAKE_BUILD_TYPE=Release
cmake --build "${build}" --target ggml -j"${jobs}"

staging="${destination}.staging"
trap 'rm -rf -- "${staging}"; restore_ggml_source' EXIT
mkdir -p "${staging}/lib"
copy_library() {
    local pattern="$1" destination_name="$2"
    local source_path
    source_path="$(find "${build}" -type f -name "${pattern}" | sort | tail -1)"
    [[ -n "${source_path}" ]] || { printf 'missing GGML library: %s\n' "${pattern}" >&2; exit 4; }
    cp -L -- "${source_path}" "${staging}/lib/${destination_name}"
}
copy_library 'libggml.so.0*' 'libggml.so.0'
copy_library 'libggml-base.so.0*' 'libggml-base.so.0'
copy_library 'libggml-cpu.so*' 'libggml-cpu.so'
copy_library 'libggml-vulkan.so*' 'libggml-vulkan.so'
for library in "${staging}"/lib/*; do
    patchelf --set-rpath '$ORIGIN' "${library}"
done

{
    printf '{\n  "libraries": {\n'
    first=1
    for library in "${staging}"/lib/*; do
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
restore_ggml_source
trap - EXIT
printf 'Pinned GGML shared-library runtime built at %s\n' "${destination}"
