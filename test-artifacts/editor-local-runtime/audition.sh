#!/usr/bin/env bash
set -euo pipefail
root="$PWD/test-artifacts/editor-local-runtime"
run="$root/audition"
mkdir -p "$run"
runtime=$(mktemp -d)
weston_pid=
app_pid=
cleanup() {
  [[ -z "$app_pid" ]] || kill "$app_pid" 2>/dev/null || true
  [[ -z "$weston_pid" ]] || kill "$weston_pid" 2>/dev/null || true
  rm -rf "$runtime"
}
trap cleanup EXIT
export PULSE_SERVER="unix:${XDG_RUNTIME_DIR}/pulse/native"
export PIPEWIRE_REMOTE="${XDG_RUNTIME_DIR}/pipewire-0"
XDG_RUNTIME_DIR="$runtime" /nix/store/bym3mrisbsrph2654wj5bhc1x5ijhv0q-weston-16.0.0/bin/weston --backend=headless-backend.so --socket=uta-studio-audition --idle-time=0 --width=1600 --height=1000 --no-config >"$run/weston.log" 2>&1 &
weston_pid=$!
for attempt in $(seq 1 100); do
  [[ ! -S "$runtime/uta-studio-audition" ]] || break
  sleep .1
done
test -S "$runtime/uta-studio-audition"
# Slower command pacing leaves a sustained real-chart audition after Play.
# Extra read-only UI steps prevent script completion from exiting early.
python3 - "$root" <<'PY'
import json,pathlib,sys
root=pathlib.Path(sys.argv[1])
steps=[json.loads(line) for line in (root/'editor.ndjson').read_text().splitlines()]
steps += [{'command':'ui.editor.action.select_all'} for _ in range(24)]
(root/'audition'/'script.ndjson').write_text(''.join(json.dumps(step)+'\n' for step in steps))
PY
# Unset build-shell library/plugin discovery. The local ELF must stand alone.
env -u LD_LIBRARY_PATH -u GST_PLUGIN_SYSTEM_PATH_1_0 -u GST_PLUGIN_SYSTEM_PATH -u GST_PLUGIN_PATH_1_0 -u GST_PLUGIN_PATH -u GST_PLUGIN_SCANNER -u GST_PLUGIN_SCANNER_1_0 \
  GST_REGISTRY="$run/registry.bin" XDG_RUNTIME_DIR="$runtime" WAYLAND_DISPLAY=uta-studio-audition WINIT_UNIX_BACKEND=wayland \
  VK_DRIVER_FILES=/nix/store/4cvv9wbvhz36a1fhd9px1rvjr8j61ycr-mesa-26.2.1/share/vulkan/icd.d/lvp_icd.x86_64.json \
  UTA_STUDIO_DATA_PATH="$root/data" UTA_STUDIO_DEBUG_UI_SCRIPT="$run/script.ndjson" UTA_STUDIO_DEBUG_UI_SCRIPT_PACE=90 \
  UTA_STUDIO_DEBUG_UI_REPORT="$run/report.ndjson" UTA_STUDIO_DEBUG_SCREENSHOT_PATH="$run/editor.png" \
  timeout 100s "$PWD/target/release/uta-studio" >"$run/stdout.txt" 2>"$run/stderr.txt" &
app_pid=$!
for sample in $(seq 1 12); do
  sleep 3
  pactl -f json list sink-inputs >"$run/streams-$sample.json"
done
pw-top -b -n 6 >"$run/pw-top.txt"
wait "$app_pid"
app_pid=
