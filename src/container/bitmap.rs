//! 65536-bit bitmap container.
//!
//! The bits and the cached cardinality live in one 64-byte aligned heap block so that AVX-512
//! loads never split cache lines and the container slot stays one pointer wide.
//!
//! Whole-bitmap work (popcounts, bitmap-bitmap operations, conversion to an array, run counting)
//! goes through the vectorized kernels of [`crate::simd`]. Partial spans (ranks, ranges) are
//! counted on the shorter side of the cached cardinality, so at most half of the words are read.

use core::mem::size_of;
use core::ops::RangeInclusive;

use alloc::boxed::Box;

use super::array::ArrayStore;
use super::{ARRAY_LIMIT, BITMAP_BYTES};
use crate::simd::{self, BITMAP_WORDS, BitOp, Words};

#[repr(C, align(64))]
pub(crate) struct BitmapData {
    pub(crate) bits: Words,
    /// Cached cardinality (may be stale only between `lazy_*` operations and `repair_lazy`).
    pub(crate) len: u64,
}

/// A bitmap of 65536 bits with cached cardinality.
pub(crate) struct BitmapStore(Box<BitmapData>);

const _: () = assert!(core::mem::size_of::<BitmapStore>() == 8);
const _: () = assert!(size_of::<Words>() == BITMAP_BYTES);

/// Word index of a value.
#[inline(always)]
pub(crate) const fn key(index: u16) -> usize {
    index as usize / 64
}

/// Bit position of a value inside its word.
#[inline(always)]
pub(crate) const fn bit(index: u16) -> u32 {
    index as u32 % 64
}

/// Mask of the bits `lo..=hi` of a word (`lo <= hi < 64`).
#[inline(always)]
const fn span_mask(lo: u32, hi: u32) -> u64 {
    (u64::MAX << lo) & (u64::MAX >> (63 - hi))
}

/// Number of set bits of a few words (spans shorter than half a bitmap).
#[inline]
fn count_words(words: &[u64]) -> u64 {
    words.iter().map(|w| w.count_ones() as u64).sum()
}

/// Position of the `n`-th (0 based) set bit of `word`; `n` must be below its popcount.
#[inline]
pub(crate) fn select_in_word(mut word: u64, mut n: u32) -> u32 {
    debug_assert!(n < word.count_ones());
    let mut base = 0;
    let low = (word as u32).count_ones();
    if n >= low {
        n -= low;
        word >>= 32;
        base += 32;
    }
    let low = (word as u16).count_ones();
    if n >= low {
        n -= low;
        word >>= 16;
        base += 16;
    }
    let low = (word as u8).count_ones();
    if n >= low {
        n -= low;
        word >>= 8;
        base += 8;
    }
    for _ in 0..n {
        word &= word - 1;
    }
    base + word.trailing_zeros()
}

/// The bits viewed as little-endian bytes in memory order (on little-endian targets byte `i`
/// holds the values `8 * i .. 8 * i + 8`).
#[inline]
fn words_as_bytes_mut(words: &mut Words) -> &mut [u8; BITMAP_BYTES] {
    // SAFETY: `[u64; 1024]` and `[u8; 8192]` have the same size, `u8` has no alignment
    // requirement and every byte pattern is valid for both types.
    unsafe { &mut *(words as *mut Words).cast::<[u8; BITMAP_BYTES]>() }
}

