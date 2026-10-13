#!/bin/sh
#
# restore.sh: restore the Kaia application directory from a backup made by
# backup.sh. Runs on this computer with the Kobo mounted over USB.
#
# Usage:
#   sh restore.sh <backup-dir> <mount-path>            dry run, writes nothing
#   sh restore.sh <backup-dir> <mount-path> --apply    restore
#
# What it does:
#   1. Checks the backup manifest. A mismatch stops the script before any write.
#   2. Renames the current .adds/kaia to .adds/kaia.replaced-<UTC time>.
#      Nothing is deleted, so the failed version stays available for diagnosis.
#   3. Copies .adds/kaia from the backup and checks every restored file.
#
# What it never does:
#   - delete or modify book files, sidecars, .kobo or .adds/koreader
#   - restore .kobo/KoboReader.sqlite (Nickel's database is not restored here)
#
# The only deletion is the rollback of a partial copy inside .adds/kaia when a
# copy fails. That step removes the new partial copy and moves the replaced
# directory back.

set -eu

export COPYFILE_DISABLE=1

usage() {
    cat <<'EOF'
Usage: restore.sh <backup-dir> <mount-path> [--apply]

Without --apply, print the plan and write nothing.
EOF
}

die() {
    echo "restore: $*" >&2
    exit 1
}

info() {
    echo "restore: $*"
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

# check_manifest BASE MANIFEST PREFIX: every manifest entry whose path starts
# with PREFIX must exist under BASE and match its recorded hash. Prints the
# number of checked files on success and exits on the first mismatch.
check_manifest() {
    _base="$1"
    _manifest="$2"
    _prefix="$3"
    _checked=0
    while IFS=' ' read -r _hash _path; do
        case "${_path}" in
            "${_prefix}"*) ;;
            *) continue ;;
        esac
        if [ ! -f "${_base}/${_path}" ]; then
            die "missing file: ${_path}"
        fi
        if [ "$(sha256_file "${_base}/${_path}")" != "${_hash}" ]; then
            die "hash mismatch: ${_path}"
        fi
        _checked=$((_checked + 1))
    done <"${_manifest}"
    echo "${_checked}"
}

APPLY=0
NPOS=0
BACKUP_IN=""
MOUNT_IN=""
for _arg in "$@"; do
    case "${_arg}" in
        --apply)
            APPLY=1
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        -*)
            usage >&2
            die "unknown option: ${_arg}"
            ;;
        *)
            NPOS=$((NPOS + 1))
            if [ "${NPOS}" -eq 1 ]; then
                BACKUP_IN="${_arg}"
            elif [ "${NPOS}" -eq 2 ]; then
                MOUNT_IN="${_arg}"
            fi
            ;;
    esac
done

if [ "${NPOS}" -ne 2 ]; then
    usage >&2
    exit 2
fi

BACKUP="$(cd "${BACKUP_IN}" 2>/dev/null && pwd -P)" || die "backup directory not found: ${BACKUP_IN}"
[ -d "${MOUNT_IN}" ] || die "mount path is not a directory: ${MOUNT_IN}"
MOUNT="$(cd "${MOUNT_IN}" && pwd -P)"

[ -f "${BACKUP}/MANIFEST.sha256" ] || die "no MANIFEST.sha256 in ${BACKUP}"
[ -f "${BACKUP}/BACKUP_INFO.txt" ] || die "no BACKUP_INFO.txt in ${BACKUP}"
[ -d "${BACKUP}/.adds/kaia" ] || die "backup has no .adds/kaia directory"

[ -f "${MOUNT}/.kobo/version" ] || die "not a Kobo volume (no .kobo/version): ${MOUNT}"
[ -d "${MOUNT}/.adds" ] || die "volume has no .adds directory: ${MOUNT}"

case "${BACKUP}" in
    "${MOUNT}" | "${MOUNT}"/*)
        die "the backup must not be on the Kobo volume"
        ;;
esac

info "checking backup manifest"
_total="$(check_manifest "${BACKUP}" "${BACKUP}/MANIFEST.sha256" "")"
info "manifest ok: ${_total} files"

_kaia_files="$(check_manifest "${BACKUP}" "${BACKUP}/MANIFEST.sha256" ".adds/kaia/")"
_need_kb="$(du -sk "${BACKUP}/.adds/kaia" | awk '{ print $1 }')"
_free_kb="$(df -P -k "${MOUNT}" | awk 'NR == 2 { print $4 }')"

STAMP="$(date -u '+%Y%m%dT%H%M%SZ')"
REPLACED="${MOUNT}/.adds/kaia.replaced-${STAMP}"

info "plan"
info "  backup:        ${BACKUP}"
info "  target:        ${MOUNT}/.adds/kaia"
info "  files to copy: ${_kaia_files}"
info "  size to copy:  ${_need_kb} kB, free on volume: ${_free_kb} kB"
if [ -e "${MOUNT}/.adds/kaia" ]; then
    info "  current .adds/kaia is renamed to: ${REPLACED}"
else
    info "  no current .adds/kaia; nothing is renamed"
fi
info "  not touched: books, sidecars, .kobo, .adds/koreader, .adds/nm"

if [ "${APPLY}" -eq 0 ]; then
    info "dry run: nothing written. Run again with --apply to restore."
    exit 0
fi

if [ -e "${REPLACED}" ]; then
    die "${REPLACED} already exists; move it aside and retry"
fi
if [ "${_free_kb}" -lt $((_need_kb + 4096)) ]; then
    die "not enough free space on the volume (need ${_need_kb} kB plus 4096 kB margin)"
fi

PARTIAL=0
MOVED=0
rollback() {
    if [ "${PARTIAL}" -eq 1 ]; then
        rm -rf "${MOUNT}/.adds/kaia"
    fi
    if [ "${MOVED}" -eq 1 ]; then
        mv "${REPLACED}" "${MOUNT}/.adds/kaia"
        info "rolled back: original .adds/kaia restored"
    fi
}

if [ -e "${MOUNT}/.adds/kaia" ]; then
    mv "${MOUNT}/.adds/kaia" "${REPLACED}"
    MOVED=1
    info "renamed current .adds/kaia to ${REPLACED}"
fi

PARTIAL=1
if ! cp -R "${BACKUP}/.adds/kaia" "${MOUNT}/.adds/kaia"; then
    rollback
    die "copy failed; rolled back"
fi

if ! check_manifest "${MOUNT}" "${BACKUP}/MANIFEST.sha256" ".adds/kaia/" >/dev/null; then
    rollback
    die "restored files do not match the manifest; rolled back"
fi
PARTIAL=0

sync
info "restored ${_kaia_files} files to ${MOUNT}/.adds/kaia"
if [ "${MOVED}" -eq 1 ]; then
    info "the replaced copy is kept at ${REPLACED}"
    info "delete it by hand after Kaia starts from the restored copy"
fi
info "next: eject the volume cleanly, then reboot the Kobo"
