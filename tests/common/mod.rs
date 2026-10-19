//! Helpers shared by the integration test binaries.
//!
//! * proptest configuration (`config`): default case counts chosen per property, overridable
//!   with the `PROPTEST_CASES` environment variable,
//! * `SetSpec`: a plain-data description of a structured `u128` set (clusters, dense blocks,
//!   intervals, strided values, sparse UUID-like values, array/bitmap boundary blocks) placed
//!   around interesting anchors (0, 2^32, 2^64, 2^100, 2^112, `u128::MAX`), built into a
//!   `RoaringBitmap128` *and* a `BTreeSet<u128>` model,
//! * `RangeSpec`: arbitrary `RangeBounds<u128>` with the model interpretation,
//! * `ModelIter` / `IterOp`: a model of the bitmap iterators (front/back cursors),
//! * assertions comparing a bitmap with its model.
//!
//! Strategies only produce plain data (`Debug` output stays readable and never calls into the
//! bitmap under test); the bitmaps are built inside the test bodies.

#![allow(dead_code, unused_macros)]

use std::collections::BTreeSet;
use std::ops::{Bound, RangeBounds, RangeInclusive};

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::Index;
use roaring128::RoaringBitmap128;

pub type Model = BTreeSet<u128>;

/// Number of values per container (`1 << 16`).
pub const KEY: u128 = 1 << 16;
/// Largest cardinality of an array container.
pub const ARRAY_LIMIT: u64 = 4096;
/// Largest container key.
pub const MAX_KEY: u128 = u128::MAX >> 16;

// -------------------------------------------------------------------------------------------------
// proptest configuration
// -------------------------------------------------------------------------------------------------

/// proptest configuration with `default_cases` cases (at most 256), overridable with the
/// `PROPTEST_CASES` environment variable. Failure persistence is disabled so that runs do not
/// create `proptest-regressions` files; failing inputs are printed by proptest.
///
/// Shrinking is bounded (2048 iterations / 20 s, overridable with `PROPTEST_MAX_SHRINK_ITERS`
/// and `PROPTEST_MAX_SHRINK_TIME`): shrinking collections of 100k values one element at a time
/// would otherwise take minutes.
pub fn config(default_cases: u32) -> ProptestConfig {
    let base = ProptestConfig::default();
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(default_cases.min(256));
    let env_set = |name: &str| std::env::var_os(name).is_some();
    ProptestConfig {
        cases,
        failure_persistence: None,
        max_shrink_iters: if env_set("PROPTEST_MAX_SHRINK_ITERS") {
            base.max_shrink_iters
        } else {
            2048
        },
        max_shrink_time: if env_set("PROPTEST_MAX_SHRINK_TIME") {
            base.max_shrink_time
        } else {
            20_000
        },
        ..base
    }
}

// -------------------------------------------------------------------------------------------------
// Offsets used to derive 128-bit variants of the ported roaring tests
// -------------------------------------------------------------------------------------------------

/// The original (`u32`/`u64`) placement.
pub const OFF_ORIG: u128 = 0;
/// Unaligned, values straddle the key boundary at 2^32 (the treemap bucket boundary).
pub const OFF_STRADDLE_2P32: u128 = (1 << 32) - 1500;
/// Unaligned, values straddle `u64::MAX` / 2^64.
pub const OFF_STRADDLE_2P64: u128 = (1 << 64) - 1500;
/// Key aligned at 2^64 (same container layout as the original test).
pub const OFF_2P64: u128 = 1 << 64;
/// Unaligned offset above 2^100.
pub const OFF_2P100_UNALIGNED: u128 = (1 << 100) + 0x1234;
/// Unaligned, values straddle 2^112 (the boundary of the key bits).
pub const OFF_STRADDLE_2P112: u128 = (1 << 112) - 1500;
/// Key aligned, `2^128 - 2^24`: there is room for 16 Mi values, up to `u128::MAX`.
pub const OFF_NEAR_MAX: u128 = u128::MAX - 0xFF_FFFF;

/// All the offsets above, with a short name.
pub const OFFSETS: &[(&str, u128)] = &[
    ("orig", OFF_ORIG),
    ("straddle_2p32", OFF_STRADDLE_2P32),
    ("straddle_2p64", OFF_STRADDLE_2P64),
    ("at_2p64", OFF_2P64),
    ("unaligned_2p100", OFF_2P100_UNALIGNED),
    ("straddle_2p112", OFF_STRADDLE_2P112),
    ("near_max", OFF_NEAR_MAX),
];

/// Key aligned, `2^128 - 2^42`: room for values up to 2^42 above the offset.
pub const OFF_NEAR_MAX_WIDE: u128 = u128::MAX - ((1 << 42) - 1);

/// `variants!({ name = value, ... }; f, g, ...)` generates, for each function `f(x: u128)`, a
/// module `f` with one `#[test] fn name() { f(value) }` per listed variant.
macro_rules! variants {
    ($vars:tt; $($f:ident),* $(,)?) => {
        $( variants_one!($f $vars); )*
    };
}

macro_rules! variants_one {
    ($f:ident { $($vname:ident = $val:expr),* $(,)? }) => {
        mod $f {
            $(
                #[test]
                fn $vname() {
                    super::$f($val)
                }
            )*
        }
    };
}

/// For each listed function `f(offset: u128)`, generates a module `f` with one `#[test]` per
/// offset: `f::orig` is the faithful port of the roaring test, the others are 128-bit variants.
macro_rules! offset_variants {
    ($($f:ident),* $(,)?) => {
        variants!({
            orig = crate::common::OFF_ORIG,
            straddle_2p32 = crate::common::OFF_STRADDLE_2P32,
            straddle_2p64 = crate::common::OFF_STRADDLE_2P64,
            at_2p64 = crate::common::OFF_2P64,
            unaligned_2p100 = crate::common::OFF_2P100_UNALIGNED,
            straddle_2p112 = crate::common::OFF_STRADDLE_2P112,
            near_max = crate::common::OFF_NEAR_MAX,
        }; $($f),*);
    };
}

