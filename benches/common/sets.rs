//! A common interface over roaring128 and the baselines, so every benchmark and the memory
//! table run the same code for each implementation.

use std::collections::{BTreeSet, HashSet};

use roaring::{MultiOps as _, RoaringTreemap};
use roaring128::{MultiOps as _, RoaringBitmap128};
use rustc_hash::FxBuildHasher;

use super::sortedvec::SortedVec;
use super::wrapped64::Wrapped64;

pub type R128 = RoaringBitmap128;
pub type Bts = BTreeSet<u128>;
/// `std`'s default SipHash-1-3, as a user gets without choosing a hasher.
pub type Hs = HashSet<u128>;
/// FxHash (`rustc-hash`), a fast non-cryptographic hasher.
pub type HsFx = HashSet<u128, FxBuildHasher>;
pub type Tm = RoaringTreemap;
pub type Sv = SortedVec;

/// Every implementation name, as used in benchmark ids and by `BENCH_IMPL`.
pub const IMPLS: [&str; 7] = [
    R128::NAME,
    Wrapped64::NAME,
    Bts::NAME,
    Sv::NAME,
    Hs::NAME,
    HsFx::NAME,
    Tm::NAME,
];

/// Order-independent digest of a set's contents (count, wrapping sum, xor), used both as the
/// black-boxed result of iteration benchmarks and by the self-check. Deliberately cheap (a few
/// ALU operations per value) so that iteration benchmarks measure the iterator.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Digest {
    pub count: u64,
    pub sum: u128,
    pub mix: u128,
}

impl Digest {
    #[inline]
    pub fn add(mut self, v: u128) -> Digest {
        self.count += 1;
        self.sum = self.sum.wrapping_add(v);
        self.mix ^= v;
        self
    }

    pub fn of<I: IntoIterator<Item = u128>>(values: I) -> Digest {
        values.into_iter().fold(Digest::default(), Digest::add)
    }
}

/// Operations every implementation supports.
pub trait Set128: Clone + Sized {
    /// Name used in benchmark ids: `<group>/<NAME>/<workload>`.
    const NAME: &'static str;
    /// [`Set128::build_sorted`] is supported.
    const BULK_BUILD: bool = false;
    /// [`Set128::range_digest`] is supported.
    const RANGE: bool = false;
    /// Single-value `insert` / `remove` are efficient (not O(n) shifts), so they are benchmarked.
    const POINT_UPDATES: bool = true;

    fn new_empty() -> Self;
    fn insert(&mut self, v: u128) -> bool;
    fn contains(&self, v: u128) -> bool;
    fn remove(&mut self, v: u128) -> bool;
    fn len(&self) -> u64;
    /// Full iteration.
    fn digest(&self) -> Digest;
    /// All values in increasing order (self-check only).
    fn to_sorted_vec(&self) -> Vec<u128>;

    /// Build by inserting values one by one (in the given order).
    fn build_insert(values: &[u128]) -> Self {
        let mut s = Self::new_empty();
        for &v in values {
            s.insert(v);
        }
        s
    }

    /// Build from values in any order the way a user builds from a batch: the bulk API if there
    /// is one (`collect()`, or sort + sorted bulk build), otherwise inserts.
    fn build_from_iter(values: &[u128]) -> Self {
        Self::build_insert(values)
    }

    /// Build from strictly increasing values with the implementation's bulk API, if any.
    fn build_sorted(_sorted: &[u128]) -> Option<Self> {
        None
    }

    /// Iterate the values in `lo..=hi`; `None` if the implementation has no ordered range API.
    fn range_digest(&self, _lo: u128, _hi: u128) -> Option<Digest> {
        None
    }

    /// Convert containers to run containers where smaller (roaring implementations only).
    fn optimize(&mut self) {}
}

/// `&a op &b` and friends.
pub trait SetAlgebra: Set128 {
    fn and(&self, o: &Self) -> Self;
    fn or(&self, o: &Self) -> Self;
    fn sub(&self, o: &Self) -> Self;
    fn xor(&self, o: &Self) -> Self;
    fn intersection_len(&self, o: &Self) -> u64;
    fn is_subset(&self, o: &Self) -> bool;
    fn union_many(sets: &[&Self]) -> Self;
    fn intersection_many(sets: &[&Self]) -> Self;
}

