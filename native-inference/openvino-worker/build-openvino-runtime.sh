#!/usr/bin/env bash
set -euo pipefail

# Explicit developer/user action only. Uta Studio never invokes this script on
# launch or during diagnostics.
readonly OPENVINO_TAG="2026.3.0"
readonly OPENVINO_COMMIT="8a17657b995fd3b4a52f8484acfcf2bb61214623"
readonly OPENVINO_URL="https://github.com/openvinotoolkit/openvino.git"
readonly RECIPE_SHA256="bd349389e6d0d0b742ae103892c1e5774599dd8733460aec80cb74bcf20ddab6"
readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

source_dir="${UTA_OPENVINO_SOURCE_DIR:-/tmp/openvino-${OPENVINO_TAG}}"
build_dir="${UTA_OPENVINO_BUILD_DIR:-${HOME}/.cache/uta-studio/native-runtime/build/openvino-${OPENVINO_TAG}}"
install_dir="${UTA_OPENVINO_INSTALL_DIR:-${HOME}/.local/share/uta-studio/runtime/openvino-${OPENVINO_TAG}}"
cmake_bin="${CMAKE:-cmake}"
ninja_bin="${NINJA:-ninja}"
cxx_bin="${CXX:-g++}"

for tool in git "${cmake_bin}" "${ninja_bin}" "${cxx_bin}"; do
    command -v "${tool}" >/dev/null 2>&1 || {
        printf 'required build tool is unavailable: %s\n' "${tool}" >&2
        exit 2
    }
done

if [[ ! -d "${source_dir}/.git" ]]; then
    git clone --filter=blob:none --depth 1 --branch "${OPENVINO_TAG}" \
        "${OPENVINO_URL}" "${source_dir}"
fi

actual_commit="$(git -C "${source_dir}" rev-parse HEAD)"
if [[ "${actual_commit}" != "${OPENVINO_COMMIT}" ]]; then
    printf 'OpenVINO source identity mismatch: expected %s, got %s\n' \
        "${OPENVINO_COMMIT}" "${actual_commit}" >&2
    exit 3
fi

# Initialize only dependencies needed by the native runtime, ONNX frontend,
# and Intel GPU plugin. CPU/NPU plugins and all script bindings stay disabled.
git -C "${source_dir}" submodule update --init --depth 1 -- \
    thirdparty/ittapi/ittapi \
    thirdparty/json/nlohmann_json \
    thirdparty/ocl/cl_headers \
    thirdparty/ocl/clhpp_headers \
    thirdparty/ocl/icd_loader \
    thirdparty/onnx/onnx \
    thirdparty/protobuf/protobuf \
    thirdparty/pugixml \
    thirdparty/snappy \
    thirdparty/telemetry \
    thirdparty/xbyak \
    thirdparty/zlib/zlib

mkdir -p "${build_dir}" "${install_dir}"
"${cmake_bin}" -S "${source_dir}" -B "${build_dir}" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_MAKE_PROGRAM="$(command -v "${ninja_bin}")" \
    -DCMAKE_INSTALL_PREFIX="${install_dir}" \
    -DCMAKE_INSTALL_RPATH='$ORIGIN' \
    -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
    -DBUILD_SHARED_LIBS=ON \
    -DBUILD_TESTING=OFF \
    -DENABLE_TESTS=OFF \
    -DENABLE_SAMPLES=OFF \
    -DENABLE_PYTHON=OFF \
    -DENABLE_JS=OFF \
    -DENABLE_DOCS=OFF \
    -DENABLE_INTEL_CPU=OFF \
    -DENABLE_INTEL_GPU=ON \
    -DENABLE_INTEL_NPU=OFF \
    -DENABLE_AUTO=OFF \
    -DENABLE_AUTO_BATCH=OFF \
    -DENABLE_MULTI=OFF \
    -DENABLE_HETERO=OFF \
    -DENABLE_TEMPLATE=OFF \
    -DENABLE_ONEDNN_FOR_GPU=OFF \
    -DENABLE_CM_FOR_GPU=OFF \
    -DENABLE_OPENCV=OFF \
    -DENABLE_OV_IR_FRONTEND=ON \
    -DENABLE_OV_ONNX_FRONTEND=ON \
    -DENABLE_OV_PADDLE_FRONTEND=OFF \
    -DENABLE_OV_PYTORCH_FRONTEND=OFF \
    -DENABLE_OV_JAX_FRONTEND=OFF \
    -DENABLE_OV_TF_FRONTEND=OFF \
    -DENABLE_OV_TF_LITE_FRONTEND=OFF \
    -DENABLE_SYSTEM_OPENCL=OFF \
    -DENABLE_SYSTEM_PROTOBUF=OFF \
    -DENABLE_SYSTEM_PUGIXML=OFF \
    -DENABLE_SYSTEM_SNAPPY=OFF \
    -DENABLE_SYSTEM_TBB=OFF \
    -DTHREADING=SEQ \
    -DENABLE_LTO=OFF

