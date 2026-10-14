#!/usr/bin/env bash
# Measure Rust app page turns on the desktop release build (R1 exit check).
#
# Usage: tools/kaia/bench/rust-page-turn.sh [--quick | --books LIST] [OUT_DIR]
#
#   --quick          juliet.epub and paper.pdf only, for a short check.
#   --books LIST     comma-separated file names or stems to run, such as
#                    juliet.epub,paper.pdf or 1342. Names are matched in test/
#                    and in the library folder.
#
# Builds rust/kaia-app/examples/page_turn_bench in release, runs it on every
# EPUB and PDF under test/ and on every EPUB in ~/kaia-test-library (skipped when
# that folder is missing), and writes to OUT_DIR:
#   page-turns.csv   one row per turn (warm-up rows are marked)
#   run.txt          machine, commit, build profile, corpus SHA-256 values
#   summary.md       p50, p95 and max per book, configuration and direction
#
# OUT_DIR defaults to ~/kaia-bench/rust-page-turn-YYYY-MM-DD. The Lua workload
# keeps its raw results outside the repository in ~/kaia-bench, so this run does
# the same. Pass a directory to keep the results elsewhere.
#
# Environment:
#   KAIA_BENCH_LARGE_FONT   font size in pixels for the second run of each
#                           reflowable EPUB (default 85, the largest the Text
#                           settings allow: 40 units at this panel).
#   KAIA_BENCH_LIBRARY      extra library folder (default ~/kaia-test-library).
#   KAIA_BENCH_BUDGET_S     wall-clock budget for the measured turns of one layout
#                           (default 900). A slow book stops early and keeps its turns.
#   KAIA_BENCH_TURN_LIMIT_S longest single turn before that book stops (default 120).
set -euo pipefail

BOOK_FILTER=""
OUT_ARG=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --quick) BOOK_FILTER="juliet.epub,paper.pdf" ;;
        --books)
            [ "$#" -ge 2 ] || { echo "--books needs a list" >&2; exit 2; }
            BOOK_FILTER="$2"
            shift
            ;;
        -h | --help) sed -n '2,25p' "$0"; exit 0 ;;
        -*) echo "unknown option: $1" >&2; exit 2 ;;
        *) OUT_ARG="$1" ;;
    esac
    shift
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
RUST="${ROOT}/rust"
TEST_DIR="${ROOT}/test"
LIBRARY="${KAIA_BENCH_LIBRARY:-${HOME}/kaia-test-library}"
LARGE_FONT="${KAIA_BENCH_LARGE_FONT:-85}"
OUT_DIR="${OUT_ARG:-${HOME}/kaia-bench/rust-page-turn-$(date +%Y-%m-%d)}"

case "${LARGE_FONT}" in
    '' | *[!0-9]*) echo "KAIA_BENCH_LARGE_FONT must be a whole number of pixels: ${LARGE_FONT}" >&2; exit 2 ;;
esac

# shellcheck source=/dev/null
. "${ROOT}/tools/kaia/env.sh"

mkdir -p "${OUT_DIR}"
OUT_DIR="$(cd "${OUT_DIR}" && pwd)"
CSV="${OUT_DIR}/page-turns.csv"
META="${OUT_DIR}/run.txt"
SUMMARY="${OUT_DIR}/summary.md"
rm -f "${CSV}" "${META}" "${SUMMARY}"

echo "building page_turn_bench in release" >&2
(cd "${RUST}" && cargo build --release -p kaia-app --example page_turn_bench)
BIN="${RUST}/target/release/examples/page_turn_bench"
[ -x "${BIN}" ] || { echo "missing ${BIN}" >&2; exit 1; }

BOOKS=()
for f in "${TEST_DIR}"/*.epub "${TEST_DIR}"/*.pdf; do
    [ -f "${f}" ] && BOOKS+=("${f}")
done
if [ -d "${LIBRARY}" ]; then
    for f in "${LIBRARY}"/*.epub; do
        [ -f "${f}" ] && BOOKS+=("${f}")
    done
else
    echo "skipping ${LIBRARY}: not found" >&2
fi

if [ -n "${BOOK_FILTER}" ]; then
    SELECTED=()
    IFS=',' read -r -a WANTED <<<"${BOOK_FILTER}"
    for name in "${WANTED[@]}"; do
        match=""
        for f in "${BOOKS[@]}"; do
            base="$(basename "${f}")"
            if [ "${base}" = "${name}" ] || [ "${base%.*}" = "${name}" ]; then
                match="${f}"
                break
            fi
        done
        [ -n "${match}" ] || { echo "no book named ${name} in test/ or the library" >&2; exit 2; }
        SELECTED+=("${match}")
    done
    BOOKS=("${SELECTED[@]}")
fi
[ "${#BOOKS[@]}" -gt 0 ] || { echo "no books found" >&2; exit 1; }

{
    echo "date: $(date '+%Y-%m-%d %H:%M:%S %z')"
    echo "commit: $(git -C "${ROOT}" rev-parse --short HEAD 2>/dev/null || echo unknown)"
    dirty="$(git -C "${ROOT}" status --porcelain 2>/dev/null | wc -l | tr -d ' ')"
    echo "working tree: ${dirty} changed paths (0 means clean)"
    echo "machine: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
    echo "cpu cores: $(sysctl -n hw.ncpu 2>/dev/null || echo unknown)"
    echo "memory bytes: $(sysctl -n hw.memsize 2>/dev/null || echo unknown)"
    echo "os: $(sw_vers -productName 2>/dev/null) $(sw_vers -productVersion 2>/dev/null)"
    echo "rustc: $(rustc --version)"
    echo "profile: release, $(sed -n '/^\[profile.release\]/,/^\[/p' "${RUST}/Cargo.toml" | grep -v '^\[' | tr -s ' ' | paste -sd ';' -)"
    echo "kaia-app features: default (desktop, lua)"
    echo "engine libraries: $(ls -d "${ROOT}"/base/build/*/staging 2>/dev/null | sed 's#.*/build/##; s#/staging##' | paste -sd ' ' -) (KOReader build directories; MuPDF and CREngine flags come from that build)"
    echo "large font: ${LARGE_FONT} px (reflowable EPUB only)"
    echo "working directory for the binary: resources/ (MuPDF fonts path)"
    echo "turns: 5 warm-up forward, 50 measured forward, 20 measured backward; default layout and large font"
    echo "corpus:"
    for f in "${BOOKS[@]}"; do
        printf '  %s  %s bytes  sha256 %s\n' "$(basename "${f}")" "$(wc -c <"${f}" | tr -d ' ')" \
            "$(shasum -a 256 "${f}" | cut -d' ' -f1)"
    done
} >"${META}"

# MuPDF opens its fonts from ./fonts, so the binary runs from resources/, which
# holds the fonts folder the desktop build uses.
failures=0
for book in "${BOOKS[@]}"; do
    echo "page turns: $(basename "${book}")" >&2
    if ! (cd "${ROOT}/resources" && "${BIN}" --book "${book}" --csv "${CSV}" --large-font "${LARGE_FONT}"); then
        failures=$((failures + 1))
    fi
done

[ -f "${CSV}" ] || { echo "no rows written" >&2; exit 1; }
python3 -I "${ROOT}/tools/kaia/bench/rust_page_turn_summary.py" "${CSV}" --meta "${META}" --out "${SUMMARY}"
echo "results: ${OUT_DIR}" >&2

if [ "${failures}" -gt 0 ]; then
    echo "${failures} book run(s) failed" >&2
    exit 1
fi
