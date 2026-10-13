#!/usr/bin/env python3
"""Static inventory of the bundled KOReader plugins.

Run from the repository root:

    python3 -I tools/kaia/inventory/plugin_inventory.py --root . --out docs/kaia/inventory

The script reads every plugins/*.koplugin directory and frontend/pluginloader.lua.
It writes plugins.json (facts from the scan) and plugins.md (table with the
curated platform, class and workflow columns from curation.json).

The output is sorted and has no timestamps, so a rerun on the same tree
produces no diff. The scan is textual. It cannot see dynamic Lua behavior;
runtime tracing (KR-012) covers that.

Standard library only.
"""

import argparse
import bisect
import json
import os
import re
import sys

CLASSES = (
    "Applicable bundled plugin",
    "Plugin using shared widgets",
    "Custom-drawing or deeply coupled",
    "Not applicable on Kobo",
)
PLATFORMS = ("applicable", "not applicable", "debug only")
OWNER = "compatibility engineer"
META_FIELDS = ("name", "fullname", "description", "deprecated", "disabled")
NETWORK_PREFIXES = ("socket", "ssl", "ui/network/manager", "httpclient", "socketutil")
DEVICE_NAME_RE = re.compile(
    r"\b(is(?:Kobo|Kindle|Android|Cervantes|PocketBook|Remarkable|Emulator|SDL|Desktop))\b"
)
SPECIAL_RE = re.compile(r"[-\"'\[]")
LONG_OPEN_RE = re.compile(r"\[(=*)\[")
LIT_DQ_RE = re.compile(r'"((?:[^"\\\n]|\\.)*)"')
LIT_SQ_RE = re.compile(r"'((?:[^'\\\n]|\\.)*)'")
ANY_LITERAL_RE = re.compile(r"\[(=*)\[(.*?)\]\1\]|\"((?:[^\"\\\n]|\\.)*)\"|'((?:[^'\\\n]|\\.)*)'", re.S)
REQUIRE_RE = re.compile(r"(?<![\w.:])require\b\s*\(?\s*")
WRITE_METHOD_RE = re.compile(
    r"(?<!\w)([A-Za-z_][\w.]*)\s*:\s*"
    r"(saveSetting|flipSetting|flipNilOrFalse|makeTrue|makeFalse|delSetting|toggleSetting|updateSetting|flush)"
    r"\b\s*\(?\s*"
)
LUASETTINGS_OPEN_RE = re.compile(r"\bLuaSettings:open\s*\(")

# Keyword groups matched against the masked source (comments and string contents blanked).
REGISTRATION_PATTERNS = {
    "registerToMainMenu": r"\bregisterToMainMenu\b",
    "addToMainMenu": r"\baddToMainMenu\b",
    "UIManager:show": r"\bUIManager:show\s*\(",
    "UIManager:scheduleIn": r"\bUIManager:scheduleIn\s*\(",
    "UIManager:nextTick": r"\bUIManager:nextTick\s*\(",
    "Dispatcher:registerAction": r"\bDispatcher:registerAction\s*\(",
    "onDispatcherRegisterActions": r"\bonDispatcherRegisterActions\b",
}
NATIVE_PATTERNS = {
    "ffi.load": r"\bffi\.load\b",
    "ffi.cdef": r"\bffi\.cdef\b",
    "os.execute": r"\bos\.execute\b",
    "io.popen": r"\bio\.popen\b",
}
FILE_ACCESS_PATTERNS = {
    "io.open": r"\bio\.open\b",
    "lfs.": r"\blfs\.",
    "DataStorage": r"\bDataStorage\b",
    "LuaSettings:open": r"\bLuaSettings:open\b",
    "G_reader_settings": r"\bG_reader_settings\b",
    "DocSettings": r"\bDocSettings\b",
}
DOCUMENT_PATTERNS = {
    "self.ui.document": r"\bself\.ui\.document\b",
    "self.document": r"\bself\.document\b",
    "getPageText": r"\bgetPageText\b",
    "getTextFromPositions": r"\bgetTextFromPositions\b",
}
DEVICE_CALL_RE = re.compile(r"\bDevice[:.]((?:is|has)[A-Z]\w*|model|firmware_rev)\b")
DBG_CALL_RE = re.compile(r"\bdbg\.(\w+)")
# Names of the plugin's own classes or tables. Other objects that receive methods are patch sites.
OWN_DEF_RE = re.compile(r"(?<![\w.])local\s+(\w+)\s*=\s*(?:(?:require\([^)]*\)|[\w.]+):(?:extend|new)\b|\{)")
ASSIGN_PATCH_RE = re.compile(r"^\s*([A-Za-z_]\w*)[.:]\w+\s*=", re.M)
# "function Name.field(" or "function Name:field(" defines a method on Name.
PATCH_DEF_RE = re.compile(r"\bfunction\s+([A-Za-z_]\w*)\s*[.:]\s*\w+\s*\(")
BLITBUFFER_RE = re.compile(r"\bBlitbuffer\b|\bpaintTo\b")
GATING_RE = re.compile(r"\breturn\s*\{[^}]*?\bdisabled\s*=")
FILE_WRITE_RE = re.compile(r"\bio\.open\s*\(([^\n]*)")
WRITE_MODE_RE = re.compile(r"[\"'][wa]\+?b?[\"']")
OTHER_WRITE_PATTERNS = {
    "os.remove": r"\bos\.remove\b",
    "os.rename": r"\bos\.rename\b",
    "lfs.mkdir": r"\blfs\.mkdir\b",
    "lfs.rmdir": r"\blfs\.rmdir\b",
}


