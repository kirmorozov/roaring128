//! Sorted array container with small-size optimization.
//!
//! Up to [`INLINE_CAPACITY`] values are stored inline (no allocation); larger arrays use a
//! [`HeapVec`]. The representation is an implementation detail: every operation is expressed on
//! the sorted slice returned by [`ArrayStore::as_slice`].
//!
//! Array-array set operations use the vectorized kernels of [`crate::simd`]. Results whose
//! cardinality could exceed [`super::ARRAY_LIMIT`] are computed into a bitmap instead (like
//! CRoaring), see `super::Container`.
//!
//! The inline/heap choice is not canonical: a heap array may hold few values (point removals only
//! move an array back inline once it is well below [`INLINE_CAPACITY`], so that alternating
//! insertions and removals at the boundary do not reallocate). Constructors and bulk operations
//! always store small results inline.

use core::mem::{MaybeUninit, size_of};
use core::ops::{Range, RangeInclusive};

use super::ARRAY_LIMIT;
use super::bitmap::BitmapStore;
use super::heap::HeapVec;
use crate::simd;

/// Number of values stored without heap allocation.
pub(crate) const INLINE_CAPACITY: usize = 7;

/// Capacity of the first allocation when an inline array overflows.
const FIRST_HEAP_CAPACITY: usize = 16;

