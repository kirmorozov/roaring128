//! Bitmap container kernels (65536 bits = 512 q-registers).
//!
//! The container is streamed in 64-byte chunks (`ld1`/`st1` of four registers). Cardinalities are
//! computed with `cnt` (per-byte popcount); four `cnt` results are summed with byte adds (at most
//! 32 per lane) and folded into `u16` lanes with `uadalp`. A whole container holds 65536 bits, so a
//! `u16x8` accumulator (at most 8192 per lane) can never overflow and no periodic widening is
//! needed.

use core::arch::aarch64::*;
use core::mem::MaybeUninit;

use super::tables::{BIT_POSITIONS, POPCOUNT_X2};
use crate::simd::scalar::{AND, ANDNOT, OR, XOR};
use crate::simd::{BITMAP_WORDS, Words};

/// `u64` words per 64-byte chunk.
const CHUNK_WORDS: usize = 8;

/// Loads the 64 bytes at `p`.
///
/// # Safety
/// `p` must be valid for reading 8 words.
#[inline]
#[target_feature(enable = "neon")]
unsafe fn load4(p: *const u64) -> uint8x16x4_t {
    // SAFETY: guaranteed by the caller.
    unsafe { vld1q_u8_x4(p.cast()) }
}

/// Stores 64 bytes at `p`.
///
/// # Safety
/// `p` must be valid for writing 8 words.
#[inline]
#[target_feature(enable = "neon")]
unsafe fn store4(p: *mut u64, v: uint8x16x4_t) {
    // SAFETY: guaranteed by the caller.
    unsafe { vst1q_u8_x4(p.cast(), v) }
}

#[inline]
#[target_feature(enable = "neon")]
fn op<const OP: u8>(a: uint8x16_t, b: uint8x16_t) -> uint8x16_t {
    match OP {
        AND => vandq_u8(a, b),
        OR => vorrq_u8(a, b),
        XOR => veorq_u8(a, b),
        _ => vbicq_u8(a, b),
    }
}

#[inline]
#[target_feature(enable = "neon")]
fn op4<const OP: u8>(a: uint8x16x4_t, b: uint8x16x4_t) -> uint8x16x4_t {
    uint8x16x4_t(
        op::<OP>(a.0, b.0),
        op::<OP>(a.1, b.1),
        op::<OP>(a.2, b.2),
        op::<OP>(a.3, b.3),
    )
}

/// Per-byte popcounts of 64 bytes, summed lane-wise (at most 32 per lane).
#[inline]
#[target_feature(enable = "neon")]
fn cnt4(v: uint8x16x4_t) -> uint8x16_t {
    vaddq_u8(
        vaddq_u8(vcntq_u8(v.0), vcntq_u8(v.1)),
        vaddq_u8(vcntq_u8(v.2), vcntq_u8(v.3)),
    )
}

#[inline]
#[target_feature(enable = "neon")]
fn or4(v: uint8x16x4_t) -> uint8x16_t {
    vorrq_u8(vorrq_u8(v.0, v.1), vorrq_u8(v.2, v.3))
}

#[inline]
#[target_feature(enable = "neon")]
fn any_nonzero(v: uint8x16_t) -> bool {
    let v = vreinterpretq_u32_u8(v);
    vgetq_lane_u64::<0>(vreinterpretq_u64_u32(vpmaxq_u32(v, v))) != 0
}

