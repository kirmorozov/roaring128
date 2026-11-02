# roaring128 benchmark report

## Environment

```
date: 2026-11-01T15:26:48Z
command: benches/run.sh all
commit: 46cbf8724d2b338b1051e2ba3660ce02cbff5898 (dirty: no)
branch: main
rustc -vV:
  rustc 1.99.1 (58cf8f9c3 2026-10-13)
  binary: rustc
  commit-hash: 58cf8f9c35b355199727627d07ae34a4e2846d67
  commit-date: 2026-10-13
  host: x86_64-unknown-linux-gnu
  release: 1.99.1
  LLVM version: 23.1.1
cargo: cargo 1.99.1 (5f94df478 2026-08-27)
RUSTFLAGS: <unset>
os: Linux 6.12.99-production+truenas x86_64
cpu: AMD Ryzen 7 5825U with Radeon Graphics
cpus online: 16
cpu simd flags: abm avx avx2 bmi1 bmi2 fma popcnt sse4_2 
roaring128 backend: avx2 (arch x86_64, supported: avx2=yes avx512=no neon=no)
governor: powersave, epp: performance, boost: 1
load average at start: 5.68 6.20 5.22
memory: 62.2 GiB total, 10.9 GiB available
settings: warmup=1s measure=3s samples=30 repeat=1 seed=949073077 pin=none features=--all-features skip_check=0
process per implementation: yes (BENCH_IMPL=<name> per process, order below)
order run 1 ops: treemap roaring128 hashset btreeset sortedvec hashset_fx wrapped64
order run 1 setops: treemap wrapped64 btreeset roaring128 sortedvec
```

1 run(s). Times are criterion medians ± the half-width of the confidence interval.
`vs X` = X time / roaring128 time (> 1: roaring128 faster). `≈`: the intervals overlap (a tie); **bold**: a loss with non-overlapping intervals; † holds by construction (see below, not in the geometric means). Best-of columns name the baseline used: btree = btreeset, vec = sortedvec, sip = hashset, fx = hashset_fx.

### insert

| workload | roaring128 | wrapped64 | btreeset | sortedvec | hashset | hashset_fx | treemap | vs wrapped64 | vs treemap | vs best ordered | vs best hash |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 31.9 ms ±2.0% | 25.3 ms ±1.1% | 201 ms ±1.6% |  | 106 ms ±1.6% | 46.4 ms ±2.7% |  | **0.79x** |  | 6.31x (btree) | 1.45x (fx) |
| dense_gaps50_u64_1M | 31.5 ms ±1.3% |  |  |  |  |  | 26.4 ms ±0.8% |  | **0.84x** |  |  |
| dense_runs_1M | 25 ms ±0.5% | 20.7 ms ±0.4% | 210 ms ±4.2% |  | 106 ms ±1.4% | 46.8 ms ±2.4% |  | **0.83x** |  | 8.37x (btree) | 1.87x (fx) |
| dense_runs_u64_1M | 24.5 ms ±2.8% |  |  |  |  |  | 20.8 ms ±0.8% |  | **0.85x** |  |  |
| dense_small_1M | 120 ms ±2.5% | 101 ms ±0.8% | 211 ms ±3.4% |  | 107 ms ±1.5% | 47.7 ms ±0.9% |  | **0.84x** |  | 1.76x (btree) | **0.40x** (fx) |
| dense_small_u64_1M | 118 ms ±1.6% |  |  |  |  |  | 97.1 ms ±0.8% |  | **0.82x** |  |  |
| mixed_1M | 130 ms ±4.6% | 301 ms ±1.6% | 205 ms ±3.9% |  | 108 ms ±3.4% | 46.8 ms ±1.5% |  | 2.32x |  | 1.58x (btree) | **0.36x** (fx) |
| random_u64_1M | 277 ms ±3.1% |  |  |  |  |  | 595 ms ±35.4% |  | 2.15x |  |  |
| uuid_v4_100k | 10.7 ms ±0.9% | 36.9 ms ±4.7% | 8.87 ms ±1.5% |  | 5.74 ms ±4.5% | 1.63 ms ±2.6% |  | 3.47x |  | **0.83x** (btree) | **0.15x** (fx) |
| uuid_v4_10k | 752 µs ±0.8% | 1.56 ms ±2.4% | 621 µs ±0.8% |  | 482 µs ±2.2% | 168 µs ±1.4% |  | 2.07x |  | **0.83x** (btree) | **0.22x** (fx) |
| uuid_v4_1M | 260 ms ±1.7% | 482 ms ±1.7% | 201 ms ±2.2% |  | 117 ms ±2.4% | 44.8 ms ±1.3% |  | 1.85x |  | **0.77x** (btree) | **0.17x** (fx) |
| uuid_v7_1M | 269 ms ±2.7% | 479 ms ±8.4% | 211 ms ±5.7% |  | 106 ms ±1.3% | 44.6 ms ±5.1% |  | 1.78x |  | **0.78x** (btree) | **0.17x** (fx) |
| uuid_v7_1M_inorder | 70.1 ms ±1.7% | 181 ms ±47.5% | 56.7 ms ±0.7% | 15.9 ms ±0.9% | 108 ms ±3.2% | 44.5 ms ±1.8% |  | 2.58x |  | **0.23x** (vec) | **0.63x** (fx) |

### from_iter

| workload | roaring128 | wrapped64 | btreeset | sortedvec | hashset | hashset_fx | treemap | vs wrapped64 | vs treemap | vs best ordered | vs best hash |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 24.2 ms ±1.6% | 32.5 ms ±1.1% | 39 ms ±2.2% | 26 ms ±1.2% | 105 ms ±8.2% | 35.3 ms ±1.2% |  | 1.34x |  | 1.07x (vec) | 1.46x (fx) |
| dense_runs_1M | 22.9 ms ±1.8% | 32.6 ms ±1.0% | 38.3 ms ±1.7% | 25.8 ms ±1.0% | 97.2 ms ±3.8% | 36.9 ms ±1.9% |  | 1.42x |  | 1.12x (vec) | 1.61x (fx) |
| dense_small_1M | 69.6 ms ±1.5% | 33.7 ms ±1.2% | 38.1 ms ±1.7% | 26.3 ms ±2.0% | 95.7 ms ±3.3% | 36.1 ms ±1.9% |  | **0.48x** |  | **0.38x** (vec) | **0.52x** (fx) |
| mixed_1M | 94.4 ms ±4.2% | 63.7 ms ±1.0% | 37.8 ms ±0.8% | 25.9 ms ±1.1% | 96.3 ms ±2.4% | 37.9 ms ±4.2% |  | **0.68x** |  | **0.27x** (vec) | **0.40x** (fx) |
| random_u64_1M | 152 ms ±1.0% |  |  |  |  |  | 115 ms ±0.6% |  | **0.76x** |  |  |
| uuid_v4_10k | 1.11 ms ±0.5% | 928 µs ±1.5% | 175 µs ±1.1% | 130 µs ±0.6% | 273 µs ±3.8% | 59.7 µs ±1.7% |  | **0.84x** |  | **0.12x** (vec) | **0.05x** (fx) |
| uuid_v4_1M | 159 ms ±2.5% | 142 ms ±5.7% | 38.1 ms ±0.6% | 25.5 ms ±0.8% | 106 ms ±7.9% | 32.1 ms ±5.6% |  | **0.90x** |  | **0.16x** (vec) | **0.20x** (fx) |
| uuid_v7_1M | 147 ms ±3.2% | 91.8 ms ±1.5% | 37.7 ms ±2.4% | 26.3 ms ±2.5% | 97.6 ms ±4.8% | 34.3 ms ±2.5% |  | **0.62x** |  | **0.18x** (vec) | **0.23x** (fx) |

### from_sorted

| workload | roaring128 | wrapped64 | btreeset | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 4.44 ms ±4.0% | 6.2 ms ±0.6% | 5.18 ms ±1.8% | 930 µs ±0.6% |  | 1.40x |  | **0.21x** (vec) |
| dense_gaps50_u64_1M | 4.42 ms ±3.5% |  |  |  | 6.07 ms ±0.4% |  | 1.37x |  |
| dense_runs_1M | 4.27 ms ±0.8% | 6.07 ms ±0.4% | 5.3 ms ±1.7% | 964 µs ±3.7% |  | 1.42x |  | **0.23x** (vec) |
| dense_runs_u64_1M | 4.3 ms ±6.0% |  |  |  | 5.9 ms ±1.0% |  | 1.37x |  |
| dense_small_1M | 4.38 ms ±2.1% | 6.75 ms ±1.2% | 5.25 ms ±3.2% | 937 µs ±3.8% |  | 1.54x |  | **0.21x** (vec) |
| dense_small_u64_1M | 4.33 ms ±2.0% |  |  |  | 6.18 ms ±1.9% |  | 1.43x |  |
| mixed_1M | 14.8 ms ±1.9% | 36.4 ms ±3.9% | 5.19 ms ±1.1% | 928 µs ±0.7% |  | 2.46x |  | **0.06x** (vec) |
| random_u64_1M | 26.4 ms ±1.4% |  |  |  | 157 ms ±36.8% |  | 5.94x |  |
| uuid_v4_10k | 223 µs ±0.5% | 743 µs ±1.1% | 39.3 µs ±1.3% | 3.98 µs ±0.9% |  | 3.34x |  | **0.02x** (vec) |
| uuid_v4_1M | 39.4 ms ±0.7% | 106 ms ±3.4% | 7.56 ms ±1.4% | 840 µs ±3.2% |  | 2.69x |  | **0.02x** (vec) |
| uuid_v7_1M | 25.7 ms ±0.5% | 63.4 ms ±1.5% | 5.06 ms ±0.8% | 1.01 ms ±4.3% |  | 2.47x |  | **0.04x** (vec) |

