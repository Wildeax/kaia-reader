#!/bin/sh
# Signs the draft GitHub release for TAG with the Kaia release key.
#
# Usage: sign-release.sh TAG
#
# Run this on the maintainer's computer, never in CI. The script:
#   1. checks that the key exists and has a passphrase,
#   2. checks that the release is still a draft and has no signature yet,
#   3. downloads the draft assets and checks every file against SHA256SUMS,
#   4. signs SHA256SUMS, which covers every file through its sums,
#   5. verifies the signature with docs/kaia/release/allowed_signers,
#   6. uploads SHA256SUMS.sig to the draft.
# It never publishes the release. Publish it by hand on GitHub.
#
# Environment:
#   KAIA_RELEASE_KEY  Private key file. Default: ~/.kaia-release/kaia-release-ed25519
#
# Exit status: 0 when the signature is uploaded. 1 when a check fails.
# 2 on a usage error.

set -eu

REPO="Wildeax/kaia-reader"
NAMESPACE="kaia-release"
KEY="${KAIA_RELEASE_KEY:-${HOME}/.kaia-release/kaia-release-ed25519}"
HERE=$(cd "$(dirname "$0")" && pwd)
TAG=""
WORK=""

die() {
    echo "sign-release: $*" >&2
    exit 1
}

usage() {
    echo "usage: sign-release.sh TAG    (for example v1.2.3)" >&2
    exit 2
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

# Succeeds when the draft release exists and is still a draft.
require_draft() {
    if ! draft=$(gh release view "${TAG}" -R "${REPO}" --json isDraft --jq '.isDraft'); then
        die "no release ${TAG} in ${REPO}"
    fi
    [ "${draft}" = "true" ] || die "release ${TAG} is published. This script signs drafts only."
}

[ "$#" -eq 1 ] || usage
TAG=$1
printf '%s\n' "${TAG}" | grep -Eq '^kaia-v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$' || usage

# 1. The key must exist and must have a passphrase.
if [ ! -f "${KEY}" ]; then
    cat >&2 <<EOF
sign-release: release key not found: ${KEY}

The owner creates the key once, on the signing Mac. Run these commands and
choose a strong passphrase when ssh-keygen asks for one:

  mkdir -p ~/.kaia-release && chmod 700 ~/.kaia-release
  ssh-keygen -t ed25519 -a 100 -C release@kaia-reader -f ~/.kaia-release/kaia-release-ed25519

Then commit the public key and the signer list. docs/kaia/RELEASE_PROCESS.md
("Releases and signatures") has the exact steps.
EOF
    exit 1
fi

# ssh-keygen -y with an empty passphrase reads an unencrypted key and fails on
# an encrypted one. A key that reads this way has no passphrase and is refused.
if ssh-keygen -y -P "" -f "${KEY}" </dev/null >/dev/null 2>&1; then
    die "the release key has no passphrase: ${KEY}. Create a new key with a passphrase."
fi

# 2. The release must be a draft with no signature yet.
command -v gh >/dev/null 2>&1 || die "gh is not installed"
require_draft
if gh release view "${TAG}" -R "${REPO}" --json assets --jq '.assets[].name' |
    grep -qx 'SHA256SUMS.sig'; then
    die "release ${TAG} already has SHA256SUMS.sig. To sign again, delete it first: gh release delete-asset ${TAG} SHA256SUMS.sig -R ${REPO}"
fi

# 3. Download the draft and check every file against SHA256SUMS.
WORK=$(mktemp -d "${TMPDIR:-/tmp}/kaia-sign.XXXXXX")
trap 'rm -rf "${WORK}"' EXIT
gh release download "${TAG}" -R "${REPO}" -D "${WORK}" || die "download failed for ${TAG}"
[ -f "${WORK}/SHA256SUMS" ] || die "the draft ${TAG} has no SHA256SUMS"
check_sums "${WORK}" || die "the draft files do not match SHA256SUMS. Nothing was signed."

# 4. Sign SHA256SUMS. ssh-keygen asks for the passphrase.
echo "Signing SHA256SUMS for ${TAG} with ${KEY}."
ssh-keygen -Y sign -f "${KEY}" -n "${NAMESPACE}" "${WORK}/SHA256SUMS" || die "signing failed"

# 5. Verify with the committed signer list before anything is uploaded.
"${HERE}/verify-release.sh" "${WORK}" ||
    die "the new signature does not verify with docs/kaia/release/allowed_signers. Nothing was uploaded."

# 6. Upload the signature to the draft.
require_draft
gh release upload "${TAG}" "${WORK}/SHA256SUMS.sig" -R "${REPO}" || die "upload failed for ${TAG}"
echo "Uploaded SHA256SUMS.sig to the draft ${TAG} in ${REPO}."
echo "Check the draft on GitHub, then publish it by hand."
