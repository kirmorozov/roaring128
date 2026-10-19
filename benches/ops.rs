//! Single-set operations: building (random-order inserts and sorted bulk builds), point lookups,
//! removals, full and windowed iteration, cardinality, rank / select and serialization.
//!
//! Benchmark ids are `<operation>/<implementation>/<workload>`; implementations are
//! `roaring128`, `wrapped64` (`BTreeMap<u64, RoaringTreemap>`), `btreeset`, `hashset` and
//! `treemap` (`RoaringTreemap`, only for workloads that fit in `u64`). Filter with a regex, e.g.
//! `cargo bench --bench ops -- '/(wrapped64|btreeset)/'`.
//!
//! See `benches/run.sh` for the settings used for the reported numbers.

mod common;

use std::hint::black_box;

use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, BenchmarkId, Criterion, Throughput};

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

const K10: usize = 10_000;
const K100: usize = 100_000;
const M1: usize = 1_000_000;

const BUILD_QUERY: u32 = INSERT | SORTED | HIT | MISS | ITER | RANGE | SERDE;

/// Which measurements run on which workload. Removal runs on 100k sets (each iteration needs a
/// fresh clone).
const SPECS: &[(Kind, usize, u32)] = &[
    (Kind::UuidV4, K10, INSERT | SORTED | HIT | MISS | ITER),
    (Kind::UuidV4, K100, INSERT | HIT | REMOVE),
    (Kind::UuidV4, M1, BUILD_QUERY | RANK | LEN),
    (Kind::UuidV7, K100, REMOVE),
    (Kind::UuidV7, M1, BUILD_QUERY | INSERT_ORDERED),
    (Kind::DenseRuns, K100, REMOVE),
    (Kind::DenseRuns, M1, BUILD_QUERY),
    (Kind::DenseGaps50, K100, REMOVE),
    (Kind::DenseGaps50, M1, BUILD_QUERY | RANK | LEN),
    (Kind::DenseSmall, K100, REMOVE),
    (Kind::DenseSmall, M1, BUILD_QUERY),
    (Kind::Mixed, K100, REMOVE),
    (Kind::Mixed, M1, BUILD_QUERY | RANK),
    (
        Kind::RandomU64,
        M1,
        INSERT | SORTED | HIT | MISS | ITER | SERDE,
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

type Group<'a> = BenchmarkGroup<'a, WallTime>;

fn id<S: Set128>(w: &Workload) -> BenchmarkId {
    BenchmarkId::new(S::NAME, &w.name)
}

fn insert<S: Set128>(g: &mut Group<'_>, w: &Workload) {
    g.bench_function(id::<S>(w), |b| {
        time_build(b, || S::build_insert(&w.shuffled))
    });
}

fn insert_ordered<S: Set128>(g: &mut Group<'_>, w: &Workload) {
    let name = format!("{}_inorder", w.name);
    g.bench_function(BenchmarkId::new(S::NAME, name), |b| {
        time_build(b, || S::build_insert(&w.sorted))
    });
}

fn from_sorted<S: Set128>(g: &mut Group<'_>, w: &Workload) {
    g.bench_function(id::<S>(w), |b| {
        time_build(b, || S::build_sorted(&w.sorted).expect("bulk build"))
    });
}

fn contains<S: Cached>(g: &mut Group<'_>, sets: &Sets<'_>, probes: &[u128]) {
    g.bench_function(id::<S>(sets.w), |b| {
        let s = sets.get::<S>();
        b.iter(|| probes.iter().filter(|&&v| s.contains(v)).count())
    });
}

fn remove_all<S: Cached>(g: &mut Group<'_>, sets: &Sets<'_>) {
    g.bench_function(id::<S>(sets.w), |b| {
        let s = sets.get::<S>();
        time_with_setup(
            b,
            || s.clone(),
            |mut s| {
                for &v in &sets.w.shuffled {
                    s.remove(v);
                }
                s
            },
        )
    });
}

fn iter<S: Cached>(g: &mut Group<'_>, sets: &Sets<'_>) {
    g.bench_function(id::<S>(sets.w), |b| {
        let s = sets.get::<S>();
        b.iter(|| s.digest())
    });
}

fn range<S: Cached>(g: &mut Group<'_>, sets: &Sets<'_>, windows: &[(u128, u128)]) {
    g.bench_function(id::<S>(sets.w), |b| {
        let s = sets.get::<S>();
        b.iter(|| {
            windows
                .iter()
                .map(|&(lo, hi)| s.range_digest(lo, hi).expect("range").count)
                .sum::<u64>()
        })
    });
}

fn len<S: Cached>(g: &mut Group<'_>, sets: &Sets<'_>) {
    g.bench_function(id::<S>(sets.w), |b| {
        let s = sets.get::<S>();
        b.iter(|| black_box(s).len())
    });
}

fn rank<S: Cached + Ranked>(g: &mut Group<'_>, sets: &Sets<'_>, probes: &[u128]) {
    g.bench_function(id::<S>(sets.w), |b| {
        let s = sets.get::<S>();
        b.iter(|| probes.iter().map(|&v| s.rank(v)).sum::<u64>())
    });
}

fn select<S: Cached + Ranked>(g: &mut Group<'_>, sets: &Sets<'_>, ranks: &[u64]) {
    g.bench_function(id::<S>(sets.w), |b| {
        let s = sets.get::<S>();
        b.iter(|| {
            ranks
                .iter()
                .filter_map(|&n| s.select(n))
                .fold(0, u128::wrapping_add)
        })
    });
}

fn serialize<S: Cached + Serial>(g: &mut Group<'_>, optimized: &Sets<'_>) {
    g.bench_function(id::<S>(optimized.w), |b| {
        let s = optimized.get::<S>();
        let mut buf = Vec::with_capacity(s.serialized_size());
        b.iter(|| {
            buf.clear();
            s.serialize(&mut buf);
            buf.len()
        })
    });
}

fn deserialize<S: Cached + Serial>(g: &mut Group<'_>, optimized: &Sets<'_>) {
    g.bench_function(id::<S>(optimized.w), |b| {
        let mut bytes = Vec::new();
        optimized.get::<S>().serialize(&mut bytes);
        time_build(b, || S::deserialize(&bytes))
    });
}

/// Runs `$f::<Impl>($args)` for every implementation that applies to the workload:
/// `[roaring128, wrapped64, btreeset, hashset]` for 128-bit data, `[roaring128, treemap]` for
/// data that fits in `u64`. `ordered` drops `hashset`, `roaring` keeps only the roaring types.
macro_rules! each {
    ($w:expr, all, $f:ident($($a:expr),*)) => {
        if $w.kind.fits_u64() {
            $f::<R128>($($a),*);
            $f::<Tm>($($a),*);
        } else {
            $f::<R128>($($a),*);
            $f::<Wrapped64>($($a),*);
            $f::<Bts>($($a),*);
            $f::<Hs>($($a),*);
        }
    };
    ($w:expr, ordered, $f:ident($($a:expr),*)) => {
        if $w.kind.fits_u64() {
            $f::<R128>($($a),*);
            $f::<Tm>($($a),*);
        } else {
            $f::<R128>($($a),*);
            $f::<Wrapped64>($($a),*);
            $f::<Bts>($($a),*);
        }
    };
    ($w:expr, roaring, $f:ident($($a:expr),*)) => {
        if $w.kind.fits_u64() {
            $f::<R128>($($a),*);
            $f::<Tm>($($a),*);
        } else {
            $f::<R128>($($a),*);
            $f::<Wrapped64>($($a),*);
        }
    };
}

fn group<'c>(c: &'c mut Criterion, name: &str, elements: usize) -> Group<'c> {
    let mut g = c.benchmark_group(name);
    g.throughput(Throughput::Elements(elements as u64));
    g
}

