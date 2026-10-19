//! Tests of the run container (`run.rs`).
//!
//! * Unit tests ported from the `roaring` crate (0.11, `store/interval_store.rs`), expressed
//!   through the store methods and the container-level free functions.
//! * Model-based tests against `BTreeSet<u16>` for every `RunStore` method and for every
//!   container operation with a run operand, for every operand representation (run × run,
//!   run × array, array × run, run × bitmap, bitmap × run). Results are checked for their
//!   values, their representation (which follows the `roaring` crate) and the `Container`
//!   invariants. The container iterators live elsewhere, so values are read out of each
//!   representation by the small helpers below.
//!
//! Under Miri the randomized tests run a handful of small cases.

use alloc::collections::BTreeSet;
use alloc::vec;
use alloc::vec::Vec;

use super::*;
use crate::container::Kind;

/// Random cases per test (per operand combination for the container operations).
const CASES: usize = if cfg!(miri) { 1 } else { 250 };

/// Largest random set (Miri keeps the sets small; the edges of the value space are still
/// covered because the random windows touch 0 and 65535).
const MAX_SET: usize = if cfg!(miri) { 500 } else { 1 << 16 };

/// Longest random range of the range operations.
const MAX_RANGE: u32 = if cfg!(miri) { 600 } else { 1 << 16 };

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn iv(start: u16, end: u16) -> Interval {
    Interval::new_unchecked(start, end)
}

fn len_of(start: u16, end: u16) -> u64 {
    iv(start, end).run_len()
}

fn store(pairs: &[(u16, u16)]) -> RunStore {
    let intervals: Vec<Interval> = pairs.iter().map(|&(s, e)| iv(s, e)).collect();
    RunStore::from_intervals_unchecked(&intervals)
}

fn pairs(runs: &RunStore) -> Vec<(u16, u16)> {
    runs.intervals()
        .iter()
        .map(|iv| (iv.start, iv.end))
        .collect()
}

fn run(pairs: &[(u16, u16)]) -> Container {
    Container::Run(store(pairs))
}

fn array(values: &[u16]) -> ArrayStore {
    ArrayStore::from_sorted_unchecked(values)
}

fn bitmap(values: impl IntoIterator<Item = u16>) -> BitmapStore {
    let mut bitmap = BitmapStore::new();
    for v in values {
        bitmap.insert(v);
    }
    assert_eq!(bitmap.len(), bitmap.clone().recount());
    bitmap
}

#[track_caller]
fn as_run(c: &Container) -> &RunStore {
    match c {
        Container::Run(runs) => runs,
        _ => panic!("expected a run container, got {:?}", c.kind()),
    }
}

fn run_values(runs: &RunStore) -> Vec<u16> {
    runs.intervals()
        .iter()
        .flat_map(|iv| iv.start..=iv.end)
        .collect()
}

fn word_values(words: &Words) -> Vec<u16> {
    let mut out = Vec::new();
    for (i, &w) in words.iter().enumerate() {
        let mut w = w;
        while w != 0 {
            out.push((i * 64) as u16 + w.trailing_zeros() as u16);
            w &= w - 1;
        }
    }
    out
}

fn values(c: &Container) -> Vec<u16> {
    match c {
        Container::Array(a) => a.as_slice().to_vec(),
        Container::Bitmap(b) => word_values(b.as_words()),
        Container::Run(r) => run_values(r),
    }
}

/// Checks the `Container` invariants (empty results are allowed).
#[track_caller]
fn check_container(c: &Container) {
    match c {
        Container::Array(a) => {
            assert!(a.len() <= ARRAY_LIMIT, "array with {} values", a.len());
            assert!(
                a.as_slice().windows(2).all(|w| w[0] < w[1]),
                "unsorted array"
            );
        }
        Container::Bitmap(b) => {
            assert!(b.len() > ARRAY_LIMIT, "bitmap with {} values", b.len());
            assert_eq!(
                b.len(),
                word_values(b.as_words()).len() as u64,
                "stale bitmap len"
            );
        }
        Container::Run(r) => {
            if !r.is_empty() {
                r.internal_validate().unwrap();
            }
        }
    }
}

#[track_caller]
fn assert_values(got: &[u16], expected: &[u16], context: &str) {
    if got != expected {
        let first = got
            .iter()
            .zip(expected)
            .position(|(g, e)| g != e)
            .unwrap_or(got.len().min(expected.len()));
        panic!(
            "{context}: {} values instead of {}, first difference at position {first} \
             (got {:?}, expected {:?})",
            got.len(),
            expected.len(),
            got.get(first),
            expected.get(first),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Model and random generation
// ---------------------------------------------------------------------------------------------

type Model = BTreeSet<u16>;

fn model_runs(m: &Model) -> Vec<Interval> {
    let mut out = Vec::new();
    let mut it = m.iter().copied();
    if let Some(first) = it.next() {
        let (mut s, mut e) = (first, first);
        for v in it {
            if v as u32 == e as u32 + 1 {
                e = v;
            } else {
                out.push(iv(s, e));
                (s, e) = (v, v);
            }
        }
        out.push(iv(s, e));
    }
    out
}

fn run_of(m: &Model) -> RunStore {
    RunStore::from_intervals_unchecked(&model_runs(m))
}

fn vec_of(m: &Model) -> Vec<u16> {
    m.iter().copied().collect()
}

fn array_of(m: &Model) -> ArrayStore {
    assert!(m.len() as u64 <= ARRAY_LIMIT);
    ArrayStore::from_sorted_unchecked(&vec_of(m))
}

fn bitmap_of(m: &Model) -> BitmapStore {
    bitmap(m.iter().copied())
}

fn make(m: &Model, kind: Kind) -> Container {
    match kind {
        Kind::Array => Container::Array(array_of(m)),
        Kind::Bitmap => {
            assert!(m.len() as u64 > ARRAY_LIMIT);
            Container::Bitmap(bitmap_of(m))
        }
        Kind::Run => Container::Run(run_of(m)),
    }
}

/// Whether `m` may be stored with the given representation.
fn fits(m: &Model, kind: Kind) -> bool {
    match kind {
        Kind::Array => m.len() as u64 <= ARRAY_LIMIT,
        Kind::Bitmap => m.len() as u64 > ARRAY_LIMIT,
        Kind::Run => true,
    }
}

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D))
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (`n > 0`).
    fn below(&mut self, n: u32) -> u32 {
        (self.next() % n as u64) as u32
    }

    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }

    /// A value, biased towards the container and word edges.
    fn index(&mut self) -> u16 {
        match self.below(10) {
            0 => {
                [0, 1, 63, 64, 65, 4095, 4096, 65471, 65472, 65534, 65535][self.below(11) as usize]
            }
            _ => self.below(1 << 16) as u16,
        }
    }

    /// A value at or near a value of `m` (to hit the merge / split paths), or any value.
    fn near(&mut self, m: &Model) -> u16 {
        if m.is_empty() || self.chance(25) {
            return self.index();
        }
        let probe = self.index();
        let anchor = m
            .range(probe..)
            .next()
            .or_else(|| m.last())
            .copied()
            .unwrap();
        let delta = self.below(5) as i32 - 2;
        (anchor as i32 + delta).clamp(0, u16::MAX as i32) as u16
    }

    /// A random range, often aligned with values of `m`.
    fn range(&mut self, m: &Model) -> RangeInclusive<u16> {
        let s = self.near(m);
        let len = match self.below(50) {
            0 => MAX_RANGE,
            1..=5 => self.below(1 << 13),
            _ => self.below(300),
        };
        let e = (s as u32 + len).min(u16::MAX as u32) as u16;
        if self.chance(50) {
            s..=e
        } else {
            (u16::MAX - e)..=(u16::MAX - s)
        }
    }
}

/// A random window of the value space, often touching 0 or 65535.
fn window(rng: &mut Rng) -> (u32, u32) {
    match rng.below(6) {
        0 | 1 => (0, u16::MAX as u32),
        2 => (0, 2999),
        3 => (62_536, u16::MAX as u32),
        4 => (30_000, 37_999),
        _ => {
            let (a, b) = (rng.below(1 << 16), rng.below(1 << 16));
            (a.min(b), a.max(b))
        }
    }
}

