//! Portable reference kernels.
//!
//! These are the semantic reference for every vectorized kernel (checked by the differential
//! tests) and provide building blocks such as tails and galloping searches. They are never
//! selected as a backend on their own. They are `#[inline(always)]` so that backends can
//! re-instantiate them inside `#[target_feature]` functions.

// Real builds only reach the reference kernels through the Miri dispatch, the tests and the few
// building blocks the vector backends reuse.
#![cfg_attr(not(any(test, miri)), allow(dead_code))]

use core::mem::MaybeUninit;

use super::{BitOp, GALLOP_RATIO, Words, check_out};

// ---------------------------------------------------------------------------------------------
// Bitmap kernels
// ---------------------------------------------------------------------------------------------

#[inline(always)]
pub(crate) fn bitmap_popcount(a: &Words) -> u32 {
    let mut acc = [0u32; 4];
    for chunk in a.as_chunks::<4>().0 {
        for (acc, &w) in acc.iter_mut().zip(chunk) {
            *acc += w.count_ones();
        }
    }
    acc.iter().sum()
}

#[inline(always)]
fn op_into<const OP: u8>(a: &Words, b: &Words, out: &mut Words) -> u32 {
    let op = op_from_const::<OP>();
    let mut acc = [0u32; 4];
    for ((a, b), out) in a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .zip(out.as_chunks_mut::<4>().0)
    {
        for i in 0..4 {
            let w = op.apply(a[i], b[i]);
            out[i] = w;
            acc[i] += w.count_ones();
        }
    }
    acc.iter().sum()
}

#[inline(always)]
fn op_inplace<const OP: u8>(a: &mut Words, b: &Words) -> u32 {
    let op = op_from_const::<OP>();
    let mut acc = [0u32; 4];
    for (a, b) in a
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(b.as_chunks::<4>().0)
    {
        for i in 0..4 {
            let w = op.apply(a[i], b[i]);
            a[i] = w;
            acc[i] += w.count_ones();
        }
    }
    acc.iter().sum()
}

#[inline(always)]
fn op_inplace_nocard<const OP: u8>(a: &mut Words, b: &Words) {
    let op = op_from_const::<OP>();
    for (a, &b) in a.iter_mut().zip(b.iter()) {
        *a = op.apply(*a, b);
    }
}

#[inline(always)]
fn op_card<const OP: u8>(a: &Words, b: &Words) -> u32 {
    let op = op_from_const::<OP>();
    let mut acc = [0u32; 4];
    for (a, b) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        for i in 0..4 {
            acc[i] += op.apply(a[i], b[i]).count_ones();
        }
    }
    acc.iter().sum()
}

pub(crate) const AND: u8 = 0;
pub(crate) const OR: u8 = 1;
pub(crate) const XOR: u8 = 2;
pub(crate) const ANDNOT: u8 = 3;

#[inline(always)]
pub(crate) const fn op_from_const<const OP: u8>() -> BitOp {
    match OP {
        AND => BitOp::And,
        OR => BitOp::Or,
        XOR => BitOp::Xor,
        _ => BitOp::AndNot,
    }
}

/// Expands a runtime [`BitOp`] into a call of a function generic over the op constant.
macro_rules! with_op {
    ($op:expr, $f:ident($($arg:expr),*)) => {
        match $op {
            BitOp::And => $f::<{ AND }>($($arg),*),
            BitOp::Or => $f::<{ OR }>($($arg),*),
            BitOp::Xor => $f::<{ XOR }>($($arg),*),
            BitOp::AndNot => $f::<{ ANDNOT }>($($arg),*),
        }
    };
}
pub(crate) use with_op;

#[inline(always)]
pub(crate) fn bitmap_op(op: BitOp, a: &Words, b: &Words, out: &mut Words) -> u32 {
    with_op!(op, op_into(a, b, out))
}

#[inline(always)]
pub(crate) fn bitmap_op_inplace(op: BitOp, a: &mut Words, b: &Words) -> u32 {
    with_op!(op, op_inplace(a, b))
}

#[inline(always)]
pub(crate) fn bitmap_op_inplace_nocard(op: BitOp, a: &mut Words, b: &Words) {
    with_op!(op, op_inplace_nocard(a, b))
}

#[inline(always)]
pub(crate) fn bitmap_op_card(op: BitOp, a: &Words, b: &Words) -> u32 {
    with_op!(op, op_card(a, b))
}

