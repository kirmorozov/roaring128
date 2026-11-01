//! Single-set operations: building (random-order inserts, unsorted and sorted bulk builds), point
//! lookups, removals, full and windowed iteration, cardinality, rank / select and serialization.
//!
//! Benchmark ids are `<operation>/<implementation>/<workload>`; implementations are
//! `roaring128`, `wrapped64` (`BTreeMap<u64, RoaringTreemap>`), `btreeset`, `sortedvec` (sorted
//! `Vec<u128>`), `hashset` (SipHash), `hashset_fx` (FxHash) and `treemap` (`RoaringTreemap`, only
//! for workloads that fit in `u64`). `BENCH_IMPL=<name>` selects implementations (run.sh runs
//! each in its own process); a regex filters benchmarks, e.g.
//! `cargo bench --bench ops -- 'contains_hit/.*/uuid_v4'`.
//!
//! See `benches/run.sh` for the settings used for the reported numbers.

mod common;

use std::hint::black_box;

use criterion::{Bencher, BenchmarkId, Criterion, Throughput};

use common::*;

const INSERT: u32 = 1 << 0;
const SORTED: u32 = 1 << 1;
const HIT: u32 = 1 << 2;
const MISS: u32 = 1 << 3;
const ITER: u32 = 1 << 4;
const RANGE: u32 = 1 << 5;
const RANK: u32 = 1 << 6;
const SERDE: u32 = 1 << 7;
const LEN: u32 = 1 << 8;
const REMOVE: u32 = 1 << 9;
/// Also insert in generation order (time order for UUID v7).
const INSERT_ORDERED: u32 = 1 << 10;
/// Build from the values in random order with the bulk API (`collect()` or sort + bulk build).
const FROM_ITER: u32 = 1 << 11;
/// Query an optimized structure (run containers where smaller); the workload name gets `_opt`.
const OPT: u32 = 1 << 12;

const K10: usize = 10_000;
const K100: usize = 100_000;
const M1: usize = 1_000_000;

const BUILD_QUERY: u32 = INSERT | FROM_ITER | SORTED | HIT | MISS | ITER | RANGE | SERDE;

/// Which measurements run on which workload. Removal runs on 100k sets (each iteration needs a
/// fresh clone).
const SPECS: &[(Kind, usize, u32)] = &[
    (
        Kind::UuidV4,
        K10,
        INSERT | FROM_ITER | SORTED | HIT | MISS | ITER,
    ),
    (Kind::UuidV4, K100, INSERT | HIT | REMOVE),
    (Kind::UuidV4, M1, BUILD_QUERY | RANK | LEN),
    (Kind::UuidV7, K100, REMOVE),
    (Kind::UuidV7, M1, BUILD_QUERY | INSERT_ORDERED),
    (Kind::DenseRuns, K100, REMOVE),
    (Kind::DenseRuns, M1, BUILD_QUERY),
    (Kind::DenseRuns, M1, HIT | MISS | ITER | RANGE | OPT),
    (Kind::DenseGaps50, K100, REMOVE),
    (Kind::DenseGaps50, M1, BUILD_QUERY | RANK | LEN),
    (Kind::DenseSmall, K100, REMOVE),
    (Kind::DenseSmall, M1, BUILD_QUERY),
    (Kind::Mixed, K100, REMOVE),
    (Kind::Mixed, M1, BUILD_QUERY | RANK),
    (
        Kind::RandomU64,
        M1,
        INSERT | FROM_ITER | SORTED | HIT | MISS | ITER | SERDE,
    ),
    (Kind::DenseRunsU64, M1, INSERT | SORTED | ITER | SERDE),
    (
        Kind::DenseGaps50U64,
        M1,
        INSERT | SORTED | HIT | ITER | RANK | SERDE,
    ),
    (
        Kind::DenseSmallU64,
        M1,
        INSERT | SORTED | HIT | ITER | SERDE,
    ),
];

const PROBES: usize = 4096;
const RANK_PROBES: usize = 16;
const WINDOWS: usize = 16;

/// Inputs shared by every implementation for one workload.
struct Ctx<'a> {
    w: &'a Workload,
    flags: u32,
    hit: Vec<u128>,
    miss: Vec<u128>,
    windows: Vec<(u128, u128)>,
    window_values: usize,
    rank_probes: Vec<u128>,
    ranks: Vec<u64>,
}

