# roaring128 benchmarks

This harness compares `RoaringBitmap128` with the obvious ways to keep a set of `u128` values
(UUIDs), and reports losses as well as wins. It runs on any machine with Rust, Python 3, git and
bash: Linux on x86_64 or aarch64, or macOS. No paths, core numbers or CPU names are hard-coded.

## Implementations

| id | what it is | where it runs |
|---|---|---|
| `roaring128` | `RoaringBitmap128` | everywhere |
| `wrapped64` | `BTreeMap<u64, RoaringTreemap>` keyed by the high 64 bits: the obvious wrapper around the `roaring` crate ([common/wrapped64.rs](common/wrapped64.rs)) | 128-bit workloads |
| `btreeset` | `BTreeSet<u128>` | 128-bit workloads |
| `sortedvec` | sorted `Vec<u128>`: binary-search lookups, merges into a new `Vec`, `retain` for `&=` / `-=` ([common/sortedvec.rs](common/sortedvec.rs)) | 128-bit workloads |
| `hashset` | `HashSet<u128>` with `std`'s default SipHash-1-3 | single-set operations |
| `hashset_fx` | `HashSet<u128>` with FxHash (`rustc-hash`) | single-set operations |
| `treemap` | `roaring::RoaringTreemap`, for data that fits in `u64` | `*_u64` and `random_u64` workloads |

The baselines are written the way a competent user would write them: bulk builds where the API
has them, the smaller side visited in binary operations, and probing instead of merging when one
side is much smaller. For sparse random UUIDs the bar to clear is `btreeset` / `sortedvec`, not
`wrapped64`, so the report compares against both.

## Workloads

[common/workloads.rs](common/workloads.rs) generates them deterministically from fixed seeds.

- `uuid_v4`: random UUID v4 (122 random bits).
- `uuid_v7`: UUID v7 laid out like `uuid::Uuid::now_v7()`: a millisecond timestamp, a 42-bit
  counter reseeded every millisecond, random low bits, about 4 values per millisecond.
- `dense_runs`, `dense_gaps50`, `dense_small`: `(tenant << 64) | sequence` for 4 tenants, with
  long runs (run containers after `optimize`, the `_opt` variants), 50% density (bitmap
  containers) and small gaps (array containers).
- `mixed`: v4, v7 and the three dense shapes in equal parts.
- `random_u64` and `*_u64`: data that fits in `u64`, for the direct `RoaringTreemap` comparison.

Sizes go from 10k to 1M values. The binary operations use pairs with 50% and 1% overlap, a skewed
pair (1M against 1k), independent samples of the dense shapes, and 16-way unions and
intersections.

## Operations

- `ops` (single set): `insert` (one call per value, in random order, plus `_inorder` for v7),
  `from_iter` (bulk build from unsorted values), `from_sorted`, `contains_hit`, `contains_miss`,
  `remove_all`, `iter`, `range`, `len`, `rank`, `select`, `serialize`, `deserialize`, and
  `insert_uuid` / `contains_uuid` with the `uuid` feature.
- `setops`: `and`, `or`, `sub`, `xor`, the in-place forms `*_assign` (the clone of the target is
  not timed), `intersection_len`, `is_subset`, `multi_union16` and `multi_intersection16`.
- `memory`, not a criterion benchmark: heap bytes per value, counted with an allocator wrapper,
  after an insert build, a sorted build and `optimize()`, together with glibc's real chunk sizes,
  allocation counts, serialized sizes and roaring128's container composition.

A sorted `Vec` is not timed on per-value `insert` / `remove_all`, which are O(n) per value for it.
Benchmark ids are `<operation>/<implementation>/<workload>`.

## Running

```sh
benches/run.sh                      # full suite: ops + setops, memory table, metadata, report
benches/run.sh baselines            # everything except roaring128
BENCH_REPEAT=3 benches/run.sh       # three complete runs: adds the run-to-run spread
benches/run.sh ops 'contains_hit/'  # one binary, with a criterion regex filter
benches/run.sh memory uuid          # memory table for workloads whose names contain "uuid"
benches/run.sh report               # (re)generate the report of the latest results
benches/run.sh report DIR --subject btreeset   # compare another implementation instead
benches/run.sh test                 # every benchmark once, no measurement (a quick check)
benches/run.sh meta                 # print the metadata block only
```

Settings are environment variables:

| variable | default | meaning |
|---|---|---|
| `BENCH_WARMUP` | `1` | criterion warm-up, seconds |
| `BENCH_MEASURE` | `3` | criterion measurement time, seconds |
| `BENCH_SAMPLES` | `30` | samples per benchmark |
| `BENCH_REPEAT` | `1` | complete runs of ops + setops |
| `BENCH_IMPLS` | all | implementations to run, e.g. `"roaring128 wrapped64"` |
| `BENCH_SEED` | random | seed of the order shuffle (recorded in `meta.txt`) |
| `BENCH_CPU` | unset | optional `taskset -c` CPU list (Linux only) |
| `BENCH_OUT` | `target/bench-results/<UTC time>` | results directory |
| `BENCH_SKIP_CHECK` | `0` | `1` skips the self-check (not recommended) |

