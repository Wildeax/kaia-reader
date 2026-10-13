#!/bin/sh
# Runs lua/driver.lua under the Rust host or under the emulator's own luajit.
# Usage: run.sh host|luajit [host-binary]
#   host-binary defaults to target/release/kaia-host-spike.
# The working directory is the emulator's koreader/ directory. Scratch files go
# to a new temporary directory that is removed on exit.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
EMU="$REPO/koreader-emulator-arm64-apple-darwin25.3.0-debug/koreader"
DRIVER="$HERE/lua/driver.lua"
MODE=${1:?usage: run.sh host|luajit [host-binary]}
TMP=$(mktemp -d "${TMPDIR:-/tmp}/kr011.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
export KAIA_SPIKE_TMP="$TMP" KAIA_SPIKE_REPO="$REPO"
export KAIA_LIBKAIA="$REPO/rust/target/release/libkaia.dylib"
case "$MODE" in
host)
    BIN=${2:-$HERE/target/release/kaia-host-spike}
    (cd "$EMU" && "$BIN" "$DRIVER")
    ;;
luajit)
    (cd "$EMU" && ./luajit "$DRIVER")
    ;;
*)
    echo "usage: run.sh host|luajit [host-binary]" >&2
    exit 2
    ;;
esac
