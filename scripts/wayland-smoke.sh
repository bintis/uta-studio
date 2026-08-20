#!/usr/bin/env bash
set -euo pipefail

binary=${1:?usage: wayland-smoke.sh BINARY SCREENSHOT LOCALE WIDTHxHEIGHT}
screenshot=${2:?usage: wayland-smoke.sh BINARY SCREENSHOT LOCALE WIDTHxHEIGHT}
locale=${3:?usage: wayland-smoke.sh BINARY SCREENSHOT LOCALE WIDTHxHEIGHT}
window_size=${4:?usage: wayland-smoke.sh BINARY SCREENSHOT LOCALE WIDTHxHEIGHT [FONT_PERCENT]}
font_percent=${5:-100}

runtime_dir=$(mktemp -d)
config_dir=$(mktemp -d)
weston_log="$runtime_dir/weston.log"
socket=uta-studio-smoke
weston_pid=

cleanup() {
  if [[ -n "$weston_pid" ]]; then
    kill "$weston_pid" 2>/dev/null || true
    wait "$weston_pid" 2>/dev/null || true
  fi
  rm -rf -- "$runtime_dir" "$config_dir"
}
trap cleanup EXIT

chmod 0700 "$runtime_dir"
mkdir -p "$(dirname "$screenshot")"

XDG_RUNTIME_DIR="$runtime_dir" \
  weston \
    --backend=headless-backend.so \
    --socket="$socket" \
    --idle-time=0 \
    --width=1600 \
    --height=1000 \
    --no-config \
    >"$weston_log" 2>&1 &
weston_pid=$!

for _ in $(seq 1 100); do
  if [[ -S "$runtime_dir/$socket" ]]; then
    break
  fi
  if ! kill -0 "$weston_pid" 2>/dev/null; then
    cat "$weston_log" >&2
    exit 1
  fi
  sleep 0.1
done
test -S "$runtime_dir/$socket"

XDG_RUNTIME_DIR="$runtime_dir" \
XDG_CONFIG_HOME="$config_dir" \
WAYLAND_DISPLAY="$socket" \
WINIT_UNIX_BACKEND=wayland \
WGPU_BACKEND=vulkan \
UTA_STUDIO_LOCALE="$locale" \
UTA_STUDIO_DEBUG_WINDOW_SIZE="$window_size" \
UTA_STUDIO_DEBUG_FONT_SCALE_PERCENT="$font_percent" \
UTA_STUDIO_DEBUG_SCREENSHOT_PATH="$screenshot" \
timeout 45s "$binary"

test -s "$screenshot"
