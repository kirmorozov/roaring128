# roaring128

[![CI](https://github.com/kirmorozov/roaring128/actions/workflows/ci.yml/badge.svg)](https://github.com/kirmorozov/roaring128/actions/workflows/ci.yml)

Native 128-bit [Roaring bitmaps](https://roaringbitmap.org/) for Rust. `RoaringBitmap128` is a
compressed set of `u128` values, built so that UUIDs and other 128-bit identifiers can be stored
in a Roaring bitmap directly, with exact set semantics and the usual Roaring set algebra. The API
mirrors the [`roaring`](https://docs.rs/roaring) crate, and the container kernels are written for
AVX2, AVX-512 and NEON, selected at runtime.

> **Status:** early development. The public API, the implementation (containers, SIMD kernels,
> serialization, test suites) and the benchmark harness are in place. Not on crates.io yet, not
> ready for production use.

## Why

Existing Roaring libraries are 32-bit (`RoaringBitmap`) or 64-bit (`RoaringTreemap` in Rust,
`roaring64` in CRoaring, `Roaring64NavigableMap` in Java). A UUID does not fit, so today it has to
be squeezed into one of them in one of these ways:

- **Dictionary to surrogate IDs.** Map each UUID to a dense `u32`/`u64` and store those. This
  means extra storage for the dictionary (in both directions, to turn results back into UUIDs),
  an extra lookup on every insert and query, and a dictionary that every shard, service and
  stored bitmap has to agree on.
- **Hash down to 64 bits.** Collisions give wrong answers: false positives in `contains` and in
  intersections, and wrong cardinalities. The birthday bound makes them likely at scale (about a
  39% chance of at least one collision among 2^32 values). The members can't be listed as UUIDs
  either.
- **Wrap the 64-bit bitmap.** Exact, but expensive for UUIDs; see below.

`roaring128` removes the indirection: insert `Uuid::as_u128()` (or use the `uuid` feature's
`insert_uuid`, `contains_uuid`, `iter_uuids`, ...) and get union, intersection, difference and
symmetric difference, cardinality-only versions of them, many-way operations, rank/select, range
queries and serialization on UUID sets.

### Why not wrap roaring64?

The obvious 128-bit set built from existing parts is a two-level map:

```rust
// high 64 bits -> 64-bit treemap of the low 64 bits
type Wrapped = BTreeMap<u64, roaring::RoaringTreemap>;
// where RoaringTreemap = BTreeMap<u32, RoaringBitmap>
//       RoaringBitmap  = Vec<Container { key: u16, store }>
//       array store    = Vec<u16>
```

With random UUIDs every value has its own high 64 bits, so every level holds exactly one entry
per value:

| | `BTreeMap<u64, RoaringTreemap>` | `RoaringBitmap128` |
|---|---|---|
| Heap allocations per sparse value | 3: the inner treemap's B-tree leaf node (a full node for one entry), the bitmap's `Vec<Container>`, the array's `Vec<u16>`; plus an entry in the outer B-tree | none: one 16-byte key and one 16-byte container slot in a shared B-tree node |
| `contains` | outer B-tree search, inner B-tree search, binary search over containers, then the container | one B-tree search, then the container |
| `a & b`, `a \| b`, ... | merge the outer maps, recurse into each pair of inner treemaps, then into their container vectors | one merge loop over the keys of both B-trees, one container operation per common key |
| `len()` | `roaring` sums the cardinalities of all containers: O(values) for sparse data | cached: O(1) |
| Container kernels | the `roaring` crate's (scalar on stable Rust; its `simd` feature needs nightly) | AVX2 / AVX-512 / NEON, dispatched at runtime |

For dense data both designs end up with the same Roaring containers; the remaining differences are
the single-level index and the SIMD container kernels. See [Benchmarks](#benchmarks) for the
measurements.

## When to use it

`roaring128` is a good fit when:

- **Your 128-bit IDs cluster:** a shared prefix with a sequence or counter in the low bits (a
  tenant or shard with a sequence, snowflake-style IDs under a prefix, UUIDv8 layouts with a
  counter), or ranges added with `insert_range`. Set operations were the fastest of everything
  measured in most cases, and memory and serialized size drop to 0.2–2.3 bytes per value against
  16 or more for a `Vec`, `BTreeSet` or `HashSet` of `u128`.
- **You need set algebra on 128-bit sets:** union, intersection, difference and symmetric
  difference, their cardinality-only forms and many-way versions, on clustered or mixed data.
- **You would otherwise wrap roaring64** (`BTreeMap<u64, RoaringTreemap>`): even on random UUIDs
  `roaring128` is 1.2–13x faster on set operations and 5–15x smaller.
- **Your code already uses the `roaring` crate** and needs 128-bit values: the API mirrors
  `RoaringTreemap`.
- **Your data is partly clustered and partly random and changes over time:** on mixed data
  `roaring128` beat `BTreeSet` (1.4–3.2x) and wrapped roaring64 (2.5–5.3x) on set operations;
  only a static sorted `Vec` was faster.

Something else fits better when:

- **The IDs are random UUIDs** (v4, or v7: with 62 random low bits, two of them practically never
  share a container) and the set changes over time: `BTreeSet<u128>` was faster on most
  operations and smaller (18–27 against 34–51 bytes per value), because every random UUID lands
  in a container of its own and Roaring compression has nothing to work with.
- **The set is built once and then only read:** a sorted `Vec<u128>` had 4–16x faster set
  operations, binary-search `rank` / `select`, and 16 bytes per value.
- **You only need membership** (`insert`, `contains`, `remove`): a hash set, such as
  `HashSet<u128>` with FxHash, was 4–55x faster on random UUIDs.
- **You exchange the data with other languages or tools:** the serialization format is
  `roaring128`'s own and nothing else reads it yet. A sorted array of 16-byte values is readable
  everywhere and was 20–29x faster to write and read for random UUIDs.

The numbers come from a first, provisional benchmark run; see [Benchmarks](#benchmarks) for the
conditions and the full what-wins-when table.

## Quick start

Not on crates.io yet; use the git repository:

```toml
[dependencies]
roaring128 = { git = "https://github.com/kirmorozov/roaring128" }
```

Plain `u128` values. Here the IDs have a dense low part (a tenant in the high 64 bits, a sequence
in the low 64 bits), which is where Roaring compression applies:

```rust
use roaring128::RoaringBitmap128;

let id = |tenant: u64, seq: u64| ((tenant as u128) << 64) | seq as u128;

let mut active = RoaringBitmap128::new();
active.insert_range(id(7, 0)..id(7, 100_000)); // stored as run containers
active.insert(id(9, 42));

let flagged = RoaringBitmap128::from([id(7, 10), id(7, 99_999), id(8, 1)]);

assert_eq!(active.len(), 100_001); // O(1)
assert_eq!(active.intersection_len(&flagged), 2); // the intersection is not materialized
assert_eq!((&active & &flagged).iter().collect::<Vec<_>>(), vec![id(7, 10), id(7, 99_999)]);
assert_eq!(active.range_cardinality(id(7, 0)..id(8, 0)), 100_000);
assert_eq!(active.select(100_000), Some(id(9, 42)));
```

UUIDs, with the `uuid` feature:

```toml
[dependencies]
roaring128 = { git = "https://github.com/kirmorozov/roaring128", features = ["uuid"] }
uuid = { version = "1", features = ["v7"] }
```

```rust
use roaring128::{MultiOps, RoaringBitmap128};
use uuid::Uuid;

let (alice, bob, carol) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());

// Straight from UUIDs: no dictionary, no hashing.
let opened_email: RoaringBitmap128 = [alice, bob].into_iter().collect();
let mut purchased = RoaringBitmap128::new();
purchased.insert_uuid(bob);
purchased.insert_uuid(carol);

assert_eq!(opened_email.intersection_len(&purchased), 1);
let converted = &opened_email & &purchased;
assert!(converted.contains_uuid(bob));
assert_eq!(converted.iter_uuids().collect::<Vec<_>>(), vec![bob]);

// Many-way operations over any number of sets.
let reached = [&opened_email, &purchased].union();
assert_eq!(reached.len(), 3);
```

Without the feature, the same works with `Uuid::as_u128` / `Uuid::from_u128`.

## How it works

`roaring128` is not a wrapper around a 32- or 64-bit Roaring bitmap. Each value is split into a
112-bit key and a 16-bit index:

```text
value (u128) = [ key: high 112 bits | index: low 16 bits ]

RoaringBitmap128
  len: u64                          cached cardinality, len() is O(1)
  BTreeMap<key, Container>          one ordered index, no empty containers
                 |
                 +-- Array   sorted u16 indices, <= 4096 values (<= 7 stored inline)
                 +-- Bitmap  65536 bits = 8 KiB, > 4096 values
                 +-- Run     sorted (start, length) intervals
```

- Every key owns one native Roaring container. The container kinds, the array/bitmap threshold
  (4096) and the run conversion rules of `optimize()` follow the Roaring specification and the
  `roaring` crate, so cardinalities and container choices agree with other implementations.
- A container is exactly 16 bytes, so the B-tree stays dense. Arrays of up to 7 values are stored
  inline in that slot; only larger arrays, run lists and bitmaps allocate.
- Set operations are a single merge over the sorted keys of both bitmaps, with one container
  operation per common key; when one side has far fewer containers than the other, its keys are
  looked up in the other side instead. The container operations (array merges, bitmap
  and/or/xor/andnot, popcounts, bitmap-to-array conversion, run counting) run on vectorized
  kernels.

### Compression: what to expect

Random UUIDs don't compress like dense integers. A v4 UUID has 122 random bits and a v7 UUID has
62 random low bits, so two of them essentially never share a 112-bit key: every UUID lands in a
container of its own. The design keeps that sparse case cheap within the Roaring family: on
random UUIDs `roaring128` uses 34–51 heap bytes and 18 serialized bytes per value, against
265–539 for wrapped roaring64 — though a `BTreeSet<u128>` (18–27) or a sorted `Vec<u128>` (16)
is smaller still, and faster where it fits ([When to use it](#when-to-use-it)):

- In memory, a sparse value costs one 16-byte key and one 16-byte container slot in the B-tree
  (plus the amortized B-tree node overhead), with no per-value heap allocation.
- Serialized, a sparse value costs 18 bytes (a 16-byte descriptor and a 2-byte payload), plus a
  16-byte header for the whole bitmap. See [FORMAT.md](docs/FORMAT.md).

Compression applies when the low bits of 128-bit IDs are sequential or dense: a tenant or shard
prefix with a sequence (`(prefix << 64) | seq`), snowflake-style IDs under a prefix, custom UUIDv8
layouts with a counter in the low bits, or ranges inserted with `insert_range`. There arrays,
bitmaps and runs work exactly as in 32-bit Roaring.

### Time-ordered UUIDs

UUIDs are stored as their big-endian `u128` (`Uuid::as_u128`), the same order as comparing their
bytes. Time-ordered UUIDs (v6, v7) therefore iterate in timestamp order. Appending at the end is
cheap (`try_push`, `append`, `from_sorted_iter` for already sorted input such as `ORDER BY id`),
and a time window is a range of keys:

```rust
// UUIDv7 keeps Unix time in milliseconds in its top 48 bits.
fn v7_window(from_ms: u64, to_ms: u64) -> std::ops::Range<u128> {
    ((from_ms as u128) << 80)..((to_ms as u128) << 80)
}

let created = events.range_cardinality(v7_window(t0, t1)); // count only
let ids: Vec<Uuid> = events.range(v7_window(t0, t1)).map(Uuid::from_u128).collect();
```

## Hardware requirements

`roaring128` targets current hardware and deliberately has no fallback for old CPUs:

| Architecture | Requirement | Used when available |
|---|---|---|
| `x86_64` | AVX2 + BMI1 + BMI2 + POPCNT (x86-64-v3: Intel Haswell / AMD Zen or newer) | AVX-512 F/BW/VL/DQ/VPOPCNTDQ/BITALG/VBMI2 (Intel Ice Lake, AMD Zen 4 and newer) |
| `aarch64` | NEON (every AArch64 CPU: Apple Silicon, AWS Graviton, Ampere, ...) | - |
| anything else | does not compile | - |

- With `std` (the default) the backend is detected at runtime, once, so a single binary uses
  AVX-512 where it exists and AVX2 elsewhere. CPUs with AVX-512 but without VPOPCNTDQ, BITALG and
  VBMI2 (Skylake-SP, Cascade Lake) use the AVX2 path.
- On an x86_64 CPU without AVX2, roaring128 panics with a message naming the requirement as soon
  as it selects a backend: on the first operation that needs a vector kernel, or on a call to
  `simd_backend()`.
- Without `std` the backend is fixed at compile time from the enabled target features: build with
  at least `-C target-cpu=x86-64-v3`. The AVX-512 path additionally needs VPOPCNTDQ, BITALG and
  VBMI2 (for example `-C target-cpu=icelake-server` or `znver4`; `x86-64-v4` alone does not
  include them).

`roaring128::simd_backend()` reports the selected backend. With `std`, the environment variable
`ROARING128_FORCE_BACKEND=avx2|avx512|neon` downgrades the selection for testing; requests for a
backend the CPU doesn't support are ignored.

```rust
println!("roaring128 backend: {}", roaring128::simd_backend().name());
```

## API parity with `roaring`

`RoaringBitmap128` follows `roaring::RoaringTreemap` / `RoaringBitmap` with `u128` values, so
code written against `roaring` ports mostly by changing the type:

- Construction: `new`, `Default`, `From<[u128; N]>`, `FromIterator`, `Extend`, `from_sorted_iter`,
  `append`, `try_push`, `from_lsb0_bytes`.
- Values and ranges: `insert`, `remove`, `contains`, `insert_range`, `remove_range`,
  `contains_range`, `range_cardinality`, `clear`, `remove_smallest`, `remove_biggest`.
- Queries: `len`, `is_empty`, `min`, `max`, `rank`, `select`, `is_disjoint`, `is_subset`,
  `is_superset`, `statistics`.
- Set operations: `|`, `&`, `^`, `-` and their assigning forms, on owned and borrowed operands;
  `union_len`, `intersection_len`, `difference_len`, `symmetric_difference_len` without
  materializing the result; `MultiOps` (`union`, `intersection`, `difference`,
  `symmetric_difference`) over iterators of owned bitmaps, references or `Result`s.
- Iteration: `iter`, `into_iter`, `range`, `into_range`; the iterators are double-ended and
  support `advance_to`, `advance_back_to`, `next_range`, `next_range_back`.
- Storage: `optimize`, `remove_run_compression`.
- Serialization: `serialized_size`, `serialize_into`, `deserialize_from`,
  `deserialize_unchecked_from`, `intersection_with_serialized_unchecked`; `serde` support.

Deviations:

- No `full()`: a full set of 2^128 values cannot be materialized. `is_full()` exists for parity but
  is always `false` in practice.
- No `bitmaps()` / `from_bitmaps()`: they expose the treemap's internal 32-bit partitioning, which
  doesn't exist here.
- Counts are `u64`: `len`, `rank`, `range_cardinality`, `insert_range` and `remove_range` return
  `u64`, and `select` takes `n: u64`.
- The serialization format is its own (see below), since there is no 128-bit Roaring standard.
- Additions: the `uuid` helpers and `simd_backend()`.

## Serialization

There is no standard Roaring format for 128-bit values, so `roaring128` defines one, kept as close
as possible to the 32-bit [Roaring format specification](https://github.com/RoaringBitmap/RoaringFormatSpec).
It is specified in [FORMAT.md](docs/FORMAT.md): a 16-byte header, an optional run-container bitset,
one 16-byte descriptor per container (key and cardinality), then the container payloads. Every
container's offset is computable from the header, with no offset table, which keeps sparse data at
18 bytes per value and lets `intersection_with_serialized_unchecked` read only the containers
whose keys are present in the other operand.

```rust
let mut bytes = Vec::with_capacity(set.serialized_size());
set.serialize_into(&mut bytes)?;
let back = RoaringBitmap128::deserialize_from(&bytes[..])?; // validates the input
assert_eq!(set, back);
```

`deserialize_from` validates everything and never allocates more than the input provides;
`deserialize_unchecked_from` skips content validation for trusted input. With the `serde`
feature, a bitmap serializes as a byte string in this format.

## Features

| Feature | Default | Description |
|---|---|---|
| `std` | yes | Runtime CPU detection and `std::io` serialization. Without it the crate is `no_std` + `alloc`, and the SIMD backend is chosen at compile time. |
| `serde` | no | `Serialize` / `Deserialize` for `RoaringBitmap128` (implies `std`). |
| `uuid` | no | `insert_uuid`, `remove_uuid`, `contains_uuid`, `iter_uuids`, and `FromIterator` / `Extend` for `Uuid` and `&Uuid`. |

Minimum supported Rust version: 1.91 (edition 2024).

## Testing

- Every SIMD kernel is differentially tested against a scalar reference implementation, with
  guard zones that catch out-of-bounds writes.
- Unit tests inside the crate check every container operation, for every pair of representations
  (inline array, heap array, bitmap, run), against a `BTreeSet<u16>` model and the container
  invariants, with the `roaring` crate as the oracle for representation choices and serialized
  sizes; the top-level bitmap has its own unit tests (key boundaries, the strategy choices of the
  set operations, iterator seeks), checked against a `BTreeSet<u128>` model.
- Model-based property tests (`tests/model/`) compare every public method of `RoaringBitmap128`
  with a `BTreeSet<u128>` model on structured sets (array clusters, dense bitmap blocks, runs
  spanning several keys, sparse UUID-like values, values around 2^32, 2^64, 2^112 and
  `u128::MAX`), and check the bitmap's internal invariants after every mutation.
- Differential tests against the `roaring` crate (`tests/vs_roaring.rs`): the same calls on
  `RoaringTreemap` (`u64` inputs) or `RoaringBitmap` (`u32` inputs) must give the same return
  values and the same contents, and in the `repr` tests the same container representations.
- The test suite of `roaring` 0.11.5, ported to `u128` (`tests/port/`): its `RoaringBitmap` and
  `RoaringTreemap` tests and its in-source unit and property tests, many also run at 128-bit
  offsets (around 2^32, 2^64, 2^100, 2^112 and near `u128::MAX`).
- Serialization tests (`tests/serialization/`): round trips, exact bytes against an independent
  reference encoder, hand-written and corrupted inputs, the unchecked readers, serde, and the
  ported serialization tests of `roaring`; a separate binary (`tests/serialization_limits.rs`)
  checks that inputs announcing more containers than they hold keep the allocations of
  `deserialize_from` proportional to the input.
- Smaller test targets cover the `uuid` feature and report the selected SIMD backend.
- CI runs the AVX2 path natively, the AVX-512 path under the Intel Software Development Emulator
  (Ice Lake and Sapphire Rapids), and NEON natively on Linux ARM64 and Apple Silicon. Under the
  emulator it also checks that a Haswell CPU selects AVX2 and that a pre-AVX2 CPU panics.
- Container code runs under Miri.

## Benchmarks

The harness lives in [`benches/`](benches/README.md): it times `RoaringBitmap128` against a
wrapped-roaring64 baseline (`BTreeMap<u64, RoaringTreemap>`), `BTreeSet<u128>`, a sorted
`Vec<u128>`, `HashSet<u128>` (default and FxHash) and `roaring::RoaringTreemap` (for data that
fits in `u64`) across 10k–1M value UUID and dense workloads, and reports wins *and* losses.

### What wins when

No structure wins everywhere. From the first (provisional, see below) run, the fastest
implementation for each kind of data and operation:

| data, operations | fastest in this run | roaring128 |
|---|---|---|
| **Clustered IDs** (sequences or ranges under a shared prefix): set operations `&` `\|` `-` `^`, `intersection_len`, `is_subset`, 16-way union / intersection | **roaring128** on most, up to 7x ahead of the next best on two-way operations and 240–545x on 16-way ones; wrapped64 is 1.1–1.3x faster on run containers (`\|` and the assigning forms) | fastest |
| Clustered IDs: memory | roaring128 and wrapped64 alike, 0.2–2.3 bytes/value | 7x to over 100x smaller than a sorted `Vec` (16 B/value), `BTreeSet` (18–27) or `HashSet` (36) |
| Clustered IDs: `insert`, `contains`, `remove` | FxHash `HashSet` (wrapped64 for some inserts) | 1.1–19x slower |
| Clustered IDs: range scans | sorted `Vec` | 18–34x slower |
| **u64 values** (against `RoaringTreemap` only) | **roaring128** on set operations (1.4–9.9x), iteration (4.8–16x), `from_sorted` (1.4–5.9x), rank / select | `RoaringTreemap` is 1.1–1.6x faster on `contains`, 1.2x on `insert`, 1.3x on `from_iter`, 1.5–1.7x on 16-way operations over dense runs |
| **Random UUIDs** (v4, v7), a set that changes and needs set algebra | `BTreeSet<u128>` on most operations: up to 2.7x faster on two-way set operations (about even on a few), 1.2–1.3x on `insert`, about 1.1x on `contains`; smaller too (18–27 against 34–51 B/value) | 1.2–13x faster than wrapped roaring64 on set operations and 5–15x smaller; ahead of `BTreeSet` on some assigning operations (up to 5.3x) |
| Random UUIDs built once from sorted input, then queried | sorted `Vec<u128>`: set operations 4–16x faster, 16 B/value | slower |
| Export / import, clustered IDs | **roaring128** (wrapped64 1.1–1.2x faster on run containers) | 16x to over 1000x faster than writing or reading a sorted `Vec`, at 0.2–2 serialized bytes/value against 16 |
| Export / import, random UUIDs | a sorted `Vec<u128>` written as a raw 16-byte array: about 27–29x faster to write and 20x faster to read, 16 bytes/value | 18 bytes/value; the format is roaring128's own, no other library reads it yet |
| Membership only (`insert` / `contains` / `remove`) | FxHash `HashSet<u128>` | 4–55x slower on random UUIDs |
| `rank` / `select` on random UUIDs | sorted `Vec` (binary search, index) | far slower: the cost grows with the number of containers, one per sparse value |
| Mixed (half dense, half random) | sorted `Vec` | 1.8–4.4x slower on set operations; 1.4–3.2x faster than `BTreeSet` and 2.5–5.3x faster than wrapped64 on them |

In short: `roaring128` is the fastest structure here for set algebra on clustered or dense
128-bit IDs, and it beats the other Roaring-based way to hold UUIDs (wrapped roaring64) on almost
everything. For random UUIDs a plain `BTreeSet<u128>`, a sorted `Vec<u128>` or a hash set is
faster for the job it fits; [When to use it](#when-to-use-it) says which. The full
per-operation matrix is in
[what-wins.md](docs/benchmarks/2026-11-01-ryzen7-5825u/what-wins.md).

### The run (2026-11-01): provisional

- **Hardware:** AMD Ryzen 7 5825U laptop (Zen 3), so only the **AVX2** backend was measured.
  There are no AVX-512 or NEON timings yet (CI runs the AVX-512 path under an emulator, which
  says nothing about speed).
- **Conditions:** `powersave` governor, no core pinning, load average 5–6 from other jobs on
  the host. One run (`repeat=1`) of 30 samples × 3 s per benchmark, so run-to-run variance was
  not measured; small differences may not reproduce.
- **Incomplete:** the wrapped64 `setops` process was killed (SIGKILL, 1.5 GiB peak RSS) when it
  reached `multi_union16/uuid_v4_16x100k`, so the 16-way operations have no wrapped64 numbers.
- **Build:** main at `46cbf87`, rustc 1.99.1, `--all-features`, system allocator.
- **Full results:** every timing and every loss in
  [report.md](docs/benchmarks/2026-11-01-ryzen7-5825u/report.md), the memory table in
  [memory.md](docs/benchmarks/2026-11-01-ryzen7-5825u/memory.md), run metadata in
  [meta.txt](docs/benchmarks/2026-11-01-ryzen7-5825u/meta.txt).

Geometric-mean speedup of `RoaringBitmap128` over all workloads (> 1: faster), with wins / ties /
losses counted where the confidence intervals do not overlap. By-construction results (`len`,
`rank`, `select` against baselines that sum cardinalities) are excluded:

| vs | geometric mean | wins / ties / losses |
|---|---:|---:|
| wrapped64 | 2.37x | 117 / 0 / 25 |
| `RoaringTreemap` (u64 data) | 2.05x | 29 / 0 / 12 |
| best ordered (`BTreeSet`, sorted `Vec`) | 1.16x | 51 / 2 / 95 |
| best hash (`HashSet`, FxHash) | 0.22x | 8 / 0 / 39 |

Still to do before these numbers count: repeated runs on a quiet machine with a fixed governor
and pinned cores, an AVX-512 machine (Zen 4 or Sapphire Rapids), an AArch64 machine for NEON,
and a wrapped64 `setops` run that is not killed.

## Status and roadmap

- [x] Public API, documentation, serialization format
- [x] SIMD dispatch, scalar reference kernels, CI (x86_64, AVX-512 under SDE, ARM64, Miri)
- [x] Container and bitmap implementation
- [x] Specialized AVX2 / AVX-512 / NEON kernels
- [x] Benchmark harness ([`benches/`](benches/README.md)); first, provisional run published above
- [ ] Benchmarks on a quiet machine: repeated runs, AVX-512 (Zen 4 / Sapphire Rapids) and NEON hardware, wrapped64 setops without the memory kill
- [ ] GPU benchmark comparison: RoaringBitmap128 (AVX2/AVX-512 CPU path) vs GPU-accelerated 128-bit sets (e.g. a DGX Spark / CUDA baseline such as a cuCollections-style bitmap or a GPU roaring implementation), to settle whether the GPU path beats the CPU kernels
- [ ] First release on crates.io

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
