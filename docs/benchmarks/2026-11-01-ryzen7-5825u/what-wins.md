# What wins when

Source: report.md. Fastest implementation per operation and kind of data; a tie means
the intervals overlap. `n/m` = won n of the m workloads of that kind. Margins: "ahead" is the
runner-up's time over roaring128's; "slower" is roaring128's time over the winner's.
u64 data compares roaring128 with RoaringTreemap only. † holds by construction.

| operation | dense | mixed | sparse UUIDs | u64 data |
|---|---|---|---|---|
| insert | wrapped64 2/3, FxHashSet 1/3 (roaring128 1.2–2.5x slower) | FxHashSet (roaring128 2.8x slower) | FxHashSet 4/5, sorted Vec 1/5 (roaring128 4.4–6.6x slower) | RoaringTreemap 3/4, roaring128 1/4 (roaring128 1.2x slower) |
| from_iter | roaring128 2/3, sorted Vec 1/3 (roaring128 2.6x slower) | sorted Vec (roaring128 3.6x slower) | sorted Vec 2/3, FxHashSet 1/3 (roaring128 5.6–18.6x slower) | RoaringTreemap (roaring128 1.3x slower) |
| from_sorted | sorted Vec (roaring128 4.4–4.8x slower) | sorted Vec (roaring128 15.9x slower) | sorted Vec (roaring128 25.4–56.0x slower) | roaring128 (1.4–5.9x ahead) |
| contains_hit | FxHashSet (roaring128 1.1–11.7x slower) | FxHashSet (roaring128 11.2x slower) | FxHashSet (roaring128 18.2–22.4x slower) | RoaringTreemap (roaring128 1.1–1.6x slower) |
| contains_miss | FxHashSet (roaring128 2.8–18.5x slower) | FxHashSet (roaring128 42.2x slower) | FxHashSet (roaring128 29.0–54.6x slower) | RoaringTreemap (roaring128 1.2x slower) |
| remove_all | FxHashSet (roaring128 3.1–9.8x slower) | FxHashSet (roaring128 13.4x slower) | FxHashSet (roaring128 15.4–16.3x slower) |  |
| iter | sorted Vec 2/4, roaring128 2/4 (roaring128 1.2–1.5x slower) | sorted Vec (roaring128 15.6x slower) | sorted Vec (roaring128 30.1–39.3x slower) | roaring128 (4.8–16.3x ahead) |
| range | sorted Vec (roaring128 17.6–34.0x slower) | sorted Vec (roaring128 173.3x slower) | sorted Vec (roaring128 352.8–369.9x slower) |  |
| len † | sorted Vec (roaring128 1.1x slower) |  | sorted Vec (roaring128 1.2x slower) |  |
| rank † | sorted Vec (roaring128 6.3x slower) | sorted Vec (roaring128 67782.4x slower) | sorted Vec (roaring128 354811.7x slower) | roaring128 (2.8x ahead) |
| select † | sorted Vec (roaring128 182.7x slower) | sorted Vec (roaring128 2513736.3x slower) | sorted Vec (roaring128 10416088.8x slower) | roaring128 (8.3x ahead) |
| serialize | roaring128 2/3, wrapped64 1/3 (roaring128 1.1x slower) | sorted Vec (roaring128 10.7x slower) | sorted Vec (roaring128 26.7–29.0x slower) | roaring128 3/4, RoaringTreemap 1/4 (roaring128 1.1x slower) |
| deserialize | roaring128 2/3, wrapped64 1/3 (roaring128 1.2x slower) | sorted Vec (roaring128 5.1x slower) | sorted Vec (roaring128 20.0–20.4x slower) | roaring128 3/4, RoaringTreemap 1/4 (roaring128 1.4x slower) |
| and | roaring128 (1.1–5.6x ahead) | sorted Vec (roaring128 2.3x slower) | sorted Vec (roaring128 4.0–6.1x slower) | roaring128 (1.5–9.9x ahead) |
| or | roaring128 2/3, wrapped64 1/3 (roaring128 1.1x slower) | sorted Vec (roaring128 3.7x slower) | sorted Vec (roaring128 9.3–13.4x slower) | roaring128 (1.7–8.6x ahead) |
| sub | roaring128 (1.0–6.9x ahead) | sorted Vec (roaring128 2.6x slower) | sorted Vec (roaring128 6.3–10.7x slower) | roaring128 (1.5–9.1x ahead) |
| xor | roaring128 (1.4–5.7x ahead) | sorted Vec (roaring128 3.6x slower) | sorted Vec (roaring128 8.6–13.2x slower) | roaring128 (1.4–5.6x ahead) |
| and_assign | roaring128 1/2, wrapped64 1/2 (roaring128 1.3x slower) | sorted Vec (roaring128 3.1x slower) | sorted Vec (roaring128 6.4–8.1x slower) | roaring128 (2.2x ahead) |
| or_assign | roaring128 1/2, wrapped64 1/2 (roaring128 1.2x slower) | sorted Vec (roaring128 3.0x slower) | sorted Vec (roaring128 7.4–11.5x slower) | roaring128 (2.4x ahead) |
| sub_assign | roaring128 1/2, wrapped64 1/2 (roaring128 1.1x slower) | sorted Vec (roaring128 3.1x slower) | sorted Vec (roaring128 6.7–7.9x slower) | roaring128 (2.4x ahead) |
| xor_assign | roaring128 (1.7–2.6x ahead) | sorted Vec (roaring128 4.4x slower) | sorted Vec (roaring128 11.8–12.0x slower) | roaring128 (2.4x ahead) |
| intersection_len | roaring128 (1.4–6.7x ahead) | sorted Vec (roaring128 1.8x slower) | sorted Vec (roaring128 4.3–5.8x slower) | roaring128 (3.0–3.2x ahead) |
| is_subset | roaring128 (1.6x ahead) | sorted Vec (roaring128 3.3x slower) | sorted Vec (roaring128 5.6–9.4x slower) |  |
| multi_union16 | roaring128 (544.9x ahead) |  | sorted Vec (roaring128 2.6x slower) | RoaringTreemap (roaring128 1.5x slower) |
| multi_intersection16 | roaring128 (243.5x ahead) |  | sorted Vec (roaring128 15.8x slower) | RoaringTreemap (roaring128 1.7x slower) |