/// Like `offset_variants!` for tests that need up to 2^42 values of room above the offset.
macro_rules! wide_offset_variants {
    ($($f:ident),* $(,)?) => {
        variants!({
            orig = crate::common::OFF_ORIG,
            at_2p64 = crate::common::OFF_2P64,
            unaligned_2p100 = crate::common::OFF_2P100_UNALIGNED,
            straddle_2p112 = crate::common::OFF_STRADDLE_2P112,
            near_max = crate::common::OFF_NEAR_MAX_WIDE,
        }; $($f),*);
    };
}

/// Builds a bitmap from `u64` values shifted by `off`.
pub fn shifted<I: IntoIterator<Item = u64>>(off: u128, values: I) -> RoaringBitmap128 {
    values.into_iter().map(|v| off + v as u128).collect()
}

/// `off + v` for every value.
pub fn shift_vec<I: IntoIterator<Item = u64>>(off: u128, values: I) -> Vec<u128> {
    values.into_iter().map(|v| off + v as u128).collect()
}

// -------------------------------------------------------------------------------------------------
// Assertions
// -------------------------------------------------------------------------------------------------

#[track_caller]
pub fn assert_valid(rb: &RoaringBitmap128) {
    if let Err(e) = rb.internal_validate() {
        panic!("internal_validate failed: {e}");
    }
}

/// Compares two sequences element by element and reports the first difference (values in hex).
#[track_caller]
pub fn assert_same_seq<A, E>(actual: A, expected: E, what: &str)
where
    A: IntoIterator<Item = u128>,
    E: IntoIterator<Item = u128>,
{
    let mut a = actual.into_iter();
    let mut e = expected.into_iter();
    let mut i = 0usize;
    loop {
        match (a.next(), e.next()) {
            (None, None) => return,
            (x, y) if x == y => i += 1,
            (x, y) => panic!(
                "{what}: first difference at position {i}: got {x:x?}, expected {y:x?} (hex)"
            ),
        }
    }
}

/// Full `O(n)` comparison of a bitmap with its model, including `internal_validate`.
#[track_caller]
pub fn assert_matches(rb: &RoaringBitmap128, model: &Model) {
    assert_valid(rb);
    assert_eq!(rb.len(), model.len() as u64, "len");
    assert_eq!(rb.is_empty(), model.is_empty(), "is_empty");
    assert_eq!(rb.min(), model.first().copied(), "min");
    assert_eq!(rb.max(), model.last().copied(), "max");
    let it = rb.iter();
    assert_eq!(
        it.size_hint(),
        (model.len(), Some(model.len())),
        "size_hint"
    );
    assert_same_seq(it, model.iter().copied(), "iter");
}

/// Values worth probing for `contains`/`rank`/... on a model: up to `samples` evenly spaced
/// elements, their neighbours and the boundaries of their containers, plus fixed specials.
pub fn probes(model: &Model, samples: usize) -> Vec<u128> {
    let sorted: Vec<u128> = model.iter().copied().collect();
    let mut out: Vec<u128> = SPECIAL_VALUES.to_vec();
    if !sorted.is_empty() {
        let step = (sorted.len() / samples.max(1)).max(1);
        for &v in sorted.iter().step_by(step).chain(sorted.last()) {
            out.push(v);
            out.push(v.wrapping_sub(1));
            out.push(v.wrapping_add(1));
            out.push(v & !(KEY - 1));
            out.push(v | (KEY - 1));
            out.push((v & !(KEY - 1)).wrapping_sub(1));
        }
    }
    out
}

/// Number of model values `<= v`, computed with a binary search on the sorted values.
pub fn rank_in(sorted: &[u128], v: u128) -> u64 {
    sorted.partition_point(|&x| x <= v) as u64
}

pub fn sorted(model: &Model) -> Vec<u128> {
    model.iter().copied().collect()
}

/// Number of distinct container keys of the model.
pub fn n_keys(model: &Model) -> u64 {
    let mut n = 0;
    let mut last = None;
    for &v in model {
        let k = v >> 16;
        if last != Some(k) {
            n += 1;
            last = Some(k);
        }
    }
    n
}

/// Per container: `(key, cardinality, number of runs)`.
pub fn container_shapes(model: &Model) -> Vec<(u128, u64, u64)> {
    let mut out: Vec<(u128, u64, u64)> = Vec::new();
    let mut prev: Option<u128> = None;
    for &v in model {
        let k = v >> 16;
        match out.last_mut() {
            Some((key, card, runs)) if *key == k => {
                *card += 1;
                if prev != Some(v - 1) {
                    *runs += 1;
                }
            }
            _ => out.push((k, 1, 1)),
        }
        prev = Some(v);
    }
    out
}

/// Expected container kinds `(n_array, n_bitset, n_run)`.
///
/// * `runs == false`: the state after `remove_run_compression()` (array iff cardinality <= 4096).
/// * `runs == true`: the state after `remove_run_compression()` then `optimize()`: a container
///   becomes a run iff that is strictly smaller with the `roaring` size rules
///   (`2 + 4 * runs` bytes versus `2 * cardinality` for arrays and 8192 for bitmaps).
pub fn expected_kinds(model: &Model, runs: bool) -> (u64, u64, u64) {
    let (mut a, mut b, mut r) = (0, 0, 0);
    for (_, card, n_runs) in container_shapes(model) {
        let run_size = 2 + 4 * n_runs;
        if card <= ARRAY_LIMIT {
            if runs && run_size < 2 * card {
                r += 1
            } else {
                a += 1
            }
        } else if runs && run_size < 8192 {
            r += 1
        } else {
            b += 1
        }
    }
    (a, b, r)
}

