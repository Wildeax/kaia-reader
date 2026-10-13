#!/usr/bin/env python3
"""Fail when Kaia-owned text uses an em dash or an en dash.

Usage: python3 -I tools/kaia/ci/check_style.py [REPO_ROOT]

The check covers the Kaia docs, Lua code, plugins, tools and Rust sources, and
CLAUDE.md. Upstream KOReader files are outside the check. The two plan files
that came from the project owner are excluded by name. Exit 0 when no dash
is found, 1 when at least one is found, 2 on a usage error.

Standard library only. The dash characters are written as escapes so that
this file contains none.
"""

import sys
from pathlib import Path

EM_DASH = "\u2014"
EN_DASH = "\u2013"
DASHES = {EM_DASH: "em dash (U+2014)", EN_DASH: "en dash (U+2013)"}

# Files under docs/kaia/ that the project owner wrote. They keep their dashes.
EXCLUDED = {
    "docs/kaia/ENGINEERING_PLAN.md",
    "docs/kaia/INITIAL_BACKLOG.md",
}

# Directory names that never hold source text.
SKIP_DIRS = {"target", "__pycache__", ".git"}

BINARY_PROBE_BYTES = 8192


def iter_files(root: Path):
    """Yield every file in the checked scope, relative to root."""
    scopes = [
        ("docs/kaia", "dir"),
        ("frontend/kaia", "dir"),
        ("tools/kaia", "dir"),
        ("rust", "rs"),
    ]
    for rel, kind in scopes:
        base = root / rel
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*")):
            if not path.is_file():
                continue
            parts = path.relative_to(root).parts
            if any(part in SKIP_DIRS for part in parts):
                continue
            if kind == "rs" and path.suffix != ".rs":
                continue
            yield path.relative_to(root).as_posix(), path

    # Kaia plugins: plugins/kaia*.koplugin and any other plugins/kaia* entry.
    plugins = root / "plugins"
    if plugins.is_dir():
        for entry in sorted(plugins.glob("kaia*")):
            if entry.is_file():
                yield entry.relative_to(root).as_posix(), entry
            elif entry.is_dir():
                for path in sorted(entry.rglob("*")):
                    if path.is_file() and not any(p in SKIP_DIRS for p in path.parts):
                        yield path.relative_to(root).as_posix(), path

    claude = root / "CLAUDE.md"
    if claude.is_file():
        yield "CLAUDE.md", claude


def is_binary(data: bytes) -> bool:
    return b"\x00" in data[:BINARY_PROBE_BYTES]


def check(root: Path):
    """Return (checked_count, findings). A finding is (path, line, col, name)."""
    findings = []
    checked = 0
    for rel, path in iter_files(root):
        if rel in EXCLUDED:
            continue
        data = path.read_bytes()
        if is_binary(data):
            continue
        checked += 1
        text = data.decode("utf-8", errors="replace")
        for lineno, line in enumerate(text.splitlines(), start=1):
            for col, ch in enumerate(line, start=1):
                if ch in DASHES:
                    findings.append((rel, lineno, col, DASHES[ch]))
    return checked, findings


def main(argv):
    if len(argv) > 2:
        print("usage: check_style.py [REPO_ROOT]", file=sys.stderr)
        return 2
    root = Path(argv[1] if len(argv) == 2 else Path(__file__).resolve().parents[3])
    if not (root / "CLAUDE.md").is_file():
        print(f"not a Kaia Reader checkout: {root}", file=sys.stderr)
        return 2

    checked, findings = check(root)
    for rel, lineno, col, name in findings:
        print(f"{rel}:{lineno}:{col}: {name}")
    if findings:
        print(f"style check failed: {len(findings)} dash(es) in {checked} files", file=sys.stderr)
        return 1
    print(f"style check passed: {checked} files, no em or en dashes")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