impl Ctx<'_> {
    fn new(w: &Workload, flags: u32) -> Ctx<'_> {
        let span = (w.len() / 1000).max(16);
        let mut rng = Rng::new(5);
        Ctx {
            w,
            flags,
            hit: w.hit_probes(PROBES, 1),
            miss: w.miss_probes(PROBES, 2),
            windows: w.windows(WINDOWS, span, 3),
            window_values: WINDOWS * span,
            rank_probes: w.hit_probes(RANK_PROBES, 4),
            ranks: (0..RANK_PROBES)
                .map(|_| rng.below(w.len() as u64))
                .collect(),
        }
    }

    fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

/// One benchmark in its own group (groups are per operation, so each implementation adds its
/// entry to the same `<operation>` report).
fn bench(
    c: &mut Criterion,
    group: &str,
    elements: usize,
    id: BenchmarkId,
    f: impl FnMut(&mut Bencher<'_>),
) {
    let mut g = c.benchmark_group(group);
    g.throughput(Throughput::Elements(elements as u64));
    g.bench_function(id, f);
    g.finish();
}

fn id<S: Set128>(w: &Workload) -> BenchmarkId {
    BenchmarkId::new(S::NAME, &w.name)
}

/// One `insert` call per value, in the given order.
fn insert_each<S: Set128>(values: &[u128]) -> S {
    let mut s = S::new_empty();
    for &v in values {
        s.insert(v);
    }
    s
}

/// The structure the queries run on: insert build, optimized for `OPT` workloads.
fn built<'a, S: Set128>(x: &Ctx<'a>) -> Built<'a, S> {
    if x.has(OPT) {
        Built::optimized(x.w)
    } else {
        Built::new(x.w)
    }
}

/// Builds and queries supported by every implementation.
fn basic<S: Set128>(c: &mut Criterion, x: &Ctx<'_>, built: &Built<'_, S>) {
    let w = x.w;
    let n = w.len();
    if x.has(INSERT) && S::POINT_UPDATES {
        bench(c, "insert", n, id::<S>(w), |b| {
            time_build(b, || insert_each::<S>(&w.shuffled))
        });
    }
    if x.has(INSERT_ORDERED) {
        // Appending in increasing order is cheap for every implementation (a sorted vector
        // pushes), so this runs for all of them.
        let ordered = BenchmarkId::new(S::NAME, format!("{}_inorder", w.name));
        bench(c, "insert", n, ordered, |b| {
            time_build(b, || insert_each::<S>(&w.sorted))
        });
    }
    if x.has(FROM_ITER) {
        bench(c, "from_iter", n, id::<S>(w), |b| {
            time_build(b, || S::build_from_iter(&w.shuffled))
        });
    }
    if x.has(SORTED) && S::BULK_BUILD {
        bench(c, "from_sorted", n, id::<S>(w), |b| {
            time_build(b, || S::build_sorted(&w.sorted).expect("bulk build"))
        });
    }
    for (flag, group, probes) in [
        (HIT, "contains_hit", &x.hit),
        (MISS, "contains_miss", &x.miss),
    ] {
        if x.has(flag) {
            bench(c, group, PROBES, id::<S>(w), |b| {
                let s = built.get();
                b.iter(|| probes.iter().filter(|&&v| s.contains(v)).count())
            });
        }
    }
    if x.has(REMOVE) && S::POINT_UPDATES {
        // Remove every value (random order) from a fresh clone; the clone is not timed.
        bench(c, "remove_all", n, id::<S>(w), |b| {
            let s = built.get();
            time_with_setup(
                b,
                || s.clone(),
                |mut s| {
                    for &v in &w.shuffled {
                        s.remove(v);
                    }
                    s
                },
            )
        });
    }
    if x.has(ITER) {
        bench(c, "iter", n, id::<S>(w), |b| {
            let s = built.get();
            b.iter(|| s.digest())
        });
    }
    if x.has(RANGE) && S::RANGE {
        // 16 windows of n/1000 consecutive values each.
        bench(c, "range", x.window_values, id::<S>(w), |b| {
            let s = built.get();
            b.iter(|| {
                x.windows
                    .iter()
                    .map(|&(lo, hi)| s.range_digest(lo, hi).expect("range").count)
                    .sum::<u64>()
            })
        });
    }
}

