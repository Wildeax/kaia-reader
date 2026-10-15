#!/usr/bin/env bash
# Build a Kobo release package inside the pinned KOReader toolchain image.
#
# Usage: tools/kaia/build-kobo.sh [kobo|kobov5]
#
# The source tree is copied into a Docker volume and built there, because
# building directly on a macOS bind mount fails when the build copies
# read-only git pack files. The package is copied to dist/.
#
# KAIA_DOWNLOAD_CACHE_DIR, when set, is a host folder that keeps the third-party
# downloads between runs. The build seeds the volume from it before make and
# copies the fetched files back after a successful build. CI sets it with
# actions/cache. Each subfolder is named after a thirdparty project and holds
# that project's build/downloads folder.
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

# Optional download cache. The array stays empty when the variable is unset,
# and the expansion below is written so that bash 3.2 accepts an empty array.
DOWNLOAD_MOUNT=()
if [ -n "${KAIA_DOWNLOAD_CACHE_DIR:-}" ]; then
    mkdir -p "${KAIA_DOWNLOAD_CACHE_DIR}"
    DOWNLOAD_MOUNT=(-v "$(cd "${KAIA_DOWNLOAD_CACHE_DIR}" && pwd)":/downloads)
fi

docker volume create "${VOLUME}" >/dev/null
docker run --rm --platform linux/amd64 \
    -v "${ROOT}":/src:ro \
    -v "${VOLUME}":/home/ko/build \
    -v "${ROOT}/dist":/dist \
    ${DOWNLOAD_MOUNT[@]+"${DOWNLOAD_MOUNT[@]}"} \
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
# Packages from earlier builds stay in the volume; remove them so only this
# build is copied out.
rm -f koreader-*.zip
# Seed the third-party downloads from the host cache, when it is mounted. Each
# cached folder goes back into the thirdparty project it came from.
if [ -d /downloads ]; then
    for cached in /downloads/*/; do
        [ -d "${cached}" ] || continue
        name="$(basename "${cached}")"
        [ -d "base/thirdparty/${name}" ] || continue
        mkdir -p "base/thirdparty/${name}/build/downloads"
        cp -a "${cached}." "base/thirdparty/${name}/build/downloads/"
    done
fi
# Rust library, when the workspace exists.
if [ -f rust/Cargo.toml ] && command -v cargo >/dev/null; then
    echo "cargo found in image; the Rust library is built by tools/kaia/build-libkaia.sh"
fi
# The release target runs "make po", which would move the l10n submodule
# to upstream master. Build and package without touching submodule pins.
make TARGET="${TARGET}" KODEBUG= update
# Save the downloads for the host cache. This runs only after a successful
# build, so a failed run leaves the cache as it was.
if [ -d /downloads ]; then
    for dl in base/thirdparty/*/build/downloads; do
        [ -d "${dl}" ] || continue
        name="$(basename "$(dirname "$(dirname "${dl}")")")"
        mkdir -p "/downloads/${name}"
        cp -a "${dl}/." "/downloads/${name}/"
    done
fi
for f in koreader-*"${TARGET}"*.zip koreader-"${TARGET}"*.zip; do
    [ -f "$f" ] && cp -v "$f" /dist/
done
'
ls -la "${ROOT}/dist"
