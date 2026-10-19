//! Run-length encoded container: a sorted list of disjoint, non adjacent inclusive intervals.
//!
//! Besides the store itself this module owns every container-level operation in which at least
//! one operand is a run container (the `Container` methods in `mod.rs` delegate to the free
//! functions at the bottom of this file).

use core::ops::RangeInclusive;

use super::Container;
use super::array::ArrayStore;
use super::bitmap::BitmapStore;
use super::heap::HeapVec;
use crate::simd::Words;

/// An inclusive interval `start..=end` with `start <= end`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub(crate) struct Interval {
    start: u16,
    end: u16,
}

#[allow(dead_code)]
impl Interval {
    /// `start <= end` must hold.
    #[inline]
    pub(crate) const fn new_unchecked(start: u16, end: u16) -> Interval {
        debug_assert!(start <= end);
        Interval { start, end }
    }

    #[inline]
    pub(crate) const fn start(&self) -> u16 {
        self.start
    }

    #[inline]
    pub(crate) const fn end(&self) -> u16 {
        self.end
    }

    /// Number of values in the interval (1..=65536).
    #[inline]
    pub(crate) const fn run_len(&self) -> u64 {
        self.end as u64 - self.start as u64 + 1
    }
}

/// A run container.
#[derive(Clone)]
pub(crate) struct RunStore(HeapVec<Interval>);

const _: () = assert!(core::mem::size_of::<RunStore>() == 8);

/// Serialized size of a run container with `runs` intervals, using the standard Roaring format
/// (a 2-byte run count followed by 4 bytes per run). Used for `optimize` decisions so that they
/// agree with other Roaring implementations.
pub(crate) const fn serialized_byte_size(runs: u64) -> usize {
    2 + 4 * runs as usize
}

#[allow(dead_code)]
impl RunStore {
    pub(crate) const fn new() -> RunStore {
        RunStore(HeapVec::new())
    }

    pub(crate) fn full() -> RunStore {
        RunStore::from_range(0..=u16::MAX)
    }

    pub(crate) fn from_range(range: RangeInclusive<u16>) -> RunStore {
        todo!("{range:?}")
    }

    /// `intervals` must be sorted, disjoint and non adjacent.
    pub(crate) fn from_intervals_unchecked(intervals: &[Interval]) -> RunStore {
        todo!("{}", intervals.len())
    }

    /// Validating constructor used by deserialization.
    pub(crate) fn try_from_intervals(intervals: &[Interval]) -> Result<RunStore, &'static str> {
        todo!("{}", intervals.len())
    }

    pub(crate) fn from_array(array: &ArrayStore) -> RunStore {
        todo!("{}", array.len())
    }

    pub(crate) fn from_bitmap(bitmap: &BitmapStore) -> RunStore {
        todo!("{}", bitmap.len())
    }

    pub(crate) fn intervals(&self) -> &[Interval] {
        self.0.as_slice()
    }

    /// Number of intervals.
    pub(crate) fn run_amount(&self) -> u64 {
        self.0.len() as u64
    }

    pub(crate) fn len(&self) -> u64 {
        todo!()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn insert(&mut self, index: u16) -> bool {
        todo!("{index}")
    }

    pub(crate) fn insert_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        todo!("{range:?}")
    }

    pub(crate) fn push(&mut self, index: u16) -> bool {
        todo!("{index}")
    }

    pub(crate) fn remove(&mut self, index: u16) -> bool {
        todo!("{index}")
    }

    pub(crate) fn remove_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        todo!("{range:?}")
    }

    pub(crate) fn remove_smallest(&mut self, n: u64) {
        todo!("{n}")
    }

    pub(crate) fn remove_biggest(&mut self, n: u64) {
        todo!("{n}")
    }

    pub(crate) fn contains(&self, index: u16) -> bool {
        todo!("{index}")
    }

    pub(crate) fn contains_range(&self, range: RangeInclusive<u16>) -> bool {
        todo!("{range:?}")
    }

    pub(crate) fn min(&self) -> Option<u16> {
        self.intervals().first().map(Interval::start)
    }

    pub(crate) fn max(&self) -> Option<u16> {
        self.intervals().last().map(Interval::end)
    }

    pub(crate) fn rank(&self, index: u16) -> u64 {
        todo!("{index}")
    }

    pub(crate) fn select(&self, n: u16) -> Option<u16> {
        todo!("{n}")
    }

    pub(crate) fn range_cardinality(&self, range: RangeInclusive<u16>) -> u64 {
        todo!("{range:?}")
    }

    pub(crate) fn to_array(&self) -> ArrayStore {
        todo!()
    }

    pub(crate) fn to_bitmap(&self) -> BitmapStore {
        todo!()
    }

    /// Sets every bit covered by the runs (does not touch any cached cardinality).
    pub(crate) fn or_into_words(&self, words: &mut Words) {
        let _ = words;
        todo!()
    }

    /// Flips every bit covered by the runs (does not touch any cached cardinality).
    pub(crate) fn xor_into_words(&self, words: &mut Words) {
        let _ = words;
        todo!()
    }

    /// Serialized payload size of this container in the standard Roaring format
    /// (`2 + 4 * runs`), see [`serialized_byte_size`].
    pub(crate) fn byte_size(&self) -> usize {
        serialized_byte_size(self.run_amount())
    }

    pub(crate) fn mem_size(&self) -> usize {
        todo!()
    }

    pub(crate) fn internal_validate(&self) -> Result<(), &'static str> {
        todo!()
    }
}

impl Default for RunStore {
    fn default() -> Self {
        RunStore::new()
    }
}

impl core::fmt::Debug for RunStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.intervals().iter().map(|iv| iv.start..=iv.end))
            .finish()
    }
}

// ---------------------------------------------------------------------------------------------
// Container operations with at least one run operand.
//
// Representation choices of the results follow the `roaring` crate (0.11, `store/mod.rs`) and
// must satisfy the `Container` invariants (arrays <= ARRAY_LIMIT < bitmaps; runs unrestricted).
// ---------------------------------------------------------------------------------------------

pub(crate) fn union(a: &Container, b: &Container) -> Container {
    todo!("{} {}", a.len(), b.len())
}

pub(crate) fn intersection(a: &Container, b: &Container) -> Container {
    todo!("{} {}", a.len(), b.len())
}

pub(crate) fn difference(a: &Container, b: &Container) -> Container {
    todo!("{} {}", a.len(), b.len())
}

pub(crate) fn symmetric_difference(a: &Container, b: &Container) -> Container {
    todo!("{} {}", a.len(), b.len())
}

pub(crate) fn intersection_len(a: &Container, b: &Container) -> u64 {
    todo!("{} {}", a.len(), b.len())
}

pub(crate) fn is_disjoint(a: &Container, b: &Container) -> bool {
    todo!("{} {}", a.len(), b.len())
}

/// Whether `a ⊆ b`.
pub(crate) fn is_subset(a: &Container, b: &Container) -> bool {
    todo!("{} {}", a.len(), b.len())
}

pub(crate) fn eq_values(a: &Container, b: &Container) -> bool {
    todo!("{} {}", a.len(), b.len())
}
