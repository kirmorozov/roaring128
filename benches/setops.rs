//! Set algebra between two (or sixteen) sets: `&a op &b`, `a op= &b`, `intersection_len`,
//! `is_subset` and 16-way union / intersection (`MultiOps` for the roaring types, a fold for the
//! others).
//!
//! Pairs: random UUID v4 sets with 50% and 1% overlap and skewed sizes (1M vs 1k), UUID v7 with
//! 50% overlap, two independent samples of each dense distribution, the mixed workload, and two
//! `u64` workloads for the direct `RoaringTreemap` comparison.
//!
//! Benchmark ids are `<operation>/<implementation>/<pair>`.

mod common;

use criterion::{Bencher, BenchmarkId, Criterion, Throughput};

use common::*;

#[derive(Clone, Copy)]
enum Op {
    And,
    Or,
    Sub,
    Xor,
}

impl Op {
    const ALL: [Op; 4] = [Op::And, Op::Or, Op::Sub, Op::Xor];

    fn name(self) -> &'static str {
        match self {
            Op::And => "and",
            Op::Or => "or",
            Op::Sub => "sub",
            Op::Xor => "xor",
        }
    }

    fn apply<S: SetAlgebra>(self, a: &S, b: &S) -> S {
        match self {
            Op::And => a.and(b),
            Op::Or => a.or(b),
            Op::Sub => a.sub(b),
            Op::Xor => a.xor(b),
        }
    }

    fn apply_assign<S: SetAlgebraInPlace>(self, a: &mut S, b: &S) {
        match self {
            Op::And => a.and_assign(b),
            Op::Or => a.or_assign(b),
            Op::Sub => a.sub_assign(b),
            Op::Xor => a.xor_assign(b),
        }
    }
}

/// One benchmark in its own group (groups are per operation).
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

const ALGEBRA: u32 = 1;
const IN_PLACE: u32 = 2;
const SUBSET: u32 = 4;

/// Every pairwise measurement for one implementation; its structures are dropped before the
/// next implementation starts.
fn pair<S: SetAlgebra + SetAlgebraInPlace>(
    c: &mut Criterion,
    p: &Pair,
    sub: &Workload,
    flags: u32,
) {
    let name = p.name.as_str();
    let id = || BenchmarkId::new(S::NAME, name);
    let elements = p.a.len() + p.b.len();
    let (a, b) = (Built::<S>::new(&p.a), Built::<S>::new(&p.b));
    if flags & ALGEBRA != 0 {
        for op in Op::ALL {
            bench(c, op.name(), elements, id(), |bench| {
                let (x, y) = (a.get(), b.get());
                time_build(bench, || op.apply(x, y))
            });
        }
        bench(c, "intersection_len", elements, id(), |bench| {
            let (x, y) = (a.get(), b.get());
            bench.iter(|| x.intersection_len(y))
        });
    }
    if flags & IN_PLACE != 0 {
        // `target op= &other`, the clone of `target` is not timed. With skewed sizes the small
        // set is the target (cloning the big one per iteration would dominate the run time).
        let (target, other) = if p.a.len() > 10 * p.b.len() {
            (&b, &a)
        } else {
            (&a, &b)
        };
        for op in Op::ALL {
            bench(
                c,
                &format!("{}_assign", op.name()),
                elements,
                id(),
                |bench| {
                    let (x, y) = (target.get(), other.get());
                    time_with_setup(
                        bench,
                        || x.clone(),
                        |mut x| {
                            op.apply_assign(&mut x, y);
                            x
                        },
                    )
                },
            );
        }
    }
    if flags & SUBSET != 0 {
        let sub_built = Built::<S>::new(sub);
        let sub_id = BenchmarkId::new(S::NAME, &sub.name);
        bench(c, "is_subset", sub.len() + p.a.len(), sub_id, |bench| {
            let (x, y) = (sub_built.get(), a.get());
            assert!(x.is_subset(y));
            bench.iter(|| x.is_subset(y))
        });
    }
}

const K500: usize = 500_000;
const M1: usize = 1_000_000;