### contains_hit

| workload | roaring128 | wrapped64 | btreeset | sortedvec | hashset | hashset_fx | treemap | vs wrapped64 | vs treemap | vs best ordered | vs best hash |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 48.3 µs ±2.2% | 40.1 µs ±1.9% | 514 µs ±0.8% | 455 µs ±0.6% | 203 µs ±0.6% | 27.1 µs ±1.2% |  | **0.83x** |  | 9.42x (vec) | **0.56x** (fx) |
| dense_gaps50_u64_1M | 46 µs ±2.2% |  |  |  |  |  | 35.8 µs ±0.7% |  | **0.78x** |  |  |
| dense_runs_1M | 34.3 µs ±3.9% | 38.2 µs ±0.5% | 551 µs ±4.6% | 447 µs ±2.2% | 193 µs ±0.8% | 30.2 µs ±0.8% |  | 1.11x |  | 13.0x (vec) | **0.88x** (fx) |
| dense_runs_1M_opt | 53.8 µs ±1.4% | 46.5 µs ±0.9% | 521 µs ±1.7% | 515 µs ±3.8% | 200 µs ±0.3% | 30.8 µs ±1.7% |  | **0.86x** |  | 9.57x (vec) | **0.57x** (fx) |
| dense_small_1M | 344 µs ±5.9% | 207 µs ±2.6% | 513 µs ±1.1% | 451 µs ±0.9% | 202 µs ±0.9% | 29.4 µs ±1.4% |  | **0.60x** |  | 1.31x (vec) | **0.09x** (fx) |
| dense_small_u64_1M | 322 µs ±1.4% |  |  |  |  |  | 197 µs ±0.5% |  | **0.61x** |  |  |
| mixed_1M | 333 µs ±1.0% | 344 µs ±2.2% | 516 µs ±0.5% | 450 µs ±1.7% | 200 µs ±0.9% | 29.7 µs ±0.7% |  | 1.03x |  | 1.35x (vec) | **0.09x** (fx) |
| random_u64_1M | 642 µs ±9.7% |  |  |  |  |  | 563 µs ±0.4% |  | **0.88x** |  |  |
| uuid_v4_100k | 353 µs ±0.8% | 456 µs ±1.9% | 326 µs ±0.4% | 335 µs ±0.9% | 143 µs ±2.1% | 19.4 µs ±1.6% |  | 1.29x |  | **0.92x** (btree) | **0.05x** (fx) |
| uuid_v4_10k | 211 µs ±0.2% | 263 µs ±0.7% | 194 µs ±1.1% | 210 µs ±0.6% | 101 µs ±1.0% | 11.5 µs ±0.7% |  | 1.25x |  | **0.92x** (btree) | **0.05x** (fx) |
| uuid_v4_1M | 582 µs ±3.5% | 686 µs ±1.5% | 515 µs ±1.9% | 448 µs ±0.8% | 199 µs ±1.5% | 29 µs ±6.8% |  | 1.18x |  | **0.77x** (vec) | **0.05x** (fx) |
| uuid_v7_1M | 633 µs ±3.2% | 689 µs ±4.0% | 518 µs ±1.0% | 468 µs ±1.2% | 199 µs ±0.4% | 28.2 µs ±0.7% |  | 1.09x |  | **0.74x** (vec) | **0.04x** (fx) |

### contains_miss

| workload | roaring128 | wrapped64 | btreeset | sortedvec | hashset | hashset_fx | treemap | vs wrapped64 | vs treemap | vs best ordered | vs best hash |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 41.1 µs ±2.3% | 25 µs ±1.8% | 313 µs ±1.0% | 214 µs ±0.8% | 109 µs ±0.8% | 12.7 µs ±1.3% |  | **0.61x** |  | 5.21x (vec) | **0.31x** (fx) |
| dense_runs_1M | 32.6 µs ±4.7% | 18.5 µs ±0.9% | 122 µs ±0.7% | 86 µs ±2.8% | 98.4 µs ±0.7% | 11.5 µs ±2.1% |  | **0.57x** |  | 2.64x (vec) | **0.35x** (fx) |
| dense_runs_1M_opt | 35.8 µs ±0.8% | 19.9 µs ±0.9% | 142 µs ±0.7% | 84.1 µs ±0.9% | 108 µs ±1.2% | 12.2 µs ±0.9% |  | **0.56x** |  | 2.35x (vec) | **0.34x** (fx) |
| dense_small_1M | 220 µs ±4.1% | 121 µs ±1.5% | 338 µs ±5.8% | 227 µs ±1.9% | 103 µs ±1.9% | 11.9 µs ±2.0% |  | **0.55x** |  | ≈1.03x (vec) | **0.05x** (fx) |
| mixed_1M | 485 µs ±1.9% | 447 µs ±0.6% | 498 µs ±0.9% | 402 µs ±0.5% | 102 µs ±0.6% | 11.5 µs ±5.1% |  | **0.92x** |  | **0.83x** (vec) | **0.02x** (fx) |
| random_u64_1M | 665 µs ±2.2% |  |  |  |  |  | 550 µs ±0.8% |  | **0.83x** |  |  |
| uuid_v4_10k | 217 µs ±0.4% | 239 µs ±0.8% | 199 µs ±0.4% | 209 µs ±0.3% | 81.4 µs ±2.0% | 7.49 µs ±1.0% |  | 1.10x |  | **0.92x** (btree) | **0.03x** (fx) |
| uuid_v4_1M | 578 µs ±0.7% | 631 µs ±2.4% | 535 µs ±1.3% | 466 µs ±8.1% | 101 µs ±0.6% | 11.2 µs ±1.8% |  | 1.09x |  | **0.81x** (vec) | **0.02x** (fx) |
| uuid_v7_1M | 623 µs ±1.5% | 558 µs ±1.2% | 581 µs ±15.4% | 468 µs ±1.1% | 103 µs ±0.6% | 11.4 µs ±1.7% |  | **0.90x** |  | **0.75x** (vec) | **0.02x** (fx) |

### remove_all

| workload | roaring128 | wrapped64 | btreeset | hashset | hashset_fx | vs wrapped64 | vs best ordered | vs best hash |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_100k | 2.63 ms ±1.2% | 2.93 ms ±0.3% | 9.53 ms ±1.6% | 3.96 ms ±2.2% | 797 µs ±0.8% | 1.11x | 3.62x | **0.30x** (fx) |
| dense_runs_100k | 2.51 ms ±1.6% | 2.64 ms ±1.3% | 9.75 ms ±2.9% | 3.91 ms ±0.9% | 798 µs ±2.9% | 1.05x | 3.89x | **0.32x** (fx) |
| dense_small_100k | 7.12 ms ±2.7% | 6.64 ms ±1.0% | 9.61 ms ±0.6% | 3.91 ms ±1.7% | 727 µs ±0.8% | **0.93x** | 1.35x | **0.10x** (fx) |
| mixed_100k | 9.85 ms ±3.6% | 23.5 ms ±6.9% | 9.64 ms ±3.8% | 3.9 ms ±1.6% | 734 µs ±2.1% | 2.39x | ≈0.98x | **0.07x** (fx) |
| uuid_v4_100k | 11.8 ms ±2.2% | 64.9 ms ±4.4% | 9.49 ms ±1.0% | 4.24 ms ±3.1% | 723 µs ±1.0% | 5.52x | **0.81x** | **0.06x** (fx) |
| uuid_v7_100k | 12.3 ms ±1.0% | 46.9 ms ±5.2% | 9.65 ms ±0.9% | 3.86 ms ±0.6% | 799 µs ±2.1% | 3.82x | **0.78x** | **0.07x** (fx) |

### iter

