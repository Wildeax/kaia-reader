#!/bin/sh
#
# test-collect-profile.sh: check collect-profile.sh against fake Kobo roots.
#
# Builds two fake roots with a fake serial, Wi-Fi name, MAC address, account
# e-mail and database marker. The tests check that none of those reach the
# profile. Runs on a desktop with POSIX tools only. Exit 0 when every check
# passes.

set -u

HERE="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)"
COLLECT="${HERE}/collect-profile.sh"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/kaia-collect-profile-test.XXXXXX")" || exit 1
trap 'rm -rf "${WORK}"' EXIT INT TERM

# Fake values. None of them is a real device identifier.
SERIAL="N905000012345678"
EMAIL="reader@example.com"
SSID="Home SSID Example"
MAC="aa:bb:cc:dd:ee:ff"
DB_MARKER="DB-MARKER-7f3a"

PASS=0
FAIL=0

ok() {
    PASS=$((PASS + 1))
    echo "ok    $1"
}

bad() {
    FAIL=$((FAIL + 1))
    echo "FAIL  $1"
}

# expect_line FILE LINE DESCRIPTION: an exact line must exist.
expect_line() {
    if grep -q -x -F -- "$2" "$1"; then
        ok "$3"
    else
        bad "$3 (wanted line: $2)"
    fi
}

# expect_absent FILE TEXT DESCRIPTION: the text must not appear.
expect_absent() {
    if grep -q -F -- "$2" "$1"; then
        bad "$3"
    else
        ok "$3"
    fi
}

# expect_pattern FILE REGEX DESCRIPTION: a line must match the regex.
expect_pattern() {
    if grep -q -E -- "$2" "$1"; then
        ok "$3"
    else
        bad "$3 (no line matches $2)"
    fi
}

# make_root DIR PRODUCT_OUTPUT: build a fake root tree.
make_root() {
    _r="$1"
    mkdir -p "${_r}/bin" "${_r}/lib" "${_r}/proc" \
        "${_r}/sys/class/graphics/fb0" "${_r}/sys/class/net/wlan0" \
        "${_r}/etc/wpa_supplicant" \
        "${_r}/mnt/onboard/.kobo/Kobo" \
        "${_r}/mnt/onboard/.adds/koreader" \
        "${_r}/mnt/onboard/.adds/kaia" \
        "${_r}/mnt/onboard/.adds/nm" \
        "${_r}/mnt/onboard/.adds/kfmon"

    # Product id helper. The second fixture prints the serial here on purpose.
    printf '#!/bin/sh\necho %s\n' "$2" >"${_r}/bin/kobo_config.sh"
    chmod 755 "${_r}/bin/kobo_config.sh"

    # Version file: serial, unused field, firmware, two more fields.
    printf '%s,FAKE-FIELD-2,4.38.23171,FAKE-FIELD-4,FAKE-FIELD-5\n' "${SERIAL}" \
        >"${_r}/mnt/onboard/.kobo/version"

    # Account and database files that the profile must never read or print.
    printf '[Account]\nUserEmail=%s\n' "${EMAIL}" \
        >"${_r}/mnt/onboard/.kobo/Kobo/Kobo eReader.conf"
    printf '%s %s\n' "${EMAIL}" "${DB_MARKER}" \
        >"${_r}/mnt/onboard/.kobo/KoboReader.sqlite"

    # Wi-Fi name and MAC address. The profile must not read these files.
    printf 'network={\n\tssid="%s"\n}\n' "${SSID}" \
        >"${_r}/etc/wpa_supplicant/wpa_supplicant.conf"
    printf '%s\n' "${MAC}" >"${_r}/sys/class/net/wlan0/address"

    # Kernel and CPU. The Serial line is the trap: it must not be printed.
    printf 'MemTotal:       524288 kB\nMemFree:        100000 kB\nMemAvailable:   250000 kB\n' \
        >"${_r}/proc/meminfo"
    printf 'Processor\t: ARMv7 Processor rev 10 (v7l)\nprocessor\t: 0\nCPU architecture: 7\nFeatures\t: half thumb fastmult vfp edsp neon vfpv3 tls vfpd32\nHardware\t: FAKE BOARD\nSerial\t\t: %s\n' \
        "${SERIAL}" >"${_r}/proc/cpuinfo"

    # Framebuffer attributes.
    printf '1264,1680\n' >"${_r}/sys/class/graphics/fb0/virtual_size"
    printf '32\n' >"${_r}/sys/class/graphics/fb0/bits_per_pixel"
    printf '1\n' >"${_r}/sys/class/graphics/fb0/rotate"

    # Loader and libc banner. Only the string matters, not a working loader.
    printf 'loader\n' >"${_r}/lib/ld-linux-armhf.so.3"
    printf '\177ELF fake libc GNU C Library (buildroot) stable release version 2.23.\n' \
        >"${_r}/lib/libc.so.6"

    # KOReader and Kaia install directories.
    printf 'v2026.07.1\n' >"${_r}/mnt/onboard/.adds/koreader/git-rev"
    printf 'v0.1.0-dev\n' >"${_r}/mnt/onboard/.adds/kaia/git-rev"
    printf '#!/bin/sh\necho "viewWidth=1264;viewHeight=1680;hasColorPanel=1;FONTH=16;"\n' \
        >"${_r}/mnt/onboard/.adds/koreader/fbink"
    chmod 755 "${_r}/mnt/onboard/.adds/koreader/fbink"

    # Minimal ARM ELF header with EF_ARM_ABI_FLOAT_HARD set in e_flags.
    {
        printf '\177ELF\001\001\001'
        zeros 9
        printf '\002\000\050\000\001\000\000\000'
        zeros 12
        printf '\000\004\000\005'
    } >"${_r}/mnt/onboard/.adds/koreader/luajit"
}

