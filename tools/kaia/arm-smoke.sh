#!/usr/bin/env bash
# Run the Kobo build's LuaJIT and libkaia.so under armv7 emulation.
#
# Usage: tools/kaia/arm-smoke.sh dist/koreader-kobo-*.zip dist/kobo/libkaia.so
#
# Needs Docker with an armv7 binfmt handler, for example:
#   docker run --privileged --rm tonistiigi/binfmt --install arm
# The work directory is under $HOME because Colima shares only $HOME.
# Emulation checks the ABI, the float ABI and the FFI binding. It does not
# replace a test on the device.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ZIP="$1"
LIB="$2"
WORK="${HOME}/.cache/kaia-arm-smoke"
rm -rf "${WORK}"
mkdir -p "${WORK}"
unzip -q "${ZIP}" -d "${WORK}"
APP="${WORK}/koreader"
[ -d "${APP}" ] || APP="${WORK}/kaia"
[ -d "${APP}" ] || APP="${WORK}/.adds/kaia"
cp "${LIB}" "${APP}/libs/libkaia.so"
rm -rf "${APP}/frontend/kaia"
cp -R "${ROOT}/frontend/kaia" "${APP}/frontend/kaia"
cp "${ROOT}/tools/kaia/arm/kaia_arm_smoke.lua" "${APP}/"
docker run --rm --platform linux/arm/v7 -v "${APP}":/app -w /app debian:bookworm-slim \
    ./luajit kaia_arm_smoke.lua