/// The values whose `n`-th bit (LSB 0) is set in `bytes`, shifted by `offset`.
pub fn lsb0_model(offset: u128, bytes: &[u8]) -> Model {
    let mut m = Model::new();
    for (i, &byte) in bytes.iter().enumerate() {
        for bit in 0..8 {
            if byte & (1 << bit) != 0 {
                m.insert(offset + (i as u128) * 8 + bit as u128);
            }
        }
    }
    m
}

// -------------------------------------------------------------------------------------------------
// Value and range strategies
// -------------------------------------------------------------------------------------------------

/// Special values probed by many tests.
pub const SPECIAL_VALUES: &[u128] = &[
    0,
    1,
    0xFFFF,
    0x1_0000,
    0x1_0001,
    u32::MAX as u128,
    1 << 32,
    u64::MAX as u128 - 1,
    u64::MAX as u128,
    1 << 64,
    (1 << 64) + 1,
    1 << 100,
    (1 << 112) - 1,
    1 << 112,
    u128::MAX - 0x1_0000,
    u128::MAX - 0xFFFF,
    u128::MAX - 1,
    u128::MAX,
];

/// Key-aligned anchors: each set is built in a window of 4 keys after an anchor. The windows of
/// the fixed anchors straddle 2^32, 2^64 and 2^112, and end at `u128::MAX`.
pub fn anchor() -> impl Strategy<Value = u128> + Clone {
    prop_oneof![
        3 => Just(0u128),
        1 => Just((1u128 << 32) - 2 * KEY),
        2 => Just((1u128 << 64) - 2 * KEY),
        1 => Just(1u128 << 100),
        2 => Just((1u128 << 112) - 2 * KEY),
        2 => Just(u128::MAX - 4 * KEY + 1),
        1 => any::<u128>().prop_map(|v| v & !(KEY - 1)),
    ]
}

/// A value near an anchor (most of the time), a special value or a random one.
pub fn value() -> impl Strategy<Value = u128> + Clone {
    prop_oneof![
        6 => (anchor(), 0..4 * KEY).prop_map(|(a, o)| a.saturating_add(o)),
        1 => any::<u128>(),
        1 => prop::sample::select(SPECIAL_VALUES),
    ]
}

/// A `u64` flavoured value (for differential tests against `RoaringTreemap`).
pub fn value_u64() -> impl Strategy<Value = u64> + Clone {
    prop_oneof![
        6 => (anchor_u64(), 0..4 * KEY as u64).prop_map(|(a, o)| a.saturating_add(o)),
        1 => any::<u64>(),
        1 => prop::sample::select(vec![0u64, 1, 0xFFFF, 0x1_0000, u32::MAX as u64, 1 << 32, u64::MAX - 1, u64::MAX]),
    ]
}

pub fn anchor_u64() -> impl Strategy<Value = u64> + Clone {
    prop_oneof![
        3 => Just(0u64),
        2 => Just((1u64 << 32) - 2 * KEY as u64),
        1 => Just(1u64 << 48),
        2 => Just(u64::MAX - 4 * KEY as u64 + 1),
        1 => any::<u64>().prop_map(|v| v & !(KEY as u64 - 1)),
    ]
}

/// A range given by its two bounds (implements `RangeBounds<u128>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeSpec(pub Bound<u128>, pub Bound<u128>);

impl RangeBounds<u128> for RangeSpec {
    fn start_bound(&self) -> Bound<&u128> {
        self.0.as_ref()
    }

    fn end_bound(&self) -> Bound<&u128> {
        self.1.as_ref()
    }
}

impl RangeSpec {
    /// The non-empty inclusive interval described by the bounds, if any.
    pub fn bounds(&self) -> Option<(u128, u128)> {
        let lo = match self.0 {
            Bound::Included(s) => s,
            Bound::Excluded(s) => s.checked_add(1)?,
            Bound::Unbounded => 0,
        };
        let hi = match self.1 {
            Bound::Included(e) => e,
            Bound::Excluded(e) => e.checked_sub(1)?,
            Bound::Unbounded => u128::MAX,
        };
        (lo <= hi).then_some((lo, hi))
    }

    /// Whether `range()` / `into_range()` must panic: the start is greater than the end, or both
    /// bounds are equal and excluded (same rules as `BTreeMap::range` and `roaring`).
    pub fn panics_as_iter_range(&self) -> bool {
        match (self.0, self.1) {
            (Bound::Excluded(s), Bound::Excluded(e)) if s == e => true,
            (Bound::Included(s) | Bound::Excluded(s), Bound::Included(e) | Bound::Excluded(e)) => {
                s > e
            }
            _ => false,
        }
    }

    /// Number of integers in the range, `None` for the full `u128` range (2^128).
    pub fn width(&self) -> Option<u128> {
        match self.bounds() {
            None => Some(0),
            Some((lo, hi)) => (hi - lo).checked_add(1),
        }
    }

    /// The model values inside the range.
    pub fn model_values<'a>(&self, model: &'a Model) -> impl Iterator<Item = u128> + 'a {
        let b = self.bounds();
        b.into_iter()
            .flat_map(move |(lo, hi)| model.range(lo..=hi).copied())
    }

    pub fn model_count(&self, model: &Model) -> u64 {
        self.model_values(model).count() as u64
    }
}

fn range_len() -> impl Strategy<Value = u128> + Clone {
    prop_oneof![
        2 => 0..=2u128,
        3 => 3..=100u128,
        2 => 100..=70_000u128,
        1 => 70_000..=200_000u128,
    ]
}

