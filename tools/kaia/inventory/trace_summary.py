#!/usr/bin/env python3
"""Summarize a Kaia runtime trace as a Markdown report.

Usage:
    python3 -I tools/kaia/inventory/trace_summary.py trace.jsonl > summary.md

The input is the JSONL file written by frontend/kaia/trace.lua. Each line has
"t", "kind", "plugin" and "detail". The trace holds no book text, file names
or setting values. Standard library only.
"""

import collections
import json
import sys


def load(path):
    """Return (records, skipped_line_count). Malformed lines are skipped."""
    records = []
    skipped = 0
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except ValueError:
                skipped += 1
                continue
            if not isinstance(record, dict) or not isinstance(record.get("kind"), str):
                skipped += 1
                continue
            if not isinstance(record.get("detail"), dict):
                record["detail"] = {}
            records.append(record)
    return records, skipped


def new_stats():
    return {
        "modules": set(),
        "widgets": collections.Counter(),
        "timers": collections.Counter(),
        "events": collections.Counter(),
        "settings": collections.Counter(),
        "menus": collections.Counter(),
        "files": collections.Counter(),
    }


def add(stats, record):
    kind = record["kind"]
    detail = record["detail"]
    if kind == "require":
        stats["modules"].add(str(detail.get("module", "other")))
    elif kind == "widget_show":
        stats["widgets"][str(detail.get("widget", "other"))] += 1
    elif kind == "timer":
        stats["timers"]["%s (delay %s)" % (detail.get("api", "other"), detail.get("delay", "other"))] += 1
    elif kind == "event":
        stats["events"][str(detail.get("handler", "other"))] += 1
    elif kind == "setting":
        stats["settings"]["%s.%s" % (detail.get("store", "settings"), detail.get("key", "other"))] += 1
    elif kind == "menu_register":
        stats["menus"][str(detail.get("menu", "other"))] += 1
    elif kind == "file_open":
        stats["files"]["%s / %s / %s" % (
            detail.get("location", "other"), detail.get("ext", "other"), detail.get("mode", "other"))] += 1


def cell(text):
    return str(text).replace("|", "\\|").replace("\n", " ")


def describe(counter):
    if not counter:
        return "none"
    items = sorted(counter.items(), key=lambda kv: (-kv[1], kv[0]))
    return ", ".join("%s (%d)" % (name, count) for name, count in items)


def report(path, records, skipped):
    per_plugin = collections.defaultdict(new_stats)
    times = []
    for record in records:
        plugin = str(record.get("plugin", "core"))
        add(per_plugin[plugin], record)
        if isinstance(record.get("t"), (int, float)):
            times.append(record["t"])

    lines = ["# Kaia runtime trace summary", ""]
    lines.append("Source: `%s`" % cell(path))
    lines.append("Records: %d. Malformed lines skipped: %d." % (len(records), skipped))
    if times:
        lines.append("Time span: %.2f s." % (max(times) - min(times)))
    lines.append("")

    lines.append("## Totals by plugin")
    lines.append("")
    lines.append("| Plugin | Modules | Widgets shown | Timers | Events | Settings keys | Menu registrations | File opens |")
    lines.append("|---|---|---|---|---|---|---|---|")
    names = sorted(per_plugin, key=lambda n: (n != "core", n))
    for name in names:
        s = per_plugin[name]
        lines.append("| %s |" % " | ".join(cell(v) for v in [
            name,
            len(s["modules"]),
            sum(s["widgets"].values()),
            sum(s["timers"].values()),
            sum(s["events"].values()),
            len(s["settings"]),
            sum(s["menus"].values()),
            sum(s["files"].values()),
        ]))
    if not names:
        lines.append("| (no records) | | | | | | | |")
    lines.append("")

    lines.append("## Details by plugin")
    for name in names:
        s = per_plugin[name]
        lines.append("")
        lines.append("### %s" % cell(name))
        lines.append("")
        lines.append("- Modules required (%d): %s" % (
            len(s["modules"]), ", ".join(sorted(s["modules"])) or "none"))
        lines.append("- Widgets shown: %s" % describe(s["widgets"]))
        lines.append("- Timers: %s" % describe(s["timers"]))
        lines.append("- Events: %s" % describe(s["events"]))
        lines.append("- Settings keys (store.key): %s" % describe(s["settings"]))
        lines.append("- Menu registrations: %s" % describe(s["menus"]))
        lines.append("- File access (location / extension / mode): %s" % describe(s["files"]))
    lines.append("")
    return "\n".join(lines)


def main(argv):
    if len(argv) != 2:
        sys.stderr.write("usage: trace_summary.py trace.jsonl\n")
        return 2
    records, skipped = load(argv[1])
    sys.stdout.write(report(argv[1], records, skipped))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
