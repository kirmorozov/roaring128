//! Sorted `u16` array algorithms shared by the AVX2 and AVX-512 backends.
//!
//! The algorithms follow CRoaring (Lemire et al., "Roaring Bitmaps: Implementation of an
//! Optimized Software Library", 2018) with a few changes:
//!
//! * intersection, intersection cardinality, `intersects` and difference compare blocks of
//!   eight values with the SSE4.2 `pcmpistrm` instruction. The implicit-length string compare
//!   stops at the first zero lane, so a leading zero value is peeled off first, and the final
//!   partial blocks are zero padded, which makes the padding lanes invalid instead of requiring
//!   a scalar tail. Blocks are advanced without branches.
//! * union and symmetric difference merge blocks of eight values with a bitonic merge network
//!   (10 instructions deep instead of the 16 of the rotate-and-compare network) and remove
//!   duplicates from the merged stream before packing it.
//! * the merge tails are merged into the rest of the longer input with galloping searches and
//!   bulk copies.
//! * when one input is much smaller than the other (ratio 10 to 24 depending on the kernel),
//!   its values are located in the larger input with branch-free binary searches, eight in
//!   lockstep, and the runs in between are copied in bulk. This replaces CRoaring's galloping,
//!   whose single chain of dependent, mispredicted branches is 3 to 4 times slower.
//!
//! Each backend provides the lane primitives through [`Lanes`]: AVX2 packs lanes with a `pshufb`
//! lookup table, AVX-512 uses `vpcompressw` and masked loads and stores.
//!
//! All functions are `#[inline(always)]` and only meant to be instantiated inside the
//! `#[target_feature]` kernels of a backend (they rely on at least AVX2 + SSE4.2 + POPCNT).

use core::arch::x86_64::*;
use core::mem::MaybeUninit;

use crate::simd::check_out;
use crate::simd::scalar::{copy_tail, gallop};

// Size ratios from which searching the values of the smaller input in the larger one beats the
// vector merges (which touch every element of both inputs). Measured on Zen 3 with 4096 values
// on the large side: a search costs ~5.5 ns per value, a block merge ~0.6 ns per value of both
// inputs.

/// Intersection, intersection cardinality and `intersects`.
const SEARCH_RATIO_INTERSECT: usize = 10;
/// Union and symmetric difference (the runs between the found positions are copied).
const SEARCH_RATIO_MERGE: usize = 14;
/// Difference with a small minuend.
const SEARCH_RATIO_DIFF_SMALL_A: usize = 14;
/// Difference with a small subtrahend (the runs between the found positions are copied).
const SEARCH_RATIO_DIFF_SMALL_B: usize = 24;

/// Lane primitives on vectors of eight `u16`.
pub(crate) trait Lanes {
    /// Bit `i` is set iff lane `i` of `a` equals lane `i` of `b`.
    unsafe fn eq_mask(a: __m128i, b: __m128i) -> u32;

    /// Moves the lanes selected by `keep` (bit `i` <=> lane `i`, 8 bits) to the front, in order.
    /// The other lanes are unspecified.
    unsafe fn pack(v: __m128i, keep: u32) -> __m128i;

    /// Loads `n < 8` values from `p`, zeroing the other lanes. Never reads past `p + n`.
    #[inline(always)]
    unsafe fn load_partial(p: *const u16, n: usize) -> __m128i {
        let mut tmp = [0u16; 8];
        unsafe {
            copy_lt8(p, tmp.as_mut_ptr(), n);
            _mm_loadu_si128(tmp.as_ptr().cast())
        }
    }

