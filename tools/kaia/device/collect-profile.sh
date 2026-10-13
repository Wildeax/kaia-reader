#!/bin/sh
#
# collect-profile.sh: write a sanitized Kobo hardware profile to stdout.
#
# Runs under BusyBox sh on the device (over SSH or from a launcher). On a
# desktop it runs against a fake root tree when KAIA_FAKE_ROOT is set.
#
# Output is key=value lines. The script does not read the Kobo account
# database, Wi-Fi settings or network addresses. It also drops any value that
# contains the device serial, so a misbehaving helper cannot leak it.
#
# Usage:
#   sh collect-profile.sh               print the profile
#   sh collect-profile.sh --self-test   collect from KAIA_FAKE_ROOT and check
#                                       the leak rules
#
# Environment:
#   KAIA_FAKE_ROOT  read device paths below this directory instead of /

set -u

PROFILE_FORMAT=1
ROOT="${KAIA_FAKE_ROOT:-}"
ONBOARD="${ROOT}/mnt/onboard"
VERSION_FILE="${ONBOARD}/.kobo/version"
SERIAL=""

usage() {
    cat <<'EOF'
Usage: collect-profile.sh [--self-test]

Without arguments, print a sanitized hardware profile as key=value lines.

  --self-test  collect a profile from KAIA_FAKE_ROOT and check it against the
               leak rules: serial, MAC address, Wi-Fi name, e-mail address and
               database-style serial numbers. Needs KAIA_FAKE_ROOT.
EOF
}

# Keep only characters that can appear in a device token.
clean() {
    tr -cd 'A-Za-z0-9._/+,-'
}

# Print the first line of a file without carriage returns, or nothing.
first_line() {
    head -n 1 "$1" 2>/dev/null | tr -d '\r'
}

# Print one profile line. A value that contains the serial becomes "redacted".
emit() {
    _v="$2"
    if [ -n "${_v}" ] && [ -n "${SERIAL}" ]; then
        case "${_v}" in
            *"${SERIAL}"*) _v="redacted" ;;
        esac
    fi
    [ -n "${_v}" ] || _v="unknown"
    printf '%s=%s\n' "$1" "${_v}"
}

# The serial is the first field of the version file. It is only used for
# redaction and is never printed.
load_serial() {
    SERIAL="$(first_line "${VERSION_FILE}" | cut -d, -f1)"
}

# Print a value that rcS exports to udevd, if the caller runs on the device.
udevd_env() {
    [ -z "${ROOT}" ] || return 0
    command -v pidof >/dev/null 2>&1 || return 0
    _pid="$(pidof -s udevd 2>/dev/null)"
    [ -n "${_pid}" ] || return 0
    tr '\0' '\n' <"/proc/${_pid}/environ" 2>/dev/null |
        awk -F= -v k="$1" '$1 == k { print substr($0, length(k) + 2); exit }'
}

# Product id, the same order of lookup as the upstream launcher, with
# kobo_config.sh first.
model_id() {
    _m=""
    if [ -x "${ROOT}/bin/kobo_config.sh" ]; then
        _m="$("${ROOT}/bin/kobo_config.sh" 2>/dev/null | head -n 1 | clean)"
    fi
    if [ -z "${_m}" ]; then
        _m="$(udevd_env PRODUCT | clean)"
    fi
    if [ -z "${_m}" ] && [ -x "${ROOT}/usr/bin/hwdetect.sh" ]; then
        _m="$("${ROOT}/usr/bin/hwdetect.sh" 2>/dev/null | head -n 1 | clean)"
    fi
    printf '%s' "${_m}"
}

# Firmware version is field 3 of the version file. The upstream launcher reads
# the same field for its "Running FW" log line.
firmware_version() {
    first_line "${VERSION_FILE}" | cut -d, -f3 | clean
}

# Print the value of a /proc/cpuinfo key, first match only.
cpu_field() {
    awk -v re="$1" '
        $0 ~ ("^(" re ")[ \t]*:") && !done {
            sub(/^[^:]*:[ \t]*/, "")
            print
            done = 1
        }
    ' "${ROOT}/proc/cpuinfo" 2>/dev/null | tr -d '\r'
}

# Print yes, no or unknown for a word in a space-separated feature list.
has_feature() {
    if [ -z "$1" ]; then
        printf 'unknown'
        return 0
    fi
    case " $1 " in
        *" $2 "*) printf 'yes' ;;
        *) printf 'no' ;;
    esac
}

