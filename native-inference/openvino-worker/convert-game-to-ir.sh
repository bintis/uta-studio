#!/usr/bin/env bash
set -euo pipefail

# Explicit model-install action only. Uta! Studio never invokes this script on
# launch, rendering, planning, or diagnostics.
readonly GAME_REPOSITORY="https://github.com/openvpi/GAME.git"
readonly GAME_SOURCE_COMMIT="475a8ee781fe8cca980b3b12fbe6c80c768a813a"
readonly GAME_RELEASE="v1.0.3"
readonly GAME_ASSET="GAME-1.0.3-medium-onnx.zip"
readonly GAME_ASSET_URL="https://github.com/openvpi/GAME/releases/download/v1.0.3/${GAME_ASSET}"
readonly GAME_ASSET_SHA256="5b7a21e64c6310efac399f5d12838fffa70565be162436b5a4a65f290721e7d8"
readonly GAME_LICENSE="CC-BY-NC-SA-4.0"
readonly OPENVINO_RECIPE_SHA256="bdeac2a4e1299e4bf82cb2d4edf64c7bdbc613fa40f58727c58793cf7f1a4093"
readonly CHUNK_SAMPLES=1323000
readonly CHUNK_FRAMES=3000
readonly CHUNK_OVERLAP_SAMPLES=88200

readonly ENCODER_ONNX_SHA256="fb00295abf8156806a3bf2477ccadc9dd6a8f464fa4ee1a65c85100ce17e36fe"
readonly SEGMENTER_ONNX_SHA256="49c20f890d02f8877be4b092fae3913ec969e9236cf0a91e661e8bf52465ffda"
readonly ESTIMATOR_ONNX_SHA256="2662a01e24548dc3b9f8ebc081b26947873c01eeaa573c9d0a87e7bf2a43642b"
readonly CONFIG_SHA256="4d62b6d058a820e981184ea6f04605d37659c4331ee01979eb14a14be08af890"

readonly ENCODER_XML_SHA256="2b250b19864bf448adeae8c403c98a98ce5527ed15b713cc24d7026cee17268e"
readonly ENCODER_BIN_SHA256="c022b90dd6ca0f534dcc16d97dbc677c9309addebe2c9356f60586433fa78157"
readonly SEGMENTER_XML_SHA256="0506afbb639193f38864c1d7bfdca2e8f4a2d9b31f4470cd0b0826b3f17f91ee"
readonly SEGMENTER_BIN_SHA256="c85ba5d42f128af4076f2fe27c1b46f7c357378bff6cac86b5480ab65a7c5f5b"
readonly ESTIMATOR_BIN_SHA256="df8f512ad3c33bfc38aabd8a683bcc3e61df6ed6aa27c9724fe400fe547a82d0"
readonly ESTIMATOR_0032_XML_SHA256="e94bc2be910cfd2d55aa68bf7627b07a81d15e3e03cc9f2c6494bd3e8db1a349"
readonly ESTIMATOR_0064_XML_SHA256="47ef059fff69eb6fcf7c4b4eea06adcccb723952fc4be92bfbd33b97efd887dc"
readonly ESTIMATOR_0128_XML_SHA256="7e1d7a3dae42753f4f8a2370ce2302f24a20596cbd2283e914b88270f3796f04"
readonly ESTIMATOR_0256_XML_SHA256="15c732bd02cfb982b3f561695c364e9bcc27261e1550ed5b0dddd23efae7ac1e"
readonly ESTIMATOR_0512_XML_SHA256="a2651d915d73e44ad8b81725a360ce2fe48b871754611c3ea3e9c3732de00e1d"
readonly ESTIMATOR_1024_XML_SHA256="cf328c4c52bdb337d324ce672a577dfbb684a3773d83ac66e056db3ad70c111b"
readonly MANIFEST_SHA256="aa9f3a4c2d107527913ef3947f337b41bff7b6de39de6c91ce46b82ced15ac87"