| workload | roaring128 | wrapped64 | btreeset | sortedvec | hashset | hashset_fx | treemap | vs wrapped64 | vs treemap | vs best ordered | vs best hash |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 999 µs ±1.0% | 7.3 ms ±5.5% | 14.2 ms ±2.7% | 665 µs ±2.0% | 2.09 ms ±1.4% | 2.07 ms ±0.9% |  | 7.30x |  | **0.67x** (vec) | 2.07x (fx) |
| dense_gaps50_u64_1M | 999 µs ±0.9% |  |  |  |  |  | 6.42 ms ±1.5% |  | 6.43x |  |  |
| dense_runs_1M | 843 µs ±3.0% | 7.26 ms ±0.8% | 14 ms ±2.3% | 688 µs ±2.3% | 2.06 ms ±1.0% | 2.01 ms ±2.9% |  | 8.62x |  | **0.82x** (vec) | 2.38x (fx) |
| dense_runs_1M_opt | 503 µs ±2.6% | 7.08 ms ±0.8% | 13.5 ms ±4.0% | 719 µs ±2.9% | 2.04 ms ±1.6% | 1.96 ms ±2.7% |  | 14.1x |  | 1.43x (vec) | 3.89x (fx) |
| dense_runs_u64_1M | 839 µs ±2.6% |  |  |  |  |  | 6.63 ms ±5.2% |  | 7.90x |  |  |
| dense_small_1M | 420 µs ±3.6% | 6.76 ms ±0.6% | 14 ms ±2.3% | 665 µs ±5.4% | 2.07 ms ±0.4% | 2.09 ms ±1.0% |  | 16.1x |  | 1.58x (vec) | 4.92x (sip) |
| dense_small_u64_1M | 409 µs ±0.9% |  |  |  |  |  | 6.68 ms ±4.0% |  | 16.3x |  |  |
| mixed_1M | 10.7 ms ±1.7% | 83.2 ms ±1.4% | 13.8 ms ±3.4% | 688 µs ±3.3% | 2.07 ms ±0.7% | 2.09 ms ±0.7% |  | 7.78x |  | **0.06x** (vec) | **0.19x** (sip) |
| random_u64_1M | 26.6 ms ±1.5% |  |  |  |  |  | 128 ms ±0.8% |  | 4.81x |  |  |
| uuid_v4_10k | 98.1 µs ±0.4% | 508 µs ±2.1% | 16.8 µs ±2.7% | 3.26 µs ±1.2% | 10.8 µs ±4.0% | 8.65 µs ±2.0% |  | 5.18x |  | **0.03x** (vec) | **0.09x** (fx) |
| uuid_v4_1M | 25.9 ms ±0.5% | 247 ms ±3.3% | 13.5 ms ±3.3% | 659 µs ±4.0% | 2.09 ms ±1.2% | 2.03 ms ±1.0% |  | 9.51x |  | **0.03x** (vec) | **0.08x** (fx) |
| uuid_v7_1M | 26.7 ms ±1.8% | 161 ms ±1.3% | 14.4 ms ±2.0% | 694 µs ±2.2% | 2.1 ms ±2.6% | 2.07 ms ±1.8% |  | 6.04x |  | **0.03x** (vec) | **0.08x** (fx) |

### range

| workload | roaring128 | wrapped64 | btreeset | sortedvec | vs wrapped64 | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 17.2 µs ±5.2% | 109 µs ±1.1% | 28.5 µs ±1.1% | 506 ns ±1.0% | 6.38x | **0.03x** (vec) |
| dense_runs_1M | 15.3 µs ±0.8% | 107 µs ±0.7% | 30.4 µs ±1.8% | 499 ns ±0.8% | 7.01x | **0.03x** (vec) |
| dense_runs_1M_opt | 9.32 µs ±0.5% | 108 µs ±1.6% | 30.1 µs ±1.8% | 531 ns ±0.6% | 11.6x | **0.06x** (vec) |
| dense_small_1M | 9.9 µs ±4.4% | 99 µs ±1.3% | 28.3 µs ±0.9% | 503 ns ±0.8% | 9.99x | **0.05x** (vec) |
| mixed_1M | 87.7 µs ±5.2% | 603 µs ±0.8% | 29.8 µs ±0.5% | 506 ns ±1.2% | 6.88x | **0.01x** (vec) |
| uuid_v4_1M | 175 µs ±1.2% | 1.9 ms ±7.5% | 30.4 µs ±1.3% | 496 ns ±1.0% | 10.8x | **0.00x** (vec) |
| uuid_v7_1M | 189 µs ±3.4% | 861 µs ±4.1% | 29.7 µs ±1.6% | 511 ns ±1.9% | 4.56x | **0.00x** (vec) |

### len

| workload | roaring128 | wrapped64 | sortedvec | vs wrapped64 | vs best ordered |
|---|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 0.517 ns ±2.4% | 39.7 ns ±1.5% | 0.473 ns ±0.5% | 76.7x † | **0.91x** |
| uuid_v4_1M | 0.588 ns ±3.6% | 58.7 ms ±1.2% | 0.475 ns ±0.4% | 99917049x † | **0.81x** |

### rank

| workload | roaring128 | wrapped64 | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 1.61 µs ±1.8% | 4.9 µs ±0.6% | 256 ns ±1.4% |  | 3.05x † |  | **0.16x** † |
| dense_gaps50_u64_1M | 1.64 µs ±1.5% |  |  | 4.59 µs ±0.5% |  | 2.80x † |  |
| mixed_1M | 16.2 ms ±5.1% | 56 ms ±0.6% | 239 ns ±0.8% |  | 3.45x † |  | **0.00x** † |
| uuid_v4_1M | 84.8 ms ±1.6% | 512 ms ±2.2% | 239 ns ±1.0% |  | 6.03x † |  | **0.00x** † |

### select

| workload | roaring128 | wrapped64 | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 1.31 µs ±1.3% | 11.6 µs ±0.3% | 7.17 ns ±1.3% |  | 8.83x † |  | **0.01x** † |
| dense_gaps50_u64_1M | 1.36 µs ±2.2% |  |  | 11.3 µs ±0.8% |  | 8.28x † |  |
| mixed_1M | 18.3 ms ±2.8% | 47 ms ±1.1% | 7.28 ns ±2.8% |  | 2.56x † |  | **0.00x** † |
| uuid_v4_1M | 75.1 ms ±0.6% | 436 ms ±2.9% | 7.21 ns ±2.1% |  | 5.81x † |  | **0.00x** † |

### serialize

| workload | roaring128 | wrapped64 | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 14.3 µs ±2.7% | 23.7 µs ±3.1% | 2.35 ms ±2.5% |  | 1.65x |  | 164x |
| dense_gaps50_u64_1M | 13.9 µs ±1.4% |  |  | 21.6 µs ±0.6% |  | 1.55x |  |
| dense_runs_1M | 326 ns ±1.9% | 289 ns ±1.4% | 2.13 ms ±1.9% |  | **0.88x** |  | 6526x |
| dense_runs_u64_1M | 279 ns ±1.0% |  |  | 255 ns ±0.6% |  | **0.91x** |  |
| dense_small_1M | 122 µs ±3.4% | 1.05 ms ±15.9% | 2.04 ms ±1.4% |  | 8.61x |  | 16.8x |
| dense_small_u64_1M | 114 µs ±0.9% |  |  | 1.03 ms ±0.1% |  | 9.06x |  |
| mixed_1M | 23.6 ms ±3.3% | 37.6 ms ±1.1% | 2.21 ms ±1.5% |  | 1.60x |  | **0.09x** |
| random_u64_1M | 61.1 ms ±2.3% |  |  | 99.6 ms ±0.7% |  | 1.63x |  |
| uuid_v4_1M | 60.8 ms ±0.4% | 124 ms ±3.0% | 2.1 ms ±1.3% |  | 2.03x |  | **0.03x** |
| uuid_v7_1M | 60.9 ms ±1.4% | 95.2 ms ±1.7% | 2.28 ms ±4.5% |  | 1.56x |  | **0.04x** |

### deserialize

| workload | roaring128 | wrapped64 | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M | 17.7 µs ±2.8% | 27.3 µs ±1.3% | 3.84 ms ±5.8% |  | 1.55x |  | 217x |
| dense_gaps50_u64_1M | 17.7 µs ±3.8% |  |  | 26.6 µs ±0.9% |  | 1.50x |  |
| dense_runs_1M | 2.38 µs ±1.5% | 1.93 µs ±0.9% | 4.14 ms ±4.0% |  | **0.81x** |  | 1744x |
| dense_runs_u64_1M | 2.42 µs ±2.4% |  |  | 1.76 µs ±0.6% |  | **0.73x** |  |
| dense_small_1M | 239 µs ±2.5% | 643 µs ±2.6% | 3.94 ms ±2.7% |  | 2.69x |  | 16.5x |
| dense_small_u64_1M | 237 µs ±3.3% |  |  | 620 µs ±0.9% |  | 2.62x |  |
| mixed_1M | 22.3 ms ±4.7% | 32.5 ms ±0.7% | 4.35 ms ±6.9% |  | 1.46x |  | **0.20x** |
| random_u64_1M | 79.1 ms ±2.7% |  |  | 151 ms ±0.8% |  | 1.91x |  |
| uuid_v4_1M | 75.3 ms ±3.1% | 100 ms ±1.7% | 3.77 ms ±4.9% |  | 1.33x |  | **0.05x** |
| uuid_v7_1M | 79.9 ms ±3.0% | 74.4 ms ±1.5% | 3.92 ms ±7.6% |  | **0.93x** |  | **0.05x** |

