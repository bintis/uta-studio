#!/usr/bin/env bash
# Explicit host-only packaging fixture. Run inside bash dev.sh when authorized.
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/uta-studio-native-packaging.XXXXXX")"
trap 'rm -rf -- "$fixture"' EXIT
mkdir -p "$fixture/runtime/providers" "$fixture/external"
printf 'int fixture_value(void) { return 7; }\n' > "$fixture/library.c"
"${CC:-cc}" -shared -fPIC -Wl,-soname,libfixture.so.1 "$fixture/library.c" -o "$fixture/runtime/libfixture.so.1"
cp "$fixture/runtime/libfixture.so.1" "$fixture/runtime/libfixture.so"
cp "$fixture/runtime/libfixture.so.1" "$fixture/runtime/libfixture.so.1.2"
printf 'int fixture_value(void) { return 9; }\n' > "$fixture/library.c"
"${CC:-cc}" -shared -fPIC -Wl,-soname,libfixture.so.1 "$fixture/library.c" -o "$fixture/runtime/distinct.so"
cp "$fixture/runtime/libfixture.so.1" "$fixture/expected"
cp "$fixture/runtime/distinct.so" "$fixture/distinct"
printf 'not an ELF\n' > "$fixture/runtime/text.so"
printf 'device resource\n' > "$fixture/runtime/providers/kernels.spv"
printf 'runtime bitcode\n' > "$fixture/runtime/device.bc"
printf 'runtime object\n' > "$fixture/runtime/device.o"
printf 'license notice\n' > "$fixture/runtime/NOTICE"
printf 'static link archive\n' > "$fixture/runtime/libunused.a"
printf 'debug companion\n' > "$fixture/runtime/libunused.dbg"
printf 'external archive\n' > "$fixture/external/untouched.a"
ln -s ../external "$fixture/runtime/external"

bash "$repo_root/native-inference/libtorch-runtime/compact-native-libraries.sh" "$fixture/runtime"
[ -f "$fixture/runtime/libfixture.so.1" ] && [ ! -L "$fixture/runtime/libfixture.so.1" ]
[ "$(readlink "$fixture/runtime/libfixture.so")" = libfixture.so.1 ]
[ "$(readlink "$fixture/runtime/libfixture.so.1.2")" = libfixture.so.1 ]
cmp "$fixture/expected" "$fixture/runtime/libfixture.so"
cmp "$fixture/expected" "$fixture/runtime/libfixture.so.1.2"
cmp "$fixture/distinct" "$fixture/runtime/distinct.so"
[ ! -L "$fixture/runtime/distinct.so" ]
for resource in text.so providers/kernels.spv device.bc device.o NOTICE; do
  [ -f "$fixture/runtime/$resource" ]
done
[ ! -e "$fixture/runtime/libunused.a" ] && [ ! -e "$fixture/runtime/libunused.dbg" ]
[ -f "$fixture/external/untouched.a" ]
printf 'native packaging fixture passed\n'
