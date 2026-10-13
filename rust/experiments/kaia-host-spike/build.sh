#!/bin/sh
# Builds the KR-011 host against KOReader's LuaJIT. Usage: build.sh [dylib|static] [cargo args]
#
# dylib (default): links libs/libluajit.dylib, the copy the native modules
#   (libkoreader-lfs.so, libkoreader-xtext.so, rapidjson.so) bind to.
# static: links staging/lib/libluajit-5.1.a, the archive the emulator's luajit
#   executable is built from. Uses its own target directory.
#
# No vendored feature is used, so mlua builds no LuaJIT of its own.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
BUILD="$REPO/base/build/arm64-apple-darwin25.3.0-debug"
. "$REPO/tools/kaia/env.sh"

MODE=${1:-dylib}
[ $# -gt 0 ] && shift

case "$MODE" in
dylib)
    export LUA_LIB="$BUILD/libs"
    export LUA_LIB_NAME=luajit
    export KAIA_SPIKE_LUAJIT_DIR="$BUILD/libs"
    TARGET_DIR="$HERE/target"
    ;;
static)
    export LUA_LIB="$BUILD/staging/lib"
    export LUA_LIB_NAME=luajit-5.1
    export LUA_LINK=static
    export KAIA_SPIKE_LUAJIT_DIR="$BUILD/staging/lib"
    TARGET_DIR="$HERE/target-static"
    ;;
*)
    echo "usage: $0 [dylib|static] [cargo args]" >&2
    exit 2
    ;;
esac

cd "$HERE"
CARGO_TARGET_DIR="$TARGET_DIR" cargo build --release "$@"
