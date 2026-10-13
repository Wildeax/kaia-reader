#!/usr/bin/env python3
"""Aggregate KR-006 workload results into a Markdown report.

Usage: summarize.py RESULTS_DIR [--out FILE] [--latency-threshold PCT] [--memory-threshold PCT]

Reads every *.json written by run.sh in RESULTS_DIR. Results are grouped by book
and mode. Stdlib only.

Statistics, per metric and mode:
- A list metric (page turns, highlights) gives each run its median and p95.
  The table shows the median of the run medians and the p95 pooled over all
  runs, as MEASUREMENT_METHOD.md asks. Min and max use the run medians.
- A number metric gives one value per run. The table shows the median of the
  run values. The p95 of a number metric is the nearest-rank p95 of the run
  values. With 5 runs that is the largest value.
- Spread is (max - min) / median of the run values, in percent.

Review gates (plan section 8, proposed): latency metrics (names ending in _ms)
compare p95 and flag a Kaia value more than LATENCY_PCT above the baseline.
Memory metrics (names ending in _kb) compare the median and flag more than
MEMORY_PCT above the baseline. Each flag says whether the difference falls
inside the baseline run-to-run range. Desktop noise can exceed both gates.
"""

import argparse
import glob
import json
import math
import os
import statistics
import sys

BENCH_ID = "kaia-kr006"
MODES = ("baseline", "kaia")
METRIC_ORDER = [
    "kaia_home_first_paint_ms",
    "notes_first_paint_ms",
    "cold_open_ms",
    "page_turn_ms",
    "search_first_ms",
    "search_found",
    "highlight_create_ms",
    "highlight_save_ms",
    "lua_kb",
    "lua_kb_after_gc",
    "rss_kb",
]


def percentile(values, fraction):
    """Nearest-rank percentile. fraction is in (0, 1]."""
    ordered = sorted(values)
    rank = max(1, math.ceil(fraction * len(ordered)))
    return ordered[rank - 1]


def load_runs(results_dir):
    runs = []
    for path in sorted(glob.glob(os.path.join(results_dir, "*.json"))):
        with open(path, encoding="utf-8") as handle:
            data = json.load(handle)
        if data.get("bench") == BENCH_ID:
            data["_path"] = path
            runs.append(data)
    return runs


def group_runs(runs):
    groups = {}
    for run in runs:
        key = (run.get("book", "?"), run.get("mode", "?"))
        groups.setdefault(key, []).append(run)
    for group in groups.values():
        group.sort(key=lambda r: str(r.get("run", "")))
    return groups


def aggregate(runs, metric):
    run_values = []
    pooled = []
    is_samples = False
    for run in runs:
        value = run.get("metrics", {}).get(metric)
        if value is None:
            continue
        if isinstance(value, list):
            if not value:
                continue
            is_samples = True
            pooled.extend(value)
            run_values.append(statistics.median(value))
        else:
            run_values.append(float(value))
    if not run_values:
        return None
    low, high = min(run_values), max(run_values)
    median = statistics.median(run_values)
    if is_samples:
        p95 = percentile(pooled, 0.95)
    else:
        p95 = percentile(run_values, 0.95)
    spread = (high - low) / median * 100 if median else 0.0
    return {
        "samples": is_samples,
        "runs": len(run_values),
        "pooled": len(pooled),
        "median": median,
        "p95": p95,
        "min": low,
        "max": high,
        "spread": spread,
    }


def metric_names(runs):
    seen = set()
    for run in runs:
        seen.update(run.get("metrics", {}).keys())
    ordered = [m for m in METRIC_ORDER if m in seen]
    ordered += sorted(seen - set(METRIC_ORDER))
    return ordered


def fmt(value, metric=""):
    if value is None:
        return "-"
    if metric.endswith("_kb"):
        return f"{value:,.0f}"
    if metric.endswith("_ms"):
        return f"{value:,.1f}"
    return f"{value:,.2f}"


def pct_diff(new, old):
    if old == 0:
        return None
    return (new - old) / old * 100


def check_for(metric, latency_pct, memory_pct):
    if metric.endswith("_ms"):
        return "p95", latency_pct
    if metric.endswith("_kb"):
        return "median", memory_pct
    return None, None


