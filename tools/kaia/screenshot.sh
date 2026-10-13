#!/usr/bin/env bash
# Take a screenshot of the desktop emulator at Libra Colour resolution.
#
# Usage: tools/kaia/screenshot.sh OUT.png [ACTION.lua] [DELAY_SECONDS]
#
# Uses a throwaway data directory (KO_HOME) seeded with a library path from
# KAIA_LIBRARY (default ~/kaia-test-library). ACTION.lua runs inside the app
# one second after startup, for example to open a screen.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
ACTION="${2:-}"
[ -n "${ACTION}" ] && ACTION="$(cd "$(dirname "${ACTION}")" && pwd)/$(basename "${ACTION}")"
DELAY="${3:-3}"
LIBRARY="${KAIA_LIBRARY:-$HOME/kaia-test-library}"
HOME_DIR="${KAIA_SHOT_HOME:-$(mktemp -d)}"
mkdir -p "${HOME_DIR}/patches"
cp "${ROOT}/tools/kaia/shot/2-kaia-shot.lua" "${HOME_DIR}/patches/"
if [ ! -f "${HOME_DIR}/settings.reader.lua" ]; then
    cat >"${HOME_DIR}/settings.reader.lua" <<LUA
return {
    ["home_dir"] = "${LIBRARY}",
    ["lastdir"] = "${LIBRARY}",
    ["quickstart_shown_version"] = 999999999999,
    ["language"] = "en",
    ["color_rendering"] = true,
}
LUA
fi
EMU="$(ls -d "${ROOT}"/koreader-emulator-*/koreader | head -1)"
cd "${EMU}"
KO_HOME="${HOME_DIR}" KAIA_SHOT_OUT="${OUT}" KAIA_SHOT_ACTION="${ACTION}" KAIA_SHOT_DELAY="${DELAY}" \
    SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-offscreen}" \
    EMULATE_READER_W=1264 EMULATE_READER_H=1680 EMULATE_READER_DPI=300 \
    timeout 120 ./reader.lua "${LIBRARY}" >"${HOME_DIR}/emulator.log" 2>&1 || true
if [ -f "${OUT}" ]; then echo "${OUT}"; else echo "no screenshot; log: ${HOME_DIR}/emulator.log" >&2; tail -20 "${HOME_DIR}/emulator.log" >&2; exit 1; fi
