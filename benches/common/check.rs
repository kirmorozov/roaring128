//! Self-check run before the benchmarks: every selected implementation (see `BENCH_IMPL`) must
//! compute the same results as a plain sorted-slice reference on small instances of every
//! workload shape, otherwise the timings would compare different work. Checking each
//! implementation against the same reference cross-checks them all, also when each runs in its
//! own process. roaring128 is checked under `catch_unwind` so that the baseline benchmarks can
//! run while its implementation is still `todo!()`.

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};

use super::*;

type Res = Result<(), String>;

macro_rules! ensure {
    ($cond:expr, $($fmt:tt)*) => {
        if !$cond {
            return Err(format!($($fmt)*));
        }
    };
}

fn in_range(sorted: &[u128], lo: u128, hi: u128) -> &[u128] {
    let a = sorted.partition_point(|&v| v < lo);
    let b = sorted.partition_point(|&v| v <= hi);
    &sorted[a..b]
}

fn merge(a: &[u128], b: &[u128], keep: fn(bool, bool) -> bool) -> Vec<u128> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() || j < b.len() {
        let (v, in_a, in_b) = match (a.get(i), b.get(j)) {
            (Some(&x), Some(&y)) if x == y => {
                i += 1;
                j += 1;
                (x, true, true)
            }
            (Some(&x), Some(&y)) if x < y => {
                i += 1;
                (x, true, false)
            }
            (Some(&x), None) => {
                i += 1;
                (x, true, false)
            }
            (_, Some(&y)) => {
                j += 1;
                (y, false, true)
            }
            (None, None) => unreachable!(),
        };
        if keep(in_a, in_b) {
            out.push(v);
        }
    }
    out
}

fn and_ref(a: &[u128], b: &[u128]) -> Vec<u128> {
    merge(a, b, |x, y| x && y)
}
fn or_ref(a: &[u128], b: &[u128]) -> Vec<u128> {
    merge(a, b, |x, y| x || y)
}
fn sub_ref(a: &[u128], b: &[u128]) -> Vec<u128> {
    merge(a, b, |x, y| x && !y)
}
fn xor_ref(a: &[u128], b: &[u128]) -> Vec<u128> {
    merge(a, b, |x, y| x != y)
}

/// Insert build, optionally followed by `optimize()` (run containers for the roaring types).
fn build<S: Set128>(w: &Workload, optimize: bool) -> S {
    let mut s = S::build_insert(&w.shuffled);
    if optimize {
        s.optimize();
    }
    s
}

pub fn check_set<S: Set128>(w: &Workload) -> Res {
    let n = w.len() as u64;
    let name = &w.name;
    let s = S::build_insert(&w.shuffled);
    ensure!(s.len() == n, "{name}: len {} != {n}", s.len());
    ensure!(s.to_sorted_vec() == w.sorted, "{name}: iteration differs");
    ensure!(
        s.digest() == Digest::of(w.sorted.iter().copied()),
        "{name}: digest differs"
    );
    let mut dup = s.clone();
    ensure!(
        !dup.insert(w.sorted[0]),
        "{name}: duplicate insert returned true"
    );
    ensure!(dup.len() == n, "{name}: duplicate insert changed len");
    let mut with_dups = w.shuffled.clone();
    with_dups.extend_from_slice(&w.shuffled[..w.len().min(100)]);
    ensure!(
        S::build_from_iter(&with_dups).to_sorted_vec() == w.sorted,
        "{name}: build_from_iter differs"
    );
    if let Some(sorted) = S::build_sorted(&w.sorted) {
        ensure!(
            sorted.to_sorted_vec() == w.sorted,
            "{name}: build_sorted differs"
        );
    }
    for v in w.hit_probes(500, 1) {
        ensure!(s.contains(v), "{name}: missing {v:#x}");
    }
    for v in w.miss_probes(500, 2) {
        ensure!(!s.contains(v), "{name}: false positive {v:#x}");
    }
    for (lo, hi) in w.windows(20, (w.len() / 50).max(3), 3) {
        if let Some(d) = s.range_digest(lo, hi) {
            let want = Digest::of(in_range(&w.sorted, lo, hi).iter().copied());
            ensure!(d == want, "{name}: range {lo:#x}..={hi:#x} differs");
        }
    }
    let mut r = s.clone();
    // Per-value removal is O(n) in a sorted vector: fewer of them.
    let half = if S::POINT_UPDATES {
        w.len() / 2
    } else {
        w.len().min(400) / 2
    };
    for &v in &w.shuffled[..half] {
        ensure!(r.remove(v), "{name}: remove({v:#x}) returned false");
    }
    ensure!(
        !r.remove(w.shuffled[0]),
        "{name}: second remove returned true"
    );
    let mut rest = w.shuffled[half..].to_vec();
    rest.sort_unstable();
    ensure!(
        r.to_sorted_vec() == rest,
        "{name}: contents after removals differ"
    );
    ensure!(r.len() == rest.len() as u64, "{name}: len after removals");
    let mut o = s;
    o.optimize();
    ensure!(
        o.to_sorted_vec() == w.sorted,
        "{name}: optimize changed contents"
    );
    Ok(())
}

