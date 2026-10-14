#!/usr/bin/env python3
"""Summarize the Rust page turn CSV from page_turn_bench into Markdown.

Usage: rust_page_turn_summary.py PAGE_TURNS_CSV --meta RUN_TXT --out SUMMARY_MD

Only measured rows count (phase "measured"). Warm-up rows are left out.
Percentiles are nearest-rank over the turns of each group. p50 is the median.
Stdlib only.
"""

import argparse
import csv
import math
import sys
from collections import OrderedDict

LIMIT_MS = 100.0
STAGES = [
    ("dispatch_ms", "command call"),
    ("wait_ms", "wait: reader job and pump tick"),
    ("paint_ms", "paint: page update, repaint and present"),
    ("engine_go_to_ms", "engine: go to"),
    ("engine_render_ms", "engine: render"),
    ("engine_save_ms", "engine: save state"),
    ("engine_marks_ms", "engine: marks"),
    ("engine_bookmark_ms", "engine: bookmark"),
    ("engine_transform_ms", "engine: page transform"),
    ("engine_links_ms", "engine: links"),
    ("engine_store_ms", "engine: sticker and ink store reads"),
    ("engine_ink_anchor_ms", "engine: ink anchor of the page"),
    ("engine_total_ms", "engine: replayed total"),
    ("wait_residual_ms", "wait residual (pump tick and unreplayed job work)"),
]


def nearest_rank(sorted_values, p):
    if not sorted_values:
        return float("nan")
    rank = max(1, min(len(sorted_values), math.ceil(p * len(sorted_values))))
    return sorted_values[rank - 1]


def stats(rows, column):
    values = sorted(float(r[column]) for r in rows)
    return {
        "n": len(values),
        "p50": nearest_rank(values, 0.5),
        "p95": nearest_rank(values, 0.95),
        "max": values[-1] if values else float("nan"),
    }


def fmt(v):
    return "n/a" if v != v else f"{v:.1f}"


def read_meta(path):
    with open(path, encoding="utf-8") as f:
        return f.read().rstrip("\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("csv_path")
    parser.add_argument("--meta", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    with open(args.csv_path, newline="", encoding="utf-8") as f:
        rows = [r for r in csv.DictReader(f) if r["phase"] == "measured"]
    if not rows:
        print("no measured rows", file=sys.stderr)
        return 1

    groups = OrderedDict()
    for r in rows:
        key = (r["book"], r["format"], r["config"], r["direction"])
        groups.setdefault(key, []).append(r)

    out = []
    out.append("# Rust app page turns, desktop release build")
    out.append("")
    out.append(f"Exit check (REWRITE_PLAN.md R1): page turn under {LIMIT_MS:.0f} ms. "
               "Times are end to end, from the page turn command to the first frame the "
               "headless display receives for the new page.")
    out.append("")
    out.append("Run conditions are in the metadata below. Raw turns are in `page-turns.csv` in the same folder.")
    out.append("")
    out.append("## Per book and direction (end to end, ms)")
    out.append("")
    out.append("| Book | Format | Config | Direction | Turns | p50 | p95 | Max | Under 100 ms (p95) |")
    out.append("|---|---|---|---|---:|---:|---:|---:|---|")
    for (book, fmt_name, config, direction), group in groups.items():
        s = stats(group, "end_to_end_ms")
        verdict = "yes" if s["p95"] < LIMIT_MS else "no"
        out.append(f"| {book} | {fmt_name} | {config} | {direction} | {s['n']} | "
                   f"{fmt(s['p50'])} | {fmt(s['p95'])} | {fmt(s['max'])} | {verdict} |")
    out.append("")

    short = []
    for (book, fmt_name, config, direction), group in groups.items():
        planned = 50 if direction == "forward" else 20
        if len(group) < planned:
            short.append(f"{book} ({config}, {direction}): {len(group)} of {planned} measured turns")
    if short:
        out.append("Books with fewer measured turns than the full plan (too few pages):")
        out.append("")
        for line in short:
            out.append(f"- {line}")
        out.append("")

    out.append("## Overall (pooled over all books, ms)")
    out.append("")
    out.append("| Config | Direction | Turns | p50 | p95 | Max | Under 100 ms (p95) |")
    out.append("|---|---|---:|---:|---:|---:|---|")
    configs = OrderedDict()
    for r in rows:
        configs.setdefault((r["config"], r["direction"]), []).append(r)
    configs[("all", "all")] = rows
    for (config, direction), group in configs.items():
        s = stats(group, "end_to_end_ms")
        verdict = "yes" if s["p95"] < LIMIT_MS else "no"
        out.append(f"| {config} | {direction} | {s['n']} | {fmt(s['p50'])} | {fmt(s['p95'])} | "
                   f"{fmt(s['max'])} | {verdict} |")
    out.append("")

    out.append("## Stage medians (default layout, all books, ms)")
    out.append("")
    default_rows = [r for r in rows if r["config"] == "default"]
    out.append("| Stage | p50 | p95 | Max |")
    out.append("|---|---:|---:|---:|")
    for column, label in STAGES:
        s = stats(default_rows, column) if default_rows else None
        if s:
            out.append(f"| {label} | {fmt(s['p50'])} | {fmt(s['p95'])} | {fmt(s['max'])} |")
    out.append("")

    out.append("## Stage medians per book (default layout, p50, ms)")
    out.append("")
    per_book_cols = [
        ("wait_ms", "wait"),
        ("paint_ms", "paint"),
        ("engine_render_ms", "engine render"),
        ("engine_save_ms", "engine save"),
        ("engine_ink_anchor_ms", "engine ink anchor"),
        ("engine_total_ms", "engine total"),
    ]
    out.append("| Book | Turns | " + " | ".join(label for _, label in per_book_cols) + " |")
    out.append("|---|---:|" + "---:|" * len(per_book_cols))
    books = OrderedDict()
    for r in default_rows:
        books.setdefault((r["book"], r["format"]), []).append(r)
    for (book, fmt_name), group in books.items():
        cells = [fmt(stats(group, column)["p50"]) for column, _ in per_book_cols]
        out.append(f"| {book} ({fmt_name}) | {len(group)} | " + " | ".join(cells) + " |")
    out.append("")

    refresh_none = sum(1 for r in rows if r["refreshes"] == "0")
    out.append("## Checks on the run")
    out.append("")
    out.append(f"- Turns that presented no frame: {refresh_none} (expected 0).")
    out.append(f"- Turns that changed under 1 percent of the sampled frame: "
               f"{sum(1 for r in rows if float(r['changed_share']) < 0.01)}.")
    out.append(f"- Turns whose engine render was a page cache hit: "
               f"{sum(1 for r in rows if r['render_cache_hit'] == 'true')} of {len(rows)}.")
    out.append("")

    out.append("## Metadata")
    out.append("")
    out.append("```text")
    out.append(read_meta(args.meta))
    out.append("```")
    out.append("")

    with open(args.out, "w", encoding="utf-8") as f:
        f.write("\n".join(out))
    print(f"wrote {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
