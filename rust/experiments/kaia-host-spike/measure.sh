#!/bin/sh
# Start-to-ready wall time and peak RSS for the host and for the emulator's luajit.
# Usage: measure.sh [runs]
#
# "empty" runs an empty script, so the cost is process start and Lua state creation.
# "ready" runs lua/driver.lua with SPIKE_STOP_AFTER=ready, so the cost is process
# start plus setupkoenv, the reader globals and the device (SDL window opens).
# Host runs also print the host clock at the READY line (seconds after main starts).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
EMU="$REPO/koreader-emulator-arm64-apple-darwin25.3.0-debug/koreader"
DRIVER="$HERE/lua/driver.lua"
RUNS=${1:-10}
DYLIB_BIN="$HERE/target/release/kaia-host-spike"
STATIC_BIN="$HERE/target-static/release/kaia-host-spike"
TMP=$(mktemp -d "${TMPDIR:-/tmp}/kr011-measure.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
: > "$TMP/empty.lua"
export KAIA_SPIKE_TMP="$TMP" KAIA_SPIKE_REPO="$REPO"
export KAIA_LIBKAIA="$REPO/rust/target/release/libkaia.dylib"
export SPIKE_HOST_TESTS=0
cd "$EMU"

# one LABEL STOP COMMAND...
one() {
    label=$1
    stop=$2
    shift 2
    SPIKE_STOP_AFTER="$stop" /usr/bin/time -l "$@" > "$TMP/out" 2> "$TMP/err" || true
    real=$(awk '/ real /{print $1}' "$TMP/err")
    rss=$(awk '/maximum resident set size/{print $1}' "$TMP/err")
    clock=$(sed -n 's/^READY .*clock=\([0-9.]*\).*/\1/p' "$TMP/out")
    echo "$label real=$real rss_bytes=$rss clock=${clock:--}"
}

i=1
while [ "$i" -le "$RUNS" ]; do
    one host-dylib-empty "" "$DYLIB_BIN" "$TMP/empty.lua"
    one host-dylib-ready ready "$DYLIB_BIN" "$DRIVER"
    one host-static-empty "" "$STATIC_BIN" "$TMP/empty.lua"
    one host-static-ready ready "$STATIC_BIN" "$DRIVER"
    one luajit-empty "" ./luajit "$TMP/empty.lua"
    one luajit-ready ready ./luajit "$DRIVER"
    i=$((i + 1))
done