### and

| workload | roaring128 | wrapped64 | btreeset | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M_indep | 23.9 µs ±0.6% | 36.8 µs ±2.2% | 45.6 ms ±1.0% | 9.38 ms ±0.3% |  | 1.54x |  | 392x (vec) |
| dense_gaps50_u64_1M_indep | 23.7 µs ±2.2% |  |  |  | 35.6 µs ±1.5% |  | 1.50x |  |
| dense_runs_1M_indep_opt | 2.79 µs ±1.1% | 3 µs ±0.7% | 39.2 ms ±0.5% | 2.36 ms ±3.0% |  | 1.08x |  | 847x (vec) |
| dense_small_1M_indep | 991 µs ±0.8% | 5.52 ms ±0.5% | 38.5 ms ±0.9% | 7.65 ms ±0.4% |  | 5.57x |  | 7.72x (vec) |
| mixed_500k_ov50 | 10.6 ms ±1.1% | 43.8 ms ±0.9% | 22.3 ms ±0.8% | 4.64 ms ±0.6% |  | 4.15x |  | **0.44x** (vec) |
| random_u64_500k_ov50 | 27.8 ms ±5.4% |  |  |  | 276 ms ±16.5% |  | 9.95x |  |
| uuid_v4_1M_vs_1k | 131 µs ±1.0% | 172 µs ±0.8% | 107 µs ±1.0% | 21.6 µs ±1.4% |  | 1.31x |  | **0.17x** (vec) |
| uuid_v4_500k_ov1 | 19 ms ±0.6% | 30.5 ms ±2.4% | 19.7 ms ±0.6% | 4.81 ms ±0.5% |  | 1.61x |  | **0.25x** (vec) |
| uuid_v4_500k_ov50 | 27.3 ms ±0.8% | 108 ms ±3.9% | 24 ms ±0.6% | 4.67 ms ±0.7% |  | 3.94x |  | **0.17x** (vec) |
| uuid_v7_500k_ov50 | 27.9 ms ±2.1% | 116 ms ±1.0% | 22.1 ms ±1.0% | 4.66 ms ±0.7% |  | 4.15x |  | **0.17x** (vec) |

### or

| workload | roaring128 | wrapped64 | btreeset | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M_indep | 17 µs ±0.6% | 29.1 µs ±1.7% | 54.4 ms ±0.4% | 10.3 ms ±0.2% |  | 1.71x |  | 609x (vec) |
| dense_gaps50_u64_1M_indep | 16.8 µs ±0.4% |  |  |  | 28.2 µs ±1.7% |  | 1.67x |  |
| dense_runs_1M_indep_opt | 2.65 µs ±0.6% | 2.38 µs ±1.3% | 41.5 ms ±1.1% | 2.45 ms ±0.9% |  | **0.90x** |  | 925x (vec) |
| dense_small_1M_indep | 1.23 ms ±0.6% | 8.34 ms ±0.4% | 55.6 ms ±1.3% | 9.23 ms ±1.1% |  | 6.79x |  | 7.51x (vec) |
| mixed_500k_ov50 | 19 ms ±2.6% | 66.4 ms ±0.8% | 26.7 ms ±0.6% | 5.14 ms ±0.4% |  | 3.50x |  | **0.27x** (vec) |
| random_u64_500k_ov50 | 48.4 ms ±0.7% |  |  |  | 416 ms ±1.5% |  | 8.61x |  |
| uuid_v4_1M_vs_1k | 25.7 ms ±1.2% | 277 ms ±1.2% | 26.1 ms ±0.9% | 2.2 ms ±1.3% |  | 10.8x |  | **0.09x** (vec) |
| uuid_v4_500k_ov1 | 76.1 ms ±1.3% | 283 ms ±2.2% | 28.8 ms ±0.7% | 5.67 ms ±0.5% |  | 3.72x |  | **0.07x** (vec) |
| uuid_v4_500k_ov50 | 48.3 ms ±0.8% | 234 ms ±1.9% | 32.5 ms ±1.6% | 5.2 ms ±0.4% |  | 4.84x |  | **0.11x** (vec) |
| uuid_v7_500k_ov50 | 48.7 ms ±1.0% | 126 ms ±1.4% | 26.7 ms ±0.4% | 5.2 ms ±0.7% |  | 2.59x |  | **0.11x** (vec) |

### sub

| workload | roaring128 | wrapped64 | btreeset | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M_indep | 22.3 µs ±0.6% | 32.4 µs ±0.8% | 45.4 ms ±0.6% | 9.44 ms ±0.6% |  | 1.46x |  | 424x (vec) |
| dense_gaps50_u64_1M_indep | 24 µs ±0.5% |  |  |  | 36.2 µs ±0.9% |  | 1.51x |  |
| dense_runs_1M_indep_opt | 2.3 µs ±0.7% | 2.41 µs ±1.2% | 31 ms ±0.9% | 1.55 ms ±1.9% |  | 1.05x |  | 673x (vec) |
| dense_small_1M_indep | 803 µs ±1.7% | 5.54 ms ±0.6% | 46.2 ms ±0.6% | 8.34 ms ±0.7% |  | 6.90x |  | 10.4x (vec) |
| mixed_500k_ov50 | 12.1 ms ±1.9% | 63.9 ms ±0.7% | 22.4 ms ±0.5% | 4.7 ms ±0.2% |  | 5.29x |  | **0.39x** (vec) |
| random_u64_500k_ov50 | 30.7 ms ±1.0% |  |  |  | 280 ms ±1.6% |  | 9.12x |  |
| uuid_v4_1M_vs_1k | 25.5 ms ±0.7% | 274 ms ±3.0% | 24.4 ms ±1.0% | 2.38 ms ±3.6% |  | 10.7x |  | **0.09x** (vec) |
| uuid_v4_500k_ov1 | 32.6 ms ±1.0% | 147 ms ±4.2% | 24.2 ms ±0.3% | 5.15 ms ±0.6% |  | 4.52x |  | **0.16x** (vec) |
| uuid_v4_500k_ov50 | 30.8 ms ±0.9% | 181 ms ±2.1% | 24 ms ±0.5% | 4.73 ms ±0.3% |  | 5.88x |  | **0.15x** (vec) |
| uuid_v7_500k_ov50 | 30.8 ms ±0.9% | 151 ms ±0.8% | 22.2 ms ±1.2% | 4.72 ms ±0.8% |  | 4.92x |  | **0.15x** (vec) |

### xor

| workload | roaring128 | wrapped64 | btreeset | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M_indep | 21.1 µs ±0.5% | 30.2 µs ±1.7% | 50.4 ms ±0.8% | 9.96 ms ±0.8% |  | 1.43x |  | 473x (vec) |
| dense_gaps50_u64_1M_indep | 21.4 µs ±0.5% |  |  |  | 29.5 µs ±0.9% |  | 1.38x |  |
| dense_runs_1M_indep_opt | 2.71 µs ±0.7% | 4.64 µs ±0.9% | 34.1 ms ±0.6% | 1.64 ms ±2.6% |  | 1.72x |  | 605x (vec) |
| dense_small_1M_indep | 1.38 ms ±3.1% | 7.87 ms ±0.8% | 53.7 ms ±0.7% | 9.1 ms ±0.5% |  | 5.69x |  | 6.58x (vec) |
| mixed_500k_ov50 | 17.8 ms ±1.9% | 68.6 ms ±1.2% | 24.7 ms ±0.3% | 4.96 ms ±0.2% |  | 3.86x |  | **0.28x** (vec) |
| random_u64_500k_ov50 | 43.6 ms ±1.3% |  |  |  | 245 ms ±0.7% |  | 5.62x |  |
| uuid_v4_1M_vs_1k | 25.5 ms ±0.8% | 274 ms ±1.6% | 25.2 ms ±1.5% | 2.18 ms ±2.6% |  | 10.7x |  | **0.09x** (vec) |
| uuid_v4_500k_ov1 | 74.7 ms ±2.2% | 287 ms ±1.7% | 27.4 ms ±0.9% | 5.67 ms ±0.9% |  | 3.84x |  | **0.08x** (vec) |
| uuid_v4_500k_ov50 | 43.5 ms ±0.7% | 222 ms ±2.4% | 28.5 ms ±1.3% | 5.03 ms ±1.3% |  | 5.10x |  | **0.12x** (vec) |
| uuid_v7_500k_ov50 | 43.9 ms ±0.8% | 136 ms ±1.2% | 25.1 ms ±1.1% | 4.97 ms ±0.7% |  | 3.11x |  | **0.11x** (vec) |

### and_assign

