#!/bin/sh
# Tests the safe mode logic in kaia-rust.sh. The real launcher runs against a fake
# kaia binary in a scratch folder. The fake records its arguments and the failed
# start count it sees, then exits with the code in FAKE_EXIT.
#
# Run: sh platform/kobo/kaia/test-kaia-rust.sh

HERE="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)"
LAUNCHER="${HERE}/kaia-rust.sh"
WORK="$(mktemp -d)" || exit 1
WORK="$(CDPATH='' cd -- "${WORK}" && pwd -P)"
trap 'rm -rf "${WORK}"' 0

PASSES=0
FAILS=0
pass() {
    PASSES=$((PASSES + 1))
    echo "ok: $1"
}
fail() {
    FAILS=$((FAILS + 1))
    echo "FAIL: $1"
}

INSTALL="${WORK}/install"
mkdir -p "${INSTALL}"
cat >"${INSTALL}/kaia" <<'EOF'
#!/bin/sh
# Fake Rust app. The launcher passes --data-dir DIR first.
count=none
if [ -f "$2/kaia-startup-attempts" ]; then
    count="$(cat "$2/kaia-startup-attempts")"
fi
echo "args=$* count=${count}" >>"${FAKE_LOG}"
exit "${FAKE_EXIT:-0}"
EOF
printf '#!/bin/sh\nexit 0\n' >"${INSTALL}/fbink"
printf '#!/bin/sh\nexit 0\n' >"${INSTALL}/fbdepth"
chmod 755 "${INSTALL}/kaia" "${INSTALL}/fbink" "${INSTALL}/fbdepth"

CASE_DIR=""
DATA=""
RUN_TMP=""
LAST_STATUS=""

# new_case NAME: a fresh data folder and scratch folder for one scenario.
new_case() {
    CASE_DIR="${WORK}/$1"
    DATA="${CASE_DIR}/data"
    RUN_TMP="${CASE_DIR}/tmp"
    mkdir -p "${DATA}" "${RUN_TMP}"
    RUN_TMP="$(CDPATH='' cd -- "${RUN_TMP}" && pwd -P)"
    : >"${CASE_DIR}/app.log"
}

# run_app EXIT_CODE: one launch of the launcher. The fake app exits with EXIT_CODE.
run_app() {
    FAKE_EXIT="$1" FAKE_LOG="${CASE_DIR}/app.log" \
        KOREADER_DIR="${INSTALL}" KAIA_DATA_DIR="${DATA}" KAIA_RUST_TMP="${RUN_TMP}" \
        sh "${LAUNCHER}" >"${CASE_DIR}/launcher.out" 2>&1
    LAST_STATUS=$?
}

# expect_line LABEL PATTERN FILE: some line of FILE matches PATTERN.
expect_line() {
    if grep -q -- "$2" "$3"; then pass "$1"; else fail "$1 (no line matches '$2' in $3)"; fi
}

# expect_no_line LABEL PATTERN FILE: no line of FILE matches PATTERN.
expect_no_line() {
    if grep -q -- "$2" "$3"; then fail "$1 (a line matches '$2' in $3)"; else pass "$1"; fi
}

# expect_file_value LABEL FILE VALUE: FILE holds VALUE on its first line.
expect_file_value() {
    if [ -f "$2" ] && [ "$(head -n 1 "$2")" = "$3" ]; then pass "$1"; else fail "$1 (wanted $3 in $2)"; fi
}

# expect_absent LABEL PATH: PATH does not exist.
expect_absent() {
    if [ -e "$2" ]; then fail "$1 ($2 still exists)"; else pass "$1"; fi
}

# expect_present LABEL PATH: PATH exists.
expect_present() {
    if [ -e "$2" ]; then pass "$1"; else fail "$1 ($2 is missing)"; fi
}

if sh -n "${LAUNCHER}"; then pass "launcher parses with sh -n"; else fail "launcher parses with sh -n"; fi

