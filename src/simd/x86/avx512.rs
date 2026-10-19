//! AVX-512 kernels (F/BW/VL/DQ/VPOPCNTDQ/BITALG/VBMI2 on top of the AVX2 tier; Ice Lake,
//! Zen 4 and newer).
//!
//! * Bitmap operations stream 512-bit vectors, four per iteration, and count bits with
//!   `vpopcntq` into independent accumulators.
//! * `intersects` / `is_subset` fold 1 KiB blocks with `vpternlogq` and exit early.
//! * `bitmap_to_array` compresses index vectors with `vpcompressw` (32 bits at a time, no
//!   data-dependent branches); full width stores while there is room, masked stores at the
//!   end of the output. Blocks of 16 words with at most one bit per word use a `tzcnt` loop.
//! * `count_runs` uses one `vpternlogq` per vector for `w & !(w << 1 | carry)`.
//! * Array kernels: see [`super::arrays`]; lanes are packed with `vpcompressw`, partial blocks
//!   are read and written with masked loads and stores.
//!
//! Every kernel is `unsafe`: it must only be called when [`super::has_avx512`] returns `true`.

#![allow(clippy::missing_safety_doc)]

use core::arch::x86_64::*;
use core::mem::MaybeUninit;

use super::arrays::{self, Lanes};
use super::decode_sparse_words;
use crate::simd::scalar::{AND, ANDNOT, OR, XOR, with_op};
use crate::simd::{BITMAP_WORDS, BitOp, Words};

