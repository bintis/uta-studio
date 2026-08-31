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
# Studio discovers packaged machine-protocol executables beside its own
# binary. Build that complete local set together so a fresh UI cannot talk to
# stale Runtime Manager / Analysis Engine policy from an earlier build.
cargo build --release --locked \
    -p uta-studio-desktop --bin uta-studio \
    -p uta-runtime-manager --bin uta-runtime \
    -p uta-fusion-agent-adapter --bins \
    -p uta-analysis-engine --bin uta-analyze \
    -p uta-ggml-worker --bin uta-ggml-worker \
    -p uta-openvino-worker --bin uta-openvino-worker \
    -p uta-qwen-worker --bin uta-qwen-asr-worker \
    -p uta-qwen-worker --bin uta-qwen-align-worker \
    "$@"

printf '\nBuilt Studio and packaged protocols in: %s\n' "$repo_root/target/release"