#[inline(always)]
pub(crate) fn bitmap_intersects(a: &Words, b: &Words) -> bool {
    a.as_chunks::<16>()
        .0
        .iter()
        .zip(b.as_chunks::<16>().0)
        .any(|(a, b)| a.iter().zip(b).fold(0, |acc, (a, b)| acc | (a & b)) != 0)
}

#[inline(always)]
pub(crate) fn bitmap_is_subset(a: &Words, b: &Words) -> bool {
    a.as_chunks::<16>()
        .0
        .iter()
        .zip(b.as_chunks::<16>().0)
        .all(|(a, b)| a.iter().zip(b).fold(0, |acc, (a, b)| acc | (a & !b)) == 0)
}

#[inline(always)]
pub(crate) fn bitmap_to_array(a: &Words, out: &mut [MaybeUninit<u16>]) -> usize {
    bitmap_to_array_from(a, 0, out, 0)
}

/// Extracts set bits of `a[first_word..]`, writing from `out[written..]`.
#[inline(always)]
pub(crate) fn bitmap_to_array_from(
    a: &Words,
    first_word: usize,
    out: &mut [MaybeUninit<u16>],
    mut written: usize,
) -> usize {
    for (i, &w) in a.iter().enumerate().skip(first_word) {
        let mut w = w;
        let base = (i * 64) as u16;
        while w != 0 {
            let bit = w.trailing_zeros() as u16;
            out[written].write(base + bit);
            written += 1;
            w &= w - 1;
        }
    }
    written
}

#[inline(always)]
pub(crate) fn bitmap_count_runs(a: &Words) -> u32 {
    // Count run starts: set bits whose lower neighbour is clear.
    let mut runs = 0u32;
    let mut carry = 0u64;
    for &w in a.iter() {
        runs += (w & !((w << 1) | carry)).count_ones();
        carry = w >> 63;
    }
    runs
}

// ---------------------------------------------------------------------------------------------
// Array kernels
// ---------------------------------------------------------------------------------------------

/// Index of the first element `>= value` in `a[from..]`, found by exponential search.
#[inline(always)]
pub(crate) fn gallop(a: &[u16], from: usize, value: u16) -> usize {
    let len = a.len();
    if from >= len || a[from] >= value {
        return from;
    }
    // Invariant: a[lo] < value.
    let mut lo = from;
    let mut step = 1;
    let mut hi = from + 1;
    while hi < len && a[hi] < value {
        lo = hi;
        step <<= 1;
        hi = from + step;
    }
    let hi = hi.min(len);
    // a[lo] < value <= a[hi] (or hi == len)
    lo + 1 + a[lo + 1..hi].partition_point(|&x| x < value)
}

#[inline(always)]
fn intersect_gallop(small: &[u16], large: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    let mut k = 0;
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

#[inline(always)]
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

/// Returns `(small, large)` if the size ratio favours galloping.
#[inline(always)]
pub(crate) fn skewed<'a>(a: &'a [u16], b: &'a [u16]) -> Option<(&'a [u16], &'a [u16])> {
    if a.len().saturating_mul(GALLOP_RATIO) < b.len() {
        Some((a, b))
    } else if b.len().saturating_mul(GALLOP_RATIO) < a.len() {
        Some((b, a))
    } else {
        None
    }
}

#[inline(always)]
pub(crate) fn array_intersect(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len().min(b.len()));
    if let Some((small, large)) = skewed(a, b) {
        return intersect_gallop(small, large, out);
    }
    array_intersect_merge(a, b, out, 0, 0, 0)
}

/// Branchless merge intersection starting at `a[i..]`, `b[j..]`, writing from `out[k..]`.
#[inline(always)]
pub(crate) fn array_intersect_merge(
    a: &[u16],
    b: &[u16],
    out: &mut [MaybeUninit<u16>],
    mut i: usize,
    mut j: usize,
    mut k: usize,
) -> usize {
    debug_assert!(out.len() >= k + (a.len() - i).min(b.len() - j));
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i], b[j]);
        // k <= min(i, j) < out.len(), so this write is always in bounds.
        out[k].write(x);
        k += (x == y) as usize;
        i += (x <= y) as usize;
        j += (x >= y) as usize;
    }
    k
}

