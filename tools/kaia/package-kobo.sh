#!/usr/bin/env bash
# Repack an upstream KOReader Kobo package as a Kaia Reader package.
#
# Usage: tools/kaia/package-kobo.sh <koreader-kobo-zip> <libkaia.so> <out-dir>
#
#   <koreader-kobo-zip>  dist/koreader-kobo-*.zip or dist/koreader-kobov5-*.zip
#                        from tools/kaia/build-kobo.sh. Top directory: koreader/.
#   <libkaia.so>         ARM build from tools/kaia/build-libkaia.sh kobo or kobov5.
#   <out-dir>            Receives kaia-kobo[-v5]-<version>-<git hash>.zip and SHA256SUMS.
#
# The output unzips at the root of the Kobo storage and adds .adds/kaia/ and
# .adds/nm/kaia. The upstream top directory koreader/ becomes .adds/kaia/.
# Kaia has no update channel yet, so the package has no OTA index and no
# update marker. The zip is deterministic: entries are sorted and every file
# gets the commit time as its mtime. The git hash in the name is HEAD. The
# Engine libraries come from the upstream zip. Frontend, plugins and shims
# come from this checkout.
set -euo pipefail

usage() {
    echo "usage: $0 <koreader-kobo-zip> <libkaia.so> <out-dir>" >&2
    exit 2
}

fail() {
    echo "package-kobo: $*" >&2
    exit 1
}

# Checksums in the same format as sha256sum, on macOS and Linux.
sha_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$@"
    else
        shasum -a 256 "$@"
    fi
}

[ "$#" -eq 3 ] || usage
IN_ZIP="$1"
LIB="$2"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LAUNCHER="${ROOT}/platform/kobo/kaia/kaia.sh"
README="${ROOT}/platform/kobo/kaia/README.txt"
NICKELMENU="${ROOT}/platform/kobo/kaia/nickelmenu-kaia"
IDENTITY="${ROOT}/frontend/kaia/identity.lua"

[ -f "${IN_ZIP}" ] || fail "no such file: ${IN_ZIP}"
[ -f "${LIB}" ] || fail "no such file: ${LIB}"
for f in "${LAUNCHER}" "${README}" "${NICKELMENU}" "${IDENTITY}"; do
    [ -f "${f}" ] || fail "missing repository file: ${f}"
done

# The firmware target comes from the upstream zip name.
case "$(basename "${IN_ZIP}")" in
    koreader-kobo-*)
        TARGET_SUFFIX=""
        TARGET=kobo
        ;;
    koreader-kobov5-*)
        TARGET_SUFFIX="-v5"
        TARGET=kobov5
        ;;
    *) fail "cannot tell the firmware from the zip name: $(basename "${IN_ZIP}")" ;;
esac

RUST_BIN="${ROOT}/dist/rust/${TARGET}/kaia"
[ -f "${RUST_BIN}" ] || fail "missing ${RUST_BIN}; run build-rust-kobo.sh ${TARGET} build -p kaia-app --bin kaia --no-default-features --features kobo"
case "$(file -b "${RUST_BIN}")" in
    *ELF*ARM*executable* | *ELF*executable*ARM*) ;;
    *) fail "not an ARM executable: ${RUST_BIN}" ;;
esac

case "$(file -b "${LIB}")" in
    *ARM*) ;;
    *) fail "not an ARM library: ${LIB}" ;;
esac

OUT_DIR="$(mkdir -p "$3" && cd "$3" && pwd)"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

unzip -Z1 "${IN_ZIP}" >"${WORK}/list.txt" || fail "cannot read zip: ${IN_ZIP}"
grep -qx 'koreader/reader.lua' "${WORK}/list.txt" ||
    fail "${IN_ZIP} has no koreader/reader.lua, so it is not a KOReader package"
# Only koreader/ and the top-level koreader.png are expected. Refuse anything
# else, because it would be dropped without notice.
unexpected="$(grep -Ev '^(koreader/|koreader\.png$)' "${WORK}/list.txt" || true)"
[ -z "${unexpected}" ] || fail "unexpected top-level entries in ${IN_ZIP}: ${unexpected}"

VERSION="$(sed -n 's/^[[:space:]]*version = "\([^"]*\)",.*/\1/p' "${IDENTITY}")"
case "${VERSION}" in
    "" | *[!0-9A-Za-z.+-]*) fail "no usable version in ${IDENTITY}" ;;
esac

HASH="$(git -C "${ROOT}" rev-parse --short HEAD)" || fail "not a git checkout: ${ROOT}"
COMMIT_TIME="$(git -C "${ROOT}" log -1 --format=%ct HEAD)"
# touch -t takes a local time. TZ=UTC makes the stamp UTC. GNU date takes
# "@epoch" and BSD date (macOS) takes -r.
STAMP="$(TZ=UTC date -u -d "@${COMMIT_TIME}" +%Y%m%d%H%M.%S 2>/dev/null ||
    TZ=UTC date -u -r "${COMMIT_TIME}" +%Y%m%d%H%M.%S)"