fn pairs(c: &mut Criterion) {
    // Symmetric sparse pairs use 500k values per side: the wrapped-roaring64 baseline needs about
    // 540 bytes per random UUID, so 1M-value operands (plus results and clones) would not leave
    // a laptop's memory alone.
    type Make = fn() -> Pair;
    let specs: [(Make, u32); 10] = [
        (
            || Pair::split(Kind::UuidV4, K500, K500, K500 / 2, 100),
            ALGEBRA | IN_PLACE | SUBSET,
        ),
        (
            || Pair::split(Kind::UuidV4, K500, K500, K500 / 100, 101),
            ALGEBRA,
        ),
        (
            || Pair::split(Kind::UuidV4, M1, 1_000, 500, 102),
            ALGEBRA | IN_PLACE | SUBSET,
        ),
        (
            || Pair::split(Kind::UuidV7, K500, K500, K500 / 2, 103),
            ALGEBRA,
        ),
        (|| Pair::independent(Kind::DenseRuns, M1, 104), ALGEBRA),
        (
            || Pair::independent(Kind::DenseGaps50, M1, 105),
            ALGEBRA | IN_PLACE | SUBSET,
        ),
        (|| Pair::independent(Kind::DenseSmall, M1, 106), ALGEBRA),
        (
            || Pair::split(Kind::Mixed, K500, K500, K500 / 2, 107),
            ALGEBRA | IN_PLACE | SUBSET,
        ),
        (
            || Pair::split(Kind::RandomU64, K500, K500, K500 / 2, 108),
            ALGEBRA,
        ),
        (
            || Pair::independent(Kind::DenseGaps50U64, M1, 109),
            ALGEBRA | IN_PLACE,
        ),
    ];
    for (make, flags) in specs {
        let p = make();
        // A random half of `a`, or a 1k subset of the big side of a skewed pair.
        let fraction = if p.b.len() * 10 < p.a.len() {
            p.b.len() as f64 / p.a.len() as f64
        } else {
            0.5
        };
        let mut sub = p.a.subset(fraction, 7);
        sub.name = format!("{}_sub", p.name);
        pair::<R128>(c, &p, &sub, flags);
        if p.a.kind.fits_u64() {
            pair::<Tm>(c, &p, &sub, flags);
        } else {
            pair::<Wrapped64>(c, &p, &sub, flags);
            pair::<Bts>(c, &p, &sub, flags);
        }
    }
}

fn many<S: SetAlgebra>(c: &mut Criterion, ws: &[Workload]) {
    let built: Vec<Built<'_, S>> = ws.iter().map(Built::new).collect();
    let elements: usize = ws.iter().map(Workload::len).sum();
    let id = BenchmarkId::new(S::NAME, &ws[0].name);
    bench(c, "multi_union16", elements, id.clone(), |b| {
        let refs: Vec<&S> = built.iter().map(Built::get).collect();
        time_build(b, || S::union_many(&refs))
    });
    bench(c, "multi_intersection16", elements, id, |b| {
        let refs: Vec<&S> = built.iter().map(Built::get).collect();
        time_build(b, || S::intersection_many(&refs))
    });
}

fn multi(c: &mut Criterion) {
    let specs: [fn() -> Vec<Workload>; 3] = [
        || many_with_core(Kind::UuidV4, 16, 100_000, 10_000, 200),
        || many_independent(Kind::DenseRuns, 16, 100_000, 201),
        || many_independent(Kind::DenseRunsU64, 16, 100_000, 202),
    ];
    for make in specs {
        let ws = make();
        many::<R128>(c, &ws);
        if ws[0].kind.fits_u64() {
            many::<Tm>(c, &ws);
        } else {
            many::<Wrapped64>(c, &ws);
            many::<Bts>(c, &ws);
        }
    }
}

fn main() {
    prelude("setops");
    let mut c = Criterion::default()
        .nresamples(20_000)
        .configure_from_args();
    pairs(&mut c);
    multi(&mut c);
    c.final_summary();
}