fn single_set(c: &mut Criterion) {
    for (seed, &(kind, n, flags)) in SPECS.iter().enumerate() {
        let w = Workload::new(kind, n, 1000 + seed as u64);
        let w = &w;
        let sets = Sets::new(w);
        let sets = &sets;
        let optimized = Sets::optimized(w);
        let optimized = &optimized;

        if flags & INSERT != 0 {
            let mut g = group(c, "insert", w.len());
            each!(w, all, insert(&mut g, w));
            if flags & INSERT_ORDERED != 0 {
                each!(w, all, insert_ordered(&mut g, w));
            }
            g.finish();
        }
        if flags & SORTED != 0 {
            let mut g = group(c, "from_sorted", w.len());
            each!(w, ordered, from_sorted(&mut g, w));
            g.finish();
        }
        if flags & HIT != 0 {
            let probes = w.hit_probes(PROBES, 1);
            let mut g = group(c, "contains_hit", PROBES);
            each!(w, all, contains(&mut g, sets, &probes));
            g.finish();
        }
        if flags & MISS != 0 {
            let probes = w.miss_probes(PROBES, 2);
            let mut g = group(c, "contains_miss", PROBES);
            each!(w, all, contains(&mut g, sets, &probes));
            g.finish();
        }
        if flags & REMOVE != 0 {
            let mut g = group(c, "remove_all", w.len());
            each!(w, all, remove_all(&mut g, sets));
            g.finish();
        }
        if flags & ITER != 0 {
            let mut g = group(c, "iter", w.len());
            each!(w, all, iter(&mut g, sets));
            g.finish();
        }
        if flags & RANGE != 0 {
            let span = (w.len() / 1000).max(16);
            let windows = w.windows(WINDOWS, span, 3);
            let mut g = group(c, "range", WINDOWS * span);
            if w.kind.fits_u64() {
                range::<R128>(&mut g, sets, &windows);
            } else {
                range::<R128>(&mut g, sets, &windows);
                range::<Wrapped64>(&mut g, sets, &windows);
                range::<Bts>(&mut g, sets, &windows);
            }
            g.finish();
        }
        if flags & LEN != 0 {
            let mut g = group(c, "len", 1);
            each!(w, roaring, len(&mut g, sets));
            g.finish();
        }
        if flags & RANK != 0 {
            let probes = w.hit_probes(RANK_PROBES, 4);
            let mut g = group(c, "rank", RANK_PROBES);
            each!(w, roaring, rank(&mut g, sets, &probes));
            g.finish();
            let mut rng = Rng::new(5);
            let ranks: Vec<u64> = (0..RANK_PROBES)
                .map(|_| rng.below(w.len() as u64))
                .collect();
            let mut g = group(c, "select", RANK_PROBES);
            each!(w, roaring, select(&mut g, sets, &ranks));
            g.finish();
        }
        if flags & SERDE != 0 {
            let mut g = group(c, "serialize", w.len());
            each!(w, roaring, serialize(&mut g, optimized));
            g.finish();
            let mut g = group(c, "deserialize", w.len());
            each!(w, roaring, deserialize(&mut g, optimized));
            g.finish();
        }
    }
}

