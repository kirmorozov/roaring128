//! Roaring containers: sets of `u16` indices stored as a sorted array, a 65536-bit bitmap or a
//! list of runs.
//!
//! A [`Container`] is exactly 16 bytes (checked at compile time) so that the B-tree index of the
//! 128-bit bitmap stays dense. Small arrays (up to [`array::INLINE_CAPACITY`] values) are stored
//! inline without any heap allocation, which matters for sparse 128-bit data such as random UUIDs
//! where almost every value lives in its own container.
//!
//! Invariants maintained by every public (crate) operation of [`Container`]:
//!
//! * An `Array` container holds at most [`ARRAY_LIMIT`] values, a `Bitmap` container holds more
//!   than [`ARRAY_LIMIT`] values. Run containers may hold any cardinality (the representation is
//!   only chosen by [`Container::optimize`] or by range insertions).
//! * Containers returned by operations may be empty; the caller (the top-level bitmap) removes
//!   empty containers from its index.
//!
//! The conversion rules intentionally match the Roaring specification and the `roaring` crate so
//! that cardinalities, `optimize` decisions and serialized sizes agree with other
//! implementations.
//!
//! Operations whose operands are all arrays or bitmaps are implemented here with the vectorized
//! kernels of [`crate::simd`] (following CRoaring's strategies: array unions that may overflow
//! go through a bitmap, bitmap results that turn out small are extracted to arrays after
//! computing their cardinality first). Every operation with a run operand is delegated to the
//! functions of [`run`].

use core::mem::size_of;
use core::ops::RangeInclusive;

use alloc::vec::Vec;

use crate::simd::BitOp;

pub(crate) mod array;
pub(crate) mod bitmap;
pub(crate) mod heap;
pub(crate) mod iter;
pub(crate) mod run;

#[cfg(test)]
mod tests;

pub(crate) use self::array::ArrayStore;
pub(crate) use self::bitmap::BitmapStore;
pub(crate) use self::iter::{IntoIter, Iter};
#[allow(unused_imports)] // `Interval` is part of the container API used by the top level.
pub(crate) use self::run::{Interval, RunStore};

/// Maximum cardinality of an array container.
pub(crate) const ARRAY_LIMIT: u64 = 4096;

/// Serialized size of a bitmap container's payload in bytes.
pub(crate) const BITMAP_BYTES: usize = 8192;

/// Inputs of `from_lsb0_bytes` up to this many bytes (at most 512 values) are decoded directly;
/// larger ones are copied into a bitmap and counted / extracted with the vectorized kernels.
const LSB0_DIRECT_BYTES: usize = 64;

/// One Roaring container. See the module documentation for the invariants.
#[derive(Clone)]
pub(crate) enum Container {
    Array(ArrayStore),
    Bitmap(BitmapStore),
    Run(RunStore),
}

const _: () = assert!(core::mem::size_of::<Container>() == 16);

/// The kind of a container, as used by serialization and statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Array,
    Bitmap,
    Run,
}

impl Default for Container {
    fn default() -> Self {
        Container::new()
    }
}

/// Array or bitmap holding the values of `bitmap`, according to [`ARRAY_LIMIT`].
#[inline]
fn from_bitmap(bitmap: BitmapStore) -> Container {
    if bitmap.len() <= ARRAY_LIMIT {
        Container::Array(bitmap.to_array_store())
    } else {
        Container::Bitmap(bitmap)
    }
}

/// `a | b` for two arrays. Like CRoaring, a union that may exceed [`ARRAY_LIMIT`] is computed by
/// setting bits in a bitmap, which is converted back if the result is small.
fn array_union(a: &ArrayStore, b: &ArrayStore) -> Container {
    if a.len() + b.len() <= ARRAY_LIMIT {
        return Container::Array(a.union(b));
    }
    let mut bitmap = BitmapStore::from_sorted_unchecked(a.as_slice());
    bitmap.or_array(b.as_slice());
    from_bitmap(bitmap)
}

/// `a ^ b` for two arrays, through a bitmap when the result may exceed [`ARRAY_LIMIT`].
fn array_symmetric_difference(a: &ArrayStore, b: &ArrayStore) -> Container {
    if a.len() + b.len() <= ARRAY_LIMIT {
        return Container::Array(a.symmetric_difference(b));
    }
    let mut bitmap = BitmapStore::from_sorted_unchecked(a.as_slice());
    bitmap.xor_array(b.as_slice());
    from_bitmap(bitmap)
}