fn bound_of(kind: u8, v: u128) -> Bound<u128> {
    match kind % 3 {
        0 => Bound::Included(v),
        1 => Bound::Excluded(v),
        _ => Bound::Unbounded,
    }
}

/// Ranges that are safe to insert: at most ~200k values. Includes empty, reversed and equal
/// bounds, and unbounded ends near 0 / `u128::MAX`.
pub fn insertable_range() -> impl Strategy<Value = RangeSpec> + Clone {
    prop_oneof![
        8 => (value(), range_len(), 0..2u8, 0..2u8, prop::bool::weighted(0.1)).prop_map(
            |(s, len, ks, ke, rev)| {
                let e = s.saturating_add(len);
                let (s, e) = if rev { (e, s) } else { (s, e) };
                RangeSpec(bound_of(ks, s), bound_of(ke, e))
            }
        ),
        1 => (0..200_000u128, 0..2u8).prop_map(|(e, ke)| RangeSpec(Bound::Unbounded, bound_of(ke, e))),
        1 => (0..200_000u128, 0..2u8)
            .prop_map(|(d, ks)| RangeSpec(bound_of(ks, u128::MAX - d), Bound::Unbounded)),
    ]
}

/// Any range, including huge and unbounded ones (for operations that do not allocate per
/// value: `remove_range`, `contains_range`, `range_cardinality`, `range`).
pub fn any_range() -> impl Strategy<Value = RangeSpec> + Clone {
    prop_oneof![
        6 => insertable_range(),
        1 => (value(), value(), 0..3u8, 0..3u8).prop_map(|(a, b, ks, ke)| {
            let (s, e) = if a <= b { (a, b) } else { (b, a) };
            RangeSpec(bound_of(ks, s), bound_of(ke, e))
        }),
        1 => (value(), 0..3u8, 0..3u8).prop_map(|(v, ks, ke)| RangeSpec(bound_of(ks, v), bound_of(ke, v))),
        1 => Just(RangeSpec(Bound::Unbounded, Bound::Unbounded)),
    ]
}

// -------------------------------------------------------------------------------------------------
// Structured sets
// -------------------------------------------------------------------------------------------------

/// One building block of a set. Offsets are relative to a key-aligned anchor and saturate at
/// `u128::MAX`.
#[derive(Debug, Clone)]
pub enum Piece {
    /// A few scattered values in a window of 4 keys (array containers).
    Points { anchor: u128, offsets: Vec<u32> },
    /// Random bits in one key: `words[i]` bit `b` is the index `64 * i + b` (array or bitmap
    /// containers, depending on the number of words).
    Dense {
        anchor: u128,
        key: u8,
        words: Vec<u64>,
    },
    /// A contiguous interval (run containers; long ones span several keys).
    Interval { anchor: u128, start: u32, len: u32 },
    /// `count` values with a constant stride (not run friendly; bitmaps when dense).
    Strided {
        anchor: u128,
        start: u32,
        stride: u8,
        count: u16,
    },
    /// Random values, virtually one per key (the UUID case).
    Sparse { values: Vec<u128> },
    /// `n` even indices of one key, `n` around the array limit of 4096.
    Boundary { anchor: u128, key: u8, n: u16 },
}

fn key_base(anchor: u128, key: u8) -> u128 {
    ((anchor >> 16).saturating_add(key as u128)).min(MAX_KEY) << 16
}

impl Piece {
    /// The values of this piece (unsorted, may contain duplicates).
    pub fn values(&self) -> Vec<u128> {
        match self {
            Piece::Points { anchor, offsets } => offsets
                .iter()
                .map(|&o| anchor.saturating_add(o as u128))
                .collect(),
            Piece::Dense { anchor, key, words } => {
                let base = key_base(*anchor, *key);
                let mut out = Vec::new();
                for (i, &w) in words.iter().enumerate() {
                    for b in 0..64 {
                        if w & (1 << b) != 0 {
                            out.push(base + 64 * i as u128 + b);
                        }
                    }
                }
                out
            }
            Piece::Interval { anchor, start, len } => {
                let lo = anchor.saturating_add(*start as u128);
                let hi = lo.saturating_add(*len as u128 - 1);
                (lo..=hi).collect()
            }
            Piece::Strided {
                anchor,
                start,
                stride,
                count,
            } => {
                let lo = anchor.saturating_add(*start as u128);
                (0..*count as u128)
                    .map_while(|k| lo.checked_add(k * *stride as u128))
                    .collect()
            }
            Piece::Sparse { values } => values.clone(),
            Piece::Boundary { anchor, key, n } => {
                let base = key_base(*anchor, *key);
                (0..*n as u128).map(|i| base + 2 * i).collect()
            }
        }
    }
}

/// How a piece is added to the bitmap (each mode exercises other code paths and container
/// representations).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// `insert` one value at a time.
    Insert,
    /// `extend` with all values.
    Extend,
    /// `insert_range` of every maximal interval (creates run containers).
    Ranges,
    /// `append` (sorted values) when they are all above the current maximum, else `extend`.
    Append,
    /// `try_push` in sorted order, falling back to `insert` on `IntegerTooSmall`.
    Push,
}

/// What to do once all pieces are added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    Nothing,
    Optimize,
    RemoveRunCompression,
    OptimizeThenRemoveRunCompression,
}

/// A structured set: built with [`SetSpec::build`].
#[derive(Debug, Clone)]
pub struct SetSpec {
    pub pieces: Vec<(Piece, Build)>,
    pub finish: Finish,
}

/// Sorted, deduplicated values split into maximal intervals.
pub fn intervals(sorted: &[u128]) -> Vec<RangeInclusive<u128>> {
    let mut out: Vec<RangeInclusive<u128>> = Vec::new();
    for &v in sorted {
        match out.last_mut() {
            Some(r) if *r.end() != u128::MAX && *r.end() + 1 == v => *r = *r.start()..=v,
            _ => out.push(v..=v),
        }
    }
    out
}

