//! 65536-bit bitmap container.
//!
//! The bits and the cached cardinality live in one 64-byte aligned heap block so that AVX-512
//! loads never split cache lines and the container slot stays one pointer wide.

use core::ops::RangeInclusive;

use super::array::ArrayStore;
use crate::simd::Words;

#[repr(C, align(64))]
pub(crate) struct BitmapData {
    pub(crate) bits: Words,
    /// Cached cardinality (may be stale only between `lazy_*` operations and `repair_lazy`).
    pub(crate) len: u64,
}

/// A bitmap of 65536 bits with cached cardinality.
pub(crate) struct BitmapStore(alloc::boxed::Box<BitmapData>);

const _: () = assert!(core::mem::size_of::<BitmapStore>() == 8);

#[allow(dead_code)]
impl BitmapStore {
    /// An empty bitmap (zeroed allocation).
    pub(crate) fn new() -> BitmapStore {
        todo!()
    }

    pub(crate) fn full() -> BitmapStore {
        todo!()
    }

    /// `len` must equal the popcount of `bits`.
    pub(crate) fn from_words_unchecked(len: u64, bits: alloc::boxed::Box<Words>) -> BitmapStore {
        let _ = bits;
        todo!("{len}")
    }

    /// Validating constructor used by deserialization: `len` must match the popcount and be
    /// greater than `ARRAY_LIMIT`.
    pub(crate) fn try_from_words(
        len: u64,
        bits: alloc::boxed::Box<Words>,
    ) -> Result<BitmapStore, &'static str> {
        let _ = bits;
        todo!("{len}")
    }

    pub(crate) fn len(&self) -> u64 {
        self.0.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.len == 0
    }

    pub(crate) fn as_words(&self) -> &Words {
        &self.0.bits
    }

    /// Mutable access to the bits; the caller must keep `len` consistent (see `set_len` and
    /// `recount`).
    pub(crate) fn as_words_mut(&mut self) -> &mut Words {
        &mut self.0.bits
    }

    pub(crate) fn set_len(&mut self, len: u64) {
        self.0.len = len;
    }

    /// Recomputes the cached cardinality from the bits.
    pub(crate) fn recount(&mut self) -> u64 {
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

    pub(crate) fn contains(&self, index: u16) -> bool {
        todo!("{index}")
    }

    pub(crate) fn contains_range(&self, range: RangeInclusive<u16>) -> bool {
        todo!("{range:?}")
    }

    pub(crate) fn min(&self) -> Option<u16> {
        todo!()
    }

    pub(crate) fn max(&self) -> Option<u16> {
        todo!()
    }

    pub(crate) fn rank(&self, index: u16) -> u64 {
        todo!("{index}")
    }

    pub(crate) fn select(&self, n: u16) -> Option<u16> {
        todo!("{n}")
    }

    /// Number of set bits inside the range.
    pub(crate) fn range_cardinality(&self, range: RangeInclusive<u16>) -> u64 {
        todo!("{range:?}")
    }

    pub(crate) fn to_array_store(&self) -> ArrayStore {
        todo!()
    }

    pub(crate) fn count_runs(&self) -> u64 {
        todo!()
    }

    pub(crate) fn from_lsb0_bytes_unchecked(
        bytes: &[u8],
        byte_offset: usize,
        bits_set: u64,
    ) -> BitmapStore {
        todo!("{} {byte_offset} {bits_set}", bytes.len())
    }

    pub(crate) fn internal_validate(&self) -> Result<(), &'static str> {
        todo!()
    }
}

impl Clone for BitmapStore {
    fn clone(&self) -> Self {
        todo!()
    }

    fn clone_from(&mut self, source: &Self) {
        // Reuses the existing allocation.
        self.0.bits = source.0.bits;
        self.0.len = source.0.len;
    }
}

impl core::fmt::Debug for BitmapStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "BitmapStore<{}>", self.len())
    }
}