/// A random set with between `min` and `max` values (`min <= max`), shaped like real data: a
/// few runs of random lengths and/or scattered values inside a random window.
fn gen_set(rng: &mut Rng, min: usize, max: usize) -> Model {
    let mut values: Vec<u16> = Vec::new();
    let (lo, hi) = window(rng);
    let width = hi - lo + 1;
    if rng.chance(20) {
        // Random density over (a part of) the window: many short runs and gaps.
        let hi = if cfg!(miri) { hi.min(lo + 1_000) } else { hi };
        let density = 5 + rng.below(90);
        values.extend(
            (lo..=hi)
                .filter(|_| rng.below(100) < density)
                .map(|v| v as u16),
        );
        return finish_set(rng, values, min, max);
    }
    let max_len = match rng.below(4) {
        0 => 1,
        1 => 4,
        2 => 100,
        _ => width.min(max as u32).max(1),
    };
    let pieces = 1 + rng.below(if cfg!(miri) { 4 } else { 48 });
    for _ in 0..pieces {
        if values.len() >= max {
            break;
        }
        let start = lo + rng.below(width);
        let end = (start + rng.below(max_len)).min(hi);
        values.extend((start..=end).map(|v| v as u16));
    }
    finish_set(rng, values, min, max)
}

/// Sorts and deduplicates `values`, then pads or trims them to `min..=max` values.
fn finish_set(rng: &mut Rng, mut values: Vec<u16>, min: usize, max: usize) -> Model {
    values.sort_unstable();
    values.dedup();
    while values.len() < min {
        let missing = (min - values.len()) as u32;
        let start = rng.below(1 << 16);
        let end = (start + missing + rng.below(missing + 1)).min(u16::MAX as u32);
        values.extend((start..=end).map(|v| v as u16));
        values.sort_unstable();
        values.dedup();
    }
    if values.len() > max {
        // Drop a random part of the values while keeping some structure.
        let cut = rng.below((values.len() - max) as u32 + 1) as usize;
        values.drain(..cut);
        values.truncate(max);
    }
    Model::from_iter(values)
}

/// Bounds on the cardinality of a random operand of the given representation.
fn bounds(kind: Kind) -> (usize, usize) {
    let bitmap_max = if cfg!(miri) { 4_300 } else { 1 << 16 };
    match kind {
        Kind::Array => (0, ARRAY_LIMIT as usize),
        Kind::Bitmap => (ARRAY_LIMIT as usize + 1, bitmap_max),
        Kind::Run => (0, MAX_SET),
    }
}

/// A second operand related to `a` (equal, subset, superset, shifted, complement, ...) so that
/// the subset / equality / disjointness answers are not almost always `false`.
fn derive(rng: &mut Rng, a: &Model, kind: Kind) -> Model {
    let (min, max) = bounds(kind);
    let b = match rng.below(9) {
        0 => a.clone(),
        1 => a.iter().copied().filter(|_| rng.below(10) != 0).collect(),
        2 => {
            let mut b = a.clone();
            for _ in 0..1 + rng.below(4) {
                let r = rng.range(a);
                b.retain(|v| !r.contains(v));
            }
            b
        }
        3 => {
            let extra = gen_set(rng, 0, MAX_SET.min(2000));
            a.union(&extra).copied().collect()
        }
        4 => a.iter().map(|&v| v.saturating_add(1)).collect(),
        6 => {
            // Same cardinality (usually), a few values moved: equality and subset checks must
            // look at the values, not only at the lengths and the end points.
            let mut b = a.clone();
            for _ in 0..1 + rng.below(3) {
                let gone = rng.near(a);
                if b.remove(&gone) {
                    b.insert(rng.near(a));
                }
            }
            b
        }
        5 => {
            let (lo, hi) = window(rng);
            let hi = if cfg!(miri) { hi.min(lo + 4_400) } else { hi };
            (lo..=hi)
                .map(|v| v as u16)
                .filter(|v| !a.contains(v))
                .collect()
        }
        _ => gen_set(rng, min, max),
    };
    if (min..=max).contains(&b.len()) {
        b
    } else {
        gen_set(rng, min, max)
    }
}

/// Hand-picked sets around the edges (cheap ones first; Miri only uses the small ones, its
/// bitmap operands come from the random bitmap tests).
fn special_sets() -> Vec<Model> {
    let mut sets: Vec<Model> = vec![
        Model::new(),
        Model::from([0]),
        Model::from([u16::MAX]),
        Model::from([0, u16::MAX]),
        (60..=70).chain(126..=130).collect(),
    ];
    // Not even built under Miri: building these models alone takes minutes there.
    if !cfg!(miri) {
        sets.extend([
            (0..=4096).collect(),
            (0..=4095).collect(),
            (0..=8192).step_by(2).collect(),
            (61_439..=u16::MAX).collect(),
            (0..=u16::MAX).collect(),
            (1..u16::MAX).collect(),
            (0..=u16::MAX).filter(|v| v % 64 != 63).collect(),
            (0..=u16::MAX).step_by(3).collect(),
        ]);
    }
    sets
}

// ---------------------------------------------------------------------------------------------
// Ported from `roaring` (store/interval_store.rs)
// ---------------------------------------------------------------------------------------------

#[test]
fn insert_empty() {
    let mut s = store(&[]);
    assert!(s.insert(1));
    assert_eq!(pairs(&s), [(1, 1)]);
}

#[test]
fn insert_consecutive_begin() {
    let mut s = store(&[(0, 0)]);
    assert!(s.insert(1));
    assert_eq!(pairs(&s), [(0, 1)]);
}

#[test]
fn insert_consecutive_end() {
    let mut s = store(&[(1, 1)]);
    assert!(s.insert(0));
    assert_eq!(pairs(&s), [(0, 1)]);
}

#[test]
fn insert_consecutive_begin_end() {
    let mut s = store(&[(0, 0), (2, 2)]);
    assert!(s.insert(1));
    assert_eq!(pairs(&s), [(0, 2)]);
}

#[test]
fn insert_arbitrary() {
    let mut s = store(&[(0, 3), (9, 10)]);
    assert!(s.insert(5));
    assert_eq!(pairs(&s), [(0, 3), (5, 5), (9, 10)]);
}

#[test]
fn insert_u16_max() {
    let mut s = store(&[(0, 3)]);
    assert!(s.insert(u16::MAX));
    assert_eq!(pairs(&s), [(0, 3), (u16::MAX, u16::MAX)]);
}

#[test]
fn insert_u16_max_consecutive() {
    let mut s = store(&[(0, u16::MAX - 1)]);
    assert!(s.insert(u16::MAX));
    assert_eq!(pairs(&s), [(0, u16::MAX)]);
}

#[test]
fn insert_consecutive_end_with_extra() {
    let mut s = store(&[(65079, 65079), (65179, 65179)]);
    assert!(s.insert(65080));
    assert_eq!(pairs(&s), [(65079, 65080), (65179, 65179)]);
}

#[test]
fn insert_existing() {
    let mut s = store(&[(5, 10)]);
    assert!(!s.insert(5));
    assert!(!s.insert(7));
    assert!(!s.insert(10));
    assert_eq!(pairs(&s), [(5, 10)]);
}

#[test]
fn insert_range_empty() {
    let mut s = store(&[]);
    assert_eq!(s.insert_range(1..=2), len_of(1, 2));
    assert_eq!(pairs(&s), [(1, 2)]);
}

#[test]
fn insert_range_overlap_begin() {
    let mut s = store(&[(1, 20)]);
    assert_eq!(s.insert_range(5..=50), len_of(21, 50));
    assert_eq!(pairs(&s), [(1, 50)]);
}

#[test]
fn insert_range_overlap_end() {
    let mut s = store(&[(10, 20)]);
    assert_eq!(s.insert_range(5..=15), len_of(5, 9));
    assert_eq!(pairs(&s), [(5, 20)]);
}

#[test]
fn insert_range_overlap_begin_end() {
    let mut s = store(&[(10, 20), (40, 60)]);
    assert_eq!(s.insert_range(15..=50), len_of(21, 39));
    assert_eq!(pairs(&s), [(10, 60)]);
}

