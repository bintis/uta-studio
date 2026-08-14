#!/usr/bin/env bash
# Extracts the CHANGELOG.md section for <version> into <output-path>.
#
# A section starts at "## [<version>]" (the leading "v" and the brackets
# are both optional) and ends at the next "## [...]" heading. If no
# section is found the output file is populated with a fallback body so
# the release pipeline can still proceed.
#
# Usage: .github/scripts/extract-changelog-notes.sh <version> <output-path>
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $0 <version> <output-path>" >&2
  exit 2
fi

VERSION="$1"
OUTPUT="$2"

awk -v v="$VERSION" '
  $0 ~ "^## \\[?v?" v "\\]?" { flag=1; next }
  flag && /^## / { flag=0 }
  flag
' CHANGELOG.md > "$OUTPUT"

if [ ! -s "$OUTPUT" ]; then
  echo "::warning::No CHANGELOG.md section found for $VERSION; using fallback release body."
  echo "Release v$VERSION" > "$OUTPUT"
fi

echo "--- $OUTPUT ---"
cat "$OUTPUT"
