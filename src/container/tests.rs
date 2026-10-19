//! Container level tests (cross-representation semantics, invariants, iterators).
//!
//! Every `Container` operation is checked against a `BTreeSet<u16>` model for every pair of
//! representations (inline array, heap array, bitmap and run), with `internal_validate` and the
//! array/bitmap invariant checked after each operation. The `roaring` crate is used as an oracle
//! for representation decisions (`optimize`, conversions) and serialized sizes.
//!
//! Tests whose name starts with `run_` need the run containers of `run.rs`; all other tests only
//! use array and bitmap containers (run intervals may still appear through borrowed slices).

use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use core::ops::RangeInclusive;

use super::array::INLINE_CAPACITY;
use super::*;

// ---------------------------------------------------------------------------------------------
// Shared helpers (also used by the unit tests of the stores and iterators)
// ---------------------------------------------------------------------------------------------

pub(crate) type Model = BTreeSet<u16>;

/// Scales a case count down under Miri, which is several orders of magnitude slower.
pub(crate) const fn cases(native: usize) -> usize {
    if cfg!(miri) {
        let n = native / 64;
        if n == 0 { 1 } else { n }
    } else {
        native
    }
}

/// Small deterministic PRNG (splitmix64), so failures are reproducible.
#[derive(Clone)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub(crate) fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    pub(crate) fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    pub(crate) fn u16(&mut self) -> u16 {
        self.next() as u16
    }

    /// A value biased towards interesting positions: present values and their neighbours,
    /// word and container boundaries, or uniform.
    pub(crate) fn value(&mut self, model: &Model) -> u16 {
        const EDGES: [u16; 10] = [0, 1, 63, 64, 127, 4095, 4096, 32767, 65534, 65535];
        match self.below(4) {
            0 if !model.is_empty() => {
                // A present value (the successor of a random one) or one of its neighbours.
                let from = self.u16();
                let v = *model.range(from..).next().unwrap_or(model.last().unwrap());
                v.wrapping_add(self.below(3) as u16).wrapping_sub(1)
            }
            1 => EDGES[self.below(EDGES.len() as u64) as usize],
            _ => self.u16(),
        }
    }

    /// A range with a length drawn from several scales.
    pub(crate) fn range(&mut self, model: &Model) -> RangeInclusive<u16> {
        let len = match self.below(5) {
            0 => 1,
            1 => 1 + self.below(8),
            2 => 1 + self.below(200),
            // Long ranges make the `BTreeSet` model very slow under Miri.
            _ if cfg!(miri) => 1 + self.below(300),
            3 => 1 + self.below(5000),
            _ => 1 + self.below(65536),
        };
        let start = self.value(model) as u64;
        let end = (start + len - 1).min(u16::MAX as u64);
        start as u16..=end as u16
    }
}

/// `count` distinct values of `lo..lo + width`, sampling the smaller of the set and its
/// complement (the work is proportional to `count`, which keeps Miri runs short).
fn sample(rng: &mut Rng, lo: usize, width: usize, count: usize) -> Vec<u16> {
    let count = count.min(width);
    let invert = count > width / 2;
    let picks = if invert { width - count } else { count };
    let mut taken = alloc::vec![false; width];
    let mut picked = Vec::with_capacity(picks);
    while picked.len() < picks {
        let i = rng.below(width as u64) as usize;
        if !taken[i] {
            taken[i] = true;
            picked.push(i);
        }
    }
    if invert {
        (0..width)
            .filter(|&i| !taken[i])
            .map(|i| (lo + i) as u16)
            .collect()
    } else {
        picked.into_iter().map(|i| (lo + i) as u16).collect()
    }
}

/// A random set of about `n` values with one of several shapes.
pub(crate) fn random_set(rng: &mut Rng, n: usize) -> Model {
    let n = n.min(65536);
    if n == 0 {
        return Model::new();
    }
    let mut values: Vec<u16> = match rng.below(5) {
        // Uniform over the whole container.
        0 => sample(rng, 0, 65536, n),
        // Clustered in a window.
        1 => {
            let width = (n * (1 + rng.below(4) as usize)).min(65536);
            let lo = rng.below((65536 - width + 1) as u64) as usize;
            sample(rng, lo, width, n)
        }
        // Runs of consecutive values (about `n` values, overlaps are merged).
        2 => {
            let mut values = Vec::with_capacity(n);
            while values.len() < n {
                let start = rng.below(65536) as usize;
                let len = 1 + rng.below(1 + (n as u64 / 4).min(3000)) as usize;
                values.extend((start..(start + len).min(65536)).map(|v| v as u16));
            }
            values
        }
        // Dense prefix or suffix (whole words, word boundaries, container edges) with a few
        // holes.
        3 => {
            let range = if rng.chance(50) {
                65536 - n..65536
            } else {
                0..n
            };
            let holes: Vec<usize> = (0..rng.below(8))
                .map(|_| range.start + rng.below(n as u64) as usize)
                .collect();
            range
                .filter(|v| !holes.contains(v))
                .map(|v| v as u16)
                .collect()
        }
        // Every k-th value.
        _ => {
            let step = 1 + rng.below(16) as usize;
            let start = rng.below(step as u64) as usize;
            (start..65536)
                .step_by(step)
                .take(n)
                .map(|v| v as u16)
                .collect()
        }
    };
    values.sort_unstable();
    values.dedup();
    values.into_iter().collect()
}

