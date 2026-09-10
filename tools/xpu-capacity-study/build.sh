#!/usr/bin/env bash
set -euo pipefail
root=$(pwd)
torch_root=${UTA_STUDIO_XPU_TORCH_ROOT:-$root/test-artifacts/libtorch-xpu-isolated/native/torch}
deps_root=${UTA_STUDIO_XPU_DEPS_ROOT:-$root/test-artifacts/libtorch-xpu-isolated/venv/lib}
level_root=${UTA_STUDIO_XPU_LEVEL_ROOT:-/nix/store/yfvm0a8avc10lw18ps7xp7ym5smh8kn0-level-zero-1.32.0}
output=${UTA_STUDIO_XPU_OUTPUT:-$root/test-artifacts/xpu-capacity-study/build}
mkdir -p "$output"
export LD_LIBRARY_PATH="$torch_root/lib:$deps_root:$level_root/lib:/run/opengl-driver/lib:${LD_LIBRARY_PATH:-}"
c++ -std=c++20 -O3 -pthread -D_GLIBCXX_USE_CXX11_ABI=1 \
    -I"$torch_root/include" -I"$torch_root/include/torch/csrc/api/include" \
    tools/xpu-capacity-study/benchmark.cpp -L"$torch_root/lib" -L"$deps_root" \
    -Wl,-rpath,"$torch_root/lib" -Wl,-rpath,"$deps_root" -Wl,-rpath-link,"$deps_root" \
    -Wl,--no-as-needed -ltorch -ltorch_cpu -ltorch_xpu -lc10 -lc10_xpu -ldl \
    -o "$output/benchmark"
c++ -std=c++20 -O2 -I"$level_root/include" tools/xpu-capacity-study/metrics.cpp \
    -L"$level_root/lib" -Wl,-rpath,"$level_root/lib" -lze_loader -o "$output/metrics"
c++ -std=c++20 -O2 -I"$level_root/include" tools/xpu-capacity-study/sample.cpp \
    -L"$level_root/lib" -Wl,-rpath,"$level_root/lib" -lze_loader -o "$output/sample"
printf 'BUILT %s\n' "$output"
ldd "$output/benchmark"