/// `a op b` for two bitmaps (`op` is not `Or`): the cardinality is computed first so that a
/// small result is extracted to an array without allocating a bitmap.
fn bitmap_op(op: BitOp, a: &BitmapStore, b: &BitmapStore) -> Container {
    let card = crate::simd::bitmap_op_card(op, a.as_words(), b.as_words()) as u64;
    if card > ARRAY_LIMIT {
        Container::Bitmap(BitmapStore::op(op, a, b))
    } else {
        Container::Array(BitmapStore::op_to_array(op, a, b, card))
    }
}

/// `bitmap - array`.
fn bitmap_difference_array(bitmap: &BitmapStore, array: &ArrayStore) -> Container {
    let values = array.as_slice();
    let card = bitmap.len() - bitmap.intersection_len_array(values);
    if card > ARRAY_LIMIT {
        let mut result = bitmap.clone();
        result.andnot_array(values);
        Container::Bitmap(result)
    } else {
        Container::Array(bitmap.array_op_to_array(values, false, card))
    }
}

/// `bitmap ^ array`.
fn bitmap_symmetric_difference_array(bitmap: &BitmapStore, array: &ArrayStore) -> Container {
    let values = array.as_slice();
    let common = bitmap.intersection_len_array(values);
    let card = bitmap.len() + values.len() as u64 - 2 * common;
    if card > ARRAY_LIMIT {
        let mut result = bitmap.clone();
        result.xor_array(values);
        Container::Bitmap(result)
    } else {
        Container::Array(bitmap.array_op_to_array(values, true, card))
    }
}

#[allow(dead_code)] // Parts of the API are only used by some top-level operations.
impl Container {
    // -----------------------------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------------------------

    /// An empty (array) container.
    pub(crate) fn new() -> Container {
        Container::Array(ArrayStore::new())
    }

    /// A container holding exactly `index`.
    pub(crate) fn with_single(index: u16) -> Container {
        Container::Array(ArrayStore::with_single(index))
    }

    /// A container holding exactly the values of `range` (non empty). Ranges of up to two values
    /// become arrays, longer ranges become a single run (same rule as the `roaring` crate).
    pub(crate) fn from_range(range: RangeInclusive<u16>) -> Container {
        let (start, end) = (*range.start(), *range.end());
        if start > end {
            return Container::new();
        }
        if end - start < 2 {
            let mut array = ArrayStore::new();
            array.insert_range(range);
            Container::Array(array)
        } else {
            Container::Run(RunStore::from_range(range))
        }
    }

    /// The container holding all 65536 values (a single run).
    pub(crate) fn full() -> Container {
        Container::Run(RunStore::full())
    }

    /// Builds a container from strictly increasing values, choosing an array or a bitmap
    /// according to [`ARRAY_LIMIT`].
    pub(crate) fn from_sorted_unchecked(values: &[u16]) -> Container {
        if values.len() as u64 <= ARRAY_LIMIT {
            Container::Array(ArrayStore::from_sorted_unchecked(values))
        } else {
            Container::Bitmap(BitmapStore::from_sorted_unchecked(values))
        }
    }

    /// Builds a container from little-endian LSB-0 bits, where `bytes[0]` describes the indices
    /// `byte_offset * 8 .. byte_offset * 8 + 8`. Returns `None` if no bit is set.
    /// Panics if `byte_offset + bytes.len() > 8192`.
    pub(crate) fn from_lsb0_bytes(bytes: &[u8], byte_offset: usize) -> Option<Container> {
        assert!(
            byte_offset
                .checked_add(bytes.len())
                .is_some_and(|end| end <= BITMAP_BYTES),
            "byte_offset + bytes.len() must be at most 8192"
        );
        if bytes.len() > LSB0_DIRECT_BYTES {
            let bitmap = BitmapStore::from_lsb0_bytes(bytes, byte_offset);
            return (!bitmap.is_empty()).then(|| from_bitmap(bitmap));
        }
        // At most 512 bits: always an array, decoded directly.
        let count: u32 = bytes.iter().map(|b| b.count_ones()).sum();
        if count == 0 {
            return None;
        }
        let array = ArrayStore::build(count as usize, |out| {
            let mut k = 0;
            for (i, &byte) in bytes.iter().enumerate() {
                let base = ((byte_offset + i) * 8) as u16;
                let mut byte = byte;
                while byte != 0 {
                    out[k].write(base + byte.trailing_zeros() as u16);
                    k += 1;
                    byte &= byte - 1;
                }
            }
            k
        });
        Some(Container::Array(array))
    }