| workload | roaring128 | wrapped64 | btreeset | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M_indep | 13.2 µs ±0.8% | 28.2 µs ±1.3% | 74.7 ms ±1.5% | 7.33 ms ±0.5% |  | 2.14x |  | 554x (vec) |
| dense_gaps50_u64_1M_indep | 13.3 µs ±1.4% |  |  |  | 28.6 µs ±0.9% |  | 2.15x |  |
| dense_runs_1M_indep_opt | 3.07 µs ±0.6% | 2.37 µs ±0.6% | 57.1 ms ±0.9% | 1.84 ms ±1.6% |  | **0.77x** |  | 598x (vec) |
| mixed_500k_ov50 | 11.4 ms ±0.5% | 33.1 ms ±1.5% | 36 ms ±1.0% | 3.66 ms ±0.9% |  | 2.91x |  | **0.32x** (vec) |
| uuid_v4_1M_vs_1k | 136 µs ±0.9% | 184 µs ±0.7% | 105 µs ±0.3% | 21.2 µs ±1.2% |  | 1.35x |  | **0.16x** (vec) |
| uuid_v4_500k_ov50 | 29.4 ms ±1.0% | 127 ms ±45.6% | 35.9 ms ±0.7% | 3.67 ms ±0.7% |  | 4.33x |  | **0.13x** (vec) |
| uuid_v7_500k_ov50 | 29.3 ms ±1.1% | 114 ms ±26.6% | 36.3 ms ±0.9% | 3.63 ms ±0.7% |  | 3.89x |  | **0.12x** (vec) |

### or_assign

| workload | roaring128 | wrapped64 | btreeset | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M_indep | 8.57 µs ±2.3% | 20.9 µs ±0.6% | 71.4 ms ±0.6% | 10.4 ms ±0.6% |  | 2.44x |  | 1209x (vec) |
| dense_gaps50_u64_1M_indep | 8.89 µs ±0.4% |  |  |  | 21 µs ±0.6% |  | 2.36x |  |
| dense_runs_1M_indep_opt | 2.18 µs ±0.8% | 1.87 µs ±0.6% | 56.5 ms ±0.8% | 2.39 ms ±2.2% |  | **0.86x** |  | 1095x (vec) |
| mixed_500k_ov50 | 15.7 ms ±2.0% | 47.5 ms ±1.6% | 34.1 ms ±1.7% | 5.17 ms ±0.6% |  | 3.03x |  | **0.33x** (vec) |
| uuid_v4_1M_vs_1k | 25.4 ms ±1.2% | 335 ms ±14.4% | 75.5 ms ±0.7% | 2.2 ms ±3.1% |  | 13.2x |  | **0.09x** (vec) |
| uuid_v4_500k_ov50 | 39.3 ms ±0.6% | 185 ms ±24.8% | 36.6 ms ±0.6% | 5.19 ms ±0.5% |  | 4.71x |  | **0.13x** (vec) |
| uuid_v7_500k_ov50 | 39.6 ms ±1.2% | 119 ms ±14.3% | 34.8 ms ±1.2% | 5.34 ms ±3.5% |  | 3.00x |  | **0.13x** (vec) |

### sub_assign

| workload | roaring128 | wrapped64 | btreeset | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M_indep | 10.3 µs ±0.5% | 24.4 µs ±0.4% | 66.6 ms ±0.9% | 7.68 ms ±0.2% |  | 2.36x |  | 743x (vec) |
| dense_gaps50_u64_1M_indep | 11.9 µs ±0.5% |  |  |  | 28.2 µs ±1.7% |  | 2.36x |  |
| dense_runs_1M_indep_opt | 2.17 µs ±0.6% | 2.01 µs ±1.5% | 53.2 ms ±0.8% | 1.94 ms ±2.0% |  | **0.93x** |  | 893x (vec) |
| mixed_500k_ov50 | 12.1 ms ±0.9% | 35.3 ms ±0.7% | 31.8 ms ±0.5% | 3.87 ms ±0.5% |  | 2.93x |  | **0.32x** (vec) |
| uuid_v4_1M_vs_1k | 142 µs ±0.7% | 193 µs ±0.6% | 113 µs ±0.5% | 21.1 µs ±1.0% |  | 1.36x |  | **0.15x** (vec) |
| uuid_v4_500k_ov50 | 30.7 ms ±1.0% | 125 ms ±21.1% | 31.8 ms ±0.9% | 3.89 ms ±0.5% |  | 4.08x |  | **0.13x** (vec) |
| uuid_v7_500k_ov50 | 30.7 ms ±0.7% | 85.4 ms ±18.1% | 31.6 ms ±0.4% | 3.88 ms ±0.3% |  | 2.79x |  | **0.13x** (vec) |

### xor_assign

| workload | roaring128 | wrapped64 | btreeset | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M_indep | 8.69 µs ±0.6% | 22.2 µs ±1.2% | 104 ms ±0.4% | 10 ms ±0.6% |  | 2.55x |  | 1154x (vec) |
| dense_gaps50_u64_1M_indep | 9.02 µs ±2.0% |  |  |  | 21.6 µs ±1.2% |  | 2.39x |  |
| dense_runs_1M_indep_opt | 2.38 µs ±0.3% | 4.04 µs ±0.9% | 61.2 ms ±0.8% | 1.71 ms ±2.3% |  | 1.70x |  | 718x (vec) |
| mixed_500k_ov50 | 22.1 ms ±1.5% | 54.5 ms ±1.6% | 51 ms ±0.5% | 5.06 ms ±2.4% |  | 2.47x |  | **0.23x** (vec) |
| uuid_v4_1M_vs_1k | 25.3 ms ±1.1% | 332 ms ±10.7% | 134 ms ±0.7% | 2.15 ms ±1.6% |  | 13.1x |  | **0.09x** (vec) |
| uuid_v4_500k_ov50 | 59.6 ms ±1.3% | 155 ms ±1.0% | 50.5 ms ±0.5% | 5.02 ms ±0.7% |  | 2.60x |  | **0.08x** (vec) |
| uuid_v7_500k_ov50 | 60 ms ±0.9% | 116 ms ±0.8% | 50.5 ms ±1.0% | 5 ms ±1.1% |  | 1.93x |  | **0.08x** (vec) |

### intersection_len

| workload | roaring128 | wrapped64 | btreeset | sortedvec | treemap | vs wrapped64 | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M_indep | 6.3 µs ±0.5% | 19.1 µs ±1.9% | 33.9 ms ±0.5% | 7.84 ms ±0.2% |  | 3.03x |  | 1245x (vec) |
| dense_gaps50_u64_1M_indep | 6.56 µs ±1.8% |  |  |  | 19.6 µs ±1.2% |  | 2.99x |  |
| dense_runs_1M_indep_opt | 283 ns ±1.5% | 388 ns ±0.9% | 27 ms ±1.1% | 1.28 ms ±3.8% |  | 1.37x |  | 4542x (vec) |
| dense_small_1M_indep | 929 µs ±0.6% | 6.24 ms ±1.2% | 30.3 ms ±0.4% | 6.77 ms ±1.0% |  | 6.73x |  | 7.29x (vec) |
| mixed_500k_ov50 | 7.31 ms ±1.7% | 31.1 ms ±0.6% | 16.7 ms ±0.4% | 3.97 ms ±1.3% |  | 4.25x |  | **0.54x** (vec) |
| random_u64_500k_ov50 | 20.1 ms ±0.9% |  |  |  | 64 ms ±2.3% |  | 3.18x |  |
| uuid_v4_1M_vs_1k | 112 µs ±0.6% | 135 µs ±0.8% | 98.1 µs ±1.0% | 19.3 µs ±0.6% |  | 1.21x |  | **0.17x** (vec) |
| uuid_v4_500k_ov1 | 18.5 ms ±0.4% | 29.3 ms ±1.9% | 15.7 ms ±0.9% | 4.27 ms ±0.8% |  | 1.58x |  | **0.23x** (vec) |
| uuid_v4_500k_ov50 | 19.9 ms ±0.8% | 78.2 ms ±3.4% | 16.2 ms ±1.5% | 3.98 ms ±0.5% |  | 3.92x |  | **0.20x** (vec) |
| uuid_v7_500k_ov50 | 19.9 ms ±0.7% | 79.1 ms ±0.7% | 16.5 ms ±0.8% | 3.97 ms ±0.4% |  | 3.97x |  | **0.20x** (vec) |

### is_subset

| workload | roaring128 | wrapped64 | btreeset | sortedvec | vs wrapped64 | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|
| dense_gaps50_1M_indep_sub | 9.27 µs ±0.5% | 14.5 µs ±0.7% | 28.2 ms ±0.6% | 3.63 ms ±0.3% | 1.57x | 392x (vec) |
| mixed_500k_ov50_sub | 5.96 ms ±2.1% | 26.2 ms ±0.5% | 13.6 ms ±1.0% | 1.82 ms ±1.2% | 4.40x | **0.30x** (vec) |
| uuid_v4_1M_vs_1k_sub | 106 µs ±0.6% | 145 µs ±0.5% | 80.7 µs ±0.6% | 18.9 µs ±0.7% | 1.37x | **0.18x** (vec) |
| uuid_v4_500k_ov50_sub | 17.2 ms ±1.2% | 71 ms ±1.7% | 13.5 ms ±0.6% | 1.84 ms ±1.0% | 4.14x | **0.11x** (vec) |
| uuid_v7_500k_ov50_sub | 17.1 ms ±1.8% | 63.4 ms ±0.8% | 13.6 ms ±0.9% | 1.82 ms ±0.6% | 3.70x | **0.11x** (vec) |

