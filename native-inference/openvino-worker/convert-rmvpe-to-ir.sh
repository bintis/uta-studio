#!/usr/bin/env bash
set -euo pipefail

# Explicit model-install action only. This script never runs during application
# startup, rendering, or diagnostics, and never alters the source ONNX model.
readonly MODEL_SHA256="5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd"
readonly RECIPE_SHA256="ac3df548a9e51d36b5d5817ba6988eeaaa29f168d121588fd088daf91dbdf876"
readonly MIN_FRAMES=32
readonly MAX_FRAMES=1024
readonly FRAME_STEP=32
readonly OVERLAP_FRAMES=128

models_dir="${UTA_STUDIO_MODELS_PATH:-${HOME}/.local/share/uta-studio/models}"
runtime_dir="${UTA_STUDIO_OPENVINO_INSTALL_DIR:-${HOME}/.local/share/uta-studio/runtime/openvino-2026.3.0}"
converter="${runtime_dir}/bin/uta-openvino-convert"
source_model="${UTA_RMVPE_ONNX_PATH:-}"

if [[ -z "${source_model}" ]]; then
    for candidate in \
        "${models_dir}/pitch/rmvpe/rmvpe.onnx" \
        "${models_dir}/pitch/rmvpe.onnx"; do
        if [[ -f "${candidate}" ]]; then
            source_model="${candidate}"
            break
        fi
    done
fi
[[ -f "${source_model}" ]] || {
    printf 'RMVPE ONNX source is unavailable\n' >&2
    exit 2
}
[[ -x "${converter}" ]] || {
    printf 'source-built OpenVINO converter is unavailable: %s\n' "${converter}" >&2
    exit 2
}

destination="${models_dir}/pitch/rmvpe/openvino-ir-2026.3.0-bucketed"
[[ ! -e "${destination}" ]] || {
    printf 'refusing to replace existing RMVPE IR: %s\n' "${destination}" >&2
    exit 4
}
mkdir -p "$(dirname "${destination}")"
temporary="${destination}.tmp.$$"
trap 'rm -rf -- "${temporary}"' EXIT
mkdir "${temporary}"

# IR serialization requires static shapes. Keep one shared immutable weights
# file and a deterministic XML graph for every 320 ms frame bucket. This avoids
# long silence padding at song tails without loading ONNX in production.
for frames in $(seq "${MIN_FRAMES}" "${FRAME_STEP}" "${MAX_FRAMES}"); do
    name="rmvpe-$(printf '%04d' "${frames}")"
    bin_output="${temporary}/${name}.bin.tmp"
    if [[ "${frames}" -eq "${MIN_FRAMES}" ]]; then
        bin_output="${temporary}/rmvpe.bin"
    fi
    "${converter}" "${source_model}" "1,128,${frames}" \
        "${temporary}/${name}.xml" "${bin_output}"
    if [[ "${frames}" -ne "${MIN_FRAMES}" ]]; then
        rm "${bin_output}"
    fi
done

manifest="${temporary}/manifest.json"
cat >"${manifest}" <<EOF
{
  "schema_version": 2,
  "model_id": "rmvpe",
  "format": "openvino_ir_v11_bucketed",
  "source_onnx_sha256": "${MODEL_SHA256}",
  "runtime_recipe_sha256": "${RECIPE_SHA256}",
  "input_frame_buckets": {
    "minimum": ${MIN_FRAMES},
    "maximum": ${MAX_FRAMES},
    "step": ${FRAME_STEP},
    "overlap": ${OVERLAP_FRAMES}
  },
  "files": {
EOF
for frames in $(seq "${MIN_FRAMES}" "${FRAME_STEP}" "${MAX_FRAMES}"); do
    name="rmvpe-$(printf '%04d' "${frames}").xml"
    digest="$(sha256sum "${temporary}/${name}" | cut -d' ' -f1)"
    printf '    "%s": "%s",\n' "${name}" "${digest}" >>"${manifest}"
done
printf '    "rmvpe.bin": "%s"\n  }\n}\n' "${BIN_SHA256}" >>"${manifest}"
sync -f "${temporary}"
mv "${temporary}" "${destination}"
trap - EXIT
printf 'RMVPE bucketed OpenVINO IR installed at %s\n' "${destination}"
