//! AArch64 NEON (Advanced SIMD) kernels.
//!
//! NEON is part of the AArch64 baseline: it is enabled at compile time on every supported target
//! (Linux, Android, Apple, Windows), so the helpers are ordinary `#[inline(always)]` functions
//! that call the intrinsics directly. SVE is not used (its intrinsics are not stable in Rust).
//!
//! * Bitmaps: 64-byte `ld1`/`st1` streams, `and`/`orr`/`eor`/`bic`, cardinalities with `cnt` +
//!   `uadalp`, run counting with `shl`/`sri`/`bic` on `u64x2` lanes, early-exit tests every
//!   128 bytes, and set-bit extraction through a per-byte position table (`uaddw` + overlapping
//!   stores) for dense chunks or a `rbit`/`clz` loop for sparse ones. See [`bitmap`].
//! * Arrays: all-pairs block comparison with `tbl` compaction (intersection, difference), bitonic
//!   merge networks (union, symmetric difference) and galloping for short or skewed inputs. See
//!   [`array`].
//!
//! The kernels have the same contracts as the reference in [`super::scalar`] and produce
//! identical results.

#[cfg(target_endian = "big")]
compile_error!("the NEON kernels assume little-endian lane order (aarch64_be is not supported)");

#[cfg(not(target_feature = "neon"))]
compile_error!("roaring128 requires NEON on aarch64 (targets without FP/SIMD are not supported)");

use core::mem::MaybeUninit;

use crate::simd::scalar::{AND, ANDNOT, OR, XOR, with_op};
use crate::simd::{BitOp, Words};

mod array;
mod bitmap;
mod tables;

use bitmap::{op_card, op_inplace, op_inplace_nocard, op_into};

/// NEON is part of the AArch64 baseline on every supported target.
#[inline]
pub(crate) fn has_neon() -> bool {
    #[cfg(feature = "std")]
    {
        std::arch::is_aarch64_feature_detected!("neon")
    }
    #[cfg(not(feature = "std"))]
    {
        cfg!(target_feature = "neon")
    }
}

/// Defines the `unsafe` kernel entry points.
///
/// # Safety
/// The CPU must support NEON ([`has_neon`]). Otherwise the contracts are those of the safe
/// reference kernels in [`super::scalar`]: invalid arguments panic, they never cause undefined
/// behaviour.
macro_rules! kernels {
    ($($name:ident($($arg:ident : $ty:ty),*) $(-> $ret:ty)? = $body:expr;)*) => {
        $(
            #[target_feature(enable = "neon")]
            #[allow(clippy::missing_safety_doc)]
            pub(crate) unsafe fn $name($($arg: $ty),*) $(-> $ret)? {
                $body
            }
        )*
    };
}

kernels! {
    bitmap_popcount(a: &Words) -> u32 = bitmap::popcount(a);
    bitmap_op(op: BitOp, a: &Words, b: &Words, out: &mut Words) -> u32 =
        with_op!(op, op_into(a, b, out));
    bitmap_op_inplace(op: BitOp, a: &mut Words, b: &Words) -> u32 =
        with_op!(op, op_inplace(a, b));
    bitmap_op_inplace_nocard(op: BitOp, a: &mut Words, b: &Words) =
        with_op!(op, op_inplace_nocard(a, b));
    bitmap_op_card(op: BitOp, a: &Words, b: &Words) -> u32 =
        with_op!(op, op_card(a, b));
    bitmap_intersects(a: &Words, b: &Words) -> bool = bitmap::intersects(a, b);
    bitmap_is_subset(a: &Words, b: &Words) -> bool = bitmap::is_subset(a, b);
    bitmap_to_array(a: &Words, out: &mut [MaybeUninit<u16>]) -> usize = bitmap::to_array(a, out);
    bitmap_count_runs(a: &Words) -> u32 = bitmap::count_runs(a);
    array_intersect(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize =
        array::intersect(a, b, out);
    array_intersect_card(a: &[u16], b: &[u16]) -> usize = array::intersect_card(a, b);
    array_intersects(a: &[u16], b: &[u16]) -> bool = array::intersects(a, b);
    array_union(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize =
        array::union(a, b, out);
    array_difference(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize =
        array::difference(a, b, out);
    array_xor(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize =
        array::xor(a, b, out);
}
