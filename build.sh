#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# Enter the Nix shell only for native libraries. flake.nix deliberately does
# not provide cargo/rustc in this shell, so the installed rustup toolchain is
# retained across this exec.
if [[ "${UTA_STUDIO_LOCAL_BUILD_SHELL:-}" != "1" ]]; then
    exec nix develop "path:$repo_root" -c env \
        UTA_STUDIO_LOCAL_BUILD_SHELL=1 \
        "$repo_root/build.sh" "$@"
fi

rust_sysroot="$(rustc --print sysroot)"
if [[ "$rust_sysroot" == /nix/store/* ]]; then
    printf 'error: expected the installed rustup toolchain, got %s\n' "$rust_sysroot" >&2
    exit 1
fi

printf 'Building Uta Studio with %s (%s)\n' \
    "$(cargo --version)" "$rust_sysroot"

cd "$repo_root"
cargo build --release --locked -p uta-studio-desktop --bin uta-studio "$@"

printf '\nBuilt: %s\n' "$repo_root/target/release/uta-studio"