/// A cardinality drawn from the interesting classes (inline, heap array, boundary, bitmap).
pub(crate) fn random_len(rng: &mut Rng) -> usize {
    match rng.below(8) {
        0 => rng.below(INLINE_CAPACITY as u64 + 2) as usize,
        1 => 8 + rng.below(56) as usize,
        2 => 64 + rng.below(4033) as usize,
        3 => 4090 + rng.below(12) as usize,
        4 if !cfg!(miri) => 4097 + rng.below(60000) as usize,
        5 if !cfg!(miri) => 65530 + rng.below(7) as usize,
        _ => rng.below(30) as usize,
    }
}

/// A random set with a random cardinality class.
pub(crate) fn random_model(rng: &mut Rng) -> Model {
    let n = random_len(rng);
    random_set(rng, n)
}

/// The maximal runs of a sorted set.
pub(crate) fn runs_of(values: &[u16]) -> Vec<RangeInclusive<u16>> {
    let mut runs: Vec<RangeInclusive<u16>> = Vec::new();
    for &v in values {
        match runs.last_mut() {
            Some(r) if *r.end() as u32 + 1 == v as u32 => *r = *r.start()..=v,
            _ => runs.push(v..=v),
        }
    }
    runs
}

pub(crate) fn intervals_of(values: &[u16]) -> Vec<Interval> {
    runs_of(values)
        .into_iter()
        .map(|r| Interval::new_unchecked(*r.start(), *r.end()))
        .collect()
}

/// Container representations under test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Repr {
    /// Array stored inline (at most `INLINE_CAPACITY` values).
    Inline,
    /// Array stored on the heap (any cardinality up to `ARRAY_LIMIT`).
    Heap,
    Bitmap,
    Run,
}

/// The valid representations of a set of `len` values.
pub(crate) fn reprs_for(len: usize, with_runs: bool) -> Vec<Repr> {
    let mut reprs = Vec::new();
    if len <= INLINE_CAPACITY {
        reprs.push(Repr::Inline);
    }
    if len as u64 <= ARRAY_LIMIT {
        reprs.push(Repr::Heap);
    } else {
        reprs.push(Repr::Bitmap);
    }
    if with_runs {
        reprs.push(Repr::Run);
    }
    reprs
}

pub(crate) fn make(model: &Model, repr: Repr) -> Container {
    let values: Vec<u16> = model.iter().copied().collect();
    let container = match repr {
        Repr::Inline => {
            assert!(values.len() <= INLINE_CAPACITY);
            Container::Array(ArrayStore::from_sorted_unchecked(&values))
        }
        Repr::Heap => Container::Array(ArrayStore::heap_from_slice(&values)),
        Repr::Bitmap => Container::Bitmap(BitmapStore::from_sorted_unchecked(&values)),
        Repr::Run => Container::Run(RunStore::from_intervals_unchecked(&intervals_of(&values))),
    };
    if let Container::Array(a) = &container {
        assert_eq!(a.is_heap(), repr == Repr::Heap);
    }
    container
}

/// The values of a container read from its representation (not through the iterators).
pub(crate) fn contents(c: &Container) -> Vec<u16> {
    match c {
        Container::Array(a) => a.as_slice().to_vec(),
        Container::Bitmap(b) => {
            let mut out = Vec::new();
            for (k, &w) in b.as_words().iter().enumerate() {
                let mut w = w;
                while w != 0 {
                    out.push((k * 64) as u16 + w.trailing_zeros() as u16);
                    w &= w - 1;
                }
            }
            out
        }
        Container::Run(r) => r
            .intervals()
            .iter()
            .flat_map(|iv| iv.start()..=iv.end())
            .collect(),
    }
}

/// Checks the values, the cardinality and the invariants of a container.
#[track_caller]
pub(crate) fn check(c: &Container, model: &Model) {
    let expected: Vec<u16> = model.iter().copied().collect();
    let actual = contents(c);
    if actual != expected {
        panic!(
            "{c:?}: contents differ from the model ({} values expected)",
            expected.len()
        );
    }
    assert_eq!(c.len(), model.len() as u64, "{c:?}: wrong cardinality");
    assert_eq!(c.is_empty(), model.is_empty());
    assert_eq!(c.is_full(), model.len() == 65536);
    match c {
        Container::Array(a) => assert!(a.len() <= ARRAY_LIMIT, "{c:?}: array above the limit"),
        Container::Bitmap(b) => assert!(b.len() > ARRAY_LIMIT, "{c:?}: bitmap below the limit"),
        Container::Run(_) => {}
    }
    if !model.is_empty()
        && let Err(e) = c.internal_validate()
    {
        panic!("{c:?}: internal_validate failed: {e}");
    }
}

fn model_range(r: &RangeInclusive<u16>) -> usize {
    if r.start() > r.end() {
        0
    } else {
        (*r.end() - *r.start()) as usize + 1
    }
}

