#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
dev_flake="$repo_root/nix/dev-shell"
dev_shell="${UTA_STUDIO_DEV_SHELL:-default}"
case "$dev_shell" in
    default)
        dev_uri="path:$dev_flake"
        ;;
    rocm)
        dev_uri="path:$dev_flake#rocm"
        ;;
    *)
        printf 'unsupported Uta! Studio development shell: %s\n' "$dev_shell" >&2
        exit 2
        ;;
esac
cd "$repo_root"

# The dev-shell flake is tiny and independently locked, so repository/Git
# changes do not alter its source identity or dependency closure. Keep the lock
# immutable during routine shell entry; Nix will reuse existing store paths and
# only fetch a missing pinned path once (normally from the binary cache).
#
# Strict offline mode is opt-in only after the shell has already been realized.
# Nix --offline disables substituters, so making it the default can cause
# expensive local source builds when a store path is missing.
if [[ "${UTA_STUDIO_NIX_OFFLINE:-}" == "1" ]]; then
    exec nix develop --offline --no-write-lock-file "$dev_uri" "$@"
fi

exec nix develop --no-write-lock-file "$dev_uri" "$@"
