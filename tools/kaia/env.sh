# Source this file to put the GNU tools KOReader's build expects ahead of the macOS ones.
if [ "$(uname -s)" = "Darwin" ]; then
    B="$(brew --prefix 2>/dev/null || echo /opt/homebrew)"
    export PATH="$HOME/.cargo/bin:$B/opt/findutils/libexec/gnubin:$B/opt/gnu-getopt/bin:$B/opt/make/libexec/gnubin:$B/opt/util-linux/bin:$B/bin:$PATH"
else
    export PATH="$HOME/.cargo/bin:$PATH"
fi
