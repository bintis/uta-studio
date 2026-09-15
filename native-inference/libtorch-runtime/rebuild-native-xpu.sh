#!/usr/bin/env bash
# Rebuild only Uta! Studio's native bridge against the installed LibTorch SDK.
# No upstream acquisition, model execution, GPU probing, or dependency updates.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
runtime_root="${UTA_STUDIO_LIBTORCH_RUNTIME_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/uta-studio/runtime/libtorch-xpu}"
work_root="${UTA_STUDIO_LIBTORCH_WORK_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/uta-studio/libtorch-xpu}"
include_root="${UTA_STUDIO_LIBTORCH_INCLUDE_DIR:-}"
if [[ -z "$include_root" ]]; then
    for candidate in "$runtime_root/torch/include" "$work_root/install/include"; do
        if [[ -f "$candidate/ATen/ATen.h" ]]; then
            include_root="$candidate"
            break
        fi
    done
fi
if [[ ! -f "$include_root/ATen/ATen.h" ]]; then
    printf 'Native LibTorch headers are missing. Set UTA_STUDIO_LIBTORCH_INCLUDE_DIR to the headers matching the installed runtime. The installed library was not changed.\n' >&2
    exit 1
fi

# Configure on every explicit build: the native sources and their provenance
# must be refreshed even when Cargo sees no Rust changes. This builds the app
# bridge, not PyTorch itself; the installed dependencies remain read-only.
cmake -S "$repo_root/native-inference/libtorch-runtime/native" -B "$work_root/native-build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DTORCH_ROOT="$runtime_root/torch" \
    -DTORCH_INCLUDE_ROOT="$include_root" \
    -DXPU_DEPENDENCY_LIB="$runtime_root/deps/lib" \
    -DUTA_LIBTORCH_BACKEND=xpu \
    -DTORCH_CXX_ABI="${TORCH_CXX_ABI:-1}"
cmake --build "$work_root/native-build" --target uta_libtorch \
    -j "${UTA_STUDIO_LIBTORCH_BUILD_JOBS:-4}"

# Build-time file publication only; the helper never imports/loads LibTorch.
python3 "$repo_root/native-inference/libtorch-runtime/publish-native-xpu.py" \
    "$work_root/native-build" "$runtime_root"
printf '[native-xpu] Updated %s using headers %s; no models were executed.\n' \
    "$runtime_root/lib/libuta_libtorch.so" "$include_root" >&2
