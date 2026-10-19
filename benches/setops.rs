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

use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, BenchmarkId, Criterion, Throughput};

use common::*;

type Group<'a> = BenchmarkGroup<'a, WallTime>;

const M1: usize = 1_000_000;

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

fn binop<S: Cached + SetAlgebra>(
    g: &mut Group<'_>,
    name: &str,
    a: &Sets<'_>,
    b: &Sets<'_>,
    op: Op,
) {
    g.bench_function(BenchmarkId::new(S::NAME, name), |bench| {
        let (x, y) = (a.get::<S>(), b.get::<S>());
        time_build(bench, || op.apply(x, y))
    });
}

/// `target op= &other`; the clone of `target` is made outside the timed region.
fn assign<S: Cached + SetAlgebraInPlace>(
    g: &mut Group<'_>,
    name: &str,
    target: &Sets<'_>,
    other: &Sets<'_>,
    op: Op,
) {
    g.bench_function(BenchmarkId::new(S::NAME, name), |bench| {
        let (x, y) = (target.get::<S>(), other.get::<S>());
        time_with_setup(
            bench,
            || x.clone(),
            |mut x| {
                op.apply_assign(&mut x, y);
                x
            },
        )
    });
}

fn intersection_len<S: Cached + SetAlgebra>(
    g: &mut Group<'_>,
    name: &str,
    a: &Sets<'_>,
    b: &Sets<'_>,
) {
    g.bench_function(BenchmarkId::new(S::NAME, name), |bench| {
        let (x, y) = (a.get::<S>(), b.get::<S>());
        bench.iter(|| x.intersection_len(y))
    });
}

fn is_subset<S: Cached + SetAlgebra>(
    g: &mut Group<'_>,
    name: &str,
    sub: &Sets<'_>,
    sup: &Sets<'_>,
) {
    g.bench_function(BenchmarkId::new(S::NAME, name), |bench| {
        let (x, y) = (sub.get::<S>(), sup.get::<S>());
        assert!(x.is_subset(y));
        bench.iter(|| x.is_subset(y))
    });
}

fn many<S: Cached + SetAlgebra>(g: &mut Group<'_>, name: &str, sets: &[Sets<'_>], union: bool) {
    g.bench_function(BenchmarkId::new(S::NAME, name), |bench| {
        let refs: Vec<&S> = sets.iter().map(|s| s.get::<S>()).collect();
        if union {
            time_build(bench, || S::union_many(&refs))
        } else {
            time_build(bench, || S::intersection_many(&refs))
        }
    });
}

/// `[roaring128, wrapped64, btreeset]` for 128-bit data, `[roaring128, treemap]` for `u64` data;
/// `in_place` drops `btreeset` (no in-place operators in `std`).
macro_rules! each {
    ($u64:expr, all, $f:ident($($a:expr),*)) => {
        if $u64 {
            $f::<R128>($($a),*);
            $f::<Tm>($($a),*);
        } else {
            $f::<R128>($($a),*);
            $f::<Wrapped64>($($a),*);
            $f::<Bts>($($a),*);
        }
    };
    ($u64:expr, in_place, $f:ident($($a:expr),*)) => {
        if $u64 {
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

const ALGEBRA: u32 = 1;
const IN_PLACE: u32 = 2;
const SUBSET: u32 = 4;

fn pairs(c: &mut Criterion) {
    let specs: Vec<(Box<dyn Fn() -> Pair>, u32)> = vec![
        (
            Box::new(|| Pair::split(Kind::UuidV4, M1, M1, M1 / 2, 100)),
            ALGEBRA | IN_PLACE | SUBSET,
        ),
        (
            Box::new(|| Pair::split(Kind::UuidV4, M1, M1, M1 / 100, 101)),
            ALGEBRA,
        ),
        (
            Box::new(|| Pair::split(Kind::UuidV4, M1, 1_000, 500, 102)),
            ALGEBRA | IN_PLACE | SUBSET,
        ),
        (
            Box::new(|| Pair::split(Kind::UuidV7, M1, M1, M1 / 2, 103)),
            ALGEBRA,
        ),
        (
            Box::new(|| Pair::independent(Kind::DenseRuns, M1, 104)),
            ALGEBRA,
        ),
        (
            Box::new(|| Pair::independent(Kind::DenseGaps50, M1, 105)),
            ALGEBRA | IN_PLACE | SUBSET,
        ),
        (
            Box::new(|| Pair::independent(Kind::DenseSmall, M1, 106)),
            ALGEBRA,
        ),
        (
            Box::new(|| Pair::split(Kind::Mixed, M1, M1, M1 / 2, 107)),
            ALGEBRA | IN_PLACE | SUBSET,
        ),
        (
            Box::new(|| Pair::split(Kind::RandomU64, M1, M1, M1 / 2, 108)),
            ALGEBRA,
        ),
        (
            Box::new(|| Pair::independent(Kind::DenseGaps50U64, M1, 109)),
            ALGEBRA | IN_PLACE,
        ),
    ];
    for (make, flags) in specs {
        let p = make();
        let u64 = p.a.kind.fits_u64();
        let name = p.name.as_str();
        let (a, b) = (Sets::new(&p.a), Sets::new(&p.b));
        let (a, b) = (&a, &b);
        let elements = p.a.len() + p.b.len();

        if flags & ALGEBRA != 0 {
            for op in Op::ALL {
                let mut g = group(c, op.name(), elements);
                each!(u64, all, binop(&mut g, name, a, b, op));
                g.finish();
            }
            let mut g = group(c, "intersection_len", elements);
            each!(u64, all, intersection_len(&mut g, name, a, b));
            g.finish();
        }
        if flags & IN_PLACE != 0 {
            // With skewed sizes the small set is the target (cloning the big one per iteration
            // would dominate the run time).
            let (target, other) = if p.a.len() > 10 * p.b.len() {
                (b, a)
            } else {
                (a, b)
            };
            for op in Op::ALL {
                let mut g = group(c, &format!("{}_assign", op.name()), elements);
                each!(u64, in_place, assign(&mut g, name, target, other, op));
                g.finish();
            }
        }
        if flags & SUBSET != 0 {
            // A random half of `a` (or the whole small set of a skewed pair) against `a`.
            let sub = if p.b.len() * 10 < p.a.len() {
                p.a.subset(p.b.len() as f64 / p.a.len() as f64, 7)
            } else {
                p.a.subset(0.5, 7)
            };
            let sub_sets = Sets::new(&sub);
            let mut g = group(c, "is_subset", sub.len() + p.a.len());
            let sub_name = format!("{name}_sub");
            each!(u64, all, is_subset(&mut g, &sub_name, &sub_sets, a));
            g.finish();
        }
    }
}

fn multi(c: &mut Criterion) {
    let specs: Vec<Vec<Workload>> = vec![
        many_with_core(Kind::UuidV4, 16, 100_000, 10_000, 200),
        many_independent(Kind::DenseRuns, 16, 100_000, 201),
        many_independent(Kind::DenseRunsU64, 16, 100_000, 202),
    ];
    for ws in &specs {
        let u64 = ws[0].kind.fits_u64();
        let name = ws[0].name.as_str();
        let sets: Vec<Sets<'_>> = ws.iter().map(Sets::new).collect();
        let elements: usize = ws.iter().map(Workload::len).sum();
        for (union, group_name) in [(true, "multi_union16"), (false, "multi_intersection16")] {
            let mut g = group(c, group_name, elements);
            each!(u64, all, many(&mut g, name, &sets, union));
            g.finish();
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
