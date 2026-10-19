//! Differential tests against the `roaring` crate (0.11): on `u64` inputs against
//! `RoaringTreemap`, on `u32` inputs against `RoaringBitmap` (which has the more complete API:
//! `range`, `next_range`, `remove_smallest`, `statistics`, `from_lsb0_bytes`, ...).
//!
//! Both sides are built with the same sequence of calls and must agree on every return value
//! and on the content. Tests whose name contains `repr` also compare the container
//! representations (`statistics()` kinds, `optimize()` / `remove_run_compression()` return
//! values): `roaring128` intends to follow the same conversion rules, so a failure there is a
//! representation difference rather than a wrong set.

mod common;

use std::ops::{Bound, RangeInclusive};

use proptest::collection::vec;
use proptest::prelude::*;
use roaring::MultiOps as RoaringMultiOps;
use roaring::{RoaringBitmap, RoaringTreemap};
use roaring128::{MultiOps, RoaringBitmap128};

use common::*;

// -------------------------------------------------------------------------------------------------
// Domains and strategies
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dom {
    U32,
    U64,
}

impl Dom {
    fn max(self) -> u128 {
        match self {
            Dom::U32 => u32::MAX as u128,
            Dom::U64 => u64::MAX as u128,
        }
    }
}

fn anchor_in(d: Dom) -> BoxedStrategy<u128> {
    match d {
        Dom::U32 => prop_oneof![
            3 => Just(0u128),
            2 => Just((1u128 << 31) - 2 * KEY),
            2 => Just(u32::MAX as u128 - 4 * KEY + 1),
            1 => any::<u32>().prop_map(|v| (v & !0xFFFF) as u128),
        ]
        .boxed(),
        Dom::U64 => anchor_u64().prop_map(u128::from).boxed(),
    }
}

fn sparse_in(d: Dom) -> BoxedStrategy<u128> {
    match d {
        Dom::U32 => any::<u32>().prop_map(u128::from).boxed(),
        Dom::U64 => any::<u64>().prop_map(u128::from).boxed(),
    }
}

fn value_in(d: Dom) -> BoxedStrategy<u128> {
    let max = d.max();
    prop_oneof![
        6 => (anchor_in(d), 0..4 * KEY).prop_map(move |(a, o)| (a + o).min(max)),
        1 => sparse_in(d),
        1 => prop::sample::select(vec![0u128, 1, 0xFFFF, 0x1_0000, max - 0x1_0000, max - 1, max]),
    ]
    .boxed()
}

fn spec_in(d: Dom, max_pieces: usize) -> BoxedStrategy<SetSpec> {
    (
        vec(
            (piece_with(anchor_in(d), sparse_in(d)), build_mode()),
            0..=max_pieces,
        ),
        finish(),
    )
        .prop_map(|(pieces, finish)| SetSpec { pieces, finish })
        .boxed()
}

fn pair_in(d: Dom) -> BoxedStrategy<(SetSpec, SetSpec)> {
    (
        spec_in(d, 3),
        spec_in(d, 3),
        vec(
            (
                piece_with(anchor_in(d), sparse_in(d)),
                build_mode(),
                build_mode(),
            ),
            0..=2,
        ),
    )
        .prop_map(|(mut a, mut b, shared)| {
            for (p, ba, bb) in shared {
                a.pieces.push((p.clone(), ba));
                b.pieces.push((p, bb));
            }
            (a, b)
        })
        .boxed()
}

/// A range inside the domain; `Unbounded` ends mean the end of the domain.
fn range_in(d: Dom) -> BoxedStrategy<RangeSpec> {
    let max = d.max();
    let kind = |k: u8, v: u128| match k % 3 {
        0 => Bound::Included(v),
        1 => Bound::Excluded(v),
        _ => Bound::Unbounded,
    };
    prop_oneof![
        4 => (value_in(d), 0..200_000u128, 0..3u8, 0..3u8).prop_map(move |(s, len, ks, ke)| {
            RangeSpec(kind(ks, s), kind(ke, (s + len).min(max)))
        }),
        1 => (value_in(d), value_in(d), 0..3u8, 0..3u8).prop_map(move |(a, b, ks, ke)| {
            RangeSpec(kind(ks, a), kind(ke, b))
        }),
    ]
    .boxed()
}

/// Ranges that are cheap to insert (at most ~200k values).
fn insertable_in(d: Dom) -> BoxedStrategy<RangeSpec> {
    let max = d.max();
    (
        value_in(d),
        0..200_000u128,
        0..2u8,
        0..2u8,
        prop::bool::weighted(0.1),
    )
        .prop_map(move |(s, len, ks, ke, rev)| {
            let e = (s + len).min(max);
            let (s, e) = if rev { (e, s) } else { (s, e) };
            let b = |k: u8, v: u128| {
                if k == 0 {
                    Bound::Included(v)
                } else {
                    Bound::Excluded(v)
                }
            };
            RangeSpec(b(ks, s), b(ke, e))
        })
        .boxed()
}