## Fastest-or-tied count per kind (excluding †)

| kind | roaring128 fastest or tied | another implementation fastest |
|---|--:|--:|
| dense | 30 | 30 |
| mixed | 0 | 20 |
| sparse UUIDs | 0 | 66 |
| u64 data | 29 | 12 |

## Per workload

| operation | workload | kind | fastest | tied with it | margin | roaring128 / fastest |
|---|---|---|---|---|--:|--:|
| insert | dense_gaps50_1M | dense | wrapped64 |  | 1.26x | 1.26x |
| insert | dense_gaps50_u64_1M | u64 data | RoaringTreemap |  | 1.19x | 1.19x |
| insert | dense_runs_1M | dense | wrapped64 |  | 1.21x | 1.21x |
| insert | dense_runs_u64_1M | u64 data | RoaringTreemap |  | 1.18x | 1.18x |
| insert | dense_small_1M | dense | FxHashSet |  | 2.12x | 2.52x |
| insert | dense_small_u64_1M | u64 data | RoaringTreemap |  | 1.22x | 1.22x |
| insert | mixed_1M | mixed | FxHashSet |  | 2.31x | 2.78x |
| insert | random_u64_1M | u64 data | roaring128 |  | 2.15x | 1.00x |
| insert | uuid_v4_100k | sparse UUIDs | FxHashSet |  | 3.52x | 6.56x |
| insert | uuid_v4_10k | sparse UUIDs | FxHashSet |  | 2.87x | 4.48x |
| insert | uuid_v4_1M | sparse UUIDs | FxHashSet |  | 2.61x | 5.80x |
| insert | uuid_v7_1M | sparse UUIDs | FxHashSet |  | 2.38x | 6.03x |
| insert | uuid_v7_1M_inorder | sparse UUIDs | sorted Vec |  | 2.80x | 4.41x |
| from_iter | dense_gaps50_1M | dense | roaring128 |  | 1.07x | 1.00x |
| from_iter | dense_runs_1M | dense | roaring128 |  | 1.13x | 1.00x |
| from_iter | dense_small_1M | dense | sorted Vec |  | 1.28x | 2.65x |
| from_iter | mixed_1M | mixed | sorted Vec |  | 1.46x | 3.64x |
| from_iter | random_u64_1M | u64 data | RoaringTreemap |  | 1.32x | 1.32x |
| from_iter | uuid_v4_10k | sparse UUIDs | FxHashSet |  | 2.18x | 18.59x |
| from_iter | uuid_v4_1M | sparse UUIDs | sorted Vec |  | 1.26x | 6.24x |
| from_iter | uuid_v7_1M | sparse UUIDs | sorted Vec |  | 1.30x | 5.59x |
| from_sorted | dense_gaps50_1M | dense | sorted Vec |  | 4.77x | 4.77x |
| from_sorted | dense_gaps50_u64_1M | u64 data | roaring128 |  | 1.37x | 1.00x |
| from_sorted | dense_runs_1M | dense | sorted Vec |  | 4.43x | 4.43x |
| from_sorted | dense_runs_u64_1M | u64 data | roaring128 |  | 1.37x | 1.00x |
| from_sorted | dense_small_1M | dense | sorted Vec |  | 4.67x | 4.67x |
| from_sorted | dense_small_u64_1M | u64 data | roaring128 |  | 1.43x | 1.00x |
| from_sorted | mixed_1M | mixed | sorted Vec |  | 5.59x | 15.95x |
| from_sorted | random_u64_1M | u64 data | roaring128 |  | 5.95x | 1.00x |
| from_sorted | uuid_v4_10k | sparse UUIDs | sorted Vec |  | 9.87x | 56.03x |
| from_sorted | uuid_v4_1M | sparse UUIDs | sorted Vec |  | 9.00x | 46.90x |
| from_sorted | uuid_v7_1M | sparse UUIDs | sorted Vec |  | 5.01x | 25.45x |
| contains_hit | dense_gaps50_1M | dense | FxHashSet |  | 1.48x | 1.78x |
| contains_hit | dense_gaps50_u64_1M | u64 data | RoaringTreemap |  | 1.28x | 1.28x |
| contains_hit | dense_runs_1M | dense | FxHashSet |  | 1.14x | 1.14x |
| contains_hit | dense_runs_1M_opt | dense | FxHashSet |  | 1.51x | 1.75x |
| contains_hit | dense_small_1M | dense | FxHashSet |  | 6.87x | 11.70x |
| contains_hit | dense_small_u64_1M | u64 data | RoaringTreemap |  | 1.63x | 1.63x |
| contains_hit | mixed_1M | mixed | FxHashSet |  | 6.73x | 11.21x |
| contains_hit | random_u64_1M | u64 data | RoaringTreemap |  | 1.14x | 1.14x |
| contains_hit | uuid_v4_100k | sparse UUIDs | FxHashSet |  | 7.37x | 18.20x |
| contains_hit | uuid_v4_10k | sparse UUIDs | FxHashSet |  | 8.78x | 18.35x |
| contains_hit | uuid_v4_1M | sparse UUIDs | FxHashSet |  | 6.86x | 20.07x |
| contains_hit | uuid_v7_1M | sparse UUIDs | FxHashSet |  | 7.06x | 22.45x |
| contains_miss | dense_gaps50_1M | dense | FxHashSet |  | 1.97x | 3.24x |
| contains_miss | dense_runs_1M | dense | FxHashSet |  | 1.61x | 2.83x |
| contains_miss | dense_runs_1M_opt | dense | FxHashSet |  | 1.63x | 2.93x |
| contains_miss | dense_small_1M | dense | FxHashSet |  | 8.66x | 18.49x |
| contains_miss | mixed_1M | mixed | FxHashSet |  | 8.87x | 42.17x |
| contains_miss | random_u64_1M | u64 data | RoaringTreemap |  | 1.21x | 1.21x |
| contains_miss | uuid_v4_10k | sparse UUIDs | FxHashSet |  | 10.87x | 28.97x |
| contains_miss | uuid_v4_1M | sparse UUIDs | FxHashSet |  | 9.02x | 51.61x |
| contains_miss | uuid_v7_1M | sparse UUIDs | FxHashSet |  | 9.04x | 54.65x |
| remove_all | dense_gaps50_100k | dense | FxHashSet |  | 3.30x | 3.30x |
| remove_all | dense_runs_100k | dense | FxHashSet |  | 3.15x | 3.15x |
| remove_all | dense_small_100k | dense | FxHashSet |  | 5.38x | 9.79x |
| remove_all | mixed_100k | mixed | FxHashSet |  | 5.31x | 13.42x |
| remove_all | uuid_v4_100k | sparse UUIDs | FxHashSet |  | 5.86x | 16.32x |
| remove_all | uuid_v7_100k | sparse UUIDs | FxHashSet |  | 4.83x | 15.39x |
| iter | dense_gaps50_1M | dense | sorted Vec |  | 1.50x | 1.50x |
| iter | dense_gaps50_u64_1M | u64 data | roaring128 |  | 6.43x | 1.00x |
| iter | dense_runs_1M | dense | sorted Vec |  | 1.23x | 1.23x |
| iter | dense_runs_1M_opt | dense | roaring128 |  | 1.43x | 1.00x |
| iter | dense_runs_u64_1M | u64 data | roaring128 |  | 7.90x | 1.00x |
| iter | dense_small_1M | dense | roaring128 |  | 1.58x | 1.00x |
| iter | dense_small_u64_1M | u64 data | roaring128 |  | 16.33x | 1.00x |
| iter | mixed_1M | mixed | sorted Vec |  | 3.01x | 15.55x |
| iter | random_u64_1M | u64 data | roaring128 |  | 4.81x | 1.00x |
| iter | uuid_v4_10k | sparse UUIDs | sorted Vec |  | 2.65x | 30.09x |
| iter | uuid_v4_1M | sparse UUIDs | sorted Vec |  | 3.08x | 39.30x |
| iter | uuid_v7_1M | sparse UUIDs | sorted Vec |  | 2.98x | 38.47x |
| range | dense_gaps50_1M | dense | sorted Vec |  | 33.99x | 33.99x |
| range | dense_runs_1M | dense | sorted Vec |  | 30.66x | 30.66x |
| range | dense_runs_1M_opt | dense | sorted Vec |  | 17.55x | 17.55x |
| range | dense_small_1M | dense | sorted Vec |  | 19.68x | 19.68x |
| range | mixed_1M | mixed | sorted Vec |  | 58.89x | 173.32x |
| range | uuid_v4_1M | sparse UUIDs | sorted Vec |  | 61.29x | 352.82x |
| range | uuid_v7_1M | sparse UUIDs | sorted Vec |  | 58.12x | 369.86x |
| len | dense_gaps50_1M | dense | sorted Vec |  | 1.09x | 1.09x |
| len | uuid_v4_1M | sparse UUIDs | sorted Vec |  | 1.24x | 1.24x |
| rank | dense_gaps50_1M | dense | sorted Vec |  | 6.29x | 6.29x |
| rank | dense_gaps50_u64_1M | u64 data | roaring128 |  | 2.80x | 1.00x |
| rank | mixed_1M | mixed | sorted Vec |  | 67782.43x | 67782.43x |
| rank | uuid_v4_1M | sparse UUIDs | sorted Vec |  | 354811.72x | 354811.72x |
| select | dense_gaps50_1M | dense | sorted Vec |  | 182.71x | 182.71x |
| select | dense_gaps50_u64_1M | u64 data | roaring128 |  | 8.31x | 1.00x |
| select | mixed_1M | mixed | sorted Vec |  | 2513736.26x | 2513736.26x |
| select | uuid_v4_1M | sparse UUIDs | sorted Vec |  | 10416088.77x | 10416088.77x |
| serialize | dense_gaps50_1M | dense | roaring128 |  | 1.66x | 1.00x |
| serialize | dense_gaps50_u64_1M | u64 data | roaring128 |  | 1.55x | 1.00x |
| serialize | dense_runs_1M | dense | wrapped64 |  | 1.13x | 1.13x |
| serialize | dense_runs_u64_1M | u64 data | RoaringTreemap |  | 1.09x | 1.09x |
| serialize | dense_small_1M | dense | roaring128 |  | 8.61x | 1.00x |
| serialize | dense_small_u64_1M | u64 data | roaring128 |  | 9.04x | 1.00x |
| serialize | mixed_1M | mixed | sorted Vec |  | 10.68x | 10.68x |
| serialize | random_u64_1M | u64 data | roaring128 |  | 1.63x | 1.00x |
| serialize | uuid_v4_1M | sparse UUIDs | sorted Vec |  | 28.95x | 28.95x |
| serialize | uuid_v7_1M | sparse UUIDs | sorted Vec |  | 26.71x | 26.71x |
| deserialize | dense_gaps50_1M | dense | roaring128 |  | 1.54x | 1.00x |
| deserialize | dense_gaps50_u64_1M | u64 data | roaring128 |  | 1.50x | 1.00x |
| deserialize | dense_runs_1M | dense | wrapped64 |  | 1.23x | 1.23x |
| deserialize | dense_runs_u64_1M | u64 data | RoaringTreemap |  | 1.38x | 1.38x |
| deserialize | dense_small_1M | dense | roaring128 |  | 2.69x | 1.00x |
| deserialize | dense_small_u64_1M | u64 data | roaring128 |  | 2.62x | 1.00x |
| deserialize | mixed_1M | mixed | sorted Vec |  | 5.13x | 5.13x |
| deserialize | random_u64_1M | u64 data | roaring128 |  | 1.91x | 1.00x |
| deserialize | uuid_v4_1M | sparse UUIDs | sorted Vec |  | 19.97x | 19.97x |
| deserialize | uuid_v7_1M | sparse UUIDs | sorted Vec |  | 18.98x | 20.38x |
| and | dense_gaps50_1M_indep | dense | roaring128 |  | 1.54x | 1.00x |
| and | dense_gaps50_u64_1M_indep | u64 data | roaring128 |  | 1.50x | 1.00x |
| and | dense_runs_1M_indep_opt | dense | roaring128 |  | 1.08x | 1.00x |
| and | dense_small_1M_indep | dense | roaring128 |  | 5.57x | 1.00x |
| and | mixed_500k_ov50 | mixed | sorted Vec |  | 2.28x | 2.28x |
| and | random_u64_500k_ov50 | u64 data | roaring128 |  | 9.93x | 1.00x |
| and | uuid_v4_1M_vs_1k | sparse UUIDs | sorted Vec |  | 4.95x | 6.06x |
| and | uuid_v4_500k_ov1 | sparse UUIDs | sorted Vec |  | 3.95x | 3.95x |
| and | uuid_v4_500k_ov50 | sparse UUIDs | sorted Vec |  | 5.14x | 5.85x |
| and | uuid_v7_500k_ov50 | sparse UUIDs | sorted Vec |  | 4.74x | 5.99x |
| or | dense_gaps50_1M_indep | dense | roaring128 |  | 1.71x | 1.00x |
| or | dense_gaps50_u64_1M_indep | u64 data | roaring128 |  | 1.68x | 1.00x |
| or | dense_runs_1M_indep_opt | dense | wrapped64 |  | 1.11x | 1.11x |
| or | dense_small_1M_indep | dense | roaring128 |  | 6.78x | 1.00x |
| or | mixed_500k_ov50 | mixed | sorted Vec |  | 3.70x | 3.70x |
| or | random_u64_500k_ov50 | u64 data | roaring128 |  | 8.60x | 1.00x |
| or | uuid_v4_1M_vs_1k | sparse UUIDs | sorted Vec |  | 11.68x | 11.68x |
| or | uuid_v4_500k_ov1 | sparse UUIDs | sorted Vec |  | 5.08x | 13.42x |
| or | uuid_v4_500k_ov50 | sparse UUIDs | sorted Vec |  | 6.25x | 9.29x |
| or | uuid_v7_500k_ov50 | sparse UUIDs | sorted Vec |  | 5.13x | 9.37x |
| sub | dense_gaps50_1M_indep | dense | roaring128 |  | 1.45x | 1.00x |
| sub | dense_gaps50_u64_1M_indep | u64 data | roaring128 |  | 1.51x | 1.00x |
| sub | dense_runs_1M_indep_opt | dense | roaring128 |  | 1.05x | 1.00x |
| sub | dense_small_1M_indep | dense | roaring128 |  | 6.90x | 1.00x |
| sub | mixed_500k_ov50 | mixed | sorted Vec |  | 2.57x | 2.57x |
| sub | random_u64_500k_ov50 | u64 data | roaring128 |  | 9.12x | 1.00x |
| sub | uuid_v4_1M_vs_1k | sparse UUIDs | sorted Vec |  | 10.25x | 10.71x |
| sub | uuid_v4_500k_ov1 | sparse UUIDs | sorted Vec |  | 4.70x | 6.33x |
| sub | uuid_v4_500k_ov50 | sparse UUIDs | sorted Vec |  | 5.07x | 6.51x |
| sub | uuid_v7_500k_ov50 | sparse UUIDs | sorted Vec |  | 4.70x | 6.53x |
| xor | dense_gaps50_1M_indep | dense | roaring128 |  | 1.43x | 1.00x |
| xor | dense_gaps50_u64_1M_indep | u64 data | roaring128 |  | 1.38x | 1.00x |
| xor | dense_runs_1M_indep_opt | dense | roaring128 |  | 1.71x | 1.00x |
| xor | dense_small_1M_indep | dense | roaring128 |  | 5.70x | 1.00x |
| xor | mixed_500k_ov50 | mixed | sorted Vec |  | 3.59x | 3.59x |
| xor | random_u64_500k_ov50 | u64 data | roaring128 |  | 5.62x | 1.00x |
| xor | uuid_v4_1M_vs_1k | sparse UUIDs | sorted Vec |  | 11.56x | 11.70x |
| xor | uuid_v4_500k_ov1 | sparse UUIDs | sorted Vec |  | 4.83x | 13.17x |
| xor | uuid_v4_500k_ov50 | sparse UUIDs | sorted Vec |  | 5.67x | 8.65x |
| xor | uuid_v7_500k_ov50 | sparse UUIDs | sorted Vec |  | 5.05x | 8.83x |
| and_assign | dense_gaps50_1M_indep | dense | roaring128 |  | 2.14x | 1.00x |
| and_assign | dense_gaps50_u64_1M_indep | u64 data | roaring128 |  | 2.15x | 1.00x |
| and_assign | dense_runs_1M_indep_opt | dense | wrapped64 |  | 1.30x | 1.30x |
| and_assign | mixed_500k_ov50 | mixed | sorted Vec |  | 3.11x | 3.11x |
| and_assign | uuid_v4_1M_vs_1k | sparse UUIDs | sorted Vec |  | 4.95x | 6.42x |
| and_assign | uuid_v4_500k_ov50 | sparse UUIDs | sorted Vec |  | 8.01x | 8.01x |
| and_assign | uuid_v7_500k_ov50 | sparse UUIDs | sorted Vec |  | 8.07x | 8.07x |
| or_assign | dense_gaps50_1M_indep | dense | roaring128 |  | 2.44x | 1.00x |
| or_assign | dense_gaps50_u64_1M_indep | u64 data | roaring128 |  | 2.36x | 1.00x |
| or_assign | dense_runs_1M_indep_opt | dense | wrapped64 |  | 1.17x | 1.17x |
| or_assign | mixed_500k_ov50 | mixed | sorted Vec |  | 3.04x | 3.04x |
| or_assign | uuid_v4_1M_vs_1k | sparse UUIDs | sorted Vec |  | 11.55x | 11.55x |
| or_assign | uuid_v4_500k_ov50 | sparse UUIDs | sorted Vec |  | 7.05x | 7.57x |
| or_assign | uuid_v7_500k_ov50 | sparse UUIDs | sorted Vec |  | 6.52x | 7.42x |
| sub_assign | dense_gaps50_1M_indep | dense | roaring128 |  | 2.37x | 1.00x |
| sub_assign | dense_gaps50_u64_1M_indep | u64 data | roaring128 |  | 2.37x | 1.00x |
| sub_assign | dense_runs_1M_indep_opt | dense | wrapped64 |  | 1.08x | 1.08x |
| sub_assign | mixed_500k_ov50 | mixed | sorted Vec |  | 3.13x | 3.13x |
| sub_assign | uuid_v4_1M_vs_1k | sparse UUIDs | sorted Vec |  | 5.36x | 6.73x |
| sub_assign | uuid_v4_500k_ov50 | sparse UUIDs | sorted Vec |  | 7.89x | 7.89x |
| sub_assign | uuid_v7_500k_ov50 | sparse UUIDs | sorted Vec |  | 7.91x | 7.91x |
| xor_assign | dense_gaps50_1M_indep | dense | roaring128 |  | 2.55x | 1.00x |
| xor_assign | dense_gaps50_u64_1M_indep | u64 data | roaring128 |  | 2.39x | 1.00x |
| xor_assign | dense_runs_1M_indep_opt | dense | roaring128 |  | 1.70x | 1.00x |
| xor_assign | mixed_500k_ov50 | mixed | sorted Vec |  | 4.37x | 4.37x |
| xor_assign | uuid_v4_1M_vs_1k | sparse UUIDs | sorted Vec |  | 11.77x | 11.77x |
| xor_assign | uuid_v4_500k_ov50 | sparse UUIDs | sorted Vec |  | 10.06x | 11.87x |
| xor_assign | uuid_v7_500k_ov50 | sparse UUIDs | sorted Vec |  | 10.10x | 12.00x |
| intersection_len | dense_gaps50_1M_indep | dense | roaring128 |  | 3.03x | 1.00x |
| intersection_len | dense_gaps50_u64_1M_indep | u64 data | roaring128 |  | 2.99x | 1.00x |
| intersection_len | dense_runs_1M_indep_opt | dense | roaring128 |  | 1.37x | 1.00x |
| intersection_len | dense_small_1M_indep | dense | roaring128 |  | 6.72x | 1.00x |
| intersection_len | mixed_500k_ov50 | mixed | sorted Vec |  | 1.84x | 1.84x |
| intersection_len | random_u64_500k_ov50 | u64 data | roaring128 |  | 3.18x | 1.00x |
| intersection_len | uuid_v4_1M_vs_1k | sparse UUIDs | sorted Vec |  | 5.08x | 5.80x |
| intersection_len | uuid_v4_500k_ov1 | sparse UUIDs | sorted Vec |  | 3.68x | 4.33x |
| intersection_len | uuid_v4_500k_ov50 | sparse UUIDs | sorted Vec |  | 4.07x | 5.00x |
| intersection_len | uuid_v7_500k_ov50 | sparse UUIDs | sorted Vec |  | 4.16x | 5.01x |
| is_subset | dense_gaps50_1M_indep_sub | dense | roaring128 |  | 1.56x | 1.00x |
| is_subset | mixed_500k_ov50_sub | mixed | sorted Vec |  | 3.27x | 3.27x |
| is_subset | uuid_v4_1M_vs_1k_sub | sparse UUIDs | sorted Vec |  | 4.27x | 5.61x |
| is_subset | uuid_v4_500k_ov50_sub | sparse UUIDs | sorted Vec |  | 7.34x | 9.35x |
| is_subset | uuid_v7_500k_ov50_sub | sparse UUIDs | sorted Vec |  | 7.47x | 9.40x |
| multi_union16 | dense_runs_16x100k_opt | dense | roaring128 |  | 544.89x | 1.00x |
| multi_union16 | dense_runs_u64_16x100k_opt | u64 data | RoaringTreemap |  | 1.49x | 1.49x |
| multi_union16 | uuid_v4_16x100k | sparse UUIDs | sorted Vec |  | 2.47x | 2.62x |
| multi_intersection16 | dense_runs_16x100k_opt | dense | roaring128 |  | 243.50x | 1.00x |
| multi_intersection16 | dense_runs_u64_16x100k_opt | u64 data | RoaringTreemap |  | 1.66x | 1.66x |
| multi_intersection16 | uuid_v4_16x100k | sparse UUIDs | sorted Vec |  | 7.18x | 15.83x |
