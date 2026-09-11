#!/usr/bin/env bash
# Installs the native LibTorch XPU runtime that Uta! Studio's packaged worker
# loads for the explicit `libtorch_xpu` route.
#
# No Python is involved at any step. The official release wheels are plain zip
# archives: they are fetched with curl, unpacked with unzip, and only their
# native shared libraries and C++ headers are kept. The app-owned native
# library is then built with CMake from native-inference/libtorch-runtime/native.
#
# Layout of the installed runtime directory (default
# ~/.local/share/uta-studio/runtime/libtorch-xpu, override with
# UTA_STUDIO_LIBTORCH_RUNTIME_DIR):
#   downloads/            retained official wheel archives and SHA256SUMS
#   torch/include, torch/lib   LibTorch headers and shared libraries
#   deps/lib              Intel SYCL, oneMKL, UR, Level Zero loader, ... libraries
#   build/                CMake build tree of the app-owned native library
#   lib/libuta_libtorch.so     the library the worker loads
#   runtime-manifest.json      backend identity, library digests, environment
#
# Subcommands: acquire | unpack | build | manifest | all (default: all).
# Run `build` inside `bash dev.sh` (CMake, Ninja and the compiler live there);
# `acquire` and `unpack` need only curl, unzip and sha256sum.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
native_source="$repo_root/native-inference/libtorch-runtime/native"
runtime_root="${UTA_STUDIO_LIBTORCH_RUNTIME_DIR:-$HOME/.local/share/uta-studio/runtime/libtorch-xpu}"
index="${UTA_STUDIO_LIBTORCH_XPU_INDEX:-https://download.pytorch.org/whl/xpu}"
torch_release="2.13.0+xpu"
torch_wheel="torch-2.13.0+xpu-cp311-cp311-manylinux_2_28_x86_64.whl"
# Native dependency wheels pinned by the torch wheel's own Requires-Dist list.
# Packages that ship only Python code, licenses or headers are not needed.
dependencies=(
  "intel-sycl-rt=2026.0.0"
  "intel-cmplr-lib-rt=2026.0.0"
  "intel-cmplr-lib-ur=2026.0.0"
  "intel-openmp=2026.0.0"
  "intel-pti=0.17.0"
  "umf=1.1.0"
  "tcmlib=1.5.0"
  "tbb=2023.0.0"
  "oneccl=2022.0.0"
  "impi-rt=2021.18.0"
  "mkl=2026.0.0"
  "onemkl-sycl-blas=2026.0.0"
  "onemkl-sycl-dft=2026.0.0"
  "onemkl-sycl-lapack=2026.0.0"
  "onemkl-sycl-rng=2026.0.0"
  "onemkl-sycl-sparse=2026.0.0"
)

log() { printf '[install-libtorch-xpu] %s\n' "$*" >&2; }
fail() { log "$*"; exit 1; }

fetch() {
  # fetch URL DESTINATION: one explicit transfer split into bounded parallel
  # byte ranges (the release CDN throttles a single connection), then joined.
  # No retry loop: a failed part fails the whole fetch and leaves nothing behind.
  local url="$1" destination="$2" size parts workers
  if [ -s "$destination" ]; then
    log "retained $(basename "$destination")"
    return 0
  fi
  size="$(curl --fail --location --silent --show-error --head "$url" \
    | tr -d '\r' | grep -i '^content-length:' | tail -n 1 | awk '{print $2}')"
  [ -n "$size" ] && [ "$size" -gt 0 ] || fail "cannot determine size of $url"
  workers="${UTA_STUDIO_LIBTORCH_FETCH_WORKERS:-32}"
  parts="$destination.parts"
  rm -rf "$parts"
  mkdir -p "$parts"
  log "fetching $url ($size bytes, $workers parallel ranges)"
  local block=$((4 * 1024 * 1024)) offset=0 index=0
  while [ "$offset" -lt "$size" ]; do
    local end=$((offset + block - 1))
    [ "$end" -ge "$size" ] && end=$((size - 1))
    printf '%08d %d %d\n' "$index" "$offset" "$end"
    offset=$((end + 1))
    index=$((index + 1))
  done > "$parts/ranges"
  xargs -P "$workers" -L 1 sh -c \
    'curl --fail --location --silent --show-error --range "$2-$3" --output "$0/$1" "$4"' \
    "$parts" < <(awk -v url="$url" '{print $1, $2, $3, url}' "$parts/ranges") \
    || fail "a byte range of $url failed"
  cat "$parts"/[0-9]* > "$destination.partial"
  local actual
  actual="$(stat -c %s "$destination.partial")"
  [ "$actual" = "$size" ] || fail "assembled $actual bytes, expected $size for $url"
  rm -rf "$parts"
  mv "$destination.partial" "$destination"
}

