# shellcheck shell=sh
# Source this file before cargo to link KOReader's LuaJIT:
#   . rust/kaia-lua/env.sh && cargo test -p kaia-lua
#
# mlua-sys reads LUA_LIB and LUA_LIB_NAME at build time, and a build script
# cannot set them for another crate. build.rs checks that they match the
# library it finds, so a stale value fails the build.
#
# Overrides: KAIA_LUAJIT_LIB (directory with libluajit.dylib or libluajit.so)
# and KAIA_LUAJIT_INCLUDE (directory with luajit.h). When unset, the first
# desktop build under base/build/*/ that has the library is used.

# $0 is the shell when this file is sourced, so the repository comes from git.
_kaia_lua_repo=$(git rev-parse --show-toplevel 2>/dev/null) || _kaia_lua_repo=$(pwd)
_kaia_lua_tag=apple-darwin
case "$(uname -s)" in
Linux) _kaia_lua_tag=linux ;;
esac

if [ -z "${KAIA_LUAJIT_LIB:-}" ]; then
    for _kaia_lua_dir in "$_kaia_lua_repo"/base/build/*/; do
        case "$_kaia_lua_dir" in
        *"$_kaia_lua_tag"*) ;;
        *) continue ;;
        esac
        case "$_kaia_lua_dir" in
        *kobo*) continue ;;
        esac
        if [ -f "${_kaia_lua_dir}libs/libluajit.dylib" ] || [ -f "${_kaia_lua_dir}libs/libluajit.so" ]; then
            KAIA_LUAJIT_LIB="${_kaia_lua_dir}libs"
            break
        fi
    done
fi
if [ -z "${KAIA_LUAJIT_INCLUDE:-}" ] && [ -n "${KAIA_LUAJIT_LIB:-}" ]; then
    KAIA_LUAJIT_INCLUDE="$(dirname "$KAIA_LUAJIT_LIB")/staging/include/luajit-2.1"
fi

if [ -n "${KAIA_LUAJIT_LIB:-}" ]; then
    export KAIA_LUAJIT_LIB KAIA_LUAJIT_INCLUDE
    export LUA_LIB="$KAIA_LUAJIT_LIB"
    export LUA_LIB_NAME=luajit
else
    echo "kaia-lua/env.sh: no KOReader LuaJIT found under base/build. Build the emulator or set KAIA_LUAJIT_LIB." >&2
fi
unset _kaia_lua_repo _kaia_lua_tag _kaia_lua_dir