/// The bounds for `roaring128`: an unbounded end is the end of the domain.
fn ours_range(r: RangeSpec, max: u128) -> RangeSpec {
    let end = match r.1 {
        Bound::Unbounded => Bound::Included(max),
        b => b,
    };
    RangeSpec(r.0, end)
}

fn narrow<T: TryFrom<u128>>(b: Bound<u128>) -> Bound<T>
where
    T::Error: std::fmt::Debug,
{
    match b {
        Bound::Included(v) => Bound::Included(T::try_from(v).unwrap()),
        Bound::Excluded(v) => Bound::Excluded(T::try_from(v).unwrap()),
        Bound::Unbounded => Bound::Unbounded,
    }
}

// -------------------------------------------------------------------------------------------------
// The reference side
// -------------------------------------------------------------------------------------------------

/// `(n_containers, n_array, n_bitset, n_run, values in arrays, values in bitsets, values in runs)`
type Kinds = (u64, u64, u64, u64, u64, u64, u64);

fn kinds_ours(rb: &RoaringBitmap128) -> Kinds {
    let s = rb.statistics();
    (
        s.n_containers,
        s.n_array_containers,
        s.n_bitset_containers,
        s.n_run_containers,
        s.n_values_array_containers,
        s.n_values_bitset_containers,
        s.n_values_run_containers,
    )
}

fn kinds_bitmap(b: &RoaringBitmap) -> Kinds {
    let s = b.statistics();
    (
        s.n_containers as u64,
        s.n_array_containers as u64,
        s.n_bitset_containers as u64,
        s.n_run_containers as u64,
        s.n_values_array_containers as u64,
        s.n_values_bitset_containers,
        s.n_values_run_containers as u64,
    )
}

fn kinds_treemap(t: &RoaringTreemap) -> Kinds {
    t.bitmaps()
        .map(|(_, b)| kinds_bitmap(b))
        .fold((0, 0, 0, 0, 0, 0, 0), |a, b| {
            (
                a.0 + b.0,
                a.1 + b.1,
                a.2 + b.2,
                a.3 + b.3,
                a.4 + b.4,
                a.5 + b.5,
                a.6 + b.6,
            )
        })
}

fn rrc_bitmap(b: &mut RoaringBitmap) -> bool {
    b.remove_run_compression()
}

/// `RoaringTreemap` has no `remove_run_compression`: apply it to every inner bitmap.
fn rrc_treemap(t: &mut RoaringTreemap) -> bool {
    let mut changed = false;
    *t = RoaringTreemap::from_bitmaps(t.bitmaps().map(|(p, b)| {
        let mut b = b.clone();
        changed |= b.remove_run_compression();
        (p, b)
    }));
    changed
}

fn select_bitmap(b: &RoaringBitmap, n: u64) -> Option<u128> {
    match u32::try_from(n) {
        Ok(n) => b.select(n).map(u128::from),
        Err(_) => None,
    }
}

fn select_treemap(t: &RoaringTreemap, n: u64) -> Option<u128> {
    t.select(n).map(u128::from)
}

