#!/bin/sh
#
# test-backup-restore.sh: check backup.sh and restore.sh against a fake Kobo volume.
#
# The fake volume holds books, a loose book, .kobo, KOReader and NickelMenu
# files, the Kaia application directory (.adds/kaia) and the Kaia data
# directory (.kaia). The checks cover:
#   - backup copies .kaia with the same hashes and leaves the volume unchanged
#   - a dry run writes nothing
#   - restore brings .kaia and .adds/kaia back by hash, and leaves books, .kobo,
#     KOReader and NickelMenu files byte-identical
#   - a copy that fails midway, and a copy that fails the manifest check, roll
#     both directories back
#   - a backup without .kaia leaves the current .kaia in place
#
# Runs on macOS or Linux with POSIX tools only. Fault injection replaces cp for
# the restore runs only. Exit 0 when every check passes.

set -u

HERE="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)"
BACKUP_SH="${HERE}/backup.sh"
RESTORE_SH="${HERE}/restore.sh"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/kaia-backup-restore-test.XXXXXX")" || exit 1
trap 'rm -rf "${WORK}"' EXIT INT TERM

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

if command -v sha256sum >/dev/null 2>&1; then
    sha256_file() { sha256sum "$1" | cut -d' ' -f1; }
else
    sha256_file() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi

# snapshot ROOT PATH: one "hash  path" line per file under ROOT/PATH, sorted.
# Prints nothing when ROOT/PATH does not exist.
snapshot() {
    [ -e "$1/$2" ] || return 0
    (
        cd "$1" &&
            find "$2" -type f | LC_ALL=C sort | while IFS= read -r _f; do
                printf '%s  %s\n' "$(sha256_file "${_f}")" "${_f}"
            done
    )
}

# protected ROOT: hashes of everything restore must never change.
protected() {
    snapshot "$1" Books
    snapshot "$1" "Loose note.epub"
    snapshot "$1" .kobo
    snapshot "$1" .adds/koreader
    snapshot "$1" .adds/nm
}

# replaced_list ROOT: the .kaia.replaced-* and .adds/kaia.replaced-* entries.
replaced_list() {
    (cd "$1" && find . -maxdepth 2 \( -name '.kaia.replaced-*' -o -name 'kaia.replaced-*' \) | LC_ALL=C sort)
}

# first_match DIR PATTERN: the first entry in DIR that matches PATTERN.
first_match() {
    for _m in "$1"/$2; do
        if [ -e "${_m}" ]; then
            echo "${_m}"
            return 0
        fi
    done
    return 1
}

# only_backup PARENT: the kaia-backup-* directory inside PARENT.
only_backup() {
    for _d in "$1"/kaia-backup-*; do
        if [ -d "${_d}" ]; then
            echo "${_d}"
            return 0
        fi
    done
    return 1
}

same_as() {
    if cmp -s "$2" "$3"; then
        ok "$1"
    else
        bad "$1"
        diff "$2" "$3" | head -n 20
    fi
}

expect_rc() {
    if [ "$2" -eq "$3" ]; then
        ok "$1"
    else
        bad "$1 (exit $3, wanted $2)"
    fi
}

expect_failed() {
    if [ "$2" -ne 0 ]; then
        ok "$1"
    else
        bad "$1 (exit 0, wanted a failure)"
    fi
}

expect_text() {
    if grep -q -E -- "$2" "$1"; then
        ok "$3"
    else
        bad "$3 (no line matches $2 in $1)"
    fi
}

expect_line() {
    if grep -q -x -F -- "$2" "$1"; then
        ok "$3"
    else
        bad "$3 (wanted line: $2)"
    fi
}

expect_file() {
    if [ -f "$1" ]; then
        ok "$2"
    else
        bad "$2"
    fi
}

expect_absent() {
    if [ -e "$1" ]; then
        bad "$2"
    else
        ok "$2"
    fi
}

