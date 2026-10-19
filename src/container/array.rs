//! Sorted array container with small-size optimization.
//!
//! Up to [`INLINE_CAPACITY`] values are stored inline (no allocation); larger arrays use a
//! [`HeapVec`]. The representation is an implementation detail: every operation is expressed on
//! the sorted slice returned by [`ArrayStore::as_slice`].
//!
//! Array-array set operations use the vectorized kernels of [`crate::simd`]. Results whose
//! cardinality could exceed [`super::ARRAY_LIMIT`] are computed into a bitmap instead (like
//! CRoaring), see `super::Container`.

use core::mem::MaybeUninit;
use core::ops::RangeInclusive;

use super::bitmap::BitmapStore;
use super::heap::HeapVec;
use crate::simd;

/// Number of values stored without heap allocation.
pub(crate) const INLINE_CAPACITY: usize = 7;

#[derive(Clone)]
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
        if capacity <= INLINE_CAPACITY {
            ArrayStore::new()
        } else {
            ArrayStore(Repr::Heap(HeapVec::with_capacity(capacity)))
        }
    }

    /// `values` must be strictly increasing.
    pub(crate) fn from_sorted_unchecked(values: &[u16]) -> ArrayStore {
        debug_assert!(values.windows(2).all(|w| w[0] < w[1]));
        if values.len() <= INLINE_CAPACITY {
            let mut data = [0; INLINE_CAPACITY];
            data[..values.len()].copy_from_slice(values);
            ArrayStore(Repr::Inline {
                len: values.len() as u8,
                data,
            })
        } else {
            ArrayStore(Repr::Heap(HeapVec::from_slice(values)))
        }
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
        f: impl FnOnce(&mut [MaybeUninit<u16>]) -> usize,
    ) -> ArrayStore {
        if bound <= INLINE_CAPACITY {
            let mut buf = [MaybeUninit::<u16>::uninit(); INLINE_CAPACITY];
            let n = f(&mut buf[..bound]);
            assert!(n <= bound, "array kernel exceeded its bound");
            // SAFETY: the kernel initialized the first `n` values.
            return ArrayStore::from_sorted_unchecked(unsafe { simd::assume_init(&buf, n) });
        }
        let mut heap = HeapVec::with_capacity(bound);
        let n = f(&mut heap.spare_capacity_mut()[..bound]);
        assert!(n <= bound, "array kernel exceeded its bound");
        // SAFETY: the kernel initialized the first `n` values of the spare capacity.
        unsafe { heap.set_len(n) };
        if n <= INLINE_CAPACITY {
            return ArrayStore::from_sorted_unchecked(heap.as_slice());
        }
        // Give back memory when the result is much smaller than its bound (e.g. intersections).
        if n < bound / 2 {
            heap.shrink_to_fit();
        }
        ArrayStore(Repr::Heap(heap))
    }

    pub(crate) fn len(&self) -> u64 {
        self.as_slice().len() as u64
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[u16] {
        match &self.0 {
            Repr::Inline { len, data } => &data[..*len as usize],
            Repr::Heap(heap) => heap.as_slice(),
        }
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