# Print the dynamic loader paths found under /lib, comma separated.
dynamic_loaders() {
    _list=""
    for _f in "${ROOT}"/lib/ld-linux*; do
        if [ -e "${_f}" ]; then
            _list="${_list:+${_list},}${_f#"${ROOT}"}"
        fi
    done
    printf '%s' "${_list}"
}

# Hint from the loader name. The loader is a hint only. The ELF check below
# reads the KOReader binary itself.
loader_float_hint() {
    _first="${1%%,*}"
    case "${_first}" in
        */ld-linux-armhf.so.*) printf 'hard' ;;
        */ld-linux.so.*) printf 'soft' ;;
        *) printf 'unknown' ;;
    esac
}

# glibc version from the banner string in libc.so.6. This avoids running the
# loader, which keeps the check safe on any firmware.
glibc_version() {
    grep -a -o 'release version [0-9][0-9.]*' "${ROOT}/lib/libc.so.6" 2>/dev/null |
        head -n 1 | sed -e 's/^release version //' -e 's/\.$//' | clean
}

# Float ABI of an ARM ELF file from e_flags (EF_ARM_ABI_FLOAT_HARD 0x400,
# EF_ARM_ABI_FLOAT_SOFT 0x200). e_flags sits at offset 36 in a 32-bit
# little-endian ELF, so byte 37 holds bits 8 to 15.
binary_float_abi() {
    [ -r "$1" ] || return 0
    _magic="$(dd if="$1" bs=1 count=4 2>/dev/null | od -An -tx1 | tr -d ' \n')"
    [ "${_magic}" = "7f454c46" ] || return 0
    _flags_b1="$(dd if="$1" bs=1 skip=37 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')"
    case "${_flags_b1}" in
        04) printf 'hard' ;;
        02) printf 'soft' ;;
        *) return 0 ;;
    esac
}

# Print the /proc/meminfo value in kB for one key.
meminfo_kb() {
    awk -v k="$1:" '$1 == k { print $2; exit }' "${ROOT}/proc/meminfo" 2>/dev/null |
        tr -cd '0-9'
}

# Print the value of a framebuffer attribute from sysfs.
fb_value() {
    first_line "${ROOT}/sys/class/graphics/fb0/$1" | tr -cd '0-9,'
}

# Print the key=value pairs from fbink -e, prefixed with fbink_. The first
# binary found in the KOReader or Kaia directory is used.
fbink_info() {
    for _d in koreader kaia; do
        if [ -x "${ONBOARD}/.adds/${_d}/fbink" ]; then
            "${ONBOARD}/.adds/${_d}/fbink" -e 2>/dev/null | tr ';' '\n' |
                grep -E '^[A-Za-z0-9_]+=[A-Za-z0-9._,+-]*$' | sed -e 's/^/fbink_/'
            return 0
        fi
    done
    return 0
}

# Print "total_kb free_kb" for the storage that holds the books.
storage_onboard() {
    df -P -k "${ONBOARD}" 2>/dev/null | awk 'NR == 2 { print $2, $4 }'
}

# Print yes or no for a directory.
dir_state() {
    if [ -d "$1" ]; then
        printf 'yes'
    else
        printf 'no'
    fi
}

