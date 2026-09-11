#!/usr/bin/env bash
# Build the current upstream LibTorch source for Uta! Studio's native XPU route.
# Run explicitly inside bash dev.sh with Intel's SYCL compiler/oneMKL SDK in
# CMAKE_PREFIX_PATH/PATH. Python is an upstream build-time code generator only;
# BUILD_PYTHON=OFF and the installed worker never starts an interpreter.
#
# UTA_STUDIO_XPU_LIBRARY_DIRS: colon-separated SDK runtime library directories
# (SYCL/Unified Runtime, oneMKL, OpenMP/TBB and their provider resources).
# These are copied, not moved. System GPU drivers remain system-owned.
# UTA_STUDIO_LIBTORCH_SOURCE_DIR optionally supplies a local source checkout;
# otherwise acquire clones the latest upstream default branch, recursively.
# Only one source/build layout is used. No release wheel or version selector.
#
# acquire | build | manifest | all (default). Downloads occur only on acquire.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
native_source="$repo_root/native-inference/libtorch-runtime/native"
runtime_root="${UTA_STUDIO_LIBTORCH_RUNTIME_DIR:-$HOME/.local/share/uta-studio/runtime/libtorch-xpu}"
work_root="${UTA_STUDIO_LIBTORCH_WORK_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/uta-studio/libtorch-xpu}"
source_root="${UTA_STUDIO_LIBTORCH_SOURCE_DIR:-$work_root/source}"
source_repository="https://github.com/pytorch/pytorch.git"

log() { printf '[install-libtorch-xpu] %s\n' "$*" >&2; }

acquire() {
  if [ -n "${UTA_STUDIO_LIBTORCH_SOURCE_DIR:-}" ]; then
    log "using explicit source checkout without modifying it: $source_root"
    return
  fi
  mkdir -p "$work_root"
  local staging
  staging="$(mktemp -d "$work_root/source.XXXXXX")"
  # The remote's default branch, not a tag or a previously saved commit.
  if ! git clone --depth 1 --recurse-submodules --shallow-submodules "$source_repository" "$staging"; then
    rm -rf -- "$staging"
    return 1
  fi
  rm -rf -- "$work_root/source"
  mv -- "$staging" "$work_root/source"
}

copy_sdk_libraries() {
  local directory
  local directories=()
  IFS=: read -r -a directories <<< "${UTA_STUDIO_XPU_LIBRARY_DIRS:?set SDK runtime library directories for SYCL and oneMKL}"
  mkdir -p "$runtime_root/deps/lib"
  for directory in "${directories[@]}"; do
    cp -a "$directory"/. "$runtime_root/deps/lib/"
  done
  # Do not prune to DT_NEEDED: oneDNN/SYCL load providers, OpenCL and device
  # images lazily. Preserve those resources and compact only identical aliases.
  bash "$repo_root/native-inference/libtorch-runtime/compact-native-libraries.sh" "$runtime_root/deps/lib"
}