#[test]
fn insert_range_consecutive_begin() {
    let mut s = store(&[(10, 20)]);
    assert_eq!(s.insert_range(21..=50), len_of(21, 50));
    assert_eq!(pairs(&s), [(10, 50)]);
}

#[test]
fn insert_range_consecutive_begin_overlap_end() {
    let mut s = store(&[(10, 20), (40, 60)]);
    assert_eq!(s.insert_range(21..=50), len_of(21, 39));
    assert_eq!(pairs(&s), [(10, 60)]);
}

#[test]
fn insert_range_consecutive_end() {
    let mut s = store(&[(50, 70)]);
    assert_eq!(s.insert_range(21..=49), len_of(21, 49));
    assert_eq!(pairs(&s), [(21, 70)]);
}

#[test]
fn insert_range_consecutive_begin_end() {
    let mut s = store(&[(10, 20), (50, 70)]);
    assert_eq!(s.insert_range(21..=49), len_of(21, 49));
    assert_eq!(pairs(&s), [(10, 70)]);
}

#[test]
fn insert_range_no_overlap() {
    let mut s = store(&[(10, 20), (50, 70)]);
    assert_eq!(s.insert_range(25..=30), len_of(25, 30));
    assert_eq!(pairs(&s), [(10, 20), (25, 30), (50, 70)]);
}

#[test]
fn insert_range_u16_max_no_overlap() {
    let mut s = store(&[(10, 20), (50, 70)]);
    assert_eq!(s.insert_range(90..=u16::MAX), len_of(90, u16::MAX));
    assert_eq!(pairs(&s), [(10, 20), (50, 70), (90, u16::MAX)]);
}

#[test]
fn insert_range_u16_max_overlap_begin() {
    let mut s = store(&[(10, 20), (50, 70)]);
    assert_eq!(s.insert_range(70..=u16::MAX), len_of(71, u16::MAX));
    assert_eq!(pairs(&s), [(10, 20), (50, u16::MAX)]);
}

#[test]
fn insert_range_u16_max_overlap_all() {
    let mut s = store(&[(10, 20), (50, 70)]);
    assert_eq!(
        s.insert_range(0..=u16::MAX),
        len_of(0, u16::MAX) - len_of(10, 20) - len_of(50, 70)
    );
    assert_eq!(pairs(&s), [(0, u16::MAX)]);
}

#[test]
fn insert_range_overlap_some() {
    let mut s = store(&[(10, 20), (50, 70), (500, 700)]);
    assert_eq!(
        s.insert_range(0..=100),
        len_of(0, 100) - len_of(10, 20) - len_of(50, 70)
    );
    assert_eq!(pairs(&s), [(0, 100), (500, 700)]);
}

#[test]
fn insert_range_begin_overlap_consecutive_end() {
    let mut s = store(&[(2, 10), (12, 700)]);
    assert_eq!(s.insert_range(2..=11), 1);
    assert_eq!(pairs(&s), [(2, 700)]);
}

#[test]
fn insert_range_pin_1() {
    let mut s = store(&[(65079, 65079)]);
    assert_eq!(s.insert_range(65080..=65080), 1);
    assert_eq!(pairs(&s), [(65079, 65080)]);
}

#[test]
#[allow(clippy::reversed_empty_ranges)]
fn insert_and_remove_empty_range() {
    let mut s = store(&[(10, 20)]);
    assert_eq!(s.insert_range(30..=29), 0);
    assert_eq!(s.remove_range(15..=14), 0);
    assert_eq!(pairs(&s), [(10, 20)]);
}

#[test]
fn push_empty() {
    let mut s = store(&[]);
    assert!(s.push(80));
    assert_eq!(pairs(&s), [(80, 80)]);
}

#[test]
fn push_new_max() {
    let mut s = store(&[(50, 70)]);
    assert!(s.push(80));
    assert_eq!(pairs(&s), [(50, 70), (80, 80)]);
}

#[test]
fn push_new_max_consecutive() {
    let mut s = store(&[(50, 70)]);
    assert!(s.push(71));
    assert_eq!(pairs(&s), [(50, 71)]);
}

#[test]
fn push_existing() {
    let mut s = store(&[(50, 70)]);
    assert!(!s.push(60));
    assert!(!s.push(70));
    assert_eq!(pairs(&s), [(50, 70)]);
}

#[test]
fn push_non_existing_non_max() {
    let mut s = store(&[(50, 70)]);
    assert!(!s.push(10));
    assert_eq!(pairs(&s), [(50, 70)]);
}

#[test]
fn push_existing_u16_max() {
    let mut s = store(&[(50, u16::MAX)]);
    assert!(!s.push(u16::MAX));
    assert_eq!(pairs(&s), [(50, u16::MAX)]);
}

#[test]
fn push_new_u16_max() {
    let mut s = store(&[(50, u16::MAX - 1)]);
    assert!(s.push(u16::MAX));
    assert_eq!(pairs(&s), [(50, u16::MAX)]);
}

#[test]
fn remove_end_of_interval() {
    let mut s = store(&[(50, 60)]);
    assert!(s.remove(60));
    assert_eq!(pairs(&s), [(50, 59)]);
}

#[test]
fn remove_begin_of_interval() {
    let mut s = store(&[(50, 60)]);
    assert!(s.remove(50));
    assert_eq!(pairs(&s), [(51, 60)]);
}

#[test]
fn remove_middle() {
    let mut s = store(&[(1, 3)]);
    assert!(s.remove(2));
    assert_eq!(pairs(&s), [(1, 1), (3, 3)]);
}

#[test]
fn remove_nothing() {
    let mut s = store(&[(50, 60)]);
    assert!(!s.remove(90));
    assert!(!s.remove(49));
    assert_eq!(pairs(&s), [(50, 60)]);
}

#[test]
fn remove_u16_max() {
    let mut s = store(&[(50, u16::MAX)]);
    assert!(s.remove(u16::MAX));
    assert_eq!(pairs(&s), [(50, u16::MAX - 1)]);
}

#[test]
fn remove_interval() {
    let mut s = store(&[(50, 50)]);
    assert!(s.remove(50));
    assert!(s.is_empty());
}

#[test]
fn remove_range_exact_one() {
    let mut s = store(&[(40, 60)]);
    assert_eq!(s.remove_range(40..=60), 21);
    assert!(s.is_empty());
}

#[test]
fn remove_range_one_with_extra_1() {
    let mut s = store(&[(40, 60)]);
    assert_eq!(s.remove_range(40..=70), 21);
    assert!(s.is_empty());
}

#[test]
fn remove_range_one_with_extra_2() {
    let mut s = store(&[(40, 60), (400, 600), (4000, 6000)]);
    assert_eq!(s.remove_range(40..=70), 21);
    assert_eq!(pairs(&s), [(400, 600), (4000, 6000)]);
}

#[test]
fn remove_range_exact_many() {
    let mut s = store(&[(40, 60), (80, 90), (100, 200)]);
    assert_eq!(
        s.remove_range(40..=200),
        len_of(40, 60) + len_of(80, 90) + len_of(100, 200)
    );
    assert!(s.is_empty());
}

#[test]
fn remove_range_begin_exact_overlap_end_one() {
    let mut s = store(&[(40, 60), (70, 90)]);
    assert_eq!(s.remove_range(40..=80), len_of(40, 60) + len_of(70, 80));
    assert_eq!(pairs(&s), [(81, 90)]);
}

#[test]
fn remove_range_begin_overlap_end_exact_one() {
    let mut s = store(&[(40, 60), (70, 90)]);
    assert_eq!(s.remove_range(50..=90), len_of(70, 90) + len_of(50, 60));
    assert_eq!(pairs(&s), [(40, 49)]);
}

#[test]
fn remove_range_begin_no_overlap_end_exact_one_1() {
    let mut s = store(&[(40, 60), (70, 90)]);
    assert_eq!(s.remove_range(30..=90), len_of(70, 90) + len_of(40, 60));
    assert!(s.is_empty());
}