pub fn check_ranked<S: Ranked>(w: &Workload) -> Res {
    let s = S::build_insert(&w.shuffled);
    let name = &w.name;
    let mut probes = w.hit_probes(50, 4);
    probes.extend(w.miss_probes(50, 5));
    for v in probes {
        let want = w.sorted.partition_point(|&x| x <= v) as u64;
        ensure!(
            s.rank(v) == want,
            "{name}: rank({v:#x}) {} != {want}",
            s.rank(v)
        );
    }
    let mut rng = Rng::new(6);
    for _ in 0..50 {
        let i = rng.below(w.len() as u64);
        ensure!(
            s.select(i) == Some(w.sorted[i as usize]),
            "{name}: select({i})"
        );
    }
    ensure!(s.select(w.len() as u64).is_none(), "{name}: select(len)");
    Ok(())
}

pub fn check_serial<S: Serial>(w: &Workload) -> Res {
    let mut s = S::build_insert(&w.shuffled);
    for optimize in [false, true] {
        if optimize {
            s.optimize();
        }
        let mut buf = Vec::new();
        s.serialize(&mut buf);
        ensure!(
            buf.len() == s.serialized_size(),
            "{}: serialized_size",
            w.name
        );
        let back = S::deserialize(&buf);
        ensure!(back.to_sorted_vec() == w.sorted, "{}: round trip", w.name);
    }
    Ok(())
}

pub fn check_algebra<S: SetAlgebra>(p: &Pair, optimize: bool) -> Res {
    let (a, b): (S, S) = (build(&p.a, optimize), build(&p.b, optimize));
    let (ra, rb) = (&p.a.sorted, &p.b.sorted);
    let name = &format!("{}{}", p.name, if optimize { " (optimized)" } else { "" });
    ensure!(a.and(&b).to_sorted_vec() == and_ref(ra, rb), "{name}: and");
    ensure!(a.or(&b).to_sorted_vec() == or_ref(ra, rb), "{name}: or");
    ensure!(a.sub(&b).to_sorted_vec() == sub_ref(ra, rb), "{name}: sub");
    ensure!(
        b.sub(&a).to_sorted_vec() == sub_ref(rb, ra),
        "{name}: sub (reversed)"
    );
    ensure!(a.xor(&b).to_sorted_vec() == xor_ref(ra, rb), "{name}: xor");
    let shared = p.shared() as u64;
    ensure!(a.intersection_len(&b) == shared, "{name}: intersection_len");
    ensure!(
        b.intersection_len(&a) == shared,
        "{name}: intersection_len (reversed)"
    );
    let sub = p.a.subset(0.5, 7);
    let s: S = build(&sub, optimize);
    ensure!(
        s.is_subset(&a),
        "{name}: is_subset(subset, a) returned false"
    );
    ensure!(
        a.is_subset(&b) == (shared == p.a.len() as u64),
        "{name}: is_subset(a, b)"
    );
    Ok(())
}

