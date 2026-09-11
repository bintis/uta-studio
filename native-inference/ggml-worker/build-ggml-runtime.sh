#!/usr/bin/env bash
set -euo pipefail

# Explicit local source build. The product runtime contains only upstream GGML
# shared libraries; every model graph and invocation is owned by Rust.
work="${UTA_STUDIO_GGML_WORK_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/uta-studio/native-runtime}"
ggml_source="${work}/source"
build="${UTA_GGML_BUILD_DIR:-${HOME}/.cache/uta-studio/native-runtime/build/ggml-vulkan}"
destination="${UTA_GGML_RUNTIME_DIR:-${HOME}/.local/share/uta-studio/runtime/ggml-vulkan}"
jobs="${UTA_GGML_BUILD_JOBS:-2}"

for tool in git cmake patchelf; do
    command -v "${tool}" >/dev/null || { printf 'missing build tool: %s\n' "${tool}" >&2; exit 2; }
done
mkdir -p "${work}"
source_staging="$(mktemp -d "${work}/source.XXXXXX")"
trap 'rm -rf -- "${source_staging}"' EXIT
if [[ -n "${UTA_GGML_SOURCE_DIR:-}" ]]; then
    # Copy an explicit checkout (including local edits) rather than patching or
    # resetting the operator's tree. There is no clean-tree or commit gate.
    cp -a "${UTA_GGML_SOURCE_DIR}"/. "${source_staging}/"
else
    git clone --depth 1 https://github.com/ggml-org/ggml.git "${source_staging}"
fi
rm -rf -- "${ggml_source}"
mv -- "${source_staging}" "${ggml_source}"
trap - EXIT
actual_commit="$(git -C "${ggml_source}" rev-parse HEAD)"

# Apply the declared backend fixes to the private current-source checkout.
# A conflicting patch must be rebased; never silently drop precision fixes or
# fetch an older upstream revision to make it apply.
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
trap 'rm -rf -- "${staging}"' EXIT
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
runtime_rpath="\$ORIGIN${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
for library in "${staging}"/lib/*; do
    patchelf --set-rpath "${runtime_rpath}" "${library}"
done

{
    printf '{\n  "source_repository": "ggml-org/ggml",\n  "source_commit": "%s",\n  "libraries": {\n' "${actual_commit}"
    first=1
    for library in "${staging}"/lib/*; do
        name="$(basename "${library}")"
        (( first )) || printf ',\n'
        printf '    "lib/%s": "source-build"' "${name}"
        first=0
    done
    printf '\n  }\n}\n'
} > "${staging}/runtime-manifest.json"

rm -rf "${destination}"
mv -- "${staging}" "${destination}"
trap - EXIT
printf 'Current-source GGML runtime (%s) built at %s\n' "${actual_commit}" "${destination}"