wheel_url() {
  # wheel_url NAME VERSION: locate the Linux x86_64 wheel on the index page.
  local name="$1" version="$2" normalized page candidates
  normalized="${name//-/_}"
  page="$(curl --fail --location --silent --show-error "$index/$name/")"
  candidates="$(printf '%s\n' "$page" \
    | grep -o 'href="[^"]*"' \
    | sed -e 's/^href="//' -e 's/"$//' -e 's/#.*$//' \
    | grep -E "/${normalized}-${version}-[^/]*manylinux[^/]*x86_64\.whl$" || true)"
  [ -n "$candidates" ] || fail "no Linux x86_64 wheel for $name $version on $index"
  # Prefer the newest manylinux tag when several are published.
  printf '%s\n' "$candidates" | sort -V | tail -n 1
}

acquire() {
  mkdir -p "$runtime_root/downloads"
  fetch "$index/torch-2.13.0%2Bxpu-cp311-cp311-manylinux_2_28_x86_64.whl" \
    "$runtime_root/downloads/$torch_wheel"
  local entry name version url filename
  for entry in "${dependencies[@]}"; do
    name="${entry%%=*}"
    version="${entry#*=}"
    filename="$(ls "$runtime_root/downloads" 2>/dev/null \
      | grep -E "^${name//-/_}-${version}-.*x86_64\.whl$" | head -n 1 || true)"
    if [ -n "$filename" ]; then
      log "retained $filename"
      continue
    fi
    url="$(wheel_url "$name" "$version")"
    case "$url" in
      http://*|https://*) ;;
      /*) url="https://download.pytorch.org$url" ;;
      *) url="$index/$name/$url" ;;
    esac
    fetch "$url" "$runtime_root/downloads/$(basename "$url")"
  done
  (cd "$runtime_root/downloads" && sha256sum ./*.whl > SHA256SUMS)
  log "acquired $(ls "$runtime_root/downloads"/*.whl | wc -l) wheel archives"
}

unpack() {
  local staging="$runtime_root/staging"
  rm -rf "$staging" "$runtime_root/torch" "$runtime_root/deps"
  mkdir -p "$staging" "$runtime_root/deps/lib"
  log "unpacking LibTorch headers and libraries"
  unzip -q -o "$runtime_root/downloads/$torch_wheel" \
    'torch/include/*' 'torch/lib/*' 'torch/share/cmake/*' -d "$staging/torch-wheel"
  mkdir -p "$runtime_root/torch"
  mv "$staging/torch-wheel/torch/include" "$runtime_root/torch/include"
  mv "$staging/torch-wheel/torch/lib" "$runtime_root/torch/lib"
  mv "$staging/torch-wheel/torch/share" "$runtime_root/torch/share"
  # The Python binding library needs a Python interpreter and is never loaded.
  rm -f "$runtime_root/torch/lib/libtorch_python.so"
  local wheel
  for wheel in "$runtime_root/downloads"/*.whl; do
    [ "$(basename "$wheel")" = "$torch_wheel" ] && continue
    local name
    name="$(basename "$wheel" .whl)"
    log "unpacking native libraries from $name"
    unzip -q -o "$wheel" -d "$staging/$name"
    # Wheel data trees place native libraries under */lib; merge every such
    # tree (including provider subdirectories) into one dependency directory.
    while IFS= read -r -d '' directory; do
      cp -a "$directory"/. "$runtime_root/deps/lib/"
    done < <(find "$staging/$name" -type d -name lib -print0)
    # Some wheels also carry libraries beside their package modules.
    while IFS= read -r -d '' library; do
      case "$library" in */lib/*) continue ;; esac
      cp -a "$library" "$runtime_root/deps/lib/"
    done < <(find "$staging/$name" -type f -name '*.so*' -print0)
  done
  rm -rf "$staging"
  # Python extension modules and bytecode are never loaded; keep only native
  # runtime libraries in the dependency directory.
  find "$runtime_root/deps/lib" \( -name '*.py' -o -name '*.pyc' -o -name '*.cpython-*.so' \) -delete
  ensure_level_zero_loader
  ensure_opencl_loader
  log "native dependency libraries: $(find "$runtime_root/deps/lib" -name '*.so*' | wc -l)"
}

ensure_level_zero_loader() {
  # The Unified Runtime Level Zero adapter loads libze_loader.so.1 by name.
  # The Intel wheels above do not ship it; take it from an explicit root or
  # the system's Level Zero package.
  if ls "$runtime_root/deps/lib"/libze_loader.so.1* >/dev/null 2>&1; then
    return 0
  fi
  local root="${UTA_STUDIO_LEVEL_ZERO_ROOT:-}"
  if [ -z "$root" ]; then
    root="$(ls -d /nix/store/*-level-zero-*/ 2>/dev/null | grep -v '\.drv' | sort -V | tail -n 1 || true)"
  fi
  if [ -n "$root" ] && ls "$root"/lib/libze_loader.so* >/dev/null 2>&1; then
    cp -a "$root"/lib/libze_loader.so* "$runtime_root/deps/lib/"
    log "Level Zero loader copied from $root"
  else
    fail "libze_loader.so.1 is unavailable; set UTA_STUDIO_LEVEL_ZERO_ROOT to a Level Zero installation"
  fi
}