# zeros N: print N NUL bytes.
zeros() {
    _n="$1"
    while [ "${_n}" -gt 0 ]; do
        printf '\000'
        _n=$((_n - 1))
    done
}

FIXTURE="${WORK}/root-a"
make_root "${FIXTURE}" "monza"
OUT="${WORK}/profile-a.txt"

echo "== profile from fixture A"
if KAIA_FAKE_ROOT="${FIXTURE}" sh "${COLLECT}" >"${OUT}" 2>"${WORK}/err-a.txt"; then
    ok "collector exits 0 under sh"
else
    bad "collector exits 0 under sh"
fi

expect_absent "${OUT}" "${SERIAL}" "serial is absent from output"
expect_absent "${OUT}" "${EMAIL}" "account e-mail is absent from output"
expect_absent "${OUT}" "${SSID}" "Wi-Fi network name is absent from output"
expect_absent "${OUT}" "${MAC}" "MAC address is absent from output"
expect_absent "${OUT}" "${DB_MARKER}" "database contents are absent from output"
expect_absent "${OUT}" "Serial" "cpuinfo Serial line is absent from output"
expect_absent "${OUT}" "ssid" "no ssid key in output"

if grep -q -v -E '^[A-Za-z0-9_]+=' "${OUT}"; then
    bad "every output line is key=value"
else
    ok "every output line is key=value"
fi

