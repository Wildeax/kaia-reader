#!/bin/sh
# Tests the release signing and verification path with a throwaway key and
# fake packages. It makes no network calls, uses no GitHub release and never
# reads the real release key.
#
# Usage: sh tools/kaia/release/test-release.sh
#
# Exit status: 0 when every check behaves as expected. 1 otherwise.

set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
VERIFY="${HERE}/verify-release.sh"
SIGN="${HERE}/sign-release.sh"
TMP=$(mktemp -d "${TMPDIR:-/tmp}/kaia-release-test.XXXXXX")
trap 'rm -rf "${TMP}"' EXIT

pass() {
    echo "PASS: $*"
}

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

sha256_list() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$@"
    else
        shasum -a 256 "$@"
    fi
}

# Runs a command that must fail. Its output stays in last.out for checks.
expect_reject() {
    what=$1
    shift
    if "$@" >"${TMP}/last.out" 2>&1; then
        cat "${TMP}/last.out" >&2
        fail "${what} was accepted"
    fi
    pass "${what} is rejected"
}

# Runs a command that must succeed.
expect_accept() {
    what=$1
    shift
    if ! "$@" >"${TMP}/last.out" 2>&1; then
        cat "${TMP}/last.out" >&2
        fail "${what} was rejected"
    fi
    pass "${what} is accepted"
}

verify() {
    env KAIA_ALLOWED_SIGNERS="${TMP}/allowed_signers" sh "${VERIFY}" "$@"
}

# Throwaway keys, with no passphrase, in the test folder only.
ssh-keygen -q -t ed25519 -N "" -C "test-release@kaia-reader" -f "${TMP}/key" </dev/null
ssh-keygen -q -t ed25519 -N "" -f "${TMP}/other" </dev/null
printf 'release@kaia-reader namespaces="kaia-release" %s\n' \
    "$(cut -d ' ' -f 1,2 "${TMP}/key.pub")" >"${TMP}/allowed_signers"
printf '# no signing key recorded\n' >"${TMP}/empty_signers"

# A fake release: two packages, their sums and the signature over the sums.
RELEASE="${TMP}/release"
mkdir "${RELEASE}"
printf 'fake kobo package for the test\n' >"${RELEASE}/kaia-kobo-test-0000.zip"
printf 'fake kobov5 package for the test\n' >"${RELEASE}/kaia-kobo-v5-test-0000.zip"
(cd "${RELEASE}" && sha256_list kaia-kobo-test-0000.zip kaia-kobo-v5-test-0000.zip >SHA256SUMS)
ssh-keygen -Y sign -f "${TMP}/key" -n kaia-release "${RELEASE}/SHA256SUMS" >/dev/null

expect_accept "a signed release with matching files" verify "${RELEASE}"

# Tamper with one package.
cp -R "${RELEASE}" "${TMP}/t-file"
printf 'x' >>"${TMP}/t-file/kaia-kobo-test-0000.zip"
expect_reject "a package with one changed byte" verify "${TMP}/t-file"

# Tamper with the signature: change one character in the base64 body.
cp -R "${RELEASE}" "${TMP}/t-sig"
awk 'NR == 3 && !done { c = substr($0, 1, 1); r = (c == "A") ? "B" : "A"; $0 = r substr($0, 2); done = 1 } { print }' \
    "${RELEASE}/SHA256SUMS.sig" >"${TMP}/t-sig/SHA256SUMS.sig"
cmp -s "${RELEASE}/SHA256SUMS.sig" "${TMP}/t-sig/SHA256SUMS.sig" &&
    fail "the signature tamper did not change the signature file"
expect_reject "a signature with one changed character" verify "${TMP}/t-sig"

# Tamper with the sums: the signature covers SHA256SUMS, so this must fail too.
cp -R "${RELEASE}" "${TMP}/t-sums"
printf 'x  extra.zip\n' >>"${TMP}/t-sums/SHA256SUMS"
expect_reject "SHA256SUMS with an added line" verify "${TMP}/t-sums"

# A file that the sums do not list.
cp -R "${RELEASE}" "${TMP}/t-extra"
printf 'unlisted\n' >"${TMP}/t-extra/unlisted.zip"
expect_reject "a folder with an unlisted file" verify "${TMP}/t-extra"

# Signed by a key that the signer list does not contain.
cp -R "${RELEASE}" "${TMP}/t-key"
rm "${TMP}/t-key/SHA256SUMS.sig"
ssh-keygen -Y sign -f "${TMP}/other" -n kaia-release "${TMP}/t-key/SHA256SUMS" >/dev/null
expect_reject "a release signed by another key" verify "${TMP}/t-key"

# No key in the signer list yet.
expect_reject "verification with no signing key recorded" \
    env KAIA_ALLOWED_SIGNERS="${TMP}/empty_signers" sh "${VERIFY}" "${RELEASE}"
grep -q 'no signing key is recorded' "${TMP}/last.out" ||
    fail "the no-key message is missing"

# sign-release.sh refuses before it touches GitHub in these cases.
expect_reject "a tag that is not a version" sh "${SIGN}" main
expect_reject "an upstream KOReader style tag" sh "${SIGN}" v2026.07.1
expect_reject "signing with a key file that does not exist" \
    env KAIA_RELEASE_KEY="${TMP}/no-such-key" sh "${SIGN}" kaia-v0.0.0-test
grep -q 'ssh-keygen -t ed25519' "${TMP}/last.out" ||
    fail "the missing-key message does not print the key command"
expect_reject "signing with a key that has no passphrase" \
    env KAIA_RELEASE_KEY="${TMP}/key" sh "${SIGN}" kaia-v0.0.0-test
grep -q 'has no passphrase' "${TMP}/last.out" ||
    fail "the no-passphrase message is missing"

# The no-passphrase guard relies on this: an encrypted key does not read with an
# empty passphrase, so the guard passes it on.
ssh-keygen -q -t ed25519 -N "test-only-passphrase" -f "${TMP}/enc" </dev/null
if ssh-keygen -y -P "" -f "${TMP}/enc" </dev/null >/dev/null 2>&1; then
    fail "an encrypted key read with an empty passphrase"
fi
pass "an encrypted key does not read with an empty passphrase"

echo "All release checks passed."