### multi_union16

| workload | roaring128 | btreeset | sortedvec | treemap | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|
| dense_runs_16x100k_opt | 6.46 µs ±0.3% | 79.7 ms ±2.1% | 3.52 ms ±2.9% |  |  | 546x (vec) |
| dense_runs_u64_16x100k_opt | 6.17 µs ±1.8% |  |  | 4.13 µs ±0.8% | **0.67x** |  |
| uuid_v4_16x100k | 154 ms ±0.9% | 145 ms ±1.5% | 58.7 ms ±2.4% |  |  | **0.38x** (vec) |

### multi_intersection16

| workload | roaring128 | btreeset | sortedvec | treemap | vs treemap | vs best ordered |
|---|--:|--:|--:|--:|--:|--:|
| dense_runs_16x100k_opt | 8.46 µs ±2.2% | 48.3 ms ±1.8% | 2.06 ms ±2.4% |  |  | 243x (vec) |
| dense_runs_u64_16x100k_opt | 8.06 µs ±1.5% |  |  | 4.87 µs ±0.9% | **0.60x** |  |
| uuid_v4_16x100k | 41 ms ±0.6% | 18.6 ms ±0.6% | 2.59 ms ±0.4% |  |  | **0.06x** (vec) |

### insert_uuid

| workload | roaring128 |
|---|--:|
| uuid_v4_100k | 12 ms ±8.4% |

### contains_uuid

| workload | roaring128 |
|---|--:|
| uuid_v4_100k | 375 µs ±1.2% |

## Summary for roaring128

| vs | wins | ties | losses | by construction (excluded) | geometric-mean speedup |
|---|--:|--:|--:|--:|--:|
| wrapped64 | 117 | 0 | 25 | 8 | 2.37x |
| treemap | 29 | 0 | 12 | 2 | 2.05x |
| best ordered | 51 | 2 | 95 | 6 | 1.16x |
| best hash | 8 | 0 | 39 | 0 | 0.22x |

Geometric-mean speedup per operation (ties included, † excluded):

| operation | vs wrapped64 | vs treemap | vs best ordered | vs best hash |
|---|--:|--:|--:|--:|
| insert | 1.62x | 1.06x | 1.34x | 0.40x |
| from_iter | 0.84x | 0.76x | 0.33x | 0.38x |
| from_sorted | 2.08x | 2.00x | 0.07x |  |
| contains_hit | 1.00x | 0.75x | 2.15x | 0.13x |
| contains_miss | 0.75x | 0.83x | 1.41x | 0.07x |
| remove_all | 1.95x |  | 1.51x | 0.12x |
| iter | 8.70x | 7.95x | 0.19x | 0.56x |
| range | 7.80x |  | 0.01x |  |
| len |  |  | 0.86x |  |
| serialize | 2.00x | 2.14x | 3.60x |  |
| deserialize | 1.35x | 1.53x | 3.80x |  |
| and | 2.46x | 3.87x | 2.46x |  |
| or | 3.41x | 3.80x | 1.74x |  |
| sub | 4.09x | 3.71x | 2.11x |  |
| xor | 3.70x | 2.79x | 1.61x |  |
| and_assign | 2.19x | 2.15x | 2.52x |  |
| or_assign | 3.25x | 2.36x | 2.96x |  |
| sub_assign | 2.15x | 2.36x | 2.82x |  |
| xor_assign | 2.98x | 2.39x | 2.20x |  |
| intersection_len | 2.78x | 3.08x | 3.70x |  |
| is_subset | 2.71x |  | 0.75x |  |
| multi_union16 |  | 0.67x | 14.4x |  |
| multi_intersection16 |  | 0.60x | 3.92x |  |

### Losses: roaring128 slower, intervals not overlapping (171)