/// `a op= &b`.
pub trait SetAlgebraInPlace: Set128 {
    fn and_assign(&mut self, o: &Self);
    fn or_assign(&mut self, o: &Self);
    fn sub_assign(&mut self, o: &Self);
    fn xor_assign(&mut self, o: &Self);
}

pub trait Ranked: Set128 {
    fn rank(&self, v: u128) -> u64;
    fn select(&self, n: u64) -> Option<u128>;
}

pub trait Serial: Set128 {
    fn serialized_size(&self) -> usize;
    fn serialize(&self, out: &mut Vec<u8>);
    fn deserialize(bytes: &[u8]) -> Self;
}

// ------------------------------------------------------------------------------------------
// roaring128
// ------------------------------------------------------------------------------------------

impl Set128 for R128 {
    const NAME: &'static str = "roaring128";
    const BULK_BUILD: bool = true;
    const RANGE: bool = true;

    fn new_empty() -> Self {
        R128::new()
    }
    #[inline]
    fn insert(&mut self, v: u128) -> bool {
        R128::insert(self, v)
    }
    #[inline]
    fn contains(&self, v: u128) -> bool {
        R128::contains(self, v)
    }
    #[inline]
    fn remove(&mut self, v: u128) -> bool {
        R128::remove(self, v)
    }
    fn len(&self) -> u64 {
        R128::len(self)
    }
    fn digest(&self) -> Digest {
        self.iter().fold(Digest::default(), Digest::add)
    }
    fn to_sorted_vec(&self) -> Vec<u128> {
        self.iter().collect()
    }
    /// `collect()`: batched sort-and-merge `Extend`.
    fn build_from_iter(values: &[u128]) -> Self {
        values.iter().copied().collect()
    }
    fn build_sorted(sorted: &[u128]) -> Option<Self> {
        Some(R128::from_sorted_iter(sorted.iter().copied()).expect("sorted"))
    }
    fn range_digest(&self, lo: u128, hi: u128) -> Option<Digest> {
        Some(self.range(lo..=hi).fold(Digest::default(), Digest::add))
    }
    fn optimize(&mut self) {
        R128::optimize(self);
    }
}

impl SetAlgebra for R128 {
    fn and(&self, o: &Self) -> Self {
        self & o
    }
    fn or(&self, o: &Self) -> Self {
        self | o
    }
    fn sub(&self, o: &Self) -> Self {
        self - o
    }
    fn xor(&self, o: &Self) -> Self {
        self ^ o
    }
    fn intersection_len(&self, o: &Self) -> u64 {
        R128::intersection_len(self, o)
    }
    fn is_subset(&self, o: &Self) -> bool {
        R128::is_subset(self, o)
    }
    fn union_many(sets: &[&Self]) -> Self {
        sets.iter().copied().union()
    }
    fn intersection_many(sets: &[&Self]) -> Self {
        sets.iter().copied().intersection()
    }
}

impl SetAlgebraInPlace for R128 {
    fn and_assign(&mut self, o: &Self) {
        *self &= o;
    }
    fn or_assign(&mut self, o: &Self) {
        *self |= o;
    }
    fn sub_assign(&mut self, o: &Self) {
        *self -= o;
    }
    fn xor_assign(&mut self, o: &Self) {
        *self ^= o;
    }
}

impl Ranked for R128 {
    fn rank(&self, v: u128) -> u64 {
        R128::rank(self, v)
    }
    fn select(&self, n: u64) -> Option<u128> {
        R128::select(self, n)
    }
}

impl Serial for R128 {
    fn serialized_size(&self) -> usize {
        R128::serialized_size(self)
    }
    fn serialize(&self, out: &mut Vec<u8>) {
        self.serialize_into(out).unwrap();
    }
    fn deserialize(bytes: &[u8]) -> Self {
        R128::deserialize_from(bytes).unwrap()
    }
}

