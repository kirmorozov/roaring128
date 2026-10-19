//! Sorted `u16` array kernels.
//!
//! * Intersection and difference compare blocks of 8 values all-pairs (`a` against the 8
//!   rotations of `b`), advance the block with the smaller maximum without branching, and compact
//!   the selected lanes with `tbl` and a 256-entry shuffle table.
//! * Union and symmetric difference stream both inputs through a bitonic merge network
//!   (`umin`/`umax` + `zip`/`trn`) that emits 8 sorted values per step. Duplicates are always
//!   adjacent in the merged stream and are removed by comparing every value with its neighbours
//!   before the compacting store.
//! * When one side is short (fewer than 8 values left, or a size ratio of at least
//!   [`GALLOP_RATIO`](crate::simd::GALLOP_RATIO)) every value of the short side is located in the
//!   long one by galloping, and untouched stretches of the long side are block-copied.
//!
//! Stores of full vectors are only issued where at least 8 lanes of the output are in bounds;
//! everything else goes through bounds-checked slice accesses.

use core::arch::aarch64::*;
use core::mem::MaybeUninit;

use super::tables::{POPCOUNT, SHUFFLE};
use crate::simd::scalar::{self, gallop, skewed};
use crate::simd::{GALLOP_RATIO, assume_init, check_out};

/// `u16` lanes per vector.
const LANES: usize = 8;

/// Lane `i` holds `1 << i`.
const LANE_BITS: [u16; LANES] = [1, 2, 4, 8, 16, 32, 64, 128];

/// Loads 8 values.
///
/// # Safety
/// `p` must be valid for reading 8 values.
#[inline]
#[target_feature(enable = "neon")]
unsafe fn load(p: *const u16) -> uint16x8_t {
    // SAFETY: guaranteed by the caller.
    unsafe { vld1q_u16(p) }
}

/// Lanes of `a` that are equal to some lane of `b`.
#[inline]
#[target_feature(enable = "neon")]
fn matrix_eq(a: uint16x8_t, b: uint16x8_t) -> uint16x8_t {
    let e0 = vceqq_u16(a, b);
    let e1 = vceqq_u16(a, vextq_u16::<1>(b, b));
    let e2 = vceqq_u16(a, vextq_u16::<2>(b, b));
    let e3 = vceqq_u16(a, vextq_u16::<3>(b, b));
    let e4 = vceqq_u16(a, vextq_u16::<4>(b, b));
    let e5 = vceqq_u16(a, vextq_u16::<5>(b, b));
    let e6 = vceqq_u16(a, vextq_u16::<6>(b, b));
    let e7 = vceqq_u16(a, vextq_u16::<7>(b, b));
    vorrq_u16(
        vorrq_u16(vorrq_u16(e0, e1), vorrq_u16(e2, e3)),
        vorrq_u16(vorrq_u16(e4, e5), vorrq_u16(e6, e7)),
    )
}

/// 8-bit mask of the lanes of `m` that are all zeros (`m` holds all-zero or all-one lanes).
#[inline]
#[target_feature(enable = "neon")]
fn clear_lanes(m: uint16x8_t) -> u8 {
    // SAFETY: `LANE_BITS` holds 8 values.
    let bits = unsafe { load(LANE_BITS.as_ptr()) };
    vaddvq_u16(vbicq_u16(bits, m)) as u8
}

/// 8-bit mask of the lanes of `m` that are all ones (`m` holds all-zero or all-one lanes).
#[inline]
#[target_feature(enable = "neon")]
fn set_lanes(m: uint16x8_t) -> u8 {
    // SAFETY: `LANE_BITS` holds 8 values.
    let bits = unsafe { load(LANE_BITS.as_ptr()) };
    vaddvq_u16(vandq_u16(bits, m)) as u8
}

#[inline]
#[target_feature(enable = "neon")]
fn any_set(m: uint16x8_t) -> bool {
    vget_lane_u64::<0>(vreinterpret_u64_u8(vmovn_u16(m))) != 0
}

