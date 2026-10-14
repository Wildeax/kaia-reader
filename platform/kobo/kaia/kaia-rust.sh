#!/bin/sh
# Start the Rust app and restore Nickel after it exits.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)"
export KOREADER_DIR="${KOREADER_DIR:-${SCRIPT_DIR}}"
# The two overrides below are for platform/kobo/kaia/test-kaia-rust.sh. The device uses the defaults.
KAIA_DATA_DIR="${KAIA_DATA_DIR:-/mnt/onboard/.kaia}"
KAIA_RUST_TMP="${KAIA_RUST_TMP:-/tmp}"
export KAIA_RESOURCES="${KOREADER_DIR}/resources"
export LD_LIBRARY_PATH="${KOREADER_DIR}/libs${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export LC_ALL=en_US.UTF-8
export RUST_BACKTRACE=1

# Keep the launcher outside onboard while Nickel takes over USB storage.
if [ "${SCRIPT_DIR}" != "${KAIA_RUST_TMP}" ] && [ "${SCRIPT_DIR}" != /tmp ] && [ "${SCRIPT_DIR}" != /var/volatile/tmp ]; then
    cp -pf "$0" "${KAIA_RUST_TMP}/kaia-rust.sh" || exit 1
    chmod 755 "${KAIA_RUST_TMP}/kaia-rust.sh"
    exec "${KAIA_RUST_TMP}/kaia-rust.sh" "$@"
fi
mkdir "${KAIA_RUST_TMP}/kaia-rust.lock" 2>/dev/null || exit 1
mkdir -p "${KAIA_DATA_DIR}" || {
    rmdir "${KAIA_RUST_TMP}/kaia-rust.lock"
    exit 1
}
LOG="${KAIA_DATA_DIR}/rust.log"
if [ -f "${LOG}" ]; then
    tail -c 500000 "${LOG}" >"${LOG}.new"
    mv -f "${LOG}.new" "${LOG}"
fi
exec >>"${LOG}" 2>&1

VIA_NICKEL=false
APP_PID=
ORIG_FB_BPP=
ORIG_FB_ROTA=
# shellcheck disable=SC2329
cleanup() {
    trap - 0 INT TERM HUP
    if [ -n "${APP_PID}" ]; then
        kill -TERM "${APP_PID}" 2>/dev/null
        wait "${APP_PID}" 2>/dev/null
    fi
    if [ -n "${ORIG_FB_ROTA}" ]; then
        if [ -n "${ORIG_FB_BPP}" ]; then
            ./fbdepth -d "${ORIG_FB_BPP}" -r "${ORIG_FB_ROTA}"
        else
            ./fbdepth -r "${ORIG_FB_ROTA}"
        fi
    fi
    sync
    rm -f "${KAIA_RUST_TMP}/kaia-rust.sh"
    rmdir "${KAIA_RUST_TMP}/kaia-rust.lock"
    if [ "${VIA_NICKEL}" = true ]; then
        # Close the onboard log before Nickel offers to unmount storage.
        ./nickel.sh </dev/null >/dev/null 2>&1 &
    fi
}
trap cleanup 0
trap 'exit 143' TERM HUP
trap 'exit 130' INT
cd "${KOREADER_DIR}" || exit 1

# Read only the variables used by Kobo's platform scripts.
read_kobo_env() {
    while IFS= read -r entry; do
        case "${entry}" in
            DBUS_SESSION_BUS_ADDRESS=* | NICKEL_HOME=* | WIFI_MODULE=* | LANG=* | INTERFACE=* | PRODUCT=* | PLATFORM=*) export "${entry?}" ;;
        esac
    done <<ENV
$(tr '\000' '\n' <"/proc/$1/environ" 2>/dev/null)
ENV
}
if pkill -0 nickel; then
    VIA_NICKEL=true
    read_kobo_env "$(pidof -s nickel)"
    sync
    # Match kaia.sh's process list and bounded wait.
    killall -q -TERM nickel hindenburg sickel fickel strickel fontickel adobehost foxitpdf iink dhcpcd-dbus dhcpcd bluealsa bluetoothd fmon nanoclock.lua memorylogger QtWebEngineProcess
    kill_timeout=0
    while pkill -0 nickel; do
        [ "${kill_timeout}" -ge 15 ] && break
        usleep 250000
        kill_timeout=$((kill_timeout + 1))
    done
    rm -f /tmp/nickel-hardware-status