Criterion always collects `BENCH_SAMPLES` samples. An operation slower than
`BENCH_MEASURE / BENCH_SAMPLES` (a 1M-value build, for instance) gets that many single-iteration
samples, and the measurement time stretches to fit them.

How the run is set up:

- Each implementation runs in its own process (`BENCH_IMPL=<id>`), so each starts on a fresh
  heap. The order of the processes is shuffled for every run and binary, and recorded.
- Before timing anything, each process checks its implementation against a sorted-slice
  reference on small instances of every workload shape: lookups, iteration, ranges, rank /
  select, removals, serialization round trips, every set operation (also on run containers) and
  the 16-way operations. A wrong result aborts the run.
- The structures a benchmark queries are built untimed. Build results are dropped outside the
  timed region, and only one implementation's structures are alive at a time.

The defaults take roughly an hour per run: about 760 benchmarks at 4 s each, plus the operations
that need more than 3 s for 30 samples, plus the untimed clones of the in-place benchmarks. The
largest process (`setops` for `wrapped64`) peaks at about 1.6 GB RSS, and every other process
stays under 0.5 GB. The processes run one after another, so about 2 GB of free memory is enough.
For stable numbers use an otherwise idle machine on AC power with a fixed frequency policy (the
`performance` governor, for example), and keep the settings the same between runs.

## The results directory

| file | content |
|---|---|
| `meta.txt` | date, command, commit SHA (and whether the tree was dirty, with the modified files), branch, `rustc -vV`, cargo, `RUSTFLAGS`, OS, CPU model, online CPUs, SIMD CPU flags (`avx2`, `avx512*`, `asimd` / `sve`, ...), the backend roaring128 selected and the backends the CPU supports, governor / EPP / boost, load average at the start, memory, the settings and seed, and the implementation order of every run |
| `run-N/criterion/` | criterion's data for run N (`CRITERION_HOME`) |
| `run-N/logs/<binary>-<impl>.log` | output of each process, including its self-check line |
| `processes.tsv` | wall time, peak RSS and exit code of every process |
| `memory.md` | the memory table |
| `report.md` | the summary below |

## Reading report.md

One table per operation. Each cell gives criterion's median ± the half-width of its confidence
interval (95%, bootstrapped). With several runs, the point is the median of the run medians and
the interval spans all the runs' intervals, so it includes the run-to-run variation.

The comparison columns give the baseline's time divided by roaring128's (above 1: roaring128 is
faster):

- `vs wrapped64` and `vs treemap`;
- `vs best ordered`: whichever of `btreeset` and `sortedvec` is faster on that benchmark (the
  cell names which one);
- `vs best hash`: whichever of the two hash sets is faster.

Each comparison is one of:

- a **win** or a **loss** only when the two confidence intervals do not overlap (a loss is
  printed in bold);
- a **tie**, printed `≈1.03x`, when the intervals overlap. Ties count as ties, whatever the
  ratio.
- **by construction**, marked †, when the result follows from a design choice rather than from
  the implementation's quality. These are listed separately and kept out of the geometric means:
  - `len` against `wrapped64` / `treemap`: roaring128 caches its cardinality (O(1)), while they
    sum per-key cardinalities (O(keys));
  - `rank` / `select` against `wrapped64` / `treemap`: roaring128 uses the cached cardinality to
    sum the shorter side of the key;
  - `rank` / `select` against `sortedvec`: a sorted `Vec` answers by binary search or by index
    (a loss by construction for roaring128).

The summary counts wins, ties, losses and by-construction results for each comparison. It gives
the geometric-mean speedup (ties included, † excluded), overall and per operation, and lists
every loss. With `BENCH_REPEAT` > 1, a run-to-run spread section gives each implementation's
median, p90 and maximum spread, with the least stable benchmarks. A processes table shows wall
time and peak RSS.

`report.py --subject <id>` puts another implementation in roaring128's place. This is useful for
comparing the baselines against each other, e.g. `--subject btreeset`.

## Without run.sh

```sh
cargo bench --all-features --bench ops                         # all implementations, one process
BENCH_IMPL=wrapped64 cargo bench --all-features --bench setops -- 'and/'
cargo bench --all-features --bench memory -- --probe           # SIMD backend roaring128 selects
python3 benches/report.py target/criterion
```

The binaries use the same defaults (30 samples, 3 s, 1 s warm-up), and criterion flags override
them.