/// Moves the lanes of `v` selected by `mask` to the front, in order.
#[inline]
#[target_feature(enable = "neon")]
fn compact(v: uint16x8_t, mask: u8) -> uint16x8_t {
    // SAFETY: every table entry holds 16 bytes.
    let idx = unsafe { vld1q_u8(SHUFFLE.0[mask as usize].as_ptr()) };
    vreinterpretq_u16_u8(vqtbl1q_u8(vreinterpretq_u8_u16(v), idx))
}

/// Writes the lanes of `v` selected by `mask` to `dst[k..]`, returns the new length.
///
/// # Safety
/// `dst[k..k + 8]` must be valid for writes (all 8 lanes are stored).
#[inline]
#[target_feature(enable = "neon")]
unsafe fn store_compact(dst: *mut u16, k: usize, v: uint16x8_t, mask: u8) -> usize {
    // SAFETY: guaranteed by the caller.
    unsafe { vst1q_u16(dst.add(k), compact(v, mask)) };
    k + POPCOUNT.0[mask as usize] as usize
}

/// Like [`store_compact`], but falls back to an exact copy when fewer than 8 lanes of `out` are
/// left. The selected lanes themselves must fit.
#[inline]
#[target_feature(enable = "neon")]
fn store_compact_checked(out: &mut [MaybeUninit<u16>], k: usize, v: uint16x8_t, mask: u8) -> usize {
    if k + LANES <= out.len() {
        // SAFETY: 8 lanes are in bounds.
        unsafe { store_compact(out.as_mut_ptr().cast(), k, v, mask) }
    } else {
        store_compact_tail(out, k, compact(v, mask), POPCOUNT.0[mask as usize] as usize)
    }
}

#[cold]
#[inline(never)]
#[target_feature(enable = "neon")]
fn store_compact_tail(
    out: &mut [MaybeUninit<u16>],
    k: usize,
    packed: uint16x8_t,
    n: usize,
) -> usize {
    let mut tmp = [0u16; LANES];
    // SAFETY: `tmp` holds 8 values.
    unsafe { vst1q_u16(tmp.as_mut_ptr(), packed) };
    for (o, &t) in out[k..k + n].iter_mut().zip(&tmp) {
        o.write(t);
    }
    k + n
}

/// Copies `src` to `out[k..]`, returns the new length.
#[inline]
#[target_feature(enable = "neon")]
fn copy(src: &[u16], out: &mut [MaybeUninit<u16>], k: usize) -> usize {
    let n = src.len();
    if n > 64 {
        return scalar::copy_tail(src, out, k);
    }
    let dst = &mut out[k..k + n];
    if n >= LANES {
        let (s, d) = (src.as_ptr(), dst.as_mut_ptr().cast::<u16>());
        // SAFETY: all accesses are within the first `n` values of `src` and `dst`; the last
        // vector overlaps the previous one when `n` is not a multiple of 8.
        unsafe {
            let mut x = 0;
            while x + LANES <= n {
                vst1q_u16(d.add(x), load(s.add(x)));
                x += LANES;
            }
            if x < n {
                vst1q_u16(d.add(n - LANES), load(s.add(n - LANES)));
            }
        }
    } else {
        for (d, &s) in dst.iter_mut().zip(src) {
            d.write(s);
        }
    }
    k + n
}

/// The values of `r` (1 to 7 of them) in a vector, padded with copies of `r[0]`.
#[inline]
#[target_feature(enable = "neon")]
fn padded(r: &[u16]) -> uint16x8_t {
    let mut buf = [r[0]; LANES];
    buf[..r.len()].copy_from_slice(r);
    // SAFETY: `buf` holds 8 values.
    unsafe { load(buf.as_ptr()) }
}

// ---------------------------------------------------------------------------------------------
// Intersection
// ---------------------------------------------------------------------------------------------