def blank(text):
    """Replace every character except newlines with a space."""
    return re.sub(r"[^\n]", " ", text)


def long_bracket_end(src, match):
    """Return (end, body_end, closed) for a long bracket opened by match."""
    closer = "]" + match.group(1) + "]"
    close_at = src.find(closer, match.end())
    if close_at < 0:
        return len(src), len(src), False
    return close_at + len(closer), close_at, True


def lex_lua(src):
    """Return (code, masked), both the same length as src.

    code keeps string literals and drops comments.
    masked also blanks string contents, so keyword searches never match text
    inside a string. Quote and long-bracket delimiters are kept in masked.
    """
    code = []
    masked = []
    i = 0
    n = len(src)
    while i < n:
        special = SPECIAL_RE.search(src, i)
        if special is None or special.start() > i:
            end = special.start() if special else n
            chunk = src[i:end]
            code.append(chunk)
            masked.append(chunk)
            i = end
            continue
        if src.startswith("--", i):
            m = LONG_OPEN_RE.match(src, i + 2)
            if m:
                end, _body_end, _closed = long_bracket_end(src, m)
            else:
                end = src.find("\n", i)
                end = n if end < 0 else end
            piece = blank(src[i:end])
            code.append(piece)
            masked.append(piece)
            i = end
            continue
        if src[i] == "[":
            m = LONG_OPEN_RE.match(src, i)
            if m:
                end, body_end, closed = long_bracket_end(src, m)
                code.append(src[i:end])
                closer = "]" + m.group(1) + "]" if closed else ""
                masked.append(m.group(0) + blank(src[m.end():body_end]) + closer)
                i = end
                continue
            code.append("[")
            masked.append("[")
            i += 1
            continue
        if src[i] in "\"'":
            quote = src[i]
            j = i + 1
            closed = False
            while j < n:
                ch = src[j]
                if ch == "\\":
                    j += 2
                    continue
                if ch == quote:
                    closed = True
                    break
                if ch == "\n":
                    break
                j += 1
            end = j + 1 if closed else min(j, n)
            raw = src[i:end]
            code.append(raw)
            inner = src[i + 1:(j if closed else end)]
            masked.append(quote + blank(inner) + (quote if closed else ""))
            i = end
            continue
        # A lone "-" that is not a comment marker.
        code.append(src[i])
        masked.append(src[i])
        i += 1
    return "".join(code), "".join(masked)


def line_starts(text):
    return [0] + [m.end() for m in re.finditer("\n", text)]


def line_of(starts, pos):
    return bisect.bisect_right(starts, pos)


