#!/usr/bin/env bash
# Atomically updates version across all workspace Cargo.toml manifests and synchronizes Cargo.lock.
#
# Usage: tools/bump-version.sh <new_version>
set -euo pipefail

if [ $# -ne 1 ]; then
  printf 'Usage: %s <new_version>\n' "$0" >&2
  exit 2
fi

target_version="$1"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

if ! command -v pkg-config >/dev/null 2>&1; then
  printf 'Native build environment not active, entering dev.sh...\n'
  exec bash "$repo_root/dev.sh" -c "$script_dir/bump-version.sh" "$target_version"
fi

manifest_list=(
  analysis-engine/Cargo.toml
  app-core/Cargo.toml
  desktop/Cargo.toml
  fusion-agent-adapter/Cargo.toml
  native-audio/Cargo.toml
  native-inference/ggml-runtime/Cargo.toml
  native-inference/ggml-worker/Cargo.toml
  native-inference/gpu-probes/Cargo.toml
  runtime-manager/Cargo.toml
  studio-diagnostics/Cargo.toml
  utz-export/Cargo.toml
  xtask/Cargo.toml
)

printf 'Updating workspace manifests to %s...\n' "$target_version"
for manifest_file in "${manifest_list[@]}"; do
  sed -i -E "s/^(version *= *\")[^\"]+(\".*)$/\1$target_version\2/" "$manifest_file"
done

printf 'Updating Cargo.lock to match new workspace versions...\n'
cargo check --workspace --all-targets --quiet

printf 'Verifying version consistency across workspace...\n'
.github/scripts/verify-version.sh "$target_version"

printf 'Verifying lockfile synchronization...\n'
tools/check-lockfile.sh

printf '\033[0;32mVersion bump to %s and lockfile synchronization complete.\033[0m\n' "$target_version"
printf 'Changed files:\n'
git status --short Cargo.lock "${manifest_list[@]}"
