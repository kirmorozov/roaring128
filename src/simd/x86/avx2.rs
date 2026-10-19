//! AVX2 kernels (`x86-64-v3`: AVX2, BMI1/BMI2, POPCNT and the SSE4.2 string instructions).
//!
//! * Bitmap operations stream 256-bit vectors and count bits with the Harley-Seal carry-save
//!   adder tree over Muła's `pshufb` nibble popcount (Kurz, Muła, Lemire, "Faster population
//!   counts using AVX2 instructions", 2018): one vector popcount per sixteen vectors. This is
//!   twice as fast as scalar `popcnt` even on Zen 3, where `popcnt` has four ports.
//! * `intersects` / `is_subset` test 1 KiB blocks with `vptest` and exit early.
//! * `count_runs` loads every vector a second time one word earlier (unaligned) for the carry.
//! * `bitmap_to_array` picks a decoder per block of 64 words from a sampled popcount: a
//!   `tzcnt` loop for sparse blocks (the usual case when a bitmap shrinks to an array), an
//!   unrolled `tzcnt` sequence with packed stores for medium density, and a per-byte index
//!   table (`vpmovzxbw` + 16-byte stores) for dense blocks.
//! * Array kernels: see [`super::arrays`] (`pcmpistrm`, bitonic merges, lockstep binary
//!   searches); lanes are packed with a `pshufb` lookup table.
//!
//! Every kernel is `unsafe`: it must only be called when [`super::has_avx2`] returns `true`.

#![allow(clippy::missing_safety_doc)]

use core::arch::x86_64::*;
use core::mem::MaybeUninit;

use super::arrays::{self, Lanes};
use super::decode_sparse_words;
use super::tables::{BIT_INDICES, PACK16};
use crate::simd::scalar::{self, AND, ANDNOT, OR, XOR, with_op};
use crate::simd::{BITMAP_WORDS, BitOp, Words};