def read_literal(code, pos):
    """Return the string value of a literal starting at pos in code, or None."""
    ch = code[pos:pos + 1]
    if ch == '"':
        m = LIT_DQ_RE.match(code, pos)
        return m.group(1) if m else None
    if ch == "'":
        m = LIT_SQ_RE.match(code, pos)
        return m.group(1) if m else None
    m = LONG_OPEN_RE.match(code, pos)
    if m:
        closer = "]" + m.group(1) + "]"
        end = code.find(closer, m.end())
        if end >= 0:
            return code[m.end():end]
    return None


def collapse(text):
    return " ".join(text.split())


def matching_paren(masked, start):
    """Return the offset of the ')' closing an open paren just before start, or -1."""
    depth = 1
    for j in range(start, len(masked)):
        ch = masked[j]
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return j
    return -1


def meta_field(code, masked, field):
    """Return (value, raw) for a field in a _meta.lua table, or (None, None)."""
    m = re.search(r"(?<![\w.])%s\s*=" % field, masked)
    if not m:
        return None, None
    start = m.end()
    depth = 0
    j = start
    while j < len(masked):
        ch = masked[j]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                break
            depth -= 1
        elif ch == "," and depth == 0:
            break
        j += 1
    expr = code[start:j]
    cleaned = re.sub(r"require\s*\(\s*([\"'])[^\"'\n]*\1\s*\)", "", expr)
    literals = []
    for lm in ANY_LITERAL_RE.finditer(cleaned):
        value = next(g for g in (lm.group(2), lm.group(3), lm.group(4)) if g is not None)
        literals.append(value)
    value = collapse(" ".join(literals)) if literals else None
    return value, collapse(expr)[:200]


def first_sentence(text):
    if not text:
        return None
    m = re.match(r"(.+?[.!?])(\s|$)", text)
    return m.group(1) if m else text


