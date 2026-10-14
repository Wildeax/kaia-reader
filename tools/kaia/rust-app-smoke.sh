#!/usr/bin/env bash
# Run a packaged Rust app under armv7 emulation and save Home and EPUB frames.
# Usage: rust-app-smoke.sh [--skip-binfmt] package.zip output-directory
#   --skip-binfmt  Do not install the armv7 binfmt handler. CI installs it with
#                  docker/setup-qemu-action, and the pinned tonistiigi/binfmt
#                  image is then not needed.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SKIP_BINFMT=0
if [ "${1:-}" = --skip-binfmt ]; then
    SKIP_BINFMT=1
    shift
fi
ZIP="${1:?usage: rust-app-smoke.sh [--skip-binfmt] package.zip output-directory}"
OUT="${2:?missing output directory under HOME}"
mkdir -p "${OUT}"
OUT="$(cd "${OUT}" && pwd)"
case "${OUT}" in
    "${HOME}"/*) ;;
    *)
        echo 'Use an output directory under HOME so Colima can mount it.' >&2
        exit 2
        ;;
esac
mkdir -p "${OUT}/package" "${OUT}/books"
unzip -qo "${ZIP}" -d "${OUT}/package"
cp "${ROOT}/test/juliet.epub" "${OUT}/books/juliet.epub"
for ((i = 0; i < 10; i++)); do
    echo 'key RPgFwd'
done >"${OUT}/turn-pages.txt"
if [ "${SKIP_BINFMT}" -eq 0 ]; then
    docker run --privileged --rm tonistiigi/binfmt --install arm
fi
docker run --rm --platform linux/arm/v7 --network none \
    -v "${OUT}:/work" \
    -v "${OUT}/package:/work/package:ro" \
    -e HOME=/work/home \
    -e KAIA_RESOURCES=/work/package/.adds/kaia/resources \
    -e LD_LIBRARY_PATH=/work/package/.adds/kaia/libs \
    debian:bookworm-slim sh -ec '
mkdir -p /work/home
APP=/work/package/.adds/kaia/kaia
RUN_DATA=$(mktemp -d /work/run-data.XXXXXX)
"${APP}" --headless --data-dir "${RUN_DATA}/home" --books /work/books --screenshot /work/home.png
"${APP}" --headless --data-dir "${RUN_DATA}/reader" --books /work/books --open /work/books/juliet.epub --screenshot /work/epub.png
"${APP}" --headless --data-dir "${RUN_DATA}/reader" --books /work/books --open /work/books/juliet.epub --script /work/turn-pages.txt --screenshot /work/epub-page10.png
# Device mode must report the missing Kobo instead of touching a framebuffer.
if "${APP}" > /work/no-device.log 2>&1; then
    echo "device mode unexpectedly succeeded without a Kobo" >&2
    exit 1
fi
grep -F "requires a Libra Colour" /work/no-device.log
' 2>&1 | tee "${OUT}/qemu.log"
test -s "${OUT}/home.png"
test -s "${OUT}/epub.png"
test -s "${OUT}/epub-page10.png"
echo "ARM app smoke passed: ${OUT}/home.png ${OUT}/epub.png"