/// Runs the block-wise all-pairs comparison while both sides have 8 values left, calling `f`
/// with the block of `a` and the lanes of it found in the block of `b`. Returns the positions
/// where one side has fewer than 8 values left, or `None` when `f` asked to stop.
///
/// Every common value is reported exactly once, in increasing order.
#[inline]
#[target_feature(enable = "neon")]
fn intersect_blocks(
    a: &[u16],
    b: &[u16],
    mut f: impl FnMut(uint16x8_t, uint16x8_t) -> bool,
) -> Option<(usize, usize)> {
    let (mut i, mut j) = (0, 0);
    if a.len() < LANES || b.len() < LANES {
        return Some((i, j));
    }
    let (pa, pb) = (a.as_ptr(), b.as_ptr());
    let (a_last, b_last) = (a.len() - LANES, b.len() - LANES);
    loop {
        // SAFETY: `i <= a_last` and `j <= b_last`.
        let (va, vb, amax, bmax) = unsafe {
            (
                load(pa.add(i)),
                load(pb.add(j)),
                *pa.add(i + LANES - 1),
                *pb.add(j + LANES - 1),
            )
        };
        if !f(va, matrix_eq(va, vb)) {
            return None;
        }
        // The block with the smaller maximum cannot match any later block of the other side.
        i += LANES * (amax <= bmax) as usize;
        j += LANES * (bmax <= amax) as usize;
        if i > a_last || j > b_last {
            return Some((i, j));
        }
    }
}

/// Intersection of a short array with a long one, appended to `out[k..]`.
#[inline]
#[target_feature(enable = "neon")]
fn intersect_gallop(
    small: &[u16],
    large: &[u16],
    out: &mut [MaybeUninit<u16>],
    mut k: usize,
) -> usize {
    let mut j = 0;
    for &v in small {
        j = gallop(large, j, v);
        if j == large.len() {
            break;
        }
        if large[j] == v {
            out[k].write(v);
            k += 1;
            j += 1;
        }
    }
    k
}

#[inline]
#[target_feature(enable = "neon")]
fn intersect_card_gallop(small: &[u16], large: &[u16]) -> usize {
    let mut k = 0;
    let mut j = 0;
    for &v in small {
        j = gallop(large, j, v);
        if j == large.len() {
            break;
        }
        if large[j] == v {
            k += 1;
            j += 1;
        }
    }
    k
}

#[inline]
#[target_feature(enable = "neon")]
fn intersects_gallop(small: &[u16], large: &[u16]) -> bool {
    let mut j = 0;
    for &v in small {
        j = gallop(large, j, v);
        if j == large.len() {
            return false;
        }
        if large[j] == v {
            return true;
        }
    }
    false
}