def compare_row(metric, base, kaia, latency_pct, memory_pct):
    stat, limit = check_for(metric, latency_pct, memory_pct)
    base_stat = base["p95"] if stat == "p95" else base["median"]
    kaia_stat = kaia["p95"] if stat == "p95" else kaia["median"]
    diff = pct_diff(kaia["median"], base["median"])
    stat_diff = pct_diff(kaia_stat, base_stat) if stat else None
    if stat is None:
        verdict = "not gated"
    elif stat_diff is not None and stat_diff > limit:
        inside = abs(kaia["median"] - base["median"]) <= (base["max"] - base["min"])
        if inside:
            verdict = f"above {limit:g}% gate, inside baseline range (noise?)"
        else:
            verdict = f"REVIEW: kaia {stat} {stat_diff:+.1f}% (gate {limit:g}%)"
    else:
        verdict = "ok"
    fewest = min(base["runs"], kaia["runs"])
    if fewest < 3 and verdict not in ("ok", "not gated"):
        verdict += f" (only {fewest} run(s): not evidence)"
    return {
        "metric": metric,
        "base_median": base["median"],
        "kaia_median": kaia["median"],
        "median_diff": diff,
        "base_p95": base["p95"],
        "kaia_p95": kaia["p95"],
        "p95_diff": pct_diff(kaia["p95"], base["p95"]),
        "stat": stat or "-",
        "verdict": verdict,
    }


def signed(value):
    return "-" if value is None else f"{value:+.1f}%"


def render(groups, latency_pct, memory_pct):
    out = []
    books = sorted({book for book, _ in groups})
    out.append("# KR-006 workload summary")
    out.append("")
    out.append(
        f"Gates: latency p95 above {latency_pct:g}% and memory above {memory_pct:g}% "
        "against the baseline mode. Proposed thresholds, not measured limits."
    )
    for book in books:
        out.append("")
        out.append(f"## {book}")
        out.append("")
        present = {mode: groups[(book, mode)] for mode in MODES if (book, mode) in groups}
        for mode, runs in present.items():
            out.append(f"- {mode}: {len(runs)} run(s), commit {runs[0].get('commit', '?')}, "
                       f"platform {runs[0].get('platform', '?')}, "
                       f"search term \"{runs[0].get('search_term', '')}\"")
        out.append("")
        names = metric_names([r for runs in present.values() for r in runs])

        out.append("| Metric | Mode | Runs | Median | p95 | Min | Max | Spread |")
        out.append("|---|---|---:|---:|---:|---:|---:|---:|")
        stats = {}
        for metric in names:
            for mode, runs in present.items():
                agg = aggregate(runs, metric)
                if agg is None:
                    continue
                stats[(metric, mode)] = agg
                label = metric + (" (p95 pooled over runs)" if agg["samples"] else "")
                out.append(
                    f"| {label} | {mode} | {agg['runs']} | {fmt(agg['median'], metric)} | "
                    f"{fmt(agg['p95'], metric)} | {fmt(agg['min'], metric)} | "
                    f"{fmt(agg['max'], metric)} | {agg['spread']:.1f}% |"
                )
        out.append("")

        if "baseline" in present and "kaia" in present:
            out.append("| Metric | Baseline median | Kaia median | Median diff | Baseline p95 | "
                       "Kaia p95 | p95 diff | Check | Result |")
            out.append("|---|---:|---:|---:|---:|---:|---:|---|---|")
            for metric in names:
                base = stats.get((metric, "baseline"))
                kaia = stats.get((metric, "kaia"))
                if not base or not kaia:
                    continue
                row = compare_row(metric, base, kaia, latency_pct, memory_pct)
                out.append(
                    f"| {metric} | {fmt(row['base_median'], metric)} | "
                    f"{fmt(row['kaia_median'], metric)} | {signed(row['median_diff'])} | "
                    f"{fmt(row['base_p95'], metric)} | {fmt(row['kaia_p95'], metric)} | "
                    f"{signed(row['p95_diff'])} | {row['stat']} | {row['verdict']} |"
                )
            out.append("")
    return "\n".join(out) + "\n"


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("results_dir")
    parser.add_argument("--out", help="write the Markdown here instead of stdout")
    parser.add_argument("--latency-threshold", type=float, default=5.0,
                        help="percent, p95 latency gate (default 5)")
    parser.add_argument("--memory-threshold", type=float, default=10.0,
                        help="percent, memory gate (default 10)")
    args = parser.parse_args(argv)

    runs = load_runs(args.results_dir)
    if not runs:
        print(f"no {BENCH_ID} results in {args.results_dir}", file=sys.stderr)
        return 1
    text = render(group_runs(runs), args.latency_threshold, args.memory_threshold)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as handle:
            handle.write(text)
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
