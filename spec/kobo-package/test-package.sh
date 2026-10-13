#!/usr/bin/env bash
# Build the Kaia Kobo package from the real dist inputs and check the result.
#
# Usage: bash spec/kobo-package/test-package.sh
#        KAIA_SMOKE=1 bash spec/kobo-package/test-package.sh   (also runs the ARM smoke test)
#
# Inputs: dist/koreader-kobo-*.zip with dist/kobo/libkaia.so. The kobov5 pair is
# tested when both files exist and is skipped otherwise. Needs unzip, zip, file
# and shellcheck. shfmt is used when it is installed. No device is needed.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACKAGER="${ROOT}/tools/kaia/package-kobo.sh"
LAUNCHER="${ROOT}/platform/kobo/kaia/kaia.sh"
SC_FLAGS=(--enable=require-variable-braces --norc) # same flags as base/utils/shellcheck.sh
PASS=0
FAIL=0

ok() {
    PASS=$((PASS + 1))
    echo "ok   $*"
}

bad() {
    FAIL=$((FAIL + 1))
    echo "FAIL $*"
}

TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

VERSION="$(sed -n 's/^[[:space:]]*version = "\([^"]*\)",.*/\1/p' "${ROOT}/frontend/kaia/identity.lua")"

# Prints the newest existing file among the arguments, or nothing.
newest() {
    local f best=""
    for f in "$@"; do
        [ -f "${f}" ] || continue
        if [ -z "${best}" ] || [ "${f}" -nt "${best}" ]; then
            best="${f}"
        fi
    done
    printf '%s' "${best}"
}

# Verifies a sha256sum-format file, with sha256sum on Linux and shasum on macOS.
check_sums() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -c "$1"
    else
        shasum -a 256 -c "$1"
    fi
}

# Lists the SC codes that shellcheck reports for one file, one per line, sorted.
sc_codes() {
    { shellcheck -f gcc -s sh "${SC_FLAGS[@]}" "$1" 2>/dev/null || true; } |
        { grep -o 'SC[0-9]*' || true; } | sort
}

# Checks one firmware target. Arguments: label, upstream zip, libkaia.so, zip name prefix.
test_target() {
    local label="$1" upstream_zip="$2" lib="$3" prefix="$4"
    local run1="${TMP}/${label}-run1" run2="${TMP}/${label}-run2"
    local zip zip2 list extract

    echo "== ${label}: $(basename "${upstream_zip}")"

    if ! bash "${PACKAGER}" "${upstream_zip}" "${lib}" "${run1}" >"${TMP}/${label}-run1.log" 2>&1; then
        bad "${label}: packager failed"
        cat "${TMP}/${label}-run1.log"
        return
    fi
    if ! bash "${PACKAGER}" "${upstream_zip}" "${lib}" "${run2}" >"${TMP}/${label}-run2.log" 2>&1; then
        bad "${label}: second packager run failed"
        return
    fi

    zip="$(ls "${run1}"/*.zip)"
    zip2="$(ls "${run2}"/*.zip)"
    case "$(basename "${zip}")" in
        "${prefix}"*.zip) ok "${label}: zip name starts with ${prefix}" ;;
        *) bad "${label}: unexpected zip name $(basename "${zip}")" ;;
    esac

    if cmp -s "${zip}" "${zip2}"; then
        ok "${label}: two runs produce byte-identical zips"
    else
        bad "${label}: two runs differ"
    fi

    # Layout: everything lives under .adds/kaia/ or .adds/nm/kaia.
    list="${TMP}/${label}.list"
    unzip -Z1 "${zip}" >"${list}"
    if [ -z "$(grep -Ev '^\.adds/(kaia/|nm/kaia$)' "${list}" || true)" ]; then
        ok "${label}: all paths are under .adds/kaia/ or .adds/nm/"
    else
        bad "${label}: paths outside .adds/kaia/ and .adds/nm/"
        grep -Ev '^\.adds/(kaia/|nm/kaia$)' "${list}" | head -5
    fi
    if grep -qx '\.adds/nm/kaia' "${list}"; then
        ok "${label}: NickelMenu entry .adds/nm/kaia is present"
    else
        bad "${label}: NickelMenu entry .adds/nm/kaia is missing"
    fi

    extract="${TMP}/${label}-x"
    unzip -q "${zip}" -d "${extract}"
    local app="${extract}/.adds/kaia"

    if [ -x "${app}/kaia.sh" ]; then
        ok "${label}: kaia.sh exists and is executable"
    else
        bad "${label}: kaia.sh is missing or not executable"
    fi

    if cmp -s "${app}/kaia.sh" "${LAUNCHER}"; then
        ok "${label}: kaia.sh matches platform/kobo/kaia/kaia.sh"
    else
        bad "${label}: kaia.sh differs from platform/kobo/kaia/kaia.sh"
    fi

    case "$(file -b "${app}/libs/libkaia.so" 2>/dev/null)" in
        *ARM*) ok "${label}: libs/libkaia.so is an ARM library" ;;
        *) bad "${label}: libs/libkaia.so is missing or not ARM" ;;
    esac

    if [ -z "$(find "${extract}" -name koreader.sh)" ]; then
        ok "${label}: no file named koreader.sh"
    else
        bad "${label}: a file named koreader.sh is still in the package"
    fi

    if [ ! -e "${extract}/koreader.png" ] && [ ! -e "${app}/ota" ]; then
        ok "${label}: no top-level koreader.png and no ota/ directory"
    else
        bad "${label}: koreader.png or ota/ is still in the package"
    fi

    if grep -qF '/tmp/koreader.sh' "${app}/kaia.sh"; then
        bad "${label}: kaia.sh mentions /tmp/koreader.sh"
    else
        ok "${label}: kaia.sh does not mention /tmp/koreader.sh"
    fi

    if grep -qF '.adds/koreader' "${app}/kaia.sh"; then
        bad "${label}: kaia.sh mentions .adds/koreader"
    else
        ok "${label}: kaia.sh does not mention .adds/koreader"
    fi

    if grep -qF 'exec /mnt/onboard/.adds/kaia/kaia.sh' "${extract}/.adds/nm/kaia"; then
        ok "${label}: NickelMenu entry starts .adds/kaia/kaia.sh"
    else
        bad "${label}: NickelMenu entry does not point at .adds/kaia/kaia.sh"
    fi

    # The Lua entry point keeps its interpreter line and the launcher still runs it.
    if [ "$(head -1 "${app}/reader.lua")" = "#!./luajit" ] && grep -q '^    ./reader.lua "\$@"' "${app}/kaia.sh"; then
        ok "${label}: launcher runs ./reader.lua, which runs through ./luajit"
    else
        bad "${label}: launcher or reader.lua interpreter line changed"
    fi

    # git-rev comes from upstream unchanged.
    unzip -p "${upstream_zip}" koreader/git-rev >"${TMP}/${label}-git-rev.upstream"
    if cmp -s "${TMP}/${label}-git-rev.upstream" "${app}/git-rev"; then
        ok "${label}: git-rev is unchanged from upstream"
    else
        bad "${label}: git-rev differs from upstream"
    fi

    # The launcher must lint no worse than upstream koreader.sh under the CI flags.
    unzip -p "${upstream_zip}" koreader/koreader.sh >"${TMP}/${label}-koreader.sh.upstream"
    sc_codes "${TMP}/${label}-koreader.sh.upstream" >"${TMP}/${label}-sc-upstream"
    sc_codes "${app}/kaia.sh" >"${TMP}/${label}-sc-kaia"
    local extra
    extra="$(comm -13 "${TMP}/${label}-sc-upstream" "${TMP}/${label}-sc-kaia")"
    if [ -z "${extra}" ]; then
        ok "${label}: kaia.sh has no shellcheck codes that upstream koreader.sh lacks"
    else
        bad "${label}: kaia.sh adds shellcheck codes: $(printf '%s' "${extra}" | tr '\n' ' ')"
    fi

    if sh -n "${app}/kaia.sh"; then
        ok "${label}: sh -n accepts kaia.sh"
    else
        bad "${label}: sh -n rejects kaia.sh"
    fi

    if command -v shfmt >/dev/null 2>&1; then
        if shfmt -i 4 -ci -d "${LAUNCHER}" >/dev/null 2>&1; then
            ok "${label}: shfmt -i 4 -ci reports no changes to kaia.sh"
        else
            bad "${label}: shfmt would change kaia.sh"
        fi
    fi

    # The published checksums must match the zip.
    if (cd "${run1}" && check_sums SHA256SUMS >/dev/null 2>&1); then
        ok "${label}: SHA256SUMS matches the zip"
    else
        bad "${label}: SHA256SUMS does not match the zip"
    fi

    if [ "${KAIA_SMOKE:-0}" = "1" ]; then
        smoke "${label}" "${zip}" "${lib}"
    fi
}

