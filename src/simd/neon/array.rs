//! Sorted `u16` array kernels.
//!
//! * Intersection and difference compare blocks of 8 values all-pairs (`a` against the 8
//!   rotations of `b`), advance the block with the smaller maximum without branching, and compact
//!   the selected lanes with `tbl` and a 256-entry shuffle table.
//! * Union and symmetric difference stream both inputs through a bitonic merge network
//!   (`umin`/`umax` + `zip`/`trn`) that emits 8 sorted values per step. Duplicates are always
//!   adjacent in the merged stream and are removed by comparing every value with its neighbours
//!   before the compacting store.
//!   The last, partial block of each side is completed with padding values that sort last and
//!   are dropped on output, so no scalar tail is needed.
//! * When one side is short (fewer than 8 values left, or a size ratio of at least
//!   [`GALLOP_RATIO`](crate::simd::GALLOP_RATIO)) every value of the short side is located in the
//!   long one with 32-value vector probes (galloping beyond 128 values), and the stretches of
//!   the long side in between are block-copied.
//!
//! Stores of full vectors are only issued where at least 8 lanes of the output are in bounds;
//! everything else goes through bounds-checked slice accesses.

use core::arch::aarch64::*;
use core::mem::MaybeUninit;

use super::tables::{PAD, POPCOUNT, SHUFFLE, TAIL_PADDING, TAIL_SHUFFLE};
use crate::simd::scalar::{self, gallop, skewed};
use crate::simd::{GALLOP_RATIO, check_out};

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