/// The `uuid` feature's typed API on top of `insert` / `contains` (expected: no overhead).
#[cfg(feature = "uuid")]
fn uuid_api(c: &mut Criterion) {
    use uuid::Uuid;
    let w = Workload::new(Kind::UuidV4, K100, 2000);
    let uuids: Vec<Uuid> = w.shuffled.iter().map(|&v| Uuid::from_u128(v)).collect();
    let probes: Vec<Uuid> = w
        .hit_probes(PROBES, 1)
        .into_iter()
        .map(Uuid::from_u128)
        .collect();
    let mut g = group(c, "insert_uuid", uuids.len());
    g.bench_function(BenchmarkId::new("roaring128", &w.name), |b| {
        time_build(b, || {
            let mut s = R128::new();
            for &u in &uuids {
                s.insert_uuid(u);
            }
            s
        })
    });
    g.finish();
    let sets = Sets::new(&w);
    let mut g = group(c, "contains_uuid", PROBES);
    g.bench_function(BenchmarkId::new("roaring128", &w.name), |b| {
        let s = sets.get::<R128>();
        b.iter(|| probes.iter().filter(|&&u| s.contains_uuid(u)).count())
    });
    g.finish();
}

#[cfg(not(feature = "uuid"))]
fn uuid_api(_: &mut Criterion) {}

fn main() {
    prelude("ops");
    let mut c = Criterion::default()
        .nresamples(20_000)
        .configure_from_args();
    single_set(&mut c);
    uuid_api(&mut c);
    c.final_summary();
}
