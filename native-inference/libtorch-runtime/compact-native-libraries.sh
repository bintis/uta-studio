#!/usr/bin/env bash
# Packaging-only compaction of a newly unpacked native library directory.
# Never loads an ELF, follows a directory symlink, or removes a distinct DSO.
set -euo pipefail

root="${1:?usage: compact-native-libraries.sh UNPACKED_LIBRARY_DIRECTORY}"
command -v readelf >/dev/null

# Wheel ZIPs often store the SONAME and linker aliases as full copies. Keep the
# SONAME file and turn only byte-identical regular aliases into relative links.
# Do not infer dlopen dependencies from DT_NEEDED: SYCL/oneMKL/CCL also need
# plugins, device images and provider resources that must remain in the tree.
while IFS= read -r -d '' library; do
  soname="$(LC_ALL=C readelf -d "$library" 2>/dev/null \
    | awk '/\(SONAME\)/ { sub(/^.*\[/, ""); sub(/\].*$/, ""); print; exit }' || true)"
  case "$soname" in ''|*/*|.|..) continue ;; esac
  canonical="$(dirname -- "$library")/$soname"
  [ "$library" != "$canonical" ] || continue
  [ -f "$canonical" ] && [ ! -L "$canonical" ] || continue
  if cmp -s -- "$library" "$canonical"; then
    rm -- "$library"
    ln -s -- "$soname" "$library"
  fi
done < <(find "$root" -type f -name '*.so*' -print0)

# These are standalone host link/debug artifacts, not runtime device bitcode.
# Keep .o/.bc/.spv, JIT libraries, compiler resources, notices and providers.
find "$root" -type f \( -name '*.a' -o -name '*.dbg' \) -delete