impl SetSpec {
    pub fn empty() -> SetSpec {
        SetSpec {
            pieces: Vec::new(),
            finish: Finish::Nothing,
        }
    }

    /// Builds the bitmap and its model. Return values of `insert`, `insert_range`, `append` and
    /// `try_push` are checked against the model and `internal_validate` is asserted after
    /// every piece.
    #[track_caller]
    pub fn build(&self) -> (RoaringBitmap128, Model) {
        let mut rb = RoaringBitmap128::new();
        let mut model = Model::new();
        for (piece, build) in &self.pieces {
            add_values(&mut rb, &mut model, piece.values(), *build);
        }
        match self.finish {
            Finish::Nothing => {}
            Finish::Optimize => {
                rb.optimize();
            }
            Finish::RemoveRunCompression => {
                rb.remove_run_compression();
            }
            Finish::OptimizeThenRemoveRunCompression => {
                rb.optimize();
                rb.remove_run_compression();
            }
        }
        assert_valid(&rb);
        (rb, model)
    }

    pub fn bitmap(&self) -> RoaringBitmap128 {
        self.build().0
    }

    pub fn model(&self) -> Model {
        let mut model = Model::new();
        for (piece, _) in &self.pieces {
            model.extend(piece.values());
        }
        model
    }
}

/// Adds `values` to both the bitmap and the model using `build`, checking return values.
#[track_caller]
pub fn add_values(rb: &mut RoaringBitmap128, model: &mut Model, values: Vec<u128>, build: Build) {
    match build {
        Build::Insert => {
            for v in values {
                let expected = model.insert(v);
                assert_eq!(rb.insert(v), expected, "insert({v:#x}) return value");
            }
        }
        Build::Extend => {
            model.extend(values.iter().copied());
            rb.extend(values);
        }
        Build::Ranges => {
            let mut s = values;
            s.sort_unstable();
            s.dedup();
            for r in intervals(&s) {
                let new = r.clone().filter(|v| !model.contains(v)).count() as u64;
                assert_eq!(
                    rb.insert_range(r.clone()),
                    new,
                    "insert_range({r:#x?}) return value"
                );
                model.extend(r);
            }
        }
        Build::Append => {
            let mut s = values;
            s.sort_unstable();
            s.dedup();
            let above_max = match (s.first(), model.last()) {
                (Some(first), Some(max)) => first > max,
                _ => true,
            };
            if above_max {
                let n = s.len() as u64;
                model.extend(s.iter().copied());
                assert_eq!(rb.append(s), Ok(n), "append return value");
            } else {
                model.extend(s.iter().copied());
                rb.extend(s);
            }
        }
        Build::Push => {
            let mut s = values;
            s.sort_unstable();
            s.dedup();
            for v in s {
                let ok = model.last().is_none_or(|&m| v > m);
                let res = rb.try_push(v);
                assert_eq!(res.is_ok(), ok, "try_push({v:#x}) result");
                if !ok {
                    assert_eq!(
                        rb.insert(v),
                        !model.contains(&v),
                        "insert({v:#x}) return value"
                    );
                }
                model.insert(v);
            }
        }
    }
    assert_valid(rb);
}

fn dense_words() -> impl Strategy<Value = Vec<u64>> + Clone {
    prop_oneof![
        2 => vec(any::<u64>(), 1..=16),
        2 => vec(any::<u64>(), 120..=140),
        1 => vec(any::<u64>(), 141..=1024),
    ]
}

fn interval_len() -> impl Strategy<Value = u32> + Clone {
    prop_oneof![
        3 => 1..=100u32,
        2 => 100..=10_000u32,
        1 => 60_000..=140_000u32,
    ]
}

/// One piece of a structured set.
pub fn piece() -> impl Strategy<Value = Piece> + Clone {
    piece_with(anchor(), any::<u128>())
}

/// One piece of a structured set with the given anchors and sparse values (used to restrict
/// sets to the `u32` / `u64` domains for the differential tests).
pub fn piece_with<A, S>(anchor: A, sparse: S) -> impl Strategy<Value = Piece> + Clone
where
    A: Strategy<Value = u128> + Clone + 'static,
    S: Strategy<Value = u128> + Clone + 'static,
{
    prop_oneof![
        4 => (anchor.clone(), vec(0..4 * KEY as u32, 1..=64))
            .prop_map(|(anchor, offsets)| Piece::Points { anchor, offsets }),
        2 => (anchor.clone(), 0..4u8, dense_words())
            .prop_map(|(anchor, key, words)| Piece::Dense { anchor, key, words }),
        3 => (anchor.clone(), 0..4 * KEY as u32, interval_len())
            .prop_map(|(anchor, start, len)| Piece::Interval { anchor, start, len }),
        1 => (anchor.clone(), 0..4 * KEY as u32, 2..=5u8, 1..=20_000u16)
            .prop_map(|(anchor, start, stride, count)| Piece::Strided { anchor, start, stride, count }),
        2 => vec(sparse, 1..=32).prop_map(|values| Piece::Sparse { values }),
        1 => (anchor, 0..4u8, 4090..=4100u16)
            .prop_map(|(anchor, key, n)| Piece::Boundary { anchor, key, n }),
    ]
}

pub fn build_mode() -> impl Strategy<Value = Build> + Clone {
    prop_oneof![
        3 => Just(Build::Insert),
        2 => Just(Build::Extend),
        3 => Just(Build::Ranges),
        1 => Just(Build::Append),
        1 => Just(Build::Push),
    ]
}