# Unpack and stage. The upstream top directory koreader/ becomes .adds/kaia/.
mkdir -p "${WORK}/src" "${WORK}/stage/.adds/nm"
unzip -q "${IN_ZIP}" -d "${WORK}/src"
[ -d "${WORK}/src/koreader" ] || fail "no koreader/ directory after unpacking ${IN_ZIP}"
mv "${WORK}/src/koreader" "${WORK}/stage/.adds/kaia"
APP="${WORK}/stage/.adds/kaia"

# Kaia: no KOReader launcher, OTA index or update marker. No KOReader artwork at the storage root.
rm -f "${APP}/koreader.sh"
rm -rf "${APP}/ota"
find "${APP}" \( -name 'koreader.updated*' -o -name 'koreader.installed*' \) -exec rm -f {} +
[ ! -e "${APP}/koreader.sh" ] || fail "koreader.sh is still in the package"
[ ! -e "${APP}/ota" ] || fail "ota/ is still in the package"

# Kaia files. The launcher and the README are copied. The library is placed in libs/.
cp "${LAUNCHER}" "${APP}/kaia.sh"
chmod 755 "${APP}/kaia.sh"
cp "${README}" "${APP}/README.txt"
chmod 644 "${APP}/README.txt"
mkdir -p "${APP}/libs"
cp "${LIB}" "${APP}/libs/libkaia.so"
chmod 644 "${APP}/libs/libkaia.so"
cp "${NICKELMENU}" "${WORK}/stage/.adds/nm/kaia"
chmod 644 "${WORK}/stage/.adds/nm/kaia"

# RuntimeOptions finds CREngine data/ and fonts/ beside kaia. Keep the upstream
# copies unchanged. UI assets have their own resources/ folder.
cp "${RUST_BIN}" "${APP}/kaia"
cp -R "${ROOT}/rust/kaia-lua/lua" "${APP}/lua"
cp -R "${ROOT}/frontend/." "${APP}/frontend/"
cp -R "${ROOT}/plugins/." "${APP}/plugins/"
cp "${ROOT}/dist/rust/${TARGET}/libluajit.so" "${APP}/libs/"
cp "${ROOT}/dist/rust/${TARGET}/libluajit-5.1.so.2" "${APP}/libs/"
cp "${ROOT}/platform/kobo/kaia/kaia-rust.sh" "${APP}/kaia-rust.sh"
chmod 755 "${APP}/kaia" "${APP}/kaia-rust.sh"
cat "${ROOT}/platform/kobo/kaia/nickelmenu-kaia-rust" >>"${WORK}/stage/.adds/nm/kaia"
mkdir -p "${APP}/resources/fonts" "${APP}/resources/icons"
cp -R "${ROOT}/resources/fonts/noto" "${APP}/resources/fonts/"
cp -R "${ROOT}/resources/icons/mdlight" "${APP}/resources/icons/"
bash "${ROOT}/tools/kaia/check-rust-package.sh" "${WORK}/stage" "${RUST_BIN}"

# Fixed mtimes, so the zip does not depend on when the files were unpacked.
find "${WORK}/stage" -exec env TZ=UTC touch -t "${STAMP}" {} +

ZIP_NAME="kaia-kobo${TARGET_SUFFIX}-${VERSION}-${HASH}.zip"
OUT_ZIP="${OUT_DIR}/${ZIP_NAME}"
rm -f "${OUT_ZIP}"
# -X drops uid and gid, -D drops directory entries. Names are sorted in byte order.
(
    cd "${WORK}/stage"
    find . -type f | sed 's|^\./||' | LC_ALL=C sort |
        zip -X -D -q -9 "${OUT_ZIP}" -@
)
unzip -tq "${OUT_ZIP}" >/dev/null || fail "built zip failed the integrity check: ${OUT_ZIP}"

# SHA256SUMS keeps one line per package in the output directory.
SUMS="${OUT_DIR}/SHA256SUMS"
NEW_SUMS="${WORK}/SHA256SUMS.new"
if [ -f "${SUMS}" ]; then
    grep -v -F "  ${ZIP_NAME}" "${SUMS}" >"${NEW_SUMS}" || true
else
    : >"${NEW_SUMS}"
fi
(cd "${OUT_DIR}" && sha_of "${ZIP_NAME}") >>"${NEW_SUMS}"
LC_ALL=C sort -k2 "${NEW_SUMS}" >"${SUMS}"

echo "wrote ${OUT_ZIP}"
echo "wrote ${SUMS}"