# Runs tools/kaia/arm-smoke.sh. The script expects the top directory to be
# koreader/ or kaia/, so the smoke input is the same package re-rooted at kaia/.
smoke() {
    local label="$1" zip="$2" lib="$3"
    local reroot="${TMP}/${label}-smoke"
    mkdir -p "${reroot}"
    unzip -q "${zip}" -d "${reroot}"
    mv "${reroot}/.adds/kaia" "${reroot}/kaia"
    rm -rf "${reroot}/.adds"
    (cd "${reroot}" && zip -q -r -X "${TMP}/${label}-smoke.zip" kaia)
    if (cd "${ROOT}" && bash tools/kaia/arm-smoke.sh "${TMP}/${label}-smoke.zip" "${lib}"); then
        ok "${label}: ARM smoke test passed"
    else
        bad "${label}: ARM smoke test failed"
    fi
}

# A zip without koreader/reader.lua must be refused.
test_refusal() {
    local lib="$1" dir="${TMP}/refusal"
    mkdir -p "${dir}/koreader"
    echo "not a KOReader package" >"${dir}/koreader/readme.txt"
    (cd "${dir}" && zip -q -r -X "${TMP}/koreader-kobo-bogus.zip" koreader)
    if bash "${PACKAGER}" "${TMP}/koreader-kobo-bogus.zip" "${lib}" "${TMP}/refusal-out" >"${TMP}/refusal.log" 2>&1; then
        bad "packager accepts a zip without koreader/reader.lua"
    elif grep -q 'reader.lua' "${TMP}/refusal.log"; then
        ok "packager refuses a zip without koreader/reader.lua"
    else
        bad "packager fails for another reason"
        cat "${TMP}/refusal.log"
    fi
}

