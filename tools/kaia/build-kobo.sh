#!/usr/bin/env bash
# Build a Kobo release package inside the pinned KOReader toolchain image.
#
# Usage: tools/kaia/build-kobo.sh [kobo|kobov5]
#
# The source tree is copied into a Docker volume and built there, because
# building directly on a macOS bind mount fails when the build copies
# read-only git pack files. The package is copied to dist/.
set -euo pipefail

TARGET="${1:-kobo}"
case "${TARGET}" in
    kobo | kobov5) ;;
    *)
        echo "unsupported target: ${TARGET} (use kobo or kobov5)" >&2
        exit 1
        ;;
esac

# koreader/kokobo:1.0.0-22.04, the image matching the v2026.07.1 baseline.
IMAGE="koreader/kokobo@sha256:e89d36fabb0650fc02f1c7b2cf9555e78485ba463569b6b13f9d063d6ecbb5b6"
VOLUME="kaia-build-${TARGET}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "${ROOT}/dist"

docker volume create "${VOLUME}" >/dev/null
docker run --rm --platform linux/amd64 \
    -v "${ROOT}":/src:ro \
    -v "${VOLUME}":/home/ko/build \
    -v "${ROOT}/dist":/dist \
    -e TARGET="${TARGET}" \
    "${IMAGE}" bash -lc '
set -euo pipefail
sudo chown -R ko:ko /home/ko/build 2>/dev/null || true
cd /home/ko/build
# Copy sources, keeping earlier build output in the volume for incremental builds.
tar -C /src \
    --exclude=./base/build --exclude="./koreader-*" --exclude=./dist \
    --exclude=./rust/target -cf - . | tar -xf - --no-same-permissions
git config --global --add safe.directory "*"
# Rust library, when the workspace exists.
if [ -f rust/Cargo.toml ] && command -v cargo >/dev/null; then
    echo "cargo found in image; the Rust library is built by tools/kaia/build-libkaia.sh"
fi
# The release target runs "make po", which would move the l10n submodule
# to upstream master. Build and package without touching submodule pins.
make TARGET="${TARGET}" KODEBUG= update
for f in koreader-*"${TARGET}"*.zip koreader-"${TARGET}"*.zip; do
    [ -f "$f" ] && cp -v "$f" /dist/
done
'
ls -la "${ROOT}/dist"