# make_volume ROOT: the fake Kobo volume with sample files in every area.
make_volume() {
    _v="$1"
    mkdir -p "${_v}/.kobo" \
        "${_v}/.adds/kaia/plugins/kaia-core" \
        "${_v}/.adds/koreader/patches" \
        "${_v}/.adds/nm" \
        "${_v}/Books/Fiction/Long Read.sdr" \
        "${_v}/.kaia/kaia/books" \
        "${_v}/.kaia/fonts" \
        "${_v}/.kaia/dict" \
        "${_v}/.kaia/cache/covers"
    printf 'Nickel fixture 4.38\n' >"${_v}/.kobo/version"
    printf 'nickel database fixture\n' >"${_v}/.kobo/KoboReader.sqlite"
    printf '#!/bin/sh\necho kaia\n' >"${_v}/.adds/kaia/kaia.sh"
    printf 'return {}\n' >"${_v}/.adds/kaia/plugins/kaia-core/main.lua"
    printf 'return { reader = true }\n' >"${_v}/.adds/koreader/settings.reader.lua"
    printf 'return {}\n' >"${_v}/.adds/koreader/patches/2-title.lua"
    printf 'nm fixture\n' >"${_v}/.adds/nm/config"
    printf 'EPUB fixture bytes\000\001\002\n' >"${_v}/Books/Fiction/Long Read.epub"
    printf 'EPUB fixture loose\n' >"${_v}/Loose note.epub"
    printf 'return { highlights = {} }\n' >"${_v}/Books/Fiction/Long Read.sdr/metadata.epub.lua"
    printf '{"format":"kaia-settings","version":1,"theme":"light"}\n' >"${_v}/.kaia/settings.json"
    printf '{"format":"kaia-book","version":1,"page":12}\n' >"${_v}/.kaia/kaia/books/0123abcd.json"
    printf 'SQLite fixture kaia\000\001\002\n' >"${_v}/.kaia/kaia/kaia.sqlite3"
    printf 'SQLite fixture library\000\001\n' >"${_v}/.kaia/library.sqlite3"
    printf 'SQLite fixture statistics\000\n' >"${_v}/.kaia/statistics.sqlite3"
    printf 'font fixture\n' >"${_v}/.kaia/fonts/Serif.ttf"
    printf 'dictionary fixture\n' >"${_v}/.kaia/dict/en.db"
    printf 'cover fixture\n' >"${_v}/.kaia/cache/covers/0123.jpg"
}

# Test double for cp, used only while restore.sh runs. It runs the real cp,
# then fails or corrupts the copy when the destination ends in FAULT_SUFFIX.
mkdir -p "${WORK}/bin"
REAL_CP="$(command -v cp)"
cat >"${WORK}/bin/cp" <<EOF
#!/bin/sh
"${REAL_CP}" "\$@" || exit \$?
for _last; do :; done
case "\${_last}" in
    *"\${FAULT_SUFFIX}") ;;
    *) exit 0 ;;
esac
case "\${FAULT_MODE}" in
    fail)
        echo "fake cp: injected failure after copying \${_last}" >&2
        exit 1
        ;;
    corrupt)
        _victim="\$(find "\${_last}" -type f | LC_ALL=C sort | head -n 1)"
        printf 'corrupt' >>"\${_victim}"
        exit 0
        ;;
esac
exit 0
EOF
chmod +x "${WORK}/bin/cp"

# run_faulty MODE SUFFIX ARGS...: restore.sh with the fault-injecting cp first on PATH.
run_faulty() {
    _mode="$1"
    _suffix="$2"
    shift 2
    (
        PATH="${WORK}/bin:${PATH}"
        FAULT_MODE="${_mode}"
        FAULT_SUFFIX="${_suffix}"
        export PATH FAULT_MODE FAULT_SUFFIX
        sh "${RESTORE_SH}" "$@"
    )
}

VOL="${WORK}/volume"
make_volume "${VOL}"
snapshot "${VOL}" . >"${WORK}/vol-orig.txt"
snapshot "${VOL}" .kaia >"${WORK}/kaia-orig.txt"
snapshot "${VOL}" .adds/kaia >"${WORK}/adds-kaia-orig.txt"
protected "${VOL}" >"${WORK}/protected-orig.txt"
if [ -s "${WORK}/protected-orig.txt" ] && [ -s "${WORK}/kaia-orig.txt" ]; then
    ok "fake volume has protected files and a .kaia tree"