| operation | workload | baseline | roaring128 | baseline time | speedup |
|---|---|---|--:|--:|--:|
| range | uuid_v7_1M | sortedvec | 189 µs | 511 ns | 0.00x |
| range | uuid_v4_1M | sortedvec | 175 µs | 496 ns | 0.00x |
| range | mixed_1M | sortedvec | 87.7 µs | 506 ns | 0.01x |
| from_sorted | uuid_v4_10k | sortedvec | 223 µs | 3.98 µs | 0.02x |
| contains_miss | uuid_v7_1M | hashset_fx | 623 µs | 11.4 µs | 0.02x |
| contains_miss | uuid_v4_1M | hashset_fx | 578 µs | 11.2 µs | 0.02x |
| from_sorted | uuid_v4_1M | sortedvec | 39.4 ms | 840 µs | 0.02x |
| contains_miss | mixed_1M | hashset_fx | 485 µs | 11.5 µs | 0.02x |
| iter | uuid_v4_1M | sortedvec | 25.9 ms | 659 µs | 0.03x |
| iter | uuid_v7_1M | sortedvec | 26.7 ms | 694 µs | 0.03x |
| range | dense_gaps50_1M | sortedvec | 17.2 µs | 506 ns | 0.03x |
| range | dense_runs_1M | sortedvec | 15.3 µs | 499 ns | 0.03x |
| iter | uuid_v4_10k | sortedvec | 98.1 µs | 3.26 µs | 0.03x |
| serialize | uuid_v4_1M | sortedvec | 60.8 ms | 2.1 ms | 0.03x |
| contains_miss | uuid_v4_10k | hashset_fx | 217 µs | 7.49 µs | 0.03x |
| serialize | uuid_v7_1M | sortedvec | 60.9 ms | 2.28 ms | 0.04x |
| from_sorted | uuid_v7_1M | sortedvec | 25.7 ms | 1.01 ms | 0.04x |
| contains_hit | uuid_v7_1M | hashset_fx | 633 µs | 28.2 µs | 0.04x |
| deserialize | uuid_v7_1M | sortedvec | 79.9 ms | 3.92 ms | 0.05x |
| contains_hit | uuid_v4_1M | hashset_fx | 582 µs | 29 µs | 0.05x |
| deserialize | uuid_v4_1M | sortedvec | 75.3 ms | 3.77 ms | 0.05x |
| range | dense_small_1M | sortedvec | 9.9 µs | 503 ns | 0.05x |
| from_iter | uuid_v4_10k | hashset_fx | 1.11 ms | 59.7 µs | 0.05x |
| contains_miss | dense_small_1M | hashset_fx | 220 µs | 11.9 µs | 0.05x |
| contains_hit | uuid_v4_10k | hashset_fx | 211 µs | 11.5 µs | 0.05x |
| contains_hit | uuid_v4_100k | hashset_fx | 353 µs | 19.4 µs | 0.05x |
| range | dense_runs_1M_opt | sortedvec | 9.32 µs | 531 ns | 0.06x |
| remove_all | uuid_v4_100k | hashset_fx | 11.8 ms | 723 µs | 0.06x |
| from_sorted | mixed_1M | sortedvec | 14.8 ms | 928 µs | 0.06x |
| multi_intersection16 | uuid_v4_16x100k | sortedvec | 41 ms | 2.59 ms | 0.06x |
| iter | mixed_1M | sortedvec | 10.7 ms | 688 µs | 0.06x |
| remove_all | uuid_v7_100k | hashset_fx | 12.3 ms | 799 µs | 0.07x |
| remove_all | mixed_100k | hashset_fx | 9.85 ms | 734 µs | 0.07x |
| or | uuid_v4_500k_ov1 | sortedvec | 76.1 ms | 5.67 ms | 0.07x |
| xor | uuid_v4_500k_ov1 | sortedvec | 74.7 ms | 5.67 ms | 0.08x |
| iter | uuid_v7_1M | hashset_fx | 26.7 ms | 2.07 ms | 0.08x |
| iter | uuid_v4_1M | hashset_fx | 25.9 ms | 2.03 ms | 0.08x |
| xor_assign | uuid_v7_500k_ov50 | sortedvec | 60 ms | 5 ms | 0.08x |
| xor_assign | uuid_v4_500k_ov50 | sortedvec | 59.6 ms | 5.02 ms | 0.08x |
| xor_assign | uuid_v4_1M_vs_1k | sortedvec | 25.3 ms | 2.15 ms | 0.09x |
| contains_hit | dense_small_1M | hashset_fx | 344 µs | 29.4 µs | 0.09x |
| or | uuid_v4_1M_vs_1k | sortedvec | 25.7 ms | 2.2 ms | 0.09x |
| xor | uuid_v4_1M_vs_1k | sortedvec | 25.5 ms | 2.18 ms | 0.09x |
| or_assign | uuid_v4_1M_vs_1k | sortedvec | 25.4 ms | 2.2 ms | 0.09x |
| iter | uuid_v4_10k | hashset_fx | 98.1 µs | 8.65 µs | 0.09x |
| contains_hit | mixed_1M | hashset_fx | 333 µs | 29.7 µs | 0.09x |
| sub | uuid_v4_1M_vs_1k | sortedvec | 25.5 ms | 2.38 ms | 0.09x |
| serialize | mixed_1M | sortedvec | 23.6 ms | 2.21 ms | 0.09x |
| remove_all | dense_small_100k | hashset_fx | 7.12 ms | 727 µs | 0.10x |
| is_subset | uuid_v7_500k_ov50_sub | sortedvec | 17.1 ms | 1.82 ms | 0.11x |
| or | uuid_v7_500k_ov50 | sortedvec | 48.7 ms | 5.2 ms | 0.11x |
| is_subset | uuid_v4_500k_ov50_sub | sortedvec | 17.2 ms | 1.84 ms | 0.11x |
| or | uuid_v4_500k_ov50 | sortedvec | 48.3 ms | 5.2 ms | 0.11x |
| xor | uuid_v7_500k_ov50 | sortedvec | 43.9 ms | 4.97 ms | 0.11x |
| xor | uuid_v4_500k_ov50 | sortedvec | 43.5 ms | 5.03 ms | 0.12x |
| from_iter | uuid_v4_10k | sortedvec | 1.11 ms | 130 µs | 0.12x |
| and_assign | uuid_v7_500k_ov50 | sortedvec | 29.3 ms | 3.63 ms | 0.12x |
| and_assign | uuid_v4_500k_ov50 | sortedvec | 29.4 ms | 3.67 ms | 0.13x |
| sub_assign | uuid_v7_500k_ov50 | sortedvec | 30.7 ms | 3.88 ms | 0.13x |
| sub_assign | uuid_v4_500k_ov50 | sortedvec | 30.7 ms | 3.89 ms | 0.13x |
| or_assign | uuid_v4_500k_ov50 | sortedvec | 39.3 ms | 5.19 ms | 0.13x |
| or_assign | uuid_v7_500k_ov50 | sortedvec | 39.6 ms | 5.34 ms | 0.13x |
| sub_assign | uuid_v4_1M_vs_1k | sortedvec | 142 µs | 21.1 µs | 0.15x |
| insert | uuid_v4_100k | hashset_fx | 10.7 ms | 1.63 ms | 0.15x |
| sub | uuid_v7_500k_ov50 | sortedvec | 30.8 ms | 4.72 ms | 0.15x |
| sub | uuid_v4_500k_ov50 | sortedvec | 30.8 ms | 4.73 ms | 0.15x |
| and_assign | uuid_v4_1M_vs_1k | sortedvec | 136 µs | 21.2 µs | 0.16x |
| sub | uuid_v4_500k_ov1 | sortedvec | 32.6 ms | 5.15 ms | 0.16x |
| from_iter | uuid_v4_1M | sortedvec | 159 ms | 25.5 ms | 0.16x |
| and | uuid_v4_1M_vs_1k | sortedvec | 131 µs | 21.6 µs | 0.17x |
| insert | uuid_v7_1M | hashset_fx | 269 ms | 44.6 ms | 0.17x |
| and | uuid_v7_500k_ov50 | sortedvec | 27.9 ms | 4.66 ms | 0.17x |
| and | uuid_v4_500k_ov50 | sortedvec | 27.3 ms | 4.67 ms | 0.17x |
| insert | uuid_v4_1M | hashset_fx | 260 ms | 44.8 ms | 0.17x |
| intersection_len | uuid_v4_1M_vs_1k | sortedvec | 112 µs | 19.3 µs | 0.17x |
| from_iter | uuid_v7_1M | sortedvec | 147 ms | 26.3 ms | 0.18x |
| is_subset | uuid_v4_1M_vs_1k_sub | sortedvec | 106 µs | 18.9 µs | 0.18x |
| iter | mixed_1M | hashset | 10.7 ms | 2.07 ms | 0.19x |
| deserialize | mixed_1M | sortedvec | 22.3 ms | 4.35 ms | 0.20x |
| intersection_len | uuid_v7_500k_ov50 | sortedvec | 19.9 ms | 3.97 ms | 0.20x |
| intersection_len | uuid_v4_500k_ov50 | sortedvec | 19.9 ms | 3.98 ms | 0.20x |
| from_iter | uuid_v4_1M | hashset_fx | 159 ms | 32.1 ms | 0.20x |
| from_sorted | dense_gaps50_1M | sortedvec | 4.44 ms | 930 µs | 0.21x |
| from_sorted | dense_small_1M | sortedvec | 4.38 ms | 937 µs | 0.21x |
| insert | uuid_v4_10k | hashset_fx | 752 µs | 168 µs | 0.22x |
| from_sorted | dense_runs_1M | sortedvec | 4.27 ms | 964 µs | 0.23x |
| insert | uuid_v7_1M_inorder | sortedvec | 70.1 ms | 15.9 ms | 0.23x |
| xor_assign | mixed_500k_ov50 | sortedvec | 22.1 ms | 5.06 ms | 0.23x |
| intersection_len | uuid_v4_500k_ov1 | sortedvec | 18.5 ms | 4.27 ms | 0.23x |
| from_iter | uuid_v7_1M | hashset_fx | 147 ms | 34.3 ms | 0.23x |
| and | uuid_v4_500k_ov1 | sortedvec | 19 ms | 4.81 ms | 0.25x |
| or | mixed_500k_ov50 | sortedvec | 19 ms | 5.14 ms | 0.27x |
| from_iter | mixed_1M | sortedvec | 94.4 ms | 25.9 ms | 0.27x |
| xor | mixed_500k_ov50 | sortedvec | 17.8 ms | 4.96 ms | 0.28x |
| remove_all | dense_gaps50_100k | hashset_fx | 2.63 ms | 797 µs | 0.30x |
| is_subset | mixed_500k_ov50_sub | sortedvec | 5.96 ms | 1.82 ms | 0.30x |
| contains_miss | dense_gaps50_1M | hashset_fx | 41.1 µs | 12.7 µs | 0.31x |
| remove_all | dense_runs_100k | hashset_fx | 2.51 ms | 798 µs | 0.32x |
| sub_assign | mixed_500k_ov50 | sortedvec | 12.1 ms | 3.87 ms | 0.32x |
| and_assign | mixed_500k_ov50 | sortedvec | 11.4 ms | 3.66 ms | 0.32x |
| or_assign | mixed_500k_ov50 | sortedvec | 15.7 ms | 5.17 ms | 0.33x |
| contains_miss | dense_runs_1M_opt | hashset_fx | 35.8 µs | 12.2 µs | 0.34x |
| contains_miss | dense_runs_1M | hashset_fx | 32.6 µs | 11.5 µs | 0.35x |
| insert | mixed_1M | hashset_fx | 130 ms | 46.8 ms | 0.36x |
| from_iter | dense_small_1M | sortedvec | 69.6 ms | 26.3 ms | 0.38x |
| multi_union16 | uuid_v4_16x100k | sortedvec | 154 ms | 58.7 ms | 0.38x |
| sub | mixed_500k_ov50 | sortedvec | 12.1 ms | 4.7 ms | 0.39x |
| insert | dense_small_1M | hashset_fx | 120 ms | 47.7 ms | 0.40x |
| from_iter | mixed_1M | hashset_fx | 94.4 ms | 37.9 ms | 0.40x |
| and | mixed_500k_ov50 | sortedvec | 10.6 ms | 4.64 ms | 0.44x |
| from_iter | dense_small_1M | wrapped64 | 69.6 ms | 33.7 ms | 0.48x |
| from_iter | dense_small_1M | hashset_fx | 69.6 ms | 36.1 ms | 0.52x |
| intersection_len | mixed_500k_ov50 | sortedvec | 7.31 ms | 3.97 ms | 0.54x |
| contains_miss | dense_small_1M | wrapped64 | 220 µs | 121 µs | 0.55x |
| contains_miss | dense_runs_1M_opt | wrapped64 | 35.8 µs | 19.9 µs | 0.56x |
| contains_hit | dense_gaps50_1M | hashset_fx | 48.3 µs | 27.1 µs | 0.56x |
| contains_miss | dense_runs_1M | wrapped64 | 32.6 µs | 18.5 µs | 0.57x |
| contains_hit | dense_runs_1M_opt | hashset_fx | 53.8 µs | 30.8 µs | 0.57x |
| contains_hit | dense_small_1M | wrapped64 | 344 µs | 207 µs | 0.60x |
| multi_intersection16 | dense_runs_u64_16x100k_opt | treemap | 8.06 µs | 4.87 µs | 0.60x |
| contains_miss | dense_gaps50_1M | wrapped64 | 41.1 µs | 25 µs | 0.61x |
| contains_hit | dense_small_u64_1M | treemap | 322 µs | 197 µs | 0.61x |
| from_iter | uuid_v7_1M | wrapped64 | 147 ms | 91.8 ms | 0.62x |
| insert | uuid_v7_1M_inorder | hashset_fx | 70.1 ms | 44.5 ms | 0.63x |
| iter | dense_gaps50_1M | sortedvec | 999 µs | 665 µs | 0.67x |
| multi_union16 | dense_runs_u64_16x100k_opt | treemap | 6.17 µs | 4.13 µs | 0.67x |
| from_iter | mixed_1M | wrapped64 | 94.4 ms | 63.7 ms | 0.68x |
| deserialize | dense_runs_u64_1M | treemap | 2.42 µs | 1.76 µs | 0.73x |
| contains_hit | uuid_v7_1M | sortedvec | 633 µs | 468 µs | 0.74x |
| contains_miss | uuid_v7_1M | sortedvec | 623 µs | 468 µs | 0.75x |
| from_iter | random_u64_1M | treemap | 152 ms | 115 ms | 0.76x |
| contains_hit | uuid_v4_1M | sortedvec | 582 µs | 448 µs | 0.77x |
| and_assign | dense_runs_1M_indep_opt | wrapped64 | 3.07 µs | 2.37 µs | 0.77x |
| insert | uuid_v4_1M | btreeset | 260 ms | 201 ms | 0.77x |
| contains_hit | dense_gaps50_u64_1M | treemap | 46 µs | 35.8 µs | 0.78x |
| insert | uuid_v7_1M | btreeset | 269 ms | 211 ms | 0.78x |
| remove_all | uuid_v7_100k | btreeset | 12.3 ms | 9.65 ms | 0.78x |
| insert | dense_gaps50_1M | wrapped64 | 31.9 ms | 25.3 ms | 0.79x |
| contains_miss | uuid_v4_1M | sortedvec | 578 µs | 466 µs | 0.81x |
| remove_all | uuid_v4_100k | btreeset | 11.8 ms | 9.49 ms | 0.81x |
| len | uuid_v4_1M | sortedvec | 0.588 ns | 0.475 ns | 0.81x |
| deserialize | dense_runs_1M | wrapped64 | 2.38 µs | 1.93 µs | 0.81x |
| iter | dense_runs_1M | sortedvec | 843 µs | 688 µs | 0.82x |
| insert | dense_small_u64_1M | treemap | 118 ms | 97.1 ms | 0.82x |
| insert | uuid_v4_10k | btreeset | 752 µs | 621 µs | 0.83x |
| insert | dense_runs_1M | wrapped64 | 25 ms | 20.7 ms | 0.83x |
| contains_miss | random_u64_1M | treemap | 665 µs | 550 µs | 0.83x |
| contains_miss | mixed_1M | sortedvec | 485 µs | 402 µs | 0.83x |
| contains_hit | dense_gaps50_1M | wrapped64 | 48.3 µs | 40.1 µs | 0.83x |
| insert | uuid_v4_100k | btreeset | 10.7 ms | 8.87 ms | 0.83x |
| from_iter | uuid_v4_10k | wrapped64 | 1.11 ms | 928 µs | 0.84x |
| insert | dense_gaps50_u64_1M | treemap | 31.5 ms | 26.4 ms | 0.84x |
| insert | dense_small_1M | wrapped64 | 120 ms | 101 ms | 0.84x |
| insert | dense_runs_u64_1M | treemap | 24.5 ms | 20.8 ms | 0.85x |
| or_assign | dense_runs_1M_indep_opt | wrapped64 | 2.18 µs | 1.87 µs | 0.86x |
| contains_hit | dense_runs_1M_opt | wrapped64 | 53.8 µs | 46.5 µs | 0.86x |
| contains_hit | random_u64_1M | treemap | 642 µs | 563 µs | 0.88x |
| contains_hit | dense_runs_1M | hashset_fx | 34.3 µs | 30.2 µs | 0.88x |
| serialize | dense_runs_1M | wrapped64 | 326 ns | 289 ns | 0.88x |
| contains_miss | uuid_v7_1M | wrapped64 | 623 µs | 558 µs | 0.90x |
| from_iter | uuid_v4_1M | wrapped64 | 159 ms | 142 ms | 0.90x |
| or | dense_runs_1M_indep_opt | wrapped64 | 2.65 µs | 2.38 µs | 0.90x |
| serialize | dense_runs_u64_1M | treemap | 279 ns | 255 ns | 0.91x |
| len | dense_gaps50_1M | sortedvec | 0.517 ns | 0.473 ns | 0.91x |
| contains_miss | uuid_v4_10k | btreeset | 217 µs | 199 µs | 0.92x |
| contains_hit | uuid_v4_10k | btreeset | 211 µs | 194 µs | 0.92x |
| contains_miss | mixed_1M | wrapped64 | 485 µs | 447 µs | 0.92x |
| contains_hit | uuid_v4_100k | btreeset | 353 µs | 326 µs | 0.92x |
| sub_assign | dense_runs_1M_indep_opt | wrapped64 | 2.17 µs | 2.01 µs | 0.93x |
| deserialize | uuid_v7_1M | wrapped64 | 79.9 ms | 74.4 ms | 0.93x |
| remove_all | dense_small_100k | wrapped64 | 7.12 ms | 6.64 ms | 0.93x |

