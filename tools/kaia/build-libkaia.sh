#!/usr/bin/env bash
# Build libkaia for this machine and copy it into the desktop emulator.
#
# Usage: tools/kaia/build-libkaia.sh host
#
# "host" builds the release library for the machine running this script. The
# output is rust/target/release/libkaia.dylib on macOS and libkaia.so on Linux.
# It is copied to koreader-emulator-*/koreader/libs/, where KOReader finds it.
# Running the script again rebuilds if needed and overwrites the copies.
set -euo pipefail

if [ "${1:-}" != "host" ]; then
    echo "usage: $0 host" >&2
    exit 2
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck disable=SC1091 # env.sh sets PATH for cargo on macOS
. "${ROOT}/tools/kaia/env.sh"

case "$(uname -s)" in
    Darwin) EXT=dylib ;;
    *) EXT=so ;;
esac

(cd "${ROOT}/rust" && cargo build --release -p kaia-ffi)

LIB="${ROOT}/rust/target/release/libkaia.${EXT}"
if [ ! -f "${LIB}" ]; then
    echo "build did not produce ${LIB}" >&2
    exit 1
fi

copied=0
for emu in "${ROOT}"/koreader-emulator-*/koreader; do
    [ -d "${emu}" ] || continue
    mkdir -p "${emu}/libs"
    cp -f "${LIB}" "${emu}/libs/libkaia.${EXT}"
    echo "copied ${LIB} to ${emu}/libs/"
    copied=$((copied + 1))
done

if [ "${copied}" -eq 0 ]; then
    echo "no koreader-emulator-*/koreader directory found, run ./kodev build first" >&2
    exit 1
fi
