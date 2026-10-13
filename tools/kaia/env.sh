# shellcheck shell=sh
# Source this file to put the GNU tools KOReader's build expects ahead of the macOS ones.
if [ "$(uname -s)" = "Darwin" ]; then
    B="$(brew --prefix 2>/dev/null || echo /opt/homebrew)"
    export PATH="$HOME/.cargo/bin:$B/opt/findutils/libexec/gnubin:$B/opt/gnu-getopt/bin:$B/opt/make/libexec/gnubin:$B/opt/util-linux/bin:$B/bin:$PATH"
else
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# KOReader's LuaJIT for the kaia-lua crate (mlua reads LUA_LIB at build time).
_kaia_root="$(git rev-parse --show-toplevel 2>/dev/null)"
if [ -n "${_kaia_root}" ] && [ -f "${_kaia_root}/rust/kaia-lua/env.sh" ]; then
    # shellcheck disable=SC1091
    . "${_kaia_root}/rust/kaia-lua/env.sh" 2>/dev/null
fi
unset _kaia_root