#[test]
fn remove_range_begin_no_overlap_end_exact_one_2() {
    let mut s = store(&[(40, 60), (70, 90), (700, 900)]);
    assert_eq!(s.remove_range(30..=90), len_of(70, 90) + len_of(40, 60));
    assert_eq!(pairs(&s), [(700, 900)]);
}

#[test]
fn remove_range_both_overlap() {
    let mut s = store(&[(40, 60), (70, 90)]);
    assert_eq!(s.remove_range(50..=80), len_of(70, 80) + len_of(50, 60));
    assert_eq!(pairs(&s), [(40, 49), (81, 90)]);
}

#[test]
fn remove_range_begin_overlap() {
    let mut s = store(&[(40, 60)]);
    assert_eq!(s.remove_range(50..=100), len_of(50, 60));
    assert_eq!(pairs(&s), [(40, 49)]);
}

#[test]
fn remove_range_begin_overlap_many() {
    let mut s = store(&[(40, 60), (80, 100), (200, 500)]);
    assert_eq!(
        s.remove_range(50..=1000),
        len_of(50, 60) + len_of(80, 100) + len_of(200, 500)
    );
    assert_eq!(pairs(&s), [(40, 49)]);
}

#[test]
fn remove_range_end_overlap() {
    let mut s = store(&[(40, 60)]);
    assert_eq!(s.remove_range(20..=50), len_of(40, 50));
    assert_eq!(pairs(&s), [(51, 60)]);
}

#[test]
fn remove_range_end_overlap_many() {
    let mut s = store(&[(40, 60), (100, 500), (800, 900)]);
    assert_eq!(
        s.remove_range(20..=850),
        len_of(40, 60) + len_of(100, 500) + len_of(800, 850)
    );
    assert_eq!(pairs(&s), [(851, 900)]);
}

#[test]
fn remove_range_no_overlap() {
    let mut s = store(&[(40, 60)]);
    assert_eq!(s.remove_range(20..=80), len_of(40, 60));
    assert!(s.is_empty());
}

#[test]
fn remove_range_no_overlap_many() {
    let mut s = store(&[(40, 60), (400, 600), (4000, 6000)]);
    assert_eq!(
        s.remove_range(20..=60000),
        len_of(40, 60) + len_of(400, 600) + len_of(4000, 6000)
    );
    assert!(s.is_empty());
}

#[test]
fn remove_range_complete_overlap() {
    let mut s = store(&[(51, 6000)]);
    assert_eq!(s.remove_range(500..=600), len_of(500, 600));
    assert_eq!(pairs(&s), [(51, 499), (601, 6000)]);
}

#[test]
fn remove_range_nothing() {
    let mut s = store(&[]);
    assert_eq!(s.remove_range(50000..=60000), 0);
    assert!(s.is_empty());
    let mut s = store(&[(10, 20), (40, 50)]);
    assert_eq!(s.remove_range(25..=35), 0);
    assert_eq!(pairs(&s), [(10, 20), (40, 50)]);
}

#[test]
fn remove_range_with_extra() {
    let mut s = store(&[(38161, 38162), (40562, 40562)]);
    assert_eq!(s.remove_range(38162..=38163), 1);
    assert_eq!(pairs(&s), [(38161, 38161), (40562, 40562)]);
}

#[test]
fn remove_smallest_one() {
    let mut s = store(&[(40, 60)]);
    s.remove_smallest(500);
    assert!(s.is_empty());
}

#[test]
fn remove_smallest_many_1() {
    let mut s = store(&[(0, 99), (400, 600), (4000, 6000)]);
    s.remove_smallest(200);
    assert_eq!(pairs(&s), [(500, 600), (4000, 6000)]);
}

#[test]
fn remove_smallest_many_2() {
    let mut s = store(&[(0, 99), (400, 599), (4000, 6000)]);
    s.remove_smallest(500);
    assert_eq!(pairs(&s), [(4200, 6000)]);
}

#[test]
fn remove_biggest_one() {
    let mut s = store(&[(40, 60)]);
    s.remove_biggest(500);
    assert!(s.is_empty());
}

#[test]
fn remove_biggest_many_1() {
    let mut s = store(&[(0, 99), (400, 600), (5901, 6000)]);
    s.remove_biggest(200);
    assert_eq!(pairs(&s), [(0, 99), (400, 500)]);
}

#[test]
fn remove_biggest_many_2() {
    let mut s = store(&[(1, 6000), (8401, 8600), (9901, 10000)]);
    s.remove_biggest(500);
    assert_eq!(pairs(&s), [(1, 5800)]);
}

#[test]
fn remove_smallest_exact_interval_boundary() {
    // roaring #359: removing exactly the length of the first interval drops it entirely.
    let mut s = store(&[(0, 2), (4, 4)]);
    s.remove_smallest(3);
    assert_eq!(pairs(&s), [(4, 4)]);
}

#[test]
fn remove_biggest_exact_interval_boundary() {
    // roaring #359: removing exactly the length of the last interval drops it entirely.
    let mut s = store(&[(2, 2), (4, 8)]);
    s.remove_biggest(5);
    assert_eq!(pairs(&s), [(2, 2)]);
}

#[test]
fn remove_smallest_biggest_zero() {
    let mut s = store(&[(2, 2), (4, 8)]);
    s.remove_smallest(0);
    s.remove_biggest(0);
    assert_eq!(pairs(&s), [(2, 2), (4, 8)]);
}

#[test]
fn contains_index() {
    let s = store(&[(1, 600), (1401, 1600), (15901, 16000)]);
    assert!(s.contains(5));
    assert!(s.contains(16000));
    assert!(!s.contains(0));
    assert!(!s.contains(601));
    assert!(!s.contains(16001));
    assert!(!store(&[]).contains(0));
}

#[test]
fn contains_range() {
    let s = store(&[(1, 600), (1401, 1600), (15901, 16000)]);
    assert!(s.contains_range(1..=500));
    assert!(!s.contains_range(1..=1500));
    assert!(s.contains_range(1..=1));
    assert!(s.contains_range(1401..=1600));
    assert!(!s.contains_range(1400..=1600));
    assert!(!s.contains_range(16000..=16001));
    assert!(!store(&[]).contains_range(3..=3));
}

#[test]
fn is_disjoint_runs() {
    let a = run(&[(1, 600), (1401, 1600), (15901, 16000)]);
    for (b, disjoint) in [
        (run(&[(601, 1200)]), true),
        (run(&[(600, 1200)]), false),
        (run(&[(15800, 15905)]), false),
    ] {
        assert!(!is_disjoint(&a, &a));
        assert!(!is_disjoint(&b, &b));
        assert_eq!(is_disjoint(&a, &b), disjoint);
        assert_eq!(is_disjoint(&b, &a), disjoint);
    }
}

#[test]
fn is_disjoint_array_store() {
    let a = Container::Array(array(&[0, 60, 200, 500]));
    assert!(is_disjoint(&run(&[(70, 199)]), &a));
    assert!(is_disjoint(&a, &run(&[(70, 199)])));
    assert!(!is_disjoint(&run(&[(1, 400)]), &a));
    assert!(!is_disjoint(&a, &run(&[(1, 400)])));
}

#[test]
fn is_disjoint_bitmap_store() {
    let b = Container::Bitmap(bitmap([500, 5001, 20, 40]));
    assert!(is_disjoint(&run(&[(1000, 4000), (8000, 10000)]), &b));
    assert!(is_disjoint(&b, &run(&[(1000, 4000), (8000, 10000)])));
    assert!(!is_disjoint(&run(&[(1, 400)]), &b));
    assert!(!is_disjoint(&b, &run(&[(1, 400)])));
}

#[test]
fn is_subset_runs() {
    let a = run(&[(1500, 1600)]);
    let b = run(&[(1, 600), (1401, 1600), (15901, 16000)]);
    let c = run(&[(50, 700)]);
    assert!(is_subset(&a, &a));
    assert!(is_subset(&b, &b));
    assert!(is_subset(&a, &b));
    assert!(!is_subset(&b, &a));
    assert!(!is_subset(&c, &b));
    assert!(!is_subset(&b, &c));
}