"${cmake_bin}" --build "${build_dir}" --parallel "${UTA_OPENVINO_BUILD_JOBS:-$(nproc)}"
"${cmake_bin}" --install "${build_dir}"

# OpenVINO links its GPU plugin to the bundled ICD loader but excludes that
# loader from the top-level install component. Keep the source-built loader
# beside the runtime so no host SDK package is required.
runtime_lib="${install_dir}/runtime/lib/intel64"
install -Dm755 "${source_dir}/bin/intel64/Release/libOpenCL.so.1.0.0" \
    "${runtime_lib}/libOpenCL.so.1.0.0"
ln -sfn libOpenCL.so.1.0.0 "${runtime_lib}/libOpenCL.so.1"
ln -sfn libOpenCL.so.1 "${runtime_lib}/libOpenCL.so"

mkdir -p "${install_dir}/bin"
"${cxx_bin}" -std=c++17 -O2 \
    -I"${install_dir}/runtime/include" \
    "${SCRIPT_DIR}/tools/convert-model.cpp" \
    -L"${runtime_lib}" -lopenvino \
    -Wl,-rpath,'$ORIGIN/../runtime/lib/intel64' \
    -o "${install_dir}/bin/uta-openvino-convert"

install -Dm644 "${source_dir}/LICENSE" "${install_dir}/share/licenses/openvino/LICENSE"
install -Dm644 "${source_dir}/thirdparty/ocl/icd_loader/LICENSE" \
    "${install_dir}/share/licenses/openvino/OpenCL-ICD-Loader-LICENSE"

actual_recipe="$(sha256sum "${SCRIPT_DIR}/runtime-recipe.json" | cut -d' ' -f1)"
if [[ "${actual_recipe}" != "${RECIPE_SHA256}" ]]; then
    printf 'runtime recipe identity mismatch: expected %s, got %s\n' \
        "${RECIPE_SHA256}" "${actual_recipe}" >&2
    exit 4
fi
install -Dm644 "${SCRIPT_DIR}/runtime-recipe.json" \
    "${install_dir}/runtime-recipe.json"

manifest_tmp="${install_dir}/runtime-manifest.json.tmp.$$"
cat >"${manifest_tmp}" <<EOF
{
  "schema_version": 1,
  "openvino_version": "${OPENVINO_TAG}",
  "source_commit": "${OPENVINO_COMMIT}",
  "recipe_sha256": "${RECIPE_SHA256}",
  "libraries": {
    "runtime/lib/intel64/libopenvino.so.2026.3.0": "$(sha256sum "${runtime_lib}/libopenvino.so.2026.3.0" | cut -d' ' -f1)",
    "runtime/lib/intel64/libopenvino_c.so.2026.3.0": "$(sha256sum "${runtime_lib}/libopenvino_c.so.2026.3.0" | cut -d' ' -f1)",
    "runtime/lib/intel64/libopenvino_onnx_frontend.so.2026.3.0": "$(sha256sum "${runtime_lib}/libopenvino_onnx_frontend.so.2026.3.0" | cut -d' ' -f1)",
    "runtime/lib/intel64/libopenvino_intel_gpu_plugin.so": "$(sha256sum "${runtime_lib}/libopenvino_intel_gpu_plugin.so" | cut -d' ' -f1)",
    "runtime/lib/intel64/libOpenCL.so.1.0.0": "$(sha256sum "${runtime_lib}/libOpenCL.so.1.0.0" | cut -d' ' -f1)"
  }
}
EOF
mv -f "${manifest_tmp}" "${install_dir}/runtime-manifest.json"
printf 'OpenVINO %s (%s) installed at %s\n' \
    "${OPENVINO_TAG}" "${OPENVINO_COMMIT}" "${install_dir}"
