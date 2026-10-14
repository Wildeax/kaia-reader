#!/usr/bin/env bash
# Check the Rust additions in an extracted Kobo package.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAGE="${1:?usage: check-rust-package.sh stage rust-binary}"
BIN="${2:?missing Rust binary}"
APP="${STAGE}/.adds/kaia"
COUNT=0
check() {
    local label="$1"
    shift
    if ! "$@"; then
        echo "FAIL Rust package: ${label}" >&2
        exit 1
    fi
    COUNT=$((COUNT + 1))
    echo "ok Rust package: ${label}"
}
check 'binary is executable' test -x "${APP}/kaia"
check 'binary matches the target build' cmp -s "${BIN}" "${APP}/kaia"
check 'launcher is executable' test -x "${APP}/kaia-rust.sh"
check 'launcher matches the source' cmp -s "${ROOT}/platform/kobo/kaia/kaia-rust.sh" "${APP}/kaia-rust.sh"
check 'launcher has valid shell syntax' sh -n "${APP}/kaia-rust.sh"
check 'Lua launcher is unchanged' cmp -s "${ROOT}/platform/kobo/kaia/kaia.sh" "${APP}/kaia.sh"
check 'Lua menu entry remains' grep -qF 'Kaia Reader :cmd_spawn :quiet:exec /mnt/onboard/.adds/kaia/kaia.sh' "${STAGE}/.adds/nm/kaia"
check 'Rust menu entry is present' grep -qF 'exec /mnt/onboard/.adds/kaia/kaia-rust.sh' "${STAGE}/.adds/nm/kaia"
check 'EPUB stylesheet is beside the binary' test -s "${APP}/data/epub.css"
check 'FB2 stylesheet is beside the binary' test -s "${APP}/data/fb2.css"
# Every pattern that the reader's language table names must be in the package.
hyphenation_patterns_present() {
    local name missing=0
    while IFS= read -r name; do
        if [ ! -s "${APP}/data/hyph/${name}" ]; then
            echo "missing hyphenation pattern: ${name}" >&2
            missing=1
        fi
    done < <(sed -n 's/.*pattern: "\([^"]*\.pattern\)".*/\1/p' "${ROOT}/rust/kaia-book/src/layout.rs")
    [ "${missing}" -eq 0 ]
}
check 'English hyphenation data is present' test -s "${APP}/data/hyph/English_US.pattern"
check 'Spanish hyphenation data is present' test -s "${APP}/data/hyph/Spanish.pattern"
check 'French hyphenation data is present' test -s "${APP}/data/hyph/French.pattern"
check 'hyphenation data matches the KOReader source' diff -rq -x LICENSES.txt "${ROOT}/base/thirdparty/kpvcrlib/crengine/cr3gui/data/hyph" "${APP}/data/hyph"
check 'every hyphenation language in the reader has its pattern' hyphenation_patterns_present
check 'hyphenation license notice is present' test -s "${APP}/data/hyph/LICENSES.txt"
check 'hyphenation license notice matches the source' cmp -s "${ROOT}/resources/hyph/LICENSES.txt" "${APP}/data/hyph/LICENSES.txt"
check 'CREngine font is beside the binary' test -s "${APP}/fonts/noto/NotoSerif-Regular.ttf"
# Kaia's reading fonts and their licenses, in the fonts/ folder beside the binary.
KAIA_READING_FONTS=(
    atkinson-hyperlegible/AtkinsonHyperlegible-Regular.ttf
    atkinson-hyperlegible/AtkinsonHyperlegible-Bold.ttf
    atkinson-hyperlegible/AtkinsonHyperlegible-Italic.ttf
    atkinson-hyperlegible/AtkinsonHyperlegible-BoldItalic.ttf
    atkinson-hyperlegible/OFL.txt
    opendyslexic/OpenDyslexic-Regular.otf
    opendyslexic/OpenDyslexic-Bold.otf
    opendyslexic/OpenDyslexic-Italic.otf
    opendyslexic/OpenDyslexic-Bold-Italic.otf
    opendyslexic/OFL.txt
)
for font in "${KAIA_READING_FONTS[@]}"; do
    check "reading font is present: ${font}" test -s "${APP}/fonts/${font}"
done
check 'Atkinson Hyperlegible folder matches the source' diff -qr "${ROOT}/resources/kaia-fonts/atkinson-hyperlegible" "${APP}/fonts/atkinson-hyperlegible"
check 'OpenDyslexic folder matches the source' diff -qr "${ROOT}/resources/kaia-fonts/opendyslexic" "${APP}/fonts/opendyslexic"
check 'UI font is present' test -s "${APP}/resources/fonts/noto/NotoSans-Regular.ttf"
check 'UI font license is present' test -s "${APP}/resources/fonts/noto/LICENSE"
check 'UI icons match the source' diff -qr "${ROOT}/resources/icons/mdlight" "${APP}/resources/icons/mdlight"
check 'Lua shims match the source' diff -qr "${ROOT}/rust/kaia-lua/lua" "${APP}/lua"
check 'LuaJIT compatibility filename is present' test -s "${APP}/libs/libluajit-5.1.so.2"
check 'LuaJIT is present' test -s "${APP}/libs/libluajit.so"
check 'KOReader bootstrap is present' test -s "${APP}/setupkoenv.lua"
check 'KOReader plugin loader is present' test -s "${APP}/frontend/pluginloader.lua"
check 'KOReader widgets are present' test -s "${APP}/frontend/ui/widget/infomessage.lua"
check 'Bundled plugins are present' test -s "${APP}/plugins/exporter.koplugin/main.lua"
check 'KOReader FFI modules are present' test -s "${APP}/ffi/blitbuffer.lua"
check 'package has no Rust user data' test ! -e "${STAGE}/.kaia"
echo "Rust package checks passed: ${COUNT}"