/// Cardinality, rank / select (roaring types and the wrapper).
fn ranked<S: Ranked>(c: &mut Criterion, x: &Ctx<'_>, built: &Built<'_, S>) {
    let w = x.w;
    if x.has(LEN) {
        bench(c, "len", 1, id::<S>(w), |b| {
            let s = built.get();
            b.iter(|| black_box(s).len())
        });
    }
    if x.has(RANK) {
        bench(c, "rank", RANK_PROBES, id::<S>(w), |b| {
            let s = built.get();
            b.iter(|| x.rank_probes.iter().map(|&v| s.rank(v)).sum::<u64>())
        });
        bench(c, "select", RANK_PROBES, id::<S>(w), |b| {
            let s = built.get();
            b.iter(|| {
                x.ranks
                    .iter()
                    .filter_map(|&n| s.select(n))
                    .fold(0, u128::wrapping_add)
            })
        });
    }
}

/// Serialization of the optimized structure (run containers where smaller), as one would
/// before storing a bitmap.
fn serde<S: Serial>(c: &mut Criterion, x: &Ctx<'_>) {
    if !x.has(SERDE) {
        return;
    }
    let w = x.w;
    let optimized = Built::<S>::optimized(w);
    bench(c, "serialize", w.len(), id::<S>(w), |b| {
        let s = optimized.get();
        let mut buf = Vec::with_capacity(s.serialized_size());
        b.iter(|| {
            buf.clear();
            s.serialize(&mut buf);
            buf.len()
        })
    });
    bench(c, "deserialize", w.len(), id::<S>(w), |b| {
        let mut bytes = Vec::new();
        optimized.get().serialize(&mut bytes);
        time_build(b, || S::deserialize(&bytes))
    });
}

/// Every measurement for one implementation; its structures are dropped before the next
/// implementation starts (the baselines need hundreds of MB for 1M random UUIDs).
fn full<S: Ranked + Serial>(c: &mut Criterion, x: &Ctx<'_>) {
    if !enabled::<S>() {
        return;
    }
    {
        let built = built::<S>(x);
        basic(c, x, &built);
        ranked(c, x, &built);
    }
    serde::<S>(c, x);
}

fn plain<S: Set128>(c: &mut Criterion, x: &Ctx<'_>) {
    if enabled::<S>() {
        basic(c, x, &built::<S>(x));
    }
}

fn single_set(c: &mut Criterion) {
    for (seed, &(kind, n, flags)) in SPECS.iter().enumerate() {
        let wanted = if kind.fits_u64() {
            enabled::<R128>() || enabled::<Tm>()
        } else {
            IMPLS.iter().any(|&i| i != Tm::NAME && selected(i))
        };
        if !wanted {
            continue;
        }
        let mut w = Workload::new(kind, n, 1000 + seed as u64);
        if flags & OPT != 0 {
            w.name.push_str("_opt");
        }
        let x = Ctx::new(&w, flags);
        full::<R128>(c, &x);
        if kind.fits_u64() {
            full::<Tm>(c, &x);
        } else {
            full::<Wrapped64>(c, &x);
            full::<Sv>(c, &x);
            plain::<Bts>(c, &x);
            plain::<Hs>(c, &x);
            plain::<HsFx>(c, &x);
        }
    }
}

/// The `uuid` feature's typed API on top of `insert` / `contains` (expected: no overhead).
#[cfg(feature = "uuid")]
fn uuid_api(c: &mut Criterion) {
    use uuid::Uuid;
    if !enabled::<R128>() {
        return;
    }
    let w = Workload::new(Kind::UuidV4, K100, 2000);
    let uuids: Vec<Uuid> = w.shuffled.iter().map(|&v| Uuid::from_u128(v)).collect();
    let probes: Vec<Uuid> = w
        .hit_probes(PROBES, 1)
        .into_iter()
        .map(Uuid::from_u128)
        .collect();
    bench(c, "insert_uuid", uuids.len(), id::<R128>(&w), |b| {
        time_build(b, || {
            let mut s = R128::new();
            for &u in &uuids {
                s.insert_uuid(u);
            }
            s
        })
    });
    let built = Built::<R128>::new(&w);
    bench(c, "contains_uuid", PROBES, id::<R128>(&w), |b| {
        let s = built.get();
        b.iter(|| probes.iter().filter(|&&u| s.contains_uuid(u)).count())
    });
}

#[cfg(not(feature = "uuid"))]
fn uuid_api(_: &mut Criterion) {}

fn main() {
    prelude("ops");
    let mut c = common::criterion();
    single_set(&mut c);
    uuid_api(&mut c);
    c.final_summary();
}
