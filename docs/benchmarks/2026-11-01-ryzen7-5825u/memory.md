== roaring128 benches: memory ==
backend=avx2 | cpu=AMD Ryzen 7 5825U with Radeon Graphics | governor=powersave epp=performance boost=on | loadavg=5.49 5.83 5.62
allocator: system (glibc malloc); `heap B/v` counts requested bytes, `malloc B/v` real chunk sizes

| workload | impl | heap B/v insert | heap B/v sorted | heap B/v optimized | malloc B/v insert | blocks/v | allocs/v insert | allocs/v sorted | ser B/v | ser B/v optimized |
|---|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| uuid_v4_10k | roaring128 | 50.6 | 34.3 | 50.6 | 52.8 | 0.13 | 0.13 | 0.09 | 18.0 | 18.0 |
| uuid_v4_10k | wrapped64 | 539 | 522 | 539 | 597 | 3.13 | 3.13 | 3.09 | 38.0 | 38.0 |
| uuid_v4_10k | btreeset | 27.1 | 18.2 | - | 29.3 | 0.13 | 0.13 | 0.09 | - | - |
| uuid_v4_10k | sortedvec | 16.0 | 16.0 | 16.0 | 16.0 | 0.00 | 0.00 | 0.00 | 16.0 | 16.0 |
| uuid_v4_10k | hashset | 27.9 | - | - | 27.9 | 0.00 | 0.00 | - | - | - |
| uuid_v4_10k | hashset_fx | 27.9 | - | - | 27.9 | 0.00 | 0.00 | - | - | - |
| uuid_v4_100k | roaring128 | 50.5 | 34.2 | 50.5 | 52.7 | 0.13 | 0.13 | 0.09 | 18.0 | 18.0 |
| uuid_v4_100k | wrapped64 | 539 | 522 | 539 | 597 | 3.13 | 3.13 | 3.09 | 38.0 | 38.0 |
| uuid_v4_100k | btreeset | 27.1 | 18.2 | - | 29.2 | 0.13 | 0.13 | 0.09 | - | - |
| uuid_v4_100k | sortedvec | 16.0 | 16.0 | 16.0 | 16.0 | 0.00 | 0.00 | 0.00 | 16.0 | 16.0 |
| uuid_v4_100k | hashset | 22.3 | - | - | 22.3 | 0.00 | 0.00 | - | - | - |
| uuid_v4_100k | hashset_fx | 22.3 | - | - | 22.3 | 0.00 | 0.00 | - | - | - |
| uuid_v4_1M | roaring128 | 50.6 | 34.2 | 50.6 | 52.8 | 0.13 | 0.13 | 0.09 | 18.0 | 18.0 |
| uuid_v4_1M | wrapped64 | 539 | 522 | 539 | 597 | 3.13 | 3.13 | 3.09 | 38.0 | 38.0 |
| uuid_v4_1M | btreeset | 27.1 | 18.2 | - | 29.3 | 0.13 | 0.13 | 0.09 | - | - |
| uuid_v4_1M | sortedvec | 16.0 | 16.0 | 16.0 | 16.0 | 0.00 | 0.00 | 0.00 | 16.0 | 16.0 |
| uuid_v4_1M | hashset | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| uuid_v4_1M | hashset_fx | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| uuid_v7_1M | roaring128 | 50.6 | 34.2 | 50.6 | 52.7 | 0.13 | 0.13 | 0.09 | 18.0 | 18.0 |
| uuid_v7_1M | wrapped64 | 269 | 265 | 269 | 314 | 2.31 | 2.31 | 2.30 | 26.0 | 26.0 |
| uuid_v7_1M | btreeset | 27.1 | 18.2 | - | 29.3 | 0.13 | 0.13 | 0.09 | - | - |
| uuid_v7_1M | sortedvec | 16.0 | 16.0 | 16.0 | 16.0 | 0.00 | 0.00 | 0.00 | 16.0 | 16.0 |
| uuid_v7_1M | hashset | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| uuid_v7_1M | hashset_fx | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| dense_runs_1M | roaring128 | 0.17 | 0.17 | 0.00 | 0.17 | 0.00 | 0.00 | 0.00 | 0.16 | 0.00 |
| dense_runs_1M | wrapped64 | 0.17 | 0.17 | 0.00 | 0.17 | 0.00 | 0.00 | 0.00 | 0.16 | 0.00 |
| dense_runs_1M | btreeset | 27.1 | 18.2 | - | 29.3 | 0.13 | 0.13 | 0.09 | - | - |
| dense_runs_1M | sortedvec | 16.0 | 16.0 | 16.0 | 16.0 | 0.00 | 0.00 | 0.00 | 16.0 | 16.0 |
| dense_runs_1M | hashset | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| dense_runs_1M | hashset_fx | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| dense_gaps50_1M | roaring128 | 0.28 | 0.28 | 0.28 | 0.28 | 0.00 | 0.00 | 0.00 | 0.28 | 0.28 |
| dense_gaps50_1M | wrapped64 | 0.28 | 0.28 | 0.28 | 0.28 | 0.00 | 0.00 | 0.00 | 0.28 | 0.28 |
| dense_gaps50_1M | btreeset | 27.1 | 18.2 | - | 29.3 | 0.13 | 0.13 | 0.09 | - | - |
| dense_gaps50_1M | sortedvec | 16.0 | 16.0 | 16.0 | 16.0 | 0.00 | 0.00 | 0.00 | 16.0 | 16.0 |
| dense_gaps50_1M | hashset | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| dense_gaps50_1M | hashset_fx | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| dense_small_1M | roaring128 | 2.26 | 2.25 | 2.26 | 2.27 | 0.00 | 0.00 | 0.00 | 2.01 | 2.01 |
| dense_small_1M | wrapped64 | 2.25 | 2.25 | 2.25 | 2.26 | 0.00 | 0.01 | 0.01 | 2.00 | 2.00 |
| dense_small_1M | btreeset | 27.1 | 18.2 | - | 29.3 | 0.13 | 0.13 | 0.09 | - | - |
| dense_small_1M | sortedvec | 16.0 | 16.0 | 16.0 | 16.0 | 0.00 | 0.00 | 0.00 | 16.0 | 16.0 |
| dense_small_1M | hashset | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| dense_small_1M | hashset_fx | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| mixed_1M | roaring128 | 20.8 | 14.3 | 20.8 | 21.7 | 0.05 | 0.05 | 0.04 | 7.74 | 7.73 |
| mixed_1M | wrapped64 | 162 | 158 | 162 | 183 | 1.09 | 1.09 | 1.08 | 13.3 | 13.3 |
| mixed_1M | btreeset | 27.1 | 18.2 | - | 29.3 | 0.13 | 0.13 | 0.09 | - | - |
| mixed_1M | sortedvec | 16.0 | 16.0 | 16.0 | 16.0 | 0.00 | 0.00 | 0.00 | 16.0 | 16.0 |
| mixed_1M | hashset | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| mixed_1M | hashset_fx | 35.7 | - | - | 35.7 | 0.00 | 0.00 | - | - | - |
| random_u64_1M | roaring128 | 50.6 | 34.2 | 50.6 | 52.8 | 0.13 | 0.13 | 0.09 | 18.0 | 18.0 |
| random_u64_1M | treemap | 212 | 224 | 212 | 254 | 2.13 | 2.13 | 2.17 | 22.0 | 22.0 |
| dense_runs_u64_1M | roaring128 | 0.16 | 0.16 | 0.00 | 0.16 | 0.00 | 0.00 | 0.00 | 0.16 | 0.00 |
| dense_runs_u64_1M | treemap | 0.16 | 0.16 | 0.00 | 0.16 | 0.00 | 0.00 | 0.00 | 0.16 | 0.00 |
| dense_gaps50_u64_1M | roaring128 | 0.28 | 0.28 | 0.28 | 0.28 | 0.00 | 0.00 | 0.00 | 0.28 | 0.28 |
| dense_gaps50_u64_1M | treemap | 0.28 | 0.28 | 0.28 | 0.28 | 0.00 | 0.00 | 0.00 | 0.28 | 0.28 |
| dense_small_u64_1M | roaring128 | 2.30 | 2.29 | 2.30 | 2.30 | 0.00 | 0.00 | 0.00 | 2.01 | 2.01 |
| dense_small_u64_1M | treemap | 2.29 | 2.29 | 2.29 | 2.30 | 0.00 | 0.01 | 0.01 | 2.00 | 2.00 |