/// A `roaring128` bitmap and a `roaring` bitmap built with the same calls.
macro_rules! dual {
    ($name:ident, $theirs:ty, $v:ty, $kinds:ident, $rrc:ident, $select:ident) => {
        #[derive(Clone)]
        struct $name {
            ours: RoaringBitmap128,
            theirs: $theirs,
            /// compare `optimize` / `remove_run_compression` return values
            strict: bool,
        }

        impl $name {
            const MAX: u128 = <$v>::MAX as u128;

            fn new(strict: bool) -> Self {
                $name {
                    ours: RoaringBitmap128::new(),
                    theirs: <$theirs>::new(),
                    strict,
                }
            }

            fn v(x: u128) -> $v {
                <$v>::try_from(x).expect("value outside the domain")
            }

            fn build(spec: &SetSpec, strict: bool) -> Self {
                let mut d = Self::new(strict);
                for (piece, build) in &spec.pieces {
                    d.add(piece.values(), *build);
                }
                match spec.finish {
                    Finish::Nothing => {}
                    Finish::Optimize => d.optimize(),
                    Finish::RemoveRunCompression => d.remove_run_compression(),
                    Finish::OptimizeThenRemoveRunCompression => {
                        d.optimize();
                        d.remove_run_compression();
                    }
                }
                d
            }

            fn add(&mut self, values: Vec<u128>, build: Build) {
                let mut values: Vec<u128> =
                    values.into_iter().filter(|&v| v <= Self::MAX).collect();
                match build {
                    Build::Insert => {
                        for v in values {
                            assert_eq!(
                                self.ours.insert(v),
                                self.theirs.insert(Self::v(v)),
                                "insert({v:#x})"
                            );
                        }
                    }
                    Build::Extend => {
                        self.ours.extend(values.iter().copied());
                        self.theirs.extend(values.iter().map(|&v| Self::v(v)));
                    }
                    Build::Ranges => {
                        values.sort_unstable();
                        values.dedup();
                        for r in intervals(&values) {
                            let (lo, hi) = (*r.start(), *r.end());
                            assert_eq!(
                                self.ours.insert_range(r),
                                self.theirs.insert_range(Self::v(lo)..=Self::v(hi)),
                                "insert_range({lo:#x}..={hi:#x})"
                            );
                        }
                    }
                    Build::Append => {
                        values.sort_unstable();
                        values.dedup();
                        let a = self
                            .ours
                            .append(values.iter().copied())
                            .map_err(|e| e.valid_until());
                        let b = self
                            .theirs
                            .append(values.iter().map(|&v| Self::v(v)))
                            .map_err(|e| e.valid_until());
                        assert_eq!(a, b, "append");
                    }
                    Build::Push => {
                        values.sort_unstable();
                        values.dedup();
                        for v in values {
                            if !self.try_push(v) {
                                assert_eq!(
                                    self.ours.insert(v),
                                    self.theirs.insert(Self::v(v)),
                                    "insert({v:#x})"
                                );
                            }
                        }
                    }
                }
            }

            /// `RoaringTreemap::try_push` (0.11.5) only compares with the maximum of the target
            /// 32-bit bucket, so it accepts values below the global maximum: the reference is
            /// only called when the push must succeed.
            fn try_push(&mut self, v: u128) -> bool {
                let ok = self.theirs.max().is_none_or(|m| v > u128::from(m));
                assert_eq!(self.ours.try_push(v).is_ok(), ok, "try_push({v:#x})");
                if ok {
                    assert!(
                        self.theirs.try_push(Self::v(v)).is_ok(),
                        "reference try_push({v:#x})"
                    );
                }
                ok
            }

            fn optimize(&mut self) {
                let (a, b) = (self.ours.optimize(), self.theirs.optimize());
                if self.strict {
                    assert_eq!(a, b, "optimize() return value");
                }
            }

            fn remove_run_compression(&mut self) {
                let (a, b) = (self.ours.remove_run_compression(), $rrc(&mut self.theirs));
                if self.strict {
                    assert_eq!(a, b, "remove_run_compression() return value");
                }
            }

            fn kinds(&self) -> (Kinds, Kinds) {
                (kinds_ours(&self.ours), $kinds(&self.theirs))
            }

            /// Same content (and a valid bitmap on our side).
            #[track_caller]
            fn check(&self) {
                assert_valid(&self.ours);
                assert_eq!(self.ours.len(), self.theirs.len(), "len");
                assert_eq!(self.ours.is_empty(), self.theirs.is_empty(), "is_empty");
                assert_eq!(self.ours.min(), self.theirs.min().map(u128::from), "min");
                assert_eq!(self.ours.max(), self.theirs.max().map(u128::from), "max");
                assert_same_seq(self.ours.iter(), self.theirs.iter().map(u128::from), "iter");
                assert_same_seq(
                    self.ours.iter().rev(),
                    self.theirs.iter().rev().map(u128::from),
                    "iter().rev()",
                );
                assert_eq!(
                    self.ours.iter().size_hint(),
                    self.theirs.iter().size_hint(),
                    "size_hint"
                );
            }

            #[track_caller]
            fn check_queries(&self, probes: &[u128], ranges: &[RangeSpec], selects: &[u64]) {
                for &p in probes {
                    if p <= Self::MAX {
                        assert_eq!(
                            self.ours.contains(p),
                            self.theirs.contains(Self::v(p)),
                            "contains({p:#x})"
                        );
                        assert_eq!(
                            self.ours.rank(p),
                            self.theirs.rank(Self::v(p)),
                            "rank({p:#x})"
                        );
                    } else {
                        assert!(!self.ours.contains(p));
                        assert_eq!(self.ours.rank(p), self.ours.len());
                    }
                }
                for &n in selects {
                    assert_eq!(self.ours.select(n), $select(&self.theirs, n), "select({n})");
                }
                for r in ranges {
                    let theirs = (narrow::<$v>(r.0), narrow::<$v>(r.1));
                    let ours = ours_range(*r, Self::MAX);
                    assert_eq!(
                        self.ours.contains_range(ours),
                        self.theirs.contains_range(theirs),
                        "contains_range({r:x?})"
                    );
                    assert_eq!(
                        self.ours.range_cardinality(ours),
                        self.theirs.range_cardinality(theirs),
                        "range_cardinality({r:x?})"
                    );
                }
            }

            fn insert_range(&mut self, r: RangeSpec) {
                let theirs = (narrow::<$v>(r.0), narrow::<$v>(r.1));
                let ours = ours_range(r, Self::MAX);
                assert_eq!(
                    self.ours.insert_range(ours),
                    self.theirs.insert_range(theirs),
                    "insert_range({r:x?})"
                );
            }

            fn remove_range(&mut self, r: RangeSpec) {
                let theirs = (narrow::<$v>(r.0), narrow::<$v>(r.1));
                let ours = ours_range(r, Self::MAX);
                assert_eq!(
                    self.ours.remove_range(ours),
                    self.theirs.remove_range(theirs),
                    "remove_range({r:x?})"
                );
            }

            fn binop(&self, other: &Self, op: SetOp, form: Form) -> Self {
                let ours = apply_op(op, form, &self.ours, &other.ours);
                let theirs = match op {
                    SetOp::Or => &self.theirs | &other.theirs,
                    SetOp::And => &self.theirs & &other.theirs,
                    SetOp::Sub => &self.theirs - &other.theirs,
                    SetOp::Xor => &self.theirs ^ &other.theirs,
                };
                $name {
                    ours,
                    theirs,
                    strict: self.strict,
                }
            }

            #[track_caller]
            fn check_relations(&self, other: &Self) {
                let (a, b) = (&self.ours, &other.ours);
                let (x, y) = (&self.theirs, &other.theirs);
                assert_eq!(a.union_len(b), x.union_len(y), "union_len");
                assert_eq!(
                    a.intersection_len(b),
                    x.intersection_len(y),
                    "intersection_len"
                );
                assert_eq!(a.difference_len(b), x.difference_len(y), "difference_len");
                assert_eq!(
                    a.symmetric_difference_len(b),
                    x.symmetric_difference_len(y),
                    "symmetric_difference_len"
                );
                assert_eq!(a.is_disjoint(b), x.is_disjoint(y), "is_disjoint");
                assert_eq!(a.is_subset(b), x.is_subset(y), "is_subset");
                assert_eq!(a.is_superset(b), x.is_superset(y), "is_superset");
                assert_eq!(a == b, x == y, "==");
            }
        }
    };
}