#[test]
fn intersection_len_runs() {
    let a = run(&[(11, 20), (51, 80), (111, 120)]);
    let b = run(&[(1, 20), (41, 80), (101, 120)]);
    let expected = len_of(11, 20) + len_of(51, 80) + len_of(111, 120);
    assert_eq!(intersection_len(&a, &b), expected);
    let c = run(&[(1, 80), (101, 120)]);
    assert_eq!(intersection_len(&a, &c), expected);
    assert_eq!(intersection_len(&c, &a), expected);
    let (d, e) = (run(&[(1, 2000)]), run(&[(1001, 3000)]));
    assert_eq!(intersection_len(&d, &e), len_of(1001, 2000));
    assert_eq!(intersection_len(&e, &d), len_of(1001, 2000));
}

#[test]
fn intersection_len_bitmap() {
    let b = Container::Bitmap(bitmap([500, 5001, 20, 40, 60]));
    assert_eq!(intersection_len(&run(&[(20, 600)]), &b), 4);
    let b = Container::Bitmap(bitmap(0..=200));
    assert_eq!(intersection_len(&run(&[(20, 600)]), &b), len_of(20, 200));
    // (The roaring test uses overlapping intervals here, which are not a valid run store.)
    let b = Container::Bitmap(bitmap(0..=20000));
    assert_eq!(
        intersection_len(&run(&[(20, 6000), (6002, 33333)]), &b),
        len_of(20, 6000) + len_of(6002, 20000)
    );
    assert_eq!(
        intersection_len(&run(&[(64, 6400), (7680, 64000)]), &b),
        len_of(64, 6400) + len_of(7680, 20000)
    );
    let b = Container::Bitmap(bitmap(0..=20005));
    assert_eq!(
        intersection_len(&run(&[(64, 6400), (7680, 64000)]), &b),
        len_of(64, 6400) + len_of(7680, 20005)
    );
    assert_eq!(intersection_len(&run(&[(64, 64)]), &b), 1);
    assert_eq!(intersection_len(&b, &run(&[(64, 64)])), 1);
}

#[test]
fn intersection_len_array() {
    let a = Container::Array(array(&[20, 40, 60, 500, 5001]));
    assert_eq!(intersection_len(&run(&[(20, 600)]), &a), 4);
    let values: Vec<u16> = (0..200).collect();
    let a = Container::Array(array(&values));
    assert_eq!(intersection_len(&run(&[(20, 600)]), &a), 200 - 20);
    assert_eq!(intersection_len(&a, &run(&[(20, 600)])), 200 - 20);
}

#[test]
fn len_and_is_empty() {
    let mut s = store(&[(20, 600), (5000, 8000)]);
    assert_eq!(s.len(), len_of(20, 600) + len_of(5000, 8000));
    assert!(!s.is_empty());
    s.remove_range(0..=u16::MAX);
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    assert_eq!(RunStore::full().len(), 1 << 16);
}

#[test]
fn min_max() {
    assert_eq!(store(&[(20, u16::MAX)]).min(), Some(20));
    assert_eq!(store(&[(20, u16::MAX)]).max(), Some(u16::MAX));
    assert_eq!(store(&[]).min(), None);
    assert_eq!(store(&[]).max(), None);
}

#[test]
fn rank() {
    let s = store(&[(0, 200), (5000, 7000), (8000, 10000)]);
    assert_eq!(s.rank(5020), len_of(0, 200) + len_of(5000, 5020));
    assert_eq!(s.rank(u16::MAX), s.len());
    assert_eq!(s.rank(0), 1);
    assert_eq!(s.rank(4999), len_of(0, 200));
    assert_eq!(store(&[(1, 5)]).rank(0), 0);
    assert_eq!(RunStore::full().rank(u16::MAX), 1 << 16);
}

#[test]
fn select() {
    let s = store(&[(0, 0), (2, 11), (5000, 7000), (8000, 10000)]);
    assert_eq!(s.select(0), Some(0));
    assert_eq!(s.select(1), Some(2));
    assert_eq!(s.select(10), Some(11));
    assert_eq!(s.select(11), Some(5000));
    assert_eq!(s.select(11 + 3), Some(5003));
    assert_eq!(s.select(11 + 2001), Some(8000));
    assert_eq!(s.select((s.len() - 1) as u16), Some(10000));
    assert_eq!(s.select(s.len() as u16), None);
    assert_eq!(RunStore::full().select(u16::MAX), Some(u16::MAX));
}

#[test]
fn union_runs() {
    let a = run(&[(0, 0), (2, 11), (5000, 7000), (8000, 10000)]);
    let b = run(&[(0, 0), (2, 10), (12, 7000), (65000, 65050)]);
    let expected = [(0, 0), (2, 7000), (8000, 10000), (65000, 65050)];
    assert_eq!(pairs(as_run(&union(&a, &b))), expected);
    assert_eq!(pairs(as_run(&union(&b, &a))), expected);
}

#[test]
fn union_array() {
    let a = Container::Array(array(&[0, 1, 2, 3, 4, 2000, 5000, u16::MAX]));
    let r = run(&[(0, 0), (2, 11), (5000, 7000), (8000, 10000)]);
    let expected = [
        (0, 11),
        (2000, 2000),
        (5000, 7000),
        (8000, 10000),
        (u16::MAX, u16::MAX),
    ];
    assert_eq!(pairs(as_run(&union(&r, &a))), expected);
    assert_eq!(pairs(as_run(&union(&a, &r))), expected);
}

#[test]
fn intersection_runs() {
    let a = run(&[(0, 0), (2, 11), (5000, 7000), (8000, 10000)]);
    let b = run(&[(0, 0), (5, 50), (4000, 10000)]);
    assert_eq!(
        pairs(as_run(&intersection(&a, &b))),
        [(0, 0), (5, 11), (5000, 7000), (8000, 10000)]
    );
    assert_eq!(pairs(as_run(&intersection(&a, &a))), pairs(as_run(&a)));
}

#[test]
fn difference_runs() {
    let a = run(&[(0, 0), (2, 11), (5000, 7000), (8000, 11000)]);
    let b = run(&[(0, 0), (5, 50), (4000, 10000)]);
    assert_eq!(pairs(as_run(&difference(&a, &b))), [(2, 4), (10001, 11000)]);
}

#[test]
fn symmetric_difference_runs() {
    let a = run(&[(0, 0), (2, 11), (5000, 7000), (8000, 11000), (40000, 50000)]);
    let b = run(&[(0, 0), (5, 50), (4000, 10000)]);
    assert_eq!(
        pairs(as_run(&symmetric_difference(&a, &b))),
        [
            (2, 4),
            (12, 50),
            (4000, 4999),
            (7001, 7999),
            (10001, 11000),
            (40000, 50000)
        ]
    );
    let (a, b) = (run(&[(0, 50)]), run(&[(100, 200)]));
    assert_eq!(
        pairs(as_run(&symmetric_difference(&a, &b))),
        [(0, 50), (100, 200)]
    );
    let a = run(&[(0, 50), (500, 600), (800, 1000)]);
    let b = run(&[(0, 6000)]);
    assert_eq!(
        pairs(as_run(&symmetric_difference(&a, &b))),
        [(51, 499), (601, 799), (1001, 6000)]
    );
}

// ---------------------------------------------------------------------------------------------
// Construction, validation, conversions
// ---------------------------------------------------------------------------------------------

#[test]
#[allow(clippy::reversed_empty_ranges)]
fn construction() {
    assert!(RunStore::new().is_empty());
    assert!(RunStore::default().is_empty());
    assert_eq!(pairs(&RunStore::full()), [(0, u16::MAX)]);
    assert_eq!(pairs(&RunStore::from_range(3..=3)), [(3, 3)]);
    assert_eq!(pairs(&RunStore::from_range(7..=900)), [(7, 900)]);
    assert!(RunStore::from_range(9..=8).is_empty());
    assert_eq!(iv(0, u16::MAX).run_len(), 1 << 16);
    assert_eq!(iv(9, 9).run_len(), 1);
    let s = store(&[(1, 2), (10, 20)]);
    assert_eq!(s.run_amount(), 2);
    assert_eq!(s.byte_size(), serialized_byte_size(2));
    assert_eq!(serialized_byte_size(2), 10);
    assert_eq!(alloc::format!("{s:?}"), "[1..=2, 10..=20]");
    let c = s.clone();
    assert_eq!(pairs(&c), pairs(&s));
}

