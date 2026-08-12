#!/usr/bin/env bash
# Verifies the given version string matches the version declared in:
#   - client/src-tauri/tauri.conf.json
#   - client/src-tauri/Cargo.toml
#   - client/package.json
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
TAURI_VERSION=$(jq -r '.version' client/src-tauri/tauri.conf.json)
CARGO_VERSION=$(awk -F'"' '/^version *= *"/ { print $2; exit }' client/src-tauri/Cargo.toml)
PKG_VERSION=$(jq -r '.version' client/package.json)

printf 'tag:    %s\ntauri:  %s\ncargo:  %s\npkg:    %s\n' \
  "$TAG_VERSION" "$TAURI_VERSION" "$CARGO_VERSION" "$PKG_VERSION"

if [ "$TAG_VERSION" != "$TAURI_VERSION" ] \
  || [ "$TAG_VERSION" != "$CARGO_VERSION" ] \
  || [ "$TAG_VERSION" != "$PKG_VERSION" ]; then
  echo "::error::Tag $TAG_VERSION does not match all manifest versions. Bump tauri.conf.json, Cargo.toml, and package.json, then re-tag."
  exit 1
fi
