#!/bin/sh
#
# restore.sh: restore the Kaia application and data directories from a backup
# made by backup.sh. Runs on this computer with the Kobo mounted over USB.
#
# Usage:
#   sh restore.sh <backup-dir> <mount-path>            dry run, writes nothing
#   sh restore.sh <backup-dir> <mount-path> --apply    restore
#
# The two directories restore as one unit:
#   .adds/kaia   Kaia application directory. The backup must contain it.
#   .kaia        Kaia data directory. Restored when the backup contains it.
#
# What it does:
#   1. Checks the backup manifest. A mismatch stops the script before any write.
#   2. Renames each current directory to <path>.replaced-<UTC time>, for
#      example .kaia.replaced-<UTC time>. Nothing is deleted, so the replaced
#      copy stays available for diagnosis.
#   3. Copies each directory from the backup and checks every restored file.
#   4. If a step fails, removes the new copies and moves the replaced
#      directories back. Both directories return to their state before the run.
#
# What it never does:
#   - delete or modify book files, sidecars, .kobo or .adds/koreader
#   - restore .kobo/KoboReader.sqlite (Nickel's database is not restored here)
#   - touch .kaia when the backup has no .kaia. An older backup keeps the
#     current data directory.
#
# The only deletion is the rollback of the new copies of .adds/kaia and .kaia.

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
# number of checked files on success. On the first mismatch it prints the
# problem to stderr and returns 1, so the caller can roll back before it exits.
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
            echo "restore: missing file: ${_path}" >&2
            return 1
        fi
        if [ "$(sha256_file "${_base}/${_path}")" != "${_hash}" ]; then
            echo "restore: hash mismatch: ${_path}" >&2
            return 1
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
_total="$(check_manifest "${BACKUP}" "${BACKUP}/MANIFEST.sha256" "")" || die "backup manifest does not match; nothing was written"
info "manifest ok: ${_total} files"

# Directories restored as one unit. A backup made before .kaia was added has no
# .kaia, so the current data directory stays as it is.
TREES=".adds/kaia"
if [ -d "${BACKUP}/.kaia" ]; then
    TREES=".adds/kaia .kaia"
else
    info "backup has no .kaia: the current .kaia, if any, is kept"
fi

_files=0
_need_kb=0
for _tree in ${TREES}; do
    _n="$(check_manifest "${BACKUP}" "${BACKUP}/MANIFEST.sha256" "${_tree}/")" || die "backup manifest does not match for ${_tree}"
    _files=$((_files + _n))
    _kb="$(du -sk "${BACKUP}/${_tree}" | awk '{ print $1 }')"
    _need_kb=$((_need_kb + _kb))
done
_free_kb="$(df -P -k "${MOUNT}" | awk 'NR == 2 { print $4 }')"

STAMP="$(date -u '+%Y%m%dT%H%M%SZ')"

info "plan"
info "  backup:        ${BACKUP}"
for _tree in ${TREES}; do
    info "  target:        ${MOUNT}/${_tree}"
done
info "  files to copy: ${_files}"
info "  size to copy:  ${_need_kb} kB, free on volume: ${_free_kb} kB"
for _tree in ${TREES}; do
    if [ -e "${MOUNT}/${_tree}" ]; then
        info "  current ${_tree} is renamed to: ${MOUNT}/${_tree}.replaced-${STAMP}"
    else
        info "  no current ${_tree}; nothing is renamed"
    fi
done
info "  not touched: books, sidecars, .kobo, .adds/koreader, .adds/nm"

if [ "${APPLY}" -eq 0 ]; then
    info "dry run: nothing written. Run again with --apply to restore."
    exit 0
fi

for _tree in ${TREES}; do
    if [ -e "${MOUNT}/${_tree}.replaced-${STAMP}" ]; then
        die "${MOUNT}/${_tree}.replaced-${STAMP} already exists; move it aside and retry"
    fi
done
if [ "${_free_kb}" -lt $((_need_kb + 4096)) ]; then
    die "not enough free space on the volume (need ${_need_kb} kB plus 4096 kB margin)"
fi

# undo_moves: renames back the directories already moved aside. It runs when a
# rename fails, before any copy starts.
undo_moves() {
    for _tree in ${MOVED}; do
        mv "${MOUNT}/${_tree}.replaced-${STAMP}" "${MOUNT}/${_tree}"
    done
    info "rolled back: original directories restored"
}

# rollback_copies: removes the new copy of every tree and moves each replaced
# directory back. It runs only after every tree has been moved aside.
rollback_copies() {
    for _tree in ${TREES}; do
        rm -rf "${MOUNT:?}/${_tree:?}"
        if [ -e "${MOUNT}/${_tree}.replaced-${STAMP}" ]; then
            mv "${MOUNT}/${_tree}.replaced-${STAMP}" "${MOUNT}/${_tree}"
        fi
    done
    info "rolled back: original directories restored"
}

# Step 1: move each current directory aside. Nothing is deleted.
MOVED=""
for _tree in ${TREES}; do
    if [ -e "${MOUNT}/${_tree}" ]; then
        if ! mv "${MOUNT}/${_tree}" "${MOUNT}/${_tree}.replaced-${STAMP}"; then
            undo_moves
            die "could not rename ${MOUNT}/${_tree}; rolled back"
        fi
        MOVED="${MOVED} ${_tree}"
        info "renamed current ${_tree} to ${MOUNT}/${_tree}.replaced-${STAMP}"
    fi
done

# Step 2: copy each directory from the backup.
for _tree in ${TREES}; do
    if ! cp -R "${BACKUP}/${_tree}" "${MOUNT}/${_tree}"; then
        rollback_copies
        die "copy of ${_tree} failed; rolled back"
    fi
done

# Step 3: check every restored file against the manifest.
for _tree in ${TREES}; do
    if ! check_manifest "${MOUNT}" "${BACKUP}/MANIFEST.sha256" "${_tree}/" >/dev/null; then
        rollback_copies
        die "restored ${_tree} does not match the manifest; rolled back"
    fi
done

sync
for _tree in ${TREES}; do
    info "restored ${_tree} to ${MOUNT}/${_tree}"
done
info "checked ${_files} files against the manifest"
for _tree in ${MOVED}; do
    info "the replaced copy is kept at ${MOUNT}/${_tree}.replaced-${STAMP}"
done
if [ -n "${MOVED}" ]; then
    info "delete the replaced copies by hand after Kaia starts from the restored copy"
fi
info "next: eject the volume cleanly, then reboot the Kobo"