/// Defines the public kernels of this backend with the tier's target features enabled.
macro_rules! kernels {
    ($($(#[$meta:meta])* fn $name:ident $(<$(const $c:ident: $ct:ty),*>)? ($($arg:ident : $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block)*) => {
        $(
            $(#[$meta])*
            #[target_feature(enable = "avx2,bmi1,bmi2,popcnt,sse4.2")]
            pub(crate) unsafe fn $name $(<$(const $c: $ct),*>)? ($($arg: $ty),*) $(-> $ret)? $body
        )*
    };
}

/// Number of 256-bit vectors in a bitmap container.
const VECTORS: usize = BITMAP_WORDS / 4;

// ---------------------------------------------------------------------------------------------
// Popcount building blocks
// ---------------------------------------------------------------------------------------------

/// Per 64-bit lane population count (Muła: nibble lookup with `pshufb`, summed by `psadbw`).
#[inline(always)]
unsafe fn popcnt_epi64(v: __m256i) -> __m256i {
    unsafe {
        let lookup = _mm256_setr_epi8(
            0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4, 0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2,
            3, 3, 4,
        );
        let nibble = _mm256_set1_epi8(0x0f);
        let lo = _mm256_and_si256(v, nibble);
        let hi = _mm256_and_si256(_mm256_srli_epi16::<4>(v), nibble);
        let bytes = _mm256_add_epi8(
            _mm256_shuffle_epi8(lookup, lo),
            _mm256_shuffle_epi8(lookup, hi),
        );
        _mm256_sad_epu8(bytes, _mm256_setzero_si256())
    }
}

/// Carry-save adder: returns `(carry, sum)` of the bitwise addition `a + b + c`.
#[inline(always)]
unsafe fn csa(a: __m256i, b: __m256i, c: __m256i) -> (__m256i, __m256i) {
    unsafe {
        let u = _mm256_xor_si256(a, b);
        (
            _mm256_or_si256(_mm256_and_si256(a, b), _mm256_and_si256(u, c)),
            _mm256_xor_si256(u, c),
        )
    }
}

#[inline(always)]
unsafe fn hsum_epi64(v: __m256i) -> u64 {
    unsafe {
        let s = _mm_add_epi64(_mm256_castsi256_si128(v), _mm256_extracti128_si256::<1>(v));
        (_mm_cvtsi128_si64(s) + _mm_extract_epi64::<1>(s)) as u64
    }
}

/// Total population count of the 256 vectors produced by `f(0)`, `f(1)`, ... (called in order).
#[inline(always)]
unsafe fn harley_seal(mut f: impl FnMut(usize) -> __m256i) -> u32 {
    unsafe {
        let z = _mm256_setzero_si256();
        let (mut total, mut ones, mut twos, mut fours, mut eights) = (z, z, z, z, z);
        let mut i = 0;
        while i < VECTORS {
            let (twos_a, o) = csa(ones, f(i), f(i + 1));
            let (twos_b, o) = csa(o, f(i + 2), f(i + 3));
            let (fours_a, t) = csa(twos, twos_a, twos_b);
            let (twos_a, o) = csa(o, f(i + 4), f(i + 5));
            let (twos_b, o) = csa(o, f(i + 6), f(i + 7));
            let (fours_b, t) = csa(t, twos_a, twos_b);
            let (eights_a, fo) = csa(fours, fours_a, fours_b);
            let (twos_a, o) = csa(o, f(i + 8), f(i + 9));
            let (twos_b, o) = csa(o, f(i + 10), f(i + 11));
            let (fours_a, t) = csa(t, twos_a, twos_b);
            let (twos_a, o) = csa(o, f(i + 12), f(i + 13));
            let (twos_b, o) = csa(o, f(i + 14), f(i + 15));
            let (fours_b, t) = csa(t, twos_a, twos_b);
            let (eights_b, fo) = csa(fo, fours_a, fours_b);
            let (sixteens, e) = csa(eights, eights_a, eights_b);
            total = _mm256_add_epi64(total, popcnt_epi64(sixteens));
            (ones, twos, fours, eights) = (o, t, fo, e);
            i += 16;
        }
        total = _mm256_slli_epi64::<4>(total);
        total = _mm256_add_epi64(total, _mm256_slli_epi64::<3>(popcnt_epi64(eights)));
        total = _mm256_add_epi64(total, _mm256_slli_epi64::<2>(popcnt_epi64(fours)));
        total = _mm256_add_epi64(total, _mm256_slli_epi64::<1>(popcnt_epi64(twos)));
        total = _mm256_add_epi64(total, popcnt_epi64(ones));
        hsum_epi64(total) as u32
    }
}

#[inline(always)]
unsafe fn load(p: *const u64, i: usize) -> __m256i {
    unsafe { _mm256_loadu_si256(p.add(4 * i).cast()) }
}

#[inline(always)]
unsafe fn store(p: *mut u64, i: usize, v: __m256i) {
    unsafe { _mm256_storeu_si256(p.add(4 * i).cast(), v) }
}

#[inline(always)]
unsafe fn op256<const OP: u8>(a: __m256i, b: __m256i) -> __m256i {
    unsafe {
        match OP {
            AND => _mm256_and_si256(a, b),
            OR => _mm256_or_si256(a, b),
            XOR => _mm256_xor_si256(a, b),
            _ => _mm256_andnot_si256(b, a),
        }
    }
}

/// `out = a op b` with the cardinality; `out` may equal `a` (in-place operation).
#[inline(always)]
unsafe fn op_store_card<const OP: u8>(a: *const u64, b: *const u64, out: *mut u64) -> u32 {
    unsafe {
        harley_seal(
            #[inline(always)]
            |i| {
                let w = op256::<OP>(load(a, i), load(b, i));
                store(out, i, w);
                w
            },
        )
    }
}

#[inline(always)]
unsafe fn op_card<const OP: u8>(a: &Words, b: &Words) -> u32 {
    let (a, b) = (a.as_ptr(), b.as_ptr());
    unsafe {
        harley_seal(
            #[inline(always)]
            |i| op256::<OP>(load(a, i), load(b, i)),
        )
    }
}

#[inline(always)]
unsafe fn op_nocard<const OP: u8>(a: &mut Words, b: &Words) {
    let (pa, pb) = (a.as_mut_ptr(), b.as_ptr());
    unsafe {
        // Four loads before four stores: the loads never wait behind a store.
        for i in (0..VECTORS).step_by(4) {
            let v: [_; 4] = core::array::from_fn(
                #[inline(always)]
                |u| op256::<OP>(load(pa, i + u), load(pb, i + u)),
            );
            for (u, v) in v.into_iter().enumerate() {
                store(pa, i + u, v);
            }
        }
    }
}

#[inline(always)]
unsafe fn op_into<const OP: u8>(a: &Words, b: &Words, out: &mut Words) -> u32 {
    unsafe { op_store_card::<OP>(a.as_ptr(), b.as_ptr(), out.as_mut_ptr()) }
}

#[inline(always)]
unsafe fn op_inplace<const OP: u8>(a: &mut Words, b: &Words) -> u32 {
    let pa = a.as_mut_ptr();
    unsafe { op_store_card::<OP>(pa, b.as_ptr(), pa) }
}

/// Whether `test(a, b)` is non-zero anywhere, checking blocks of 32 words.
#[inline(always)]
unsafe fn any_block(a: &Words, b: &Words, test: impl Fn(__m256i, __m256i) -> __m256i) -> bool {
    let (pa, pb) = (a.as_ptr(), b.as_ptr());
    unsafe {
        let mut i = 0;
        while i < VECTORS {
            let mut acc = test(load(pa, i), load(pb, i));
            for u in 1..8 {
                acc = _mm256_or_si256(acc, test(load(pa, i + u), load(pb, i + u)));
            }
            if _mm256_testz_si256(acc, acc) == 0 {
                return true;
            }
            i += 8;
        }
        false
    }
}

// ---------------------------------------------------------------------------------------------
// Bitmap decoding
// ---------------------------------------------------------------------------------------------

/// Words per density decision of `bitmap_to_array`.
const DECODE_BLOCK: usize = 64;
/// Blocks with at most this many bits (5.5 per word) are decoded one bit at a time: below that
/// density the plain loop is as fast as any vector decoder.
const SPARSE_BLOCK: u32 = 11 * DECODE_BLOCK as u32 / 2;
/// Blocks with at most this many bits (7 per word) use the unrolled `tzcnt` decoder; denser
/// blocks use the byte table.
const MEDIUM_BLOCK: u32 = 7 * DECODE_BLOCK as u32;

/// Four set bit indices of `x` (lowest first, 64 when exhausted) plus `base` in each 16-bit
/// lane of the result; clears them from `x`.
#[inline(always)]
unsafe fn four_indices(x: &mut u64, base: u64) -> u64 {
    unsafe {
        let mut packed = 0;
        for lane in 0..4 {
            packed |= _tzcnt_u64(*x) << (16 * lane);
            *x = _blsr_u64(*x);
        }
        // A lane only carries into the next one when it is exhausted (64 + 65472 in the last
        // word), and then all the lanes above it are exhausted scratch as well.
        packed.wrapping_add(base * 0x0001_0001_0001_0001)
    }
}

/// Writes the set bit indices of `w` plus `base` to `dst`: eight values with a fixed unrolled
/// sequence and two 64-bit stores (values past the popcount are scratch), then a loop for the
/// rest. Writes `max(8, popcount)` lanes.
#[inline(always)]
unsafe fn decode_word_unrolled(w: u64, base: u32, dst: *mut u16) {
    unsafe {
        let mut x = w;
        let lo = four_indices(&mut x, base as u64);
        let hi = four_indices(&mut x, base as u64);
        dst.cast::<u64>().write_unaligned(lo);
        dst.add(4).cast::<u64>().write_unaligned(hi);
        let mut k = 8;
        while x != 0 {
            *dst.add(k) = (base + _tzcnt_u64(x) as u32) as u16;
            x = _blsr_u64(x);
            k += 1;
        }
    }
}

/// Writes the set bit indices of `w` plus `base` to `dst`, one byte at a time from the index
/// table (eight lanes stored per byte, at most 64 lanes in total).
#[inline(always)]
unsafe fn decode_word_table(w: u64, base: u32, dst: *mut u16) {
    unsafe {
        let table = &BIT_INDICES.0;
        let mut pos = 0;
        let mut vbase = _mm_set1_epi16(base as i16);
        let step = _mm_set1_epi16(8);
        for byte in w.to_le_bytes() {
            let idx = _mm_loadl_epi64(table.get_unchecked(byte as usize).as_ptr().cast());
            let v = _mm_add_epi16(_mm_cvtepu8_epi16(idx), vbase);
            _mm_storeu_si128(dst.add(pos).cast(), v);
            pos += byte.count_ones() as usize;
            vbase = _mm_add_epi16(vbase, step);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Array lane primitives
// ---------------------------------------------------------------------------------------------

/// AVX2 lane primitives: `pshufb` packing table; partial loads and stores go through a small
/// stack buffer (the trait's default methods).
pub(crate) struct Avx2Lanes;

impl Lanes for Avx2Lanes {
    #[inline(always)]
    unsafe fn eq_mask(a: __m128i, b: __m128i) -> u32 {
        unsafe {
            let eq = _mm_cmpeq_epi16(a, b);
            _mm_movemask_epi8(_mm_packs_epi16(eq, _mm_setzero_si128())) as u32
        }
    }

    #[inline(always)]
    unsafe fn pack(v: __m128i, keep: u32) -> __m128i {
        debug_assert!(keep < 256);
        unsafe {
            let control = PACK16.0.get_unchecked(keep as usize);
            _mm_shuffle_epi8(v, _mm_loadu_si128(control.as_ptr().cast()))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Kernels
// ---------------------------------------------------------------------------------------------

kernels! {
    fn bitmap_popcount(a: &Words) -> u32 {
        let p = a.as_ptr();
        unsafe { harley_seal(#[inline(always)] |i| load(p, i)) }
    }

    fn bitmap_op(op: BitOp, a: &Words, b: &Words, out: &mut Words) -> u32 {
        unsafe { with_op!(op, op_into(a, b, out)) }
    }

    fn bitmap_op_inplace(op: BitOp, a: &mut Words, b: &Words) -> u32 {
        unsafe { with_op!(op, op_inplace(a, b)) }
    }

    fn bitmap_op_inplace_nocard(op: BitOp, a: &mut Words, b: &Words) {
        unsafe { with_op!(op, op_nocard(a, b)) }
    }

    fn bitmap_op_card(op: BitOp, a: &Words, b: &Words) -> u32 {
        unsafe { with_op!(op, op_card(a, b)) }
    }

    fn bitmap_intersects(a: &Words, b: &Words) -> bool {
        unsafe { any_block(a, b, #[inline(always)] |x, y| _mm256_and_si256(x, y)) }
    }

    fn bitmap_is_subset(a: &Words, b: &Words) -> bool {
        unsafe { !any_block(a, b, #[inline(always)] |x, y| _mm256_andnot_si256(y, x)) }
    }

    fn bitmap_to_array(a: &Words, out: &mut [MaybeUninit<u16>]) -> usize {
        if out.len() <= BITMAP_WORDS {
            // At most one value per word on average (`out` bounds the cardinality): a single
            // pass of the sparse loop, without per-block decisions.
            return decode_sparse_words(a, 0, out, 0);
        }
        let len = out.len();
        let dst = out.as_mut_ptr().cast::<u16>();
        let mut written = 0usize;
        let mut wi = 0;
        // The decoder is chosen per block of words from the block's density, estimated from
        // eight of its 64 words: cheap next to the sparse loop, resolved early by the branch
        // predictor and stable on uniform data. (Using the previous block's exact output count
        // instead makes a mispredicted choice resolve only after the previous block is done,
        // which costs far more.)
        'blocks: while wi < BITMAP_WORDS {
            let block = &a[wi..wi + DECODE_BLOCK];
            let card = 8 * (0..8).map(|i| block[9 * i].count_ones()).sum::<u32>();
            if card <= SPARSE_BLOCK {
                // Few bits per word: one `tzcnt` per value, exact stores.
                written = decode_sparse_words(block, wi, out, written);
            } else {
                for (t, &w) in block.iter().enumerate() {
                    // A word writes at most 64 lanes past `written` (the last 8-lane store of
                    // the table decoder starts at most 56 lanes in); near the end of `out` the
                    // scalar tail takes over.
                    if len - written < 64 {
                        wi += t;
                        break 'blocks;
                    }
                    let base = ((wi + t) * 64) as u32;
                    unsafe {
                        if card <= MEDIUM_BLOCK {
                            decode_word_unrolled(w, base, dst.add(written));
                        } else {
                            decode_word_table(w, base, dst.add(written));
                        }
                    }
                    written += w.count_ones() as usize;
                }
            }
            wi += DECODE_BLOCK;
        }
        scalar::bitmap_to_array_from(a, wi, out, written)
    }

    fn bitmap_count_runs(a: &Words) -> u32 {
        let p = a.as_ptr();
        unsafe {
            harley_seal(#[inline(always)] |i| {
                let v = load(p, i);
                // The word preceding each lane; zero before the first word.
                let prev = if i == 0 {
                    _mm256_blend_epi32::<0b0000_0011>(
                        _mm256_permute4x64_epi64::<0b10_01_00_00>(v),
                        _mm256_setzero_si256(),
                    )
                } else {
                    _mm256_loadu_si256(p.add(4 * i - 1).cast())
                };
                let starts = _mm256_or_si256(_mm256_slli_epi64::<1>(v), _mm256_srli_epi64::<63>(prev));
                _mm256_andnot_si256(starts, v)
            })
        }
    }

    fn array_intersect(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
        unsafe { arrays::intersect::<Avx2Lanes>(a, b, out) }
    }

    fn array_intersect_card(a: &[u16], b: &[u16]) -> usize {
        unsafe { arrays::intersect_card::<Avx2Lanes>(a, b) }
    }

    fn array_intersects(a: &[u16], b: &[u16]) -> bool {
        unsafe { arrays::intersects::<Avx2Lanes>(a, b) }
    }

    fn array_union(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
        unsafe { arrays::union::<Avx2Lanes>(a, b, out) }
    }

    fn array_difference(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
        unsafe { arrays::difference::<Avx2Lanes>(a, b, out) }
    }

    fn array_xor(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
        unsafe { arrays::xor::<Avx2Lanes>(a, b, out) }
    }
}