#[test]
fn try_from_intervals_validation() {
    let ok = |p: &[(u16, u16)]| {
        let ivs: Vec<Interval> = p
            .iter()
            .map(|&(s, e)| Interval { start: s, end: e })
            .collect();
        RunStore::try_from_intervals(&ivs).map(|s| pairs(&s))
    };
    assert_eq!(ok(&[(0, 0)]), Ok(vec![(0, 0)]));
    assert_eq!(ok(&[(0, u16::MAX)]), Ok(vec![(0, u16::MAX)]));
    assert_eq!(
        ok(&[(0, 1), (3, 3), (u16::MAX, u16::MAX)]),
        Ok(vec![(0, 1), (3, 3), (u16::MAX, u16::MAX)])
    );
    assert_eq!(ok(&[]), Err("run container with zero runs"));
    assert_eq!(ok(&[(5, 4)]), Err("run with start greater than end"));
    assert_eq!(
        ok(&[(1, 2), (9, 4)]),
        Err("run with start greater than end")
    );
    assert_eq!(ok(&[(1, 5), (5, 9)]), Err("overlapping or unordered runs"));
    assert_eq!(ok(&[(1, 5), (3, 9)]), Err("overlapping or unordered runs"));
    assert_eq!(
        ok(&[(10, 15), (1, 5)]),
        Err("overlapping or unordered runs")
    );
    assert_eq!(ok(&[(1, 5), (6, 9)]), Err("adjacent runs"));
    assert_eq!(ok(&[(0, 0), (1, 1)]), Err("adjacent runs"));
}

#[test]
fn internal_validate() {
    assert!(store(&[(1, 5), (7, 9)]).internal_validate().is_ok());
    assert!(RunStore::full().internal_validate().is_ok());
    assert_eq!(
        RunStore::new().internal_validate(),
        Err("run container with zero runs")
    );
    let bad = RunStore(HeapVec::from_slice(&[iv(1, 5), iv(6, 9)]));
    assert_eq!(bad.internal_validate(), Err("adjacent runs"));
    let bad = RunStore(HeapVec::from_slice(&[iv(1, 5), iv(5, 9)]));
    assert_eq!(
        bad.internal_validate(),
        Err("overlapping or unordered runs")
    );
}

#[test]
fn mem_size() {
    // Inline handle (8 bytes) + heap block (8-byte header + 4 bytes per interval of capacity).
    let mut s = RunStore::new();
    assert_eq!(s.mem_size(), 8);
    s.insert(5);
    assert!(s.mem_size() >= 8 + 8 + 4);
    let full = RunStore::full();
    assert_eq!(full.mem_size(), 8 + 8 + 4);
}

#[test]
fn full_conversions() {
    let full = RunStore::full();
    let b = full.to_bitmap();
    assert_eq!(b.len(), 1 << 16);
    assert!(b.as_words().iter().all(|&w| w == u64::MAX));
    assert_eq!(pairs(&RunStore::from_bitmap(&b)), [(0, u16::MAX)]);
    assert_eq!(
        pairs(&RunStore::from_bitmap(&BitmapStore::full())),
        [(0, u16::MAX)]
    );
    assert!(RunStore::from_bitmap(&BitmapStore::new()).is_empty());
    assert!(RunStore::from_array(&ArrayStore::new()).is_empty());
    assert!(RunStore::new().to_array().is_empty());
    assert_eq!(RunStore::new().to_bitmap().len(), 0);
    if !cfg!(miri) {
        let a = full.to_array();
        assert_eq!(a.len(), 1 << 16);
        assert!(
            a.as_slice()
                .iter()
                .enumerate()
                .all(|(i, &v)| i == v as usize)
        );
    }
}

#[test]
fn conversions_word_edges() {
    // Runs touching and crossing word boundaries, including the last word.
    let cases: [&[(u16, u16)]; 6] = [
        &[(0, 0), (63, 64), (127, 128), (u16::MAX, u16::MAX)],
        &[(0, 63), (65, 127), (129, 191)],
        &[(1, 62), (64, 64), (66, 1000), (1002, 65533)],
        &[(64, 127), (192, 255)],
        &[(65_472, u16::MAX)],
        &[(0, 65_470), (65_472, 65_533), (u16::MAX, u16::MAX)],
    ];
    for p in cases {
        let s = store(p);
        if cfg!(miri) && s.len() > 10_000 {
            continue;
        }
        let b = s.to_bitmap();
        assert_eq!(b.len(), s.len());
        assert_values(&word_values(b.as_words()), &run_values(&s), "to_bitmap");
        assert_eq!(pairs(&RunStore::from_bitmap(&b)), p);
        if s.len() <= ARRAY_LIMIT {
            let a = s.to_array();
            assert_values(a.as_slice(), &run_values(&s), "to_array");
            assert_eq!(pairs(&RunStore::from_array(&a)), p);
        }
        let mut words = [0u64; BITMAP_WORDS];
        s.or_into_words(&mut words);
        assert_eq!(&words, b.as_words());
        s.xor_into_words(&mut words);
        assert!(words.iter().all(|&w| w == 0));
    }
}

// ---------------------------------------------------------------------------------------------
// Model-based tests of the store methods
// ---------------------------------------------------------------------------------------------

#[track_caller]
fn check_store(r: &RunStore, m: &Model, context: &str) {
    assert_eq!(
        r.intervals(),
        &model_runs(m)[..],
        "{context}: intervals differ"
    );
    assert_eq!(r.len(), m.len() as u64, "{context}: len");
}

#[test]
fn model_point_ops() {
    let mut rng = Rng::new(1);
    let steps = if cfg!(miri) { 16 } else { 200 };
    for case in 0..CASES {
        let mut m = gen_set(&mut rng, 0, MAX_SET.min(1500));
        let mut r = run_of(&m);
        for step in 0..steps {
            let v = if rng.chance(10) {
                m.last()
                    .map_or(0, |&max| max.saturating_add(1 + rng.below(2) as u16))
            } else {
                rng.near(&m)
            };
            let context = alloc::format!("case {case} step {step} value {v}");
            match rng.below(3) {
                0 => assert_eq!(r.insert(v), m.insert(v), "{context}: insert"),
                1 => assert_eq!(r.remove(v), m.remove(&v), "{context}: remove"),
                _ => {
                    let appends = m.last().is_none_or(|&max| v > max);
                    assert_eq!(r.push(v), appends, "{context}: push");
                    if appends {
                        m.insert(v);
                    }
                }
            }
            check_store(&r, &m, &context);
        }
    }
}

#[test]
fn model_range_ops() {
    let mut rng = Rng::new(2);
    let steps = if cfg!(miri) { 8 } else { 40 };
    for case in 0..CASES {
        let mut m = gen_set(&mut rng, 0, MAX_SET.min(4000));
        let mut r = run_of(&m);
        for step in 0..steps {
            let range = rng.range(&m);
            let (s, e) = (*range.start(), *range.end());
            let present = m.range(s..=e).count() as u64;
            let context = alloc::format!("case {case} step {step} range {s}..={e}");
            if rng.chance(50) {
                let added = r.insert_range(range);
                assert_eq!(
                    added,
                    (e - s) as u64 + 1 - present,
                    "{context}: insert_range"
                );
                m.extend(s..=e);
            } else {
                let removed = r.remove_range(range);
                assert_eq!(removed, present, "{context}: remove_range");
                m.retain(|v| !(s..=e).contains(v));
            }
            check_store(&r, &m, &context);
        }
    }
}

