#!/usr/bin/env bash
# Explicit per-process environment; never installs or modifies a runtime.
set -euo pipefail
root="$(realpath "${1:?private probe root}")"
backend="${2:?torch or ggml}"
shift 2
export OMP_NUM_THREADS=2 MKL_NUM_THREADS=2
unset UTA_STUDIO_GGML_F32_MATMUL UTA_STUDIO_GGML_FA_QUERY_OWNED
unset UTA_STUDIO_GGML_FA_SHARED_GROUPS UTA_STUDIO_GGML_FA_QUERY_ROWS
unset UTA_STUDIO_GGML_FA_KEY_COLUMNS UTA_STUDIO_GGML_FA_STAGE_KV
unset UTA_STUDIO_GGML_FA_SMALL_SUBGROUP UTA_STUDIO_GGML_FA_SG32
case "$backend" in
  torch)
    export LD_LIBRARY_PATH="$root/native/torch/lib:$root/venv/lib:/run/opengl-driver/lib:${UTA_PROBE_LEVEL_ZERO_LIB_DIR:-/run/opengl-driver/lib}:${UTA_PROBE_OPENCL_LIB_DIR:-/run/opengl-driver/lib}:${LD_LIBRARY_PATH:-}"
    export OCL_ICD_VENDORS="${UTA_PROBE_OPENCL_VENDORS:-/run/opengl-driver/etc/OpenCL/vendors}"
    export ONEAPI_DEVICE_SELECTOR=level_zero:gpu
    unset SYCL_DEVICE_FILTER TORCH_ALLOW_TF32_CUBLAS_OVERRIDE
    export SYCL_CACHE_PERSISTENT=1 SYCL_CACHE_DIR="$root/sycl-cache"
    export ONEDNN_DEFAULT_FPMATH_MODE=strict DNNL_DEFAULT_FPMATH_MODE=strict
    export ONEDNN_VERBOSE="${UTA_PROBE_VERBOSE:-0}"
    exec "$root/cmake-build/torch-probe" "$@"
    ;;
  ggml)
    export UTA_TEST_GGML_RUNTIME_DIR="$root/ggml-reference/lib"
    export VK_DRIVER_FILES="${UTA_PROBE_VULKAN_ICD:-/run/opengl-driver/share/vulkan/icd.d/intel_icd.x86_64.json}"
    export GGML_VK_PERF_LOGGER=1
    exec "$root/cmake-build/ggml-probe" "$@"
    ;;
  *) printf 'Unknown probe backend: %s\n' "$backend" >&2; exit 2 ;;
esac