// ------------------------------------------------------------------------------------------
// Wrapped64: BTreeMap<u64, RoaringTreemap>
// ------------------------------------------------------------------------------------------

impl Set128 for Wrapped64 {
    const NAME: &'static str = "wrapped64";
    const BULK_BUILD: bool = true;
    const RANGE: bool = true;

    fn new_empty() -> Self {
        Wrapped64::new()
    }
    #[inline]
    fn insert(&mut self, v: u128) -> bool {
        Wrapped64::insert(self, v)
    }
    #[inline]
    fn contains(&self, v: u128) -> bool {
        Wrapped64::contains(self, v)
    }
    #[inline]
    fn remove(&mut self, v: u128) -> bool {
        Wrapped64::remove(self, v)
    }
    fn len(&self) -> u64 {
        Wrapped64::len(self)
    }
    fn digest(&self) -> Digest {
        self.iter().fold(Digest::default(), Digest::add)
    }
    fn to_sorted_vec(&self) -> Vec<u128> {
        self.iter().collect()
    }
    /// Sort a copy, then the sorted bulk build.
    fn build_from_iter(values: &[u128]) -> Self {
        Wrapped64::from_unsorted(values)
    }
    fn build_sorted(sorted: &[u128]) -> Option<Self> {
        Some(Wrapped64::from_sorted_iter(sorted.iter().copied()).expect("sorted"))
    }
    fn range_digest(&self, lo: u128, hi: u128) -> Option<Digest> {
        Some(self.range(lo..=hi).fold(Digest::default(), Digest::add))
    }
    fn optimize(&mut self) {
        Wrapped64::optimize(self);
    }
}

impl SetAlgebra for Wrapped64 {
    fn and(&self, o: &Self) -> Self {
        self & o
    }
    fn or(&self, o: &Self) -> Self {
        self | o
    }
    fn sub(&self, o: &Self) -> Self {
        self - o
    }
    fn xor(&self, o: &Self) -> Self {
        self ^ o
    }
    fn intersection_len(&self, o: &Self) -> u64 {
        Wrapped64::intersection_len(self, o)
    }
    fn is_subset(&self, o: &Self) -> bool {
        Wrapped64::is_subset(self, o)
    }
    fn union_many(sets: &[&Self]) -> Self {
        Wrapped64::union_many(sets.iter().copied())
    }
    fn intersection_many(sets: &[&Self]) -> Self {
        Wrapped64::intersection_many(sets.iter().copied())
    }
}

impl SetAlgebraInPlace for Wrapped64 {
    fn and_assign(&mut self, o: &Self) {
        *self &= o;
    }
    fn or_assign(&mut self, o: &Self) {
        *self |= o;
    }
    fn sub_assign(&mut self, o: &Self) {
        *self -= o;
    }
    fn xor_assign(&mut self, o: &Self) {
        *self ^= o;
    }
}

impl Ranked for Wrapped64 {
    fn rank(&self, v: u128) -> u64 {
        Wrapped64::rank(self, v)
    }
    fn select(&self, n: u64) -> Option<u128> {
        Wrapped64::select(self, n)
    }
}

impl Serial for Wrapped64 {
    fn serialized_size(&self) -> usize {
        Wrapped64::serialized_size(self)
    }
    fn serialize(&self, out: &mut Vec<u8>) {
        self.serialize_into(out).unwrap();
    }
    fn deserialize(bytes: &[u8]) -> Self {
        Wrapped64::deserialize_from(bytes).unwrap()
    }
}

// ------------------------------------------------------------------------------------------
// BTreeSet<u128>
// ------------------------------------------------------------------------------------------

impl Set128 for Bts {
    const NAME: &'static str = "btreeset";
    const BULK_BUILD: bool = true;
    const RANGE: bool = true;