def scan_plugin(root, plugin_rel):
    plugin_dir = os.path.join(root, plugin_rel)
    lua_files = []
    for dirpath, dirnames, filenames in os.walk(plugin_dir):
        dirnames.sort()
        for filename in sorted(filenames):
            if filename.endswith(".lua"):
                full = os.path.join(dirpath, filename)
                lua_files.append(os.path.relpath(full, plugin_dir).replace(os.sep, "/"))
    lua_files.sort()

    result = {
        "name": os.path.basename(plugin_rel)[: -len(".koplugin")],
        "path": plugin_rel,
        "lua_files": lua_files,
        "lua_line_count": 0,
        "meta": {},
        "meta_raw": {},
        "main_present": os.path.isfile(os.path.join(plugin_dir, "main.lua")),
        "requires": {"external": set(), "local": set(), "dynamic": []},
        "ui": {"modules": set(), "widgets": set()},
        "registration": {k: 0 for k in REGISTRATION_PATTERNS},
        "native": {k: 0 for k in NATIVE_PATTERNS},
        "file_access": {k: 0 for k in FILE_ACCESS_PATTERNS},
        "network": {"modules": set(), "count": 0},
        "document_access": {k: 0 for k in DOCUMENT_PATTERNS},
        "device_checks": set(),
        "debug_calls": set(),
        "gating": [],
        "settings_writes": {},
        "lua_settings_files": [],
        "file_writes": {},
        "blitbuffer_refs": 0,
    }

    sources = {}
    for rel in lua_files:
        with open(os.path.join(plugin_dir, rel), encoding="utf-8", errors="replace") as fh:
            sources[rel] = fh.read()
    lexed = {rel: lex_lua(src) for rel, src in sources.items()}
    local_names = set()
    for rel in lua_files:
        local_names.update(OWN_DEF_RE.findall(lexed[rel][1]))
    patched = set()

    for rel in lua_files:
        src = sources[rel]
        code, masked = lexed[rel]
        result["lua_line_count"] += src.count("\n") + (1 if src and not src.endswith("\n") else 0)
        starts = line_starts(src)
        code_lines = code.split("\n")

        if rel == "_meta.lua":
            for field in META_FIELDS:
                value, raw = meta_field(code, masked, field)
                if raw is not None:
                    result["meta_raw"][field] = raw
                if value is not None:
                    result["meta"][field] = value

        for m in REQUIRE_RE.finditer(masked):
            pos = m.end()
            site = "%s:%d" % (rel, line_of(starts, pos))
            target = read_literal(code, pos) if pos < len(code) else None
            if target is None:
                result["requires"]["dynamic"].append(site)
                continue
            is_local = os.path.isfile(os.path.join(plugin_dir, target + ".lua")) or os.path.isdir(
                os.path.join(plugin_dir, target)
            )
            if is_local:
                result["requires"]["local"].add(target)
            else:
                result["requires"]["external"].add(target)
            if target.startswith("ui/"):
                result["ui"]["modules"].add(target)
                if target.startswith("ui/widget/"):
                    result["ui"]["widgets"].add(target)
            if any(target == p or target.startswith(p + "/") or target.startswith(p + ".")
                   for p in NETWORK_PREFIXES):
                result["network"]["modules"].add(target)
                result["network"]["count"] += 1

        for key, pattern in REGISTRATION_PATTERNS.items():
            result["registration"][key] += len(re.findall(pattern, masked))
        for key, pattern in NATIVE_PATTERNS.items():
            result["native"][key] += len(re.findall(pattern, masked))
        for key, pattern in FILE_ACCESS_PATTERNS.items():
            result["file_access"][key] += len(re.findall(pattern, masked))
        for key, pattern in DOCUMENT_PATTERNS.items():
            result["document_access"][key] += len(re.findall(pattern, masked))

        for m in DEVICE_CALL_RE.finditer(masked):
            result["device_checks"].add(m.group(1))
        for m in DEVICE_NAME_RE.finditer(masked):
            result["device_checks"].add(m.group(1))
        for m in DBG_CALL_RE.finditer(masked):
            result["debug_calls"].add(m.group(1))
        for pattern in (PATCH_DEF_RE, ASSIGN_PATCH_RE):
            for m in pattern.finditer(masked):
                name = m.group(1)
                if name[0].isupper() and name not in local_names:
                    patched.add(name)
        result["blitbuffer_refs"] += len(BLITBUFFER_RE.findall(masked))

        if rel in ("main.lua", "_meta.lua"):
            for m in GATING_RE.finditer(masked):
                line = line_of(starts, m.start())
                statement_end = code.find("}", m.start())
                statement = collapse(code[m.start(): statement_end + 1 if statement_end >= 0 else m.end()])
                condition = None
                for back in range(line - 2, max(line - 8, -1), -1):
                    text = code_lines[back].strip()
                    if text.startswith("if ") or text.startswith("elseif "):
                        condition = collapse(text)
                        break
                result["gating"].append({
                    "file": rel,
                    "line": line,
                    "statement": statement[:200],
                    "enclosing_condition": condition,
                })

        for m in WRITE_METHOD_RE.finditer(masked):
            receiver = m.group(1)
            method = m.group(2)
            if method == "flush":
                key = None
            else:
                key = read_literal(code, m.end()) if m.end() < len(code) else None
                if key is None:
                    key = "<dynamic>"
            entry = result["settings_writes"].setdefault(
                (receiver, method, key), {"receiver": receiver, "method": method, "key": key, "files": set(), "sites": 0}
            )
            entry["files"].add(rel)
            entry["sites"] += 1

        for m in LUASETTINGS_OPEN_RE.finditer(masked):
            close = matching_paren(masked, m.end())
            expr = collapse(code[m.end(): close if close >= 0 else m.end()])
            result["lua_settings_files"].append({"file": rel, "expr": expr[:160]})

        for m in FILE_WRITE_RE.finditer(code):
            if WRITE_MODE_RE.search(m.group(1)):
                result["file_writes"]["io.open (write mode)"] = result["file_writes"].get("io.open (write mode)", 0) + 1
        for key, pattern in OTHER_WRITE_PATTERNS.items():
            count = len(re.findall(pattern, masked))
            if count:
                result["file_writes"][key] = result["file_writes"].get(key, 0) + count

    result["patched_objects"] = sorted(patched)
    result["requires"]["external"] = sorted(result["requires"]["external"])
    result["requires"]["local"] = sorted(result["requires"]["local"])
    result["requires"]["dynamic"] = sorted(result["requires"]["dynamic"])
    result["requires"]["dynamic_count"] = len(result["requires"]["dynamic"])
    del result["requires"]["dynamic"]
    result["ui"]["modules"] = sorted(result["ui"]["modules"])
    result["ui"]["widgets"] = sorted(result["ui"]["widgets"])
    result["network"]["modules"] = sorted(result["network"]["modules"])
    result["device_checks"] = sorted(result["device_checks"])
    result["debug_calls"] = sorted(result["debug_calls"])
    result["gating"].sort(key=lambda g: (g["file"], g["line"]))
    writes = []
    for entry in result["settings_writes"].values():
        writes.append({
            "receiver": entry["receiver"],
            "method": entry["method"],
            "key": entry["key"],
            "files": sorted(entry["files"]),
            "sites": entry["sites"],
        })
    writes.sort(key=lambda w: (w["receiver"], w["method"], w["key"] or ""))
    result["settings_writes"] = writes
    for f in result["lua_settings_files"]:
        ident = re.fullmatch(r"(?:self\.)?([\w.]+)", f["expr"])
        f["value"] = None
        if ident:
            last = ident.group(1).split(".")[-1]
            assign = re.compile(r"(?<![\w])%s\s*=\s*([^\n]+)" % re.escape(last))
            for other in lua_files:
                hit = assign.search(lexed[other][1])
                if hit:
                    code_text = lexed[other][0]
                    start = hit.start(1)
                    value = collapse(code_text[start:start + len(hit.group(1))]).rstrip(",")
                    f["value"] = value[:160]
                    break
    result["lua_settings_files"].sort(key=lambda f: (f["file"], f["expr"]))
    result["file_writes"] = dict(sorted(result["file_writes"].items()))
    result["meta_summary"] = first_sentence(result["meta"].get("description"))
    return result


