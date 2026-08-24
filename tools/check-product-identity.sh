#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Match the punctuation-free display name without embedding that disallowed
# spelling in this gate itself. Stable machine IDs use '-' or '_' and therefore
# cannot match this expression.
pattern='[Uu][Tt][Aa][[:space:]]+[Ss][Tt][Uu][Dd][Ii][Oo]'
mapfile -t matches < <(
    grep -I -R -n -E "$pattern" . \
        --exclude-dir=.git \
        --exclude-dir=target \
        --exclude-dir=node_modules \
        --exclude-dir=result \
        --exclude='*.zip' \
        || true
)

if ((${#matches[@]} > 0)); then
    printf 'error: punctuation-free product display identity found:\n' >&2
    printf '  %s\n' "${matches[@]}" >&2
    exit 1
fi

printf 'product display identity is canonical\n'