/// Checks every query against the model.
#[track_caller]
fn check_queries(rng: &mut Rng, c: &Container, model: &Model) {
    assert_eq!(c.min(), model.first().copied());
    assert_eq!(c.max(), model.last().copied());
    for _ in 0..4 {
        let v = rng.value(model);
        assert_eq!(c.contains(v), model.contains(&v), "contains({v})");
        assert_eq!(
            c.rank(v),
            model.range(..=v).count() as u64,
            "{c:?}: rank({v})"
        );
        let r = rng.range(model);
        let in_range = model.range(r.clone()).count();
        assert_eq!(
            c.range_cardinality(r.clone()),
            in_range as u64,
            "range_cardinality({r:?})"
        );
        assert_eq!(
            c.contains_range(r.clone()),
            in_range == model_range(&r),
            "contains_range({r:?})"
        );
    }
    // Ranges made of present values.
    if let (Some(&lo), Some(&hi)) = (model.first(), model.last()) {
        let r = lo..=hi.min(lo.saturating_add(rng.below(4) as u16));
        let in_range = model.range(r.clone()).count();
        assert_eq!(c.contains_range(r.clone()), in_range == model_range(&r));
    }
    let len = model.len();
    for n in [0, len / 3, len / 2, len.saturating_sub(1), len, len + 1] {
        if n <= u16::MAX as usize {
            assert_eq!(
                c.select(n as u16),
                model.iter().nth(n).copied(),
                "{c:?}: select({n})"
            );
        }
    }
}

/// Whether inserting the range would fill the container (which creates a run container).
fn fills(model: &Model, r: &RangeInclusive<u16>) -> bool {
    model.len() + model_range(r) - model.range(r.clone()).count() == 65536
}

// ---------------------------------------------------------------------------------------------
// Point and range mutations
// ---------------------------------------------------------------------------------------------

fn point_ops(rng: &mut Rng, model: &mut Model, c: &mut Container, steps: usize, runs: bool) {
    for _ in 0..steps {
        match rng.below(11) {
            0 | 1 => {
                let v = rng.value(model);
                assert_eq!(c.insert(v), model.insert(v), "insert({v})");
            }
            2 | 3 => {
                let v = rng.value(model);
                assert_eq!(c.remove(v), model.remove(&v), "remove({v})");
            }
            4 => {
                let r = rng.range(model);
                if !runs && fills(model, &r) {
                    continue;
                }
                let added = model_range(&r) - model.range(r.clone()).count();
                model.extend(r.clone());
                assert_eq!(
                    c.insert_range(r.clone()),
                    added as u64,
                    "insert_range({r:?})"
                );
            }
            5 => {
                let r = rng.range(model);
                let removed: Vec<u16> = model.range(r.clone()).copied().collect();
                for v in &removed {
                    model.remove(v);
                }
                assert_eq!(
                    c.remove_range(r.clone()),
                    removed.len() as u64,
                    "remove_range({r:?})"
                );
            }
            6 => {
                let v = if rng.chance(50) {
                    model
                        .last()
                        .map_or(0, |m| m.saturating_add(1 + rng.below(3) as u16))
                } else {
                    rng.value(model)
                };
                let appended = model.last().is_none_or(|&m| m < v);
                if appended {
                    model.insert(v);
                }
                assert_eq!(c.push(v), appended, "push({v})");
            }
            7 => {
                let Some(v) = (match model.last() {
                    None => Some(rng.u16()),
                    Some(&u16::MAX) => None,
                    Some(&m) => Some(m + 1 + rng.below((u16::MAX - m) as u64) as u16),
                }) else {
                    continue;
                };
                model.insert(v);
                c.push_unchecked(v);
            }
            8 if !model.is_empty() => {
                let n = rng.below(model.len() as u64);
                for _ in 0..n {
                    model.pop_first();
                }
                c.remove_smallest(n);
            }
            9 if !model.is_empty() => {
                let n = rng.below(model.len() as u64);
                for _ in 0..n {
                    model.pop_last();
                }
                c.remove_biggest(n);
            }
            _ => {
                check_queries(rng, c, model);
                assert!(!c.ensure_correct_store(), "the invariants already hold");
            }
        }
        check(c, model);
    }
}

fn point_ops_suite(seed: u64, runs: bool) {
    let mut rng = Rng::new(seed);
    for _ in 0..cases(300) {
        let mut model = random_model(&mut rng);
        for repr in reprs_for(model.len(), runs) {
            if !runs && model.len() == 65536 {
                continue;
            }
            let mut c = make(&model, repr);
            check(&c, &model);
            let mut m = model.clone();
            point_ops(&mut rng, &mut m, &mut c, cases(64), runs);
        }
        model.clear();
    }
}

#[test]
fn point_ops_array_bitmap() {
    point_ops_suite(0x0001, false);
}

#[test]
fn run_point_ops() {
    point_ops_suite(0x0002, true);
}

/// Conversions at exactly `ARRAY_LIMIT`.
#[test]
fn array_bitmap_boundary() {
    let mut model: Model = (0..ARRAY_LIMIT as u16 * 2).step_by(2).collect();
    let mut c = Container::from_sorted_unchecked(&model.iter().copied().collect::<Vec<_>>());
    assert_eq!(c.kind(), Kind::Array);
    check(&c, &model);
    // Already present: stays an array.
    assert!(!c.insert(0));
    assert_eq!(c.kind(), Kind::Array);
    assert!(c.insert(1));
    model.insert(1);
    assert_eq!(c.kind(), Kind::Bitmap);
    check(&c, &model);
    assert!(c.remove(1));
    model.remove(&1);
    assert_eq!(c.kind(), Kind::Array);
    check(&c, &model);
    // push at the limit.
    assert!(c.push(9000));
    model.insert(9000);
    assert_eq!(c.kind(), Kind::Bitmap);
    check(&c, &model);
    c.remove_biggest(1);
    model.pop_last();
    assert_eq!(c.kind(), Kind::Array);
    check(&c, &model);
    c.push_unchecked(9001);
    model.insert(9001);
    assert_eq!(c.kind(), Kind::Bitmap);
    check(&c, &model);
    c.remove_smallest(1);
    model.pop_first();
    assert_eq!(c.kind(), Kind::Array);
    check(&c, &model);
    // Range insertion crossing the limit, range removal going back.
    assert_eq!(c.insert_range(20000..=20001), 2);
    model.extend(20000..=20001);
    assert_eq!(c.kind(), Kind::Bitmap);
    check(&c, &model);
    assert_eq!(c.remove_range(19999..=20000), 1);
    model.remove(&20000);
    assert_eq!(c.kind(), Kind::Bitmap);
    assert_eq!(c.remove_range(20001..=20001), 1);
    model.remove(&20001);
    assert_eq!(c.kind(), Kind::Array);
    check(&c, &model);
}