/// An in-place operator and its reference implementation.
struct Case<S>(
    &'static str,
    fn(&mut S, &S),
    fn(&[u128], &[u128]) -> Vec<u128>,
);

pub fn check_in_place<S: SetAlgebraInPlace>(p: &Pair, optimize: bool) -> Res {
    let (a, b): (S, S) = (build(&p.a, optimize), build(&p.b, optimize));
    let (ra, rb) = (&p.a.sorted, &p.b.sorted);
    let name = &format!("{}{}", p.name, if optimize { " (optimized)" } else { "" });
    let cases: [Case<S>; 4] = [
        Case("and", S::and_assign, and_ref),
        Case("or", S::or_assign, or_ref),
        Case("sub", S::sub_assign, sub_ref),
        Case("xor", S::xor_assign, xor_ref),
    ];
    for Case(op, f, reference) in cases {
        let (want_ab, want_ba) = (reference(ra, rb), reference(rb, ra));
        let mut x = a.clone();
        f(&mut x, &b);
        ensure!(x.to_sorted_vec() == want_ab, "{name}: a {op}= b");
        ensure!(
            x.len() == want_ab.len() as u64,
            "{name}: len after a {op}= b"
        );
        let mut y = b.clone();
        f(&mut y, &a);
        ensure!(y.to_sorted_vec() == want_ba, "{name}: b {op}= a");
    }
    Ok(())
}

pub fn check_many<S: SetAlgebra>(ws: &[Workload], optimize: bool) -> Res {
    let sets: Vec<S> = ws.iter().map(|w| build(w, optimize)).collect();
    let refs: Vec<&S> = sets.iter().collect();
    let mut union = Vec::new();
    let mut inter = ws[0].sorted.clone();
    for w in ws {
        union = or_ref(&union, &w.sorted);
        inter = and_ref(&inter, &w.sorted);
    }
    ensure!(
        S::union_many(&refs).to_sorted_vec() == union,
        "{}: union_many",
        ws[0].name
    );
    ensure!(
        S::intersection_many(&refs).to_sorted_vec() == inter,
        "{}: intersection_many",
        ws[0].name
    );
    Ok(())
}

/// Small instances of every workload shape used by the benchmarks.
struct Fixtures {
    sets: Vec<Workload>,
    pairs: Vec<Pair>,
    many: Vec<Vec<Workload>>,
}

impl Fixtures {
    fn new() -> Fixtures {
        Fixtures {
            sets: Kind::ALL
                .iter()
                .map(|&k| Workload::new(k, 40_000, 11))
                .collect(),
            pairs: vec![
                Pair::split(Kind::UuidV4, 20_000, 20_000, 10_000, 12),
                Pair::split(Kind::UuidV4, 20_000, 200, 100, 13),
                Pair::split(Kind::UuidV7, 20_000, 20_000, 10_000, 14),
                Pair::independent(Kind::DenseRuns, 40_000, 15),
                Pair::independent(Kind::DenseGaps50, 40_000, 16),
                Pair::independent(Kind::DenseSmall, 40_000, 17),
                Pair::split(Kind::Mixed, 40_000, 40_000, 20_000, 18),
                Pair::split(Kind::RandomU64, 20_000, 20_000, 10_000, 19),
                Pair::independent(Kind::DenseGaps50U64, 40_000, 20),
                Pair::independent(Kind::DenseRunsU64, 40_000, 21),
            ],
            many: vec![
                many_with_core(Kind::UuidV4, 4, 5_000, 500, 22),
                many_independent(Kind::DenseRuns, 4, 40_000, 23),
                many_independent(Kind::DenseRunsU64, 4, 40_000, 24),
            ],
        }
    }
}