# Marker present: the launch uses safe mode, and a clean exit keeps the marker.
new_case marker
: >"${DATA}/safemode"
run_app 0
expect_line "marker: launch passes --safe-mode" "--safe-mode count=1\$" "${CASE_DIR}/app.log"
if [ "${LAST_STATUS}" = 0 ]; then pass "marker: launcher exits 0 after a clean run"; else fail "marker: launcher status ${LAST_STATUS}"; fi
expect_present "marker: marker file is kept" "${DATA}/safemode"
expect_absent "marker: clean exit clears the failed start count" "${DATA}/kaia-startup-attempts"

# Marker present and the app crashes: the count still goes up, and the marker still wins.
new_case marker-crash
: >"${DATA}/safemode"
run_app 1
expect_line "marker crash: launch passes --safe-mode" "--safe-mode count=1\$" "${CASE_DIR}/app.log"
if [ "${LAST_STATUS}" = 1 ]; then pass "marker crash: launcher passes the app status"; else fail "marker crash: launcher status ${LAST_STATUS}"; fi
expect_file_value "marker crash: count is kept after a crash" "${DATA}/kaia-startup-attempts" 1

# Count at the limit (three failed starts): the next launch is safe mode.
new_case at-limit
printf '3\n' >"${DATA}/kaia-startup-attempts"
run_app 1
expect_line "at limit: launch passes --safe-mode" "--safe-mode count=4\$" "${CASE_DIR}/app.log"
if [ "${LAST_STATUS}" = 1 ]; then pass "at limit: launcher passes the app status"; else fail "at limit: launcher status ${LAST_STATUS}"; fi
expect_file_value "at limit: count is kept after a crash" "${DATA}/kaia-startup-attempts" 4

# Count at the limit, then a clean exit: the count is cleared.
new_case at-limit-clean
printf '3\n' >"${DATA}/kaia-startup-attempts"
run_app 0
expect_line "at limit clean: launch passes --safe-mode" "--safe-mode count=4\$" "${CASE_DIR}/app.log"
expect_absent "at limit clean: clean exit clears the count" "${DATA}/kaia-startup-attempts"

# Count below the limit: a normal launch.
new_case below-limit
printf '2\n' >"${DATA}/kaia-startup-attempts"
run_app 1
expect_line "below limit: launch is normal" "count=3\$" "${CASE_DIR}/app.log"
expect_no_line "below limit: no --safe-mode" "--safe-mode" "${CASE_DIR}/app.log"
expect_file_value "below limit: count goes up by one" "${DATA}/kaia-startup-attempts" 3

# Repeated crashes from no count: the fourth launch is the first safe mode launch.
new_case repeated
i=1
while [ "${i}" -le 4 ]; do
    run_app 1
    i=$((i + 1))
done
expect_line "repeated: first launch is normal" "count=1\$" "${CASE_DIR}/app.log"
expect_no_line "repeated: launches 1 to 3 are normal" "args=.*--safe-mode count=[123]\$" "${CASE_DIR}/app.log"
expect_line "repeated: launch 4 is safe mode" "--safe-mode count=4\$" "${CASE_DIR}/app.log"
expect_file_value "repeated: count is 4 after four crashes" "${DATA}/kaia-startup-attempts" 4

# A count that is not a number counts as zero.
new_case garbage
printf 'not a number\n' >"${DATA}/kaia-startup-attempts"
run_app 1
expect_line "garbage count: launch is normal with count 1" "count=1\$" "${CASE_DIR}/app.log"
expect_no_line "garbage count: no --safe-mode" "--safe-mode" "${CASE_DIR}/app.log"

# No marker and no count: a normal launch, and a clean exit leaves no files behind.
new_case plain
run_app 0
expect_no_line "plain: no --safe-mode" "--safe-mode" "${CASE_DIR}/app.log"
expect_absent "plain: no marker is created" "${DATA}/safemode"
expect_absent "plain: the scratch lock is released" "${RUN_TMP}/kaia-rust.lock"

echo
echo "${PASSES} passed, ${FAILS} failed"
[ "${FAILS}" -eq 0 ]