expect_line "${OUT}" "model_id=monza" "model id comes from kobo_config.sh"
expect_line "${OUT}" "firmware_version=4.38.23171" "firmware is field 3 of the version file"
expect_line "${OUT}" "profile_file_name=monza-4.38.23171.txt" "profile file name uses model and firmware"
expect_line "${OUT}" "dynamic_loader=/lib/ld-linux-armhf.so.3" "dynamic loader path"
expect_line "${OUT}" "loader_float_abi_hint=hard" "loader name hints hard float"
expect_line "${OUT}" "glibc_version=2.23" "glibc version parsed from libc banner"
expect_line "${OUT}" "cpu_neon=yes" "NEON detected from features"
expect_line "${OUT}" "cpu_vfp=yes" "VFP detected from features"
expect_line "${OUT}" "koreader_elf_float_abi=hard" "ELF e_flags report hard float"
expect_line "${OUT}" "mem_total_kb=524288" "MemTotal in kB"
expect_line "${OUT}" "mem_available_kb=250000" "MemAvailable in kB"
expect_line "${OUT}" "fb_virtual_size=1264,1680" "framebuffer virtual size"
expect_line "${OUT}" "fb_bits_per_pixel=32" "framebuffer bits per pixel"
expect_line "${OUT}" "fbink_viewWidth=1264" "fbink -e values are prefixed"
expect_line "${OUT}" "koreader_version=v2026.07.1" "KOReader version from git-rev"
expect_line "${OUT}" "kaia_version=v0.1.0-dev" "Kaia version from git-rev"
expect_line "${OUT}" "nickelmenu_present=yes" "NickelMenu directory detected"
expect_line "${OUT}" "kfmon_present=yes" "KFMon directory detected"
expect_pattern "${OUT}" '^storage_onboard_free_kb=[0-9]+$' "free storage reported as a number"
expect_pattern "${OUT}" '^kernel_machine=.+$' "kernel machine reported"

echo "== --self-test on fixture A"
if KAIA_FAKE_ROOT="${FIXTURE}" sh "${COLLECT}" --self-test >"${WORK}/self-a.txt" 2>&1; then
    ok "self-test passes on fixture A"
else
    bad "self-test passes on fixture A"
    cat "${WORK}/self-a.txt"
fi

echo "== dash and sh produce the same profile"
if [ -x /bin/dash ]; then
    # collected_utc and the free-space figure change between runs, so skip them.
    KAIA_FAKE_ROOT="${FIXTURE}" /bin/dash "${COLLECT}" | grep -v -E '^(collected_utc|storage_onboard_free_kb)=' >"${WORK}/dash.txt"
    grep -v -E '^(collected_utc|storage_onboard_free_kb)=' "${OUT}" >"${WORK}/sh.txt"
    if diff "${WORK}/sh.txt" "${WORK}/dash.txt" >"${WORK}/diff.txt"; then
        ok "dash output matches sh output"
    else
        bad "dash output matches sh output"
        cat "${WORK}/diff.txt"
    fi
else
    ok "dash not installed, cross-shell check skipped"
fi

echo "== kobo_config.sh prints the serial"
FIXTURE_B="${WORK}/root-b"
make_root "${FIXTURE_B}" "${SERIAL}"
OUT_B="${WORK}/profile-b.txt"
if KAIA_FAKE_ROOT="${FIXTURE_B}" sh "${COLLECT}" >"${OUT_B}" 2>"${WORK}/err-b.txt"; then
    ok "collector exits 0 when a helper leaks the serial"
else
    bad "collector exits 0 when a helper leaks the serial"
fi
expect_absent "${OUT_B}" "${SERIAL}" "serial is absent when a helper prints it"
expect_line "${OUT_B}" "model_id=redacted" "leaking model value is replaced by redacted"
expect_line "${OUT_B}" "profile_file_name=redacted" "file name built from a leaked value is redacted"
if KAIA_FAKE_ROOT="${FIXTURE_B}" sh "${COLLECT}" --self-test >"${WORK}/self-b.txt" 2>&1; then
    ok "self-test passes on fixture B"
else
    bad "self-test passes on fixture B"
    cat "${WORK}/self-b.txt"
fi

echo "== argument handling"
KAIA_FAKE_ROOT='' sh "${COLLECT}" --self-test >/dev/null 2>&1
if [ $? -eq 2 ]; then
    ok "self-test refuses to run without KAIA_FAKE_ROOT (exit 2)"
else
    bad "self-test refuses to run without KAIA_FAKE_ROOT (exit 2)"
fi
if sh "${COLLECT}" --bogus >/dev/null 2>&1; then
    bad "unknown argument exits non-zero"
else
    ok "unknown argument exits non-zero"
fi

echo
echo "passed: ${PASS}  failed: ${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
    exit 1
fi
exit 0