/// The last value of the 8-value block at `p`, read with a scalar load.
///
/// The block maxima decide the next load addresses, so they sit on the loop-carried dependency
/// chain of the block loops. The read is volatile only to stop LLVM from turning it into a lane
/// extract (`umov`) of the vector load of the same block, which is several cycles slower than a
/// direct `ldrh`.
///
/// # Safety
/// `p` must be valid for reading 8 values.
#[inline(always)]
unsafe fn block_max(p: *const u16) -> u16 {
    // SAFETY: guaranteed by the caller.
    unsafe { p.add(LANES - 1).read_volatile() }
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

/// Copies `a[from..to]` to `out[k..]`, returns the new length. Runs of up to 8 values take a
/// single vector store when the 8 lanes are in bounds on both sides (the lanes past the run are
/// overwritten later or lie past the result).
#[inline]
#[target_feature(enable = "neon")]
fn copy_run(a: &[u16], from: usize, to: usize, out: &mut [MaybeUninit<u16>], k: usize) -> usize {
    let n = to - from;
    if n <= LANES && from + LANES <= a.len() && k + LANES <= out.len() {
        // SAFETY: both 8-value ranges were checked above.
        unsafe { vst1q_u16(out.as_mut_ptr().add(k).cast(), load(a.as_ptr().add(from))) };
        k + n
    } else {
        copy(&a[from..to], out, k)
    }
}

/// Number of values below `target` in `chunk`, a prefix of it when `chunk` is sorted.
#[inline]
#[target_feature(enable = "neon")]
fn count_below(chunk: uint16x8_t, target: uint16x8_t) -> usize {
    // The comparison lanes are 0 or -1: their sum is minus the count.
    0u16.wrapping_sub(vaddvq_u16(vcltq_u16(chunk, target))) as usize
}

/// Number of the 32 values at `p` that are below `target` (sorted values: a prefix).
///
/// # Safety
/// `p` must be valid for reading 32 values.
#[inline]
#[target_feature(enable = "neon")]
unsafe fn count_below_32(p: *const u16, target: uint16x8_t) -> usize {
    // SAFETY: guaranteed by the caller.
    let below = unsafe {
        vaddq_u16(
            vaddq_u16(
                vcltq_u16(load(p), target),
                vcltq_u16(load(p.add(LANES)), target),
            ),
            vaddq_u16(
                vcltq_u16(load(p.add(2 * LANES)), target),
                vcltq_u16(load(p.add(3 * LANES)), target),
            ),
        )
    };
    // The comparison lanes are 0 or -1: the sum is minus the count.
    0u16.wrapping_sub(vaddvq_u16(below)) as usize
}

/// Values covered by one probe of [`seek`].
const PROBE: usize = 4 * LANES;

/// Index of the first value `>= v` in `a[x..]` (sorted), or `a.len()`.
///
/// Distances below 128 values are found with probes of 32 values (four compares and one
/// horizontal sum, no dependent loads), longer ones by galloping. The first probe is inlined,
/// the rest is out of line.
#[inline]
#[target_feature(enable = "neon")]
fn seek(a: &[u16], x: usize, v: u16) -> usize {
    if x + PROBE <= a.len() {
        // SAFETY: `x + 32 <= a.len()`.
        let n = unsafe { count_below_32(a.as_ptr().add(x), vdupq_n_u16(v)) };
        if n < PROBE {
            return x + n;
        }
        return seek_far(a, x + PROBE, v);
    }
    seek_near(a, x, v)
}

/// [`seek`] past its first probe: three more probes, then galloping.
#[inline(never)]
#[target_feature(enable = "neon")]
fn seek_far(a: &[u16], mut x: usize, v: u16) -> usize {
    let target = vdupq_n_u16(v);
    for _ in 0..3 {
        if x + PROBE > a.len() {
            return seek_near(a, x, v);
        }
        // SAFETY: `x + 32 <= a.len()`.
        let n = unsafe { count_below_32(a.as_ptr().add(x), target) };
        if n < PROBE {
            return x + n;
        }
        x += PROBE;
    }
    gallop(a, x, v)
}

/// [`seek`] with fewer than 32 values left.
#[inline(never)]
#[target_feature(enable = "neon")]
fn seek_near(a: &[u16], mut x: usize, v: u16) -> usize {
    let target = vdupq_n_u16(v);
    while x + LANES <= a.len() {
        // SAFETY: `x + 8 <= a.len()`.
        let n = count_below(unsafe { load(a.as_ptr().add(x)) }, target);
        if n < LANES {
            return x + n;
        }
        x += LANES;
    }
    while x < a.len() && a[x] < v {
        x += 1;
    }
    x
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
                block_max(pa.add(i)),
                block_max(pb.add(j)),
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
fn intersect_short(
    small: &[u16],
    large: &[u16],
    out: &mut [MaybeUninit<u16>],
    mut k: usize,
) -> usize {
    let mut j = 0;
    for &v in small {
        j = seek(large, j, v);
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
fn intersect_card_short(small: &[u16], large: &[u16]) -> usize {
    let mut k = 0;
    let mut j = 0;
    for &v in small {
        j = seek(large, j, v);
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
fn intersects_short(small: &[u16], large: &[u16]) -> bool {
    let mut j = 0;
    for &v in small {
        j = seek(large, j, v);
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
        return intersect_short(small, large, out, 0);
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
    intersect_short(small, large, out, k)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn intersect_card(a: &[u16], b: &[u16]) -> usize {
    if let Some((small, large)) = skewed(a, b) {
        return intersect_card_short(small, large);
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
    vaddlvq_u16(acc) as usize + intersect_card_short(small, large)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn intersects(a: &[u16], b: &[u16]) -> bool {
    if let Some((small, large)) = skewed(a, b) {
        return intersects_short(small, large);
    }
    match intersect_blocks(a, b, |_, eq| !any_set(eq)) {
        None => true,
        Some((i, j)) => {
            let (small, large) = by_len(&a[i..], &b[j..]);
            intersects_short(small, large)
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
        j = seek(b, j, v);
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
        let next = seek(a, i, v);
        k = copy_run(a, i, next, out, k);
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
                    block_max(pa.add(i)),
                    block_max(pb.add(j)),
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

/// A vector whose first lane differs from the last lane of `v`.
#[inline]
#[target_feature(enable = "neon")]
fn not_after(v: uint16x8_t) -> uint16x8_t {
    vdupq_laneq_u16::<7>(vmvnq_u16(v))
}

/// Padding lanes of `v` (values `>= PAD`).
#[inline]
#[target_feature(enable = "neon")]
fn padding(v: uint16x8_t) -> uint16x8_t {
    vcgeq_u16(v, vdupq_n_u16(PAD))
}

/// Union output lanes of `v`: real values that differ from their predecessor (lane 7 of `prev`
/// precedes lane 0).
#[inline]
#[target_feature(enable = "neon")]
fn union_keep(prev: uint16x8_t, v: uint16x8_t) -> u8 {
    let dup = vceqq_u16(vextq_u16::<7>(prev, v), v);
    clear_lanes(vorrq_u16(dup, padding(v)))
}

/// Symmetric difference output lanes of `v`: real values that differ from both neighbours
/// (lane 7 of `prev` precedes lane 0, lane 0 of `next` follows lane 7).
#[inline]
#[target_feature(enable = "neon")]
fn xor_keep(prev: uint16x8_t, v: uint16x8_t, next: uint16x8_t) -> u8 {
    let left = vceqq_u16(vextq_u16::<7>(prev, v), v);
    let right = vceqq_u16(vextq_u16::<1>(v, next), v);
    clear_lanes(vorrq_u16(vorrq_u16(left, right), padding(v)))
}

/// Number of values of `a` below `PAD` (the others, if any, end the array).
#[inline]
#[target_feature(enable = "neon")]
fn below_pad(a: &[u16]) -> usize {
    let mut n = a.len();
    while n > 0 && a[n - 1] >= PAD {
        n -= 1;
    }
    n
}

/// The 8-value block of `a` at `x`. A partial last block (1 to 7 values) is completed with the
/// distinct values `PAD + lane`, which sort after every value of `a`.
///
/// `a` must hold at least 8 values, all below `PAD`, and `x < a.len()`.
#[inline]
#[target_feature(enable = "neon")]
fn load_block(a: &[u16], x: usize) -> uint16x8_t {
    debug_assert!(a.len() >= LANES && x < a.len());
    if x + LANES <= a.len() {
        // SAFETY: 8 values are left.
        return unsafe { load(a.as_ptr().add(x)) };
    }
    let r = (a.len() - x) & (LANES - 1);
    // SAFETY: `a` holds at least 8 values; the table entries hold 16 bytes.
    unsafe {
        let last = vreinterpretq_u8_u16(load(a.as_ptr().add(a.len() - LANES)));
        let moved = vqtbl1q_u8(last, vld1q_u8(TAIL_SHUFFLE.0[r].as_ptr()));
        vorrq_u16(
            vreinterpretq_u16_u8(moved),
            load(TAIL_PADDING.0[r].as_ptr()),
        )
    }
}

/// Streams `a` and `b` (at least 8 values each, all below `PAD`) through the merge network,
/// calling `emit(previous low, low, high)` for every sorted block of 8 that leaves it. The last
/// block of each side is completed with padding (see [`load_block`]), which the emitters drop.
///
/// Every emitted value is smaller than all values not loaded yet, so equal values of `a` and `b`
/// are adjacent in the stream of emitted blocks followed by `high`.
///
/// Once one side is exhausted, the other one is merged only while the network still holds a
/// real value at or above its next value. Returns the network content (`high`), the last
/// emitted block and the unmerged rest of the other side, all of whose values are larger than
/// the real values of `high`.
#[inline]
#[target_feature(enable = "neon")]
fn merge_stream<'a>(
    a: &'a [u16],
    b: &'a [u16],
    mut emit: impl FnMut(uint16x8_t, uint16x8_t, uint16x8_t),
) -> (uint16x8_t, uint16x8_t, &'a [u16]) {
    debug_assert!(a.len() >= LANES && b.len() >= LANES);
    // SAFETY: both inputs hold at least 8 values.
    let (mut lo, mut hi) = unsafe { merge(load(a.as_ptr()), load(b.as_ptr())) };
    emit(vdupq_laneq_u16::<0>(vmvnq_u16(lo)), lo, hi);
    let (mut i, mut j) = (LANES, LANES);
    while i < a.len() && j < b.len() {
        let take_a = a[i] <= b[j];
        let (side, x) = if take_a { (a, i) } else { (b, j) };
        let v = load_block(side, x);
        let x = (x + LANES).min(side.len());
        (i, j) = if take_a { (x, j) } else { (i, x) };
        let prev = lo;
        (lo, hi) = merge(hi, v);
        emit(prev, lo, hi);
    }
    let (rest, mut x) = if i < a.len() { (a, i) } else { (b, j) };
    let pad = vdupq_n_u16(PAD);
    while x < rest.len() {
        let next = vdupq_n_u16(rest[x]);
        if !any_set(vandq_u16(vcgeq_u16(hi, next), vcltq_u16(hi, pad))) {
            break;
        }
        let v = load_block(rest, x);
        x = (x + LANES).min(rest.len());
        let prev = lo;
        (lo, hi) = merge(hi, v);
        emit(prev, lo, hi);
    }
    (hi, lo, &rest[x..])
}

/// Union of a short array with a long one, appended to `out[k..]`.
#[inline]
#[target_feature(enable = "neon")]
fn union_short(small: &[u16], large: &[u16], out: &mut [MaybeUninit<u16>], mut k: usize) -> usize {
    let mut j = 0;
    for &v in small {
        let next = seek(large, j, v);
        k = copy_run(large, j, next, out, k);
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
        let next = seek(large, j, v);
        k = copy_run(large, j, next, out, k);
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

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn union(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len() + b.len());
    let (na, nb) = (below_pad(a), below_pad(b));
    if na < LANES || nb < LANES || skewed(a, b).is_some() {
        let (small, large) = by_len(a, b);
        return union_short(small, large, out, 0);
    }
    // The stores are checked: the padding can make the network emit after the real values ran
    // short of 8 lanes of room.
    let mut k = 0;
    let (hi, last, rest) = merge_stream(&a[..na], &b[..nb], |prev, lo, _| {
        k = store_compact_checked(out, k, lo, union_keep(prev, lo));
    });
    k = store_compact_checked(out, k, hi, union_keep(last, hi));
    k = copy(rest, out, k);
    // The values `>= PAD` (at most 8 per side) come last.
    scalar::array_union_merge(&a[na..], &b[nb..], out, 0, 0, k)
}

#[inline]
#[target_feature(enable = "neon")]
pub(super) fn xor(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len() + b.len());
    let (na, nb) = (below_pad(a), below_pad(b));
    if na < LANES || nb < LANES || skewed(a, b).is_some() {
        let (small, large) = by_len(a, b);
        return xor_short(small, large, out, 0);
    }
    let mut k = 0;
    let (hi, last, rest) = merge_stream(&a[..na], &b[..nb], |prev, lo, hi| {
        // The value after `lo` in the stream is `hi[0]`: equal values are never still unloaded.
        k = store_compact_checked(out, k, lo, xor_keep(prev, lo, hi));
    });
    // The real values of `hi` are below all values of `rest`: nothing pairs across.
    k = store_compact_checked(out, k, hi, xor_keep(last, hi, not_after(hi)));
    k = copy(rest, out, k);
    scalar::array_xor_merge(&a[na..], &b[nb..], out, 0, 0, k)
}
