#!/usr/bin/env python3
"""Summarizes benchmark results into Markdown.

Usage:
  python3 benches/report.py RESULTS_DIR [--subject NAME]
      a directory written by benches/run.sh (meta.txt, run-N/criterion, processes.tsv)
  python3 benches/report.py CRITERION_DIR [CRITERION_DIR...] [--subject NAME]
      one or more criterion output directories, each one run

For every benchmark and implementation: criterion's median and its confidence interval (from
estimates.json). With several runs the point is the median of the run medians and the interval
spans every run's interval, so it covers the run-to-run spread too.

The subject (default roaring128) is compared with wrapped64, treemap, the best ordered baseline
(btreeset or sortedvec, whichever is faster for that benchmark) and the best hash set (SipHash or
FxHash). Speedup = baseline time / subject time. Overlapping intervals are a tie; only
non-overlapping intervals count as a win or a loss. Results that hold by construction (an O(1)
cached value against an O(n) recomputation, a positional index) are marked and left out of the
geometric means.
"""

import argparse
import json
import math
import os
import statistics
import sys

IMPLS = ["roaring128", "wrapped64", "btreeset", "sortedvec", "hashset", "hashset_fx", "treemap"]
ORDER = [
    "insert", "from_iter", "from_sorted", "contains_hit", "contains_miss", "remove_all", "iter",
    "range", "len", "rank", "select", "serialize", "deserialize",
    "and", "or", "sub", "xor", "and_assign", "or_assign", "sub_assign", "xor_assign",
    "intersection_len", "is_subset", "multi_union16", "multi_intersection16",
    "insert_uuid", "contains_uuid",
]
# Comparison columns: label -> candidate baselines (the fastest present one is used).
COMPARISONS = [
    ("wrapped64", ["wrapped64"]),
    ("treemap", ["treemap"]),
    ("best ordered", ["btreeset", "sortedvec"]),
    ("best hash", ["hashset", "hashset_fx"]),
]
SHORT = {"btreeset": "btree", "sortedvec": "vec", "hashset": "sip", "hashset_fx": "fx"}

_CARDINALITY = ("cached cardinality: O(1) against summing the per-key cardinalities of every "
                "outer key (O(keys))")
_RANK = ("the baseline sums the cardinality of every key below the value (O(keys)); roaring128 "
         "walks only the shorter side using its cached total, a sorted Vec binary-searches")
# (operation, baseline) -> why the result holds by construction.
BY_CONSTRUCTION = {
    ("len", "wrapped64"): _CARDINALITY,
    ("len", "treemap"): _CARDINALITY,
    ("rank", "wrapped64"): _RANK,
    ("select", "wrapped64"): _RANK,
    ("rank", "treemap"): _RANK,
    ("select", "treemap"): _RANK,
    ("rank", "sortedvec"): "a sorted Vec ranks by binary search (O(log n)) without summing",
    ("select", "sortedvec"): "a sorted Vec selects by index (O(1))",
}


class Stat:
    def __init__(self, runs):
        points = [r[0] for r in runs]
        self.runs = len(runs)
        self.point = statistics.median(points)
        self.lo = min(r[1] for r in runs)
        self.hi = max(r[2] for r in runs)
        self.spread = (max(points) - min(points)) / self.point if len(runs) > 1 else None

    def halfwidth(self):
        return max(self.point - self.lo, self.hi - self.point) / self.point


def criterion_dirs(paths):
    """Expands results directories into their run-N/criterion directories."""
    dirs = []
    for p in paths:
        runs = sorted(
            (d for d in os.listdir(p) if d.startswith("run-") and d[4:].isdigit()),
            key=lambda d: int(d[4:]),
        ) if os.path.isdir(p) else []
        if runs:
            dirs += [os.path.join(p, r, "criterion") for r in runs]
        else:
            dirs.append(p)
    return dirs


def load_run(root):
    """(group, workload, impl) -> (median ns, CI lower, CI upper) of one criterion directory."""
    out = {}
    for dirpath, _, filenames in os.walk(root):
        if os.path.basename(dirpath) != "new" or "estimates.json" not in filenames:
            continue
        with open(os.path.join(dirpath, "benchmark.json")) as f:
            bench = json.load(f)
        with open(os.path.join(dirpath, "estimates.json")) as f:
            median = json.load(f)["median"]
        impl, workload = bench["function_id"], bench["value_str"]
        if impl not in IMPLS or workload is None:
            continue
        ci = median["confidence_interval"]
        out[(bench["group_id"], workload, impl)] = (
            median["point_estimate"], ci["lower_bound"], ci["upper_bound"])
    return out