#[inline]
fn words_as_bytes(words: &Words) -> &[u8; BITMAP_BYTES] {
    // SAFETY: see `words_as_bytes_mut`.
    unsafe { &*(words as *const Words).cast::<[u8; BITMAP_BYTES]>() }
}

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

    /// A bitmap of the strictly increasing `values`.
    pub(crate) fn from_sorted_unchecked(values: &[u16]) -> BitmapStore {
        let mut bitmap = BitmapStore::new();
        bitmap.or_array_nocard(values);
        bitmap.0.len = values.len() as u64;
        debug_assert_eq!(bitmap.0.len, simd::bitmap_popcount(&bitmap.0.bits) as u64);
        bitmap
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

    /// Number of set bits in the words `lo..hi`, reading at most half of the bitmap (the other
    /// side is subtracted from the cached cardinality).
    #[inline]
    fn count_span(&self, lo: usize, hi: usize) -> u64 {
        let bits = &self.0.bits;
        if hi - lo <= BITMAP_WORDS / 2 {
            count_words(&bits[lo..hi])
        } else {
            self.0.len - count_words(&bits[..lo]) - count_words(&bits[hi..])
        }
    }

    #[inline]
    pub(crate) fn insert(&mut self, index: u16) -> bool {
        let word = &mut self.0.bits[key(index)];
        let new = *word | (1 << bit(index));
        let added = new != *word;
        *word = new;
        self.0.len += added as u64;
        added
    }

    /// Sets every value of the range, returns the number of newly set values.
    pub(crate) fn insert_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        let (start, end) = (*range.start(), *range.end());
        if start > end {
            return 0;
        }
        let present = self.range_cardinality(range);
        let (sk, ek) = (key(start), key(end));
        let bits = &mut self.0.bits;
        if sk == ek {
            bits[sk] |= span_mask(bit(start), bit(end));
        } else {
            bits[sk] |= u64::MAX << bit(start);
            bits[sk + 1..ek].fill(u64::MAX);
            bits[ek] |= u64::MAX >> (63 - bit(end));
        }
        let added = (end - start) as u64 + 1 - present;
        self.0.len += added;
        added
    }

    /// Sets `index` if it is greater than every value. Returns whether it was set.
    pub(crate) fn push(&mut self, index: u16) -> bool {
        let bits = &self.0.bits;
        let k = key(index);
        // Any bit at or above `index`? (OR-reduction without early exit so that it vectorizes.)
        if (bits[k] >> bit(index)) != 0 || bits[k + 1..].iter().fold(0, |acc, &w| acc | w) != 0 {
            return false;
        }
        self.insert(index);
        true
    }

    pub(crate) fn push_unchecked(&mut self, index: u16) {
        debug_assert!(
            self.max().is_none_or(|max| max < index),
            "push_unchecked: index is not greater than the maximum"
        );
        self.insert(index);
    }

    #[inline]
    pub(crate) fn remove(&mut self, index: u16) -> bool {
        let word = &mut self.0.bits[key(index)];
        let new = *word & !(1 << bit(index));
        let removed = new != *word;
        *word = new;
        self.0.len -= removed as u64;
        removed
    }

    /// Clears every value of the range, returns the number of removed values.
    pub(crate) fn remove_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        let (start, end) = (*range.start(), *range.end());
        if start > end {
            return 0;
        }
        let removed = self.range_cardinality(range);
        let (sk, ek) = (key(start), key(end));
        let bits = &mut self.0.bits;
        if sk == ek {
            bits[sk] &= !span_mask(bit(start), bit(end));
        } else {
            bits[sk] &= !(u64::MAX << bit(start));
            bits[sk + 1..ek].fill(0);
            bits[ek] &= !(u64::MAX >> (63 - bit(end)));
        }
        self.0.len -= removed;
        removed
    }

    pub(crate) fn clear(&mut self) {
        self.0.bits.fill(0);
        self.0.len = 0;
    }

    /// Clears the `n` smallest values (all of them if `n >= len`).
    pub(crate) fn remove_smallest(&mut self, n: u64) {
        if n >= self.0.len {
            self.clear();
            return;
        }
        self.0.len -= n;
        let mut remaining = n;
        for word in self.0.bits.iter_mut() {
            if remaining == 0 {
                return;
            }
            let count = word.count_ones() as u64;
            if remaining < count {
                for _ in 0..remaining {
                    *word &= *word - 1;
                }
                return;
            }
            *word = 0;
            remaining -= count;
        }
    }

    /// Clears the `n` biggest values (all of them if `n >= len`).
    pub(crate) fn remove_biggest(&mut self, n: u64) {
        if n >= self.0.len {
            self.clear();
            return;
        }
        self.0.len -= n;
        let mut remaining = n;
        for word in self.0.bits.iter_mut().rev() {
            if remaining == 0 {
                return;
            }
            let count = word.count_ones() as u64;
            if remaining < count {
                for _ in 0..remaining {
                    *word &= !(1 << (63 - word.leading_zeros()));
                }
                return;
            }
            *word = 0;
            remaining -= count;
        }
    }

    #[inline]
    pub(crate) fn contains(&self, index: u16) -> bool {
        self.0.bits[key(index)] >> bit(index) & 1 == 1
    }

    pub(crate) fn contains_range(&self, range: RangeInclusive<u16>) -> bool {
        let (start, end) = (*range.start(), *range.end());
        if start > end {
            return true;
        }
        if self.0.len < (end - start) as u64 + 1 {
            return false;
        }
        let (sk, ek) = (key(start), key(end));
        let bits = &self.0.bits;
        if sk == ek {
            let mask = span_mask(bit(start), bit(end));
            return bits[sk] & mask == mask;
        }
        let first = u64::MAX << bit(start);
        let last = u64::MAX >> (63 - bit(end));
        bits[sk] & first == first
            && bits[ek] & last == last
            && bits[sk + 1..ek].iter().fold(u64::MAX, |acc, &w| acc & w) == u64::MAX
    }

    pub(crate) fn min(&self) -> Option<u16> {
        let bits = &self.0.bits;
        let k = bits.iter().position(|&w| w != 0)?;
        Some((k * 64) as u16 + bits[k].trailing_zeros() as u16)
    }

    pub(crate) fn max(&self) -> Option<u16> {
        let bits = &self.0.bits;
        let k = bits.iter().rposition(|&w| w != 0)?;
        Some((k * 64) as u16 + 63 - bits[k].leading_zeros() as u16)
    }

    /// Number of values `<= index`.
    pub(crate) fn rank(&self, index: u16) -> u64 {
        let k = key(index);
        let below = self.count_span(0, k);
        let mask = u64::MAX >> (63 - bit(index));
        below + (self.0.bits[k] & mask).count_ones() as u64
    }

    /// The `n`-th smallest value, scanning from the closer end.
    pub(crate) fn select(&self, n: u16) -> Option<u16> {
        let n = n as u64;
        let len = self.0.len;
        if n >= len {
            return None;
        }
        let bits = &self.0.bits;
        if n < len / 2 {
            let mut n = n;
            for (k, &word) in bits.iter().enumerate() {
                let count = word.count_ones() as u64;
                if n < count {
                    return Some((k * 64) as u16 + select_in_word(word, n as u32) as u16);
                }
                n -= count;
            }
        } else {
            // Position counted from the biggest value.
            let mut m = len - 1 - n;
            for (k, &word) in bits.iter().enumerate().rev() {
                let count = word.count_ones() as u64;
                if m < count {
                    let bit = select_in_word(word, (count - 1 - m) as u32);
                    return Some((k * 64) as u16 + bit as u16);
                }
                m -= count;
            }
        }
        None
    }

    /// Number of set bits inside the range.
    pub(crate) fn range_cardinality(&self, range: RangeInclusive<u16>) -> u64 {
        let (start, end) = (*range.start(), *range.end());
        if start > end {
            return 0;
        }
        let (sk, ek) = (key(start), key(end));
        let bits = &self.0.bits;
        if sk == ek {
            return (bits[sk] & span_mask(bit(start), bit(end))).count_ones() as u64;
        }
        let inner = if sk + 1 < ek {
            self.count_span(sk + 1, ek)
        } else {
            0
        };
        inner
            + (bits[sk] & (u64::MAX << bit(start))).count_ones() as u64
            + (bits[ek] & (u64::MAX >> (63 - bit(end)))).count_ones() as u64
    }

    /// The values as a sorted array (vectorized extraction). The cached cardinality must be
    /// exact.
    pub(crate) fn to_array_store(&self) -> ArrayStore {
        let bits = &self.0.bits;
        ArrayStore::build(self.0.len as usize, |out| simd::bitmap_to_array(bits, out))
    }

    pub(crate) fn count_runs(&self) -> u64 {
        simd::bitmap_count_runs(&self.0.bits) as u64
    }

    /// Copies little-endian LSB-0 bytes describing the values `byte_offset * 8 ..`; `bits_set`
    /// must be their popcount.
    pub(crate) fn from_lsb0_bytes_unchecked(
        bytes: &[u8],
        byte_offset: usize,
        bits_set: u64,
    ) -> BitmapStore {
        let mut bitmap = BitmapStore::from_lsb0_bytes_uncounted(bytes, byte_offset);
        bitmap.0.len = bits_set;
        debug_assert_eq!(bits_set, simd::bitmap_popcount(&bitmap.0.bits) as u64);
        bitmap
    }

    /// Copies little-endian LSB-0 bytes into a new bitmap and counts them (vectorized).
    pub(crate) fn from_lsb0_bytes(bytes: &[u8], byte_offset: usize) -> BitmapStore {
        let mut bitmap = BitmapStore::from_lsb0_bytes_uncounted(bytes, byte_offset);
        bitmap.recount();
        bitmap
    }

    /// Copies the bytes, leaving the cardinality at zero.
    fn from_lsb0_bytes_uncounted(bytes: &[u8], byte_offset: usize) -> BitmapStore {
        let end = byte_offset
            .checked_add(bytes.len())
            .filter(|&end| end <= BITMAP_BYTES)
            .expect("byte_offset + bytes.len() must be at most 8192");
        let mut bitmap = BitmapStore::new();
        words_as_bytes_mut(&mut bitmap.0.bits)[byte_offset..end].copy_from_slice(bytes);
        if cfg!(target_endian = "big") {
            for word in &mut bitmap.0.bits[byte_offset / 8..end.div_ceil(8)] {
                *word = u64::from_le(*word);
            }
        }
        bitmap
    }

    /// Appends the 8192 payload bytes (little-endian words).
    pub(crate) fn write_le_bytes(&self, out: &mut alloc::vec::Vec<u8>) {
        if cfg!(target_endian = "little") {
            out.extend_from_slice(words_as_bytes(&self.0.bits));
        } else {
            out.reserve(BITMAP_BYTES);
            for word in &self.0.bits {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
    }

    /// Bytes used by the store: its slot plus the heap block.
    pub(crate) fn mem_size(&self) -> usize {
        size_of::<BitmapStore>() + size_of::<BitmapData>()
    }

    pub(crate) fn internal_validate(&self) -> Result<(), &'static str> {
        if simd::bitmap_popcount(&self.0.bits) as u64 != self.0.len {
            return Err("bitmap cardinality is incorrect");
        }
        if self.0.len <= ARRAY_LIMIT {
            return Err("cardinality is too small for a bitmap container");
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Operations with arrays and other bitmaps.
    // -----------------------------------------------------------------------------------------

    /// Sets the bits of `values`, keeping the cardinality exact.
    pub(crate) fn or_array(&mut self, values: &[u16]) {
        let bits = &mut self.0.bits;
        let mut len = self.0.len;
        for &v in values {
            let word = &mut bits[key(v)];
            let mask = 1 << bit(v);
            len += (*word & mask == 0) as u64;
            *word |= mask;
        }
        self.0.len = len;
    }

    /// Sets the bits of `values` without maintaining the cardinality.
    pub(crate) fn or_array_nocard(&mut self, values: &[u16]) {
        let bits = &mut self.0.bits;
        for &v in values {
            bits[key(v)] |= 1 << bit(v);
        }
    }

    /// Clears the bits of `values`, keeping the cardinality exact.
    pub(crate) fn andnot_array(&mut self, values: &[u16]) {
        let bits = &mut self.0.bits;
        let mut len = self.0.len;
        for &v in values {
            let word = &mut bits[key(v)];
            let mask = 1 << bit(v);
            len -= (*word & mask != 0) as u64;
            *word &= !mask;
        }
        self.0.len = len;
    }

    /// Flips the bits of `values`, keeping the cardinality exact.
    pub(crate) fn xor_array(&mut self, values: &[u16]) {
        let bits = &mut self.0.bits;
        let mut len = self.0.len;
        for &v in values {
            let word = &mut bits[key(v)];
            let mask = 1 << bit(v);
            // +1 if the bit was clear, -1 if it was set.
            len = len + 1 - 2 * ((*word & mask != 0) as u64);
            *word ^= mask;
        }
        self.0.len = len;
    }

    /// Flips the bits of `values` without maintaining the cardinality.
    pub(crate) fn xor_array_nocard(&mut self, values: &[u16]) {
        let bits = &mut self.0.bits;
        for &v in values {
            bits[key(v)] ^= 1 << bit(v);
        }
    }

    /// Number of `values` present in the bitmap.
    pub(crate) fn intersection_len_array(&self, values: &[u16]) -> u64 {
        values.iter().map(|&v| self.contains(v) as u64).sum()
    }

    /// The `values` whose membership equals `present` (branchless filter): `true` keeps the
    /// intersection, `false` the difference `values - self`.
    pub(crate) fn filter_array(&self, values: &[u16], present: bool) -> ArrayStore {
        ArrayStore::build(values.len(), |out| {
            let mut k = 0;
            for &v in values {
                // `k` never passes the input index, so the write is in bounds.
                out[k].write(v);
                k += (self.contains(v) == present) as usize;
            }
            k
        })
    }

    /// `self = self op other`, returns the new cardinality.
    pub(crate) fn op_assign(&mut self, op: BitOp, other: &BitmapStore) -> u64 {
        let len = simd::bitmap_op_inplace(op, &mut self.0.bits, &other.0.bits) as u64;
        self.0.len = len;
        len
    }

    /// `self = self op other`, leaving the cached cardinality stale.
    pub(crate) fn op_assign_nocard(&mut self, op: BitOp, other: &BitmapStore) {
        simd::bitmap_op_inplace_nocard(op, &mut self.0.bits, &other.0.bits);
    }

    /// A new bitmap `a op b`.
    pub(crate) fn op(op: BitOp, a: &BitmapStore, b: &BitmapStore) -> BitmapStore {
        let mut out = BitmapStore::new();
        out.0.len = simd::bitmap_op(op, &a.0.bits, &b.0.bits, &mut out.0.bits) as u64;
        out
    }

    /// The array of `a op b`, whose cardinality `card` is already known (and small): the
    /// result is computed into a stack buffer and extracted with the vectorized kernel.
    pub(crate) fn op_to_array(
        op: BitOp,
        a: &BitmapStore,
        b: &BitmapStore,
        card: u64,
    ) -> ArrayStore {
        let mut tmp: Words = [0; BITMAP_WORDS];
        let n = simd::bitmap_op(op, &a.0.bits, &b.0.bits, &mut tmp) as u64;
        debug_assert_eq!(n, card);
        ArrayStore::build(card as usize, |out| simd::bitmap_to_array(&tmp, out))
    }

    /// The array of the values of `self` after clearing (`flip == false`) or flipping
    /// (`flip == true`) the bits of `values`; `card` is the resulting cardinality.
    pub(crate) fn array_op_to_array(&self, values: &[u16], flip: bool, card: u64) -> ArrayStore {
        let mut tmp: Words = self.0.bits;
        for &v in values {
            let mask = 1u64 << bit(v);
            let word = &mut tmp[key(v)];
            *word = if flip { *word ^ mask } else { *word & !mask };
        }
        ArrayStore::build(card as usize, |out| simd::bitmap_to_array(&tmp, out))
    }

    pub(crate) fn intersection_len(&self, other: &BitmapStore) -> u64 {
        simd::bitmap_op_card(BitOp::And, &self.0.bits, &other.0.bits) as u64
    }

    pub(crate) fn is_disjoint(&self, other: &BitmapStore) -> bool {
        !simd::bitmap_intersects(&self.0.bits, &other.0.bits)
    }

    pub(crate) fn is_subset(&self, other: &BitmapStore) -> bool {
        simd::bitmap_is_subset(&self.0.bits, &other.0.bits)
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