pub fn finish() -> impl Strategy<Value = Finish> + Clone {
    prop_oneof![
        3 => Just(Finish::Nothing),
        2 => Just(Finish::Optimize),
        1 => Just(Finish::RemoveRunCompression),
        1 => Just(Finish::OptimizeThenRemoveRunCompression),
    ]
}

/// A structured set made of up to `max_pieces` pieces.
pub fn set_spec_with(max_pieces: usize) -> impl Strategy<Value = SetSpec> + Clone {
    (vec((piece(), build_mode()), 0..=max_pieces), finish())
        .prop_map(|(pieces, finish)| SetSpec { pieces, finish })
}

pub fn set_spec() -> impl Strategy<Value = SetSpec> + Clone {
    set_spec_with(5)
}

/// Two sets that share some pieces (added with possibly different build modes, hence possibly
/// different container representations), so that set operations see overlapping containers.
pub fn spec_pair() -> impl Strategy<Value = (SetSpec, SetSpec)> {
    (
        set_spec_with(4),
        set_spec_with(4),
        vec((piece(), build_mode(), build_mode()), 0..=2),
    )
        .prop_map(|(mut a, mut b, shared)| {
            for (p, ba, bb) in shared {
                a.pieces.push((p.clone(), ba));
                b.pieces.push((p, bb));
            }
            (a, b)
        })
}

/// Three sets sharing some pieces.
pub fn spec_triple() -> impl Strategy<Value = (SetSpec, SetSpec, SetSpec)> {
    (
        set_spec_with(3),
        set_spec_with(3),
        set_spec_with(3),
        vec(
            (piece(), build_mode(), build_mode(), build_mode(), 0..7u8),
            0..=3,
        ),
    )
        .prop_map(|(mut a, mut b, mut c, shared)| {
            for (p, ba, bb, bc, mask) in shared {
                // mask selects which sets get the shared piece (at least one)
                let mask = mask + 1;
                if mask & 1 != 0 {
                    a.pieces.push((p.clone(), ba));
                }
                if mask & 2 != 0 {
                    b.pieces.push((p.clone(), bb));
                }
                if mask & 4 != 0 {
                    c.pieces.push((p, bc));
                }
            }
            (a, b, c)
        })
}

/// `n` sets sharing some pieces.
pub fn spec_list(max: usize) -> impl Strategy<Value = Vec<SetSpec>> {
    (
        vec(set_spec_with(3), 0..=max),
        vec((piece(), build_mode(), any::<u8>()), 0..=3),
    )
        .prop_map(|(mut specs, shared)| {
            let n = specs.len();
            if n > 0 {
                for (p, b, mask) in shared {
                    for (i, s) in specs.iter_mut().enumerate() {
                        if mask & (1 << (i % 8)) != 0 || i == mask as usize % n {
                            s.pieces.push((p.clone(), b));
                        }
                    }
                }
            }
            specs
        })
}

// -------------------------------------------------------------------------------------------------
// Set operations in every form
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetOp {
    Or,
    And,
    Sub,
    Xor,
}

pub const SET_OPS: [SetOp; 4] = [SetOp::Or, SetOp::And, SetOp::Sub, SetOp::Xor];

/// The six ways of writing a binary operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    RefRef,
    RefOwned,
    OwnedRef,
    OwnedOwned,
    AssignRef,
    AssignOwned,
}

pub const FORMS: [Form; 6] = [
    Form::RefRef,
    Form::RefOwned,
    Form::OwnedRef,
    Form::OwnedOwned,
    Form::AssignRef,
    Form::AssignOwned,
];

pub fn set_op() -> impl Strategy<Value = SetOp> + Clone {
    prop::sample::select(SET_OPS.to_vec())
}

pub fn form() -> impl Strategy<Value = Form> + Clone {
    prop::sample::select(FORMS.to_vec())
}

/// `a op b` computed with the given form.
pub fn apply_op(
    op: SetOp,
    form: Form,
    a: &RoaringBitmap128,
    b: &RoaringBitmap128,
) -> RoaringBitmap128 {
    let (a_, b_) = (a.clone(), b.clone());
    match (op, form) {
        (SetOp::Or, Form::RefRef) => a | b,
        (SetOp::Or, Form::RefOwned) => a | b_,
        (SetOp::Or, Form::OwnedRef) => a_ | b,
        (SetOp::Or, Form::OwnedOwned) => a_ | b_,
        (SetOp::Or, Form::AssignRef) => {
            let mut x = a_;
            x |= b;
            x
        }
        (SetOp::Or, Form::AssignOwned) => {
            let mut x = a_;
            x |= b_;
            x
        }
        (SetOp::And, Form::RefRef) => a & b,
        (SetOp::And, Form::RefOwned) => a & b_,
        (SetOp::And, Form::OwnedRef) => a_ & b,
        (SetOp::And, Form::OwnedOwned) => a_ & b_,
        (SetOp::And, Form::AssignRef) => {
            let mut x = a_;
            x &= b;
            x
        }
        (SetOp::And, Form::AssignOwned) => {
            let mut x = a_;
            x &= b_;
            x
        }
        (SetOp::Sub, Form::RefRef) => a - b,
        (SetOp::Sub, Form::RefOwned) => a - b_,
        (SetOp::Sub, Form::OwnedRef) => a_ - b,
        (SetOp::Sub, Form::OwnedOwned) => a_ - b_,
        (SetOp::Sub, Form::AssignRef) => {
            let mut x = a_;
            x -= b;
            x
        }
        (SetOp::Sub, Form::AssignOwned) => {
            let mut x = a_;
            x -= b_;
            x
        }
        (SetOp::Xor, Form::RefRef) => a ^ b,
        (SetOp::Xor, Form::RefOwned) => a ^ b_,
        (SetOp::Xor, Form::OwnedRef) => a_ ^ b,
        (SetOp::Xor, Form::OwnedOwned) => a_ ^ b_,
        (SetOp::Xor, Form::AssignRef) => {
            let mut x = a_;
            x ^= b;
            x
        }
        (SetOp::Xor, Form::AssignOwned) => {
            let mut x = a_;
            x ^= b_;
            x
        }
    }
}