dual!(
    Dual32,
    RoaringBitmap,
    u32,
    kinds_bitmap,
    rrc_bitmap,
    select_bitmap
);
dual!(
    Dual64,
    RoaringTreemap,
    u64,
    kinds_treemap,
    rrc_treemap,
    select_treemap
);

// -------------------------------------------------------------------------------------------------
// Iterators
// -------------------------------------------------------------------------------------------------

/// The reference iterators, with `u128` items.
trait TheirIter {
    fn next_(&mut self) -> Option<u128>;
    fn next_back_(&mut self) -> Option<u128>;
    fn nth_(&mut self, n: usize) -> Option<u128>;
    fn nth_back_(&mut self, n: usize) -> Option<u128>;
    fn len_(&self) -> usize;
    fn advance_to_(&mut self, n: u128);
    fn advance_back_to_(&mut self, n: u128);
    /// `None` if the reference has no `next_range` (`RoaringTreemap`).
    fn next_range_(&mut self) -> Option<Option<RangeInclusive<u128>>>;
    fn next_range_back_(&mut self) -> Option<Option<RangeInclusive<u128>>>;
}

/// The methods shared by every reference iterator (items of type `$v`).
macro_rules! their_iter_common {
    ($v:ty) => {
        fn next_(&mut self) -> Option<u128> {
            self.next().map(u128::from)
        }
        fn next_back_(&mut self) -> Option<u128> {
            self.next_back().map(u128::from)
        }
        fn nth_(&mut self, n: usize) -> Option<u128> {
            self.nth(n).map(u128::from)
        }
        fn nth_back_(&mut self, n: usize) -> Option<u128> {
            self.nth_back(n).map(u128::from)
        }
        fn len_(&self) -> usize {
            self.len()
        }
        fn advance_to_(&mut self, n: u128) {
            self.advance_to(<$v>::try_from(n).unwrap())
        }
        fn advance_back_to_(&mut self, n: u128) {
            self.advance_back_to(<$v>::try_from(n).unwrap())
        }
    };
}

fn widen(r: RangeInclusive<u32>) -> RangeInclusive<u128> {
    u128::from(*r.start())..=u128::from(*r.end())
}

impl TheirIter for roaring::bitmap::Iter<'_> {
    their_iter_common!(u32);
    fn next_range_(&mut self) -> Option<Option<RangeInclusive<u128>>> {
        Some(self.next_range().map(widen))
    }
    fn next_range_back_(&mut self) -> Option<Option<RangeInclusive<u128>>> {
        Some(self.next_range_back().map(widen))
    }
}

impl TheirIter for roaring::bitmap::IntoIter {
    their_iter_common!(u32);
    fn next_range_(&mut self) -> Option<Option<RangeInclusive<u128>>> {
        Some(self.next_range().map(widen))
    }
    fn next_range_back_(&mut self) -> Option<Option<RangeInclusive<u128>>> {
        Some(self.next_range_back().map(widen))
    }
}

