#!/bin/sh
# Checks a folder of Kaia release files before you install a package.
#
# Usage: verify-release.sh DIR
#
# DIR holds SHA256SUMS, SHA256SUMS.sig and the files that SHA256SUMS lists,
# and no other files. The script checks the signature on SHA256SUMS with the
# signer list in this checkout, then checks every file against its hash.
#
# Environment:
#   KAIA_ALLOWED_SIGNERS  Signer list to use instead of
#                         docs/kaia/release/allowed_signers in this checkout.
#
# Exit status: 0 when both checks pass. 1 when a check fails. 2 on a usage error.

set -eu

IDENTITY="release@kaia-reader"
NAMESPACE="kaia-release"
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "${HERE}/../../.." && pwd)
SIGNERS="${KAIA_ALLOWED_SIGNERS:-${ROOT}/docs/kaia/release/allowed_signers}"

die() {
    echo "verify-release: $*" >&2
    exit 1
}

sha256_check() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -c "$1"
    else
        shasum -a 256 -c "$1"
    fi
}

# Prints the names of the files in a folder, except the two sum files.
folder_files() {
    (cd "$1" && find . -mindepth 1 -maxdepth 1 -type f | sed 's|^\./||' |
        grep -v -x -e SHA256SUMS -e SHA256SUMS.sig | LC_ALL=C sort)
}

# Prints the file names that SHA256SUMS lists, sorted.
sums_files() {
    sed 's/^[0-9a-f]\{64\} [ *]//' "$1" | LC_ALL=C sort
}

# Succeeds when the folder holds exactly the files that SHA256SUMS lists and
# every hash matches.
check_sums() {
    [ "$(folder_files "$1")" = "$(sums_files "$1/SHA256SUMS")" ] || {
        echo "files in $1 do not match the names in SHA256SUMS" >&2
        return 1
    }
    (cd "$1" && sha256_check SHA256SUMS) || return 1
}

[ "$#" -eq 1 ] || {
    echo "usage: verify-release.sh DIR" >&2
    exit 2
}
DIR=$1
[ -d "${DIR}" ] || die "not a folder: ${DIR}"
[ -f "${DIR}/SHA256SUMS" ] || die "no SHA256SUMS in ${DIR}"
[ -f "${DIR}/SHA256SUMS.sig" ] || die "no SHA256SUMS.sig in ${DIR}"
[ -f "${SIGNERS}" ] || die "signer list not found: ${SIGNERS}"
grep -q '^[^#[:space:]]' "${SIGNERS}" ||
    die "no signing key is recorded in ${SIGNERS}. The release key is not created or committed yet, so no release can pass this check."

# 1. The signature on SHA256SUMS. The sums cover every file in the folder.
if ! ssh-keygen -Y verify -f "${SIGNERS}" -I "${IDENTITY}" -n "${NAMESPACE}" \
    -s "${DIR}/SHA256SUMS.sig" <"${DIR}/SHA256SUMS"; then
    die "the signature on SHA256SUMS does not verify. Do not install these files."
fi

# 2. Every file against its hash, and no file that the sums do not list.
check_sums "${DIR}" || die "the files do not match SHA256SUMS. Do not install them."
echo "OK: the signature and every file match SHA256SUMS."