/// `a op b` on models.
pub fn model_op(op: SetOp, a: &Model, b: &Model) -> Model {
    match op {
        SetOp::Or => a | b,
        SetOp::And => a & b,
        SetOp::Sub => a - b,
        SetOp::Xor => a ^ b,
    }
}

// -------------------------------------------------------------------------------------------------
// Iterator model
// -------------------------------------------------------------------------------------------------

/// The inherent iterator API shared by `Iter` and `IntoIter`.
pub trait RoaringIter: DoubleEndedIterator<Item = u128> + ExactSizeIterator + Clone {
    fn advance_to_(&mut self, n: u128);
    fn advance_back_to_(&mut self, n: u128);
    fn next_range_(&mut self) -> Option<RangeInclusive<u128>>;
    fn next_range_back_(&mut self) -> Option<RangeInclusive<u128>>;
}

impl RoaringIter for roaring128::Iter<'_> {
    fn advance_to_(&mut self, n: u128) {
        self.advance_to(n)
    }
    fn advance_back_to_(&mut self, n: u128) {
        self.advance_back_to(n)
    }
    fn next_range_(&mut self) -> Option<RangeInclusive<u128>> {
        self.next_range()
    }
    fn next_range_back_(&mut self) -> Option<RangeInclusive<u128>> {
        self.next_range_back()
    }
}

impl RoaringIter for roaring128::IntoIter {
    fn advance_to_(&mut self, n: u128) {
        self.advance_to(n)
    }
    fn advance_back_to_(&mut self, n: u128) {
        self.advance_back_to(n)
    }
    fn next_range_(&mut self) -> Option<RangeInclusive<u128>> {
        self.next_range()
    }
    fn next_range_back_(&mut self) -> Option<RangeInclusive<u128>> {
        self.next_range_back()
    }
}

/// The remaining values of an iterator: a window of the sorted values.
#[derive(Debug, Clone)]
pub struct ModelIter<'a> {
    pub rest: &'a [u128],
}

impl<'a> ModelIter<'a> {
    pub fn new(sorted: &'a [u128]) -> Self {
        ModelIter { rest: sorted }
    }

    pub fn len(&self) -> usize {
        self.rest.len()
    }

    pub fn next(&mut self) -> Option<u128> {
        let (&first, rest) = self.rest.split_first()?;
        self.rest = rest;
        Some(first)
    }

    pub fn next_back(&mut self) -> Option<u128> {
        let (&last, rest) = self.rest.split_last()?;
        self.rest = rest;
        Some(last)
    }

    pub fn nth(&mut self, n: usize) -> Option<u128> {
        if n >= self.rest.len() {
            self.rest = &[];
            None
        } else {
            let v = self.rest[n];
            self.rest = &self.rest[n + 1..];
            Some(v)
        }
    }

    pub fn nth_back(&mut self, n: usize) -> Option<u128> {
        let len = self.rest.len();
        if n >= len {
            self.rest = &[];
            None
        } else {
            let v = self.rest[len - 1 - n];
            self.rest = &self.rest[..len - 1 - n];
            Some(v)
        }
    }

    pub fn advance_to(&mut self, n: u128) {
        let i = self.rest.partition_point(|&x| x < n);
        self.rest = &self.rest[i..];
    }

    pub fn advance_back_to(&mut self, n: u128) {
        let i = self.rest.partition_point(|&x| x <= n);
        self.rest = &self.rest[..i];
    }

    pub fn next_range(&mut self) -> Option<RangeInclusive<u128>> {
        let first = *self.rest.first()?;
        let mut j = 0;
        while j + 1 < self.rest.len() && self.rest[j + 1] == self.rest[j] + 1 {
            j += 1;
        }
        let r = first..=self.rest[j];
        self.rest = &self.rest[j + 1..];
        Some(r)
    }

    pub fn next_range_back(&mut self) -> Option<RangeInclusive<u128>> {
        let len = self.rest.len();
        let last = *self.rest.last()?;
        let mut j = len - 1;
        while j > 0 && self.rest[j - 1] + 1 == self.rest[j] {
            j -= 1;
        }
        let r = self.rest[j]..=last;
        self.rest = &self.rest[..j];
        Some(r)
    }
}

/// One step on an iterator.
#[derive(Debug, Clone)]
pub enum IterOp {
    Next,
    NextBack,
    Nth(usize),
    NthBack(usize),
    AdvanceTo(u128),
    AdvanceBackTo(u128),
    /// Advance to an element of the whole set (by index), plus `delta` in -1..=1.
    AdvanceToElem(Index, i8),
    AdvanceBackToElem(Index, i8),
    NextRange,
    NextRangeBack,
    /// Clone the iterator and check that the clone yields the rest (then continue with the
    /// original).
    CloneCheck,
}

pub fn iter_op() -> impl Strategy<Value = IterOp> + Clone {
    prop_oneof![
        4 => Just(IterOp::Next),
        4 => Just(IterOp::NextBack),
        1 => (0..300usize).prop_map(IterOp::Nth),
        1 => prop_oneof![Just(4096usize), Just(65_535), Just(65_536), 0..200_000usize, Just(usize::MAX)].prop_map(IterOp::Nth),
        1 => (0..300usize).prop_map(IterOp::NthBack),
        1 => prop_oneof![Just(4096usize), Just(65_535), Just(65_536), 0..200_000usize, Just(usize::MAX)].prop_map(IterOp::NthBack),
        2 => value().prop_map(IterOp::AdvanceTo),
        2 => value().prop_map(IterOp::AdvanceBackTo),
        3 => (any::<Index>(), -1..=1i8).prop_map(|(i, d)| IterOp::AdvanceToElem(i, d)),
        3 => (any::<Index>(), -1..=1i8).prop_map(|(i, d)| IterOp::AdvanceBackToElem(i, d)),
        3 => Just(IterOp::NextRange),
        3 => Just(IterOp::NextRangeBack),
        1 => Just(IterOp::CloneCheck),
    ]
}