#[inline(always)]
pub(crate) fn array_intersect_card(a: &[u16], b: &[u16]) -> usize {
    if let Some((small, large)) = skewed(a, b) {
        return intersect_card_gallop(small, large);
    }
    array_intersect_card_merge(a, b, 0, 0, 0)
}

#[inline(always)]
pub(crate) fn array_intersect_card_merge(
    a: &[u16],
    b: &[u16],
    mut i: usize,
    mut j: usize,
    mut k: usize,
) -> usize {
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i], b[j]);
        k += (x == y) as usize;
        i += (x <= y) as usize;
        j += (x >= y) as usize;
    }
    k
}

#[inline(always)]
pub(crate) fn array_intersects(a: &[u16], b: &[u16]) -> bool {
    if let Some((small, large)) = skewed(a, b) {
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
        return false;
    }
    array_intersects_merge(a, b, 0, 0)
}

#[inline(always)]
pub(crate) fn array_intersects_merge(a: &[u16], b: &[u16], mut i: usize, mut j: usize) -> bool {
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i], b[j]);
        if x == y {
            return true;
        }
        i += (x < y) as usize;
        j += (x > y) as usize;
    }
    false
}

#[inline(always)]
pub(crate) fn array_union(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len() + b.len());
    array_union_merge(a, b, out, 0, 0, 0)
}

#[inline(always)]
pub(crate) fn array_union_merge(
    a: &[u16],
    b: &[u16],
    out: &mut [MaybeUninit<u16>],
    mut i: usize,
    mut j: usize,
    mut k: usize,
) -> usize {
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i], b[j]);
        out[k].write(x.min(y));
        k += 1;
        i += (x <= y) as usize;
        j += (y <= x) as usize;
    }
    k = copy_tail(&a[i..], out, k);
    copy_tail(&b[j..], out, k)
}

#[inline(always)]
pub(crate) fn array_difference(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len());
    if b.len().saturating_mul(GALLOP_RATIO) < a.len() {
        // Few values to remove: copy runs of `a` between the removed values.
        let mut k = 0;
        let mut i = 0;
        for &v in b {
            let next = gallop(a, i, v);
            k = copy_tail(&a[i..next], out, k);
            i = next;
            if i == a.len() {
                return k;
            }
            if a[i] == v {
                i += 1;
            }
        }
        return copy_tail(&a[i..], out, k);
    }
    if a.len().saturating_mul(GALLOP_RATIO) < b.len() {
        let mut k = 0;
        let mut j = 0;
        for &v in a {
            j = gallop(b, j, v);
            if j == b.len() || b[j] != v {
                out[k].write(v);
                k += 1;
            }
        }
        return k;
    }
    array_difference_merge(a, b, out, 0, 0, 0)
}

#[inline(always)]
pub(crate) fn array_difference_merge(
    a: &[u16],
    b: &[u16],
    out: &mut [MaybeUninit<u16>],
    mut i: usize,
    mut j: usize,
    mut k: usize,
) -> usize {
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i], b[j]);
        // k <= i < a.len() <= out.len()
        out[k].write(x);
        k += (x < y) as usize;
        i += (x <= y) as usize;
        j += (y <= x) as usize;
    }
    copy_tail(&a[i..], out, k)
}

#[inline(always)]
pub(crate) fn array_xor(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize {
    check_out(out, a.len() + b.len());
    array_xor_merge(a, b, out, 0, 0, 0)
}

#[inline(always)]
pub(crate) fn array_xor_merge(
    a: &[u16],
    b: &[u16],
    out: &mut [MaybeUninit<u16>],
    mut i: usize,
    mut j: usize,
    mut k: usize,
) -> usize {
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i], b[j]);
        out[k].write(x.min(y));
        k += (x != y) as usize;
        i += (x <= y) as usize;
        j += (y <= x) as usize;
    }
    k = copy_tail(&a[i..], out, k);
    copy_tail(&b[j..], out, k)
}

/// Copies `src` into `out[k..]`, returns the new length.
#[inline(always)]
pub(crate) fn copy_tail(src: &[u16], out: &mut [MaybeUninit<u16>], k: usize) -> usize {
    let dst = &mut out[k..k + src.len()];
    // SAFETY: `MaybeUninit<u16>` has the same layout as `u16`.
    let src: &[MaybeUninit<u16>] =
        unsafe { core::slice::from_raw_parts(src.as_ptr().cast(), src.len()) };
    dst.copy_from_slice(src);
    k + src.len()
}