ensure_opencl_loader() {
  # oneDNN's fused attention microkernels open the OpenCL ICD loader by name
  # even when execution runs on the Level Zero stream. Take the system ICD
  # loader when the wheels do not provide one; vendors resolve through
  # OCL_ICD_VENDORS at run time.
  if ls "$runtime_root/deps/lib"/libOpenCL.so.1* >/dev/null 2>&1 \
    || ls "$runtime_root/torch/lib"/libOpenCL.so.1* >/dev/null 2>&1; then
    return 0
  fi
  local root="${UTA_STUDIO_OPENCL_LOADER_ROOT:-}"
  if [ -z "$root" ]; then
    root="$(ls -d /nix/store/*-ocl-icd-*/ 2>/dev/null | grep -v '\.drv' | sort -V | tail -n 1 || true)"
  fi
  if [ -n "$root" ] && ls "$root"/lib/libOpenCL.so* >/dev/null 2>&1; then
    cp -a "$root"/lib/libOpenCL.so* "$runtime_root/deps/lib/"
    log "OpenCL ICD loader copied from $root"
  else
    log "no OpenCL ICD loader found; fused oneDNN kernels needing OpenCL will report their own error"
  fi
}

resolve_system_dependencies() {
  # The unpacked libraries name a few ordinary system libraries (zlib, ...)
  # that the app-owned library's inherited RPATH must also satisfy outside the
  # development shell. Copy each unresolved DT_NEEDED name from the shell's
  # library path into the dependency directory; C/C++ runtime libraries are
  # provided by the app-owned library's own RPATH.
  command -v readelf >/dev/null || fail "readelf is unavailable; run build inside bash dev.sh"
  local present needed name candidate directory copied=0
  present="$(ls "$runtime_root/torch/lib" "$runtime_root/deps/lib" 2>/dev/null | sort -u)"
  needed="$(find "$runtime_root/torch/lib" "$runtime_root/deps/lib" -maxdepth 1 -type f -name '*.so*' -print0 \
    | xargs -0 -n 64 sh -c 'readelf -d "$@" 2>/dev/null || true' _ \
    | { grep -o 'Shared library: \[[^]]*\]' || true; } \
    | sed -e 's/^Shared library: \[//' -e 's/\]$//' | sort -u)"
  for name in $needed; do
    printf '%s\n' "$present" | grep -qx "$name" && continue
    case "$name" in
      libc.so.*|libm.so.*|libdl.so.*|libpthread.so.*|librt.so.*|libresolv.so.*|libutil.so.*|libanl.so.*|libnsl.so.*|libgcc_s.so.*|libstdc++.so.*|ld-linux*|libmvec.so.*) continue ;;
    esac
    candidate=""
    for directory in $(printf '%s' "${LD_LIBRARY_PATH:-}" | tr ':' '\n'); do
      if [ -f "$directory/$name" ]; then candidate="$directory/$name"; break; fi
    done
    if [ -z "$candidate" ]; then
      log "unresolved optional dependency $name (loaded lazily by its owner, if ever)"
      continue
    fi
    cp -L "$candidate" "$runtime_root/deps/lib/$name"
    log "system dependency $name copied from $(dirname "$candidate")"
    copied=$((copied + 1))
  done
  log "resolved $copied system dependencies"
}

