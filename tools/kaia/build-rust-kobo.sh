#!/usr/bin/env bash
# Build Rust crates for a Kobo (armv7 hard-float) inside the kaia/kobo-rust image.
#
# Usage: tools/kaia/build-rust-kobo.sh kobo|kobov5 <cargo args...>
#   tools/kaia/build-rust-kobo.sh kobo build -p kaia-doc --example render_page
#
# The repository is mounted read-only at /src. The KOReader build volume from
# build-kobo.sh (kaia-build-<target>) is mounted read-only at /home/ko/build, so
# MuPDF and CREngine link against the staging tree that the Kobo package uses.
# Cargo writes to kaia-rust-kobo-target-<target> and the cargo registry to
# kaia-rust-cargo-registry. If KAIA_RUST_CACHE_DIR is set, both live in that
# directory instead, as registry/ and target-<target>/. CI uses this to save
# them with actions/cache.
#
# The build is a release build, as KOReader's Kobo build is. The script adds
# --release unless the arguments name a profile, and it refuses other profiles.
#
# Outputs: every executable and shared library in the release directory and in
# release/examples is stripped and copied to dist/rust/<target>/. Each one is
# checked against the newest glibc symbol version: 2.15 for kobo and 2.35 for
# kobov5 (what KOReader's own binaries need).
set -euo pipefail

if [ $# -lt 2 ]; then
    echo "usage: $0 kobo|kobov5 <cargo args...>" >&2
    exit 2
fi

TARGET="$1"
shift

case "${TARGET}" in
    kobo)
        TC=arm-kobo-linux-gnueabihf
        MAX_GLIBC=2.15
        CPU=cortex-a8
        ;;
    kobov5)
        TC=arm-kobov5-linux-gnueabihf
        MAX_GLIBC=2.35
        CPU=cortex-a53
        ;;
    *)
        echo "unsupported target: ${TARGET} (use kobo or kobov5)" >&2
        exit 2
        ;;
esac

# Same CPU and float flags as KOReader's CREngine build (build.ninja FLAGS).
# The C and C++ shim must use the ABI the Rust objects use.
ARCH_FLAGS="-march=armv7-a -mtune=${CPU} -mfpu=neon -mthumb -mfloat-abi=hard"

# koreader/kokobo:1.0.0-22.04, the image matching the v2026.07.1 baseline.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
IMAGE=kaia/kobo-rust:1
BUILD_VOLUME="kaia-build-${TARGET}"
TARGET_VOLUME="kaia-rust-kobo-target-${TARGET}"
OUT="${ROOT}/dist/rust/${TARGET}"