def fmt_time(ns):
    for unit, scale in (("s", 1e9), ("ms", 1e6), ("µs", 1e3)):
        if ns >= scale:
            return f"{ns / scale:.3g} {unit}"
    return f"{ns:.3g} ns"


def fmt_ratio(r):
    return f"{r:.2f}x" if r < 10 else f"{r:.1f}x" if r < 100 else f"{r:.0f}x"


def verdict(s, b):
    """win / tie / loss of subject stat `s` against baseline stat `b` (lower time is better)."""
    if s.hi < b.lo:
        return "win"
    if s.lo > b.hi:
        return "loss"
    return "tie"


def cell(ratio, outcome, construction, chosen=None):
    text = fmt_ratio(ratio)
    if outcome == "tie":
        text = "≈" + text
    elif outcome == "loss":
        text = f"**{text}**"
    if chosen:
        text += f" ({SHORT.get(chosen, chosen)})"
    return text + (" †" if construction else "")


def gmean(xs):
    return math.exp(sum(math.log(x) for x in xs) / len(xs)) if xs else None


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("paths", nargs="*", default=["target/criterion"])
    ap.add_argument("--subject", default="roaring128")
    args = ap.parse_args()
    subject = args.subject

    dirs = criterion_dirs(args.paths)
    runs = [load_run(d) for d in dirs]
    runs = [r for r in runs if r]
    if not runs:
        sys.exit(f"no criterion results under {', '.join(dirs)}")
    keys = set().union(*runs)
    stats = {k: Stat([r[k] for r in runs if k in r]) for k in keys}
    n_runs = len(runs)

    results_dir = args.paths[0] if len(args.paths) == 1 and os.path.isdir(args.paths[0]) else None
    meta = os.path.join(results_dir, "meta.txt") if results_dir else None
    print("# roaring128 benchmark report\n")
    if meta and os.path.exists(meta):
        print("## Environment\n\n```")
        with open(meta) as f:
            print(f.read().rstrip())
        print("```\n")
    print(f"{n_runs} run(s). Times are criterion medians ± the half-width of the confidence interval"
          + (" spanning all runs" if n_runs > 1 else "") + ".")
    print(f"`vs X` = X time / {subject} time (> 1: {subject} faster). `≈`: the intervals overlap "
          "(a tie); **bold**: a loss with non-overlapping intervals; † holds by construction (see "
          "below, not in the geometric means). Best-of columns name the baseline used: "
          + ", ".join(f"{v} = {k}" for k, v in SHORT.items()) + ".\n")

    groups = sorted({k[0] for k in keys}, key=lambda g: (ORDER.index(g) if g in ORDER else 99, g))
    tally = {label: {"win": 0, "tie": 0, "loss": 0, "construction": 0} for label, _ in COMPARISONS}
    ratios = {label: [] for label, _ in COMPARISONS}
    group_ratios = {}
    losses, constructed = [], []

    for group in groups:
        rows = sorted({k[1] for k in keys if k[0] == group})
        present = [i for i in IMPLS if any((group, w, i) in stats for w in rows)]
        comps = [(label, [c for c in cands if c != subject and c in present])
                 for label, cands in COMPARISONS]
        comps = [(label, cands) for label, cands in comps if cands] if subject in present else []
        print(f"### {group}\n")
        header = ["workload"] + present + [f"vs {label}" for label, _ in comps]
        print("| " + " | ".join(header) + " |")
        print("|---|" + "--:|" * (len(header) - 1))
        for w in rows:
            line = [w]
            for impl in present:
                st = stats.get((group, w, impl))
                line.append(f"{fmt_time(st.point)} ±{st.halfwidth() * 100:.1f}%" if st else "")
            s = stats.get((group, w, subject))
            for label, cands in comps:
                avail = [c for c in cands if (group, w, c) in stats]
                if not s or not avail:
                    line.append("")
                    continue
                best = min(avail, key=lambda c: stats[(group, w, c)].point)
                b = stats[(group, w, best)]
                ratio = b.point / s.point
                outcome = verdict(s, b)
                why = BY_CONSTRUCTION.get((group, best))
                line.append(cell(ratio, outcome, why, best if len(cands) > 1 else None))
                if why:
                    tally[label]["construction"] += 1
                    constructed.append((group, w, best, ratio, why))
                    continue
                tally[label][outcome] += 1
                ratios[label].append(ratio)
                group_ratios.setdefault((group, label), []).append(ratio)
                if outcome == "loss":
                    losses.append((ratio, group, w, best, s.point, b.point))
            print("| " + " | ".join(line) + " |")
        print()

    if not any(t["win"] + t["tie"] + t["loss"] + t["construction"] for t in tally.values()):
        print(f"No {subject} results: comparisons skipped.\n")
    else:
        labels = [label for label, _ in COMPARISONS if sum(tally[label].values())]
        print(f"## Summary for {subject}\n")
        print("| vs | wins | ties | losses | by construction (excluded) | geometric-mean speedup |")
        print("|---|--:|--:|--:|--:|--:|")
        for label in labels:
            t, g = tally[label], gmean(ratios[label])
            print(f"| {label} | {t['win']} | {t['tie']} | {t['loss']} | {t['construction']} | "
                  f"{fmt_ratio(g) if g else ''} |")
        print()
        print("Geometric-mean speedup per operation (ties included, † excluded):\n")
        print("| operation | " + " | ".join(f"vs {label}" for label in labels) + " |")
        print("|---|" + "--:|" * len(labels))
        for group in groups:
            cells = [gmean(group_ratios.get((group, label), [])) for label in labels]
            if any(cells):
                print(f"| {group} | " + " | ".join(fmt_ratio(c) if c else "" for c in cells) + " |")
        print()
        print(f"### Losses: {subject} slower, intervals not overlapping ({len(losses)})\n")
        if losses:
            print(f"| operation | workload | baseline | {subject} | baseline time | speedup |")
            print("|---|---|---|--:|--:|--:|")
            for ratio, group, w, b, sp, bp in sorted(losses):
                print(f"| {group} | {w} | {b} | {fmt_time(sp)} | {fmt_time(bp)} | {fmt_ratio(ratio)} |")
        else:
            print("None.")
        print()
        print(f"### By construction, not in the geometric means ({len(constructed)})\n")
        if constructed:
            print("| operation | workload | baseline | speedup | why |")
            print("|---|---|---|--:|---|")
            for group, w, b, ratio, why in sorted(constructed):
                print(f"| {group} | {w} | {b} | {fmt_ratio(ratio)} | {why} |")
        else:
            print("None.")
        print()

    if n_runs > 1:
        print(f"## Run-to-run spread ({n_runs} runs)\n")
        print("Spread = (slowest − fastest run median) / median of the run medians.\n")
        print("| implementation | benchmarks | median spread | p90 spread | max spread |")
        print("|---|--:|--:|--:|--:|")
        for impl in IMPLS:
            sp = sorted(st.spread for (g, w, i), st in stats.items()
                        if i == impl and st.spread is not None)
            if sp:
                p90 = sp[min(len(sp) - 1, int(round(0.9 * (len(sp) - 1))))]
                print(f"| {impl} | {len(sp)} | {statistics.median(sp) * 100:.1f}% | "
                      f"{p90 * 100:.1f}% | {sp[-1] * 100:.1f}% |")
        print()
        worst = sorted(((st.spread, k) for k, st in stats.items() if st.spread is not None),
                       reverse=True)[:12]
        print("Most variable benchmarks:\n")
        print("| benchmark | spread | median |")
        print("|---|--:|--:|")
        for spread, (g, w, i) in worst:
            print(f"| {g}/{i}/{w} | {spread * 100:.1f}% | {fmt_time(stats[(g, w, i)].point)} |")
        print()

    procs = os.path.join(results_dir, "processes.tsv") if results_dir else None
    if procs and os.path.exists(procs):
        rows = {}
        with open(procs) as f:
            for line in f:
                parts = line.rstrip("\n").split("\t")
                if len(parts) != 6 or parts[0] == "run":
                    continue
                _, bench, impl, wall, rss, rc = parts
                r = rows.setdefault((bench, impl), [0, 0.0, 0.0, 0])
                r[0] += 1
                r[1] += float(wall)
                r[2] = max(r[2], float(rss))
                r[3] += int(rc != "0")
        print("## Processes\n")
        print("| binary | implementation | processes | total wall time | peak RSS | failed |")
        print("|---|---|--:|--:|--:|--:|")
        for (bench, impl), (n, wall, rss, failed) in sorted(rows.items()):
            print(f"| {bench} | {impl} | {n} | {wall:.0f} s | {rss:.0f} MiB | {failed} |")
        print()
    if results_dir and os.path.exists(os.path.join(results_dir, "memory.md")):
        print("Memory footprint: see memory.md.")


if __name__ == "__main__":
    main()
