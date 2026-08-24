#!/usr/bin/env bash
# Standalone AMD/RADV compatibility smoke for Uta! Studio's pinned GGML RoFormer runtime.
# This does not modify Runtime Manager, Studio settings, model generations, or source media.
set -euo pipefail

readonly EXPECTED_VENDOR_ID="0x1002"
readonly EXPECTED_DEVICE_NAME="AMD Radeon 780M"
readonly EXPECTED_DRIVER_NAME="radv"
readonly EXPECTED_MODEL_SIZE="457008736"
readonly MAX_INPUT_SECONDS="12.1"

usage() {
    cat <<'EOF'
Usage:
  amd-ggml-smoke.sh INPUT_AUDIO [OUTPUT_DIRECTORY]

Environment overrides:
  UTA_STUDIO_AMD_VULKAN_DEVICE   Vulkan device index (default: 1)
  UTA_STUDIO_GGML_RUNTIME_DIR    Pinned runtime root
  UTA_STUDIO_GGML_MODELS_DIR     Pinned GGUF model root

Run from Uta! Studio's already-realized development shell, for example:
  UTA_STUDIO_NIX_OFFLINE=1 bash dev.sh -c tools/amd-ggml-smoke.sh input.flac

The harness is deliberately fixed to the Dereverb GGUF and refuses CPU,
Intel, software Vulkan, inputs longer than 12.1 seconds, or overwrite of an
existing output directory.
EOF
}

fail() {
    printf 'AMD GGML smoke: %s\n' "$*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"
}

sha256_of() {
    sha256sum -- "$1" | awk '{print $1}'
}

vulkan_device_block() {
    local summary="$1"
    local device="$2"
    awk -v header="GPU${device}:" '
        $0 == header { capture = 1; print; next }
        capture && /^GPU[0-9]+:$/ { exit }
        capture { print }
    ' <<<"$summary"
}

find_amd_drm_device() {
    local vendor
    for vendor in /sys/class/drm/card[0-9]*/device/vendor; do
        test -r "$vendor" || continue
        if grep -qx "$EXPECTED_VENDOR_ID" "$vendor"; then
            dirname "$vendor"
            return 0
        fi
    done
    return 1
}

snapshot_aer() {
    local device_path="$1"
    local destination="$2"
    : >"$destination"
    local counter
    local found=0
    for counter in aer_dev_correctable aer_dev_nonfatal aer_dev_fatal; do
        if test -r "$device_path/$counter"; then
            found=1
            printf '%s\n' "[$counter]" >>"$destination"
            cat "$device_path/$counter" >>"$destination"
        fi
    done
    if test "$found" -eq 0; then
        printf '%s\n' 'unavailable' >"$destination"
    fi
}

if test "${1:-}" = "--help" || test "${1:-}" = "-h"; then
    usage
    exit 0
fi

test "$#" -ge 1 && test "$#" -le 2 || {
    usage >&2
    exit 2
}

for command_name in vulkaninfo ffmpeg ffprobe sha256sum timeout awk grep stat; do
    require_command "$command_name"
done

readonly INPUT="$1"
test -f "$INPUT" || fail "input audio is unavailable: $INPUT"
readonly INPUT_ABSOLUTE="$(readlink -f -- "$INPUT")"
readonly INPUT_SHA256_BEFORE="$(sha256_of "$INPUT_ABSOLUTE")"
readonly INPUT_DURATION="$(ffprobe -v error -show_entries format=duration -of default=nokey=1:noprint_wrappers=1 -- "$INPUT_ABSOLUTE")"
awk -v duration="$INPUT_DURATION" -v maximum="$MAX_INPUT_SECONDS" 'BEGIN { exit !(duration > 0 && duration <= maximum) }' \
    || fail "input duration must be greater than zero and at most $MAX_INPUT_SECONDS seconds (got $INPUT_DURATION)"

readonly DEVICE_INDEX="${UTA_STUDIO_AMD_VULKAN_DEVICE:-1}"
[[ "$DEVICE_INDEX" =~ ^[0-9]+$ ]] || fail "Vulkan device index must be an unsigned integer"

VULKAN_SUMMARY="$(vulkaninfo --summary 2>/dev/null)" || fail "vulkaninfo could not enumerate devices"
readonly VULKAN_SUMMARY
DEVICE_BLOCK="$(vulkan_device_block "$VULKAN_SUMMARY" "$DEVICE_INDEX")"
readonly DEVICE_BLOCK
test -n "$DEVICE_BLOCK" || fail "Vulkan device $DEVICE_INDEX was not enumerated"
grep -Fq "vendorID           = $EXPECTED_VENDOR_ID" <<<"$DEVICE_BLOCK" \
    || fail "Vulkan device $DEVICE_INDEX is not an AMD GPU"