#[test]
fn model_remove_smallest_biggest() {
    let mut rng = Rng::new(3);
    for case in 0..CASES * 2 {
        let m = gen_set(&mut rng, 1, MAX_SET.min(20_000));
        let runs = model_runs(&m);
        let len = m.len() as u64;
        // Amounts on interval boundaries (#359), inside intervals and beyond the cardinality.
        let k = rng.below(runs.len() as u32) as usize;
        let prefix: u64 = runs[..k].iter().map(Interval::run_len).sum();
        let suffix: u64 = runs[runs.len() - k..].iter().map(Interval::run_len).sum();
        let amounts = [
            0,
            1,
            prefix,
            suffix,
            rng.below(len as u32 + 1) as u64,
            len - 1,
            len,
            len + 7,
        ];
        for n in amounts {
            let context = alloc::format!("case {case} n {n}");
            let mut r = run_of(&m);
            r.remove_smallest(n);
            let expected: Model = m.iter().copied().skip(n as usize).collect();
            check_store(&r, &expected, &context);

            let mut r = run_of(&m);
            r.remove_biggest(n);
            let keep = len.saturating_sub(n) as usize;
            let expected: Model = m.iter().copied().take(keep).collect();
            check_store(&r, &expected, &context);
        }
    }
}

#[test]
fn model_queries() {
    let mut rng = Rng::new(4);
    for case in 0..CASES {
        let m = gen_set(&mut rng, 0, MAX_SET);
        let r = run_of(&m);
        let model = vec_of(&m);
        let runs = model_runs(&m);
        assert_eq!(r.len(), m.len() as u64);
        assert_eq!(r.is_empty(), m.is_empty());
        assert_eq!(r.min(), m.first().copied());
        assert_eq!(r.max(), m.last().copied());
        assert_eq!(r.run_amount(), runs.len() as u64);
        assert_eq!(r.byte_size(), 2 + 4 * runs.len());

        let queries = if cfg!(miri) { 10 } else { 100 };
        for _ in 0..queries {
            let q = rng.near(&m);
            let context = alloc::format!("case {case} query {q}");
            assert_eq!(r.contains(q), m.contains(&q), "{context}: contains");
            let rank = m.range(..=q).count() as u64;
            assert_eq!(r.rank(q), rank, "{context}: rank");

            let n = match rng.below(4) {
                0 => rng.below(model.len() as u32 + 2),
                1 => model.len().saturating_sub(1) as u32,
                _ => rank.min(u16::MAX as u64) as u32,
            }
            .min(u16::MAX as u32) as u16;
            assert_eq!(
                r.select(n),
                model.get(n as usize).copied(),
                "{context}: select {n}"
            );

            let range = if !runs.is_empty() && rng.chance(30) {
                // A sub-range of a run, so that `contains_range` holds.
                let iv = runs[rng.below(runs.len() as u32) as usize];
                let a = iv.start + rng.below(iv.run_len() as u32) as u16;
                let b = a + rng.below((iv.end - a) as u32 + 1) as u16;
                a..=b
            } else {
                rng.range(&m)
            };
            let (s, e) = (*range.start(), *range.end());
            let inside = m.range(s..=e).count() as u64;
            let context = alloc::format!("case {case} range {s}..={e}");
            assert_eq!(
                r.range_cardinality(range.clone()),
                inside,
                "{context}: range_cardinality"
            );
            assert_eq!(
                r.contains_range(range),
                inside == (e - s) as u64 + 1,
                "{context}: contains_range"
            );
        }
    }
}

#[test]
fn model_conversions() {
    let mut rng = Rng::new(5);
    for case in 0..CASES {
        let m = gen_set(&mut rng, 0, MAX_SET);
        let model = vec_of(&m);
        let runs = model_runs(&m);
        let r = run_of(&m);
        let context = alloc::format!("case {case}");

        // run -> array / bitmap
        if m.len() as u64 <= ARRAY_LIMIT * 2 {
            let a = r.to_array();
            assert_values(a.as_slice(), &model, &context);
        }
        let b = r.to_bitmap();
        assert_eq!(b.len(), m.len() as u64, "{context}: to_bitmap len");
        assert_values(&word_values(b.as_words()), &model, &context);

        // array / bitmap -> run
        if m.len() as u64 <= ARRAY_LIMIT {
            let from_array = RunStore::from_array(&array_of(&m));
            assert_eq!(from_array.intervals(), &runs[..], "{context}: from_array");
            assert_eq!(
                from_array.0.capacity(),
                runs.len(),
                "{context}: exact allocation"
            );
        }
        let from_bitmap = RunStore::from_bitmap(&bitmap_of(&m));
        assert_eq!(from_bitmap.intervals(), &runs[..], "{context}: from_bitmap");
        assert_eq!(
            from_bitmap.0.capacity(),
            runs.len(),
            "{context}: exact allocation"
        );

        // or / xor into arbitrary words
        let base = gen_set(&mut rng, 0, MAX_SET);
        let mut words = *bitmap_of(&base).as_words();
        r.or_into_words(&mut words);
        assert_values(
            &word_values(&words),
            &vec_of(&base.union(&m).copied().collect()),
            &context,
        );
        let mut words = *bitmap_of(&base).as_words();
        r.xor_into_words(&mut words);
        let xor: Model = base.symmetric_difference(&m).copied().collect();
        assert_values(&word_values(&words), &vec_of(&xor), &context);
    }
}

// ---------------------------------------------------------------------------------------------
// Model-based tests of the container operations
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Union,
    Intersection,
    Difference,
    SymmetricDifference,
}

const OPS: [Op; 4] = [
    Op::Union,
    Op::Intersection,
    Op::Difference,
    Op::SymmetricDifference,
];

const COMBOS: [(Kind, Kind); 5] = [
    (Kind::Run, Kind::Run),
    (Kind::Run, Kind::Array),
    (Kind::Array, Kind::Run),
    (Kind::Run, Kind::Bitmap),
    (Kind::Bitmap, Kind::Run),
];

fn apply(op: Op, a: &Container, b: &Container) -> Container {
    match op {
        Op::Union => union(a, b),
        Op::Intersection => intersection(a, b),
        Op::Difference => difference(a, b),
        Op::SymmetricDifference => symmetric_difference(a, b),
    }
}

fn apply_in_place(op: Op, a: &mut Container, b: &Container) {
    match op {
        Op::Union => union_with(a, b),
        Op::Intersection => intersect_with(a, b),
        Op::Difference => difference_with(a, b),
        Op::SymmetricDifference => symmetric_difference_with(a, b),
    }
}

fn model_op(op: Op, a: &Model, b: &Model) -> Vec<u16> {
    match op {
        Op::Union => a.union(b).copied().collect(),
        Op::Intersection => a.intersection(b).copied().collect(),
        Op::Difference => a.difference(b).copied().collect(),
        Op::SymmetricDifference => a.symmetric_difference(b).copied().collect(),
    }
}

/// Result representation of the `roaring` crate (store/mod.rs followed by
/// `Container::ensure_correct_store`).
fn expected_kind(op: Op, a: Kind, b: Kind, card: usize) -> Kind {
    let by_card = if card as u64 <= ARRAY_LIMIT {
        Kind::Array
    } else {
        Kind::Bitmap
    };
    match (op, a, b) {
        (_, Kind::Run, Kind::Run) => Kind::Run,
        (Op::Union, Kind::Run, Kind::Array) | (Op::Union, Kind::Array, Kind::Run) => Kind::Run,
        (Op::Union, _, _) => Kind::Bitmap,
        (Op::Intersection, Kind::Run, Kind::Array) | (Op::Intersection, Kind::Array, Kind::Run) => {
            Kind::Array
        }
        (Op::Difference, Kind::Run, Kind::Array) => Kind::Run,
        (Op::Difference, Kind::Array, Kind::Run) => Kind::Array,
        (Op::SymmetricDifference, Kind::Run, Kind::Array)
        | (Op::SymmetricDifference, Kind::Array, Kind::Run) => Kind::Run,
        _ => by_card,
    }
}

