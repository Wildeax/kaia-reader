#!/usr/bin/env python3
"""Regenerate the "Rust crates" section of docs/kaia/inventory/LICENSES.md.

Usage, from the repository root:

    python3 -I tools/kaia/inventory/rust_licenses.py --root .

The script runs cargo in rust/, always with --locked, so Cargo.lock does not
change:

    cargo metadata --format-version 1 --locked
    cargo tree --locked -e normal --target all -p <package> [feature flags]
    cargo tree --locked -e normal --target <release triple> -p <package> [feature flags]

The product builds are kaia with default features, kaia with
--no-default-features --features kobo, and kaia-ffi with default features and
with --features glibc215 (the Kobo build of libkaia.so). Each crate version in
the --target all trees gets one row. Workspace crates get no row. The release
triples (macOS, Linux and Kobo) decide whether a row says that no release build
compiles the crate.

The allow list in rust/deny.toml is read at run time, so the notes and the
policy cannot drift apart. The script rewrites only the "## Rust crates"
section. When the section is missing, the script inserts it after
"## Native dependencies". The rest of LICENSES.md stays as it is.

Standard library only. Exit 0 on success, 1 when cargo fails or the input is
not what the script expects, 2 on a usage error.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

SECTION_TITLE = "## Rust crates"
INSERT_AFTER = "## Native dependencies"
LICENSES_MD = Path("docs/kaia/inventory/LICENSES.md")
DENY_TOML = Path("rust/deny.toml")
RUST_DIR = Path("rust")

# Product builds, each with the cargo tree arguments that select it. The two
# libkaia.so builds share one label, and their rows are merged.
BUILDS = [
    ("kaia (desktop)", ["-p", "kaia-app"]),
    ("kaia (kobo)", ["-p", "kaia-app", "--no-default-features", "--features", "kobo"]),
    ("libkaia.so", ["-p", "kaia-ffi"]),
    ("libkaia.so", ["-p", "kaia-ffi", "--features", "glibc215"]),
]
LABELS = ["kaia (desktop)", "kaia (kobo)", "libkaia.so"]

# The release targets: the macOS and Linux emulators, and the Kobo armv7 device.
SHIPPED_TARGETS = [
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "armv7-unknown-linux-gnueabihf",
]

# Notes for crates whose bundled code has a different license, or a build detail
# that the table cannot show. Keyed by crate name.
CURATED_NOTES = {
    "libsqlite3-sys": "Compiles the bundled SQLite source. The crate README says SQLite is in the public domain.",
    "ring": "Compiles BoringSSL-derived C and assembly. The LICENSE, LICENSE-BoringSSL and LICENSE-other-bits files cover it.",
    "sdl2": "The graph reaches it only through orbclient, as for sdl2-sys. No target builds it.",
    "sdl2-sys": (
        "The graph reaches it only through orbclient. minifb uses orbclient only on Redox, "
        "and orbclient uses sdl2 only off Redox, so no target builds it. "
        "The package bundles SDL2 source under SDL/ (zlib), which is not compiled."
    ),
}
NATIVE_NOTE = "Its build script compiles native code that links into the product."
PROC_MACRO_NOTE = "Proc macro. The compiler runs it, and it does not link into the product."

LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|UNLICENSE)", re.IGNORECASE)
NOTICE_FILE = re.compile(r"^NOTICE", re.IGNORECASE)
TREE_LINE = re.compile(r"^(\S+) v(\S+)")
ALLOW_ITEM = re.compile(r'^\s*"([^"]+)"')


def fail(message):
    print(f"rust_licenses: {message}", file=sys.stderr)
    sys.exit(1)


def run_cargo(rust_dir, args):
    try:
        proc = subprocess.run(
            ["cargo", *args], cwd=rust_dir, capture_output=True, text=True, check=False
        )
    except OSError as exc:
        fail(f"cannot run cargo ({exc}). Source tools/kaia/env.sh first.")
    if proc.returncode != 0:
        fail(f"cargo {args[0]} failed:\n{proc.stderr.strip()}")
    return proc.stdout


def load_metadata(rust_dir):
    return json.loads(run_cargo(rust_dir, ["metadata", "--format-version", "1", "--locked"]))


def tree_members(rust_dir, extra_args, target="all"):
    out = run_cargo(
        rust_dir,
        ["tree", "--locked", "-e", "normal", "--target", target, "--prefix", "none",
         "--format", "{p}", *extra_args],
    )
    members = set()
    for line in out.splitlines():
        match = TREE_LINE.match(line)
        if match:
            members.add((match.group(1), match.group(2)))
    return members


def read_allow_list(path):
    allowed = set()
    in_list = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if not in_list:
            in_list = re.match(r"^\s*allow\s*=\s*\[", line) is not None
            continue
        if line.strip().startswith("]"):
            break
        match = ALLOW_ITEM.match(line)
        if match:
            allowed.add(match.group(1))
    if not allowed:
        fail(f"no allow list found in {path}")
    return allowed


def version_key(version):
    return tuple((0, int(part)) if part.isdigit() else (1, part)
                 for part in re.split(r"[.+-]", version))


def license_terms(expression):
    flat = re.sub(r"[()]", " ", expression)
    return [term.strip() for term in re.split(r"\s+(?:OR|AND)\s+|/", flat) if term.strip()]


def is_choice(expression):
    return re.search(r"\s+OR\s+|/", expression) is not None


def license_note(expression, allowed):
    if expression is None:
        return "No license field in Cargo.toml. Read the license files before release."
    terms = list(dict.fromkeys(license_terms(expression)))
    bad = [term for term in terms if term not in allowed]
    if not bad:
        return ""
    good = [term for term in terms if term in allowed]
    offered = ", ".join(bad)
    if not is_choice(expression):
        return (f"The expression needs {offered}, which rust/deny.toml does not allow. "
                "Review before release.")
    if good:
        return (f"The expression offers {offered}, which rust/deny.toml does not allow. "
                f"Use {' or '.join(good)}.")
    return (f"The expression offers {offered}, which rust/deny.toml does not allow, "
            "and no other choice is allowed. Review before release.")


def text_note(has_license_file, repository):
    if has_license_file:
        return ""
    if repository:
        return (f"No license file in the crate package. "
                f"Copy the text from {repository} before release.")
    return ("No license file and no repository field in the crate package. "
            "Find the upstream license text before release.")


def notice_note(notice_files):
    if not notice_files:
        return ""
    return f"Has a NOTICE file ({', '.join(notice_files)}). Ship it with the binary."


def native_note(name, pkg_dir, built):
    if name in CURATED_NOTES:
        return CURATED_NOTES[name]
    if not built:
        return ""
    build_rs = pkg_dir / "build.rs"
    if build_rs.is_file():
        text = build_rs.read_text(encoding="utf-8", errors="replace")
        if "cc::Build" in text or "cmake::Config" in text:
            return NATIVE_NOTE
    return ""


def build_rows(meta, rust_dir, allowed):
    workspace_ids = set(meta["workspace_members"])
    workspace_nv = {(p["name"], p["version"]) for p in meta["packages"]
                    if p["id"] in workspace_ids}
    packages = {}
    for pkg in meta["packages"]:
        if pkg["id"] not in workspace_ids:
            packages.setdefault((pkg["name"], pkg["version"]), pkg)

    used = {}
    for label, extra_args in BUILDS:
        for nv in tree_members(rust_dir, extra_args):
            used.setdefault(nv, set()).add(label)
    unknown = sorted(nv for nv in used if nv not in packages and nv not in workspace_nv)
    if unknown:
        fail(f"cargo tree lists packages that cargo metadata does not know: {unknown[:5]}")

    built = set()
    for target in SHIPPED_TARGETS:
        for _, extra_args in BUILDS:
            built |= tree_members(rust_dir, extra_args, target)

    rows = []
    counts = {label: 0 for label in LABELS}
    counts["unbuilt"] = 0
    attention = 0
    third_party = [nv for nv in used if nv in packages]
    third_party.sort(key=lambda nv: (nv[0].lower(), version_key(nv[1])))
    for nv in third_party:
        name, version = nv
        pkg = packages[nv]
        pkg_dir = Path(pkg["manifest_path"]).parent
        if not pkg_dir.is_dir():
            fail(f"source directory missing for {name} {version}: {pkg_dir}")
        files = sorted(os.listdir(pkg_dir))
        has_license = any(LICENSE_FILE.match(f) for f in files)
        notices = [f for f in files if NOTICE_FILE.match(f)]
        license_parts = [
            license_note(pkg["license"], allowed),
            text_note(has_license, pkg.get("repository")),
            notice_note(notices),
        ]
        notes = []
        flagged = any(license_parts)
        if pkg["source"] is None:
            notes.append("Path dependency outside the workspace.")
            flagged = True
        if nv not in built:
            notes.append("Not in the macOS, Linux or Kobo builds.")
            counts["unbuilt"] += 1
        notes += license_parts
        if any("proc-macro" in target["kind"] for target in pkg["targets"]):
            notes.append(PROC_MACRO_NOTE)
        notes.append(native_note(name, pkg_dir, nv in built))
        labels = [label for label in LABELS if label in used[nv]]
        for label in labels:
            counts[label] += 1
        attention += int(flagged)
        rows.append((name, version, pkg["license"] or "(none)", ", ".join(labels),
                     " ".join(note for note in notes if note)))
    counts["total"] = len(rows)
    counts["attention"] = attention
    return rows, counts


def cell(text):
    return text.replace("|", "\\|")


def render_section(rows, counts):
    summary = ["| Item | Count |", "|---|---|", f"| Crate versions | {counts['total']} |"]
    summary += [f"| Used by {label} | {counts[label]} |" for label in LABELS]
    summary += [f"| Not in the macOS, Linux or Kobo builds | {counts['unbuilt']} |"]
    summary += [f"| Needs attention | {counts['attention']} |"]
    table = ["| Name | Version | License | Used by | Note |", "|---|---|---|---|---|"]
    table += [f"| {cell(name)} | {cell(version)} | {cell(lic)} | {cell(used)} | {cell(note)} |"
              for name, version, lic, used, note in rows]
    return [
        SECTION_TITLE,
        "",
        "Generated by `tools/kaia/inventory/rust_licenses.py`. Do not edit by hand. Rerun the script.",
        "",
        "Regenerate from the repository root. Source `tools/kaia/env.sh` first so that `cargo` is on `PATH`.",
        "",
        "```sh",
        "python3 -I tools/kaia/inventory/rust_licenses.py --root .",
        "```",
        "",
        "The script runs `cargo metadata` and `cargo tree` in `rust/` with `--locked`, so "
        "`rust/Cargo.lock` does not change. It rewrites only this section.",
        "",
        "Scope:",
        "",
        "- `kaia (desktop)` is the `kaia` binary with default features. `kaia (kobo)` is the same "
        "binary built with `--no-default-features --features kobo`. `libkaia.so` is `kaia-ffi` with "
        "default features, plus `--features glibc215` for the Kobo build. The Used by column lists "
        "every build that links the crate.",
        "- Normal dependencies for every target (`--target all`), so the rows cover more than the "
        "release builds. The release builds are macOS (`aarch64-apple-darwin`), Linux "
        "(`x86_64-unknown-linux-gnu`) and Kobo (`armv7-unknown-linux-gnueabihf`). A row that no "
        "release build compiles says so in the Note column.",
        "- Dev dependencies are left out. Build tools such as `cc`, `cmake` and `bindgen` run at "
        "build time and do not link into the product, so they have no rows. Native code that a "
        "build script compiles into the product is named in the Note column. Proc macros also run "
        "at compile time. They stay in the table, because cargo lists them as normal dependencies, "
        "and the Note column marks them.",
        "- Workspace crates have no rows. They carry the repository license. The workspace `-sys` "
        "crates (`kaia-mupdf-sys`, `kaia-cre-sys`, `kaia-djvu-sys`, `kaia-fbink-sys`) link native "
        "libraries that KOReader builds from `base/thirdparty`. Those licenses are in the Native "
        "dependencies section above.",
        "- The License column is the expression each crate declares in its `Cargo.toml`. The script "
        "does not read the license text.",
        "",
        *summary,
        "",
        "A row needs attention when `rust/deny.toml` does not allow a term in its license "
        "expression, when the crate package has no license file at its top level, or when the "
        "crate has a NOTICE file. Rows outside the release builds count too. The Note column says "
        "what to do. Notes about native code do not count.",
        "",
        *table,
        "",
        "### What the release must ship",
        "",
        "- Ship the license files of every crate in the table. Copy every license file the crate "
        "has, not only the one its expression names. `ring` has three.",
        "- Ship every NOTICE file under its original name. Apache-2.0 requires it in "
        "redistributions.",
        "- For a crate whose package has no license file, copy its license text from the repository "
        "named in the Note, and check that the copy matches the declared license.",
        "- Where an expression offers several licenses, the release records the one it uses. The "
        "Note lists the choices that `rust/deny.toml` allows.",
        "- `docs/kaia/RELEASE_SOURCE_CHECKLIST.md`, section 9, tracks these steps.",
    ]


def next_heading(lines, start):
    for index in range(start, len(lines)):
        if lines[index].startswith("## "):
            return index
    return len(lines)


def update_markdown(path, section):
    lines = path.read_text(encoding="utf-8").split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    start = next((i for i, line in enumerate(lines) if line == SECTION_TITLE), None)
    if start is not None:
        end = next_heading(lines, start + 1)
        tail = [""] if end < len(lines) else []
        lines[start:end] = section + tail
    else:
        anchor = next((i for i, line in enumerate(lines) if line.startswith(INSERT_AFTER)), None)
        if anchor is None:
            if lines and lines[-1] != "":
                lines.append("")
            lines.extend(section)
        else:
            end = next_heading(lines, anchor + 1)
            block = section + ([""] if end < len(lines) else [])
            if lines[end - 1] != "":
                block = [""] + block
            lines[end:end] = block
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv):
    ap = argparse.ArgumentParser(description="Regenerate the Rust crates section of LICENSES.md.")
    ap.add_argument("--root", default=".", help="repository root (default: current directory)")
    args = ap.parse_args(argv)
    root = Path(args.root).resolve()
    rust_dir = root / RUST_DIR
    md_path = root / LICENSES_MD
    if not (rust_dir / "Cargo.toml").is_file() or not md_path.is_file():
        fail(f"not a Kaia Reader checkout: {root}")
    allowed = read_allow_list(root / DENY_TOML)
    rows, counts = build_rows(load_metadata(rust_dir), rust_dir, allowed)
    update_markdown(md_path, render_section(rows, counts))
    print(f"rust_licenses: {counts['total']} crate versions, {counts['attention']} need attention. "
          f"Updated the Rust crates section of {LICENSES_MD}.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