def read_builtin_plugins(root):
    path = os.path.join(root, "frontend", "pluginloader.lua")
    with open(path, encoding="utf-8") as fh:
        src = fh.read()
    m = re.search(r"local BUILTIN_PLUGINS = \{(.*?)\n\}", src, re.S)
    if not m:
        raise SystemExit("BUILTIN_PLUGINS table not found in frontend/pluginloader.lua")
    return sorted(set(re.findall(r'\["([^"]+)"\]\s*=\s*true', m.group(1))))


def load_curation(script_dir):
    path = os.path.join(script_dir, "curation.json")
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def check_curation(plugins, curation):
    errors = []
    names = {p["name"] for p in plugins}
    entries = curation["plugins"]
    for name in sorted(names - set(entries)):
        errors.append("no curation entry for plugin %s" % name)
    for name in sorted(set(entries) - names):
        errors.append("curation entry without a plugin directory: %s" % name)
    for name, entry in entries.items():
        if entry.get("class") not in CLASSES:
            errors.append("%s: class %r is not one of the plan classes" % (name, entry.get("class")))
        if entry.get("platform") not in PLATFORMS:
            errors.append("%s: platform %r is not one of %s" % (name, entry.get("platform"), PLATFORMS))
        for key in ("platform_basis", "class_basis", "workflow"):
            if not entry.get(key):
                errors.append("%s: missing %s" % (name, key))
    return errors


def dynamic_label(key):
    return "keys built at run time" if key == "<dynamic>" else "`%s`" % key


def group_writes_text(writes):
    """Summarize settings writes for the data effects column."""
    by_receiver = {}
    for w in writes:
        if w["method"] == "flush" and w["key"] is None:
            label = w["receiver"]
            by_receiver.setdefault(label, [])
            continue
        by_receiver.setdefault(w["receiver"], []).append(w["key"])
    parts = []
    for receiver in sorted(by_receiver):
        keys = sorted(set(k for k in by_receiver[receiver] if k))
        if not keys:
            continue
        named = [k for k in keys if k != "<dynamic>"]
        shown = ", ".join("`%s`" % k for k in named[:6])
        if len(named) > 6:
            shown += " and %d more" % (len(named) - 6)
        if "<dynamic>" in keys:
            shown = (shown + ", " if shown else "") + "keys built at run time"
        if receiver == "G_reader_settings":
            label = "Global settings"
        elif "doc_settings" in receiver or receiver == "DocSettings":
            label = "Per-book settings (`%s`)" % receiver
        else:
            label = "Settings object `%s`" % receiver
        parts.append("%s: %s" % (label, shown))
    return parts