# Remove the serial from the whole output and drop lines that look like an
# e-mail address, a MAC address or a Wi-Fi network name.
redact() {
    awk -v serial="${SERIAL}" '
        serial != "" && index($0, serial) > 0 { next }
        { print }
    ' | grep -v -E -e '@' -e '([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}' -e '[Ss][Ss][Ii][Dd]' || true
}

build_profile() {
    _model="$(model_id)"
    _fw="$(firmware_version)"
    _features="$(cpu_field 'Features')"
    _loaders="$(dynamic_loaders)"
    _storage="$(storage_onboard)"
    _total_kb="$(printf '%s' "${_storage}" | cut -d' ' -f1 | tr -cd '0-9')"
    _free_kb="$(printf '%s' "${_storage}" | cut -d' ' -f2 | tr -cd '0-9')"
    _elf="${ONBOARD}/.adds/kaia/luajit"
    [ -r "${_elf}" ] || _elf="${ONBOARD}/.adds/koreader/luajit"
    if [ -n "${_model}" ] && [ -n "${_fw}" ]; then
        _pname="${_model}-${_fw}.txt"
    else
        _pname=""
    fi

    emit profile_format "${PROFILE_FORMAT}"
    emit collected_utc "$(date -u '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null)"
    emit model_id "${_model}"
    emit platform "$(udevd_env PLATFORM | clean)"
    emit firmware_version "${_fw}"
    emit kernel_release "$(uname -r 2>/dev/null | clean)"
    emit kernel_machine "$(uname -m 2>/dev/null | clean)"
    emit cpu_model "$(cpu_field 'Processor|model name')"
    emit cpu_architecture "$(cpu_field 'CPU architecture' | clean)"
    emit cpu_features "${_features}"
    emit cpu_vfp "$(has_feature "${_features}" vfp)"
    emit cpu_neon "$(has_feature "${_features}" neon)"
    emit dynamic_loader "${_loaders}"
    emit loader_float_abi_hint "$(loader_float_hint "${_loaders}")"
    emit glibc_version "$(glibc_version)"
    emit koreader_elf_float_abi "$(binary_float_abi "${_elf}")"
    emit mem_total_kb "$(meminfo_kb MemTotal)"
    emit mem_available_kb "$(meminfo_kb MemAvailable)"
    emit fb_virtual_size "$(fb_value virtual_size)"
    emit fb_bits_per_pixel "$(fb_value bits_per_pixel)"
    emit fb_rotate "$(fb_value rotate)"
    fbink_info
    emit storage_onboard_total_kb "${_total_kb}"
    emit storage_onboard_free_kb "${_free_kb}"
    emit nickelmenu_present "$(dir_state "${ONBOARD}/.adds/nm")"
    emit kfmon_present "$(dir_state "${ONBOARD}/.adds/kfmon")"
    emit koreader_dir_present "$(dir_state "${ONBOARD}/.adds/koreader")"
    emit koreader_version "$(first_line "${ONBOARD}/.adds/koreader/git-rev" | clean)"
    emit kaia_dir_present "$(dir_state "${ONBOARD}/.adds/kaia")"
    emit kaia_version "$(first_line "${ONBOARD}/.adds/kaia/git-rev" | clean)"
    emit profile_file_name "${_pname}"
}

emit_profile() {
    load_serial
    build_profile | redact
}

self_test() {
    if [ -z "${ROOT}" ] || [ ! -d "${ROOT}" ]; then
        echo "self-test: set KAIA_FAKE_ROOT to an existing fake root directory" >&2
        return 2
    fi
    load_serial
    _out="${TMPDIR:-/tmp}/kaia-collect-profile-self-test.$$"
    if ! emit_profile >"${_out}"; then
        echo "self-test: collection failed" >&2
        rm -f "${_out}"
        return 1
    fi

    _status=0
    for _key in profile_format model_id firmware_version kernel_release \
        kernel_machine cpu_features dynamic_loader glibc_version \
        mem_total_kb mem_available_kb fb_virtual_size storage_onboard_free_kb \
        nickelmenu_present kfmon_present koreader_version kaia_dir_present; do
        if ! grep -q "^${_key}=" "${_out}"; then
            echo "self-test: missing key ${_key}" >&2
            _status=1
        fi
    done
    if [ -n "${SERIAL}" ] && grep -q -F -- "${SERIAL}" "${_out}"; then
        echo "self-test: device serial found in profile" >&2
        _status=1
    fi
    if grep -q -E '([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}' "${_out}"; then
        echo "self-test: MAC-address-like value found in profile" >&2
        _status=1
    fi
    if grep -q -i 'ssid' "${_out}"; then
        echo "self-test: Wi-Fi name found in profile" >&2
        _status=1
    fi
    if grep -q '@' "${_out}"; then
        echo "self-test: e-mail-like value found in profile" >&2
        _status=1
    fi
    if grep -q -E 'N[0-9]{10,}' "${_out}"; then
        echo "self-test: serial-like number found in profile" >&2
        _status=1
    fi
    if grep -q -v -E '^[A-Za-z0-9_]+=' "${_out}"; then
        echo "self-test: line is not key=value" >&2
        _status=1
    fi

    rm -f "${_out}"
    if [ "${_status}" -eq 0 ]; then
        echo "self-test: ok"
    fi
    return "${_status}"
}

rc=0
case "${1:-}" in
    "")
        emit_profile || rc=1
        ;;
    --self-test)
        self_test || rc=$?
        ;;
    -h | --help)
        usage
        ;;
    *)
        usage >&2
        rc=2
        ;;
esac
exit "${rc}"
