#!/usr/bin/env bash
# Build libkaia for this machine and copy it into the desktop emulator.
#
# Usage: tools/kaia/build-libkaia.sh host|kobo|kobov5
#
# "host" builds the release library for the machine running this script. The
# output is rust/target/release/libkaia.dylib on macOS and libkaia.so on Linux.
# It is copied to koreader-emulator-*/koreader/libs/, where KOReader finds it.
# Running the script again rebuilds if needed and overwrites the copies.
#
# "kobo" and "kobov5" cross-compile libkaia.so for armv7 in the kaia/kobo-rust
# image (tools/kaia/docker/kobo-rust.Dockerfile) with the matching Kobo
# toolchain, check that it needs no glibc symbol newer than the sysroot, and
# write it to dist/<target>/libkaia.so.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

build_kobo() {
    local target="$1" tc max_glibc
    case "${target}" in
        kobo) tc=arm-kobo-linux-gnueabihf max_glibc=2.15 ;;
        kobov5) tc=arm-kobov5-linux-gnueabihf max_glibc=2.35 ;; # newest glibc that KOReader's own kobov5 binaries need
    esac
    local image=kaia/kobo-rust:1
    if ! docker image inspect "${image}" >/dev/null 2>&1; then
        docker build --platform linux/amd64 -t "${image}" \
            -f "${ROOT}/tools/kaia/docker/kobo-rust.Dockerfile" "${ROOT}/tools/kaia/docker"
    fi
    mkdir -p "${ROOT}/dist/${target}"
    docker volume create "kaia-rust-target-${target}" >/dev/null
    docker run --rm --platform linux/amd64 \
        -v "${ROOT}/rust":/src:ro \
        -v "kaia-rust-target-${target}":/target \
        -v "${ROOT}/dist/${target}":/out \
        -e TC="${tc}" -e MAX_GLIBC="${max_glibc}" \
        "${image}" bash -lc '
set -euo pipefail
sudo chown ko:ko /target
export CARGO_TARGET_DIR=/target
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER="${TC}-gcc"
export CC_armv7_unknown_linux_gnueabihf="${TC}-gcc" AR_armv7_unknown_linux_gnueabihf="${TC}-ar"
cd /src
cargo build --locked --release -p kaia-ffi --target armv7-unknown-linux-gnueabihf --features glibc215
lib=/target/armv7-unknown-linux-gnueabihf/release/libkaia.so
"${TC}-strip" -o /out/libkaia.so "${lib}"
newest=$("${TC}-readelf" -V /out/libkaia.so | grep -o "GLIBC_[0-9.]*" | sed s/GLIBC_// | sort -uV | tail -1)
echo "newest glibc symbol version: ${newest}"
if [ "$(printf "%s\n%s\n" "${newest}" "${MAX_GLIBC}" | sort -V | tail -1)" != "${MAX_GLIBC}" ]; then
    echo "libkaia.so needs glibc ${newest}, newer than ${MAX_GLIBC}" >&2
    exit 1
fi
"${TC}-readelf" -h /out/libkaia.so | grep -E "Machine|Flags"
'
    echo "wrote ${ROOT}/dist/${target}/libkaia.so"
}

case "${1:-}" in
    host) ;;
    kobo | kobov5)
        build_kobo "$1"
        exit 0
        ;;
    *)
        echo "usage: $0 host|kobo|kobov5" >&2
        exit 2
        ;;
esac

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