def data_effects_text(plugin, curation_entry):
    parts = group_writes_text(plugin["settings_writes"])
    seen = set()
    for f in plugin["lua_settings_files"]:
        key = (f["file"], f["value"] or f["expr"])
        if key in seen:
            continue
        seen.add(key)
        shown = f["value"] or f["expr"]
        parts.append("settings file `%s` (`%s`)" % (shown, f["file"]))
    if plugin["file_writes"]:
        ops = ", ".join("`%s`" % k for k in plugin["file_writes"])
        parts.append("file operations: %s" % ops)
    text = "; ".join(parts) if parts else "none found by the scan"
    text = text[0].upper() + text[1:] if text else text
    if curation_entry["platform"] != "applicable":
        # The plugin never loads on Kobo, so its write sites do not run there.
        if parts:
            text = "Not loaded on Kobo. If loaded, it would write " + text[0].lower() + text[1:] + "."
        else:
            text = "Not loaded on Kobo."
    note = curation_entry.get("data_note")
    if note:
        text = text.rstrip(".") + ". " + note
    return text


def md_cell(text):
    return text.replace("|", "\\|").replace("\n", " ")


METHOD_TEXT = (
    "This inventory comes from a static text scan of every `.lua` file in each "
    "`plugins/*.koplugin` directory. The scan blanks Lua comments and string contents, "
    "then counts `require` targets, UI registration calls, device checks, file and "
    "network access, document calls and settings writes. Names and descriptions come "
    "from each `_meta.lua`. Platform applicability, compatibility classes and workflows "
    "come from reading each `main.lua` and the Kobo Libra Colour capability flags in "
    "`frontend/device/kobo/device.lua`. The scan cannot see dynamic Lua behavior. It "
    "misses requires built from variables, calls made through tables, code loaded by "
    "other plugins and anything that changes at runtime. Runtime tracing (KR-012) covers "
    "those cases. Treat counts as leads for review, not as proof that code is absent. "
    "The raw counts and call sites are in `plugins.json`."
)

CLASS_TEXT = (
    "Compatibility classes use these rules. Not applicable on Kobo means the plugin "
    "disables itself on Libra Colour or is a developer template. Custom-drawing or deeply "
    "coupled means the plugin draws with Blitbuffer, patches a core class or writes core "
    "state at run time. Applicable bundled plugin means the plugin uses no dialogs or "
    "lists beyond the standard container and single message or confirm dialogs. Plugin "
    "using shared widgets covers every other applicable plugin."
)

LOADER_TEXT = (
    "The loader (`frontend/pluginloader.lua`) loads every `*.koplugin` directory in "
    "`plugins/` and in `extra_plugin_paths`. No plugin is disabled by default in the "
    "loader. Three things turn a plugin off. A user can add its name to the "
    "`plugins_disabled` setting, and the plugin then appears in the plugin manager "
    "without running `main.lua`. A user can set `plugins_disable_external`, which turns "
    "off every plugin not in `BUILTIN_PLUGINS`. And `main.lua` can return "
    "`{ disabled = true }`, which makes the loader skip the plugin without an error. "
    "The gating table lists each `return { disabled = ... }` the scan found, with the "
    "nearest enclosing `if` line. Gates inside `else` branches show no condition."
)