if [[ $# -ne 2 ]]; then
    printf 'usage: %s EXTRACTED_ONNX_DIRECTORY DESTINATION_DIRECTORY\n' "$0" >&2
    exit 2
fi
source_dir="$1"
destination="$2"
runtime_dir="${UTA_STUDIO_OPENVINO_INSTALL_DIR:-${HOME}/.local/share/uta-studio/runtime/openvino-2026.3.0}"
converter="${UTA_OPENVINO_CONVERTER:-${runtime_dir}/bin/uta-openvino-convert}"

[[ -d "${source_dir}" ]] || { printf 'GAME ONNX source directory is unavailable\n' >&2; exit 2; }
[[ -x "${converter}" ]] || { printf 'source-built OpenVINO converter is unavailable: %s\n' "${converter}" >&2; exit 2; }
[[ ! -e "${destination}" ]] || { printf 'refusing to replace existing GAME IR: %s\n' "${destination}" >&2; exit 4; }
[[ -f "${runtime_dir}/runtime-recipe.json" ]] || {
    printf 'OpenVINO runtime recipe is unavailable\n' >&2
    exit 3
}

require_source() {
    local name="$1" path="${source_dir}/$1"
    [[ -f "${path}" ]] || { printf 'GAME source file is missing: %s\n' "${name}" >&2; exit 3; }
}
require_source encoder.onnx "${ENCODER_ONNX_SHA256}"
require_source segmenter.onnx "${SEGMENTER_ONNX_SHA256}"
require_source estimator.onnx "${ESTIMATOR_ONNX_SHA256}"
require_source config.json "${CONFIG_SHA256}"

mkdir -p "$(dirname "${destination}")"
temporary="${destination}.tmp.$$"
trap 'rm -rf -- "${temporary}"' EXIT
mkdir "${temporary}"
for component in encoder segmenter; do
    "${converter}" --game-v1 "${component}" "${source_dir}/${component}.onnx" \
        "${temporary}/${component}.xml" "${temporary}/${component}.bin"
done
for bucket in 32 64 128 256 512 1024; do
    name="$(printf 'estimator-%04d' "${bucket}")"
    "${converter}" --game-v1 "estimator:${bucket}" "${source_dir}/estimator.onnx" \
        "${temporary}/${name}.xml" "${temporary}/${name}.bin"
    if [[ "${bucket}" == 32 ]]; then
        mv "${temporary}/${name}.bin" "${temporary}/estimator.bin"
    else
        rm "${temporary}/${name}.bin"
    fi
done

require_output() {
    local name="$1"
    [[ -f "${temporary}/${name}" ]] || {
        printf 'GAME IR output is unavailable: %s\n' "${name}" >&2
        exit 5
    }
}
require_output encoder.xml "${ENCODER_XML_SHA256}"
require_output encoder.bin "${ENCODER_BIN_SHA256}"
require_output segmenter.xml "${SEGMENTER_XML_SHA256}"
require_output segmenter.bin "${SEGMENTER_BIN_SHA256}"
require_output estimator.bin "${ESTIMATOR_BIN_SHA256}"
require_output estimator-0032.xml "${ESTIMATOR_0032_XML_SHA256}"
require_output estimator-0064.xml "${ESTIMATOR_0064_XML_SHA256}"
require_output estimator-0128.xml "${ESTIMATOR_0128_XML_SHA256}"
require_output estimator-0256.xml "${ESTIMATOR_0256_XML_SHA256}"
require_output estimator-0512.xml "${ESTIMATOR_0512_XML_SHA256}"
require_output estimator-1024.xml "${ESTIMATOR_1024_XML_SHA256}"
install -Dm644 "${source_dir}/config.json" "${temporary}/config.json"

cat >"${temporary}/manifest.json" <<EOF
{
  "schema_version": 2,
  "model_id": "game",
  "variant": "GAME-1.0.3-medium-onnx",
  "format": "openvino_ir_v11_static_chunked_estimator_buckets",
  "source_repository": "${GAME_REPOSITORY}",
  "source_commit": "${GAME_SOURCE_COMMIT}",
  "source_release": "${GAME_RELEASE}",
  "source_asset": "${GAME_ASSET}",
  "source_asset_url": "${GAME_ASSET_URL}",
  "source_asset_sha256": "${GAME_ASSET_SHA256}",
  "model_license": "${GAME_LICENSE}",
  "runtime_recipe_sha256": "${OPENVINO_RECIPE_SHA256}",
  "sample_rate": 44100,
  "timestep_seconds": 0.01,
  "chunk_samples": ${CHUNK_SAMPLES},
  "chunk_frames": ${CHUNK_FRAMES},
  "chunk_overlap_samples": ${CHUNK_OVERLAP_SAMPLES},
  "d3pm_steps": 8,
  "boundary_threshold": 0.2,
  "boundary_radius_frames": 2,
  "note_presence_threshold": 0.2,
  "estimator_note_buckets": [32, 64, 128, 256, 512, 1024],
  "files": {
    "config.json": "${CONFIG_SHA256}",
    "encoder.xml": "${ENCODER_XML_SHA256}",
    "encoder.bin": "${ENCODER_BIN_SHA256}",
    "segmenter.xml": "${SEGMENTER_XML_SHA256}",
    "segmenter.bin": "${SEGMENTER_BIN_SHA256}",
    "estimator.bin": "${ESTIMATOR_BIN_SHA256}",
    "estimator-0032.xml": "${ESTIMATOR_0032_XML_SHA256}",
    "estimator-0064.xml": "${ESTIMATOR_0064_XML_SHA256}",
    "estimator-0128.xml": "${ESTIMATOR_0128_XML_SHA256}",
    "estimator-0256.xml": "${ESTIMATOR_0256_XML_SHA256}",
    "estimator-0512.xml": "${ESTIMATOR_0512_XML_SHA256}",
    "estimator-1024.xml": "${ESTIMATOR_1024_XML_SHA256}"
  }
}
EOF
require_output config.json "${CONFIG_SHA256}"
require_output manifest.json "${MANIFEST_SHA256}"
sync -f "${temporary}"
mv "${temporary}" "${destination}"
trap - EXIT
printf 'GAME OpenVINO IR installed at %s\n' "${destination}"