    fn new_empty() -> Self {
        BTreeSet::new()
    }
    #[inline]
    fn insert(&mut self, v: u128) -> bool {
        BTreeSet::insert(self, v)
    }
    #[inline]
    fn contains(&self, v: u128) -> bool {
        BTreeSet::contains(self, &v)
    }
    #[inline]
    fn remove(&mut self, v: u128) -> bool {
        BTreeSet::remove(self, &v)
    }
    fn len(&self) -> u64 {
        BTreeSet::len(self) as u64
    }
    fn digest(&self) -> Digest {
        self.iter().copied().fold(Digest::default(), Digest::add)
    }
    fn to_sorted_vec(&self) -> Vec<u128> {
        self.iter().copied().collect()
    }
    /// `collect()`: `std` sorts and bulk-builds.
    fn build_from_iter(values: &[u128]) -> Self {
        values.iter().copied().collect()
    }
    /// `collect()` from a sorted iterator: `std` sorts (already sorted: linear) and bulk-builds.
    fn build_sorted(sorted: &[u128]) -> Option<Self> {
        Some(sorted.iter().copied().collect())
    }
    fn range_digest(&self, lo: u128, hi: u128) -> Option<Digest> {
        Some(
            self.range(lo..=hi)
                .copied()
                .fold(Digest::default(), Digest::add),
        )
    }
}

/// `std`'s `&a op &b` (sorted merges collected into a bulk-built tree).
impl SetAlgebra for Bts {
    fn and(&self, o: &Self) -> Self {
        self & o
    }
    fn or(&self, o: &Self) -> Self {
        self | o
    }
    fn sub(&self, o: &Self) -> Self {
        self - o
    }
    fn xor(&self, o: &Self) -> Self {
        self ^ o
    }
    fn intersection_len(&self, o: &Self) -> u64 {
        self.intersection(o).count() as u64
    }
    fn is_subset(&self, o: &Self) -> bool {
        BTreeSet::is_subset(self, o)
    }
    fn union_many(sets: &[&Self]) -> Self {
        let mut acc = BTreeSet::new();
        for s in sets {
            acc.extend(s.iter().copied());
        }
        acc
    }
    /// `&a & &b`, then `retain` with the others.
    fn intersection_many(sets: &[&Self]) -> Self {
        let Some((first, rest)) = sets.split_first() else {
            return BTreeSet::new();
        };
        let mut acc = match rest.first() {
            Some(second) => *first & *second,
            None => (*first).clone(),
        };
        for s in rest.iter().skip(1) {
            acc.retain(|v| s.contains(v));
        }
        acc
    }
}

