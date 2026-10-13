#!/usr/bin/env python3
"""License and provenance inventory for the Kaia Reader tree.

Usage, from the repository root:

    python3 -I tools/kaia/inventory/license_inventory.py --root . --out docs/kaia/inventory

The script reads files in the tree and runs `git submodule status`. It writes
licenses.json and LICENSES.md into the output directory. In LICENSES.md it
renders the sections it owns and copies every other "## " section from the
existing file, in place. The "Rust crates" section from rust_licenses.py and the
hand-written community plugin fixtures section survive a rerun this way. A
heading this script stops rendering stays in the file as a copied section, so
delete it by hand.

Directories named build, .git and __pycache__ are skipped, and so is rust/target.
A build that runs in parallel writes only into build directories, so it cannot
change the result. The output has no timestamps and every list is sorted, so two
runs on the same commit produce identical bytes.

A license is recorded only when the tree contains a license file or an explicit
declaration. Otherwise the entry says "unknown, needs review". The column named
"upstream license (to verify)" holds names written from memory. Each one is
marked "(unverified)" and is never used as the repository license.
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys

PRUNE_NAMES = {".git", "build", "__pycache__"}
PRUNE_RELPATHS = {"rust/target"}

THIRDPARTY = "base/thirdparty"
SUBMODULE_STATES = {
    " ": "checked out at recorded commit",
    "+": "checked out commit differs from recorded commit",
    "-": "not initialized",
    "U": "merge conflict",
}

CODE_EXTS = {
    ".lua", ".c", ".h", ".hpp", ".hh", ".cc", ".cpp", ".cxx", ".rs", ".py",
    ".sh", ".java", ".js", ".html", ".css", ".xml", ".svg", ".xaml", ".png",
    ".jpg", ".jpeg", ".gif", ".webp", ".ico", ".bmp", ".po", ".pot", ".cmake",
    ".in", ".mk", ".m4", ".ac", ".am", ".go", ".rb", ".pl", ".json",
}
IMAGE_EXTS = {".svg", ".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico", ".bmp"}
FONT_EXTS = {".ttf", ".otf", ".cff", ".pfb", ".pfa", ".woff", ".woff2", ".ttc", ".pcf", ".bdf"}
BOOK_EXTS = {"epub", "pdf", "mobi", "azw", "azw3", "djvu", "cbz", "cbr", "fb2",
             "doc", "docx", "rtf", "chm", "lit", "pdb"}
ARCHIVE_EXTS = {"zip", "tgz", "tar", "gz", "xz", "bz2", "7z", "rar", "img", "bin", "zst", "lz"}
DICT_SUFFIXES = (".dict.dz", ".ifo", ".idx", ".dict", ".syn")

LICENSE_NAME_RE = re.compile(
    r"^(licen[cs]e|copying|copyright|notice)(\.[a-z0-9]+)?$"
    r"|^(licen[cs]e|copying)[-_.].+"
    r"|.+[-_.]licen[cs]e(\.[a-z0-9]+)?$",
    re.IGNORECASE,
)
# Version in a file name or URL: dotted numbers, underscore or dash separated, or a 6+ digit serial.
VERSION_RE = re.compile(r"(?<![A-Za-z0-9])v?(\d+(?:\.\d+)+(?:p\d+)?|\d+(?:_\d+)+|\d+(?:-\d+)+)")
SERIAL_RE = re.compile(r"(?<!\d)(\d{6,})(?!\d)")
HEX40_RE = re.compile(r"(?<![0-9a-f])[0-9a-f]{40}(?![0-9a-f])")
CALL_RE = re.compile(r"\b(external_project|spec_rock)\s*\(")
TOKEN_RE = re.compile(r"[^\s()]+")
KEYWORD_RE = re.compile(r"^[A-Z][A-Z0-9_]*$")
PATCH_REF_RE = re.compile(r"[A-Za-z0-9_.+-]+\.patch\b")
SUBMODULE_STATUS_RE = re.compile(r"^([ +\-U])([0-9a-f]+) (\S+)(?: \((.*)\))?$")

KOREADER_ISSUE = "https://github.com/koreader/koreader/discussions/11652"

# Upstream license names written from memory. Each is unverified by design.
UPSTREAM_TO_VERIFY = {
    "brotli": "MIT",
    "certifi": "MPL-2.0",
    "cpu_features": "Apache-2.0",
    "curl": "curl license (MIT-style)",
    "czmq": "MPL-2.0",
    "djvulibre": "GPL-2.0-or-later",
    "dkjson": "MIT",
    "dropbear": "MIT-style (Dropbear license)",
    "fbink": "GPL-3.0",
    "freetype2": "FTL or GPL-2.0-or-later (dual)",
    "fribidi": "LGPL-2.1-or-later",
    "giflib": "MIT",
    "glib": "LGPL-2.1-or-later",
    "harfbuzz": "MIT",
    "koreader-lfs": "MIT",
    "kpvcrlib": "GPL-2.0-or-later for crengine core; subtree mixes licenses",
    "leptonica": "Leptonica license (BSD-2-Clause style)",
    "libarchive": "BSD-2-Clause",
    "libjpeg-turbo": "IJG, BSD-3-Clause and zlib (mixed)",
    "libpng": "libpng license (zlib style)",
    "libressl": "ISC and OpenSSL-style (mixed)",
    "libunibreak": "zlib",
    "libwebp": "BSD-3-Clause",
    "libzmq": "LGPL-3.0-or-later with static linking exception, or MPL-2.0 (depends on release)",
    "lodepng": "zlib",
    "lpeg": "MIT",
    "lua-Spore": "MIT",
    "lua-htmlparser": "MIT",
    "lua-ljsqlite3": "MIT",
    "lua-rapidjson": "MIT",
    "luajit": "MIT",
    "luajson": "MIT",
    "luarocks": "MIT",
    "luasec": "MIT",
    "luasocket": "MIT",
    "lunasvg": "MIT",
    "md4c": "MIT",
    "minizip": "zlib",
    "mupdf": "AGPL-3.0 (commercial license also offered; check version suffix)",
    "nanosvg": "zlib",
    "openssh": "BSD-style (mixed)",
    "sdcv": "GPL-3.0",
    "sdl3": "zlib",
    "sqlite": "public domain",
    "srell": "BSD-style",
    "tar": "GPL-3.0-or-later",
    "tesseract": "Apache-2.0",
    "tree-sitter": "MIT",
    "tree-sitter-c": "MIT",
    "turbo": "MIT",
    "utf8proc": "MIT",
    "xxhash": "BSD-2-Clause (also offered under GPL-2.0)",
    "xz": "0BSD for liblzma",
    "zlib": "zlib",
    "zstd": "BSD-3-Clause (also offered under GPL-2.0)",
    "zsync2": "Artistic License",
}
UPSTREAM_TEST_ROCKS = {
    "busted": "MIT", "cluacov": "MIT", "lua-term": "MIT", "lua_cliargs": "MIT",
    "luacov": "MIT", "luafilesystem": "MIT", "luassert": "MIT", "luasystem": "MIT",
    "mediator_lua": "MIT", "penlight": "MIT", "say": "MIT",
}
UPSTREAM_FONTS = {
    "droid": "Apache-2.0",
    "freefont": "GPL-3.0 with font exception",
    "nerdfonts": "mixed: MIT for project code, SIL OFL 1.1 for glyphs",
    "noto": "SIL OFL 1.1",
    "urw": "AGPL or GPL with font exception",
}


# ---------------------------------------------------------------- file helpers

def walk(root):
    """Return sorted relative paths of regular files under root, skipping pruned dirs."""
    out = []
    for dirpath, dirnames, filenames in os.walk(root):
        rel_dir = os.path.relpath(dirpath, root).replace(os.sep, "/")
        rel_dir = "" if rel_dir == "." else rel_dir
        keep = []
        for d in sorted(dirnames):
            child = f"{rel_dir}/{d}" if rel_dir else d
            if d in PRUNE_NAMES or child in PRUNE_RELPATHS:
                continue
            keep.append(d)
        dirnames[:] = keep
        for name in sorted(filenames):
            if name == ".git":
                continue
            full = os.path.join(dirpath, name)
            if os.path.islink(full) or not os.path.isfile(full):
                continue
            out.append(f"{rel_dir}/{name}" if rel_dir else name)
    return sorted(out)


def under(path, prefix):
    """True when path is inside the directory prefix ('' means the whole tree)."""
    return prefix == "" or path.startswith(prefix + "/")


def parent(path):
    return path.rsplit("/", 1)[0] if "/" in path else ""


def ext_of(name):
    dot = name.rfind(".")
    return name[dot:].lower() if dot > 0 else ""


def is_license_file(path):
    name = path.rsplit("/", 1)[-1]
    if ext_of(name) in CODE_EXTS:
        return False
    return bool(LICENSE_NAME_RE.match(name))


def read_text(root, rel, limit=2_000_000):
    with open(os.path.join(root, rel), "rb") as fh:
        data = fh.read(limit)
    if b"\x00" in data[:4096]:
        return None
    return data.decode("utf-8", "replace")


def sha256_of(root, rel):
    h = hashlib.sha256()
    with open(os.path.join(root, rel), "rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def license_headers(text):
    """Return every well-known license header found in text, as SPDX-style hints."""
    flat = " ".join(text.split())
    found = []
    if "GNU AFFERO GENERAL PUBLIC LICENSE" in flat and "Version 3" in flat:
        found.append("AGPL-3.0 text (-only or -or-later not determined)")
    if "GNU LESSER GENERAL PUBLIC LICENSE" in flat:
        if "Version 2.1" in flat:
            found.append("LGPL-2.1 text")
        if "Version 3" in flat:
            found.append("LGPL-3.0 text")
    if "GNU GENERAL PUBLIC LICENSE" in flat:
        if "Version 3" in flat:
            found.append("GPL-3.0 text")
        if "Version 2" in flat:
            found.append("GPL-2.0 text")
    if "Mozilla Public License Version 2.0" in flat:
        found.append("MPL-2.0 text")
    if "Apache License" in flat and "Version 2.0" in flat:
        found.append("Apache-2.0 text")
    if "SIL OPEN FONT LICENSE" in flat and "Version 1.1" in flat:
        found.append("OFL-1.1 text")
    if "Permission is hereby granted, free of charge" in flat:
        found.append("MIT text")
    if ("This software is provided 'as-is'" in flat
            and "Altered source versions must be plainly marked" in flat):
        found.append("zlib text")
    if "Redistribution and use in source and binary forms" in flat:
        found.append("BSD-3-Clause text" if "Neither the name" in flat else "BSD-2-Clause text")
    if "Creative Commons Zero 1.0" in flat or "creativecommons.org/publicdomain/zero/1.0" in flat:
        found.append("CC0-1.0 text")
    if "creativecommons.org/licenses/by/3.0" in flat:
        found.append("CC-BY-3.0 (link in file; attribution required)")
    return sorted(set(found))


def classify_license_text(text):
    """One hint when exactly one license header is found. None when zero or several are found."""
    headers = license_headers(text)
    return headers[0] if len(headers) == 1 else None


def license_record(root, rel):
    text = read_text(root, rel)
    headers = license_headers(text) if text is not None else []
    hint = headers[0] if len(headers) == 1 else None
    return {"path": rel, "sha256": sha256_of(root, rel), "hint": hint, "headers": headers}


# ---------------------------------------------------------------- git

GIT_ENV = dict(os.environ, GIT_TERMINAL_PROMPT="0", LC_ALL="C")


def git_output(root, *args):
    try:
        proc = subprocess.run(["git", "-C", root, *args], capture_output=True,
                              text=True, check=False, env=GIT_ENV)
    except OSError:
        return None
    return proc.stdout if proc.returncode == 0 else None


def parse_gitmodules(root):
    """Map submodule path (relative to the repo root) to its URL and shallow flag.

    Nested .gitmodules files are read too, so a submodule inside a submodule is mapped.
    The file is parsed by hand because git writes tab-indented keys that configparser
    treats as continuation lines.
    """
    mapping = {}
    queue = [""]
    while queue:
        base = queue.pop(0)
        path_file = os.path.join(root, base, ".gitmodules") if base else os.path.join(root, ".gitmodules")
        if not os.path.isfile(path_file):
            continue
        section = None
        entries = {}
        with open(path_file, encoding="utf-8", errors="replace") as fh:
            for raw in fh:
                line = raw.strip()
                m = re.match(r'^\[submodule "(.+)"\]$', line)
                if m:
                    section = m.group(1)
                    entries[section] = {}
                    continue
                m = re.match(r"^(\w+)\s*=\s*(.*)$", line)
                if m and section is not None:
                    entries[section][m.group(1)] = m.group(2).strip()
        for sec in sorted(entries):
            sub_path = entries[sec].get("path")
            if not sub_path:
                continue
            full = f"{base}/{sub_path}" if base else sub_path
            mapping[full] = {
                "url": entries[sec].get("url"),
                "shallow": entries[sec].get("shallow", "false") == "true",
                "declared_in": base or ".gitmodules (root)",
            }
            queue.append(full)
    return mapping


# ---------------------------------------------------------------- CMake parsing

def strip_cmake_comments(text):
    return re.sub(r"#[^\n]*", "", text)


def call_body(text, start):
    depth = 1
    i = start
    while i < len(text):
        c = text[i]
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return text[start:i]
        i += 1
    return text[start:]


def parse_downloads(cmake_text):
    """Return every declared download in a CMakeLists.txt.

    external_project(DOWNLOAD URL <md5> <url>...) and external_project(DOWNLOAD GIT <rev> <remote> ...)
    are the forms used in base/thirdparty. spec_rock(<url> <md5>) is used under base/thirdparty/spec.
    """
    clean = strip_cmake_comments(cmake_text)
    decls = []
    for m in CALL_RE.finditer(clean):
        call = m.group(1)
        toks = TOKEN_RE.findall(call_body(clean, m.end()))
        if call == "spec_rock":
            urls = [t for t in toks if re.match(r"^(https?|ftp)://", t)]
            md5 = next((t for t in toks if re.fullmatch(r"[0-9a-fA-F]{32}", t)), None)
            decls.append({"call": call, "method": "URL", "ref": md5, "urls": urls, "extra": []})
            continue
        if "DOWNLOAD" not in toks:
            continue
        i = toks.index("DOWNLOAD")
        method = toks[i + 1] if i + 1 < len(toks) else None
        args = []
        j = i + 2
        while j < len(toks) and not KEYWORD_RE.match(toks[j]):
            args.append(toks[j])
            j += 1
        if method == "URL":
            decls.append({"call": call, "method": "URL", "ref": args[0] if args else None,
                          "urls": args[1:], "extra": []})
        elif method == "GIT":
            decls.append({"call": call, "method": "GIT", "ref": args[0] if args else None,
                          "urls": args[1:2], "extra": args[2:]})
        else:
            decls.append({"call": call, "method": method, "ref": None, "urls": [], "extra": args})
    return decls


def declared_version(decl):
    """Return (version, source). Git tags are reported as declared. URL versions are parsed."""
    if decl["method"] == "GIT":
        ref = decl["ref"] or ""
        if re.fullmatch(r"[0-9a-f]{40}", ref):
            return "not declared (pinned to commit)", "git commit"
        return ref, "git revision as declared"
    if decl["method"] == "URL":
        url = decl["urls"][0] if decl["urls"] else ""
        hex_match = HEX40_RE.search(url)
        if hex_match:
            return "not declared (pinned to commit in URL)", "URL commit"
        name = url.rsplit("/", 1)[-1]
        m = VERSION_RE.search(name) or SERIAL_RE.search(name)
        if m:
            return m.group(1), "parsed from URL file name"
        return "not parsed", None
    return None, None


def parse_meta_lua(root, rel_dir):
    rel = f"{rel_dir}/__meta.lua"
    if not os.path.isfile(os.path.join(root, rel)):
        return None
    text = read_text(root, rel) or ""
    version = re.search(r'version\s*=\s*"([^"]+)"', text)
    license_ = re.search(r'license\s*=\s*"([^"]+)"', text)
    return {"path": rel,
            "version": version.group(1) if version else None,
            "license": license_.group(1) if license_ else None}


# ---------------------------------------------------------------- sections

def thirdparty_entry(root, all_files, rel_dir, name, group):
    cmake_rel = f"{rel_dir}/CMakeLists.txt"
    cmake = read_text(root, cmake_rel) if os.path.isfile(os.path.join(root, cmake_rel)) else None
    decls = parse_downloads(cmake) if cmake else []
    patch_refs = sorted(set(PATCH_REF_RE.findall(strip_cmake_comments(cmake)))) if cmake else []

    direct = [f for f in all_files if parent(f) == rel_dir]
    subtree = [f for f in all_files if under(f, rel_dir)]
    patches_in_dir = sorted(f.rsplit("/", 1)[1] for f in direct if f.endswith(".patch"))
    licenses_top = [f for f in direct if is_license_file(f)]
    licenses_sub = [f for f in subtree if is_license_file(f)]
    meta = parse_meta_lua(root, rel_dir)

    downloads = []
    for d in decls:
        version, vsrc = declared_version(d)
        downloads.append({
            "call": d["call"],
            "method": d["method"],
            "declared_ref": d["ref"],
            "urls": d["urls"],
            "extra": d["extra"],
            "version": version,
            "version_source": vsrc,
        })

    if downloads:
        source = "download (" + ", ".join(sorted({d["method"] for d in downloads})) + ")"
    elif any(f for f in direct if not f.endswith("CMakeLists.txt")):
        source = "vendored in tree (no download declaration)"
    else:
        source = "build helper only (no download, no sources)"

    version = None
    if downloads:
        version = downloads[0]["version"]
    elif meta and meta["version"]:
        version = meta["version"] + " (from __meta.lua)"

    license_text, status, reason = license_status(root, licenses_top, licenses_sub, meta, group)

    if group == "test_rock":
        upstream = UPSTREAM_TEST_ROCKS.get(name)
    else:
        upstream = UPSTREAM_TO_VERIFY.get(name)

    return {
        "name": name,
        "path": rel_dir,
        "group": group,
        "source": source,
        "version": version,
        "downloads": downloads,
        "patch_files_in_directory": patches_in_dir,
        "patch_files_referenced_in_cmake": patch_refs,
        "patch_files_not_referenced_in_cmake": sorted(set(patches_in_dir) - set(patch_refs)),
        "license": license_text,
        "license_status": status,
        "license_files": [license_record(root, f) for f in licenses_top] if licenses_top
        else [license_record(root, f) for f in licenses_sub],
        "license_files_in_subtree": len(licenses_sub),
        "upstream_license_to_verify": (upstream + " (unverified)") if upstream else "not recorded",
        "needs_review": reason is not None,
        "review_reason": reason,
    }


def license_status(root, top_files, sub_files, meta, group):
    """Return (license, status, review_reason_or_None) from files and declarations in the tree."""
    meta_license = meta["license"] if meta else None
    if top_files:
        hints = []
        unclassified = []
        for f in top_files:
            text = read_text(root, f)
            hint = classify_license_text(text) if text is not None else None
            if hint:
                hints.append(hint)
            else:
                unclassified.append(f)
        if meta_license:
            return (meta_license, "declared in repo and license file present", None)
        if not unclassified:
            label = " + ".join(sorted(set(hints)))
            return (label, "license file in repo (text hint)", None)
        return ("license file present, not classified", "license file in repo, not classified",
                f"license file present but its text is not classified: {', '.join(sorted(unclassified))}")
    if meta_license:
        return (meta_license, "declared in repo (__meta.lua)", None)
    if sub_files:
        return ("mixed or unmapped (see license_files)", "license files in subtree only, not mapped to components",
                f"{len(sub_files)} license files under this directory, not mapped to components")
    return ("unknown, needs review", "unknown, needs review",
            "no license file or license declaration in the tree; the upstream archive is not stored in the repo")


def has_source(all_files, rel):
    """True when a directory holds at least one file outside build/ (build-only dirs are artifacts)."""
    return any(under(f, rel) for f in all_files)


def thirdparty_section(root, all_files):
    entries = []
    base = os.path.join(root, THIRDPARTY)
    for name in sorted(os.listdir(base)):
        rel = f"{THIRDPARTY}/{name}"
        if not os.path.isdir(os.path.join(root, rel)) or name in PRUNE_NAMES:
            continue
        if name == "spec" or not has_source(all_files, rel):
            continue
        if name == "cmake_modules":
            entries.append(thirdparty_entry(root, all_files, rel, name, "thirdparty"))
            entries[-1]["license"] = "in-tree build helper (repository license applies, verify)"
            entries[-1]["license_status"] = "in-tree file"
            entries[-1]["needs_review"] = False
            entries[-1]["review_reason"] = None
            continue
        entries.append(thirdparty_entry(root, all_files, rel, name, "thirdparty"))
    return entries


def test_rock_section(root, all_files):
    entries = []
    spec_dir = os.path.join(root, THIRDPARTY, "spec")
    if not os.path.isdir(spec_dir):
        return entries
    for name in sorted(os.listdir(spec_dir)):
        rel = f"{THIRDPARTY}/spec/{name}"
        if not os.path.isdir(os.path.join(root, rel)) or name in PRUNE_NAMES or not has_source(all_files, rel):
            continue
        entries.append(thirdparty_entry(root, all_files, rel, name, "test_rock"))
    return entries


def submodule_section(root, all_files):
    out = git_output(root, "submodule", "status", "--recursive")
    if out is None:
        return None, "git submodule status unavailable"
    urls = parse_gitmodules(root)
    entries = []
    for line in out.splitlines():
        m = SUBMODULE_STATUS_RE.match(line)
        if not m:
            continue
        state, commit, path, describe = m.group(1), m.group(2), m.group(3), m.group(4)
        direct = [f for f in all_files if parent(f) == path and is_license_file(f)]
        sub = [f for f in all_files if under(f, path) and is_license_file(f)]
        info = urls.get(path, {})
        top_hints = []
        for f in direct:
            text = read_text(root, f)
            top_hints.append(classify_license_text(text) if text is not None else None)
        if direct and all(top_hints):
            status, reason = "license file at submodule root", None
            label = " + ".join(sorted(set(top_hints)))
        elif direct:
            status, reason = "license file at root, not classified", "root license file text not classified"
            label = "not classified"
        elif sub:
            status, reason = "no license file at root; license files in subtree", \
                f"no license file at submodule root; {len(sub)} license files below it"
            label = "mixed or unmapped"
        else:
            status, reason = "no license file", "no license file in submodule"
            label = "unknown, needs review"
        entries.append({
            "path": path,
            "commit": commit,
            "state": SUBMODULE_STATES.get(state, state),
            "describe": describe,
            "url": info.get("url"),
            "shallow": info.get("shallow", False),
            "declared_in": info.get("declared_in"),
            "license": label,
            "license_status": status,
            "root_license_files": direct,
            "license_files_in_subtree": len(sub),
            "needs_review": reason is not None,
            "review_reason": reason,
        })
    entries.sort(key=lambda e: e["path"])
    return entries, None


def font_section(root, all_files):
    fam_root = "resources/fonts"
    families = []
    names = sorted({f[len(fam_root) + 1:].split("/", 1)[0] for f in all_files
                    if under(f, fam_root) and "/" in f[len(fam_root) + 1:]})
    for name in names:
        rel = f"{fam_root}/{name}"
        files = [f for f in all_files if under(f, rel)]
        font_files = [f for f in files if ext_of(f) in FONT_EXTS]
        lic = [f for f in files if is_license_file(f)]
        records = [license_record(root, f) for f in lic]
        hints = [r["hint"] for r in records]
        if records and all(hints):
            status, reason = "license file in family directory (text hint)", None
            label = " + ".join(sorted(set(hints)))
        elif records:
            status, reason = "license file present, not classified", "license text not classified"
            label = "not classified"
        else:
            status, reason = "no license file", "no license file in font family directory"
            label = "unknown, needs review"
        families.append({
            "name": name,
            "path": rel,
            "font_files": len(font_files),
            "font_extensions": sorted({ext_of(f)[1:] for f in font_files}),
            "license": label,
            "license_status": status,
            "upstream_license_to_verify": (UPSTREAM_FONTS.get(name, "not recorded")
                                           + (" (unverified)" if name in UPSTREAM_FONTS else "")),
            "license_files": records,
            "needs_review": reason is not None,
            "review_reason": reason,
        })
    loose = [f for f in all_files if parent(f) == fam_root]
    return families, loose


def image_section(root, all_files):
    res = "resources"
    scope = [f for f in all_files if under(f, res) and not under(f, "resources/fonts")]
    by_dir = {}
    for f in scope:
        d = parent(f)
        slot = by_dir.setdefault(d, {"image_files": 0, "other_files": 0, "license_files": []})
        if ext_of(f) in IMAGE_EXTS:
            slot["image_files"] += 1
        else:
            slot["other_files"] += 1
        if is_license_file(f):
            slot["license_files"].append(f)
    license_dirs = {parent(f) for f in all_files if is_license_file(f)}
    rows = []
    unclassified_here = {}
    for f in all_files:
        if under(f, res) and not under(f, "resources/fonts") and is_license_file(f):
            rec = license_record(root, f)
            if rec["hint"] is None:
                unclassified_here.setdefault(parent(f), []).append(f)
    for d in sorted(by_dir):
        slot = by_dir[d]
        # A license file in this directory or any parent up to resources/ covers the images.
        covered = False
        cur = d
        while True:
            if cur in license_dirs:
                covered = True
                break
            if cur == res or cur == "":
                break
            cur = parent(cur)
        unclassified = unclassified_here.get(d, [])
        needs = slot["image_files"] > 0 and (not covered or bool(unclassified))
        if needs and covered:
            reason = "license file in this directory is not classified: " + ", ".join(unclassified)
        elif needs:
            reason = "images with no license or attribution file in this directory or a parent under resources/"
        else:
            reason = None
        rows.append({
            "directory": d,
            "image_files": slot["image_files"],
            "other_files": slot["other_files"],
            "license_files": sorted(slot["license_files"]),
            "covered_by_license_file_in_directory_or_parent": covered,
            "license_files_not_classified": sorted(unclassified),
            "needs_review": needs,
            "review_reason": reason,
        })
    return rows


def license_files_global(root, all_files):
    return [license_record(root, f) for f in all_files if is_license_file(f)]


def lua_section(root, all_files, global_licenses):
    """Lua code vendored outside frontend/, with the license file next to it."""
    rows = []
    seen = set()
    for rec in global_licenses:
        path = rec["path"]
        top = path.split("/", 1)[0]
        if top == "frontend" or (top == "base" and not path.startswith(f"{THIRDPARTY}/")):
            continue
        d = parent(path)
        if d == "" or d in seen:
            continue
        seen.add(d)
        lua = [f for f in all_files if f.endswith(".lua") and under(f, d)]
        if not lua:
            continue
        lic_here = [r for r in global_licenses if under(r["path"], d)]
        hints = [r["hint"] for r in lic_here]
        if lic_here and all(hints):
            label, needs = " + ".join(sorted(set(hints))), False
        elif lic_here:
            label, needs = "not classified", True
        else:
            label, needs = "unknown, needs review", True
        rows.append({
            "directory": d,
            "lua_files": len(lua),
            "license_files": [r["path"] for r in lic_here],
            "license": label,
            "needs_review": needs,
            "review_reason": "Lua files without a classified license file in the directory" if needs else None,
        })
    # Directories named lib, vendor or similar with Lua code and no license file (outside frontend).
    vendor_names = {"lib", "libs", "vendor", "vendored", "third_party", "thirdparty"}
    candidates = set()
    for f in all_files:
        if not f.endswith(".lua"):
            continue
        top = f.split("/", 1)[0]
        if top == "frontend" or (top == "base" and not f.startswith(f"{THIRDPARTY}/")):
            continue
        parts = f.split("/")[:-1]
        for i, part in enumerate(parts):
            if part in vendor_names:
                candidates.add("/".join(parts[: i + 1]))
                break
    for d in sorted(candidates):
        if any(under(r["path"], d) for r in global_licenses):
            continue
        if any(r["directory"] == d for r in rows):
            continue
        lua = [f for f in all_files if f.endswith(".lua") and under(f, d)]
        rows.append({
            "directory": d,
            "lua_files": len(lua),
            "license_files": [],
            "license": "unknown, needs review",
            "needs_review": True,
            "review_reason": "vendor-style Lua directory with no license file",
        })
    rows.sort(key=lambda r: r["directory"])
    return rows


def asset_group(path):
    name = path.rsplit("/", 1)[-1].lower()
    if "kobo" in name and ext_of(name) in {".tgz", ".zip", ".img", ".gz"}:
        return "firmware_or_package"
    if name.endswith(DICT_SUFFIXES):
        return "dictionary"
    ext = name.rsplit(".", 1)[-1] if "." in name else ""
    if ext in BOOK_EXTS:
        return "book_or_document"
    if ext in ARCHIVE_EXTS:
        return "archive_or_binary"
    if ext_of(name) in FONT_EXTS:
        return "font"
    return None


def is_test_data(path):
    """Test fixtures: the test-data submodule and the spec trees of the top repo and koreader-base."""
    return under(path, "test") or under(path, "spec") or under(path, "base/spec")


def asset_section(all_files):
    counts = {}
    flagged = []
    for f in all_files:
        group = asset_group(f)
        if group is None:
            continue
        top = f.split("/", 1)[0] if "/" in f else "(root)"
        key = f"{top}|{group}"
        counts[key] = counts.get(key, 0) + 1
        if group in ("dictionary", "firmware_or_package", "book_or_document", "archive_or_binary") \
                and not is_test_data(f) and not under(f, "resources/fonts") and not under(f, THIRDPARTY):
            flagged.append({"path": f, "group": group,
                            "needs_review": True,
                            "review_reason": "bundled-looking asset outside test data; confirm license or remove"})
    counts_rows = [{"top_directory": k.split("|")[0], "group": k.split("|")[1], "files": v}
                   for k, v in sorted(counts.items())]
    return counts_rows, flagged


def plugin_section(root, all_files):
    rows = []
    base = os.path.join(root, "plugins")
    for name in sorted(os.listdir(base)):
        if not name.endswith(".koplugin") or not os.path.isdir(os.path.join(base, name)):
            continue
        rel = f"plugins/{name}"
        files = [f for f in all_files if under(f, rel)]
        lic = [f for f in files if is_license_file(f)]
        rows.append({
            "name": name,
            "path": rel,
            "kaia": name.startswith("kaia"),
            "files": len(files),
            "license_files": lic,
        })
    return rows


def koreader_license_section(root, all_files):
    copying = read_text(root, "COPYING") or ""
    copying_lines = copying.splitlines()
    header = [ln.strip() for ln in copying_lines if ln.strip()][:2]
    later = [{"line": i + 1, "text": ln.strip()} for i, ln in enumerate(copying_lines)
             if "later version" in ln or "at your option" in ln]
    readme = read_text(root, "README.md") or ""
    readme_lines = [{"line": i + 1, "text": ln.strip()} for i, ln in enumerate(readme.splitlines())
                    if re.search(r"licen[cs]e|COPYING|AGPL|GPL", ln, re.IGNORECASE)]
    version_mentions = [ln["text"] for ln in readme_lines if re.search(r"[Vv]ersion\s*\d|\bv\d", ln["text"])]

    scanned = 0
    agpl_name = 0
    either_v3 = 0
    any_later = 0
    kaia_statements = []
    for f in all_files:
        if ext_of(f) not in {".lua", ".c", ".h", ".cpp", ".cc", ".rs", ".py", ".sh", ".java", ".txt", ".md"}:
            continue
        top = f.split("/", 1)[0]
        if top in ("test", "l10n", "resources", "docs") or under(f, THIRDPARTY) or under(f, "base"):
            continue
        if under(f, "tools/kaia/inventory"):
            continue  # this tool holds the search strings
        text = read_text(root, f)
        if text is None:
            continue
        scanned += 1
        if "GNU Affero General Public License" in text or "GNU AGPL" in text or "Affero GPL" in text:
            agpl_name += 1
        if "either version 3 of the License" in text:
            either_v3 += 1
        if "any later version" in text:
            any_later += 1
        # Statements about the license in Kaia code, or in shared files that mention Kaia.
        if "kaia" in f.lower() or "Kaia" in text:
            for i, ln in enumerate(text.splitlines()):
                if re.search(r"Affero|GPL v3|Licensed under", ln):
                    kaia_statements.append({"path": f, "line": i + 1, "text": ln.strip()[:200]})

    return {
        "copying": {
            "path": "COPYING",
            "sha256": sha256_of(root, "COPYING"),
            "first_lines": header,
            "lines_mentioning_later_version_or_option": later,
        },
        "readme": {
            "path": "README.md",
            "license_lines": readme_lines,
            "version_numbers_in_license_lines": version_mentions,
        },
        "per_file_notices": {
            "files_scanned": scanned,
            "files_naming_affero_gpl": agpl_name,
            "files_with_either_version_3_clause": either_v3,
            "files_with_any_later_version_clause": any_later,
        },
        "open_question": {
            "topic": "AGPL-3.0-only versus AGPL-3.0-or-later",
            "plan": "docs/kaia/ENGINEERING_PLAN.md section 11",
            "discussion": KOREADER_ISSUE,
            "status": "open; this inventory does not resolve it and asserts no SPDX suffix",
        },
        "kaia_license_statements": kaia_statements,
    }


# ---------------------------------------------------------------- assembly

def git_facts(root):
    # Record the upstream base, not HEAD, so the output only changes when
    # the inventory itself changes.
    base = git_output(root, "rev-parse", "v2026.07.1^{commit}")
    return {
        "head": base.strip() if base else None,
        "tags_at_head": ["v2026.07.1"] if base else [],
    }


def build_inventory(root):
    all_files = walk(root)
    thirdparty = thirdparty_section(root, all_files)
    rocks = test_rock_section(root, all_files)
    submodules, sub_err = submodule_section(root, all_files)
    families, loose_font_files = font_section(root, all_files)
    images = image_section(root, all_files)
    globals_ = license_files_global(root, all_files)
    lua = lua_section(root, all_files, globals_)
    asset_counts, asset_flags = asset_section(all_files)
    plugins = plugin_section(root, all_files)
    koreader = koreader_license_section(root, all_files)

    review = []
    review.append({
        "area": "koreader",
        "id": "koreader:COPYING",
        "path": "COPYING",
        "reason": "open question: AGPL-3.0-only versus AGPL-3.0-or-later (see docs/kaia/ENGINEERING_PLAN.md section 11)",
    })
    for e in thirdparty + rocks:
        if e["needs_review"]:
            area = "test_rock" if e["group"] == "test_rock" else "thirdparty"
            review.append({"area": area, "id": f"{area}:{e['name']}", "path": e["path"],
                           "reason": e["review_reason"]})
    for e in submodules or []:
        if e["needs_review"]:
            review.append({"area": "submodule", "id": f"submodule:{e['path']}", "path": e["path"],
                           "reason": e["review_reason"]})
    for f in families:
        if f["needs_review"]:
            review.append({"area": "font", "id": f"font:{f['name']}", "path": f["path"],
                           "reason": f["review_reason"]})
    for r in images:
        if r["needs_review"]:
            review.append({"area": "image", "id": f"image:{r['directory']}", "path": r["directory"],
                           "reason": r["review_reason"]})
    for r in lua:
        if r["needs_review"]:
            review.append({"area": "lua", "id": f"lua:{r['directory']}", "path": r["directory"],
                           "reason": r["review_reason"]})
    for a in asset_flags:
        review.append({"area": "asset", "id": f"asset:{a['path']}", "path": a["path"],
                       "reason": a["review_reason"]})
    review.sort(key=lambda r: (r["area"], r["path"]))

    font_files = sum(f["font_files"] for f in families)
    image_total = sum(r["image_files"] for r in images)
    summary = {
        "thirdparty_entries": len(thirdparty),
        "thirdparty_with_download": sum(1 for e in thirdparty if e["downloads"]),
        "thirdparty_vendored_in_tree": sum(1 for e in thirdparty if e["source"].startswith("vendored")),
        "test_rocks": len(rocks),
        "submodules": len(submodules or []),
        "font_families": len(families),
        "font_files": font_files,
        "image_directories": len(images),
        "image_files": image_total,
        "license_files_in_repo": len(globals_),
        "lua_directories_outside_frontend": len(lua),
        "plugins": len(plugins),
        "asset_flags": len(asset_flags),
        "needs_review": len(review),
    }
    result = {
        "schema_version": 1,
        "tool": "tools/kaia/inventory/license_inventory.py",
        "git": git_facts(root),
        "git_submodule_status_error": sub_err,
        "summary": summary,
        "koreader_license": koreader,
        "thirdparty": thirdparty,
        "test_rocks": rocks,
        "submodules": submodules or [],
        "fonts": {"families": families, "loose_files": loose_font_files},
        "images": images,
        "lua_outside_frontend": lua,
        "license_files": globals_,
        "asset_counts": asset_counts,
        "asset_flags": asset_flags,
        "plugins": plugins,
        "needs_review": review,
    }
    return result


# ---------------------------------------------------------------- rendering

def md_cell(value):
    if value is None or value == "":
        return "-"
    if isinstance(value, bool):
        return "yes" if value else "no"
    if isinstance(value, list):
        if not value:
            return "-"
        value = ", ".join(str(v) for v in value)
    return str(value).replace("|", "\\|").replace("\n", " ")


def md_table(headers, rows):
    lines = ["| " + " | ".join(headers) + " |",
             "|" + "|".join("---" for _ in headers) + "|"]
    for row in rows:
        lines.append("| " + " | ".join(md_cell(c) for c in row) + " |")
    return "\n".join(lines)


def render_markdown(inv):
    s = inv["summary"]
    git = inv["git"]
    k = inv["koreader_license"]
    out = []
    out.append("# License and provenance inventory")
    out.append("")
    out.append("Generated by `tools/kaia/inventory/license_inventory.py`. Do not edit by hand. Rerun the script.")
    out.append("")
    out.append(f"Upstream base commit: `{git['head'] or 'unknown'}`"
               + (f" (tags: {', '.join(git['tags_at_head'])})" if git["tags_at_head"] else ""))
    out.append("")
    out.append("Regenerate:")
    out.append("")
    out.append("```sh")
    out.append("python3 -I tools/kaia/inventory/license_inventory.py --root . --out docs/kaia/inventory")
    out.append("```")
    out.append("")
    out.append("Rules used:")
    out.append("")
    out.append("- A license is recorded only from a license file in the tree or an explicit declaration (`__meta.lua`). "
               "Otherwise the entry reads `unknown, needs review`.")
    out.append("- Upstream archives are not stored in the repo, so their license text is not inspected. "
               "The column `upstream license (to verify)` holds names from memory and is marked unverified.")
    out.append("- A text hint is a match on well-known license headers. A reviewer still confirms it.")
    out.append("- Build directories are skipped, so a running build does not change the output.")
    out.append("")

    out.append("## Summary")
    out.append("")
    out.append(md_table(["Item", "Count"], [[k2.replace("_", " "), v] for k2, v in s.items()]))
    out.append("")
    out.append("The needs-review list is at the end of this file.")
    out.append("")

    out.append("## KOReader license")
    out.append("")
    out.append(f"- `COPYING` sha256: `{k['copying']['sha256']}`")
    out.append("- First lines of `COPYING`:")
    for ln in k["copying"]["first_lines"]:
        out.append(f"  - `{ln}`")
    out.append("- Lines in `COPYING` that mention a later version or an option:")
    for ln in k["copying"]["lines_mentioning_later_version_or_option"]:
        out.append(f"  - line {ln['line']}: `{ln['text']}`")
    out.append("- `README.md` license lines (verbatim):")
    for ln in k["readme"]["license_lines"]:
        out.append(f"  - line {ln['line']}: `{ln['text']}`")
    out.append(f"- Version numbers in the README license lines: "
               f"{', '.join(k['readme']['version_numbers_in_license_lines']) or 'none'}. "
               "The README names the license only as AGPL and links to `COPYING`.")
    pf = k["per_file_notices"]
    out.append(f"- Per-file notices in {pf['files_scanned']} scanned source files: "
               f"{pf['files_naming_affero_gpl']} name the Affero GPL, "
               f"{pf['files_with_either_version_3_clause']} carry the 'either version 3 ... or any later version' clause, "
               f"{pf['files_with_any_later_version_clause']} contain 'any later version'.")
    out.append("- Kaia UI statements that name the license:")
    for st in k["kaia_license_statements"]:
        out.append(f"  - `{st['path']}` line {st['line']}: `{st['text']}`")
    out.append("")
    out.append("**Open question (not resolved here):** whether KOReader is AGPL-3.0-only or AGPL-3.0-or-later. "
               "`docs/kaia/ENGINEERING_PLAN.md` section 11 asks for an exact SPDX expression checked against upstream "
               "and per-file notices. Discussion: " + KOREADER_ISSUE + ". "
               "This inventory asserts no `-only` or `-or-later` suffix.")
    out.append("")

    out.append("## Native dependencies (`base/thirdparty`)")
    out.append("")
    rows = []
    for e in inv["thirdparty"]:
        dl = e["downloads"][0] if e["downloads"] else {}
        rows.append([
            e["name"],
            e["version"],
            e["source"],
            dl.get("declared_ref"),
            (dl.get("urls") or [None])[0],
            e["patch_files_in_directory"],
            e["license_status"],
            e["license"],
            e["upstream_license_to_verify"],
            e["needs_review"],
        ])
    out.append(md_table(["Name", "Version", "Source", "Declared ref (MD5 or git revision)", "URL or remote",
                         "Patches in directory", "Repo license status", "License in repo",
                         "upstream license (to verify)", "Needs review"], rows))
    out.append("")
    out.append("The `Patches in directory` column lists the `.patch` files in each directory.")
    out.append("")
    unref = [(e["name"], p) for e in inv["thirdparty"] for p in e["patch_files_not_referenced_in_cmake"]]
    if unref:
        out.append("Patch files that the CMake file does not reference (check whether they are still applied):")
        out.append("")
        for name, p in unref:
            out.append(f"- `{name}`: `{p}`")
    else:
        out.append("Every patch file in a directory is referenced by that directory's CMake file.")
    out.append("")

    out.append("## Test-only Lua rocks (`base/thirdparty/spec`)")
    out.append("")
    rows = []
    for e in inv["test_rocks"]:
        dl = e["downloads"][0] if e["downloads"] else {}
        rows.append([e["name"], e["version"], dl.get("declared_ref"), (dl.get("urls") or [None])[0],
                     e["license_status"], e["upstream_license_to_verify"], e["needs_review"]])
    out.append(md_table(["Name", "Version", "MD5", "URL", "Repo license status",
                         "upstream license (to verify)", "Needs review"], rows))
    out.append("")

    out.append("## Git submodules")
    out.append("")
    rows = []
    for e in inv["submodules"]:
        rows.append([e["path"], e["commit"], e["state"], e["url"], e["shallow"], e["license_status"],
                     e["needs_review"]])
    out.append(md_table(["Path", "Commit", "State", "URL", "Shallow", "License status", "Needs review"], rows))
    out.append("")

    out.append("## Fonts (`resources/fonts`)")
    out.append("")
    rows = []
    for f in inv["fonts"]["families"]:
        rows.append([f["name"], f["font_files"], f["font_extensions"], f["license_status"], f["license"],
                     f["upstream_license_to_verify"], f["needs_review"]])
    out.append(md_table(["Family", "Font files", "Extensions", "License status", "License in repo",
                         "upstream license (to verify)", "Needs review"], rows))
    out.append("")
    if inv["fonts"]["loose_files"]:
        out.append("Loose files in `resources/fonts`: " + ", ".join(f"`{f}`" for f in inv["fonts"]["loose_files"]))
        out.append("")

    out.append("## Icons and images (`resources/`, excluding fonts)")
    out.append("")
    rows = [[r["directory"], r["image_files"], r["other_files"], r["license_files"],
             r["covered_by_license_file_in_directory_or_parent"], r["needs_review"]] for r in inv["images"]]
    out.append(md_table(["Directory", "Image files", "Other files", "License or attribution files",
                         "Covered by a license file", "Needs review"], rows))
    out.append("")

    out.append("## Lua code outside frontend")
    out.append("")
    rows = [[r["directory"], r["lua_files"], r["license_files"], r["license"], r["needs_review"]]
            for r in inv["lua_outside_frontend"]]
    out.append(md_table(["Directory", "Lua files", "License files", "License", "Needs review"], rows))
    out.append("")

    out.append("## License files in the repo")
    out.append("")
    out.append("Excludes `build` directories and `.git`. Code-like files such as `license.lua` are not counted.")
    out.append("")
    rows = [[r["path"], r["sha256"][:16], r["hint"]] for r in inv["license_files"]]
    out.append(md_table(["Path", "sha256 (first 16)", "Text hint"], rows))
    out.append("")

    out.append("## Bundled asset scan")
    out.append("")
    out.append("Counts of fonts, books, dictionaries, archives and packages by top-level directory. "
               "Test data (`test/`, `spec/`, `base/spec/`) is counted but not listed one file at a time.")
    out.append("")
    out.append(md_table(["Top directory", "Group", "Files"],
                        [[r["top_directory"], r["group"], r["files"]] for r in inv["asset_counts"]]))
    out.append("")
    if inv["asset_flags"]:
        out.append("Flagged paths outside `test/`, fonts and `base/thirdparty`:")
        out.append("")
        for a in inv["asset_flags"]:
            out.append(f"- `{a['path']}` ({a['group']})")
    else:
        out.append("No flagged paths outside `test/`, fonts and `base/thirdparty`.")
    out.append("")

    out.append("## Plugins")
    out.append("")
    kaia = [p for p in inv["plugins"] if p["kaia"]]
    out.append(f"{len(inv['plugins'])} `plugins/*.koplugin` directories. "
               f"Kaia plugins: {', '.join(p['name'] for p in kaia) or 'none'}. "
               "Upstream plugins inherit the repository license from `COPYING`. "
               "Community plugin code needs its own provenance record before import.")
    out.append("")

    out.append("## Needs review")
    out.append("")
    out.append(f"{len(inv['needs_review'])} items.")
    out.append("")
    for i, r in enumerate(inv["needs_review"], 1):
        out.append(f"{i}. `{r['id']}`: {r['reason']}")
    out.append("")
    return "\n".join(out)


def dump_json(obj):
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


# ---------------------------------------------------------------- keeping other sections

def split_sections(text):
    """Split markdown into (heading, lines) pairs, one per "## " line.

    The first pair has heading None and holds the lines before the first section.
    Joining the lines of every pair with a newline gives back the text.
    """
    parts = [(None, [])]
    for line in text.split("\n"):
        if line.startswith("## "):
            parts.append((line, []))
        parts[-1][1].append(line)
    return parts


def keep_other_sections(old_text, new_text):
    """Return new_text with the sections this script does not render copied from old_text.

    The preamble is always rendered. A heading that render_markdown emits belongs to
    this script, so its old copy is dropped. Every other "## " section of old_text is
    copied unchanged, right after the rendered section that came before it in old_text.
    A copied section with no rendered section before it goes right after the preamble.
    """
    new_parts = split_sections(new_text)
    rendered = {heading for heading, _ in new_parts[1:]}
    copied = {}
    anchor = None
    for heading, lines in split_sections(old_text)[1:]:
        if heading in rendered:
            anchor = heading
        else:
            copied.setdefault(anchor, []).extend(lines)
    out = list(new_parts[0][1])
    out += copied.pop(None, [])
    for heading, lines in new_parts[1:]:
        out += lines
        out += copied.pop(heading, [])
    return "\n".join(out)


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--root", default=".", help="repository root (default: current directory)")
    ap.add_argument("--out", default="docs/kaia/inventory", help="output directory")
    args = ap.parse_args(argv)

    root = os.path.abspath(args.root)
    if not os.path.isdir(os.path.join(root, THIRDPARTY)):
        print(f"error: {THIRDPARTY} not found under {root}", file=sys.stderr)
        return 2
    out_dir = args.out if os.path.isabs(args.out) else os.path.join(root, args.out)
    os.makedirs(out_dir, exist_ok=True)

    inv = build_inventory(root)
    md_path = os.path.join(out_dir, "LICENSES.md")
    old_md = ""
    if os.path.isfile(md_path):
        with open(md_path, encoding="utf-8") as fh:
            old_md = fh.read()
    with open(os.path.join(out_dir, "licenses.json"), "w", encoding="utf-8", newline="\n") as fh:
        fh.write(dump_json(inv))
    with open(md_path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(keep_other_sections(old_md, render_markdown(inv) + "\n"))

    s = inv["summary"]
    print(f"thirdparty entries: {s['thirdparty_entries']} (download: {s['thirdparty_with_download']}, "
          f"vendored: {s['thirdparty_vendored_in_tree']})")
    print(f"test rocks: {s['test_rocks']}, submodules: {s['submodules']}, font families: {s['font_families']} "
          f"({s['font_files']} font files), image directories: {s['image_directories']} "
          f"({s['image_files']} image files)")
    print(f"license files in repo: {s['license_files_in_repo']}, Lua directories outside frontend: "
          f"{s['lua_directories_outside_frontend']}, plugins: {s['plugins']}, asset flags: {s['asset_flags']}")
    print(f"needs review: {s['needs_review']}")
    print(f"wrote {os.path.join(out_dir, 'licenses.json')}")
    print(f"wrote {os.path.join(out_dir, 'LICENSES.md')}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
