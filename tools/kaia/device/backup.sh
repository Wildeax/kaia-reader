#!/bin/sh
#
# backup.sh: copy Kaia data from a Kobo mounted over USB to this computer.
#
# Usage:
#   sh backup.sh [--include-nickel-db] <mount-path> <backup-parent-dir>
#
# The mount path is the volume the computer shows for the Kobo, for example
# /Volumes/KOBOeReader on macOS. The backup parent must exist and must be
# outside the mount. The script creates kaia-backup-<UTC time>/ inside it.
#
# The backup mirrors device paths. Each copied item keeps its path relative to
# the mount, so restore.sh can copy it back to the same place:
#
#   .adds/kaia/                      Kaia application directory
#   .kaia/                           Kaia data directory: settings, reading state,
#                                    library index, statistics, fonts, dictionaries
#                                    and cache, copied whole (the cache is not skipped)
#   .adds/koreader/<settings>        KOReader settings and user patches, if present
#   .adds/nm/                        NickelMenu configuration
#   <any path>/*.sdr/                book sidecars (outside .kobo and .adds)
#   .kobo/KoboReader.sqlite          only with --include-nickel-db, read-only copy
#
# The script never writes to the mount and never copies book files. Books are
# not part of this backup. Copy the library folders separately if you need
# them in the backup set.
#
# Output: BACKUP_INFO.txt and MANIFEST.sha256 (sha256 of every backed up file,
# relative to the backup directory).

set -eu

# Stop macOS from writing AppleDouble (._*) files onto FAT or exFAT volumes.
export COPYFILE_DISABLE=1

# KOReader settings names that are copied when they exist. Check the list
# against the real device once; see docs/kaia/device/BACKUP_AND_RECOVERY.md.
KOREADER_SETTINGS="settings.reader.lua settings patches defaults.persistent.lua defaults.custom.lua"

usage() {
    cat <<'EOF'
Usage: backup.sh [--include-nickel-db] <mount-path> <backup-parent-dir>

  --include-nickel-db  also copy .kobo/KoboReader.sqlite (and its -journal and
                       -wal files) as read-only files. Use only while the Kobo
                       is in USB mass-storage mode.
EOF
}

die() {
    echo "backup: $*" >&2
    exit 1
}

info() {
    echo "backup: $*"
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

INCLUDE_DB=0
while [ $# -gt 0 ]; do
    case "$1" in
        --include-nickel-db)
            INCLUDE_DB=1
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        -*)
            usage >&2
            die "unknown option: $1"
            ;;
        *)
            break
            ;;
    esac
done

