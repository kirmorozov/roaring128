//! 65536-bit bitmap container.
//!
//! The bits and the cached cardinality live in one 64-byte aligned heap block so that AVX-512
//! loads never split cache lines and the container slot stays one pointer wide.

use core::ops::RangeInclusive;

use alloc::boxed::Box;

use super::ARRAY_LIMIT;
use super::array::ArrayStore;
use crate::simd::{self, BITMAP_WORDS, Words};

#[repr(C, align(64))]
pub(crate) struct BitmapData {
    pub(crate) bits: Words,
    /// Cached cardinality (may be stale only between `lazy_*` operations and `repair_lazy`).
    pub(crate) len: u64,
}

/// A bitmap of 65536 bits with cached cardinality.
pub(crate) struct BitmapStore(Box<BitmapData>);

const _: () = assert!(core::mem::size_of::<BitmapStore>() == 8);

#[allow(dead_code)]
impl BitmapStore {
    /// An empty bitmap (zeroed allocation).
    pub(crate) fn new() -> BitmapStore {
        // SAFETY: all-zero bits are a valid `BitmapData` (plain integers).
        BitmapStore(unsafe { Box::<BitmapData>::new_zeroed().assume_init() })
    }

    pub(crate) fn full() -> BitmapStore {
        let mut bitmap = BitmapStore::new();
        bitmap.0.bits = [u64::MAX; BITMAP_WORDS];
        bitmap.0.len = 1 << 16;
        bitmap
    }

    /// `len` must equal the popcount of `bits`.
    pub(crate) fn from_words_unchecked(len: u64, bits: &Words) -> BitmapStore {
        debug_assert_eq!(len, simd::bitmap_popcount(bits) as u64);
        let mut bitmap = BitmapStore::new();
        bitmap.0.bits = *bits;
        bitmap.0.len = len;
        bitmap
    }

    /// Validating constructor used by deserialization: `len` must match the popcount and be
    /// greater than `ARRAY_LIMIT`.
    pub(crate) fn try_from_words(len: u64, bits: &Words) -> Result<BitmapStore, &'static str> {
        if len <= ARRAY_LIMIT {
            return Err("bitmap container cardinality is too small");
        }
        if simd::bitmap_popcount(bits) as u64 != len {
            return Err("bitmap container cardinality does not match its bits");
        }
        Ok(BitmapStore::from_words_unchecked(len, bits))
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
        self.0.len = simd::bitmap_popcount(&self.0.bits) as u64;
        self.0.len
    }

    #[inline]
    pub(crate) fn insert(&mut self, index: u16) -> bool {
        let word = &mut self.0.bits[index as usize / 64];
        let new = *word | (1 << (index % 64));
        let added = new != *word;
        *word = new;
        self.0.len += added as u64;
        added
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

    #[inline]
    pub(crate) fn contains(&self, index: u16) -> bool {
        self.0.bits[index as usize / 64] >> (index % 64) & 1 == 1
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
        let mut data = Box::<BitmapData>::new_uninit();
        // SAFETY: `BitmapData` is plain data; copying one value fully initializes the box.
        unsafe {
            core::ptr::copy_nonoverlapping(&*self.0, data.as_mut_ptr(), 1);
            BitmapStore(data.assume_init())
        }
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