grep -Fq "$EXPECTED_DEVICE_NAME" <<<"$DEVICE_BLOCK" \
    || fail "Vulkan device $DEVICE_INDEX is not the expected Radeon 780M"
grep -Fqi "driverName         = $EXPECTED_DRIVER_NAME" <<<"$DEVICE_BLOCK" \
    || fail "Vulkan device $DEVICE_INDEX is not using RADV"
grep -Fq "deviceType         = PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU" <<<"$DEVICE_BLOCK" \
    || fail "Vulkan device $DEVICE_INDEX is not a hardware integrated GPU"

readonly RUNTIME_ROOT="${UTA_STUDIO_GGML_RUNTIME_DIR:-$HOME/.local/share/uta-studio/runtime/ggml-vulkan-v1}"
readonly MODEL_ROOT="${UTA_STUDIO_GGML_MODELS_DIR:-$HOME/.local/share/uta-studio/runtime/ggml-models}"
readonly ENGINE="$RUNTIME_ROOT/bin/uta-roformer-runtime"
readonly MODEL="$MODEL_ROOT/melband_roformer_dereverb_anvuew/model-fp16.gguf"

for required_file in \
    "$RUNTIME_ROOT/runtime-manifest.json" \
    "$ENGINE" \
    "$RUNTIME_ROOT/lib/libggml-base.so.0" \
    "$RUNTIME_ROOT/lib/libggml-cpu.so.0" \
    "$RUNTIME_ROOT/lib/libggml.so.0" \
    "$RUNTIME_ROOT/lib/libggml-vulkan.so.0" \
    "$MODEL"; do
    test -f "$required_file" || fail "required file is unavailable: $required_file"
done
test "$(stat -c %s -- "$MODEL")" = "$EXPECTED_MODEL_SIZE" || fail "Dereverb GGUF size mismatch"

if test "$#" -eq 2; then
    OUTPUT_DIR="$2"
    test ! -e "$OUTPUT_DIR" || fail "output directory already exists: $OUTPUT_DIR"
    mkdir -p -- "$OUTPUT_DIR"
else
    OUTPUT_DIR="$(mktemp -d -t uta-amd-ggml-smoke.XXXXXXXX)"
fi
readonly OUTPUT_DIR="$(readlink -f -- "$OUTPUT_DIR")"
readonly INPUT_WAV="$OUTPUT_DIR/input-f32-stereo.wav"
readonly ENGINE_OUTPUT="$OUTPUT_DIR/dereverb-amd.wav"
readonly OUTPUT_FLAC="$OUTPUT_DIR/dereverb-amd.flac"
readonly TEMP_FLAC="$OUTPUT_DIR/dereverb-amd.flac.tmp"
readonly DIAGNOSTIC_LOG="$OUTPUT_DIR/runtime.log"
readonly RESULT_FILE="$OUTPUT_DIR/result.txt"
readonly BOOT_ID_BEFORE="$(< /proc/sys/kernel/random/boot_id)"

AMD_DRM_DEVICE="$(find_amd_drm_device)" || fail "AMD DRM sysfs device is unavailable"
readonly AMD_DRM_DEVICE
snapshot_aer "$AMD_DRM_DEVICE" "$OUTPUT_DIR/aer-before.txt"

ffmpeg -v error -nostdin -i "$INPUT_ABSOLUTE" -vn -ar 44100 -ac 2 -c:a pcm_f32le -f wav "$INPUT_WAV"

unset UTA_STUDIO_ROFORMER_FORCE_CPU GGML_VK_VISIBLE_DEVICES MESA_VK_DEVICE_SELECT
export LD_LIBRARY_PATH="$RUNTIME_ROOT/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

printf 'AMD GGML smoke: running Dereverb on Vulkan device %s\n' "$DEVICE_INDEX"
printf 'AMD GGML smoke: output directory: %s\n' "$OUTPUT_DIR"
SECONDS=0
set +e
timeout --signal=TERM --kill-after=15s 15m \
    "$ENGINE" "$MODEL" "$INPUT_WAV" "$ENGINE_OUTPUT" \
    --batch-size 1 \
    --vulkan-device "$DEVICE_INDEX" \
    --vulkan-no-async \
    --serial-pipeline \
    --diagnostic-log "$DIAGNOSTIC_LOG"