else
    bad "fake volume has protected files and a .kaia tree"
fi

echo "== backup"
BKP1="${WORK}/backups-1"
mkdir -p "${BKP1}"
RC=0
sh "${BACKUP_SH}" "${VOL}" "${BKP1}" >"${WORK}/backup.log" 2>&1 || RC=$?
expect_rc "backup exits 0" 0 "${RC}"
BK1="$(only_backup "${BKP1}")" || BK1=""
if [ -z "${BK1}" ]; then
    bad "backup directory was created"
    echo "passed ${PASS}, failed ${FAIL}"
    exit 1
fi
expect_text "${WORK}/backup.log" 'copied: \.kaia$' "backup log reports .kaia copied"
expect_line "${BK1}/BACKUP_INFO.txt" "  .kaia" "BACKUP_INFO.txt lists .kaia"
expect_text "${BK1}/MANIFEST.sha256" '  \.kaia/settings\.json$' "manifest has .kaia/settings.json"
expect_text "${BK1}/MANIFEST.sha256" '  \.kaia/cache/covers/0123\.jpg$' "manifest has the .kaia cache (cache is not skipped)"
snapshot "${BK1}" .kaia >"${WORK}/kaia-in-backup.txt"
same_as "backup holds .kaia with the same hashes as the volume" "${WORK}/kaia-orig.txt" "${WORK}/kaia-in-backup.txt"
expect_absent "${BK1}/Books/Fiction/Long Read.epub" "backup does not copy book files"
expect_absent "${BK1}/Loose note.epub" "backup does not copy the loose book"
expect_absent "${BK1}/.kobo" "backup does not copy .kobo without --include-nickel-db"
expect_file "${BK1}/Books/Fiction/Long Read.sdr/metadata.epub.lua" "backup keeps book sidecars"
snapshot "${VOL}" . >"${WORK}/vol-after-backup.txt"
same_as "backup leaves the volume unchanged" "${WORK}/vol-orig.txt" "${WORK}/vol-after-backup.txt"

echo "== change .kaia and .adds/kaia after the backup"
printf '{"format":"kaia-settings","version":1,"theme":"dark"}\n' >"${VOL}/.kaia/settings.json"
printf 'changed after backup\n' >>"${VOL}/.kaia/kaia/books/0123abcd.json"
printf '{"format":"kaia-book","version":1,"page":99}\n' >"${VOL}/.kaia/kaia/books/ffff0000.json"
rm -f "${VOL}/.kaia/fonts/Serif.ttf"
rm -rf "${VOL}/.kaia/cache"
printf 'SQLite changed\000\n' >"${VOL}/.kaia/library.sqlite3"
printf 'app changed\n' >"${VOL}/.adds/kaia/kaia.sh"
rm -f "${VOL}/.adds/kaia/plugins/kaia-core/main.lua"
printf 'sidecar edited after backup\n' >>"${VOL}/Books/Fiction/Long Read.sdr/metadata.epub.lua"
snapshot "${VOL}" .kaia >"${WORK}/kaia-changed.txt"
snapshot "${VOL}" .adds/kaia >"${WORK}/adds-kaia-changed.txt"
snapshot "${VOL}" . >"${WORK}/vol-changed.txt"
protected "${VOL}" >"${WORK}/protected-changed.txt"
if cmp -s "${WORK}/kaia-orig.txt" "${WORK}/kaia-changed.txt"; then
    bad "test edits changed .kaia"
else
    ok "test edits changed .kaia"
fi

echo "== dry run"
RC=0
sh "${RESTORE_SH}" "${BK1}" "${VOL}" >"${WORK}/dry.log" 2>&1 || RC=$?
expect_rc "dry run exits 0" 0 "${RC}"
expect_text "${WORK}/dry.log" 'target: +[^ ]*/\.kaia$' "dry run plan names the .kaia target"
expect_text "${WORK}/dry.log" 'dry run: nothing written' "dry run says it wrote nothing"
snapshot "${VOL}" . >"${WORK}/vol-after-dry.txt"
same_as "dry run leaves the volume unchanged" "${WORK}/vol-changed.txt" "${WORK}/vol-after-dry.txt"