/// Inline / heap transitions of arrays.
#[test]
fn inline_heap_transitions() {
    let mut c = Container::new();
    let mut model = Model::new();
    for v in (0..20u16).rev() {
        c.insert(v * 3);
        model.insert(v * 3);
        check(&c, &model);
        let Container::Array(a) = &c else { panic!() };
        assert_eq!(a.is_heap(), model.len() > INLINE_CAPACITY);
        assert_eq!(c.mem_size() > 16, a.is_heap());
    }
    // Point removals keep the heap block until well below the inline capacity.
    for v in 0..17u16 {
        assert!(c.remove(v * 3));
        model.remove(&(v * 3));
        check(&c, &model);
    }
    let Container::Array(a) = &c else { panic!() };
    assert!(!a.is_heap());
    // Bulk removals compact immediately.
    let mut c = Container::from_sorted_unchecked(&(0..100).collect::<Vec<_>>());
    c.remove_range(5..=99);
    let Container::Array(a) = &c else { panic!() };
    assert!(!a.is_heap());
    assert_eq!(a.as_slice(), &[0, 1, 2, 3, 4]);
    assert_eq!(c.mem_size(), 16);
}

// ---------------------------------------------------------------------------------------------
// Set operations
// ---------------------------------------------------------------------------------------------

/// A second operand related to `a` in one of several ways.
fn related_set(rng: &mut Rng, a: &Model) -> Model {
    match rng.below(7) {
        0 => a.clone(),
        1 => a.iter().copied().filter(|_| rng.chance(70)).collect(),
        2 => {
            let mut b = a.clone();
            b.extend({
                let n = random_len(rng) / 4;
                random_set(rng, n)
            });
            b
        }
        3 => {
            let mut b = a.clone();
            for _ in 0..1 + rng.below(20) {
                let v = rng.value(a);
                if !b.remove(&v) {
                    b.insert(v);
                }
            }
            b
        }
        4 => Model::new(),
        5 => {
            // Complement inside the span of `a`.
            match (a.first(), a.last()) {
                (Some(&lo), Some(&hi)) => (lo..=hi).filter(|v| !a.contains(v)).collect(),
                _ => random_model(rng),
            }
        }
        _ => random_model(rng),
    }
}

fn union_model(a: &Model, b: &Model) -> Model {
    a | b
}

type BorrowedOp = fn(&mut Container, &Container);
type OwnedOp = fn(&mut Container, Container);

fn check_set_ops(rng: &mut Rng, ma: &Model, a: &Container, mb: &Model, b: &Container) {
    let union = union_model(ma, mb);
    let inter = ma & mb;
    let diff = ma - mb;
    let xor = ma ^ mb;

    check(&a.union(b), &union);
    check(&a.intersection(b), &inter);
    check(&a.difference(b), &diff);
    check(&a.symmetric_difference(b), &xor);

    let ops: [(BorrowedOp, &Model); 4] = [
        (Container::union_with, &union),
        (Container::intersect_with, &inter),
        (Container::difference_with, &diff),
        (Container::symmetric_difference_with, &xor),
    ];
    for (op, expected) in ops {
        let mut x = a.clone();
        op(&mut x, b);
        check(&x, expected);
    }
    let owned_ops: [(OwnedOp, &Model); 3] = [
        (Container::union_with_owned, &union),
        (Container::intersect_with_owned, &inter),
        (Container::symmetric_difference_with_owned, &xor),
    ];
    for (op, expected) in owned_ops {
        let mut x = a.clone();
        op(&mut x, b.clone());
        check(&x, expected);
    }

    // The exact operations choose the same representation whatever the operation style.
    let mut x = a.clone();
    x.union_with(b);
    assert_eq!(x.kind(), a.union(b).kind());
    let mut x = a.clone();
    x.symmetric_difference_with(b);
    assert_eq!(x.kind(), a.symmetric_difference(b).kind());

    assert_eq!(a.is_disjoint(b), inter.is_empty(), "is_disjoint");
    assert_eq!(b.is_disjoint(a), inter.is_empty(), "is_disjoint (swapped)");
    assert_eq!(a.is_subset(b), ma.is_subset(mb), "is_subset");
    assert_eq!(b.is_subset(a), mb.is_subset(ma), "is_subset (swapped)");
    assert_eq!(
        a.intersection_len(b),
        inter.len() as u64,
        "intersection_len"
    );
    assert_eq!(b.intersection_len(a), inter.len() as u64);
    assert_eq!(a.eq_values(b), ma == mb, "eq_values");
    assert_eq!(a == b, ma == mb);
    assert_eq!(b == a, ma == mb);

    // Lazy operations, then repair: same values and representation as the exact operations.
    let mut lazy = a.clone();
    let mut exact = a.clone();
    lazy.lazy_union_with(b);
    exact.union_with(b);
    let mut model = union.clone();
    let extra = random_model(rng);
    for repr in reprs_for(extra.len(), matches!(b, Container::Run(_))) {
        let e = make(&extra, repr);
        lazy.lazy_union_with(&e);
        exact.union_with(&e);
    }
    model.extend(extra.iter().copied());
    lazy.lazy_union_with(b);
    exact.union_with(b);
    lazy.repair_lazy();
    check(&lazy, &model);
    check(&exact, &model);
    assert_eq!(lazy.kind(), exact.kind(), "lazy union representation");

    let mut lazy = a.clone();
    let mut exact = a.clone();
    let mut model = ma.clone();
    for _ in 0..3 {
        lazy.lazy_symmetric_difference_with(b);
        exact.symmetric_difference_with(b);
        model = &model ^ mb;
        let extra = related_set(rng, mb);
        let reprs = reprs_for(extra.len(), matches!(b, Container::Run(_)));
        let e = make(&extra, reprs[rng.below(reprs.len() as u64) as usize]);
        lazy.lazy_symmetric_difference_with(&e);
        exact.symmetric_difference_with(&e);
        model = &model ^ &extra;
    }
    lazy.repair_lazy();
    check(&lazy, &model);
    check(&exact, &model);
    assert_eq!(
        lazy.kind(),
        exact.kind(),
        "lazy symmetric difference representation"
    );
}