/// A heap array shrinking to this many values through point removals moves back inline.
const POINT_REMOVAL_INLINE_LEN: usize = INLINE_CAPACITY / 2;

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
        debug_assert!(is_strictly_increasing(values));
        if values.len() <= INLINE_CAPACITY {
            Self::inline_from(values)
        } else {
            ArrayStore(Repr::Heap(HeapVec::from_slice(values)))
        }
    }

    /// An inline array holding `values` (at most [`INLINE_CAPACITY`]).
    #[inline]
    fn inline_from(values: &[u16]) -> ArrayStore {
        let mut data = [0; INLINE_CAPACITY];
        data[..values.len()].copy_from_slice(values);
        ArrayStore(Repr::Inline {
            len: values.len() as u8,
            data,
        })
    }

    /// Validating constructor used by deserialization: strictly increasing and at most
    /// `ARRAY_LIMIT` values.
    pub(crate) fn try_from_sorted(values: &[u16]) -> Result<ArrayStore, &'static str> {
        if values.len() as u64 > ARRAY_LIMIT {
            return Err("array container cardinality exceeds ARRAY_LIMIT");
        }
        if !is_strictly_increasing(values) {
            return Err("array container values are not strictly increasing");
        }
        Ok(ArrayStore::from_sorted_unchecked(values))
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

    #[inline]
    fn as_mut_slice(&mut self) -> &mut [u16] {
        match &mut self.0 {
            Repr::Inline { len, data } => &mut data[..*len as usize],
            Repr::Heap(heap) => heap.as_mut_slice(),
        }
    }

    /// Whether the values live on the heap (used by tests and statistics).
    pub(crate) fn is_heap(&self) -> bool {
        matches!(self.0, Repr::Heap(_))
    }

    /// Inserts `value` at position `pos` (the caller keeps the order).
    fn insert_at(&mut self, pos: usize, value: u16) {
        match &mut self.0 {
            Repr::Inline { len, data } => {
                let n = *len as usize;
                if n < INLINE_CAPACITY {
                    data.copy_within(pos..n, pos + 1);
                    data[pos] = value;
                    *len += 1;
                } else {
                    let mut heap = HeapVec::with_capacity(FIRST_HEAP_CAPACITY);
                    heap.extend_from_slice(&data[..pos]);
                    heap.push(value);
                    heap.extend_from_slice(&data[pos..]);
                    self.0 = Repr::Heap(heap);
                }
            }
            Repr::Heap(heap) => heap.insert(pos, value),
        }
    }

    /// Removes the values at `positions`, then compacts the storage.
    fn remove_positions(&mut self, positions: Range<usize>) {
        if positions.is_empty() {
            return;
        }
        match &mut self.0 {
            Repr::Inline { len, data } => {
                let n = *len as usize;
                data.copy_within(positions.end..n, positions.start);
                *len -= positions.len() as u8;
            }
            Repr::Heap(heap) => heap.remove_range(positions),
        }
        self.compact();
    }

    /// Keeps the first `n` values, then compacts the storage.
    fn truncate(&mut self, n: usize) {
        match &mut self.0 {
            Repr::Inline { len, .. } => *len = (*len).min(n as u8),
            Repr::Heap(heap) => heap.truncate(n),
        }
        self.compact();
    }

    /// After a bulk removal: moves small heap arrays inline and gives back mostly unused memory.
    fn compact(&mut self) {
        if let Repr::Heap(heap) = &mut self.0 {
            if heap.len() <= INLINE_CAPACITY {
                *self = ArrayStore::inline_from(heap.as_slice());
            } else if heap.len() < heap.capacity() / 4 {
                heap.shrink_to_fit();
            }
        }
    }

    /// Position range of the values inside `start..=end`.
    #[inline]
    fn positions(&self, start: u16, end: u16) -> Range<usize> {
        let slice = self.as_slice();
        let lo = slice.partition_point(|&v| v < start);
        let hi = lo + slice[lo..].partition_point(|&v| v <= end);
        lo..hi
    }

    pub(crate) fn insert(&mut self, index: u16) -> bool {
        match self.as_slice().binary_search(&index) {
            Ok(_) => false,
            Err(pos) => {
                self.insert_at(pos, index);
                true
            }
        }
    }

    /// Inserts every value of the range. The caller is responsible for converting the
    /// container if the cardinality exceeds `ARRAY_LIMIT`.
    pub(crate) fn insert_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        let (start, end) = (*range.start(), *range.end());
        if start > end {
            return 0;
        }
        let positions = self.positions(start, end);
        let count = (end - start) as usize + 1;
        let present = positions.len();
        if present == count {
            return 0;
        }
        let old_len = self.as_slice().len();
        let new_len = old_len - present + count;
        if new_len <= INLINE_CAPACITY {
            let slice = self.as_slice();
            let mut data = [0; INLINE_CAPACITY];
            data[..positions.start].copy_from_slice(&slice[..positions.start]);
            for (slot, value) in data[positions.start..].iter_mut().zip(start..=end) {
                *slot = value;
            }
            data[positions.start + count..new_len].copy_from_slice(&slice[positions.end..]);
            self.0 = Repr::Inline {
                len: new_len as u8,
                data,
            };
        } else {
            match &mut self.0 {
                Repr::Heap(heap) => heap.splice_iter(positions, start..=end),
                Repr::Inline { len, data } => {
                    let old = &data[..*len as usize];
                    let mut heap = HeapVec::with_capacity(new_len.max(FIRST_HEAP_CAPACITY));
                    heap.extend_from_slice(&old[..positions.start]);
                    heap.splice_iter(positions.start..positions.start, start..=end);
                    heap.extend_from_slice(&old[positions.end..]);
                    self.0 = Repr::Heap(heap);
                }
            }
        }
        (count - present) as u64
    }

    pub(crate) fn push(&mut self, index: u16) -> bool {
        if self.max().is_some_and(|max| max >= index) {
            return false;
        }
        self.push_back(index);
        true
    }

    pub(crate) fn push_unchecked(&mut self, index: u16) {
        debug_assert!(
            self.max().is_none_or(|max| max < index),
            "push_unchecked: index is not greater than the maximum"
        );
        self.push_back(index);
    }

    #[inline]
    fn push_back(&mut self, index: u16) {
        match &mut self.0 {
            Repr::Heap(heap) => heap.push(index),
            Repr::Inline { len, .. } => {
                let n = *len as usize;
                self.insert_at(n, index);
            }
        }
    }

    pub(crate) fn remove(&mut self, index: u16) -> bool {
        let Ok(pos) = self.as_slice().binary_search(&index) else {
            return false;
        };
        match &mut self.0 {
            Repr::Inline { len, data } => {
                let n = *len as usize;
                data.copy_within(pos + 1..n, pos);
                *len -= 1;
            }
            Repr::Heap(heap) => {
                heap.remove(pos);
                if heap.len() <= POINT_REMOVAL_INLINE_LEN {
                    *self = ArrayStore::inline_from(heap.as_slice());
                }
            }
        }
        true
    }

    pub(crate) fn remove_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        let (start, end) = (*range.start(), *range.end());
        if start > end {
            return 0;
        }
        let positions = self.positions(start, end);
        let removed = positions.len() as u64;
        self.remove_positions(positions);
        removed
    }

    /// Removes the `n` smallest values (all of them if `n >= len`).
    pub(crate) fn remove_smallest(&mut self, n: u64) {
        let n = (n.min(self.len())) as usize;
        self.remove_positions(0..n);
    }

    /// Removes the `n` biggest values (all of them if `n >= len`).
    pub(crate) fn remove_biggest(&mut self, n: u64) {
        let len = self.as_slice().len();
        let n = (n.min(len as u64)) as usize;
        self.truncate(len - n);
    }

    /// Keeps only the values for which `f` returns `true`.
    pub(crate) fn retain(&mut self, mut f: impl FnMut(u16) -> bool) {
        let slice = self.as_mut_slice();
        let mut kept = 0;
        // Branchless compaction: every value is written, the cursor only advances when kept.
        for i in 0..slice.len() {
            let value = slice[i];
            slice[kept] = value;
            kept += f(value) as usize;
        }
        self.truncate(kept);
    }

    #[inline]
    pub(crate) fn contains(&self, index: u16) -> bool {
        match &self.0 {
            Repr::Inline { len, data } => data[..*len as usize].contains(&index),
            Repr::Heap(heap) => heap.as_slice().binary_search(&index).is_ok(),
        }
    }

    pub(crate) fn contains_range(&self, range: RangeInclusive<u16>) -> bool {
        let (start, end) = (*range.start(), *range.end());
        if start > end {
            return true;
        }
        let slice = self.as_slice();
        let count = (end - start) as usize + 1;
        if slice.len() < count {
            return false;
        }
        let Ok(pos) = slice.binary_search(&start) else {
            return false;
        };
        // Strictly increasing: `count` values from `start` reach `end` only if all are present.
        slice.get(pos + count - 1) == Some(&end)
    }

    pub(crate) fn min(&self) -> Option<u16> {
        self.as_slice().first().copied()
    }

    pub(crate) fn max(&self) -> Option<u16> {
        self.as_slice().last().copied()
    }

    /// Number of values `<= index`.
    pub(crate) fn rank(&self, index: u16) -> u64 {
        self.as_slice().partition_point(|&v| v <= index) as u64
    }

    pub(crate) fn select(&self, n: u16) -> Option<u16> {
        self.as_slice().get(n as usize).copied()
    }

    /// Number of values inside the range.
    pub(crate) fn range_cardinality(&self, range: RangeInclusive<u16>) -> u64 {
        let (start, end) = (*range.start(), *range.end());
        if start > end {
            return 0;
        }
        self.positions(start, end).len() as u64
    }

    pub(crate) fn to_bitmap_store(&self) -> BitmapStore {
        BitmapStore::from_sorted_unchecked(self.as_slice())
    }

    /// Number of runs of consecutive values.
    pub(crate) fn count_runs(&self) -> u64 {
        let slice = self.as_slice();
        if slice.is_empty() {
            return 0;
        }
        // A run starts at every value that does not follow its predecessor. Written without
        // early exits so that it vectorizes.
        let breaks: u64 = slice[1..]
            .iter()
            .zip(slice)
            .map(|(&v, &prev)| (v != prev.wrapping_add(1)) as u64)
            .sum();
        breaks + 1
    }

    /// Serialized payload size (`2 * len`).
    pub(crate) fn byte_size(&self) -> usize {
        self.as_slice().len() * 2
    }

    /// Bytes used by the store: its inline slot plus any heap allocation.
    pub(crate) fn mem_size(&self) -> usize {
        size_of::<ArrayStore>()
            + match &self.0 {
                Repr::Inline { .. } => 0,
                Repr::Heap(heap) => heap.heap_bytes(),
            }
    }

    pub(crate) fn internal_validate(&self) -> Result<(), &'static str> {
        if let Repr::Inline { len, .. } = &self.0
            && *len as usize > INLINE_CAPACITY
        {
            return Err("inline array length exceeds its capacity");
        }
        let slice = self.as_slice();
        if slice.is_empty() {
            return Err("zero cardinality in array container");
        }
        if slice.len() as u64 > ARRAY_LIMIT {
            return Err("array cardinality exceeds ARRAY_LIMIT");
        }
        if !is_strictly_increasing(slice) {
            return Err("array elements not strictly increasing");
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Array-array set operations (vectorized kernels). The caller handles results that could
    // exceed `ARRAY_LIMIT` (see `Container`).
    // -----------------------------------------------------------------------------------------

    /// `self | other`, through the merge kernel. The result may exceed `ARRAY_LIMIT`.
    pub(crate) fn union(&self, other: &ArrayStore) -> ArrayStore {
        let (a, b) = (self.as_slice(), other.as_slice());
        ArrayStore::build(a.len() + b.len(), |out| simd::array_union(a, b, out))
    }

    pub(crate) fn intersection(&self, other: &ArrayStore) -> ArrayStore {
        let (a, b) = (self.as_slice(), other.as_slice());
        ArrayStore::build(a.len().min(b.len()), |out| simd::array_intersect(a, b, out))
    }

    pub(crate) fn difference(&self, other: &ArrayStore) -> ArrayStore {
        let (a, b) = (self.as_slice(), other.as_slice());
        ArrayStore::build(a.len(), |out| simd::array_difference(a, b, out))
    }

    /// `self ^ other`, through the merge kernel. The result may exceed `ARRAY_LIMIT`.
    pub(crate) fn symmetric_difference(&self, other: &ArrayStore) -> ArrayStore {
        let (a, b) = (self.as_slice(), other.as_slice());
        ArrayStore::build(a.len() + b.len(), |out| simd::array_xor(a, b, out))
    }

    pub(crate) fn intersection_len(&self, other: &ArrayStore) -> u64 {
        simd::array_intersect_card(self.as_slice(), other.as_slice()) as u64
    }

    pub(crate) fn is_disjoint(&self, other: &ArrayStore) -> bool {
        !simd::array_intersects(self.as_slice(), other.as_slice())
    }

    pub(crate) fn is_subset(&self, other: &ArrayStore) -> bool {
        self.len() <= other.len() && self.intersection_len(other) == self.len()
    }

    /// Builds a store of exactly `values` stored on the heap, whatever their number.
    #[cfg(test)]
    pub(crate) fn heap_from_slice(values: &[u16]) -> ArrayStore {
        ArrayStore(Repr::Heap(HeapVec::from_slice(values)))
    }
}

/// Whether `values` is strictly increasing. Each chunk of pairs is checked without early exit
/// so that it vectorizes.
fn is_strictly_increasing(values: &[u16]) -> bool {
    if values.len() < 2 {
        return true;
    }
    values[1..]
        .chunks(256)
        .zip(values.chunks(256))
        .all(|(next, prev)| next.iter().zip(prev).fold(true, |ok, (&n, &p)| ok & (p < n)))
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