/// What a `BTreeSet` user writes for in-place operators, visiting the smaller side.
impl SetAlgebraInPlace for Bts {
    fn and_assign(&mut self, o: &Self) {
        if o.len() * 16 <= self.len() {
            *self = &*self & o;
        } else {
            self.retain(|v| o.contains(v));
        }
    }
    fn or_assign(&mut self, o: &Self) {
        self.extend(o.iter().copied());
    }
    fn sub_assign(&mut self, o: &Self) {
        if self.len() < o.len() {
            self.retain(|v| !o.contains(v));
        } else {
            for v in o {
                self.remove(v);
            }
        }
    }
    fn xor_assign(&mut self, o: &Self) {
        for &v in o {
            if !self.remove(&v) {
                self.insert(v);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------
// HashSet<u128>: std SipHash-1-3 (what a user gets by default) and FxHash
// ------------------------------------------------------------------------------------------

macro_rules! hash_set {
    ($ty:ty, $name:literal) => {
        impl Set128 for $ty {
            const NAME: &'static str = $name;

            fn new_empty() -> Self {
                <$ty>::default()
            }
            #[inline]
            fn insert(&mut self, v: u128) -> bool {
                HashSet::insert(self, v)
            }
            #[inline]
            fn contains(&self, v: u128) -> bool {
                HashSet::contains(self, &v)
            }
            #[inline]
            fn remove(&mut self, v: u128) -> bool {
                HashSet::remove(self, &v)
            }
            fn len(&self) -> u64 {
                HashSet::len(self) as u64
            }
            fn digest(&self) -> Digest {
                self.iter().copied().fold(Digest::default(), Digest::add)
            }
            fn to_sorted_vec(&self) -> Vec<u128> {
                let mut v: Vec<u128> = self.iter().copied().collect();
                v.sort_unstable();
                v
            }
            /// `collect()` (reserves from the exact size hint).
            fn build_from_iter(values: &[u128]) -> Self {
                values.iter().copied().collect()
            }
        }
    };
}

hash_set!(Hs, "hashset");
hash_set!(HsFx, "hashset_fx");

// ------------------------------------------------------------------------------------------
// roaring::RoaringTreemap, for workloads whose values fit in u64
// ------------------------------------------------------------------------------------------

#[inline]
fn to_u64(v: u128) -> u64 {
    u64::try_from(v).expect("RoaringTreemap workloads must fit in u64")
}

impl Set128 for Tm {
    const NAME: &'static str = "treemap";
    const BULK_BUILD: bool = true;

    fn new_empty() -> Self {
        RoaringTreemap::new()
    }
    #[inline]
    fn insert(&mut self, v: u128) -> bool {
        RoaringTreemap::insert(self, to_u64(v))
    }
    #[inline]
    fn contains(&self, v: u128) -> bool {
        u64::try_from(v).is_ok_and(|v| RoaringTreemap::contains(self, v))
    }
    #[inline]
    fn remove(&mut self, v: u128) -> bool {
        u64::try_from(v).is_ok_and(|v| RoaringTreemap::remove(self, v))
    }
    fn len(&self) -> u64 {
        RoaringTreemap::len(self)
    }
    fn digest(&self) -> Digest {
        self.iter().fold(Digest::default(), |d, v| d.add(v as u128))
    }
    fn to_sorted_vec(&self) -> Vec<u128> {
        self.iter().map(u128::from).collect()
    }
    /// Sort a copy, then `from_sorted_iter` (`collect()` inserts value by value).
    fn build_from_iter(values: &[u128]) -> Self {
        let mut v: Vec<u64> = values.iter().map(|&v| to_u64(v)).collect();
        v.sort_unstable();
        v.dedup();
        RoaringTreemap::from_sorted_iter(v).expect("sorted")
    }
    fn build_sorted(sorted: &[u128]) -> Option<Self> {
        Some(RoaringTreemap::from_sorted_iter(sorted.iter().map(|&v| to_u64(v))).expect("sorted"))
    }
    fn optimize(&mut self) {
        RoaringTreemap::optimize(self);
    }
}

impl SetAlgebra for Tm {
    fn and(&self, o: &Self) -> Self {
        self & o
    }
    fn or(&self, o: &Self) -> Self {
        self | o
    }
    fn sub(&self, o: &Self) -> Self {
        self - o
    }
    fn xor(&self, o: &Self) -> Self {
        self ^ o
    }
    fn intersection_len(&self, o: &Self) -> u64 {
        RoaringTreemap::intersection_len(self, o)
    }
    fn is_subset(&self, o: &Self) -> bool {
        RoaringTreemap::is_subset(self, o)
    }
    fn union_many(sets: &[&Self]) -> Self {
        sets.iter().copied().union()
    }
    fn intersection_many(sets: &[&Self]) -> Self {
        sets.iter().copied().intersection()
    }
}

impl SetAlgebraInPlace for Tm {
    fn and_assign(&mut self, o: &Self) {
        *self &= o;
    }
    fn or_assign(&mut self, o: &Self) {
        *self |= o;
    }
    fn sub_assign(&mut self, o: &Self) {
        *self -= o;
    }
    fn xor_assign(&mut self, o: &Self) {
        *self ^= o;
    }
}

impl Ranked for Tm {
    fn rank(&self, v: u128) -> u64 {
        match u64::try_from(v) {
            Ok(v) => RoaringTreemap::rank(self, v),
            Err(_) => RoaringTreemap::len(self),
        }
    }
    fn select(&self, n: u64) -> Option<u128> {
        RoaringTreemap::select(self, n).map(u128::from)
    }
}

impl Serial for Tm {
    fn serialized_size(&self) -> usize {
        RoaringTreemap::serialized_size(self)
    }
    fn serialize(&self, out: &mut Vec<u8>) {
        self.serialize_into(out).unwrap();
    }
    fn deserialize(bytes: &[u8]) -> Self {
        RoaringTreemap::deserialize_from(bytes).unwrap()
    }
}

// ------------------------------------------------------------------------------------------
// Sorted Vec<u128>
// ------------------------------------------------------------------------------------------

impl Set128 for Sv {
    const NAME: &'static str = "sortedvec";
    const BULK_BUILD: bool = true;
    const RANGE: bool = true;
    const POINT_UPDATES: bool = false;

    fn new_empty() -> Self {
        SortedVec::default()
    }
    #[inline]
    fn insert(&mut self, v: u128) -> bool {
        SortedVec::insert(self, v)
    }
    #[inline]
    fn contains(&self, v: u128) -> bool {
        SortedVec::contains(self, v)
    }
    #[inline]
    fn remove(&mut self, v: u128) -> bool {
        SortedVec::remove(self, v)
    }
    fn len(&self) -> u64 {
        SortedVec::len(self) as u64
    }
    fn digest(&self) -> Digest {
        self.as_slice()
            .iter()
            .copied()
            .fold(Digest::default(), Digest::add)
    }
    fn to_sorted_vec(&self) -> Vec<u128> {
        self.as_slice().to_vec()
    }
    /// Sort + dedup (per-value inserts would be O(n) each).
    fn build_insert(values: &[u128]) -> Self {
        SortedVec::from_unsorted(values)
    }
    fn build_from_iter(values: &[u128]) -> Self {
        SortedVec::from_unsorted(values)
    }
    fn build_sorted(sorted: &[u128]) -> Option<Self> {
        Some(SortedVec::from_sorted(sorted.to_vec()))
    }
    fn range_digest(&self, lo: u128, hi: u128) -> Option<Digest> {
        Some(
            self.range(lo, hi)
                .iter()
                .copied()
                .fold(Digest::default(), Digest::add),
        )
    }
}

impl SetAlgebra for Sv {
    fn and(&self, o: &Self) -> Self {
        SortedVec::and(self, o)
    }
    fn or(&self, o: &Self) -> Self {
        SortedVec::or(self, o)
    }
    fn sub(&self, o: &Self) -> Self {
        SortedVec::sub(self, o)
    }
    fn xor(&self, o: &Self) -> Self {
        SortedVec::xor(self, o)
    }
    fn intersection_len(&self, o: &Self) -> u64 {
        SortedVec::intersection_len(self, o)
    }
    fn is_subset(&self, o: &Self) -> bool {
        SortedVec::is_subset(self, o)
    }
    fn union_many(sets: &[&Self]) -> Self {
        SortedVec::union_many(sets)
    }
    fn intersection_many(sets: &[&Self]) -> Self {
        SortedVec::intersection_many(sets)
    }
}

impl SetAlgebraInPlace for Sv {
    fn and_assign(&mut self, o: &Self) {
        SortedVec::and_assign(self, o);
    }
    fn or_assign(&mut self, o: &Self) {
        SortedVec::or_assign(self, o);
    }
    fn sub_assign(&mut self, o: &Self) {
        SortedVec::sub_assign(self, o);
    }
    fn xor_assign(&mut self, o: &Self) {
        SortedVec::xor_assign(self, o);
    }
}

impl Ranked for Sv {
    fn rank(&self, v: u128) -> u64 {
        SortedVec::rank(self, v)
    }
    fn select(&self, n: u64) -> Option<u128> {
        SortedVec::select(self, n)
    }
}

impl Serial for Sv {
    fn serialized_size(&self) -> usize {
        SortedVec::serialized_size(self)
    }
    fn serialize(&self, out: &mut Vec<u8>) {
        self.serialize_into(out).unwrap();
    }
    fn deserialize(bytes: &[u8]) -> Self {
        SortedVec::deserialize_from(bytes).unwrap()
    }
}