roaring128 container composition (insert build, then after optimize):

| workload | containers | array / bitmap / run | after optimize | values/container |
|---|--:|--:|--:|--:|
| uuid_v4_10k | 10000 | 10000 / 0 / 0 | 10000 / 0 / 0 | 1.0 |
| uuid_v4_100k | 100000 | 100000 / 0 / 0 | 100000 / 0 / 0 | 1.0 |
| uuid_v4_1M | 1000000 | 1000000 / 0 / 0 | 1000000 / 0 / 0 | 1.0 |
| uuid_v7_1M | 1000000 | 1000000 / 0 / 0 | 1000000 / 0 / 0 | 1.0 |
| dense_runs_1M | 20 | 0 / 20 / 0 | 0 / 0 / 20 | 50000.0 |
| dense_gaps50_1M | 34 | 0 / 34 / 0 | 0 / 34 / 0 | 29411.8 |
| dense_small_1M | 500 | 500 / 0 / 0 | 500 / 0 / 0 | 2000.0 |
| mixed_1M | 400120 | 400104 / 16 / 0 | 400103 / 10 / 7 | 2.5 |
| random_u64_1M | 1000000 | 1000000 / 0 / 0 | 1000000 / 0 / 0 | 1.0 |
| dense_runs_u64_1M | 19 | 0 / 19 / 0 | 0 / 0 / 19 | 52631.6 |
| dense_gaps50_u64_1M | 34 | 0 / 34 / 0 | 0 / 34 / 0 | 29411.8 |
| dense_small_u64_1M | 500 | 500 / 0 / 0 | 500 / 0 / 0 | 2000.0 |

`sortedvec` is built by sorting (its insert column is a sort build); its serialized form is a raw little-endian dump.