fn elem(all: &[u128], i: &Index, d: i8) -> u128 {
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

/// Applies `ops` to `it` and to a model of it over `expected` (the sorted values the iterator
/// starts with); `all` is the sorted content of the whole bitmap (for `*Elem` ops). Checks every
/// result and the exact `size_hint`/`len` after every step, then that the rest matches in both
/// directions and that the iterator is fused.
#[track_caller]
pub fn check_iter_ops<I: RoaringIter>(mut it: I, expected: &[u128], all: &[u128], ops: &[IterOp]) {
    let mut m = ModelIter::new(expected);
    assert_eq!(
        it.size_hint(),
        (m.len(), Some(m.len())),
        "initial size_hint"
    );
    for (step, op) in ops.iter().enumerate() {
        match op {
            IterOp::Next => assert_eq!(it.next(), m.next(), "step {step}: next"),
            IterOp::NextBack => assert_eq!(it.next_back(), m.next_back(), "step {step}: next_back"),
            IterOp::Nth(n) => assert_eq!(it.nth(*n), m.nth(*n), "step {step}: nth({n})"),
            IterOp::NthBack(n) => {
                assert_eq!(
                    it.nth_back(*n),
                    m.nth_back(*n),
                    "step {step}: nth_back({n})"
                )
            }
            IterOp::AdvanceTo(n) => {
                it.advance_to_(*n);
                m.advance_to(*n);
            }
            IterOp::AdvanceBackTo(n) => {
                it.advance_back_to_(*n);
                m.advance_back_to(*n);
            }
            IterOp::AdvanceToElem(i, d) => {
                let n = elem(all, i, *d);
                it.advance_to_(n);
                m.advance_to(n);
            }
            IterOp::AdvanceBackToElem(i, d) => {
                let n = elem(all, i, *d);
                it.advance_back_to_(n);
                m.advance_back_to(n);
            }
            IterOp::NextRange => {
                assert_eq!(it.next_range_(), m.next_range(), "step {step}: next_range")
            }
            IterOp::NextRangeBack => {
                assert_eq!(
                    it.next_range_back_(),
                    m.next_range_back(),
                    "step {step}: next_range_back"
                )
            }
            IterOp::CloneCheck => {
                let c = it.clone();
                assert_eq!(c.len(), m.len(), "step {step}: clone len");
                assert_same_seq(c, m.rest.iter().copied(), "clone yields the rest");
            }
        }
        assert_eq!(
            it.size_hint(),
            (m.len(), Some(m.len())),
            "step {step} ({op:?}): size_hint"
        );
        assert_eq!(it.len(), m.len(), "step {step} ({op:?}): len");
    }
    let rest = m.rest;
    assert_same_seq(
        it.clone().rev(),
        rest.iter().rev().copied(),
        "rest (reversed)",
    );
    assert_eq!(it.clone().count(), rest.len(), "count of the rest");
    assert_same_seq(it.by_ref(), rest.iter().copied(), "rest");
    assert_eq!(it.next(), None, "fused next");
    assert_eq!(it.next_back(), None, "fused next_back");
    assert_eq!(it.next_range_(), None, "fused next_range");
    assert_eq!(it.next_range_back_(), None, "fused next_range_back");
    assert_eq!(it.size_hint(), (0, Some(0)), "exhausted size_hint");
}

/// Interleaves `next` and `next_back`, starting with `next` (from roaring's `tests/iter.rs`).
#[derive(Clone, Debug)]
pub struct OutsideInIter<T>(bool, T);

impl<T, I> Iterator for OutsideInIter<I>
where
    I: DoubleEndedIterator<Item = T>,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let res = if self.0 {
            self.1.next()
        } else {
            self.1.next_back()
        };
        self.0 = !self.0;
        res
    }
}

pub fn outside_in<U, I>(into_iter: I) -> OutsideInIter<U>
where
    U: DoubleEndedIterator,
    I: IntoIterator<IntoIter = U>,
{
    OutsideInIter(true, into_iter.into_iter())
}

// -------------------------------------------------------------------------------------------------
// Serialization helpers
// -------------------------------------------------------------------------------------------------

/// `serialize_into` into a vector, checking `serialized_size`.
#[track_caller]
pub fn serialize(rb: &RoaringBitmap128) -> Vec<u8> {
    let mut buf = Vec::new();
    rb.serialize_into(&mut buf)
        .expect("serialize_into a Vec cannot fail");
    assert_eq!(
        buf.len(),
        rb.serialized_size(),
        "serialized_size() == bytes written"
    );
    buf
}

/// Serializes, checks the size, deserializes (checked and unchecked) and validates.
#[track_caller]
pub fn round_trip(rb: &RoaringBitmap128) -> RoaringBitmap128 {
    let buf = serialize(rb);
    let back = RoaringBitmap128::deserialize_from(&buf[..]).expect("deserialize_from");
    assert_valid(&back);
    assert_eq!(&back, rb, "deserialize_from(serialize_into(x)) == x");
    let unchecked =
        RoaringBitmap128::deserialize_unchecked_from(&buf[..]).expect("deserialize_unchecked_from");
    assert_valid(&unchecked);
    assert_eq!(
        &unchecked, rb,
        "deserialize_unchecked_from(serialize_into(x)) == x"
    );
    back
}