build() {
  local compiler="${CXX:-icpx}" prefix="$work_root/install"
  local actual_commit
  actual_commit="$(git -C "$source_root" rev-parse HEAD)"
  mkdir -p "$work_root" "$runtime_root/torch/lib" "$runtime_root/lib"
  copy_sdk_libraries
  # ATen's CPU library owns common dispatch/host operations even for XPU.
  # Removing that library is not equivalent to disabling a CPU fallback.
  XPU_ENABLE_KINETO=0 cmake -S "$source_root" -B "$work_root/torch-build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_CXX_COMPILER="$compiler" \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DCMAKE_INSTALL_RPATH="$runtime_root/torch/lib;$runtime_root/deps/lib" \
    -DBUILD_SHARED_LIBS=ON \
    -DBUILD_PYTHON=OFF \
    -DBUILD_TEST=OFF \
    -DBUILD_BINARY=OFF \
    -DBUILD_FUNCTORCH=OFF \
    -DUSE_CUDA=OFF \
    -DUSE_CUDNN=OFF \
    -DUSE_CUSPARSELT=OFF \
    -DUSE_NCCL=OFF \
    -DUSE_XCCL=OFF \
    -DUSE_ROCM=OFF \
    -DUSE_XPU=ON \
    -DUSE_DISTRIBUTED=OFF \
    -DUSE_MPI=OFF \
    -DUSE_GLOO=OFF \
    -DUSE_TENSORPIPE=OFF \
    -DUSE_FBGEMM=OFF \
    -DUSE_PYTORCH_QNNPACK=OFF \
    -DUSE_NNPACK=OFF \
    -DUSE_KINETO=OFF \
    -DUSE_ITT=OFF \
    -DUSE_MKLDNN=ON
  cmake --build "$work_root/torch-build" --target install \
    -j "${UTA_STUDIO_LIBTORCH_BUILD_JOBS:-4}"
  # Copy runtime DSOs only; headers, CMake exports and codegen stay build-only.
  local library
  while IFS= read -r -d '' library; do
    cp -a "$library" "$runtime_root/torch/lib/"
  done < <(find "$prefix/lib" -maxdepth 1 \( -type f -o -type l \) -name '*.so*' -print0)
  bash "$repo_root/native-inference/libtorch-runtime/compact-native-libraries.sh" "$runtime_root/torch/lib"
  cmake -S "$native_source" -B "$work_root/build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DTORCH_ROOT="$runtime_root/torch" \
    -DTORCH_INCLUDE_ROOT="$prefix/include" \
    -DXPU_DEPENDENCY_LIB="$runtime_root/deps/lib" \
    -DUTA_LIBTORCH_BACKEND=xpu \
    -DTORCH_CXX_ABI=1
  cmake --build "$work_root/build" --target uta_libtorch \
    -j "${UTA_STUDIO_LIBTORCH_BUILD_JOBS:-4}"
  cp "$work_root/build/libuta_libtorch.so" "$runtime_root/lib/libuta_libtorch.so"
  # Provenance only, never compared against an allowed commit/release/hash.
  printf '%s\n' "$actual_commit" > "$runtime_root/source-commit.txt"
  git -C "$source_root" status --porcelain > "$runtime_root/source-status.txt"
  git -C "$source_root" submodule status --recursive > "$runtime_root/source-submodules.txt"
  log "built source $actual_commit at $runtime_root"
}

json_string() {
  printf '"%s"' "$(printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
}

manifest() {
  local actual_commit
  IFS= read -r actual_commit < "$runtime_root/source-commit.txt"
  local library_path="$runtime_root/deps/lib:$runtime_root/torch/lib:/run/opengl-driver/lib"
  {
    printf '{\n  "backend": "libtorch_xpu",\n'
    printf '  "source_repository": %s,\n' "$(json_string "$source_repository")"
    printf '  "source_commit": %s,\n' "$(json_string "$actual_commit")"
    printf '  "native_library": "lib/libuta_libtorch.so",\n'
    printf '  "library_search_paths": [%s, %s],\n' "$(json_string "$runtime_root/torch/lib")" "$(json_string "$runtime_root/deps/lib")"
    printf '  "environment": {\n'
    printf '    "ONEAPI_DEVICE_SELECTOR": "level_zero:gpu",\n'
    printf '    "SYCL_CACHE_PERSISTENT": "1",\n'
    printf '    "SYCL_CACHE_DIR": %s,\n' "$(json_string "$runtime_root/sycl-cache")"
    printf '    "ONEDNN_DEFAULT_FPMATH_MODE": "strict",\n'
    printf '    "DNNL_DEFAULT_FPMATH_MODE": "strict",\n'
    printf '    "LD_LIBRARY_PATH": %s' "$(json_string "$library_path")"
    if [ -d /run/opengl-driver/etc/OpenCL/vendors ]; then
      printf ',\n    "OCL_ICD_VENDORS": "/run/opengl-driver/etc/OpenCL/vendors"'
    fi
    printf '\n  }\n}\n'
  } > "$runtime_root/runtime-manifest.json.tmp"
  mkdir -p "$runtime_root/sycl-cache"
  mv "$runtime_root/runtime-manifest.json.tmp" "$runtime_root/runtime-manifest.json"
}

case "${1:-all}" in
  acquire) acquire ;;
  build) build ;;
  manifest) manifest ;;
  all) acquire; build; manifest ;;
  *) printf 'usage: install-libtorch-xpu-runtime.sh [acquire|build|manifest|all]\n' >&2; exit 2 ;;
esac