echo "== restore --apply"
sleep 1
RC=0
sh "${RESTORE_SH}" "${BK1}" "${VOL}" --apply >"${WORK}/apply.log" 2>&1 || RC=$?
expect_rc "restore --apply exits 0" 0 "${RC}"
snapshot "${VOL}" .kaia >"${WORK}/kaia-restored.txt"
same_as ".kaia matches the original by hash after restore" "${WORK}/kaia-orig.txt" "${WORK}/kaia-restored.txt"
snapshot "${VOL}" .adds/kaia >"${WORK}/adds-kaia-restored.txt"
same_as ".adds/kaia matches the original by hash after restore" "${WORK}/adds-kaia-orig.txt" "${WORK}/adds-kaia-restored.txt"
protected "${VOL}" >"${WORK}/protected-after.txt"
same_as "books, sidecars, .kobo, KOReader and NickelMenu files are byte-identical" "${WORK}/protected-changed.txt" "${WORK}/protected-after.txt"
expect_absent "${VOL}/.kaia/kaia/books/ffff0000.json" "a state file added after the backup is gone from .kaia"
REPL_KAIA="$(first_match "${VOL}" '.kaia.replaced-*')" || REPL_KAIA=""
REPL_ADDS="$(first_match "${VOL}/.adds" 'kaia.replaced-*')" || REPL_ADDS=""
if [ -n "${REPL_KAIA}" ] && [ -n "${REPL_ADDS}" ]; then
    ok "replaced copies of .kaia and .adds/kaia are kept"
    # Rewrite the replaced directory name to the original path before comparing.
    _name="$(basename "${REPL_KAIA}")"
    snapshot "${VOL}" "${_name}" | sed "s|  ${_name}/|  .kaia/|" >"${WORK}/kaia-replaced.txt"
    same_as "replaced .kaia keeps the edited files" "${WORK}/kaia-changed.txt" "${WORK}/kaia-replaced.txt"
    _name="$(basename "${REPL_ADDS}")"
    snapshot "${VOL}" ".adds/${_name}" | sed "s|  .adds/${_name}/|  .adds/kaia/|" >"${WORK}/adds-replaced.txt"
    same_as "replaced .adds/kaia keeps the edited files" "${WORK}/adds-kaia-changed.txt" "${WORK}/adds-replaced.txt"
else
    bad "replaced copies of .kaia and .adds/kaia are kept"
fi
expect_text "${WORK}/apply.log" 'restored \.kaia to ' "apply log reports .kaia restored"

echo "== rollback"
printf 'mid-session edit\n' >>"${VOL}/.kaia/settings.json"
printf 'mid-session app edit\n' >>"${VOL}/.adds/kaia/kaia.sh"
snapshot "${VOL}" .kaia >"${WORK}/pre-kaia.txt"
snapshot "${VOL}" .adds/kaia >"${WORK}/pre-adds.txt"
protected "${VOL}" >"${WORK}/pre-protected.txt"
replaced_list "${VOL}" >"${WORK}/pre-replaced.txt"

# check_rollback DESCRIPTION LOG: the state after a failed restore.
check_rollback() {
    snapshot "${VOL}" .kaia >"${WORK}/post-kaia.txt"
    same_as "$1: .kaia is intact" "${WORK}/pre-kaia.txt" "${WORK}/post-kaia.txt"
    snapshot "${VOL}" .adds/kaia >"${WORK}/post-adds.txt"
    same_as "$1: .adds/kaia is rolled back too" "${WORK}/pre-adds.txt" "${WORK}/post-adds.txt"
    protected "${VOL}" >"${WORK}/post-protected.txt"
    same_as "$1: books, .kobo and KOReader files are unchanged" "${WORK}/pre-protected.txt" "${WORK}/post-protected.txt"
    replaced_list "${VOL}" >"${WORK}/post-replaced.txt"
    same_as "$1: no replaced copy is left behind" "${WORK}/pre-replaced.txt" "${WORK}/post-replaced.txt"
    expect_text "$2" 'rolled back' "$1: log says rolled back"
}

