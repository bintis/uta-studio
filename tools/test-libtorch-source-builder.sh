#!/usr/bin/env bash
# CPU-only packaging/control-flow fixture. Fake CMake outputs are not runtime
# qualification; no compiler, accelerator API, download or installed asset used.
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/uta-studio-source-build.XXXXXX")"
trap 'rm -rf -- "$fixture"' EXIT
mkdir -p "$fixture/bin" "$fixture/sdk/providers" "$fixture/source" "$fixture/work/install/lib"
printf 'stale component\n' > "$fixture/work/install/lib/unused.so"
printf 'lazy device resource\n' > "$fixture/sdk/providers/kernels.spv"
printf 'source is read-only\n' > "$fixture/source/marker"
export UTA_STUDIO_LIBTORCH_RUNTIME_DIR="$fixture/runtime"
export UTA_STUDIO_LIBTORCH_WORK_DIR="$fixture/work"
export UTA_STUDIO_LIBTORCH_SOURCE_DIR="$fixture/source"
export UTA_STUDIO_XPU_LIBRARY_DIRS="$fixture/sdk"
export UTA_STUDIO_SOURCE_FIXTURE="$fixture"
cat > "$fixture/bin/git" <<'TOOL'
#!/usr/bin/env bash
case "$*" in
  *'rev-parse HEAD') printf 'fixture-source-commit\n' ;;
  *'status --porcelain') ;;
  *'submodule status --recursive') printf ' fixture-submodule\n' ;;
  *) exit 1 ;;
esac
TOOL
cat > "$fixture/bin/cmake" <<'TOOL'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$UTA_STUDIO_SOURCE_FIXTURE/commands"
if [ "$1" = --build ]; then
  if [[ "$*" == *'--target install'* ]]; then
    mkdir -p "$UTA_STUDIO_LIBTORCH_WORK_DIR/install/lib" "$UTA_STUDIO_LIBTORCH_WORK_DIR/install/include"
    printf 'fixture DSO\n' > "$UTA_STUDIO_LIBTORCH_WORK_DIR/install/lib/libtorch.so"
  else
    mkdir -p "$2"
    printf 'fixture native DSO\n' > "$2/libuta_libtorch.so"
  fi
fi
TOOL
chmod +x "$fixture/bin/git" "$fixture/bin/cmake"
PATH="$fixture/bin:$PATH" bash "$repo_root/native-inference/libtorch-runtime/install-libtorch-xpu-runtime.sh" all
[ -f "$fixture/runtime/lib/libuta_libtorch.so" ]
[ -f "$fixture/runtime/torch/lib/libtorch.so" ]
[ -f "$fixture/runtime/deps/lib/providers/kernels.spv" ]
[ ! -e "$fixture/runtime/torch/lib/unused.so" ]
[ ! -e "$fixture/runtime/torch/include" ]
[ ! -e "$fixture/runtime/build" ]
[ ! -e "$fixture/runtime/downloads" ]
grep -qx 'source is read-only' "$fixture/source/marker"
grep -q '"source_commit": "fixture-source-commit"' "$fixture/runtime/runtime-manifest.json"
for option in USE_CUDA USE_ROCM BUILD_PYTHON BUILD_TEST USE_DISTRIBUTED USE_XCCL USE_KINETO; do
  grep -q -- "-D$option=OFF" "$fixture/commands"
done
grep -q -- '-DUSE_XPU=ON' "$fixture/commands"
grep -q -- '-DUSE_MKLDNN=ON' "$fixture/commands"
printf 'source builder layout and trimming options fixture passed (mock build only)\n'