def render_plugins_md(plugins, curation, builtin, baseline):
    lines = []
    lines.append("# Bundled plugin inventory")
    lines.append("")
    lines.append("Baseline: %s. Generated by `tools/kaia/inventory/plugin_inventory.py`. "
                 "Do not edit by hand; edit `tools/kaia/inventory/curation.json` and rerun the script."
                 % baseline)
    lines.append("")
    lines.append("## Method and limits")
    lines.append("")
    lines.append(METHOD_TEXT)
    lines.append("")
    lines.append("## Compatibility classes")
    lines.append("")
    lines.append(CLASS_TEXT)
    lines.append("")
    lines.append("## Loader and gating")
    lines.append("")
    lines.append(LOADER_TEXT)
    lines.append("")
    gated = [p for p in plugins if p["gating"]]
    lines.append("| Plugin | Gate in `main.lua` or `_meta.lua` | Enclosing condition |")
    lines.append("|---|---|---|")
    for p in gated:
        for g in p["gating"]:
            cond = g["enclosing_condition"] or "none found by the scan (the gate may sit in an else branch)"
            lines.append("| `%s.koplugin` | `%s:%d`: `%s` | %s |" % (
                p["name"], g["file"], g["line"], md_cell(g["statement"]), md_cell(cond)))
    lines.append("")
    lines.append("Bundled plugins in `BUILTIN_PLUGINS`: %d of %d."
                 % (sum(1 for p in plugins if p["name"] in builtin), len(plugins)))
    lines.append("")
    lines.append("## Plugins")
    lines.append("")
    lines.append("| Plugin | What it does | Platform applicability on Kobo Libra Colour | "
                 "Initial compatibility class | Data effects | Baseline workflow | Owner |")
    lines.append("|---|---|---|---|---|---|---|")
    for p in plugins:
        entry = curation["plugins"][p["name"]]
        summary = entry.get("summary") or p["meta_summary"] or "No description in `_meta.lua`."
        platform = "%s. %s" % (entry["platform"].capitalize(), entry["platform_basis"])
        compat = "%s. %s" % (entry["class"], entry["class_basis"])
        lines.append("| `%s.koplugin` | %s | %s | %s | %s | %s | %s |" % (
            p["name"],
            md_cell(summary),
            md_cell(platform),
            md_cell(compat),
            md_cell(data_effects_text(p, entry)),
            md_cell(entry["workflow"]),
            OWNER,
        ))
    lines.append("")
    lines.append("## Community plugins to evaluate first")
    lines.append("")
    lines.append("These come from ENGINEERING_PLAN.md. Nothing is downloaded or pinned yet. "
                 "Each needs a pinned commit, a license review (KR-007) and a workflow test before use.")
    lines.append("")
    for item in curation["community"]:
        lines.append("- `%s`, %s. Version to pin: %s." % (item["name"], item["url"], item["pin"]))
    lines.append("")
    return "\n".join(lines)


def build_json(plugins, builtin, baseline):
    records = []
    for p in plugins:
        record = dict(p)
        record["loader_builtin"] = p["name"] in builtin
        records.append(record)
    return {
        "generator": "tools/kaia/inventory/plugin_inventory.py",
        "baseline": baseline,
        "loader": {
            "file": "frontend/pluginloader.lua",
            "builtin_plugins": builtin,
        },
        "plugin_count": len(records),
        "plugins": records,
    }


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", default=".", help="repository root (default: current directory)")
    parser.add_argument("--out", default="docs/kaia/inventory", help="output directory")
    args = parser.parse_args(argv)

    root = os.path.abspath(args.root)
    out_dir = os.path.join(root, args.out) if not os.path.isabs(args.out) else args.out
    script_dir = os.path.dirname(os.path.abspath(__file__))

    plugin_dirs = sorted(
        d for d in os.listdir(os.path.join(root, "plugins"))
        if d.endswith(".koplugin") and os.path.isdir(os.path.join(root, "plugins", d))
    )
    plugins = [scan_plugin(root, os.path.join("plugins", d)) for d in plugin_dirs]
    builtin = read_builtin_plugins(root)
    curation = load_curation(script_dir)

    errors = check_curation(plugins, curation)
    if errors:
        for err in errors:
            print("error: %s" % err, file=sys.stderr)
        return 1

    baseline = curation["baseline"]
    payload = build_json(plugins, builtin, baseline)
    json_text = json.dumps(payload, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    md_text = render_plugins_md(plugins, curation, builtin, baseline)

    for text in (json_text, md_text):
        if "\u2014" in text or "\u2013" in text:
            print("error: output contains an em or en dash", file=sys.stderr)
            return 1

    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "plugins.json"), "w", encoding="utf-8") as fh:
        fh.write(json_text)
    with open(os.path.join(out_dir, "plugins.md"), "w", encoding="utf-8") as fh:
        fh.write(md_text)
    print("wrote %d plugins to %s" % (len(plugins), os.path.relpath(out_dir, root)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