build() {
  command -v cmake >/dev/null || fail "cmake is unavailable; run build inside bash dev.sh"
  [ -f "$runtime_root/torch/include/ATen/ATen.h" ] || fail "LibTorch headers are not unpacked; run unpack first"
  local generator=()
  if command -v ninja >/dev/null; then generator=(-G Ninja); fi
  cmake -S "$native_source" -B "$runtime_root/build" "${generator[@]}" \
    -DCMAKE_BUILD_TYPE=Release \
    -DTORCH_ROOT="$runtime_root/torch" \
    -DXPU_DEPENDENCY_LIB="$runtime_root/deps/lib" \
    -DUTA_LIBTORCH_BACKEND=xpu \
    -DTORCH_CXX_ABI=1
  cmake --build "$runtime_root/build" --target uta_libtorch uta-libtorch-contract-check \
    -j "${UTA_STUDIO_LIBTORCH_BUILD_JOBS:-4}"
  mkdir -p "$runtime_root/lib"
  cp -f "$runtime_root/build/libuta_libtorch.so" "$runtime_root/lib/libuta_libtorch.so"
  resolve_system_dependencies
  log "built $runtime_root/lib/libuta_libtorch.so"
}

json_string() {
  # Minimal JSON string escaping for paths and identifiers.
  printf '"%s"' "$(printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
}

manifest() {
  [ -f "$runtime_root/lib/libuta_libtorch.so" ] || fail "native library is not built; run build first"
  local manifest="$runtime_root/runtime-manifest.json" temporary
  temporary="$manifest.tmp"
  # Libraries the native stack opens lazily by name (the Level Zero loader,
  # the GPU driver it discovers, the OpenCL ICD loader oneDNN asks for) are
  # found only through the process library search path, so the manifest
  # declares the runtime's own library directories followed by the system
  # GPU driver directory. Naming the driver file directly instead
  # (ZE_ENABLE_ALT_DRIVERS) aborted inside the compute runtime on this host.
  local library_path="$runtime_root/deps/lib:$runtime_root/torch/lib" ocl_vendors=""
  if [ -f /run/opengl-driver/lib/libze_intel_gpu.so.1 ]; then
    library_path="$library_path:/run/opengl-driver/lib"
  fi
  if [ -d /run/opengl-driver/etc/OpenCL/vendors ]; then
    ocl_vendors="/run/opengl-driver/etc/OpenCL/vendors"
  fi
  {
    printf '{\n'
    printf '  "backend": "libtorch_xpu",\n'
    printf '  "torch_release": %s,\n' "$(json_string "$torch_release")"
    printf '  "native_library": "lib/libuta_libtorch.so",\n'
    printf '  "native_source_commit": %s,\n' "$(json_string "$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || echo unknown)")"
    printf '  "library_search_paths": [%s, %s],\n' "$(json_string "$runtime_root/torch/lib")" "$(json_string "$runtime_root/deps/lib")"
    printf '  "environment": {\n'
    printf '    "ONEAPI_DEVICE_SELECTOR": "level_zero:gpu",\n'
    printf '    "SYCL_CACHE_PERSISTENT": "1",\n'
    printf '    "SYCL_CACHE_DIR": %s,\n' "$(json_string "$runtime_root/sycl-cache")"
    printf '    "ONEDNN_DEFAULT_FPMATH_MODE": "strict",\n'
    printf '    "DNNL_DEFAULT_FPMATH_MODE": "strict"'
    printf ',\n    "LD_LIBRARY_PATH": %s' "$(json_string "$library_path")"
    if [ -n "$ocl_vendors" ]; then
      printf ',\n    "OCL_ICD_VENDORS": %s' "$(json_string "$ocl_vendors")"
    fi
    printf '\n  },\n'
    printf '  "libraries": {\n'
    local first=1 relative digest
    while IFS= read -r -d '' library; do
      relative="${library#"$runtime_root"/}"
      digest="$(sha256sum "$library" | cut -d ' ' -f 1)"
      if [ "$first" = 1 ]; then first=0; else printf ',\n'; fi
      printf '    %s: %s' "$(json_string "$relative")" "$(json_string "$digest")"
    done < <(find "$runtime_root/lib" "$runtime_root/torch/lib" -maxdepth 1 -type f -name '*.so*' -print0 | sort -z)
    printf '\n  }\n}\n'
  } > "$temporary"
  mkdir -p "$runtime_root/sycl-cache"
  mv "$temporary" "$manifest"
  log "wrote $manifest"
}

case "${1:-all}" in
  acquire) acquire ;;
  unpack) unpack ;;
  build) build ;;
  manifest) manifest ;;
  all) acquire; unpack; build; manifest ;;
  *) fail "usage: install-libtorch-xpu-runtime.sh [acquire|unpack|build|manifest|all]" ;;
esac