    /// Writes the first `n` lanes of `v` to `out[k..k + n]` without touching anything past
    /// `out.len()`. Requires `k + n <= out.len()`.
    #[inline(always)]
    unsafe fn store_first(v: __m128i, n: usize, out: &mut [MaybeUninit<u16>], k: usize) {
        debug_assert!(k + n <= out.len());
        unsafe {
            let dst = out.as_mut_ptr().add(k).cast::<u16>();
            if out.len() - k >= 8 {
                _mm_storeu_si128(dst.cast(), v);
            } else {
                let mut tmp = [0u16; 8];
                _mm_storeu_si128(tmp.as_mut_ptr().cast(), v);
                copy_lt8(tmp.as_ptr(), dst, n);
            }
        }
    }
}

/// Copies `n < 8` values with at most three fixed size moves (no `memcpy` call, which would
/// force the hot loops around it to spill registers).
#[inline(always)]
unsafe fn copy_lt8(src: *const u16, dst: *mut u16, n: usize) {
    debug_assert!(n < 8);
    unsafe {
        let mut o = 0;
        if n & 4 != 0 {
            dst.cast::<u64>()
                .write_unaligned(src.cast::<u64>().read_unaligned());
            o = 4;
        }
        if n & 2 != 0 {
            let v = src.add(o).cast::<u32>().read_unaligned();
            dst.add(o).cast::<u32>().write_unaligned(v);
            o += 2;
        }
        if n & 1 != 0 {
            *dst.add(o) = *src.add(o);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// pcmpistrm based kernels
// ---------------------------------------------------------------------------------------------

/// `pcmpistrm` mode: unsigned words, "equal any", bit mask result.
const EQUAL_ANY: i32 = _SIDD_UWORD_OPS | _SIDD_CMP_EQUAL_ANY | _SIDD_BIT_MASK;

/// Bit `i` is set iff lane `i` of `s` is a valid (non zero) lane equal to one of the valid lanes
/// of `set`. Lanes after the first zero lane of either operand are invalid.
#[inline(always)]
unsafe fn match_mask(set: __m128i, s: __m128i) -> u32 {
    unsafe { _mm_cvtsi128_si32(_mm_cmpistrm::<EQUAL_ANY>(set, s)) as u32 }
}

/// Loads the block of (up to) eight values starting at `s[i]`, zero padded. `i < s.len()`.
#[inline(always)]
unsafe fn load_block<L: Lanes>(s: &[u16], i: usize) -> __m128i {
    debug_assert!(i < s.len());
    unsafe {
        let p = s.as_ptr().add(i);
        let rem = s.len() - i;
        if rem >= 8 {
            _mm_loadu_si128(p.cast())
        } else {
            L::load_partial(p, rem)
        }
    }
}

/// The last value of the block starting at `s[i]`. `i < s.len()`.
#[inline(always)]
unsafe fn block_max(s: &[u16], i: usize) -> u16 {
    debug_assert!(i < s.len());
    unsafe { *s.get_unchecked((i + 7).min(s.len() - 1)) }
}

/// Mask of the valid lanes of the block starting at `s[i]`.
#[inline(always)]
fn block_lanes(s: &[u16], i: usize) -> u32 {
    let n = (s.len() - i).min(8) as u32;
    (1u32 << n) - 1
}

/// Packs the lanes of `v` selected by `keep` to `out[k..]`, returns the new length.
#[inline(always)]
unsafe fn store_packed<L: Lanes>(
    v: __m128i,
    keep: u32,
    out: &mut [MaybeUninit<u16>],
    k: usize,
) -> usize {
    let n = keep.count_ones() as usize;
    unsafe { L::store_first(L::pack(v, keep), n, out, k) };
    k + n
}

/// Whether `small` is at least `ratio` times smaller than `large`.
#[inline(always)]
fn search_wins(small: &[u16], large: &[u16], ratio: usize) -> bool {
    small.len().saturating_mul(ratio) <= large.len()
}

/// The shorter and the longer of two inputs.
#[inline(always)]
fn by_len<'a>(a: &'a [u16], b: &'a [u16]) -> (&'a [u16], &'a [u16]) {
    if a.len() <= b.len() { (a, b) } else { (b, a) }
}

/// Index of the first value of `s` that is `>= v` (`s.len()` if none), branch free.
#[inline(always)]
fn lower_bound(s: &[u16], v: u16) -> usize {
    let mut n = s.len();
    if n == 0 {
        return 0;
    }
    // The answer is in `base..=base + n`, and `base + n <= s.len()`.
    let mut base = 0;
    while n > 1 {
        let half = n / 2;
        // SAFETY: `base + half < base + n <= s.len()`.
        let probe = unsafe { *s.get_unchecked(base + half) };
        base = core::hint::select_unpredictable(probe < v, base + half, base);
        n -= half;
    }
    // SAFETY: `base < base + n == base + 1 <= s.len()`.
    base + (unsafe { *s.get_unchecked(base) } < v) as usize
}

/// Number of binary searches run in lockstep by [`search_each`].
const SEARCH_LANES: usize = 8;

/// Calls `f(v, p)` for the values `v` of `small` in order, where `p` is the index of the first
/// value of `large` that is `>= v`, until `f` returns `false`.
///
/// The searches are branch-free binary searches over the whole of `large`, eight in lockstep:
/// unlike galloping (one chain of dependent, mispredicted branches) the eight independent
/// chains of loads fill the pipeline. (Narrowing each group to the part after the previous
/// group's results would serialize the groups, which costs more than the saved steps.)
#[inline(always)]
fn search_each(small: &[u16], large: &[u16], mut f: impl FnMut(u16, usize) -> bool) {
    let len = large.len();
    if len == 0 {
        for &v in small {
            if !f(v, 0) {
                return;
            }
        }
        return;
    }
    let (groups, rest) = small.as_chunks::<SEARCH_LANES>();
    for g in groups {
        let mut base = [0usize; SEARCH_LANES];
        let mut n = len;
        while n > 1 {
            let half = n / 2;
            for t in 0..SEARCH_LANES {
                // SAFETY: `base[t] + half < base[t] + n <= len`.
                let probe = unsafe { *large.get_unchecked(base[t] + half) };
                base[t] = core::hint::select_unpredictable(probe < g[t], base[t] + half, base[t]);
            }
            n -= half;
        }
        for t in 0..SEARCH_LANES {
            // SAFETY: `base[t] < base[t] + 1 <= len`.
            let p = base[t] + (unsafe { *large.get_unchecked(base[t]) } < g[t]) as usize;
            if !f(g[t], p) {
                return;
            }
        }
    }
    for &v in rest {
        if !f(v, lower_bound(large, v)) {
            return;
        }
    }
}

/// Whether `s[p] == v`, for a position returned by a search (`p <= s.len()`).
#[inline(always)]
fn found_at(s: &[u16], p: usize, v: u16) -> bool {
    s.get(p) == Some(&v)
}

/// Intersection of `small` with `large` by searches, written to `out[..]`.
#[inline(always)]
pub(super) fn intersect_search(
    small: &[u16],
    large: &[u16],
    out: &mut [MaybeUninit<u16>],
) -> usize {
    debug_assert!(out.len() >= small.len());
    let mut k = 0;
    search_each(small, large, |v, p| {
        // `k` <= index of `v` in `small` < `out.len()`.
        out[k].write(v);
        k += found_at(large, p, v) as usize;
        true
    });
    k
}

/// `a - b` for a small `a`, by searches into `b`.
#[inline(always)]
pub(super) fn difference_search_small(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    let mut k = 0;
    search_each(a, b, |v, p| {
        out[k].write(v);
        k += !found_at(b, p, v) as usize;
        true
    });
    k
}

/// `a - b` for a small `b`: copies the runs of `a` between the values of `b`.
#[inline(always)]
pub(super) fn difference_search_large(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    let (mut k, mut i) = (0, 0);
    search_each(b, a, |v, p| {
        k = copy_tail(&a[i..p], out, k);
        i = p + found_at(a, p, v) as usize;
        true
    });
    copy_tail(&a[i..], out, k)
}

/// Union (`XOR == false`) or symmetric difference (`XOR == true`) of a small set with a large
/// one: copies the runs of `large` between the values of `small`.
#[inline(always)]
pub(super) fn merge_search<const XOR: bool>(
    small: &[u16],
    large: &[u16],
    out: &mut [MaybeUninit<u16>],
) -> usize {
    let (mut k, mut j) = (0, 0);
    search_each(small, large, |v, p| {
        k = copy_tail(&large[j..p], out, k);
        let found = found_at(large, p, v);
        j = p + found as usize;
        if !(XOR && found) {
            out[k].write(v);
            k += 1;
        }
        true
    });
    copy_tail(&large[j..], out, k)
}

/// Peels a leading zero value off both inputs (`pcmpistrm` treats zero lanes as terminators).
/// Returns whether zero belongs to both inputs. Both inputs must be non-empty.
#[inline(always)]
fn strip_zero(a: &mut &[u16], b: &mut &[u16]) -> bool {
    let (za, zb) = (a[0] == 0, b[0] == 0);
    if za {
        *a = &a[1..];
    }
    if zb {
        *b = &b[1..];
    }
    za && zb
}

/// Advances the block cursors of a block-wise merge: the block(s) with the smaller last value
/// are done. Branch free, the comparison outcome is hard to predict.
#[inline(always)]
fn advance(i: &mut usize, j: &mut usize, amax: u16, bmax: u16) {
    *i += 8 * (amax <= bmax) as usize;
    *j += 8 * (bmax <= amax) as usize;
}

#[inline(always)]
pub(crate) unsafe fn intersect<L: Lanes>(
    a: &[u16],
    b: &[u16],
    out: &mut [MaybeUninit<u16>],
) -> usize {
    check_out(out, a.len().min(b.len()));
    let (small, large) = by_len(a, b);
    if small.is_empty() {
        return 0;
    }
    if search_wins(small, large, SEARCH_RATIO_INTERSECT) {
        return intersect_search(small, large, out);
    }
    unsafe { intersect_blocks::<L>(a, b, out) }
}

/// Block-wise `pcmpistrm` intersection; both inputs non-empty.
#[inline(always)]
pub(crate) unsafe fn intersect_blocks<L: Lanes>(
    a: &[u16],
    b: &[u16],
    out: &mut [MaybeUninit<u16>],
) -> usize {
    let (mut a, mut b) = (a, b);
    let mut k = 0;
    if strip_zero(&mut a, &mut b) {
        out[0].write(0);
        k = 1;
    }
    if a.is_empty() || b.is_empty() {
        return k;
    }
    let (mut i, mut j) = (0, 0);
    unsafe {
        loop {
            let va = load_block::<L>(a, i);
            let vb = load_block::<L>(b, j);
            k = store_packed::<L>(va, match_mask(vb, va), out, k);
            let (amax, bmax) = (block_max(a, i), block_max(b, j));
            advance(&mut i, &mut j, amax, bmax);
            if i >= a.len() || j >= b.len() {
                return k;
            }
        }
    }
}

#[inline(always)]
pub(crate) unsafe fn intersect_card<L: Lanes>(a: &[u16], b: &[u16]) -> usize {
    let (small, large) = by_len(a, b);
    if small.is_empty() {
        return 0;
    }
    if search_wins(small, large, SEARCH_RATIO_INTERSECT) {
        let mut k = 0;
        search_each(small, large, |v, p| {
            k += found_at(large, p, v) as usize;
            true
        });
        return k;
    }
    let (mut a, mut b) = (a, b);
    let mut k = strip_zero(&mut a, &mut b) as usize;
    if a.is_empty() || b.is_empty() {
        return k;
    }
    let (mut i, mut j) = (0, 0);
    unsafe {
        loop {
            let va = load_block::<L>(a, i);
            let vb = load_block::<L>(b, j);
            k += match_mask(vb, va).count_ones() as usize;
            let (amax, bmax) = (block_max(a, i), block_max(b, j));
            advance(&mut i, &mut j, amax, bmax);
            if i >= a.len() || j >= b.len() {
                return k;
            }
        }
    }
}

#[inline(always)]
pub(crate) unsafe fn intersects<L: Lanes>(a: &[u16], b: &[u16]) -> bool {
    let (small, large) = by_len(a, b);
    if small.is_empty() {
        return false;
    }
    if search_wins(small, large, SEARCH_RATIO_INTERSECT) {
        let mut any = false;
        search_each(small, large, |v, p| {
            any = found_at(large, p, v);
            !any
        });
        return any;
    }
    let (mut a, mut b) = (a, b);
    if strip_zero(&mut a, &mut b) {
        return true;
    }
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let (mut i, mut j) = (0, 0);
    unsafe {
        loop {
            let va = load_block::<L>(a, i);
            let vb = load_block::<L>(b, j);
            if match_mask(vb, va) != 0 {
                return true;
            }
            let (amax, bmax) = (block_max(a, i), block_max(b, j));
            advance(&mut i, &mut j, amax, bmax);
            if i >= a.len() || j >= b.len() {
                return false;
            }
        }
    }
}

#[inline(always)]
pub(crate) unsafe fn difference<L: Lanes>(
    a: &[u16],
    b: &[u16],
    out: &mut [MaybeUninit<u16>],
) -> usize {
    check_out(out, a.len());
    if a.is_empty() {
        return 0;
    }
    if b.is_empty() {
        return copy_tail(a, out, 0);
    }
    if search_wins(a, b, SEARCH_RATIO_DIFF_SMALL_A) {
        return difference_search_small(a, b, out);
    }
    if search_wins(b, a, SEARCH_RATIO_DIFF_SMALL_B) {
        return difference_search_large(a, b, out);
    }
    unsafe { difference_blocks::<L>(a, b, out) }
}

/// Block-wise `pcmpistrm` difference; both inputs non-empty.
#[inline(always)]
pub(crate) unsafe fn difference_blocks<L: Lanes>(
    a: &[u16],
    b: &[u16],
    out: &mut [MaybeUninit<u16>],
) -> usize {
    let (mut a, mut b) = (a, b);
    let mut k = 0;
    if a[0] == 0 && b[0] != 0 {
        // Zero survives; peel it off `a` only.
        out[0].write(0);
        k = 1;
        a = &a[1..];
    } else {
        strip_zero(&mut a, &mut b);
    }
    if a.is_empty() {
        return k;
    }
    if b.is_empty() {
        return copy_tail(a, out, k);
    }
    let (mut i, mut j) = (0, 0);
    // Lanes of the current block of `a` seen in `b` so far.
    let mut found = 0u32;
    unsafe {
        loop {
            let va = load_block::<L>(a, i);
            let vb = load_block::<L>(b, j);
            found |= match_mask(vb, va);
            let (amax, bmax) = (block_max(a, i), block_max(b, j));
            if amax <= bmax {
                // The rest of `b` is larger than this block: it is complete.
                k = store_packed::<L>(va, block_lanes(a, i) & !found, out, k);
                found = 0;
                i += 8;
                if i >= a.len() {
                    return k;
                }
            }
            if bmax <= amax {
                j += 8;
                if j >= b.len() {
                    // `b` is exhausted: flush the current block of `a`, copy the rest.
                    let va = load_block::<L>(a, i);
                    k = store_packed::<L>(va, block_lanes(a, i) & !found, out, k);
                    let rest = (i + 8).min(a.len());
                    return copy_tail(&a[rest..], out, k);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Merge network based kernels
// ---------------------------------------------------------------------------------------------

/// Reverses the order of the eight lanes.
#[inline(always)]
unsafe fn reverse(v: __m128i) -> __m128i {
    unsafe {
        _mm_shuffle_epi8(
            v,
            _mm_setr_epi8(14, 15, 12, 13, 10, 11, 8, 9, 6, 7, 4, 5, 2, 3, 0, 1),
        )
    }
}

/// Bitonic merge of an ascending vector `a` and a descending vector `b_rev`: returns the eight
/// smallest and the eight largest of the sixteen values, both ascending.
#[inline(always)]
unsafe fn merge8(a: __m128i, b_rev: __m128i) -> (__m128i, __m128i) {
    unsafe {
        // Half cleaner over the bitonic 16-sequence: every lane of `l` <= every lane of `h`,
        // both bitonic. The remaining levels sort `l` and `h` side by side.
        let l = _mm_min_epu16(a, b_rev);
        let h = _mm_max_epu16(a, b_rev);
        // Distance 4.
        let x = _mm_unpacklo_epi64(l, h);
        let y = _mm_unpackhi_epi64(l, h);
        let l = _mm_min_epu16(x, y);
        let h = _mm_max_epu16(x, y);
        // Distance 2 (in 32-bit units: l = [p01 p23 r01 r23], h = [q01 q23 s01 s23]).
        let x = _mm_blend_epi32::<0b1010>(l, _mm_shuffle_epi32::<0b10_10_00_00>(h));
        let y = _mm_blend_epi32::<0b1010>(_mm_shuffle_epi32::<0b11_11_01_01>(l), h);
        let l = _mm_min_epu16(x, y);
        let h = _mm_max_epu16(x, y);
        // Distance 1.
        let x = _mm_blend_epi16::<0b1010_1010>(l, _mm_slli_epi32::<16>(h));
        let y = _mm_blend_epi16::<0b1010_1010>(_mm_srli_epi32::<16>(l), h);
        let l = _mm_min_epu16(x, y);
        let h = _mm_max_epu16(x, y);
        (_mm_unpacklo_epi16(l, h), _mm_unpackhi_epi16(l, h))
    }
}

/// Writes the lanes of `cur` that differ from their predecessor in the merged stream (`prev`
/// holds the previous eight values). Writes eight lanes at `dst`, returns the number kept.
#[inline(always)]
unsafe fn store_unique<L: Lanes>(prev: __m128i, cur: __m128i, dst: *mut u16) -> usize {
    unsafe {
        let shifted = _mm_alignr_epi8::<14>(cur, prev);
        let keep = !L::eq_mask(shifted, cur) & 0xFF;
        _mm_storeu_si128(dst.cast(), L::pack(cur, keep));
        keep.count_ones() as usize
    }
}

/// Symmetric difference version of [`store_unique`], delayed by one lane: decides
/// `[prev[7], cur[0..7]]`, keeping the values equal to neither neighbour.
#[inline(always)]
unsafe fn store_xor<L: Lanes>(prev: __m128i, cur: __m128i, dst: *mut u16) -> usize {
    unsafe {
        let left = _mm_alignr_epi8::<12>(cur, prev);
        let mid = _mm_alignr_epi8::<14>(cur, prev);
        let drop = L::eq_mask(mid, left) | L::eq_mask(mid, cur);
        let keep = !drop & 0xFF;
        _mm_storeu_si128(dst.cast(), L::pack(mid, keep));
        keep.count_ones() as usize
    }
}

#[inline(always)]
unsafe fn lanes_of(v: __m128i) -> [u16; 8] {
    let mut lanes = [0u16; 8];
    unsafe { _mm_storeu_si128(lanes.as_mut_ptr().cast(), v) };
    lanes
}

/// Merges two sorted sequences into `out`, keeping duplicates. Returns the length.
#[inline(always)]
fn merge_small(x: &[u16], y: &[u16], out: &mut [u16]) -> usize {
    let (mut i, mut j, mut k) = (0, 0, 0);
    while i < x.len() && j < y.len() {
        if x[i] <= y[j] {
            out[k] = x[i];
            i += 1;
        } else {
            out[k] = y[j];
            j += 1;
        }
        k += 1;
    }
    for &v in x[i..].iter().chain(&y[j..]) {
        out[k] = v;
        k += 1;
    }
    k
}

/// Union of a short sorted sequence `pend` (duplicates allowed) with the sorted set `long`,
/// written to `out[k..]` after the value `last`; every value is at least `last`, values equal
/// to it are skipped. The runs of `long` between the values of `pend` are found by galloping
/// and copied in bulk.
#[inline(always)]
fn union_tail(
    pend: &[u16],
    long: &[u16],
    out: &mut [MaybeUninit<u16>],
    mut k: usize,
    mut last: u16,
) -> usize {
    let mut j = 0;
    for &x in pend {
        if x == last {
            continue;
        }
        let p = gallop(long, j, x);
        if p > j {
            // `long[j..p]` < x; only its first value can repeat `last`.
            let skip = (long[j] == last) as usize;
            k = copy_tail(&long[j + skip..p], out, k);
        }
        j = p;
        if j < long.len() && long[j] == x {
            j += 1;
        }
        out[k].write(x);
        k += 1;
        last = x;
    }
    if j < long.len() {
        let skip = (long[j] == last) as usize;
        k = copy_tail(&long[j + skip..], out, k);
    }
    k
}

/// Symmetric difference of a short sorted sequence `pend` (duplicates allowed) with the sorted
/// set `long`, written to `out[k..]`: a value is written iff it occurs exactly once in `pend`,
/// `long` and `left` (the last value of the stream already decided) together.
#[inline(always)]
fn xor_tail(
    pend: &[u16],
    long: &[u16],
    out: &mut [MaybeUninit<u16>],
    mut k: usize,
    left: u16,
) -> usize {
    debug_assert!(long.first().is_none_or(|&v| v != left));
    let mut j = 0;
    let mut i = 0;
    while i < pend.len() {
        let x = pend[i];
        let mut count = (x == left) as usize;
        while i < pend.len() && pend[i] == x {
            count += 1;
            i += 1;
        }
        let p = gallop(long, j, x);
        k = copy_tail(&long[j..p], out, k);
        j = p;
        if j < long.len() && long[j] == x {
            count += 1;
            j += 1;
        }
        if count == 1 {
            out[k].write(x);
            k += 1;
        }
    }
    copy_tail(&long[j..], out, k)
}

/// Runs the block-wise merge of `a` and `b` (both at least eight long), calling `emit` with
/// the previously merged vector, the newly merged vector and the output position. Returns the
/// cursors after the last full block of either input together with the last emitted vector and
/// the pending upper half of the merge.
#[inline(always)]
unsafe fn merge_blocks(
    a: &[u16],
    b: &[u16],
    mut emit: impl FnMut(__m128i, __m128i),
) -> (usize, usize, __m128i, __m128i) {
    debug_assert!(a.len() >= 8 && b.len() >= 8);
    unsafe {
        let (pa, pb) = (a.as_ptr(), b.as_ptr());
        let (st_a, st_b) = (a.len() & !7, b.len() & !7);
        let (mut lo, mut hi) = merge8(
            _mm_loadu_si128(pa.cast()),
            reverse(_mm_loadu_si128(pb.cast())),
        );
        // No value of the inputs equals the sentinel's last lane: their minimum is at most
        // 65535 - 7.
        emit(_mm_set1_epi16(-1), lo);
        let mut prev = lo;
        let (mut i, mut j) = (8, 8);
        while i < st_a && j < st_b {
            // Load the block with the smaller first value next (branch free).
            let take_a = *pa.add(i) <= *pb.add(j);
            let p = core::hint::select_unpredictable(take_a, pa.add(i), pb.add(j));
            i += 8 * take_a as usize;
            j += 8 * !take_a as usize;
            (lo, hi) = merge8(hi, reverse(_mm_loadu_si128(p.cast())));
            emit(prev, lo);
            prev = lo;
        }
        (i, j, prev, hi)
    }
}

/// The values still to merge after [`merge_blocks`]: the pending vector merged with the
/// remainder of the input whose full blocks are exhausted (at most 15 values, sorted, with
/// duplicates), plus the remainder of the other input.
#[inline(always)]
unsafe fn pending<'a>(
    a: &'a [u16],
    b: &'a [u16],
    i: usize,
    j: usize,
    first: &[u16],
    hi: __m128i,
    buf: &mut [u16; 24],
) -> (usize, &'a [u16]) {
    let (short, long) = if a.len() - i < 8 {
        (&a[i..], &b[j..])
    } else {
        (&b[j..], &a[i..])
    };
    let mut head = [0u16; 9];
    head[..first.len()].copy_from_slice(first);
    head[first.len()..first.len() + 8].copy_from_slice(&unsafe { lanes_of(hi) });
    let n = merge_small(&head[..first.len() + 8], short, buf);
    (n, long)
}

#[inline(always)]
pub(crate) unsafe fn union<L: Lanes>(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len() + b.len());
    let (short, long) = by_len(a, b);
    if short.len() < 8 || search_wins(short, long, SEARCH_RATIO_MERGE) {
        return merge_search::<false>(short, long, out);
    }
    unsafe { union_blocks::<L>(a, b, out) }
}

/// Merge network union; both inputs at least eight long.
#[inline(always)]
pub(crate) unsafe fn union_blocks<L: Lanes>(
    a: &[u16],
    b: &[u16],
    out: &mut [MaybeUninit<u16>],
) -> usize {
    unsafe {
        let (cap, dst) = (out.len(), out.as_mut_ptr().cast::<u16>());
        let mut k = 0;
        // Every emitted vector holds values of fully loaded blocks with at least eight loaded
        // values still pending, so the eight lane store stays within `a.len() + b.len()`.
        let (i, j, prev, hi) = merge_blocks(a, b, |prev, cur| {
            debug_assert!(k + 8 <= cap);
            k += store_unique::<L>(prev, cur, dst.add(k));
        });
        let mut buf = [0u16; 24];
        let (n, long) = pending(a, b, i, j, &[], hi, &mut buf);
        let last = lanes_of(prev)[7];
        union_tail(&buf[..n], long, out, k, last)
    }
}

#[inline(always)]
pub(crate) unsafe fn xor<L: Lanes>(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len() + b.len());
    let (short, long) = by_len(a, b);
    if short.len() < 8 || search_wins(short, long, SEARCH_RATIO_MERGE) {
        return merge_search::<true>(short, long, out);
    }
    unsafe { xor_blocks::<L>(a, b, out) }
}

/// Merge network symmetric difference; both inputs at least eight long.
#[inline(always)]
pub(crate) unsafe fn xor_blocks<L: Lanes>(
    a: &[u16],
    b: &[u16],
    out: &mut [MaybeUninit<u16>],
) -> usize {
    unsafe {
        let (cap, dst) = (out.len(), out.as_mut_ptr().cast::<u16>());
        let mut k = 0;
        let (i, j, prev, hi) = merge_blocks(a, b, |prev, cur| {
            debug_assert!(k + 8 <= cap);
            k += store_xor::<L>(prev, cur, dst.add(k));
        });
        // `prev[7]` is still undecided; `prev[6]` is its (decided) left neighbour.
        let lanes = lanes_of(prev);
        let mut buf = [0u16; 24];
        let (n, long) = pending(a, b, i, j, &lanes[7..], hi, &mut buf);
        xor_tail(&buf[..n], long, out, k, lanes[6])
    }
}
