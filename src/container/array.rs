//! Sorted array container with small-size optimization.
//!
//! Up to [`INLINE_CAPACITY`] values are stored inline (no allocation); larger arrays use a
//! [`HeapVec`]. The representation is an implementation detail: every operation is expressed on
//! the sorted slice returned by [`ArrayStore::as_slice`].
//!
//! Array-array set operations use the vectorized kernels of [`crate::simd`]. Results whose
//! cardinality could exceed [`super::ARRAY_LIMIT`] are computed into a bitmap instead (like
//! CRoaring), see `super::Container`.

use core::ops::RangeInclusive;

use super::bitmap::BitmapStore;
use super::heap::HeapVec;

/// Number of values stored without heap allocation.
pub(crate) const INLINE_CAPACITY: usize = 7;

#[derive(Clone)]
#[allow(dead_code)]
enum Repr {
    Inline {
        len: u8,
        data: [u16; INLINE_CAPACITY],
    },
    Heap(HeapVec<u16>),
}

/// A strictly increasing list of `u16` values.
#[derive(Clone)]
pub(crate) struct ArrayStore(Repr);

const _: () = assert!(core::mem::size_of::<ArrayStore>() == 16);

#[allow(dead_code)]
impl ArrayStore {
    pub(crate) const fn new() -> ArrayStore {
        ArrayStore(Repr::Inline {
            len: 0,
            data: [0; INLINE_CAPACITY],
        })
    }

    pub(crate) fn with_single(index: u16) -> ArrayStore {
        let mut data = [0; INLINE_CAPACITY];
        data[0] = index;
        ArrayStore(Repr::Inline { len: 1, data })
    }

    pub(crate) fn with_capacity(capacity: usize) -> ArrayStore {
        todo!("{capacity}")
    }

    /// `values` must be strictly increasing.
    pub(crate) fn from_sorted_unchecked(values: &[u16]) -> ArrayStore {
        todo!("{}", values.len())
    }

    /// Validating constructor used by deserialization: strictly increasing and at most
    /// `ARRAY_LIMIT` values.
    pub(crate) fn try_from_sorted(values: &[u16]) -> Result<ArrayStore, &'static str> {
        todo!("{}", values.len())
    }

    /// Builds an array with an output kernel: `f` receives a buffer of at least `bound`
    /// elements, initializes a prefix of it and returns its length. Small results are stored
    /// inline.
    pub(crate) fn build(
        bound: usize,
        f: impl FnOnce(&mut [core::mem::MaybeUninit<u16>]) -> usize,
    ) -> ArrayStore {
        let _ = f;
        todo!("{bound}")
    }

    pub(crate) fn len(&self) -> u64 {
        self.as_slice().len() as u64
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    pub(crate) fn as_slice(&self) -> &[u16] {
        todo!()
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

    pub(crate) fn push_unchecked(&mut self, index: u16) {
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

    pub(crate) fn retain(&mut self, f: impl FnMut(u16) -> bool) {
        let _ = f;
        todo!()
    }

    pub(crate) fn contains(&self, index: u16) -> bool {
        todo!("{index}")
    }

    pub(crate) fn contains_range(&self, range: RangeInclusive<u16>) -> bool {
        todo!("{range:?}")
    }

    pub(crate) fn min(&self) -> Option<u16> {
        self.as_slice().first().copied()
    }

    pub(crate) fn max(&self) -> Option<u16> {
        self.as_slice().last().copied()
    }

    pub(crate) fn rank(&self, index: u16) -> u64 {
        todo!("{index}")
    }

    pub(crate) fn select(&self, n: u16) -> Option<u16> {
        self.as_slice().get(n as usize).copied()
    }

    pub(crate) fn range_cardinality(&self, range: RangeInclusive<u16>) -> u64 {
        todo!("{range:?}")
    }

    pub(crate) fn to_bitmap_store(&self) -> BitmapStore {
        todo!()
    }

    /// Number of runs of consecutive values.
    pub(crate) fn count_runs(&self) -> u64 {
        todo!()
    }

    /// Serialized payload size (`2 * len`).
    pub(crate) fn byte_size(&self) -> usize {
        self.as_slice().len() * 2
    }

    pub(crate) fn mem_size(&self) -> usize {
        todo!()
    }

    pub(crate) fn internal_validate(&self) -> Result<(), &'static str> {
        todo!()
    }
}

impl Default for ArrayStore {
    fn default() -> Self {
        ArrayStore::new()
    }
}

impl core::fmt::Debug for ArrayStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}