/// Workloads whose benchmarks also use optimized (run container) operands.
fn runs(k: Kind) -> bool {
    matches!(k, Kind::DenseRuns | Kind::DenseRunsU64)
}

/// Checks one implementation; `u64_only` restricts it to workloads that fit in `u64`.
fn check_impl<S: SetAlgebra + SetAlgebraInPlace + Ranked + Serial>(
    fx: &Fixtures,
    u64_only: bool,
) -> Res {
    let ok = |k: Kind| !u64_only || k.fits_u64();
    for w in fx.sets.iter().filter(|w| ok(w.kind)) {
        check_set::<S>(w)?;
        check_ranked::<S>(w)?;
        check_serial::<S>(w)?;
    }
    for p in fx.pairs.iter().filter(|p| ok(p.a.kind)) {
        for optimize in [false, true] {
            if optimize && !runs(p.a.kind) {
                continue;
            }
            check_algebra::<S>(p, optimize)?;
            check_in_place::<S>(p, optimize)?;
        }
    }
    for ws in fx.many.iter().filter(|ws| ok(ws[0].kind)) {
        check_many::<S>(ws, runs(ws[0].kind))?;
    }
    Ok(())
}

fn check_btreeset(fx: &Fixtures) -> Res {
    for w in &fx.sets {
        check_set::<Bts>(w)?;
    }
    for p in &fx.pairs {
        check_algebra::<Bts>(p, false)?;
        check_in_place::<Bts>(p, false)?;
    }
    for ws in &fx.many {
        check_many::<Bts>(ws, false)?;
    }
    Ok(())
}

fn check_sets_only<S: Set128>(fx: &Fixtures) -> Res {
    fx.sets.iter().try_for_each(check_set::<S>)
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "<non-string panic>".to_owned())
}

/// Checks the selected baselines (a failure panics), then roaring128 if selected: a mismatch
/// panics, an implementation that is not there yet (`todo!()`) is reported and its benchmarks
/// will panic when they run.
pub fn run_all() -> String {
    type Check = fn(&Fixtures) -> Res;
    let baselines: [(&str, Check); 6] = [
        (Wrapped64::NAME, |fx| check_impl::<Wrapped64>(fx, false)),
        (Tm::NAME, |fx| check_impl::<Tm>(fx, true)),
        (Sv::NAME, |fx| check_impl::<Sv>(fx, false)),
        (Bts::NAME, check_btreeset),
        (Hs::NAME, check_sets_only::<Hs>),
        (HsFx::NAME, check_sets_only::<HsFx>),
    ];
    let wanted: Vec<_> = baselines.iter().filter(|(n, _)| selected(n)).collect();
    if wanted.is_empty() && !selected(R128::NAME) {
        return "nothing selected".to_owned();
    }
    let fx = Fixtures::new();
    let mut parts = Vec::new();
    for (name, check) in wanted {
        if let Err(e) = check(&fx) {
            panic!("{name} self-check failed: {e}");
        }
        parts.push(format!("{name} OK"));
    }
    if selected(R128::NAME) {
        let hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        let r128 = panic::catch_unwind(AssertUnwindSafe(|| check_impl::<R128>(&fx, false)));
        panic::set_hook(hook);
        let status = match r128 {
            Ok(Ok(())) => "OK".to_owned(),
            Ok(Err(e)) => {
                panic!("roaring128 self-check failed: {e} (set BENCH_SKIP_CHECK=1 to bench anyway)")
            }
            Err(payload) => {
                let msg = panic_message(payload.as_ref());
                if msg.contains("not yet implemented") || msg.contains("not implemented") {
                    "NOT IMPLEMENTED (reached todo!()); filter its benchmarks out".to_owned()
                } else {
                    panic!(
                        "roaring128 panicked during the self-check: {msg} (set BENCH_SKIP_CHECK=1 to bench anyway)"
                    )
                }
            }
        };
        parts.push(format!("roaring128 {status}"));
    }
    parts.join(" | ")
}