    // -----------------------------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------------------------

    /// Cardinality.
    pub(crate) fn len(&self) -> u64 {
        match self {
            Container::Array(a) => a.len(),
            Container::Bitmap(b) => b.len(),
            Container::Run(r) => r.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Container::Array(a) => a.is_empty(),
            Container::Bitmap(b) => b.is_empty(),
            Container::Run(r) => r.is_empty(),
        }
    }

    /// Whether all 65536 values are present, in `O(1)`: a full run container is the single
    /// interval `0..=65535` (runs are sorted, disjoint and non adjacent) and arrays never
    /// exceed `ARRAY_LIMIT` values.
    #[inline]
    pub(crate) fn is_full(&self) -> bool {
        match self {
            Container::Array(_) => false,
            Container::Bitmap(b) => b.len() == 1 << 16,
            Container::Run(r) => {
                matches!(r.intervals(), [iv] if iv.start() == 0 && iv.end() == u16::MAX)
            }
        }
    }

    pub(crate) fn kind(&self) -> Kind {
        match self {
            Container::Array(_) => Kind::Array,
            Container::Bitmap(_) => Kind::Bitmap,
            Container::Run(_) => Kind::Run,
        }
    }

    #[inline]
    fn is_run(&self) -> bool {
        matches!(self, Container::Run(_))
    }

    #[inline]
    pub(crate) fn contains(&self, index: u16) -> bool {
        match self {
            Container::Array(a) => a.contains(index),
            Container::Bitmap(b) => b.contains(index),
            Container::Run(r) => r.contains(index),
        }
    }

    /// Whether every value of the (non empty) range is present.
    pub(crate) fn contains_range(&self, range: RangeInclusive<u16>) -> bool {
        match self {
            Container::Array(a) => a.contains_range(range),
            Container::Bitmap(b) => b.contains_range(range),
            Container::Run(r) => r.contains_range(range),
        }
    }

    pub(crate) fn min(&self) -> Option<u16> {
        match self {
            Container::Array(a) => a.min(),
            Container::Bitmap(b) => b.min(),
            Container::Run(r) => r.min(),
        }
    }

    pub(crate) fn max(&self) -> Option<u16> {
        match self {
            Container::Array(a) => a.max(),
            Container::Bitmap(b) => b.max(),
            Container::Run(r) => r.max(),
        }
    }

    /// Number of values `<= index`.
    pub(crate) fn rank(&self, index: u16) -> u64 {
        match self {
            Container::Array(a) => a.rank(index),
            Container::Bitmap(b) => b.rank(index),
            Container::Run(r) => r.rank(index),
        }
    }

    /// The `n`-th smallest value (0 based).
    pub(crate) fn select(&self, n: u16) -> Option<u16> {
        match self {
            Container::Array(a) => a.select(n),
            Container::Bitmap(b) => b.select(n),
            Container::Run(r) => r.select(n),
        }
    }

    /// Number of values inside the (non empty) range.
    pub(crate) fn range_cardinality(&self, range: RangeInclusive<u16>) -> u64 {
        match self {
            Container::Array(a) => a.range_cardinality(range),
            Container::Bitmap(b) => b.range_cardinality(range),
            Container::Run(r) => r.range_cardinality(range),
        }
    }

    pub(crate) fn is_disjoint(&self, other: &Container) -> bool {
        match (self, other) {
            (Container::Run(_), _) | (_, Container::Run(_)) => run::is_disjoint(self, other),
            (Container::Array(a), Container::Array(b)) => a.is_disjoint(b),
            (Container::Bitmap(a), Container::Bitmap(b)) => a.is_disjoint(b),
            (Container::Array(a), Container::Bitmap(b))
            | (Container::Bitmap(b), Container::Array(a)) => {
                !a.as_slice().iter().any(|&v| b.contains(v))
            }
        }
    }

    /// Whether `self ⊆ other`.
    pub(crate) fn is_subset(&self, other: &Container) -> bool {
        if self.len() > other.len() {
            return false;
        }
        match (self, other) {
            (Container::Run(_), _) | (_, Container::Run(_)) => run::is_subset(self, other),
            (Container::Array(a), Container::Array(b)) => a.is_subset(b),
            (Container::Bitmap(a), Container::Bitmap(b)) => a.is_subset(b),
            (Container::Array(a), Container::Bitmap(b)) => {
                a.as_slice().iter().all(|&v| b.contains(v))
            }
            // Only reachable if the array/bitmap invariant is broken; stay correct anyway.
            (Container::Bitmap(_), Container::Array(_)) => {
                self.intersection_len(other) == self.len()
            }
        }
    }