ENGINE_STATUS=$?
set -e
readonly ELAPSED_SECONDS="$SECONDS"
test "$ENGINE_STATUS" -eq 0 || fail "engine failed with status $ENGINE_STATUS; see $DIAGNOSTIC_LOG"
test -s "$ENGINE_OUTPUT" || fail "engine did not publish a non-empty WAV"
grep -Fq "vulkan_device=$DEVICE_INDEX" "$DIAGNOSTIC_LOG" \
    || fail "runtime log does not confirm the requested Vulkan device"
grep -Fq "Using backend: Vulkan${DEVICE_INDEX}" "$DIAGNOSTIC_LOG" \
    || fail "runtime log does not confirm Vulkan device $DEVICE_INDEX"
grep -Fq "device=AMD Radeon 780M Graphics (RADV PHOENIX)" "$DIAGNOSTIC_LOG" \
    || fail "runtime log does not confirm execution on the Radeon 780M"

ffmpeg -v error -nostdin -i "$ENGINE_OUTPUT" -vn -ar 44100 -ac 2 -c:a flac -f flac "$TEMP_FLAC"
mv -- "$TEMP_FLAC" "$OUTPUT_FLAC"

readonly OUTPUT_DURATION="$(ffprobe -v error -show_entries format=duration -of default=nokey=1:noprint_wrappers=1 -- "$OUTPUT_FLAC")"
readonly OUTPUT_STREAM="$(ffprobe -v error -select_streams a:0 -show_entries stream=codec_name,sample_rate,channels -of default=noprint_wrappers=1 -- "$OUTPUT_FLAC")"
grep -qx 'codec_name=flac' <<<"$OUTPUT_STREAM" || fail "output codec is not FLAC"
grep -qx 'sample_rate=44100' <<<"$OUTPUT_STREAM" || fail "output sample rate is not 44.1 kHz"
grep -qx 'channels=2' <<<"$OUTPUT_STREAM" || fail "output is not stereo"
awk -v input="$INPUT_DURATION" -v output="$OUTPUT_DURATION" 'BEGIN { delta=input-output; if (delta<0) delta=-delta; exit !(delta <= 0.001) }' \
    || fail "output duration differs from input ($INPUT_DURATION vs $OUTPUT_DURATION)"

VOLUME_REPORT="$(ffmpeg -hide_banner -nostats -nostdin -i "$OUTPUT_FLAC" -af volumedetect -f null - 2>&1)"
readonly VOLUME_REPORT
grep -Eq 'mean_volume: -?[0-9]+([.][0-9]+)? dB' <<<"$VOLUME_REPORT" || fail "output is silent or volume validation failed"

test "$(< /proc/sys/kernel/random/boot_id)" = "$BOOT_ID_BEFORE" || fail "host rebooted during the smoke"
snapshot_aer "$AMD_DRM_DEVICE" "$OUTPUT_DIR/aer-after.txt"
cmp -s "$OUTPUT_DIR/aer-before.txt" "$OUTPUT_DIR/aer-after.txt" \
    || fail "AMD PCIe error counters changed during the smoke"

{
    printf 'status=ok\n'
    printf 'model=melband_roformer_dereverb_anvuew\n'
    printf 'backend=ggml_vulkan\n'
    printf 'device_index=%s\n' "$DEVICE_INDEX"
    printf 'device_name=%s\n' "$EXPECTED_DEVICE_NAME"
    printf 'driver=%s\n' "$EXPECTED_DRIVER_NAME"
    printf 'input_sha256=%s\n' "$INPUT_SHA256_BEFORE"
    printf 'input_duration_seconds=%s\n' "$INPUT_DURATION"
    printf 'output_duration_seconds=%s\n' "$OUTPUT_DURATION"
    printf 'output_sha256=%s\n' "$(sha256_of "$OUTPUT_FLAC")"
    printf 'elapsed_seconds=%s\n' "$ELAPSED_SECONDS"
    printf 'aer_counters=%s\n' "$(head -n 1 "$OUTPUT_DIR/aer-after.txt")"
    printf 'output=%s\n' "$OUTPUT_FLAC"
    grep -E 'mean_volume:|max_volume:' <<<"$VOLUME_REPORT" | sed 's/^.*\(mean_volume:\|max_volume:\)/\1/'
} >"$RESULT_FILE"

printf 'AMD GGML smoke: PASS in %s seconds\n' "$ELAPSED_SECONDS"
printf 'AMD GGML smoke: result: %s\n' "$RESULT_FILE"