/// Defines the public kernels of this backend with the tier's target features enabled.
macro_rules! kernels {
    ($($(#[$meta:meta])* fn $name:ident ($($arg:ident : $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block)*) => {
        $(
            $(#[$meta])*
            #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512dq,avx512vpopcntdq,avx512bitalg,avx512vbmi2,avx2,bmi1,bmi2,popcnt,sse4.2")]
            pub(crate) unsafe fn $name ($($arg: $ty),*) $(-> $ret)? $body
        )*
    };
}

/// Number of 512-bit vectors in a bitmap container.
const VECTORS: usize = BITMAP_WORDS / 8;

#[inline(always)]
unsafe fn load(p: *const u64, i: usize) -> __m512i {
    unsafe { _mm512_loadu_si512(p.add(8 * i).cast()) }
}

#[inline(always)]
unsafe fn store(p: *mut u64, i: usize, v: __m512i) {
    unsafe { _mm512_storeu_si512(p.add(8 * i).cast(), v) }
}

#[inline(always)]
unsafe fn op512<const OP: u8>(a: __m512i, b: __m512i) -> __m512i {
    unsafe {
        match OP {
            AND => _mm512_and_si512(a, b),
            OR => _mm512_or_si512(a, b),
            XOR => _mm512_xor_si512(a, b),
            _ => _mm512_andnot_si512(b, a),
        }
    }
}

/// Sums the population counts of the vectors `f(0)`, `f(1)`, ... (called in order).
#[inline(always)]
unsafe fn popcount_sum(mut f: impl FnMut(usize) -> __m512i) -> u32 {
    unsafe {
        let z = _mm512_setzero_si512();
        let mut acc = [z; 4];
        let mut i = 0;
        while i < VECTORS {
            for (u, acc) in acc.iter_mut().enumerate() {
                *acc = _mm512_add_epi64(*acc, _mm512_popcnt_epi64(f(i + u)));
            }
            i += 4;
        }
        let sum = _mm512_add_epi64(
            _mm512_add_epi64(acc[0], acc[1]),
            _mm512_add_epi64(acc[2], acc[3]),
        );
        _mm512_reduce_add_epi64(sum) as u32
    }
}

/// `out = a op b` with the cardinality; `out` may equal `a` (in-place operation).
#[inline(always)]
unsafe fn op_store_card<const OP: u8>(a: *const u64, b: *const u64, out: *mut u64) -> u32 {
    unsafe {
        popcount_sum(
            #[inline(always)]
            |i| {
                let w = op512::<OP>(load(a, i), load(b, i));
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
        popcount_sum(
            #[inline(always)]
            |i| op512::<OP>(load(a, i), load(b, i)),
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
                |u| op512::<OP>(load(pa, i + u), load(pb, i + u)),
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

/// Whether `acc | f(a, b)` (a `vpternlogq` function with `acc` as first operand) is non-zero
/// anywhere, checking blocks of 32 words.
#[inline(always)]
unsafe fn any_block<const IMM: i32>(a: &Words, b: &Words) -> bool {
    let (pa, pb) = (a.as_ptr(), b.as_ptr());
    unsafe {
        let mut i = 0;
        while i < VECTORS {
            let mut acc = _mm512_setzero_si512();
            for u in 0..4 {
                acc = _mm512_ternarylogic_epi64::<IMM>(acc, load(pa, i + u), load(pb, i + u));
            }
            if _mm512_test_epi64_mask(acc, acc) != 0 {
                return true;
            }
            i += 4;
        }
        false
    }
}

/// Words per density decision of `bitmap_to_array`.
const DECODE_BLOCK: usize = 64;
/// Blocks with at most this many bits (one per word) are decoded one bit at a time.
const SPARSE_BLOCK: u32 = DECODE_BLOCK as u32;

/// `vpternlogq` truth tables with operands (A, B, C) = (acc, a, b).
const ACC_OR_AND: i32 = 0xF8; // A | (B & C)
const ACC_OR_ANDNOT: i32 = 0xF4; // A | (B & !C)
const AND_NOR: i32 = 0x10; // A & !(B | C)

/// Mask of the `n <= 32` lowest bits.
#[inline(always)]
unsafe fn low_bits32(n: u32) -> u32 {
    unsafe { _bzhi_u32(u32::MAX, n) }
}

// ---------------------------------------------------------------------------------------------
// Array lane primitives
// ---------------------------------------------------------------------------------------------

/// AVX-512 lane primitives: `vpcompressw` packing, masked partial loads and stores.
pub(crate) struct Avx512Lanes;

impl Lanes for Avx512Lanes {
    #[inline(always)]
    unsafe fn eq_mask(a: __m128i, b: __m128i) -> u32 {
        unsafe { _mm_cmpeq_epi16_mask(a, b) as u32 }
    }

    #[inline(always)]
    unsafe fn pack(v: __m128i, keep: u32) -> __m128i {
        unsafe { _mm_maskz_compress_epi16(keep as __mmask8, v) }
    }

    #[inline(always)]
    unsafe fn load_partial(p: *const u16, n: usize) -> __m128i {
        debug_assert!(n < 8);
        unsafe { _mm_maskz_loadu_epi16(low_bits32(n as u32) as __mmask8, p.cast()) }
    }

    #[inline(always)]
    unsafe fn store_first(v: __m128i, n: usize, out: &mut [MaybeUninit<u16>], k: usize) {
        debug_assert!(k + n <= out.len());
        unsafe {
            let dst = out.as_mut_ptr().add(k).cast::<i16>();
            if out.len() - k >= 8 {
                _mm_storeu_si128(dst.cast(), v);
            } else {
                _mm_mask_storeu_epi16(dst, low_bits32(n as u32) as __mmask8, v);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Kernels
// ---------------------------------------------------------------------------------------------

kernels! {
    fn bitmap_popcount(a: &Words) -> u32 {
        let p = a.as_ptr();
        unsafe { popcount_sum(#[inline(always)] |i| load(p, i)) }
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
        unsafe { any_block::<ACC_OR_AND>(a, b) }
    }

    fn bitmap_is_subset(a: &Words, b: &Words) -> bool {
        unsafe { !any_block::<ACC_OR_ANDNOT>(a, b) }
    }

    fn bitmap_to_array(a: &Words, out: &mut [MaybeUninit<u16>]) -> usize {
        if out.len() <= BITMAP_WORDS {
            // At most one value per word on average (`out` bounds the cardinality): a single
            // pass of the sparse loop, without per-block decisions.
            return decode_sparse_words(a, 0, out, 0);
        }
        let len = out.len();
        let dst = out.as_mut_ptr().cast::<i16>();
        let iota = _mm512_set_epi16(
            31, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13, 12, 11,
            10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
        );
        let half = _mm512_set1_epi16(32);
        let mut written = 0usize;
        let mut wi = 0;
        while wi < BITMAP_WORDS {
            let block = &a[wi..wi + DECODE_BLOCK];
            // Density estimated from eight of the 64 words (see the AVX2 decoder).
            let card = 8 * (0..8).map(|i| block[9 * i].count_ones()).sum::<u32>();
            if card <= SPARSE_BLOCK {
                // At most one bit per word on average: one `tzcnt` per value.
                written = decode_sparse_words(block, wi, out, written);
            } else {
                let mut lo_idx = _mm512_add_epi16(iota, _mm512_set1_epi16((wi * 64) as i16));
                for &w in block {
                    // Branch free: every word costs two compresses whatever its density.
                    let (lo, hi) = (w as u32, (w >> 32) as u32);
                    let hi_idx = _mm512_add_epi16(lo_idx, half);
                    let vlo = _mm512_maskz_compress_epi16(lo, lo_idx);
                    let vhi = _mm512_maskz_compress_epi16(hi, hi_idx);
                    let (nlo, nhi) = (lo.count_ones() as usize, hi.count_ones() as usize);
                    unsafe {
                        if len - written >= 64 {
                            _mm512_storeu_si512(dst.add(written).cast(), vlo);
                            _mm512_storeu_si512(dst.add(written + nlo).cast(), vhi);
                        } else {
                            // `out` holds exactly the remaining values: masked stores.
                            let (mlo, mhi) = (low_bits32(nlo as u32), low_bits32(nhi as u32));
                            _mm512_mask_storeu_epi16(dst.add(written), mlo, vlo);
                            _mm512_mask_storeu_epi16(dst.add(written + nlo), mhi, vhi);
                        }
                    }
                    written += nlo + nhi;
                    lo_idx = _mm512_add_epi16(hi_idx, half);
                }
            }
            wi += DECODE_BLOCK;
        }
        written
    }

    fn bitmap_count_runs(a: &Words) -> u32 {
        let p = a.as_ptr();
        unsafe {
            popcount_sum(#[inline(always)] |i| {
                let v = load(p, i);
                // The word preceding each lane; zero before the first word.
                let prev = if i == 0 {
                    _mm512_alignr_epi64::<7>(v, _mm512_setzero_si512())
                } else {
                    _mm512_loadu_si512(p.add(8 * i - 1).cast())
                };
                _mm512_ternarylogic_epi64::<AND_NOR>(
                    v,
                    _mm512_slli_epi64::<1>(v),
                    _mm512_srli_epi64::<63>(prev),
                )
            })
        }
    }

    fn array_intersect(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
        unsafe { arrays::intersect::<Avx512Lanes>(a, b, out) }
    }

    fn array_intersect_card(a: &[u16], b: &[u16]) -> usize {
        unsafe { arrays::intersect_card::<Avx512Lanes>(a, b) }
    }

    fn array_intersects(a: &[u16], b: &[u16]) -> bool {
        unsafe { arrays::intersects::<Avx512Lanes>(a, b) }
    }

    fn array_union(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
        unsafe { arrays::union::<Avx512Lanes>(a, b, out) }
    }

    fn array_difference(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
        unsafe { arrays::difference::<Avx512Lanes>(a, b, out) }
    }

    fn array_xor(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
        unsafe { arrays::xor::<Avx512Lanes>(a, b, out) }
    }
}