fn set_ops_suite(seed: u64, runs: bool) {
    let mut rng = Rng::new(seed);
    for _ in 0..cases(250) {
        let ma = random_model(&mut rng);
        let mb = related_set(&mut rng, &ma);
        for ra in reprs_for(ma.len(), runs) {
            for rb in reprs_for(mb.len(), runs) {
                if !runs && (ma.len() == 65536 || mb.len() == 65536) {
                    continue;
                }
                if runs && ra != Repr::Run && rb != Repr::Run {
                    continue;
                }
                let (a, b) = (make(&ma, ra), make(&mb, rb));
                check_set_ops(&mut rng, &ma, &a, &mb, &b);
                check_set_ops(&mut rng, &mb, &b, &ma, &a);
            }
        }
    }
}

#[test]
fn set_ops_array_bitmap() {
    set_ops_suite(0x0003, false);
}

#[test]
fn run_set_ops() {
    set_ops_suite(0x0004, true);
}

/// Unions and symmetric differences of arrays whose total size exceeds the limit go through a
/// bitmap and come back as arrays when small.
#[test]
fn array_ops_through_bitmap() {
    let a: Model = (0..3000).collect();
    let b: Model = (1000..4000).collect();
    let (ca, cb) = (make(&a, Repr::Heap), make(&b, Repr::Heap));
    let u = ca.union(&cb);
    assert_eq!(u.kind(), Kind::Array);
    check(&u, &(&a | &b));
    let x = ca.symmetric_difference(&cb);
    assert_eq!(x.kind(), Kind::Array);
    check(&x, &(&a ^ &b));
    let c: Model = (3000..8000).collect();
    let cc = make(&c, Repr::Bitmap);
    let u = ca.union(&cb);
    assert_eq!(u.union(&cc).kind(), Kind::Bitmap);
    let d: Model = (0..2100).map(|v| v * 2).collect();
    let e: Model = (0..2100).map(|v| v * 2 + 1).collect();
    let (cd, ce) = (make(&d, Repr::Heap), make(&e, Repr::Heap));
    let u = cd.union(&ce);
    assert_eq!(u.kind(), Kind::Bitmap);
    check(&u, &(&d | &e));
    let x = cd.symmetric_difference(&ce);
    assert_eq!(x.kind(), Kind::Bitmap);
    check(&x, &(&d ^ &e));
}

// ---------------------------------------------------------------------------------------------
// Construction, conversions and serialization
// ---------------------------------------------------------------------------------------------

#[test]
fn constructors() {
    check(&Container::new(), &Model::new());
    check(&Container::default(), &Model::new());
    check(&Container::with_single(7), &[7].into());
    for (lo, hi) in [(0, 0), (5, 6), (65534, 65535), (65535, 65535)] {
        let c = Container::from_range(lo..=hi);
        assert_eq!(c.kind(), Kind::Array);
        check(&c, &(lo..=hi).collect());
    }
    let mut rng = Rng::new(0x0005);
    for _ in 0..cases(200) {
        let model = random_model(&mut rng);
        let values: Vec<u16> = model.iter().copied().collect();
        let c = Container::from_sorted_unchecked(&values);
        check(&c, &model);
        let expected_kind = if values.len() as u64 <= ARRAY_LIMIT {
            Kind::Array
        } else {
            Kind::Bitmap
        };
        assert_eq!(c.kind(), expected_kind);
        assert_eq!(
            Container::Array(ArrayStore::try_from_sorted(&values).unwrap_or_default()).len(),
            if values.len() as u64 <= ARRAY_LIMIT {
                values.len() as u64
            } else {
                0
            }
        );
    }
}

#[test]
fn run_constructors() {
    for (lo, hi) in [(0, 2), (100, 5000), (0, 65535)] {
        let c = Container::from_range(lo..=hi);
        assert_eq!(c.kind(), Kind::Run);
        check(&c, &(lo..=hi).collect());
    }
    let full = Container::full();
    assert_eq!(full.kind(), Kind::Run);
    assert!(full.is_full());
    check(&full, &(0..=u16::MAX).collect());
}

fn lsb0_bytes(model: &Model, byte_offset: usize, len: usize) -> Vec<u8> {
    let mut bytes = alloc::vec![0u8; len];
    for &v in model {
        let i = v as usize / 8 - byte_offset;
        bytes[i] |= 1 << (v % 8);
    }
    bytes
}