sleep 1
RC=0
run_faulty fail /.kaia "${BK1}" "${VOL}" --apply >"${WORK}/fail-kaia.log" 2>&1 || RC=$?
expect_failed "copy of .kaia fails midway" "${RC}"
expect_text "${WORK}/fail-kaia.log" 'fake cp: injected failure' "fault was injected into the .kaia copy"
check_rollback "copy of .kaia fails midway" "${WORK}/fail-kaia.log"

sleep 1
RC=0
run_faulty fail /.adds/kaia "${BK1}" "${VOL}" --apply >"${WORK}/fail-adds.log" 2>&1 || RC=$?
expect_failed "copy of .adds/kaia fails midway" "${RC}"
expect_text "${WORK}/fail-adds.log" 'fake cp: injected failure' "fault was injected into the .adds/kaia copy"
check_rollback "copy of .adds/kaia fails midway" "${WORK}/fail-adds.log"

sleep 1
RC=0
run_faulty corrupt /.kaia "${BK1}" "${VOL}" --apply >"${WORK}/corrupt-kaia.log" 2>&1 || RC=$?
expect_failed "restored .kaia fails the manifest check" "${RC}"
expect_text "${WORK}/corrupt-kaia.log" 'does not match the manifest' "log reports the manifest mismatch"
check_rollback "restored .kaia fails the manifest check" "${WORK}/corrupt-kaia.log"

echo "== restore after rollback"
sleep 1
RC=0
sh "${RESTORE_SH}" "${BK1}" "${VOL}" --apply >"${WORK}/retry.log" 2>&1 || RC=$?
expect_rc "restore succeeds after the rolled-back attempts" 0 "${RC}"
snapshot "${VOL}" .kaia >"${WORK}/kaia-retry.txt"
same_as ".kaia matches the original by hash after the retry" "${WORK}/kaia-orig.txt" "${WORK}/kaia-retry.txt"

echo "== backup without .kaia"
VOL_OLD="${WORK}/volume-old"
make_volume "${VOL_OLD}"
rm -rf "${VOL_OLD}/.kaia"
BKP_OLD="${WORK}/backups-old"
mkdir -p "${BKP_OLD}"
RC=0
sh "${BACKUP_SH}" "${VOL_OLD}" "${BKP_OLD}" >"${WORK}/backup-old.log" 2>&1 || RC=$?
expect_rc "backup of a volume without .kaia exits 0" 0 "${RC}"
BK_OLD="$(only_backup "${BKP_OLD}")" || BK_OLD=""
if [ -n "${BK_OLD}" ]; then
    expect_absent "${BK_OLD}/.kaia" "backup of a volume without .kaia has no .kaia"
    replaced_list "${VOL}" >"${WORK}/pre-old-replaced.txt"
    snapshot "${VOL}" .kaia >"${WORK}/pre-old-kaia.txt"
    sleep 1
    RC=0
    sh "${RESTORE_SH}" "${BK_OLD}" "${VOL}" --apply >"${WORK}/old-apply.log" 2>&1 || RC=$?
    expect_rc "restore from a backup without .kaia exits 0" 0 "${RC}"
    expect_text "${WORK}/old-apply.log" 'backup has no \.kaia' "restore says the current .kaia is kept"
    snapshot "${VOL}" .kaia >"${WORK}/post-old-kaia.txt"
    same_as "current .kaia is untouched when the backup has none" "${WORK}/pre-old-kaia.txt" "${WORK}/post-old-kaia.txt"
    replaced_list "${VOL}" >"${WORK}/post-old-replaced.txt"
    grep '^\./\.kaia\.replaced' "${WORK}/pre-old-replaced.txt" >"${WORK}/pre-old-kaia-replaced.txt" || true
    grep '^\./\.kaia\.replaced' "${WORK}/post-old-replaced.txt" >"${WORK}/post-old-kaia-replaced.txt" || true
    same_as "no .kaia.replaced copy is made when the backup has no .kaia" \
        "${WORK}/pre-old-kaia-replaced.txt" "${WORK}/post-old-kaia-replaced.txt"
else
    bad "backup of a volume without .kaia was created"
fi

echo
echo "passed ${PASS}, failed ${FAIL}"
if [ "${FAIL}" -eq 0 ]; then
    exit 0
fi
exit 1