impl TheirIter for roaring::treemap::Iter<'_> {
    their_iter_common!(u64);
    fn next_range_(&mut self) -> Option<Option<RangeInclusive<u128>>> {
        None
    }
    fn next_range_back_(&mut self) -> Option<Option<RangeInclusive<u128>>> {
        None
    }
}

fn elem(all: &[u128], i: &prop::sample::Index, d: i8) -> u128 {
    if all.is_empty() {
        return 0;
    }
    let v = all[i.index(all.len())];
    match d {
        -1 => v.wrapping_sub(1),
        1 => v.wrapping_add(1),
        _ => v,
    }
}

/// Applies the same steps to both iterators; values outside the domain are skipped.
#[track_caller]
fn diff_iter_ops<I: RoaringIter, J: TheirIter>(
    mut ours: I,
    mut theirs: J,
    ops: &[IterOp],
    all: &[u128],
    max: u128,
) {
    assert_eq!(ours.len(), theirs.len_(), "initial len");
    for (step, op) in ops.iter().enumerate() {
        match op {
            IterOp::Next => assert_eq!(ours.next(), theirs.next_(), "step {step}: next"),
            IterOp::NextBack => assert_eq!(
                ours.next_back(),
                theirs.next_back_(),
                "step {step}: next_back"
            ),
            IterOp::Nth(n) => assert_eq!(ours.nth(*n), theirs.nth_(*n), "step {step}: nth({n})"),
            IterOp::NthBack(n) => assert_eq!(
                ours.nth_back(*n),
                theirs.nth_back_(*n),
                "step {step}: nth_back({n})"
            ),
            IterOp::AdvanceTo(_)
            | IterOp::AdvanceBackTo(_)
            | IterOp::AdvanceToElem(..)
            | IterOp::AdvanceBackToElem(..) => {
                let (n, back) = match op {
                    IterOp::AdvanceTo(n) => (*n, false),
                    IterOp::AdvanceBackTo(n) => (*n, true),
                    IterOp::AdvanceToElem(i, d) => (elem(all, i, *d), false),
                    IterOp::AdvanceBackToElem(i, d) => (elem(all, i, *d), true),
                    _ => unreachable!(),
                };
                if n <= max {
                    // `RoaringTreemap`'s iterator (0.11.5) can move backwards when advanced to a
                    // position before its current one (after being advanced past the end): the
                    // reference is only advanced when that moves it forward. `roaring128` is
                    // always called (advancing backwards must be a no-op).
                    if back {
                        let forward = ours.clone().next_back().is_some_and(|b| n < b);
                        ours.advance_back_to_(n);
                        if forward {
                            theirs.advance_back_to_(n);
                        }
                    } else {
                        let forward = ours.clone().next().is_some_and(|f| n > f);
                        ours.advance_to_(n);
                        if forward {
                            theirs.advance_to_(n);
                        }
                    }
                }
            }
            IterOp::NextRange => {
                if let Some(expected) = theirs.next_range_() {
                    assert_eq!(ours.next_range_(), expected, "step {step}: next_range");
                }
            }
            IterOp::NextRangeBack => {
                if let Some(expected) = theirs.next_range_back_() {
                    assert_eq!(
                        ours.next_range_back_(),
                        expected,
                        "step {step}: next_range_back"
                    );
                }
            }
            IterOp::CloneCheck => {}
        }
        assert_eq!(ours.len(), theirs.len_(), "step {step} ({op:?}): len");
        assert_eq!(
            ours.size_hint(),
            (theirs.len_(), Some(theirs.len_())),
            "step {step} ({op:?}): size_hint"
        );
    }
    loop {
        let (a, b) = (ours.next(), theirs.next_());
        assert_eq!(a, b, "rest");
        if a.is_none() {
            break;
        }
    }
}

fn iter_op_in(d: Dom) -> BoxedStrategy<IterOp> {
    prop_oneof![
        8 => iter_op(),
        2 => value_in(d).prop_map(IterOp::AdvanceTo),
        2 => value_in(d).prop_map(IterOp::AdvanceBackTo),
    ]
    .boxed()
}

// -------------------------------------------------------------------------------------------------
// Mutation sequences
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Op {
    Insert(u128),
    Remove(u128),
    TryPush(u128),
    InsertRange(RangeSpec),
    RemoveRange(RangeSpec),
    Extend(Vec<u128>),
    Append(Vec<u128>),
    Optimize,
    RemoveRunCompression,
    Clear,
}

fn op_in(d: Dom) -> BoxedStrategy<Op> {
    prop_oneof![
        6 => value_in(d).prop_map(Op::Insert),
        6 => value_in(d).prop_map(Op::Remove),
        2 => value_in(d).prop_map(Op::TryPush),
        4 => insertable_in(d).prop_map(Op::InsertRange),
        4 => range_in(d).prop_map(Op::RemoveRange),
        2 => vec(value_in(d), 0..50).prop_map(Op::Extend),
        1 => vec(value_in(d), 0..50).prop_map(Op::Append),
        2 => Just(Op::Optimize),
        2 => Just(Op::RemoveRunCompression),
        1 => Just(Op::Clear),
    ]
    .boxed()
}

