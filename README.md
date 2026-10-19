# roaring128

[![CI](https://github.com/kirmorozov/roaring128/actions/workflows/ci.yml/badge.svg)](https://github.com/kirmorozov/roaring128/actions/workflows/ci.yml)

Native 128-bit [Roaring bitmaps](https://roaringbitmap.org/) for Rust. `RoaringBitmap128` is a
compressed set of `u128` values, built so that UUIDs and other 128-bit identifiers can be stored
in a Roaring bitmap directly, with exact set semantics and the usual Roaring set algebra. The API
mirrors the [`roaring`](https://docs.rs/roaring) crate, and the container kernels are written for
AVX2, AVX-512 and NEON, selected at runtime.

> **Status:** early development. The public API and its documentation are in place; the
> implementation is landing now. Not on crates.io yet, not ready for production use.

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
measurements in progress.

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
  operation per common key. The container operations (array merges, bitmap and/or/xor/andnot,
  popcounts, bitmap-to-array conversion, run counting) run on vectorized kernels.

### Compression: what to expect

Random UUIDs don't compress like dense integers. A v4 UUID has 122 random bits and a v7 UUID has
62 random low bits, so two of them essentially never share a 112-bit key: every UUID lands in a
container of its own. The design keeps that sparse case cheap instead:

- In memory, a sparse value costs one 16-byte key and one 16-byte container slot in the B-tree
  (plus the amortized B-tree node overhead), with no per-value heap allocation.
- Serialized, a sparse value costs 18 bytes (a 16-byte descriptor and a 2-byte payload), plus a
  16-byte header for the whole bitmap. See [FORMAT.md](FORMAT.md).

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
- On an x86_64 CPU without AVX2 the first use of a bitmap panics with a message naming the
  requirement.
- Without `std` the backend is fixed at compile time from the enabled target features: build with
  at least `-C target-cpu=x86-64-v3`. The AVX-512 path additionally needs VPOPCNTDQ, BITALG and
  VBMI2 (for example `-C target-cpu=icelake-server` or `znver4`; `x86-64-v4` alone does not
  include them).

`roaring128::simd_backend()` reports the selected backend. The environment variable
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
It is specified in [FORMAT.md](FORMAT.md): a 16-byte header, an optional run-container bitset, one
16-byte descriptor per container (key and cardinality), then the container payloads. Every
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
- CI runs the AVX2 path natively, the AVX-512 path under the Intel Software Development Emulator
  (Ice Lake and Sapphire Rapids), and NEON natively on Linux ARM64 and Apple Silicon. Under the
  emulator it also checks that a Haswell CPU selects AVX2 and that a pre-AVX2 CPU panics.
- Container code runs under Miri.
- Landing with the implementation: model-based tests against `BTreeSet<u128>`, differential tests
  against the `roaring` crate, and the `RoaringTreemap` test suite ported to `u128`.

## Benchmarks

Not published yet. Measured comparisons against a wrapped-roaring64 baseline
(`BTreeMap<u64, RoaringTreemap>`), `BTreeSet<u128>`, and `roaring::RoaringTreemap` (for data that
fits in `u64`) are in progress in [`benches/`](benches/).

## Status and roadmap

- [x] Public API, documentation, serialization format
- [x] SIMD dispatch, scalar reference kernels, CI (x86_64, AVX-512 under SDE, ARM64, Miri)
- [ ] Container and bitmap implementation (in progress)
- [ ] Specialized AVX2 / AVX-512 / NEON kernels
- [ ] Benchmarks
- [ ] First release on crates.io

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