/// Checks every operation of the pair against the model.
fn check_pair(am: &Model, ak: Kind, bm: &Model, bk: Kind, context: &str) {
    let a = make(am, ak);
    let b = make(bm, bk);
    for op in OPS {
        let context = alloc::format!("{context} {op:?} {ak:?} {bk:?}");
        let expected = model_op(op, am, bm);

        let got = apply(op, &a, &b);
        check_container(&got);
        assert_eq!(
            got.kind(),
            expected_kind(op, ak, bk, expected.len()),
            "{context}"
        );
        assert_values(&values(&got), &expected, &context);

        let mut in_place = a.clone();
        apply_in_place(op, &mut in_place, &b);
        check_container(&in_place);
        assert_eq!(in_place.kind(), got.kind(), "{context} (in place)");
        assert_values(
            &values(&in_place),
            &expected,
            &alloc::format!("{context} (in place)"),
        );
    }
    let context = alloc::format!("{context} {ak:?} {bk:?}");
    assert_eq!(
        intersection_len(&a, &b),
        am.intersection(bm).count() as u64,
        "{context}: intersection_len"
    );
    assert_eq!(
        is_disjoint(&a, &b),
        am.is_disjoint(bm),
        "{context}: is_disjoint"
    );
    assert_eq!(
        is_subset(&a, &b),
        am.is_subset(bm),
        "{context}: is_subset(a, b)"
    );
    assert_eq!(
        is_subset(&b, &a),
        bm.is_subset(am),
        "{context}: is_subset(b, a)"
    );
    assert_eq!(eq_values(&a, &b), am == bm, "{context}: eq_values(a, b)");
    assert_eq!(eq_values(&b, &a), am == bm, "{context}: eq_values(b, a)");
}

fn model_ops_for(ak: Kind, bk: Kind, seed: u64) {
    let mut rng = Rng::new(seed);
    for case in 0..CASES {
        let (min, max) = bounds(ak);
        let a = gen_set(&mut rng, min, max);
        let b = derive(&mut rng, &a, bk);
        check_pair(&a, ak, &b, bk, &alloc::format!("case {case}"));
    }
}

#[test]
fn model_ops_run_run() {
    model_ops_for(Kind::Run, Kind::Run, 10);
}

#[test]
fn model_ops_run_array() {
    model_ops_for(Kind::Run, Kind::Array, 11);
}

#[test]
fn model_ops_array_run() {
    model_ops_for(Kind::Array, Kind::Run, 12);
}

#[test]
fn model_ops_run_bitmap() {
    model_ops_for(Kind::Run, Kind::Bitmap, 13);
}

#[test]
fn model_ops_bitmap_run() {
    model_ops_for(Kind::Bitmap, Kind::Run, 14);
}

#[test]
fn model_ops_special_sets() {
    let sets = special_sets();
    for (i, a) in sets.iter().enumerate() {
        for (j, b) in sets.iter().enumerate() {
            for (ak, bk) in COMBOS {
                if fits(a, ak) && fits(b, bk) {
                    check_pair(a, ak, b, bk, &alloc::format!("special {i} {j}"));
                }
            }
        }
    }
}

#[test]
fn ops_with_empty_run() {
    let empty = Container::Run(RunStore::new());
    let a = Container::Array(array(&[1, 2, 3, 9]));
    let b = Container::Bitmap(bitmap(0..=5000));
    for other in [&a, &b, &empty] {
        assert_values(&values(&union(&empty, other)), &values(other), "union");
        assert_values(&values(&union(other, &empty)), &values(other), "union");
        assert!(values(&intersection(&empty, other)).is_empty());
        assert!(values(&intersection(other, &empty)).is_empty());
        assert!(values(&difference(&empty, other)).is_empty());
        assert_values(
            &values(&difference(other, &empty)),
            &values(other),
            "difference",
        );
        assert_values(
            &values(&symmetric_difference(other, &empty)),
            &values(other),
            "symmetric_difference",
        );
        assert_eq!(intersection_len(&empty, other), 0);
        assert!(is_disjoint(&empty, other));
        assert!(is_subset(&empty, other));
        assert_eq!(is_subset(other, &empty), values(other).is_empty());
        assert_eq!(eq_values(&empty, other), values(other).is_empty());
    }
}

#[test]
fn subset_and_equality_with_gaps() {
    // Same end points and cardinality, but a gap inside the array span.
    let r = run(&[(5, 7)]);
    let gap = Container::Array(array(&[5, 6, 8]));
    assert!(!is_subset(&r, &gap));
    assert!(!eq_values(&r, &gap));
    assert!(!eq_values(&gap, &r));
    let wide = Container::Array(array(&[4, 5, 6, 7, 9]));
    assert!(is_subset(&r, &wide));
    assert!(!is_subset(&wide, &r));
    let exact = Container::Array(array(&[5, 6, 7]));
    assert!(is_subset(&r, &exact) && is_subset(&exact, &r));
    assert!(eq_values(&r, &exact) && eq_values(&exact, &r));
    let r = run(&[(0, 2), (10, 12), (u16::MAX - 1, u16::MAX)]);
    let a = Container::Array(array(&[0, 1, 2, 10, 11, 13, u16::MAX - 1, u16::MAX]));
    assert!(!is_subset(&r, &a));
    let a = Container::Array(array(&[0, 1, 2, 10, 11, 12, u16::MAX - 1, u16::MAX]));
    assert!(is_subset(&r, &a) && eq_values(&r, &a));
    let a = Container::Array(array(&[0, 1, 2, 10, 11, 12, u16::MAX - 1]));
    assert!(!is_subset(&r, &a) && is_subset(&a, &r) && !eq_values(&r, &a));
}

#[test]
fn bitmap_queries_word_edges() {
    // Every edge position of runs with various word alignments, probed one bit at a time.
    // (Miri checks the first run with a few probes, inside a smaller value space.)
    let runs: [(u16, u16); 7] = [
        (60, 200),
        (0, 0),
        (5, 60),
        (63, 64),
        (64, 191),
        (1000, 5000),
        (65_400, u16::MAX),
    ];
    for (s, e) in runs {
        let r = run(&[(s, e)]);
        let limit = if cfg!(miri) { 5_200 } else { u16::MAX };
        let outside: Vec<u16> = (0..=limit).filter(|v| !(s..=e).contains(v)).collect();
        let mut probes = vec![s, e, s.saturating_add(1), e.saturating_sub(1)];
        probes.extend(
            ((s / 64 * 64)..=e)
                .step_by(64)
                .flat_map(|w| [w, w.saturating_add(63)]),
        );
        probes.retain(|p| (s..=e).contains(p));
        if cfg!(miri) {
            // First / last word edges of the run.
            probes.truncate(2);
        }
        for p in probes {
            let context = alloc::format!("run {s}..={e} probe {p}");
            // The complement of the run plus one probe bit inside it.
            let b = Container::Bitmap(bitmap(outside.iter().copied().chain([p])));
            assert!(!is_disjoint(&r, &b), "{context}: is_disjoint");
            assert!(!is_disjoint(&b, &r), "{context}: is_disjoint");
            assert_eq!(intersection_len(&r, &b), 1, "{context}: intersection_len");
            assert_values(&values(&intersection(&r, &b)), &[p], &context);
            // The run without the probe bit (plus far away values to stay a bitmap).
            let holed: Vec<u16> = (s..=e).filter(|&v| v != p).collect();
            let near = s.saturating_sub(300)..=e.saturating_add(300);
            let far = (0..=u16::MAX).filter(|v| !near.contains(v));
            let far = far.take(if cfg!(miri) { 4_200 } else { 5_000 });
            let b = Container::Bitmap(bitmap(holed.iter().copied().chain(far)));
            assert!(!is_subset(&r, &b), "{context}: is_subset");
            assert!(!eq_values(&r, &b), "{context}: eq_values");
            assert_eq!(
                intersection_len(&r, &b),
                (e - s) as u64,
                "{context}: intersection_len"
            );
            assert_values(&values(&difference(&r, &b)), &[p], &context);
        }
        let b = Container::Bitmap(bitmap(outside.iter().copied()));
        if outside.len() as u64 > ARRAY_LIMIT {
            assert!(is_disjoint(&r, &b) && is_disjoint(&b, &r), "run {s}..={e}");
            assert_eq!(intersection_len(&r, &b), 0);
        }
        if cfg!(miri) {
            break;
        }
    }
}

#[test]
#[should_panic(expected = "without a run operand")]
fn ops_require_a_run_operand() {
    let a = Container::Array(array(&[1]));
    let _ = union(&a, &a);
}
