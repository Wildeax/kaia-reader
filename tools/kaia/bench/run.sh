#!/usr/bin/env bash
# Run the KR-006 workload on the desktop emulator and collect JSON results.
#
# Usage: tools/kaia/bench/run.sh BOOK RUNS OUT_DIR
#
# Environment:
#   KAIA_BENCH_MODE         baseline or kaia. Unset runs both, interleaved.
#   KAIA_BENCH_SEARCH       search term (default "the").
#   KAIA_BENCH_RUN_TIMEOUT  wall clock limit for one emulator run, seconds (default 420).
#
# Each run gets a fresh KO_HOME and its own copy of BOOK, so the sidecar
# files and caches are cold. The copy keeps the test library unchanged.
#
# Modes:
#   baseline  Kaia additions off, as far as this build allows: start with the
#             file browser, disable the kaiastickers plugin, and point
#             KAIA_LIBKAIA at a path that does not exist. This is not stock
#             KOReader. Kaia Home and Notes code still exists in this build.
#             A true baseline needs the upstream build (see BENCHMARKS.md).
#   kaia      Defaults: Kaia Home at start, every plugin, libkaia loaded.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    echo "usage: $0 BOOK RUNS OUT_DIR" >&2
    exit 2
}

[ "$#" -eq 3 ] || usage
BOOK_SRC="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
RUNS="$2"
OUT_DIR="$3"

[ -f "${BOOK_SRC}" ] || { echo "no such book: $1" >&2; exit 2; }
case "${RUNS}" in
    '' | *[!0-9]*) echo "RUNS must be a positive integer: ${RUNS}" >&2; exit 2 ;;
esac
[ "${RUNS}" -ge 1 ] || { echo "RUNS must be at least 1" >&2; exit 2; }

if [ -n "${KAIA_BENCH_MODE:-}" ]; then
    case "${KAIA_BENCH_MODE}" in
        baseline | kaia) MODES=("${KAIA_BENCH_MODE}") ;;
        *) echo "KAIA_BENCH_MODE must be baseline or kaia" >&2; exit 2 ;;
    esac
else
    MODES=(baseline kaia)
fi

EMU_DIR="$(find "${ROOT}" -maxdepth 1 -type d -name 'koreader-emulator-*' | sort | head -1 || true)"
EMU="${EMU_DIR}/koreader"
[ -n "${EMU_DIR}" ] && [ -d "${EMU}" ] || { echo "no emulator build; run ./kodev build first" >&2; exit 1; }

TIMEOUT_BIN="$(command -v timeout || command -v gtimeout || true)"
[ -n "${TIMEOUT_BIN}" ] || { echo "timeout is not installed" >&2; exit 1; }

COMMIT="$(git -C "${ROOT}" rev-parse --short HEAD 2>/dev/null || echo unknown)"
SEARCH_TERM="${KAIA_BENCH_SEARCH:-the}"
RUN_TIMEOUT="${KAIA_BENCH_RUN_TIMEOUT:-420}"
BOOK_NAME="$(basename "${BOOK_SRC}")"
STEM="${BOOK_NAME%.*}"
mkdir -p "${OUT_DIR}"
OUT_DIR="$(cd "${OUT_DIR}" && pwd)"

failures=0

# run_once MODE RUN: one emulator run. Prints the result path on success.
run_once() {
    local mode="$1" run="$2"
    local work home library result log final
    work="$(mktemp -d "${TMPDIR:-/tmp}/kaia-bench.XXXXXX")"
    home="${work}/home"
    library="${work}/library"
    result="${work}/result.json"
    log="${home}/emulator.log"
    mkdir -p "${home}/patches" "${library}"
    cp "${BOOK_SRC}" "${library}/${BOOK_NAME}"
    cp "${SCRIPT_DIR}/workload.lua" "${home}/patches/2-kaia-bench.lua"

    {
        echo "return {"
        echo "    [\"home_dir\"] = \"${library}\","
        echo "    [\"lastdir\"] = \"${library}\","
        echo "    [\"quickstart_shown_version\"] = 999999999999,"
        echo "    [\"language\"] = \"en\","
        echo "    [\"color_rendering\"] = true,"
        if [ "${mode}" = baseline ]; then
            echo "    [\"start_with\"] = \"filemanager\","
            echo "    [\"plugins_disabled\"] = { [\"kaiastickers\"] = true },"
        fi
        echo "}"
    } >"${home}/settings.reader.lua"

    local libkaia_env=()
    if [ "${mode}" = baseline ]; then
        libkaia_env=("KAIA_LIBKAIA=${work}/absent/libkaia.dylib")
    fi

    (
        cd "${EMU}"
        env -u KAIA_LIBKAIA \
            KO_HOME="${home}" \
            KAIA_BENCH_OUT="${result}" \
            KAIA_BENCH_BOOK="${library}/${BOOK_NAME}" \
            KAIA_BENCH_MODE="${mode}" \
            KAIA_BENCH_RUN="${run}" \
            KAIA_BENCH_COMMIT="${COMMIT}" \
            KAIA_BENCH_SEARCH="${SEARCH_TERM}" \
            ${libkaia_env[@]+"${libkaia_env[@]}"} \
            SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-offscreen}" \
            EMULATE_READER_W=1264 EMULATE_READER_H=1680 EMULATE_READER_DPI=300 \
            "${TIMEOUT_BIN}" "${RUN_TIMEOUT}" ./reader.lua "${library}" >"${log}" 2>&1
    ) || true

    final="${OUT_DIR}/${STEM}-${mode}-run${run}.json"
    if [ -f "${result}" ]; then
        cp "${result}" "${final}"
        rm -rf "${work}"
        echo "${final}"
        return 0
    fi
    cp "${log}" "${OUT_DIR}/${STEM}-${mode}-run${run}.log"
    echo "no result for ${mode} run ${run}; log: ${OUT_DIR}/${STEM}-${mode}-run${run}.log (work dir kept: ${work})" >&2
    return 1
}

for run in $(seq 1 "${RUNS}"); do
    for mode in "${MODES[@]}"; do
        if ! run_once "${mode}" "${run}"; then
            failures=$((failures + 1))
        fi
    done
done

if [ "${failures}" -gt 0 ]; then
    echo "${failures} run(s) failed" >&2
    exit 1
fi
