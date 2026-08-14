#!/usr/bin/env bash
# Verifies the given version string matches every Uta Studio workspace crate.
#
# Exits non-zero with a GitHub Actions error annotation on any mismatch.
#
# Usage: .github/scripts/verify-version.sh <version>
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 <version>" >&2
  exit 2
fi

TAG_VERSION="$1"
MANIFESTS=(
  app-core/Cargo.toml
  desktop/Cargo.toml
  native-audio/Cargo.toml
  studio-diagnostics/Cargo.toml
  utz-export/Cargo.toml
  xtask/Cargo.toml
)

printf 'tag: %s\n' "$TAG_VERSION"
for manifest in "${MANIFESTS[@]}"; do
  manifest_version=$(awk -F'"' '/^version *= *"/ { print $2; exit }' "$manifest")
  printf '%s: %s\n' "$manifest" "$manifest_version"
  if [ "$TAG_VERSION" != "$manifest_version" ]; then
    echo "::error::Tag $TAG_VERSION does not match $manifest ($manifest_version)."
    exit 1
  fi
done