/// Orders the remainders of the block loop as (short, long).
#[inline]
#[target_feature(enable = "neon")]
fn by_len<'a>(x: &'a [u16], y: &'a [u16]) -> (&'a [u16], &'a [u16]) {
    if x.len() <= y.len() { (x, y) } else { (y, x) }
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn intersect(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len().min(b.len()));
    if let Some((small, large)) = skewed(a, b) {
        return intersect_gallop(small, large, out, 0);
    }
    let mut k = 0;
    // Every match is a distinct value of the shorter input, so `k + matches <= out.len()`.
    let Some((i, j)) = intersect_blocks(a, b, |va, eq| {
        k = store_compact_checked(out, k, va, set_lanes(eq));
        true
    }) else {
        unreachable!()
    };
    let (small, large) = by_len(&a[i..], &b[j..]);
    intersect_gallop(small, large, out, k)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn intersect_card(a: &[u16], b: &[u16]) -> usize {
    if let Some((small, large)) = skewed(a, b) {
        return intersect_card_gallop(small, large);
    }
    // Lane `l` counts the matches of lane `l` of the blocks of `a`: at most 8192 per lane.
    let mut acc = vdupq_n_u16(0);
    let Some((i, j)) = intersect_blocks(a, b, |_, eq| {
        acc = vsubq_u16(acc, eq);
        true
    }) else {
        unreachable!()
    };
    let (small, large) = by_len(&a[i..], &b[j..]);
    vaddlvq_u16(acc) as usize + intersect_card_gallop(small, large)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn intersects(a: &[u16], b: &[u16]) -> bool {
    if let Some((small, large)) = skewed(a, b) {
        return intersects_gallop(small, large);
    }
    match intersect_blocks(a, b, |_, eq| !any_set(eq)) {
        None => true,
        Some((i, j)) => {
            let (small, large) = by_len(&a[i..], &b[j..]);
            intersects_gallop(small, large)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Difference
// ---------------------------------------------------------------------------------------------

/// `a - b` for a short `a`, appended to `out[k..]`.
#[inline]
#[target_feature(enable = "neon")]
fn difference_short_a(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>], mut k: usize) -> usize {
    let mut j = 0;
    for (x, &v) in a.iter().enumerate() {
        j = gallop(b, j, v);
        if j == b.len() {
            return copy(&a[x..], out, k);
        }
        if b[j] == v {
            j += 1;
        } else {
            out[k].write(v);
            k += 1;
        }
    }
    k
}

/// `a - b` for a short `b`: copies the stretches of `a` between the removed values.
#[inline]
#[target_feature(enable = "neon")]
fn difference_short_b(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>], mut k: usize) -> usize {
    let mut i = 0;
    for &v in b {
        let next = gallop(a, i, v);
        k = copy(&a[i..next], out, k);
        i = next;
        if i == a.len() {
            return k;
        }
        if a[i] == v {
            i += 1;
        }
    }
    copy(&a[i..], out, k)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn difference(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len());
    if b.len().saturating_mul(GALLOP_RATIO) < a.len() {
        return difference_short_b(a, b, out, 0);
    }
    if a.len().saturating_mul(GALLOP_RATIO) < b.len() {
        return difference_short_a(a, b, out, 0);
    }
    let (mut i, mut j, mut k) = (0, 0, 0);
    if a.len() >= LANES && b.len() >= LANES {
        let dst = out.as_mut_ptr().cast::<u16>();
        let (pa, pb) = (a.as_ptr(), b.as_ptr());
        let (a_last, b_last) = (a.len() - LANES, b.len() - LANES);
        // Lanes of the current block of `a` seen in the blocks of `b` so far.
        let mut found = vdupq_n_u16(0);
        loop {
            // SAFETY: `i <= a_last` and `j <= b_last`.
            let (va, vb, amax, bmax) = unsafe {
                (
                    load(pa.add(i)),
                    load(pb.add(j)),
                    *pa.add(i + LANES - 1),
                    *pb.add(j + LANES - 1),
                )
            };
            found = vorrq_u16(found, matrix_eq(va, vb));
            let (adv_a, adv_b) = (amax <= bmax, bmax <= amax);
            // The block of `a` is final once no later block of `b` can hold its values. The
            // store is unconditional (it is overwritten when the block is not final yet).
            let keep = clear_lanes(found) & (adv_a as u8).wrapping_neg();
            // SAFETY: `k <= i` (only values of earlier blocks of `a` were written) and
            // `i + 8 <= a.len() <= out.len()`.
            k = unsafe { store_compact(dst, k, va, keep) };
            found = vandq_u16(found, vdupq_n_u16((adv_a as u16).wrapping_sub(1)));
            i += LANES * adv_a as usize;
            j += LANES * adv_b as usize;
            if i > a_last || j > b_last {
                if !adv_a {
                    // `b` has fewer than 8 values left: finish the pending block of `a`.
                    if j < b.len() {
                        found = vorrq_u16(found, matrix_eq(va, padded(&b[j..])));
                    }
                    // SAFETY: as above, `i` is unchanged.
                    k = unsafe { store_compact(dst, k, va, clear_lanes(found)) };
                    i += LANES;
                }
                break;
            }
        }
    }
    let (ra, rb) = (&a[i..], &b[j..]);
    if ra.len() <= rb.len() {
        difference_short_a(ra, rb, out, k)
    } else {
        difference_short_b(ra, rb, out, k)
    }
}

// ---------------------------------------------------------------------------------------------
// Union and symmetric difference
// ---------------------------------------------------------------------------------------------

/// Merges two sorted vectors with a bitonic network: returns the lowest 8 and the highest 8
/// values, both sorted. `b` is the operand that gets reversed (off the critical path when it was
/// just loaded).
#[inline]
#[target_feature(enable = "neon")]
fn merge(a: uint16x8_t, b: uint16x8_t) -> (uint16x8_t, uint16x8_t) {
    // `a` followed by `b` reversed is bitonic.
    let r = vrev64q_u16(b);
    let r = vextq_u16::<4>(r, r);
    // Distance 8: `l` holds the 8 smallest values, `h` the 8 largest (both bitonic).
    let (l, h) = (vminq_u16(a, r), vmaxq_u16(a, r));
    // Distance 4 inside `l` and `h`: compare [l0..l3 h0..h3] with [l4..l7 h4..h7].
    let (x, y) = (vreinterpretq_u64_u16(l), vreinterpretq_u64_u16(h));
    let (x, y) = (
        vreinterpretq_u16_u64(vzip1q_u64(x, y)),
        vreinterpretq_u16_u64(vzip2q_u64(x, y)),
    );
    let (l, h) = (vminq_u16(x, y), vmaxq_u16(x, y));
    // Distance 2: pair up the 32-bit lane pairs of the four groups of 4.
    let (x, y) = (vreinterpretq_u32_u16(l), vreinterpretq_u32_u16(h));
    let (x, y) = (
        vreinterpretq_u16_u32(vtrn1q_u32(x, y)),
        vreinterpretq_u16_u32(vtrn2q_u32(x, y)),
    );
    let (l, h) = (vminq_u16(x, y), vmaxq_u16(x, y));
    // Distance 1.
    let (x, y) = (vtrn1q_u16(l, h), vtrn2q_u16(l, h));
    let (l, h) = (vminq_u16(x, y), vmaxq_u16(x, y));
    (vzip1q_u16(l, h), vzip2q_u16(l, h))
}

/// A vector whose last lane differs from the first lane of `v`.
#[inline]
#[target_feature(enable = "neon")]
fn not_before(v: uint16x8_t) -> uint16x8_t {
    vdupq_laneq_u16::<0>(vmvnq_u16(v))
}

/// A vector whose first lane differs from the last lane of `v`.
#[inline]
#[target_feature(enable = "neon")]
fn not_after(v: uint16x8_t) -> uint16x8_t {
    vdupq_laneq_u16::<7>(vmvnq_u16(v))
}

/// Lanes of `v` that differ from their predecessor (lane 7 of `prev` precedes lane 0).
#[inline]
#[target_feature(enable = "neon")]
fn first_of_equal(prev: uint16x8_t, v: uint16x8_t) -> u8 {
    clear_lanes(vceqq_u16(vextq_u16::<7>(prev, v), v))
}

/// Lanes of `v` that differ from both neighbours (lane 7 of `prev` precedes lane 0, lane 0 of
/// `next` follows lane 7).
#[inline]
#[target_feature(enable = "neon")]
fn unpaired(prev: uint16x8_t, v: uint16x8_t, next: uint16x8_t) -> u8 {
    let left = vceqq_u16(vextq_u16::<7>(prev, v), v);
    let right = vceqq_u16(vextq_u16::<1>(v, next), v);
    clear_lanes(vorrq_u16(left, right))
}

/// Merges `a` and `b` (both at least 8 long) 8 values at a time, calling
/// `emit(previous low, low, high)` for every sorted block of 8 that leaves the network.
/// Returns the values still in the network (`high`, sorted), the last emitted block and the
/// positions in `a` and `b`; one side has fewer than 8 values left.
///
/// Every emitted value is smaller than all values not loaded yet, so equal values of `a` and `b`
/// are adjacent in the stream of emitted blocks followed by `high`.
#[inline]
#[target_feature(enable = "neon")]
fn merge_blocks(
    a: &[u16],
    b: &[u16],
    mut emit: impl FnMut(uint16x8_t, uint16x8_t, uint16x8_t),
) -> (uint16x8_t, uint16x8_t, usize, usize) {
    debug_assert!(a.len() >= LANES && b.len() >= LANES);
    let (pa, pb) = (a.as_ptr(), b.as_ptr());
    // SAFETY: both inputs hold at least 8 values.
    let (mut lo, mut hi) = unsafe { merge(load(pa), load(pb)) };
    emit(not_before(lo), lo, hi);
    let (mut i, mut j) = (LANES, LANES);
    while i + LANES <= a.len() && j + LANES <= b.len() {
        // SAFETY: `i < a.len()` and `j < b.len()`.
        let take_a = unsafe { *pa.add(i) <= *pb.add(j) };
        // SAFETY: the selected side has 8 values left.
        let v = unsafe { load(if take_a { pa.add(i) } else { pb.add(j) }) };
        i += LANES * take_a as usize;
        j += LANES * !take_a as usize;
        let prev = lo;
        (lo, hi) = merge(hi, v);
        emit(prev, lo, hi);
    }
    (hi, lo, i, j)
}

/// Union of a short array with a long one, appended to `out[k..]`.
#[inline]
#[target_feature(enable = "neon")]
fn union_short(small: &[u16], large: &[u16], out: &mut [MaybeUninit<u16>], mut k: usize) -> usize {
    let mut j = 0;
    for &v in small {
        let next = gallop(large, j, v);
        k = copy(&large[j..next], out, k);
        j = next;
        if j < large.len() && large[j] == v {
            j += 1;
        }
        out[k].write(v);
        k += 1;
    }
    copy(&large[j..], out, k)
}

/// Symmetric difference of a short array with a long one, appended to `out[k..]`.
#[inline]
#[target_feature(enable = "neon")]
fn xor_short(small: &[u16], large: &[u16], out: &mut [MaybeUninit<u16>], mut k: usize) -> usize {
    let mut j = 0;
    for &v in small {
        let next = gallop(large, j, v);
        k = copy(&large[j..next], out, k);
        j = next;
        if j < large.len() && large[j] == v {
            j += 1;
        } else {
            out[k].write(v);
            k += 1;
        }
    }
    copy(&large[j..], out, k)
}

/// The remainders of [`merge_blocks`] as (short, long): one of them has fewer than 8 values.
#[inline]
#[target_feature(enable = "neon")]
fn remainders<'a>(a: &'a [u16], b: &'a [u16], i: usize, j: usize) -> (&'a [u16], &'a [u16]) {
    let (ra, rb) = (&a[i..], &b[j..]);
    if ra.len() < LANES { (ra, rb) } else { (rb, ra) }
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn union(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len() + b.len());
    if a.len() < LANES || b.len() < LANES || skewed(a, b).is_some() {
        let (small, large) = by_len(a, b);
        return union_short(small, large, out, 0);
    }
    let dst = out.as_mut_ptr().cast::<u16>();
    let mut k = 0;
    let (hi, last, i, j) = merge_blocks(a, b, |prev, lo, _| {
        // SAFETY: at most `loaded - 16` values were written before this block (8 values are
        // still in the network), so the 8 lanes end within `a.len() + b.len() <= out.len()`.
        k = unsafe { store_compact(dst, k, lo, first_of_equal(prev, lo)) };
    });
    // `hi` without duplicates (of the last emitted value or inside `hi`), then the remainders.
    let mut rest = [0u16; LANES];
    // SAFETY: `rest` holds 8 values.
    let n = unsafe { store_compact(rest.as_mut_ptr(), 0, hi, first_of_equal(last, hi)) };
    let (short, long) = remainders(a, b, i, j);
    let mut small = [MaybeUninit::uninit(); 2 * LANES];
    let m = scalar::array_union_merge(&rest[..n], short, &mut small, 0, 0, 0);
    // SAFETY: the merge initialized the first `m` values.
    union_short(unsafe { assume_init(&small, m) }, long, out, k)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn xor(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len() + b.len());
    if a.len() < LANES || b.len() < LANES || skewed(a, b).is_some() {
        let (small, large) = by_len(a, b);
        return xor_short(small, large, out, 0);
    }
    let dst = out.as_mut_ptr().cast::<u16>();
    let mut k = 0;
    let (hi, last, i, j) = merge_blocks(a, b, |prev, lo, hi| {
        // The value after `lo` in the stream is `hi[0]`: equal values are never still unloaded.
        // SAFETY: as for the union.
        k = unsafe { store_compact(dst, k, lo, unpaired(prev, lo, hi)) };
    });
    // Values of `hi` may still pair with unloaded values, which the final merges handle.
    let mut rest = [0u16; LANES];
    // SAFETY: `rest` holds 8 values.
    let n = unsafe { store_compact(rest.as_mut_ptr(), 0, hi, unpaired(last, hi, not_after(hi))) };
    let (short, long) = remainders(a, b, i, j);
    let mut small = [MaybeUninit::uninit(); 2 * LANES];
    let m = scalar::array_xor_merge(&rest[..n], short, &mut small, 0, 0, 0);
    // SAFETY: the merge initialized the first `m` values.
    xor_short(unsafe { assume_init(&small, m) }, long, out, k)
}