#[test]
fn from_lsb0_bytes() {
    let mut rng = Rng::new(0x0006);
    assert!(Container::from_lsb0_bytes(&[], 0).is_none());
    assert!(Container::from_lsb0_bytes(&[0; 100], 10).is_none());
    assert!(Container::from_lsb0_bytes(&[0; 8192], 0).is_none());
    for _ in 0..cases(300) {
        let byte_offset = rng.below(8192) as usize;
        let len = match rng.below(3) {
            0 => rng.below(65) as usize,
            1 => rng.below(1000) as usize,
            _ => 8192 - byte_offset,
        }
        .min(8192 - byte_offset);
        let lo = byte_offset * 8;
        let hi = (byte_offset + len) * 8;
        let model: Model = random_model(&mut rng)
            .into_iter()
            .filter(|&v| (lo..hi).contains(&(v as usize)))
            .collect();
        let bytes = lsb0_bytes(&model, byte_offset, len);
        match Container::from_lsb0_bytes(&bytes, byte_offset) {
            None => assert!(model.is_empty()),
            Some(c) => check(&c, &model),
        }
    }
}

#[test]
#[should_panic]
fn from_lsb0_bytes_out_of_bounds() {
    Container::from_lsb0_bytes(&[1; 10], 8183);
}

/// Decodes a payload written by `write_payload` according to `FORMAT.md`.
fn decode_payload(kind: Kind, descriptor: u16, bytes: &[u8]) -> Vec<u16> {
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    match kind {
        Kind::Array => (0..descriptor as usize + 1)
            .map(|i| u16_at(2 * i))
            .collect(),
        Kind::Bitmap => {
            assert_eq!(bytes.len(), BITMAP_BYTES);
            let mut out = Vec::new();
            for (j, chunk) in bytes.as_chunks::<8>().0.iter().enumerate() {
                let mut w = u64::from_le_bytes(*chunk);
                while w != 0 {
                    out.push((64 * j) as u16 + w.trailing_zeros() as u16);
                    w &= w - 1;
                }
            }
            assert_eq!(out.len(), descriptor as usize + 1);
            out
        }
        Kind::Run => (0..descriptor as usize + 1)
            .flat_map(|i| {
                let start = u16_at(4 * i);
                start..=start + u16_at(4 * i + 2)
            })
            .collect(),
    }
}

fn check_serialization(c: &Container, model: &Model) {
    let mut out = alloc::vec![0xAA];
    c.write_payload(&mut out);
    assert_eq!(out[0], 0xAA, "write_payload must append");
    assert_eq!(out.len() - 1, c.serialized_payload_size());
    let decoded = decode_payload(c.kind(), c.descriptor(), &out[1..]);
    assert_eq!(decoded, model.iter().copied().collect::<Vec<_>>());
}

fn serialization_suite(seed: u64, runs: bool) {
    let mut rng = Rng::new(seed);
    for _ in 0..cases(150) {
        let model = random_model(&mut rng);
        if model.is_empty() {
            // Empty containers are never serialized.
            continue;
        }
        for repr in reprs_for(model.len(), runs) {
            if runs && repr != Repr::Run {
                continue;
            }
            check_serialization(&make(&model, repr), &model);
        }
    }
}

#[test]
fn serialization_array_bitmap() {
    serialization_suite(0x0007, false);
    let c = Container::with_single(0x1234);
    assert_eq!(c.descriptor(), 0);
    assert_eq!(c.serialized_payload_size(), 2);
    let mut out = Vec::new();
    c.write_payload(&mut out);
    assert_eq!(out, [0x34, 0x12]);
    let c = Container::from_sorted_unchecked(&(0..=u16::MAX).collect::<Vec<_>>());
    assert_eq!(c.descriptor(), u16::MAX);
}

#[test]
fn run_mem_size() {
    let c = Container::from_range(10..=20);
    assert_eq!(c.kind(), Kind::Run);
    // The 16-byte slot plus the heap block (8-byte header and one 4-byte interval at least).
    assert!(c.mem_size() >= 16 + 8 + 4, "{}", c.mem_size());
}

#[test]
fn run_serialization() {
    serialization_suite(0x0008, true);
    let c = Container::from_range(0x0102..=0x0304);
    assert_eq!(c.descriptor(), 0);
    assert_eq!(c.serialized_payload_size(), 4);
    let mut out = Vec::new();
    c.write_payload(&mut out);
    assert_eq!(out, [0x02, 0x01, 0x02, 0x02]);
}

/// The public iterators and bitmaps wrap these types and must stay `Send + Sync`.
#[test]
fn auto_traits() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Container>();
    send_sync::<Iter<'static>>();
    send_sync::<IntoIter>();
    assert_eq!(core::mem::size_of::<Option<Container>>(), 16);
}

#[test]
fn mem_size() {
    assert_eq!(Container::new().mem_size(), 16);
    assert_eq!(Container::with_single(1).mem_size(), 16);
    let c = Container::from_sorted_unchecked(&(0..100).collect::<Vec<_>>());
    assert!(c.mem_size() >= 16 + 200);
    let c = Container::from_sorted_unchecked(&(0..5000).collect::<Vec<_>>());
    assert!(c.mem_size() >= 16 + 8192);
}