    /// Cardinality of `self & other` without materializing it.
    pub(crate) fn intersection_len(&self, other: &Container) -> u64 {
        match (self, other) {
            (Container::Run(_), _) | (_, Container::Run(_)) => run::intersection_len(self, other),
            (Container::Array(a), Container::Array(b)) => a.intersection_len(b),
            (Container::Bitmap(a), Container::Bitmap(b)) => a.intersection_len(b),
            (Container::Array(a), Container::Bitmap(b))
            | (Container::Bitmap(b), Container::Array(a)) => b.intersection_len_array(a.as_slice()),
        }
    }

    /// Semantic equality (the same set of values, whatever the representation).
    pub(crate) fn eq_values(&self, other: &Container) -> bool {
        match (self, other) {
            (Container::Run(_), _) | (_, Container::Run(_)) => run::eq_values(self, other),
            (Container::Array(a), Container::Array(b)) => a.as_slice() == b.as_slice(),
            (Container::Bitmap(a), Container::Bitmap(b)) => {
                a.len() == b.len() && a.as_words() == b.as_words()
            }
            (Container::Array(a), Container::Bitmap(b))
            | (Container::Bitmap(b), Container::Array(a)) => {
                // Different cardinalities under the invariants; checked for robustness.
                a.len() == b.len() && b.intersection_len_array(a.as_slice()) == a.len()
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Point / range mutation (all keep the store invariants)
    // -----------------------------------------------------------------------------------------

    /// Returns `true` if the value was not already present.
    #[inline]
    pub(crate) fn insert(&mut self, index: u16) -> bool {
        match self {
            Container::Array(a) => {
                if a.len() < ARRAY_LIMIT {
                    return a.insert(index);
                }
                if a.contains(index) {
                    return false;
                }
                // Convert before growing the array beyond the limit.
                let mut bitmap = a.to_bitmap_store();
                bitmap.insert(index);
                *self = Container::Bitmap(bitmap);
                true
            }
            Container::Bitmap(b) => b.insert(index),
            Container::Run(r) => r.insert(index),
        }
    }

    /// Inserts a (non empty) range, returns the number of newly inserted values. A container
    /// that becomes full is stored as a single run.
    pub(crate) fn insert_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        let (start, end) = (*range.start(), *range.end());
        if start > end {
            return 0;
        }
        let count = (end - start) as u64 + 1;
        match self {
            Container::Array(a) => {
                let added = count - a.range_cardinality(range.clone());
                let union_len = a.len() + added;
                if union_len == 1 << 16 {
                    *self = Container::full();
                } else if union_len <= ARRAY_LIMIT {
                    a.insert_range(range);
                } else {
                    let mut bitmap = a.to_bitmap_store();
                    bitmap.insert_range(range);
                    *self = Container::Bitmap(bitmap);
                }
                added
            }
            Container::Bitmap(b) => {
                let added = count - b.range_cardinality(range.clone());
                if b.len() + added == 1 << 16 {
                    *self = Container::full();
                } else {
                    b.insert_range(range);
                }
                added
            }
            Container::Run(r) => r.insert_range(range),
        }
    }

    /// Appends `index` if it is greater than the current maximum (or the container is empty).
    /// Returns whether it was appended.
    pub(crate) fn push(&mut self, index: u16) -> bool {
        match self {
            Container::Array(a) => {
                if a.max().is_some_and(|max| max >= index) {
                    return false;
                }
                self.push_unchecked(index);
                true
            }
            Container::Bitmap(b) => b.push(index),
            Container::Run(r) => r.push(index),
        }
    }

    /// Appends `index`, which the caller guarantees to be greater than the current maximum.
    pub(crate) fn push_unchecked(&mut self, index: u16) {
        match self {
            Container::Array(a) => {
                if a.len() < ARRAY_LIMIT {
                    a.push_unchecked(index);
                } else {
                    let mut bitmap = a.to_bitmap_store();
                    bitmap.push_unchecked(index);
                    *self = Container::Bitmap(bitmap);
                }
            }
            Container::Bitmap(b) => b.push_unchecked(index),
            Container::Run(r) => {
                debug_assert!(r.max().is_none_or(|max| max < index));
                r.push(index);
            }
        }
    }

    /// Returns `true` if the value was present.
    #[inline]
    pub(crate) fn remove(&mut self, index: u16) -> bool {
        match self {
            Container::Array(a) => a.remove(index),
            Container::Bitmap(b) => {
                if !b.remove(index) {
                    return false;
                }
                if b.len() <= ARRAY_LIMIT {
                    *self = Container::Array(b.to_array_store());
                }
                true
            }
            Container::Run(r) => r.remove(index),
        }
    }

    /// Removes a (non empty) range, returns the number of removed values.
    pub(crate) fn remove_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        match self {
            Container::Array(a) => a.remove_range(range),
            Container::Bitmap(b) => {
                let removed = b.remove_range(range);
                if b.len() <= ARRAY_LIMIT {
                    *self = Container::Array(b.to_array_store());
                }
                removed
            }
            Container::Run(r) => r.remove_range(range),
        }
    }

    /// Removes the `n` smallest values; `n` must be smaller than `len()`.
    pub(crate) fn remove_smallest(&mut self, n: u64) {
        match self {
            Container::Array(a) => a.remove_smallest(n),
            Container::Bitmap(b) => {
                b.remove_smallest(n);
                if b.len() <= ARRAY_LIMIT {
                    *self = Container::Array(b.to_array_store());
                }
            }
            Container::Run(r) => r.remove_smallest(n),
        }
    }

    /// Removes the `n` biggest values; `n` must be smaller than `len()`.
    pub(crate) fn remove_biggest(&mut self, n: u64) {
        match self {
            Container::Array(a) => a.remove_biggest(n),
            Container::Bitmap(b) => {
                b.remove_biggest(n);
                if b.len() <= ARRAY_LIMIT {
                    *self = Container::Array(b.to_array_store());
                }
            }
            Container::Run(r) => r.remove_biggest(n),
        }
    }

    /// Converts arrays above [`ARRAY_LIMIT`] to bitmaps and bitmaps at or below it to arrays.
    /// Returns `true` if the representation changed.
    pub(crate) fn ensure_correct_store(&mut self) -> bool {
        match self {
            Container::Array(a) if a.len() > ARRAY_LIMIT => {
                *self = Container::Bitmap(a.to_bitmap_store());
                true
            }
            Container::Bitmap(b) if b.len() <= ARRAY_LIMIT => {
                *self = Container::Array(b.to_array_store());
                true
            }
            _ => false,
        }
    }

    /// Chooses between run and non-run representation by serialized size, exactly like the
    /// `roaring` crate. Returns `true` if the representation changed.
    pub(crate) fn optimize(&mut self) -> bool {
        match self {
            Container::Array(a) => {
                let size_as_array = a.byte_size();
                let size_as_run = run::serialized_byte_size(a.count_runs());
                if size_as_array <= size_as_run {
                    return false;
                }
                *self = Container::Run(RunStore::from_array(a));
                true
            }
            Container::Bitmap(b) => {
                let size_as_run = run::serialized_byte_size(b.count_runs());
                if BITMAP_BYTES <= size_as_run {
                    return false;
                }
                *self = Container::Run(RunStore::from_bitmap(b));
                true
            }
            Container::Run(r) => {
                let size_as_run = r.byte_size();
                let card = r.len();
                let size_as_array = card as usize * 2;
                if size_as_run <= size_as_array.min(BITMAP_BYTES) {
                    return false;
                }
                *self = if card <= ARRAY_LIMIT {
                    Container::Array(r.to_array())
                } else {
                    Container::Bitmap(r.to_bitmap())
                };
                true
            }
        }
    }

    /// Converts a run container to an array or a bitmap. Returns `true` if it was a run.
    pub(crate) fn remove_run_compression(&mut self) -> bool {
        match self {
            Container::Run(r) => {
                *self = if r.len() <= ARRAY_LIMIT {
                    Container::Array(r.to_array())
                } else {
                    Container::Bitmap(r.to_bitmap())
                };
                true
            }
            _ => false,
        }
    }

    // -----------------------------------------------------------------------------------------
    // Set operations. Results satisfy the store invariants and may be empty.
    // -----------------------------------------------------------------------------------------

    pub(crate) fn union(&self, other: &Container) -> Container {
        match (self, other) {
            (Container::Run(_), _) | (_, Container::Run(_)) => run::union(self, other),
            (Container::Array(a), Container::Array(b)) => array_union(a, b),
            (Container::Bitmap(a), Container::Bitmap(b)) => {
                Container::Bitmap(BitmapStore::op(BitOp::Or, a, b))
            }
            (Container::Array(a), Container::Bitmap(b))
            | (Container::Bitmap(b), Container::Array(a)) => {
                let mut result = b.clone();
                result.or_array(a.as_slice());
                Container::Bitmap(result)
            }
        }
    }

    pub(crate) fn intersection(&self, other: &Container) -> Container {
        match (self, other) {
            (Container::Run(_), _) | (_, Container::Run(_)) => run::intersection(self, other),
            (Container::Array(a), Container::Array(b)) => Container::Array(a.intersection(b)),
            (Container::Bitmap(a), Container::Bitmap(b)) => bitmap_op(BitOp::And, a, b),
            (Container::Array(a), Container::Bitmap(b))
            | (Container::Bitmap(b), Container::Array(a)) => {
                Container::Array(b.filter_array(a.as_slice(), true))
            }
        }
    }

    pub(crate) fn difference(&self, other: &Container) -> Container {
        match (self, other) {
            (Container::Run(_), _) | (_, Container::Run(_)) => run::difference(self, other),
            (Container::Array(a), Container::Array(b)) => Container::Array(a.difference(b)),
            (Container::Bitmap(a), Container::Bitmap(b)) => bitmap_op(BitOp::AndNot, a, b),
            (Container::Array(a), Container::Bitmap(b)) => {
                Container::Array(b.filter_array(a.as_slice(), false))
            }
            (Container::Bitmap(a), Container::Array(b)) => bitmap_difference_array(a, b),
        }
    }

    pub(crate) fn symmetric_difference(&self, other: &Container) -> Container {
        match (self, other) {
            (Container::Run(_), _) | (_, Container::Run(_)) => {
                run::symmetric_difference(self, other)
            }
            (Container::Array(a), Container::Array(b)) => array_symmetric_difference(a, b),
            (Container::Bitmap(a), Container::Bitmap(b)) => bitmap_op(BitOp::Xor, a, b),
            (Container::Array(a), Container::Bitmap(b))
            | (Container::Bitmap(b), Container::Array(a)) => {
                bitmap_symmetric_difference_array(b, a)
            }
        }
    }

    pub(crate) fn union_with(&mut self, other: &Container) {
        if self.is_run() || other.is_run() {
            *self = run::union(self, other);
            return;
        }
        match (&mut *self, other) {
            (Container::Array(a), Container::Array(b)) => *self = array_union(a, b),
            (Container::Bitmap(a), Container::Bitmap(b)) => {
                a.op_assign(BitOp::Or, b);
            }
            (Container::Bitmap(a), Container::Array(b)) => a.or_array(b.as_slice()),
            (Container::Array(a), Container::Bitmap(b)) => {
                let mut result = b.clone();
                result.or_array(a.as_slice());
                *self = Container::Bitmap(result);
            }
            (Container::Run(_), _) | (_, Container::Run(_)) => unreachable!(),
        }
    }

    /// Like [`Container::union_with`] but may reuse `other`'s allocation.
    pub(crate) fn union_with_owned(&mut self, other: Container) {
        match other {
            Container::Bitmap(mut bitmap) if matches!(self, Container::Array(_)) => {
                let Container::Array(a) = &*self else {
                    unreachable!()
                };
                bitmap.or_array(a.as_slice());
                *self = Container::Bitmap(bitmap);
            }
            other => self.union_with(&other),
        }
    }

    pub(crate) fn intersect_with(&mut self, other: &Container) {
        if self.is_run() || other.is_run() {
            *self = run::intersection(self, other);
            return;
        }
        match (&mut *self, other) {
            (Container::Array(a), Container::Array(b)) => *a = a.intersection(b),
            (Container::Bitmap(a), Container::Bitmap(b)) => {
                if a.op_assign(BitOp::And, b) <= ARRAY_LIMIT {
                    *self = Container::Array(a.to_array_store());
                }
            }
            (Container::Array(a), Container::Bitmap(b)) => a.retain(|v| b.contains(v)),
            (Container::Bitmap(a), Container::Array(b)) => {
                *self = Container::Array(a.filter_array(b.as_slice(), true));
            }
            (Container::Run(_), _) | (_, Container::Run(_)) => unreachable!(),
        }
    }

    /// Like [`Container::intersect_with`] but may reuse `other`'s allocation.
    pub(crate) fn intersect_with_owned(&mut self, other: Container) {
        match other {
            Container::Array(mut array) if matches!(self, Container::Bitmap(_)) => {
                let Container::Bitmap(b) = &*self else {
                    unreachable!()
                };
                array.retain(|v| b.contains(v));
                *self = Container::Array(array);
            }
            other => self.intersect_with(&other),
        }
    }

    pub(crate) fn difference_with(&mut self, other: &Container) {
        if self.is_run() || other.is_run() {
            *self = run::difference(self, other);
            return;
        }
        match (&mut *self, other) {
            (Container::Array(a), Container::Array(b)) => *a = a.difference(b),
            (Container::Bitmap(a), Container::Bitmap(b)) => {
                if a.op_assign(BitOp::AndNot, b) <= ARRAY_LIMIT {
                    *self = Container::Array(a.to_array_store());
                }
            }
            (Container::Array(a), Container::Bitmap(b)) => a.retain(|v| !b.contains(v)),
            (Container::Bitmap(a), Container::Array(b)) => {
                a.andnot_array(b.as_slice());
                if a.len() <= ARRAY_LIMIT {
                    *self = Container::Array(a.to_array_store());
                }
            }
            (Container::Run(_), _) | (_, Container::Run(_)) => unreachable!(),
        }
    }

    pub(crate) fn symmetric_difference_with(&mut self, other: &Container) {
        if self.is_run() || other.is_run() {
            *self = run::symmetric_difference(self, other);
            return;
        }
        match (&mut *self, other) {
            (Container::Array(a), Container::Array(b)) => {
                *self = array_symmetric_difference(a, b);
            }
            (Container::Bitmap(a), Container::Bitmap(b)) => {
                if a.op_assign(BitOp::Xor, b) <= ARRAY_LIMIT {
                    *self = Container::Array(a.to_array_store());
                }
            }
            (Container::Bitmap(a), Container::Array(b)) => {
                a.xor_array(b.as_slice());
                if a.len() <= ARRAY_LIMIT {
                    *self = Container::Array(a.to_array_store());
                }
            }
            (Container::Array(a), Container::Bitmap(b)) => {
                *self = bitmap_symmetric_difference_array(b, a);
            }
            (Container::Run(_), _) | (_, Container::Run(_)) => unreachable!(),
        }
    }

    /// Like [`Container::symmetric_difference_with`] but may reuse `other`'s allocation.
    pub(crate) fn symmetric_difference_with_owned(&mut self, other: Container) {
        match other {
            Container::Bitmap(mut bitmap) if matches!(self, Container::Array(_)) => {
                let Container::Array(a) = &*self else {
                    unreachable!()
                };
                bitmap.xor_array(a.as_slice());
                *self = from_bitmap(bitmap);
            }
            other => self.symmetric_difference_with(&other),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Lazy operations for many-way unions / symmetric differences (MultiOps).
    //
    // `lazy_*_with` may leave a bitmap container with a stale cardinality and may break the
    // array/bitmap invariant; after the last lazy operation `repair_lazy` must be called, which
    // recomputes the cardinality and restores the invariants.
    //
    // Only a bitmap `self` combined with an array or bitmap operand is lazy (no popcount); every
    // other combination uses the exact operation. Since a stale bitmap is only ever combined with
    // bitmap/array words (runs are OR-ed into the words, or the bitmap is repaired before a
    // symmetric difference with a run), the final representations equal those of the exact
    // operations, as in the `roaring` crate.
    // -----------------------------------------------------------------------------------------

    pub(crate) fn lazy_union_with(&mut self, other: &Container) {
        match (&mut *self, other) {
            (Container::Bitmap(a), Container::Bitmap(b)) => a.op_assign_nocard(BitOp::Or, b),
            (Container::Bitmap(a), Container::Array(b)) => a.or_array_nocard(b.as_slice()),
            // A union only grows the bitmap: it stays a bitmap, as with the exact operation.
            (Container::Bitmap(a), Container::Run(r)) => r.or_into_words(a.as_words_mut()),
            _ => self.union_with(other),
        }
    }

    pub(crate) fn lazy_symmetric_difference_with(&mut self, other: &Container) {
        match (&mut *self, other) {
            (Container::Bitmap(a), Container::Bitmap(b)) => a.op_assign_nocard(BitOp::Xor, b),
            (Container::Bitmap(a), Container::Array(b)) => a.xor_array_nocard(b.as_slice()),
            (Container::Bitmap(_), Container::Run(_)) => {
                // The result representation depends on the exact cardinality.
                self.repair_lazy();
                self.symmetric_difference_with(other);
            }
            _ => self.symmetric_difference_with(other),
        }
    }

    pub(crate) fn repair_lazy(&mut self) {
        match self {
            Container::Bitmap(b) => {
                if b.recount() <= ARRAY_LIMIT {
                    *self = Container::Array(b.to_array_store());
                }
            }
            Container::Array(a) if a.len() > ARRAY_LIMIT => {
                *self = Container::Bitmap(a.to_bitmap_store());
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------------------------
    // Iteration
    // -----------------------------------------------------------------------------------------

    pub(crate) fn iter(&self) -> Iter<'_> {
        match self {
            Container::Array(a) => Iter::from_array(a.as_slice()),
            Container::Bitmap(b) => Iter::from_bitmap(b.as_words(), b.len()),
            Container::Run(r) => Iter::from_runs(r.intervals()),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub(crate) fn into_iter(self) -> IntoIter {
        match self {
            Container::Array(a) => IntoIter::from_array(a),
            Container::Bitmap(b) => IntoIter::from_bitmap(b),
            Container::Run(r) => IntoIter::from_runs(r),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Serialization support (see `FORMAT.md`)
    // -----------------------------------------------------------------------------------------

    /// The low 16 bits of the serialized descriptor: `cardinality - 1` for arrays and bitmaps,
    /// `number_of_runs - 1` for runs. The container must not be empty.
    pub(crate) fn descriptor(&self) -> u16 {
        let count = match self {
            Container::Array(a) => a.len(),
            Container::Bitmap(b) => b.len(),
            Container::Run(r) => r.run_amount(),
        };
        debug_assert!(count > 0, "empty containers have no descriptor");
        (count - 1) as u16
    }

    /// Size in bytes of the serialized payload: `2 * len` for arrays, 8192 for bitmaps and
    /// `4 * runs` for runs.
    pub(crate) fn serialized_payload_size(&self) -> usize {
        match self {
            Container::Array(a) => a.byte_size(),
            Container::Bitmap(_) => BITMAP_BYTES,
            Container::Run(r) => 4 * r.run_amount() as usize,
        }
    }

    /// Appends the little-endian payload (see `FORMAT.md`) to `out`.
    pub(crate) fn write_payload(&self, out: &mut Vec<u8>) {
        match self {
            Container::Array(a) => {
                let values = a.as_slice();
                if cfg!(target_endian = "little") {
                    // SAFETY: `u16` has no padding and every byte is a valid `u8`; the byte
                    // slice covers exactly the values.
                    let bytes = unsafe {
                        core::slice::from_raw_parts(values.as_ptr().cast::<u8>(), values.len() * 2)
                    };
                    out.extend_from_slice(bytes);
                } else {
                    out.reserve(values.len() * 2);
                    for v in values {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
            Container::Bitmap(b) => b.write_le_bytes(out),
            Container::Run(r) => {
                let runs = r.intervals();
                out.reserve(runs.len() * 4);
                for iv in runs {
                    out.extend_from_slice(&iv.start().to_le_bytes());
                    out.extend_from_slice(&(iv.end() - iv.start()).to_le_bytes());
                }
            }
        }
    }

    /// Heap + inline bytes used by the container (for statistics): the 16-byte slot plus the
    /// heap allocation, if any.
    pub(crate) fn mem_size(&self) -> usize {
        match self {
            Container::Array(a) => size_of::<Container>() - size_of::<ArrayStore>() + a.mem_size(),
            Container::Bitmap(b) => {
                size_of::<Container>() - size_of::<BitmapStore>() + b.mem_size()
            }
            // `RunStore::mem_size` reports the heap block only.
            Container::Run(r) => size_of::<Container>() + r.mem_size(),
        }
    }

    /// Checks the internal invariants (empty containers are invalid, like in the `roaring`
    /// crate).
    pub(crate) fn internal_validate(&self) -> Result<(), &'static str> {
        match self {
            Container::Array(a) => a.internal_validate(),
            Container::Bitmap(b) => b.internal_validate(),
            Container::Run(r) => r.internal_validate(),
        }
    }
}

impl PartialEq for Container {
    fn eq(&self, other: &Self) -> bool {
        self.eq_values(other)
    }
}

impl Eq for Container {}

impl core::fmt::Debug for Container {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Container<{:?} {} values>", self.kind(), self.len())
    }
}