# Check the profile before starting Docker. The script builds release only.
ARGS=("$@")
# The normal app package includes plugins. --features kobo,lua is explicit.
for ((i = 0; i < ${#ARGS[@]}; i++)); do
    if [ "${ARGS[i]}" = --features ] && [ "${ARGS[i + 1]:-}" = kobo ]; then
        ARGS[i + 1]=kobo,lua
    elif [ "${ARGS[i]}" = --features=kobo ]; then
        ARGS[i]=--features=kobo,lua
    fi
done
has_profile=0
for ((i = 0; i < ${#ARGS[@]}; i++)); do
    case "${ARGS[i]}" in
        --debug)
            echo "build-rust-kobo.sh builds release only; drop --debug" >&2
            exit 2
            ;;
        --profile)
            has_profile=1
            if [ "${ARGS[i + 1]:-}" != release ]; then
                echo "build-rust-kobo.sh builds release only; got --profile ${ARGS[i + 1]:-}" >&2
                exit 2
            fi
            ;;
        --profile=*)
            has_profile=1
            if [ "${ARGS[i]#--profile=}" != release ]; then
                echo "build-rust-kobo.sh builds release only; got ${ARGS[i]}" >&2
                exit 2
            fi
            ;;
        --release) has_profile=1 ;;
    esac
done
if [ "${has_profile}" -eq 0 ]; then
    # Cargo takes the subcommand first, so --release goes right after it.
    ARGS=("${ARGS[0]}" --release "${ARGS[@]:1}")
fi

if ! docker image inspect "${IMAGE}" >/dev/null 2>&1; then
    docker build --platform linux/amd64 -t "${IMAGE}" \
        -f "${ROOT}/tools/kaia/docker/kobo-rust.Dockerfile" "${ROOT}/tools/kaia/docker"
fi
if ! docker volume inspect "${BUILD_VOLUME}" >/dev/null 2>&1; then
    echo "volume ${BUILD_VOLUME} not found, run tools/kaia/build-kobo.sh ${TARGET} first" >&2
    exit 1
fi

mkdir -p "${OUT}"
if [ -n "${KAIA_RUST_CACHE_DIR:-}" ]; then
    mkdir -p "${KAIA_RUST_CACHE_DIR}/registry" "${KAIA_RUST_CACHE_DIR}/target-${TARGET}"
    TARGET_MOUNT="${KAIA_RUST_CACHE_DIR}/target-${TARGET}"
    REGISTRY_MOUNT="${KAIA_RUST_CACHE_DIR}/registry"
else
    docker volume create "${TARGET_VOLUME}" >/dev/null
    docker volume create kaia-rust-cargo-registry >/dev/null
    TARGET_MOUNT="${TARGET_VOLUME}"
    REGISTRY_MOUNT=kaia-rust-cargo-registry
fi

docker run --rm --platform linux/amd64 \
    -v "${ROOT}":/src:ro \
    -v "${BUILD_VOLUME}":/home/ko/build:ro \
    -v "${TARGET_MOUNT}":/target \
    -v "${REGISTRY_MOUNT}":/home/ko/.cargo/registry \
    -v "${OUT}":/out \
    -e TC="${TC}" -e MAX_GLIBC="${MAX_GLIBC}" -e TARGET="${TARGET}" \
    -e ARCH_FLAGS="${ARCH_FLAGS}" \
    "${IMAGE}" bash -lc '
set -euo pipefail
# Recursive, because a cache directory restored by actions/cache starts out
# owned by the runner user, and the files below it must be writable by ko.
sudo chown -R ko:ko /target /home/ko/.cargo/registry 2>/dev/null || true

# Build tree of the KOReader build for this target, read-only.
B=/home/ko/build/base/build/${TC}
ST=${B}/staging
CRE_BUILD=${B}/thirdparty/crengine/build
if [ ! -f "${CRE_BUILD}/libcrengine.a" ] || [ ! -f "${ST}/include/mupdf/fitz.h" ]; then
    echo "missing KOReader build output under ${B}" >&2
    exit 1
fi

T=armv7_unknown_linux_gnueabihf
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_TARGET=armv7-unknown-linux-gnueabihf
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER="${TC}-gcc"
export CC_${T}="${TC}-gcc" CXX_${T}="${TC}-g++" AR_${T}="${TC}-ar"
export CFLAGS_${T}="${ARCH_FLAGS}" CXXFLAGS_${T}="${ARCH_FLAGS}"

# MuPDF (kaia-mupdf-sys).
export KAIA_MUPDF_INCLUDE="${ST}/include"
export KAIA_MUPDF_LIB="${ST}/lib"
export KAIA_MUPDF_DEP_LIBS="${ST}/lib"

# DjVuLibre (kaia-djvu-sys, a default feature of kaia-doc).
export KAIA_DJVU_INCLUDE="${ST}/include"
export KAIA_DJVU_LIB="${ST}/lib"

# CREngine (kaia-cre-sys). The headers come from the same source tree that
# libcrengine.a was built from.
export KAIA_CRE_INCLUDE=/home/ko/build/base/thirdparty/kpvcrlib/crengine/crengine/include
export KAIA_CRE_LIB="${CRE_BUILD}"
export KAIA_CRE_SETUP="${CRE_BUILD}"
export KAIA_CRE_DEP_LIBS="${ST}/lib"
export KAIA_CRE_DEP_INCLUDE="${ST}/include"

# The pinned target LuaJIT and its native modules share one dynamic image.
export KAIA_LUAJIT_LIB="${ST}/lib"
export KAIA_LUAJIT_INCLUDE="${ST}/include/luajit-2.1"
export LUA_LIB="${KAIA_LUAJIT_LIB}" LUA_LIB_NAME=luajit LUA_LINK=dynamic
export PKG_CONFIG_ALLOW_CROSS=1

cd /src/rust

# The firmware 4.x sysroot is glibc 2.15. Rust std needs a few newer symbols
# (getauxval so far). kaia-glibc-compat provides them as an rlib, which the link
# step gets as an archive, so the linker pulls in only what is missing.
# The firmware 5.x sysroot (glibc 2.35) needs no fallback.
if [ "${TARGET}" = kobo ]; then
    # The workspace release profile unwinds and uses thin LTO. A no_std crate
    # cannot unwind, so panic=abort. Thin LTO leaves LLVM bitcode in the objects,
    # which ld cannot read, so LTO is off. The crate never panics.
    cargo build -p kaia-glibc-compat --features glibc215 --release --config "profile.release.panic=\"abort\"" --config "profile.release.lto=false"
    # ld does not read the rlib itself, so take its ELF objects and pack them
    # into a plain archive with the Kobo ar.
    rm -rf /target/glibc-compat && mkdir -p /target/glibc-compat/obj
    (cd /target/glibc-compat/obj && "${TC}-ar" x /target/armv7-unknown-linux-gnueabihf/release/libkaia_glibc_compat.rlib)
    "${TC}-ar" rcs /target/glibc-compat/libkaia_glibc_compat.a /target/glibc-compat/obj/*.o
    export RUSTFLAGS="-C link-arg=/target/glibc-compat/libkaia_glibc_compat.a"
fi

cargo "$@"

TD=/target/armv7-unknown-linux-gnueabihf/release
shopt -s nullglob
count=0
for f in "${TD}"/* "${TD}"/examples/*; do
    [ -f "$f" ] || continue
    name="$(basename "$f")"
    # Skip the hashed copies that cargo writes (name-<16 hex digits>) and relocatable objects.
    [[ "${name}" =~ -[0-9a-f]{16}$ ]] && continue
    # Keep ELF executables and shared libraries only (ET_EXEC or ET_DYN).
    [ "$(head -c 4 "$f" | od -An -tx1 | tr -d " \n")" = 7f454c46 ] || continue
    elf_type=$("${TC}-readelf" -h "$f" | grep Type: | sed "s/.*Type: *//" | cut -d " " -f1)
    case "${elf_type}" in EXEC | DYN) ;; *) continue ;; esac
    "${TC}-strip" -o "/out/${name}" "$f"
    newest=$("${TC}-readelf" -V "/out/${name}" | grep -o "GLIBC_[0-9.]*" | sed s/GLIBC_// | sort -uV | tail -1 || true)
    flags=$("${TC}-readelf" -h "/out/${name}" | grep -E "Machine|Flags" | tr -s " " | tr "\n" " ")
    echo "${name}: newest glibc ${newest:-none}; ${flags}"
    if [ -n "${newest}" ] && [ "$(printf "%s\n%s\n" "${newest}" "${MAX_GLIBC}" | sort -V | tail -1)" != "${MAX_GLIBC}" ]; then
        echo "${name} needs glibc ${newest}, newer than ${MAX_GLIBC}" >&2
        exit 1
    fi
    count=$((count + 1))
done
"${TC}-strip" -o /out/libluajit.so "${ST}/lib/libluajit.so"
cp /out/libluajit.so /out/libluajit-5.1.so.2
echo "copied ${count} artifact(s) and LuaJIT for ${TARGET}"
' kaia-build "${ARGS[@]}"

ls -la "${OUT}"