#[test]
fn internal_validate_rejects_broken_containers() {
    assert!(Container::new().internal_validate().is_err());
    assert!(Container::with_single(3).internal_validate().is_ok());
    // An array above the limit and a bitmap at the limit.
    let values: Vec<u16> = (0..=ARRAY_LIMIT as u16).collect();
    let big = Container::Array(ArrayStore::heap_from_slice(&values));
    assert!(big.internal_validate().is_err());
    let small = Container::Bitmap(BitmapStore::from_sorted_unchecked(&values[1..]));
    assert!(small.internal_validate().is_err());
    let mut wrong_len = BitmapStore::from_sorted_unchecked(&values);
    wrong_len.set_len(5000);
    assert!(Container::Bitmap(wrong_len).internal_validate().is_err());
    let unsorted = Container::Array(ArrayStore::heap_from_slice(&[1, 3, 2, 4, 5, 6, 7, 8]));
    assert!(unsorted.internal_validate().is_err());
    assert!(ArrayStore::try_from_sorted(&[1, 1]).is_err());
    assert!(ArrayStore::try_from_sorted(&[2, 1]).is_err());
    assert!(ArrayStore::try_from_sorted(&values).is_err());
    assert!(ArrayStore::try_from_sorted(&values[1..]).is_ok());
}

// ---------------------------------------------------------------------------------------------
// Iterators of containers (the representation-level iterators have their own unit tests)
// ---------------------------------------------------------------------------------------------

fn iterator_suite(seed: u64, runs: bool) {
    let mut rng = Rng::new(seed);
    for _ in 0..cases(120) {
        let model = random_model(&mut rng);
        let expected: Vec<u16> = model.iter().copied().collect();
        for repr in reprs_for(model.len(), runs) {
            if runs && repr != Repr::Run {
                continue;
            }
            let c = make(&model, repr);
            assert_eq!(c.iter().len(), expected.len());
            assert!(c.iter().eq(expected.iter().copied()));
            assert!(c.iter().rev().eq(expected.iter().rev().copied()));
            assert!(c.clone().into_iter().eq(expected.iter().copied()));
            assert!(
                c.clone()
                    .into_iter()
                    .rev()
                    .eq(expected.iter().rev().copied())
            );
            super::iter::tests::random_walk(&mut rng, &expected, || c.iter());
            super::iter::tests::random_walk(&mut rng, &expected, || c.clone().into_iter());
        }
    }
}

#[test]
fn iterators_array_bitmap() {
    iterator_suite(0x0009, false);
}

#[test]
fn run_iterators() {
    iterator_suite(0x000A, true);
}

// ---------------------------------------------------------------------------------------------
// The `roaring` crate as an oracle
// ---------------------------------------------------------------------------------------------

/// Header size of a one-container bitmap in the standard Roaring format.
fn roaring_header(has_run: bool) -> usize {
    if has_run { 4 + 4 + 1 } else { 4 + 4 + 8 }
}

fn roaring_kind(rb: &roaring::RoaringBitmap) -> Option<Kind> {
    let stats = rb.statistics();
    assert!(stats.n_containers <= 1);
    if stats.n_array_containers == 1 {
        Some(Kind::Array)
    } else if stats.n_bitset_containers == 1 {
        Some(Kind::Bitmap)
    } else if stats.n_run_containers == 1 {
        Some(Kind::Run)
    } else {
        None
    }
}

/// Checks that our container and the single-container roaring bitmap agree on the values, the
/// representation and the serialized size.
#[track_caller]
fn assert_same_as_roaring(c: &Container, rb: &roaring::RoaringBitmap) {
    assert_eq!(c.len(), rb.len());
    assert!(c.iter().map(u32::from).eq(rb.iter()));
    let Some(kind) = roaring_kind(rb) else {
        assert!(c.is_empty());
        return;
    };
    assert_eq!(c.kind(), kind, "{c:?}: representation differs from roaring");
    let run_count_bytes = if kind == Kind::Run { 2 } else { 0 };
    assert_eq!(
        c.serialized_payload_size() + run_count_bytes,
        rb.serialized_size() - roaring_header(kind == Kind::Run),
        "{c:?}: serialized size differs from roaring"
    );
}

fn roaring_of(model: &Model) -> roaring::RoaringBitmap {
    roaring::RoaringBitmap::from_sorted_iter(model.iter().map(|&v| v as u32)).unwrap()
}

#[test]
fn roaring_oracle_array_bitmap() {
    let mut rng = Rng::new(0x000B);
    for _ in 0..cases(200) {
        let ma = random_model(&mut rng);
        let mb = related_set(&mut rng, &ma);
        if ma.len() == 65536 || mb.len() == 65536 {
            continue;
        }
        let (a, b) = (
            make(&ma, reprs_for(ma.len(), false)[0]),
            make(&mb, Repr::Heap),
        );
        let b = if mb.len() as u64 > ARRAY_LIMIT {
            make(&mb, Repr::Bitmap)
        } else {
            b
        };
        let (ra, rb) = (roaring_of(&ma), roaring_of(&mb));
        assert_same_as_roaring(&a, &ra);
        assert_same_as_roaring(&b, &rb);
        assert_same_as_roaring(&a.union(&b), &(&ra | &rb));
        assert_same_as_roaring(&a.intersection(&b), &(&ra & &rb));
        assert_same_as_roaring(&a.difference(&b), &(&ra - &rb));
        assert_same_as_roaring(&a.symmetric_difference(&b), &(&ra ^ &rb));
        assert_eq!(a.is_subset(&b), ra.is_subset(&rb));
        assert_eq!(a.is_disjoint(&b), ra.is_disjoint(&rb));
        assert_eq!(a.intersection_len(&b), ra.intersection_len(&rb));
        // Point mutations and conversions.
        let (mut c, mut rc) = (a.clone(), ra.clone());
        for _ in 0..cases(32) {
            let v = rng.value(&ma);
            if rng.chance(50) {
                assert_eq!(c.insert(v), rc.insert(v as u32));
            } else {
                assert_eq!(c.remove(v), rc.remove(v as u32));
            }
            let r = rng.range(&ma);
            if rng.chance(50) {
                assert_eq!(c.remove_range(r.clone()), rc.remove_range(range32(&r)));
            } else if c.is_empty() {
                // The top level drops empty containers and creates new ones with
                // `Container::from_range` (a run above two values), like roaring.
                if model_range(&r) <= 2 {
                    c = Container::from_range(r.clone());
                    assert_eq!(c.len(), rc.insert_range(range32(&r)));
                }
            } else if !fills(&contents(&c).into_iter().collect(), &r) {
                assert_eq!(c.insert_range(r.clone()), rc.insert_range(range32(&r)));
            }
            assert_same_as_roaring(&c, &rc);
        }
    }
}