# Lint the scripts that this test covers.
test_lint() {
    if shellcheck "${SC_FLAGS[@]}" "${PACKAGER}" "${BASH_SOURCE[0]}" >/dev/null 2>&1; then
        ok "shellcheck is clean for package-kobo.sh and test-package.sh"
    else
        bad "shellcheck reports problems in package-kobo.sh or test-package.sh"
    fi
}

echo "== lint"
test_lint

DIST="${ROOT}/dist"
KOBO_ZIP="$(newest "${DIST}"/koreader-kobo-*.zip)"
KOBOV5_ZIP="$(newest "${DIST}"/koreader-kobov5-*.zip)"
KOBO_LIB="${DIST}/kobo/libkaia.so"
KOBOV5_LIB="${DIST}/kobov5/libkaia.so"

if [ -n "${KOBO_ZIP}" ] && [ -f "${KOBO_LIB}" ]; then
    test_target kobo "${KOBO_ZIP}" "${KOBO_LIB}" "kaia-kobo-${VERSION}-"
    test_refusal "${KOBO_LIB}"
else
    bad "missing dist/koreader-kobo-*.zip or dist/kobo/libkaia.so"
fi

if [ -n "${KOBOV5_ZIP}" ] && [ -f "${KOBOV5_LIB}" ]; then
    test_target kobov5 "${KOBOV5_ZIP}" "${KOBOV5_LIB}" "kaia-kobo-v5-${VERSION}-"
else
    echo "skip kobov5: needs dist/koreader-kobov5-*.zip and dist/kobov5/libkaia.so"
fi

echo "passed ${PASS}, failed ${FAIL}"
[ "${FAIL}" -eq 0 ]
