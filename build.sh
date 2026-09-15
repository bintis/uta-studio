#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# Enter the Nix shell only for native libraries. flake.nix deliberately does
# not provide cargo/rustc in this shell, so the installed rustup toolchain is
# retained across this exec.
if [[ "${UTA_STUDIO_LOCAL_BUILD_SHELL:-}" != "1" ]]; then
    # Use the tiny standalone dev-shell flake rather than the repository root.
    # The root working tree contains a very large target/ directory; using it
    # as a path flake can copy changing build output into the Nix store.
    exec bash "$repo_root/dev.sh" -c env \
        UTA_STUDIO_LOCAL_BUILD_SHELL=1 \
        "$repo_root/build.sh" "$@"
fi

rust_sysroot="$(rustc --print sysroot)"
if [[ "$rust_sysroot" == /nix/store/* ]]; then
    printf 'error: expected the installed rustup toolchain, got %s\n' "$rust_sysroot" >&2
    exit 1
fi

printf 'Building Uta! Studio with %s (%s)\n' \
    "$(cargo --version)" "$rust_sysroot"

cd "$repo_root"
tools/check-product-identity.sh
# The worker dlopens this app-owned bridge from the installed runtime. Cargo
# does not rebuild it. Refresh it before publishing new Rust executables so
# native fixes cannot be silently left in the source tree after a normal build.
libtorch_runtime="${UTA_STUDIO_LIBTORCH_RUNTIME_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/uta-studio/runtime/libtorch-xpu}"
if [[ -f "$libtorch_runtime/runtime-manifest.json" ]]; then
    bash "$repo_root/native-inference/libtorch-runtime/rebuild-native-xpu.sh"
else
    printf 'LibTorch XPU is not installed; no native bridge was updated.\n'
fi
# Studio discovers packaged machine-protocol executables beside its own
# binary. Build that complete local set together so a fresh UI cannot talk to
# stale Runtime Manager / Analysis Engine policy from an earlier build.
cargo build --release --locked \
    -p uta-studio-desktop --bin uta-studio \
    -p uta-runtime-manager --bin uta-runtime \
    -p uta-fusion-agent-adapter --bins \
    -p uta-analysis-engine --bin uta-analyze \
    -p uta-ggml-worker --bin uta-ggml-worker \
    "$@"

# Embed the runtime library search paths into the produced ELF binaries so they
# can be executed directly from outside the Nix development shell (e.g. from
# a desktop launcher or a normal user terminal) without missing libwayland,
# libxkbcommon, libvulkan, libglvnd, or GPU driver libraries.
if command -v patchelf >/dev/null 2>&1 && [ -n "${LD_LIBRARY_PATH:-}" ]; then
    for binary in "$repo_root/target/release"/uta-*; do
        if [ -f "$binary" ] && [ -x "$binary" ]; then
            existing_rpath="$(patchelf --print-rpath "$binary" 2>/dev/null || true)"
            new_rpath="${existing_rpath:+${existing_rpath}:}${LD_LIBRARY_PATH}"
            patchelf --set-rpath "$new_rpath" "$binary"
        fi
    done
fi

# Keep result/bin aligned with target/release so standard desktop launchers
# pointing to result/bin/uta-studio immediately resolve to the fresh build.
mkdir -p "$repo_root/target/bin"
for binary in "$repo_root/target/release"/uta-*; do
    if [ -f "$binary" ] && [ -x "$binary" ]; then
        ln -sf "$binary" "$repo_root/target/bin/$(basename "$binary")"
    fi
done
rm -f "$repo_root/result"
ln -s target "$repo_root/result"

# If target/debug exists, keep its executable symlinks aligned with the
# fresh, patched binaries so running target/debug/uta-studio directly
# succeeds with all workers, tools, and RPATH available.
if [ -d "$repo_root/target/debug" ]; then
    for binary in "$repo_root/target/release"/uta-*; do
        if [ -f "$binary" ] && [ -x "$binary" ]; then
            ln -sf "$binary" "$repo_root/target/debug/$(basename "$binary")"
        fi
    done
fi

printf '\nBuilt Studio and packaged protocols in: %s\n' "$repo_root/target/release"
