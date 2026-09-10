#!/usr/bin/env bash
# Verifies that Cargo.lock is strictly synchronized with workspace Cargo.toml manifests
# and that no unstaged changes remain.
#
# Usage: tools/check-lockfile.sh
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

if ! command -v pkg-config >/dev/null 2>&1; then
  printf 'Native build environment not active, entering dev.sh...\n'
  exec bash "$repo_root/dev.sh" -c "$script_dir/check-lockfile.sh" "$@"
fi

printf 'Checking workspace lockfile synchronization...\n'

if ! cargo check --workspace --all-targets --locked --quiet; then
  printf '\033[0;31mError: Cargo.lock is out of sync with workspace Cargo.toml manifests!\033[0m\n' >&2
  printf 'Run: cargo check --workspace --all-targets\n' >&2
  printf 'And then stage the updated Cargo.lock.\n' >&2
  exit 1
fi

lock_diff=$(git diff --name-only Cargo.lock)
if [ -n "$lock_diff" ]; then
  printf '\033[0;33mWarning: Cargo.lock has unstaged modifications!\033[0m\n' >&2
  printf 'Remember to stage Cargo.lock before committing or pushing.\n' >&2
fi

printf '\033[0;32mWorkspace lockfile is fully synchronized.\033[0m\n'