### By construction, not in the geometric means (16)

| operation | workload | baseline | speedup | why |
|---|---|---|--:|---|
| len | dense_gaps50_1M | wrapped64 | 76.7x | cached cardinality: O(1) against summing the per-key cardinalities of every outer key (O(keys)) |
| len | uuid_v4_1M | wrapped64 | 99917049x | cached cardinality: O(1) against summing the per-key cardinalities of every outer key (O(keys)) |
| rank | dense_gaps50_1M | sortedvec | 0.16x | a sorted Vec ranks by binary search (O(log n)) without summing |
| rank | dense_gaps50_1M | wrapped64 | 3.05x | the baseline sums the cardinality of every key below the value (O(keys)); roaring128 walks only the shorter side using its cached total, a sorted Vec binary-searches |
| rank | dense_gaps50_u64_1M | treemap | 2.80x | the baseline sums the cardinality of every key below the value (O(keys)); roaring128 walks only the shorter side using its cached total, a sorted Vec binary-searches |
| rank | mixed_1M | sortedvec | 0.00x | a sorted Vec ranks by binary search (O(log n)) without summing |
| rank | mixed_1M | wrapped64 | 3.45x | the baseline sums the cardinality of every key below the value (O(keys)); roaring128 walks only the shorter side using its cached total, a sorted Vec binary-searches |
| rank | uuid_v4_1M | sortedvec | 0.00x | a sorted Vec ranks by binary search (O(log n)) without summing |
| rank | uuid_v4_1M | wrapped64 | 6.03x | the baseline sums the cardinality of every key below the value (O(keys)); roaring128 walks only the shorter side using its cached total, a sorted Vec binary-searches |
| select | dense_gaps50_1M | sortedvec | 0.01x | a sorted Vec selects by index (O(1)) |
| select | dense_gaps50_1M | wrapped64 | 8.83x | the baseline sums the cardinality of every key below the value (O(keys)); roaring128 walks only the shorter side using its cached total, a sorted Vec binary-searches |
| select | dense_gaps50_u64_1M | treemap | 8.28x | the baseline sums the cardinality of every key below the value (O(keys)); roaring128 walks only the shorter side using its cached total, a sorted Vec binary-searches |
| select | mixed_1M | sortedvec | 0.00x | a sorted Vec selects by index (O(1)) |
| select | mixed_1M | wrapped64 | 2.56x | the baseline sums the cardinality of every key below the value (O(keys)); roaring128 walks only the shorter side using its cached total, a sorted Vec binary-searches |
| select | uuid_v4_1M | sortedvec | 0.00x | a sorted Vec selects by index (O(1)) |
| select | uuid_v4_1M | wrapped64 | 5.81x | the baseline sums the cardinality of every key below the value (O(keys)); roaring128 walks only the shorter side using its cached total, a sorted Vec binary-searches |

## Processes

| binary | implementation | processes | total wall time | peak RSS | failed |
|---|---|--:|--:|--:|--:|
| memory | all | 1 | 11 s | 893 MiB | 0 |
| ops | btreeset | 1 | 442 s | 85 MiB | 0 |
| ops | hashset | 1 | 400 s | 102 MiB | 0 |
| ops | hashset_fx | 1 | 302 s | 102 MiB | 0 |
| ops | roaring128 | 1 | 747 s | 180 MiB | 0 |
| ops | sortedvec | 1 | 417 s | 96 MiB | 0 |
| ops | treemap | 1 | 224 s | 440 MiB | 0 |
| ops | wrapped64 | 1 | 684 s | 1129 MiB | 0 |
| setops | btreeset | 1 | 523 s | 184 MiB | 0 |
| setops | roaring128 | 1 | 586 s | 226 MiB | 0 |
| setops | sortedvec | 1 | 534 s | 157 MiB | 0 |
| setops | treemap | 1 | 138 s | 378 MiB | 0 |
| setops | wrapped64 | 1 | 614 s | 1497 MiB | 1 |

Memory footprint: see memory.md.
