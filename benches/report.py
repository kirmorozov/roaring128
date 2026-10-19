#!/usr/bin/env python3
"""Summarizes criterion results (target/criterion) into Markdown tables.

Usage: python3 benches/report.py [CRITERION_DIR] > report.md

One table per operation: median time of each implementation per workload, the time per
element, and the speedup of roaring128 over each baseline (baseline time / roaring128 time,
so > 1 means roaring128 is faster). Ends with geometric-mean speedups and the list of
measurements where roaring128 is slower than at least one baseline.
"""

import json
import math
import os
import sys

IMPLS = ["roaring128", "wrapped64", "btreeset", "hashset", "treemap"]
BASELINES = IMPLS[1:]
ORDER = [
    "insert", "from_sorted", "contains_hit", "contains_miss", "remove_all", "iter", "range",
    "len", "rank", "select", "serialize", "deserialize",
    "and", "or", "sub", "xor", "and_assign", "or_assign", "sub_assign", "xor_assign",
    "intersection_len", "is_subset", "multi_union16", "multi_intersection16",
    "insert_uuid", "contains_uuid",
]


def fmt_time(ns):
    for unit, scale in (("s", 1e9), ("ms", 1e6), ("µs", 1e3)):
        if ns >= scale:
            return f"{ns / scale:.3g} {unit}"
    return f"{ns:.3g} ns"


def fmt_ratio(r):
    if r is None:
        return ""
    s = f"{r:.2f}x" if r < 10 else f"{r:.1f}x" if r < 100 else f"{r:.0f}x"
    return f"**{s}**" if r < 1 else s


def load(root):
    results = {}  # (group, workload) -> impl -> (median ns, elements)
    for dirpath, dirnames, filenames in os.walk(root):
        if os.path.basename(dirpath) != "new" or "estimates.json" not in filenames:
            continue
        with open(os.path.join(dirpath, "benchmark.json")) as f:
            bench = json.load(f)
        with open(os.path.join(dirpath, "estimates.json")) as f:
            est = json.load(f)
        group, impl, workload = bench["group_id"], bench["function_id"], bench["value_str"]
        if impl not in IMPLS or workload is None:
            continue
        elements = (bench.get("throughput") or {}).get("Elements")
        results.setdefault((group, workload), {})[impl] = (est["median"]["point_estimate"], elements)
    return results


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "target/criterion"
    results = load(root)
    if not results:
        sys.exit(f"no criterion results under {root}")
    groups = sorted({g for g, _ in results}, key=lambda g: (ORDER.index(g) if g in ORDER else 99, g))
    ratios = {b: [] for b in BASELINES}
    group_ratios = {}
    losses = []

    print("Median times; `vs X` = X time / roaring128 time (> 1: roaring128 faster, **bold**: slower).\n")
    for group in groups:
        rows = sorted(w for g, w in results if g == group)
        present = [i for i in IMPLS if any(i in results[(group, w)] for w in rows)]
        base_cols = [b for b in present if b != "roaring128"]
        print(f"### {group}\n")
        header = ["workload"] + present + ([f"vs {b}" for b in base_cols] if "roaring128" in present else [])
        print("| " + " | ".join(header) + " |")
        print("|" + "---|" + "--:|" * (len(header) - 1))
        for w in rows:
            cells = results[(group, w)]
            line = [w]
            for impl in present:
                if impl in cells:
                    ns, el = cells[impl]
                    per = f" ({ns / el:.3g} ns/el)" if el and el > 1 else ""
                    line.append(fmt_time(ns) + per)
                else:
                    line.append("")
            if "roaring128" in present:
                r = cells.get("roaring128")
                for b in base_cols:
                    if r and b in cells:
                        ratio = cells[b][0] / r[0]
                        ratios[b].append(ratio)
                        group_ratios.setdefault((group, b), []).append(ratio)
                        if ratio < 1:
                            losses.append((ratio, group, w, b, r[0], cells[b][0]))
                        line.append(fmt_ratio(ratio))
                    else:
                        line.append("")
            print("| " + " | ".join(line) + " |")
        print()

    def gmean(xs):
        return math.exp(sum(math.log(x) for x in xs) / len(xs)) if xs else None

    if any(ratios.values()):
        print("### Geometric-mean speedup of roaring128\n")
        bases = [b for b in BASELINES if ratios[b]]
        print("| operation | " + " | ".join(f"vs {b}" for b in bases) + " |")
        print("|---|" + "--:|" * len(bases))
        for group in groups:
            cells = [fmt_ratio(gmean(group_ratios.get((group, b), []))) for b in bases]
            if any(cells):
                print(f"| {group} | " + " | ".join(cells) + " |")
        print("| **all** | " + " | ".join(fmt_ratio(gmean(ratios[b])) for b in bases) + " |")
        print()
        print(f"### Where roaring128 is slower than a baseline ({len(losses)} measurements)\n")
        if losses:
            print("| operation | workload | baseline | roaring128 | baseline time | speedup |")
            print("|---|---|---|--:|--:|--:|")
            for ratio, group, w, b, r, t in sorted(losses):
                print(f"| {group} | {w} | {b} | {fmt_time(r)} | {fmt_time(t)} | {fmt_ratio(ratio)} |")
        else:
            print("None.")


if __name__ == "__main__":
    main()
