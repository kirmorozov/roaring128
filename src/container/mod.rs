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

use core::ops::RangeInclusive;

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
pub(crate) use self::run::{Interval, RunStore};

/// Maximum cardinality of an array container.
pub(crate) const ARRAY_LIMIT: u64 = 4096;

/// Serialized size of a bitmap container's payload in bytes.
pub(crate) const BITMAP_BYTES: usize = 8192;

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
        todo!("container::from_range {range:?}")
    }

    /// The container holding all 65536 values (a single run).
    pub(crate) fn full() -> Container {
        Container::Run(RunStore::full())
    }

    /// Builds a container from strictly increasing values, choosing an array or a bitmap
    /// according to [`ARRAY_LIMIT`].
    pub(crate) fn from_sorted_unchecked(values: &[u16]) -> Container {
        todo!("container::from_sorted_unchecked {}", values.len())
    }

    /// Builds a container from little-endian LSB-0 bits, where `bytes[0]` describes the indices
    /// `byte_offset * 8 .. byte_offset * 8 + 8`. Returns `None` if no bit is set.
    /// Panics if `byte_offset + bytes.len() > 8192`.
    pub(crate) fn from_lsb0_bytes(bytes: &[u8], byte_offset: usize) -> Option<Container> {
        todo!("container::from_lsb0_bytes {} {byte_offset}", bytes.len())
    }

    // -----------------------------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------------------------

    /// Cardinality.
    pub(crate) fn len(&self) -> u64 {
        todo!()
    }

    pub(crate) fn is_empty(&self) -> bool {
        todo!()
    }

    pub(crate) fn is_full(&self) -> bool {
        self.len() == 1 << 16
    }

    pub(crate) fn kind(&self) -> Kind {
        match self {
            Container::Array(_) => Kind::Array,
            Container::Bitmap(_) => Kind::Bitmap,
            Container::Run(_) => Kind::Run,
        }
    }

    pub(crate) fn contains(&self, index: u16) -> bool {
        todo!("{index}")
    }

    /// Whether every value of the (non empty) range is present.
    pub(crate) fn contains_range(&self, range: RangeInclusive<u16>) -> bool {
        todo!("{range:?}")
    }

    pub(crate) fn min(&self) -> Option<u16> {
        todo!()
    }

    pub(crate) fn max(&self) -> Option<u16> {
        todo!()
    }

    /// Number of values `<= index`.
    pub(crate) fn rank(&self, index: u16) -> u64 {
        todo!("{index}")
    }

    /// The `n`-th smallest value (0 based).
    pub(crate) fn select(&self, n: u16) -> Option<u16> {
        todo!("{n}")
    }

    /// Number of values inside the (non empty) range.
    pub(crate) fn range_cardinality(&self, range: RangeInclusive<u16>) -> u64 {
        todo!("{range:?}")
    }

    pub(crate) fn is_disjoint(&self, other: &Container) -> bool {
        todo!("{}", other.len())
    }

    /// Whether `self ⊆ other`.
    pub(crate) fn is_subset(&self, other: &Container) -> bool {
        todo!("{}", other.len())
    }

    /// Cardinality of `self & other` without materializing it.
    pub(crate) fn intersection_len(&self, other: &Container) -> u64 {
        todo!("{}", other.len())
    }

    /// Semantic equality (the same set of values, whatever the representation).
    pub(crate) fn eq_values(&self, other: &Container) -> bool {
        todo!("{}", other.len())
    }

    // -----------------------------------------------------------------------------------------
    // Point / range mutation (all keep the store invariants)
    // -----------------------------------------------------------------------------------------

    /// Returns `true` if the value was not already present.
    pub(crate) fn insert(&mut self, index: u16) -> bool {
        todo!("{index}")
    }

    /// Inserts a (non empty) range, returns the number of newly inserted values. A container
    /// that becomes full is stored as a single run.
    pub(crate) fn insert_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        todo!("{range:?}")
    }

    /// Appends `index` if it is greater than the current maximum (or the container is empty).
    /// Returns whether it was appended.
    pub(crate) fn push(&mut self, index: u16) -> bool {
        todo!("{index}")
    }

    /// Appends `index`, which the caller guarantees to be greater than the current maximum.
    pub(crate) fn push_unchecked(&mut self, index: u16) {
        todo!("{index}")
    }

    /// Returns `true` if the value was present.
    pub(crate) fn remove(&mut self, index: u16) -> bool {
        todo!("{index}")
    }

    /// Removes a (non empty) range, returns the number of removed values.
    pub(crate) fn remove_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        todo!("{range:?}")
    }

    /// Removes the `n` smallest values; `n` must be smaller than `len()`.
    pub(crate) fn remove_smallest(&mut self, n: u64) {
        todo!("{n}")
    }

    /// Removes the `n` biggest values; `n` must be smaller than `len()`.
    pub(crate) fn remove_biggest(&mut self, n: u64) {
        todo!("{n}")
    }

    /// Converts arrays above [`ARRAY_LIMIT`] to bitmaps and bitmaps at or below it to arrays.
    /// Returns `true` if the representation changed.
    pub(crate) fn ensure_correct_store(&mut self) -> bool {
        todo!()
    }

    /// Chooses between run and non-run representation by serialized size, exactly like the
    /// `roaring` crate. Returns `true` if the representation changed.
    pub(crate) fn optimize(&mut self) -> bool {
        todo!()
    }

    /// Converts a run container to an array or a bitmap. Returns `true` if it was a run.
    pub(crate) fn remove_run_compression(&mut self) -> bool {
        todo!()
    }

    // -----------------------------------------------------------------------------------------
    // Set operations. Results satisfy the store invariants and may be empty.
    // -----------------------------------------------------------------------------------------

    pub(crate) fn union(&self, other: &Container) -> Container {
        todo!("{}", other.len())
    }

    pub(crate) fn intersection(&self, other: &Container) -> Container {
        todo!("{}", other.len())
    }

    pub(crate) fn difference(&self, other: &Container) -> Container {
        todo!("{}", other.len())
    }

    pub(crate) fn symmetric_difference(&self, other: &Container) -> Container {
        todo!("{}", other.len())
    }

    pub(crate) fn union_with(&mut self, other: &Container) {
        todo!("{}", other.len())
    }

    /// Like [`Container::union_with`] but may reuse `other`'s allocation.
    pub(crate) fn union_with_owned(&mut self, other: Container) {
        todo!("{}", other.len())
    }

    pub(crate) fn intersect_with(&mut self, other: &Container) {
        todo!("{}", other.len())
    }

    /// Like [`Container::intersect_with`] but may reuse `other`'s allocation.
    pub(crate) fn intersect_with_owned(&mut self, other: Container) {
        todo!("{}", other.len())
    }

    pub(crate) fn difference_with(&mut self, other: &Container) {
        todo!("{}", other.len())
    }

    pub(crate) fn symmetric_difference_with(&mut self, other: &Container) {
        todo!("{}", other.len())
    }

    /// Like [`Container::symmetric_difference_with`] but may reuse `other`'s allocation.
    pub(crate) fn symmetric_difference_with_owned(&mut self, other: Container) {
        todo!("{}", other.len())
    }

    // -----------------------------------------------------------------------------------------
    // Lazy operations for many-way unions / symmetric differences (MultiOps).
    //
    // `lazy_*_with` may leave a bitmap container with a stale cardinality and may break the
    // array/bitmap invariant; after the last lazy operation `repair_lazy` must be called, which
    // recomputes the cardinality and restores the invariants.
    // -----------------------------------------------------------------------------------------

    pub(crate) fn lazy_union_with(&mut self, other: &Container) {
        todo!("{}", other.len())
    }

    pub(crate) fn lazy_symmetric_difference_with(&mut self, other: &Container) {
        todo!("{}", other.len())
    }

    pub(crate) fn repair_lazy(&mut self) {
        todo!()
    }

    // -----------------------------------------------------------------------------------------
    // Iteration
    // -----------------------------------------------------------------------------------------

    pub(crate) fn iter(&self) -> Iter<'_> {
        todo!()
    }

    #[allow(clippy::should_implement_trait)]
    pub(crate) fn into_iter(self) -> IntoIter {
        todo!()
    }

    // -----------------------------------------------------------------------------------------
    // Serialization support (see `FORMAT.md`)
    // -----------------------------------------------------------------------------------------

    /// The low 16 bits of the serialized descriptor: `cardinality - 1` for arrays and bitmaps,
    /// `number_of_runs - 1` for runs. The container must not be empty.
    pub(crate) fn descriptor(&self) -> u16 {
        todo!()
    }

    /// Size in bytes of the serialized payload: `2 * len` for arrays, 8192 for bitmaps and
    /// `4 * runs` for runs.
    pub(crate) fn serialized_payload_size(&self) -> usize {
        todo!()
    }

    /// Appends the little-endian payload (see `FORMAT.md`) to `out`.
    pub(crate) fn write_payload(&self, out: &mut alloc::vec::Vec<u8>) {
        todo!("{}", out.len())
    }

    /// Heap + inline bytes used by the container (for statistics).
    pub(crate) fn mem_size(&self) -> usize {
        todo!()
    }

    /// Checks the internal invariants.
    pub(crate) fn internal_validate(&self) -> Result<(), &'static str> {
        todo!()
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