if [ $# -ne 2 ]; then
    usage >&2
    exit 2
fi

MOUNT_IN="$1"
DEST_IN="$2"

[ -d "${MOUNT_IN}" ] || die "mount path is not a directory: ${MOUNT_IN}"
[ -d "${DEST_IN}" ] || die "backup parent is not a directory: ${DEST_IN}"

MOUNT="$(cd "${MOUNT_IN}" && pwd -P)"
DEST_PARENT="$(cd "${DEST_IN}" && pwd -P)"

[ -f "${MOUNT}/.kobo/version" ] || die "not a Kobo volume (no .kobo/version): ${MOUNT}"

case "${DEST_PARENT}" in
    "${MOUNT}" | "${MOUNT}"/*)
        die "backup parent must be outside the Kobo volume"
        ;;
esac

STAMP="$(date -u '+%Y%m%dT%H%M%SZ')"
BK="${DEST_PARENT}/kaia-backup-${STAMP}"
[ ! -e "${BK}" ] || die "backup directory already exists: ${BK}"
mkdir "${BK}"

SDR_LIST="${TMPDIR:-/tmp}/kaia-backup-sdr.$$"
COPIED_LIST="${TMPDIR:-/tmp}/kaia-backup-copied.$$"
trap 'rm -f "${SDR_LIST}" "${COPIED_LIST}"' EXIT INT TERM
: >"${COPIED_LIST}"

# copy_item REL: copy one path relative to the mount, keeping its relative path.
copy_item() {
    _rel="$1"
    _src="${MOUNT}/${_rel}"
    if [ ! -e "${_src}" ]; then
        info "not present, skipped: ${_rel}"
        return 0
    fi
    mkdir -p "${BK}/$(dirname "${_rel}")"
    cp -R "${_src}" "${BK}/${_rel}"
    echo "${_rel}" >>"${COPIED_LIST}"
    info "copied: ${_rel}"
}

copy_item ".adds/kaia"
copy_item ".kaia"
copy_item ".adds/nm"

for _name in ${KOREADER_SETTINGS}; do
    copy_item ".adds/koreader/${_name}"
done

# macOS keeps .Spotlight-V100, .fseventsd and .Trashes on the volume and
# refuses to list the first, which would stop the script under set -e.
find "${MOUNT}" \( -name .kobo -o -name .adds -o -name .kaia \
    -o -name .Spotlight-V100 -o -name .fseventsd -o -name .Trashes \) -prune \
    -o -type d -name '*.sdr' -print >"${SDR_LIST}"
while IFS= read -r _path; do
    copy_item "${_path#"${MOUNT}"/}"
done <"${SDR_LIST}"

if [ "${INCLUDE_DB}" -eq 1 ]; then
    _db="${MOUNT}/.kobo/KoboReader.sqlite"
    [ -f "${_db}" ] || die "no .kobo/KoboReader.sqlite on this volume"
    mkdir -p "${BK}/.kobo"
    _before="$(sha256_file "${_db}")"
    cp "${_db}" "${BK}/.kobo/KoboReader.sqlite"
    _after="$(sha256_file "${_db}")"
    if [ "${_before}" != "${_after}" ]; then
        die "KoboReader.sqlite changed during the copy; discard ${BK}"
    fi
    chmod 444 "${BK}/.kobo/KoboReader.sqlite"
    echo ".kobo/KoboReader.sqlite" >>"${COPIED_LIST}"
    info "copied read-only: .kobo/KoboReader.sqlite"
    for _suffix in -journal -wal; do
        if [ -f "${_db}${_suffix}" ]; then
            cp "${_db}${_suffix}" "${BK}/.kobo/KoboReader.sqlite${_suffix}"
            chmod 444 "${BK}/.kobo/KoboReader.sqlite${_suffix}"
            echo ".kobo/KoboReader.sqlite${_suffix}" >>"${COPIED_LIST}"
            info "copied read-only: .kobo/KoboReader.sqlite${_suffix}"
        fi
    done
fi

if [ ! -d "${BK}/.adds/kaia" ]; then
    info "warning: .adds/kaia was not found on this volume"
fi
if [ ! -d "${BK}/.kaia" ]; then
    info "warning: .kaia was not found on this volume"
fi

# Manifest: sha256 of every file in the backup except the manifest and info files.
(
    cd "${BK}"
    find . -type f ! -name MANIFEST.sha256 ! -name BACKUP_INFO.txt |
        sort |
        while IFS= read -r _f; do
            printf '%s  %s\n' "$(sha256_file "${_f}")" "${_f#./}"
        done
) >"${BK}/MANIFEST.sha256"

{
    echo "format=kaia-backup-1"
    echo "created_utc=${STAMP}"
    echo "mount_name=$(basename "${MOUNT}")"
    echo "include_nickel_db=$( [ "${INCLUDE_DB}" -eq 1 ] && echo yes || echo no )"
    echo "copied:"
    sed -e 's/^/  /' "${COPIED_LIST}"
} >"${BK}/BACKUP_INFO.txt"

# Check the manifest against the files just written.
_count=0
while IFS=' ' read -r _hash _path; do
    _got="$(sha256_file "${BK}/${_path}")"
    if [ "${_got}" != "${_hash}" ]; then
        die "manifest check failed for ${_path}; discard ${BK}"
    fi
    _count=$((_count + 1))
done <"${BK}/MANIFEST.sha256"

info "backup complete: ${_count} files in ${BK}"
info "the Kobo volume was only read"