macro_rules! apply_ops {
    ($d:expr, $ops:expr, $repr:expr) => {
        for (i, op) in $ops.iter().enumerate() {
            match op {
                Op::Insert(v) => assert_eq!(
                    $d.ours.insert(*v),
                    $d.theirs.insert((*v).try_into().unwrap()),
                    "insert"
                ),
                Op::Remove(v) => assert_eq!(
                    $d.ours.remove(*v),
                    $d.theirs.remove((*v).try_into().unwrap()),
                    "remove"
                ),
                Op::TryPush(v) => {
                    $d.try_push(*v);
                }
                Op::InsertRange(r) => $d.insert_range(*r),
                Op::RemoveRange(r) => $d.remove_range(*r),
                Op::Extend(v) => $d.add(v.clone(), Build::Extend),
                Op::Append(v) => {
                    // above the current maximum most of the time
                    let base = $d.ours.max().map_or(0, |m| m + 1);
                    let shifted: Vec<u128> = v
                        .iter()
                        .map(|x| base.saturating_add(x & 0xF_FFFF))
                        .collect();
                    $d.add(shifted, Build::Append)
                }
                Op::Optimize => $d.optimize(),
                Op::RemoveRunCompression => $d.remove_run_compression(),
                Op::Clear => {
                    $d.ours.clear();
                    $d.theirs.clear();
                }
            }
            $d.check();
            if $repr {
                let (a, b) = $d.kinds();
                assert_eq!(a, b, "step {i} ({op:?}): container kinds (ours, roaring)");
            }
        }
    };
}

// -------------------------------------------------------------------------------------------------
// Properties
// -------------------------------------------------------------------------------------------------