fi
read_kobo_env "$(pidof -s udevd)"
if [ -z "${PRODUCT}" ]; then
    PRODUCT="$(/bin/kobo_config.sh 2>/dev/null)"
fi
if [ -z "${PRODUCT}" ]; then
    if [ -e /usr/bin/hwdetect ]; then
        # shellcheck disable=SC1091
        . /usr/libexec/platform/utils.sh
        PRODUCT="$(get_product_name)"
    else
        PRODUCT="$(/usr/bin/hwdetect.sh 2>/dev/null)"
    fi
fi
export PRODUCT
if [ -z "${PLATFORM}" ]; then
    PLATFORM=freescale
    if dd if=/dev/mmcblk0 bs=512 skip=1024 count=1 | grep -q 'HW CONFIG'; then
        CPU="$(ntx_hwconfig -s -p /dev/mmcblk0 CPU 2>/dev/null)"
        PLATFORM="${CPU}-ntx"
    fi
    if [ "${PLATFORM}" != freescale ] && [ ! -e "/etc/u-boot/${PLATFORM}/u-boot.mmc" ]; then
        PLATFORM=ntx508
    fi
fi
export PLATFORM
export INTERFACE="${INTERFACE:-eth0}"

IFS= read -r ORIG_FB_ROTA </sys/class/graphics/fb0/rotate
IFS= read -r ORIG_FB_BPP </sys/class/graphics/fb0/bits_per_pixel
case "${ORIG_FB_BPP}" in
    8 | 16 | 32) ;;
    *) ORIG_FB_BPP= ;;
esac
echo "Rust app start: $(date), product ${PRODUCT}, platform ${PLATFORM}"
echo "Original framebuffer: ${ORIG_FB_BPP} bpp, rotation ${ORIG_FB_ROTA}"
if [ "${PLATFORM}" = b300-ntx ]; then
    export FBINK_FORCE_ROTA=0
    ./fbdepth -R UR || exit 1
    ORIG_FB_BPP=
else
    hasColorPanel="$(./fbink -e | tr ';' '\n' | sed -n 's/^hasColorPanel=//p')"
    if [ "${hasColorPanel}" = 1 ]; then
        ./fbdepth -d 32 -R UR || exit 1
    elif [ -n "${ORIG_FB_BPP}" ]; then
        ./fbdepth -d 8 -R UR || exit 1
    fi
fi
# Safe mode, as in frontend/kaia/safemode.lua. The count of launches that have not
# reached a healthy start lives in the data folder. The app clears it after a healthy
# start (rust/kaia-app/src/safe_mode.rs). A clean exit clears it here.
# Three failed starts in a row make the fourth start safe mode.
SAFE_MODE_MARKER="${KAIA_DATA_DIR}/safemode"
SAFE_MODE_COUNTER="${KAIA_DATA_DIR}/kaia-startup-attempts"
SAFE_MODE_LIMIT=3
ATTEMPTS=0
if [ -f "${SAFE_MODE_COUNTER}" ]; then
    read -r ATTEMPTS <"${SAFE_MODE_COUNTER}" || ATTEMPTS=0
fi
case "${ATTEMPTS}" in
    '' | *[!0-9]*) ATTEMPTS=0 ;;
esac
ATTEMPTS=$((ATTEMPTS + 1))
printf '%s\n' "${ATTEMPTS}" >"${SAFE_MODE_COUNTER}"
SAFE_MODE=
if [ -f "${SAFE_MODE_MARKER}" ]; then
    SAFE_MODE=marker
elif [ "${ATTEMPTS}" -gt "${SAFE_MODE_LIMIT}" ]; then
    SAFE_MODE=failures
fi
echo "Kaia start ${ATTEMPTS}, safe mode ${SAFE_MODE:-off}"

if [ -n "${SAFE_MODE}" ]; then
    ./kaia --data-dir "${KAIA_DATA_DIR}" --safe-mode "$@" &
else
    ./kaia --data-dir "${KAIA_DATA_DIR}" "$@" &
fi
APP_PID=$!
wait "${APP_PID}"
RETURN_VALUE=$?
APP_PID=
echo "Rust app exit: ${RETURN_VALUE}"
if [ "${RETURN_VALUE}" -eq 0 ]; then
    rm -f "${SAFE_MODE_COUNTER}"
fi
exit "${RETURN_VALUE}"