/// Sum of the two accumulators (each lane is at most 8192 in total).
#[inline]
#[target_feature(enable = "neon")]
fn total(acc0: uint16x8_t, acc1: uint16x8_t) -> u32 {
    vaddlvq_u16(vaddq_u16(acc0, acc1))
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn popcount(a: &Words) -> u32 {
    let pa = a.as_ptr();
    let (mut acc0, mut acc1) = (vdupq_n_u16(0), vdupq_n_u16(0));
    for w in (0..BITMAP_WORDS).step_by(2 * CHUNK_WORDS) {
        // SAFETY: `w + 16 <= BITMAP_WORDS`.
        let (x, y) = unsafe { (load4(pa.add(w)), load4(pa.add(w + CHUNK_WORDS))) };
        acc0 = vpadalq_u8(acc0, cnt4(x));
        acc1 = vpadalq_u8(acc1, cnt4(y));
    }
    total(acc0, acc1)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn op_into<const OP: u8>(a: &Words, b: &Words, out: &mut Words) -> u32 {
    let (pa, pb, po) = (a.as_ptr(), b.as_ptr(), out.as_mut_ptr());
    let (mut acc0, mut acc1) = (vdupq_n_u16(0), vdupq_n_u16(0));
    for w in (0..BITMAP_WORDS).step_by(2 * CHUNK_WORDS) {
        // SAFETY: `w + 16 <= BITMAP_WORDS` for all three containers.
        unsafe {
            let x = op4::<OP>(load4(pa.add(w)), load4(pb.add(w)));
            let y = op4::<OP>(
                load4(pa.add(w + CHUNK_WORDS)),
                load4(pb.add(w + CHUNK_WORDS)),
            );
            store4(po.add(w), x);
            store4(po.add(w + CHUNK_WORDS), y);
            acc0 = vpadalq_u8(acc0, cnt4(x));
            acc1 = vpadalq_u8(acc1, cnt4(y));
        }
    }
    total(acc0, acc1)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn op_inplace<const OP: u8>(a: &mut Words, b: &Words) -> u32 {
    let (pa, pb) = (a.as_mut_ptr(), b.as_ptr());
    let (mut acc0, mut acc1) = (vdupq_n_u16(0), vdupq_n_u16(0));
    for w in (0..BITMAP_WORDS).step_by(2 * CHUNK_WORDS) {
        // SAFETY: `w + 16 <= BITMAP_WORDS` for both containers.
        unsafe {
            let x = op4::<OP>(load4(pa.add(w)), load4(pb.add(w)));
            let y = op4::<OP>(
                load4(pa.add(w + CHUNK_WORDS)),
                load4(pb.add(w + CHUNK_WORDS)),
            );
            store4(pa.add(w), x);
            store4(pa.add(w + CHUNK_WORDS), y);
            acc0 = vpadalq_u8(acc0, cnt4(x));
            acc1 = vpadalq_u8(acc1, cnt4(y));
        }
    }
    total(acc0, acc1)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn op_inplace_nocard<const OP: u8>(a: &mut Words, b: &Words) {
    let (pa, pb) = (a.as_mut_ptr(), b.as_ptr());
    for w in (0..BITMAP_WORDS).step_by(2 * CHUNK_WORDS) {
        // SAFETY: `w + 16 <= BITMAP_WORDS` for both containers.
        unsafe {
            let x = op4::<OP>(load4(pa.add(w)), load4(pb.add(w)));
            let y = op4::<OP>(
                load4(pa.add(w + CHUNK_WORDS)),
                load4(pb.add(w + CHUNK_WORDS)),
            );
            store4(pa.add(w), x);
            store4(pa.add(w + CHUNK_WORDS), y);
        }
    }
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn op_card<const OP: u8>(a: &Words, b: &Words) -> u32 {
    let (pa, pb) = (a.as_ptr(), b.as_ptr());
    let (mut acc0, mut acc1) = (vdupq_n_u16(0), vdupq_n_u16(0));
    for w in (0..BITMAP_WORDS).step_by(2 * CHUNK_WORDS) {
        // SAFETY: `w + 16 <= BITMAP_WORDS` for both containers.
        let (x, y) = unsafe {
            (
                op4::<OP>(load4(pa.add(w)), load4(pb.add(w))),
                op4::<OP>(
                    load4(pa.add(w + CHUNK_WORDS)),
                    load4(pb.add(w + CHUNK_WORDS)),
                ),
            )
        };
        acc0 = vpadalq_u8(acc0, cnt4(x));
        acc1 = vpadalq_u8(acc1, cnt4(y));
    }
    total(acc0, acc1)
}

/// Whether `a op b` has any bit set, checked every 128 bytes.
#[inline]
#[target_feature(enable = "neon")]
fn any_op<const OP: u8>(a: &Words, b: &Words) -> bool {
    let (pa, pb) = (a.as_ptr(), b.as_ptr());
    for w in (0..BITMAP_WORDS).step_by(2 * CHUNK_WORDS) {
        // SAFETY: `w + 16 <= BITMAP_WORDS` for both containers.
        let (x, y) = unsafe {
            (
                op4::<OP>(load4(pa.add(w)), load4(pb.add(w))),
                op4::<OP>(
                    load4(pa.add(w + CHUNK_WORDS)),
                    load4(pb.add(w + CHUNK_WORDS)),
                ),
            )
        };
        if any_nonzero(vorrq_u8(or4(x), or4(y))) {
            return true;
        }
    }
    false
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn intersects(a: &Words, b: &Words) -> bool {
    any_op::<{ AND }>(a, b)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn is_subset(a: &Words, b: &Words) -> bool {
    !any_op::<{ ANDNOT }>(a, b)
}

/// Run starts of the two words of `w`: set bits whose lower neighbour is clear. `prev` holds the
/// word preceding each lane.
#[inline]
#[target_feature(enable = "neon")]
fn run_starts(w: uint64x2_t, prev: uint64x2_t) -> uint8x16_t {
    // (w << 1) | (prev >> 63): `sri` inserts the top bit of `prev` into the (zero) low bit.
    let lower = vsriq_n_u64::<63>(vshlq_n_u64::<1>(w), prev);
    vreinterpretq_u8_u64(vbicq_u64(w, lower))
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn count_runs(a: &Words) -> u32 {
    let pa = a.as_ptr();
    let mut prev = vdupq_n_u64(0);
    let mut acc = vdupq_n_u16(0);
    for w in (0..BITMAP_WORDS).step_by(CHUNK_WORDS) {
        // SAFETY: `w + 8 <= BITMAP_WORDS`.
        let v = unsafe { vld1q_u64_x4(pa.add(w)) };
        let starts = uint8x16x4_t(
            run_starts(v.0, vextq_u64::<1>(prev, v.0)),
            run_starts(v.1, vextq_u64::<1>(v.0, v.1)),
            run_starts(v.2, vextq_u64::<1>(v.1, v.2)),
            run_starts(v.3, vextq_u64::<1>(v.2, v.3)),
        );
        prev = v.3;
        acc = vpadalq_u8(acc, cnt4(starts));
    }
    vaddlvq_u16(acc)
}

/// A chunk is decoded with the byte table when the previous chunk had at least this many set
/// bits (about 8% density), bit by bit otherwise. The table path costs 7 instructions per byte
/// whatever the density; the bit loop about 7 per set bit plus 2 per word.
const DENSE_CHUNK_BITS: usize = 40;

/// Output room that lets a chunk be decoded without counting its bits first. The table path
/// stores 8 lanes per byte after the values of the bytes before it, so it ends at most
/// `min(n + 8, 512)` values after the start of the chunk (`n` set bits).
const CHUNK_ROOM: usize = CHUNK_WORDS * 64;

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn to_array(a: &Words, out: &mut [MaybeUninit<u16>]) -> usize {
    let cap = out.len();
    let mut k = 0;
    let mut dense = false;
    for (chunk, words) in a.as_chunks::<CHUNK_WORDS>().0.iter().enumerate() {
        let base = (chunk * CHUNK_WORDS * 64) as u16;
        let start = k;
        let room = cap - k;
        let use_table = if room >= CHUNK_ROOM {
            dense
        } else {
            // Close to the end of the output: count the chunk first.
            // SAFETY: the chunk holds 8 words.
            let n = vaddlvq_u8(cnt4(unsafe { load4(words.as_ptr()) })) as usize;
            assert!(n <= room, "bitmap_to_array: output buffer too small");
            dense && room - n >= 8
        };
        let dst = out.as_mut_ptr().cast::<u16>();
        k = if use_table {
            // SAFETY: the table path writes up to `k + min(n + 8, 512) <= cap`.
            unsafe { decode_dense(words, base, dst, k) }
        } else {
            // SAFETY: exactly `n <= cap - k` values are written.
            unsafe { decode_sparse(words, base, dst, k) }
        };
        // Densities are usually similar in neighbouring chunks; predicting from the previous
        // chunk avoids counting every chunk up front.
        dense = k - start >= DENSE_CHUNK_BITS;
    }
    k
}

/// Byte table path: every byte stores the 8 lanes `base + BIT_POSITIONS[byte]` (only the first
/// popcount lanes are kept) and advances by its popcount. Branch-free.
///
/// # Safety
/// `dst[k..k + min(n + 8, 512)]` must be valid for writes, `n` being the number of set bits of
/// the chunk.
#[inline]
#[target_feature(enable = "neon")]
unsafe fn decode_dense(words: &[u64; CHUNK_WORDS], base: u16, dst: *mut u16, k: usize) -> usize {
    let eight = vdupq_n_u16(8);
    let mut base = vdupq_n_u16(base);
    // The position is kept in bytes and advanced with a doubled popcount table, so the only
    // loop-carried dependency is one plain `add` per byte (`str q` cannot scale by 2).
    let dst = dst.cast::<u8>();
    let mut offset = 2 * k;
    for &w in words {
        for byte in 0..8 {
            let b = (w >> (8 * byte)) as u8 as usize;
            // SAFETY: the table entry has 8 bytes; this store ends 8 values after the values of
            // the previous bytes, so before `k + min(n + 8, 512)` as guaranteed by the caller.
            unsafe {
                let pos = vld1_u8(BIT_POSITIONS.0[b].as_ptr());
                vst1q_u16(dst.add(offset).cast(), vaddw_u8(base, pos));
            }
            offset += POPCOUNT_X2.0[b] as usize;
            base = vaddq_u16(base, eight);
        }
    }
    offset / 2
}

/// Bit loop: `rbit` + `clz` per set bit, zero words skipped.
///
/// # Safety
/// `dst[k..k + n]` must be valid for writes, `n` being the number of set bits of the chunk.
#[inline]
#[target_feature(enable = "neon")]
unsafe fn decode_sparse(words: &[u64; CHUNK_WORDS], base: u16, dst: *mut u16, k: usize) -> usize {
    // SAFETY: `n` values are written from `k`.
    let mut p = unsafe { dst.add(k) };
    let mut base = base;
    for &w in words {
        // Bit-reversed once, so every set bit costs `clz` + clearing it by its index (a plain
        // `w & (w - 1)` loop makes LLVM count the bits up front for a trip count).
        let mut r = w.reverse_bits();
        while r != 0 {
            let lz = r.leading_zeros();
            // SAFETY: as above.
            unsafe {
                p.write(base | lz as u16);
                p = p.add(1);
            }
            r ^= (1 << 63) >> lz;
        }
        base = base.wrapping_add(64);
    }
    // SAFETY: `p` was derived from `dst` and only moved forward.
    k + unsafe { p.offset_from_unsigned(dst.add(k)) }
}