proptest! {
    #![proptest_config(config(96))]

    // ---- RoaringTreemap (u64)

    #[test]
    fn treemap_build_and_queries(
        spec in spec_in(Dom::U64, 4),
        extra in vec(value_in(Dom::U64), 0..32),
        ranges in vec(range_in(Dom::U64), 0..16),
        selects in vec(prop_oneof![0..100_000u64, any::<u64>()], 0..16),
    ) {
        let d = Dual64::build(&spec, false);
        d.check();
        let model: Model = d.ours.iter().collect();
        let mut p = probes(&model, 32);
        p.extend(extra);
        d.check_queries(&p, &ranges, &selects);
    }

    #[test]
    fn treemap_mutation_sequences(spec in spec_in(Dom::U64, 3), ops in vec(op_in(Dom::U64), 1..20)) {
        let mut d = Dual64::build(&spec, false);
        apply_ops!(d, ops, false);
    }

    #[test]
    fn treemap_repr_mutation_sequences(spec in spec_in(Dom::U64, 3), ops in vec(op_in(Dom::U64), 1..20)) {
        let mut d = Dual64::build(&spec, true);
        let (a, b) = d.kinds();
        prop_assert_eq!(a, b, "container kinds after build");
        apply_ops!(d, ops, true);
    }

    #[test]
    fn treemap_set_ops((a, b) in pair_in(Dom::U64), form in form()) {
        let (a, b) = (Dual64::build(&a, false), Dual64::build(&b, false));
        a.check_relations(&b);
        b.check_relations(&a);
        for op in SET_OPS {
            let r = a.binop(&b, op, form);
            r.check();
        }
        // MultiOps
        let ours = [&a.ours, &b.ours, &a.ours].union();
        let theirs = [&a.theirs, &b.theirs, &a.theirs].union();
        assert_same_seq(ours.iter(), theirs.iter().map(u128::from), "union()");
        let ours = [a.ours.clone(), b.ours.clone()].intersection();
        let theirs = [a.theirs.clone(), b.theirs.clone()].intersection();
        assert_same_seq(ours.iter(), theirs.iter().map(u128::from), "intersection()");
        let ours = [&b.ours, &a.ours].difference();
        let theirs = [&b.theirs, &a.theirs].difference();
        assert_same_seq(ours.iter(), theirs.iter().map(u128::from), "difference()");
        let ours = [a.ours.clone(), b.ours.clone(), b.ours.clone()].symmetric_difference();
        let theirs = [a.theirs.clone(), b.theirs.clone(), b.theirs.clone()].symmetric_difference();
        assert_same_seq(ours.iter(), theirs.iter().map(u128::from), "symmetric_difference()");
    }

    #[test]
    fn treemap_repr_after_set_ops((a, b) in pair_in(Dom::U64)) {
        let (a, b) = (Dual64::build(&a, true), Dual64::build(&b, true));
        for op in SET_OPS {
            let mut r = a.binop(&b, op, Form::RefRef);
            let (x, y) = r.kinds();
            prop_assert_eq!(x, y, "{:?}: container kinds", op);
            r.optimize();
            let (x, y) = r.kinds();
            prop_assert_eq!(x, y, "{:?} then optimize: container kinds", op);
        }
    }

    /// `RoaringTreemap`'s iterator (0.11.5) loses or resurrects values when `advance_to` /
    /// `advance_back_to` are mixed with iteration from the other end across 32-bit buckets
    /// (e.g. `{0, 2^32, 2^33}`: `next()` then `advance_back_to(2^32 + 1)` drops `2^32`). Only
    /// the reliable mixes are compared here: front only, back only, or `next`/`next_back`
    /// without advancing. `roaring128` is checked on every mix against the model and against
    /// `RoaringBitmap`.
    #[test]
    fn treemap_iter_ops(spec in spec_in(Dom::U64, 4), mode in 0..3u8, ops in vec(iter_op_in(Dom::U64), 0..40)) {
        let allowed = |op: &IterOp| match op {
            IterOp::Next | IterOp::Nth(_) => mode != 1,
            IterOp::NextBack | IterOp::NthBack(_) => mode != 0,
            IterOp::AdvanceTo(_) | IterOp::AdvanceToElem(..) => mode == 0,
            IterOp::AdvanceBackTo(_) | IterOp::AdvanceBackToElem(..) => mode == 1,
            IterOp::NextRange | IterOp::NextRangeBack | IterOp::CloneCheck => false,
        };
        let ops: Vec<IterOp> = ops.into_iter().filter(allowed).collect();
        let d = Dual64::build(&spec, false);
        let all: Vec<u128> = d.ours.iter().collect();
        diff_iter_ops(d.ours.iter(), d.theirs.iter(), &ops, &all, Dom::U64.max());
    }

    // ---- RoaringBitmap (u32)

    #[test]
    fn bitmap_build_and_queries(
        spec in spec_in(Dom::U32, 4),
        extra in vec(value_in(Dom::U32), 0..32),
        ranges in vec(range_in(Dom::U32), 0..16),
        selects in vec(prop_oneof![0..100_000u64, any::<u64>()], 0..16),
    ) {
        let d = Dual32::build(&spec, false);
        d.check();
        let model: Model = d.ours.iter().collect();
        let mut p = probes(&model, 32);
        p.extend(extra);
        d.check_queries(&p, &ranges, &selects);
    }

    #[test]
    fn bitmap_mutation_sequences(spec in spec_in(Dom::U32, 3), ops in vec(op_in(Dom::U32), 1..20)) {
        let mut d = Dual32::build(&spec, false);
        apply_ops!(d, ops, false);
    }

    #[test]
    fn bitmap_repr_mutation_sequences(spec in spec_in(Dom::U32, 3), ops in vec(op_in(Dom::U32), 1..20)) {
        let mut d = Dual32::build(&spec, true);
        let (a, b) = d.kinds();
        prop_assert_eq!(a, b, "container kinds after build");
        apply_ops!(d, ops, true);
    }

    #[test]
    fn bitmap_set_ops((a, b) in pair_in(Dom::U32), form in form()) {
        let (a, b) = (Dual32::build(&a, false), Dual32::build(&b, false));
        a.check_relations(&b);
        b.check_relations(&a);
        for op in SET_OPS {
            let r = a.binop(&b, op, form);
            r.check();
        }
    }

    #[test]
    fn bitmap_repr_after_set_ops((a, b) in pair_in(Dom::U32)) {
        let (a, b) = (Dual32::build(&a, true), Dual32::build(&b, true));
        for op in SET_OPS {
            let mut r = a.binop(&b, op, Form::RefRef);
            let (x, y) = r.kinds();
            prop_assert_eq!(x, y, "{:?}: container kinds", op);
            r.optimize();
            let (x, y) = r.kinds();
            prop_assert_eq!(x, y, "{:?} then optimize: container kinds", op);
        }
    }

    #[test]
    fn bitmap_iter_ops(spec in spec_in(Dom::U32, 4), ops in vec(iter_op_in(Dom::U32), 0..40)) {
        let d = Dual32::build(&spec, false);
        let all: Vec<u128> = d.ours.iter().collect();
        diff_iter_ops(d.ours.iter(), d.theirs.iter(), &ops, &all, Dom::U32.max());
        diff_iter_ops(d.ours.clone().into_iter(), d.theirs.clone().into_iter(), &ops, &all, Dom::U32.max());
    }

    #[test]
    fn bitmap_range_iter_ops(spec in spec_in(Dom::U32, 4), r in range_in(Dom::U32), ops in vec(iter_op_in(Dom::U32), 0..40)) {
        let d = Dual32::build(&spec, false);
        let theirs_r = (narrow::<u32>(r.0), narrow::<u32>(r.1));
        let ours_r = ours_range(r, Dom::U32.max());
        let all: Vec<u128> = d.ours.iter().collect();
        let theirs = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.theirs.range(theirs_r)));
        let ours = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.ours.range(ours_r)));
        match (ours, theirs) {
            (Ok(o), Ok(t)) => diff_iter_ops(o, t, &ops, &all, Dom::U32.max()),
            (Err(_), Err(_)) => {}
            (o, t) => prop_assert!(false, "range({:x?}): ours panicked: {}, roaring panicked: {}", r, o.is_err(), t.is_err()),
        }
        let theirs = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.theirs.clone().into_range(theirs_r)));
        let ours = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.ours.clone().into_range(ours_r)));
        match (ours, theirs) {
            (Ok(o), Ok(t)) => diff_iter_ops(o, t, &ops, &all, Dom::U32.max()),
            (Err(_), Err(_)) => {}
            (o, t) => prop_assert!(false, "into_range({:x?}): ours panicked: {}, roaring panicked: {}", r, o.is_err(), t.is_err()),
        }
    }

    #[test]
    fn bitmap_remove_smallest_biggest(spec in spec_in(Dom::U32, 4), n in prop_oneof![0..300u64, 0..200_000u64, Just(u64::MAX)]) {
        let d = Dual32::build(&spec, false);
        let mut a = d.clone();
        a.ours.remove_smallest(n);
        a.theirs.remove_smallest(n);
        a.check();
        let mut b = d.clone();
        b.ours.remove_biggest(n);
        b.theirs.remove_biggest(n);
        b.check();
    }

    #[test]
    fn bitmap_repr_remove_smallest_biggest(spec in spec_in(Dom::U32, 4), n in prop_oneof![0..300u64, 0..200_000u64]) {
        let d = Dual32::build(&spec, true);
        let mut a = d.clone();
        a.ours.remove_smallest(n);
        a.theirs.remove_smallest(n);
        let (x, y) = a.kinds();
        prop_assert_eq!(x, y, "remove_smallest: container kinds");
        let mut b = d.clone();
        b.ours.remove_biggest(n);
        b.theirs.remove_biggest(n);
        let (x, y) = b.kinds();
        prop_assert_eq!(x, y, "remove_biggest: container kinds");
    }

    #[test]
    fn bitmap_from_lsb0_bytes(offset in prop_oneof![0..100_000u32, any::<u32>()], bytes in prop_oneof![
        vec(any::<u8>(), 0..64),
        vec(prop_oneof![Just(0u8), Just(0xFF), any::<u8>()], 0..20_000),
    ]) {
        let fits = bytes.is_empty() || (offset as u64 + bytes.len() as u64 * 8) <= 1 << 32;
        prop_assume!(fits);
        let ours = RoaringBitmap128::from_lsb0_bytes(offset as u128, &bytes);
        let theirs = RoaringBitmap::from_lsb0_bytes(offset, &bytes);
        assert_valid(&ours);
        assert_same_seq(ours.iter(), theirs.iter().map(u128::from), "from_lsb0_bytes");
        prop_assert_eq!(kinds_ours(&ours), kinds_bitmap(&theirs), "from_lsb0_bytes: container kinds (repr)");
    }

    #[test]
    fn bitmap_repr_statistics(spec in spec_in(Dom::U32, 4)) {
        let d = Dual32::build(&spec, true);
        let (s1, s2) = (d.ours.statistics(), d.theirs.statistics());
        prop_assert_eq!(s1.max_value, s2.max_value.map(u128::from));
        prop_assert_eq!(s1.min_value, s2.min_value.map(u128::from));
        prop_assert_eq!(s1.cardinality, s2.cardinality);
        let (x, y) = d.kinds();
        prop_assert_eq!(x, y);
    }
}

#[test]
fn treemap_repr_full_u64_ranges() {
    let mut d = Dual64::new(true);
    d.insert_range(RangeSpec(
        Bound::Included(u64::MAX as u128 - 70_000),
        Bound::Unbounded,
    ));
    d.insert_range(RangeSpec(Bound::Unbounded, Bound::Excluded(70_000)));
    d.check();
    d.check_queries(
        &[
            0,
            69_999,
            70_000,
            u64::MAX as u128 - 70_000,
            u64::MAX as u128,
            u64::MAX as u128 + 1,
            u128::MAX,
        ],
        &[
            RangeSpec(Bound::Unbounded, Bound::Unbounded),
            RangeSpec(Bound::Included(0), Bound::Excluded(70_000)),
            RangeSpec(Bound::Excluded(0), Bound::Unbounded),
        ],
        &[0, 69_999, 70_000, 140_000, 140_001, u64::MAX],
    );
    let (a, b) = d.kinds();
    assert_eq!(a, b);
    d.optimize();
    d.remove_run_compression();
    d.check();
    let (a, b) = d.kinds();
    assert_eq!(a, b);
}