fn range32(r: &RangeInclusive<u16>) -> RangeInclusive<u32> {
    *r.start() as u32..=*r.end() as u32
}

/// `optimize` and `remove_run_compression` make the same decisions (return values and
/// representations) as the `roaring` crate.
#[test]
fn run_optimize_matches_roaring() {
    let mut rng = Rng::new(0x000C);
    for _ in 0..cases(400) {
        let model = random_model(&mut rng);
        let mut c = Container::from_sorted_unchecked(&model.iter().copied().collect::<Vec<_>>());
        let mut rb = roaring_of(&model);
        assert_same_as_roaring(&c, &rb);
        for _ in 0..3 {
            assert_eq!(c.optimize(), rb.optimize(), "optimize of {c:?}");
            assert_same_as_roaring(&c, &rb);
            check(&c, &model);
        }
        let mut c2 = c.clone();
        let mut rb2 = rb.clone();
        assert_eq!(c2.remove_run_compression(), rb2.remove_run_compression());
        assert_same_as_roaring(&c2, &rb2);
        check(&c2, &model);
        // Runs created by range insertions.
        let r = rng.range(&model);
        let mut c3 = Container::from_range(r.clone());
        let mut rb3 = roaring::RoaringBitmap::new();
        rb3.insert_range(range32(&r));
        assert_same_as_roaring(&c3, &rb3);
        let v = rng.value(&model);
        assert_eq!(c3.insert(v), rb3.insert(v as u32));
        assert_eq!(
            c3.remove(v.wrapping_add(7)),
            rb3.remove(v.wrapping_add(7) as u32)
        );
        assert_same_as_roaring(&c3, &rb3);
        assert_eq!(c3.optimize(), rb3.optimize());
        assert_same_as_roaring(&c3, &rb3);
        assert_eq!(c3.remove_run_compression(), rb3.remove_run_compression());
        assert_same_as_roaring(&c3, &rb3);
        // A range insertion filling the container creates a full run.
        let mut c4 = c.clone();
        let mut rb4 = rb.clone();
        assert_eq!(c4.insert_range(0..=u16::MAX), rb4.insert_range(0..=65535));
        assert_same_as_roaring(&c4, &rb4);
    }
}

#[test]
fn run_from_range_matches_roaring() {
    let mut rng = Rng::new(0x000D);
    for _ in 0..cases(300) {
        let r = rng.range(&Model::new());
        let c = Container::from_range(r.clone());
        let mut rb = roaring::RoaringBitmap::new();
        rb.insert_range(range32(&r));
        assert_same_as_roaring(&c, &rb);
    }
}

/// The `optimize` decisions that keep an array or a bitmap (no run container involved) match
/// the `roaring` crate. The conversions to runs are covered by `run_optimize_matches_roaring`.
#[test]
fn optimize_keeping_the_representation_matches_roaring() {
    let mut rng = Rng::new(0x000E);
    let mut kept = 0;
    for _ in 0..cases(400) {
        let model = random_model(&mut rng);
        let mut rb = roaring_of(&model);
        if rb.optimize() {
            continue;
        }
        kept += 1;
        let mut c = Container::from_sorted_unchecked(&model.iter().copied().collect::<Vec<_>>());
        assert!(!c.optimize(), "{c:?}");
        assert_same_as_roaring(&c, &rb);
        assert!(!c.remove_run_compression());
        check(&c, &model);
    }
    assert!(kept > 0);
}

#[test]
fn ensure_correct_store_converts() {
    let values: Vec<u16> = (0..5000).map(|v| v * 13).collect();
    let mut model: Model = values.iter().copied().collect();
    let mut c = Container::Array(ArrayStore::heap_from_slice(&values));
    assert!(c.ensure_correct_store());
    assert_eq!(c.kind(), Kind::Bitmap);
    check(&c, &model);
    assert!(!c.ensure_correct_store());
    let mut c = Container::Bitmap(BitmapStore::from_sorted_unchecked(&values[..4096]));
    model = values[..4096].iter().copied().collect();
    assert!(c.ensure_correct_store());
    assert_eq!(c.kind(), Kind::Array);
    check(&c, &model);
    assert!(!c.ensure_correct_store());
    let mut empty = Container::Bitmap(BitmapStore::new());
    assert!(empty.ensure_correct_store());
    assert_eq!(empty.kind(), Kind::Array);
    assert!(empty.is_empty());
}
