//! AArch64 NEON kernels.
//!
//! Placeholder: every kernel currently delegates to the scalar reference. Specialized
//! implementations replace these one by one.

#![allow(clippy::missing_safety_doc)]

use core::mem::MaybeUninit;

use crate::simd::{BitOp, Words, scalar};

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

macro_rules! neon_kernels {
    ($($name:ident($($arg:ident : $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $(
            #[target_feature(enable = "neon")]
            pub(crate) unsafe fn $name($($arg: $ty),*) $(-> $ret)? {
                scalar::$name($($arg),*)
            }
        )*
    };
}

neon_kernels! {
    bitmap_popcount(a: &Words) -> u32;
    bitmap_op(op: BitOp, a: &Words, b: &Words, out: &mut Words) -> u32;
    bitmap_op_inplace(op: BitOp, a: &mut Words, b: &Words) -> u32;
    bitmap_op_inplace_nocard(op: BitOp, a: &mut Words, b: &Words);
    bitmap_op_card(op: BitOp, a: &Words, b: &Words) -> u32;
    bitmap_intersects(a: &Words, b: &Words) -> bool;
    bitmap_is_subset(a: &Words, b: &Words) -> bool;
    bitmap_to_array(a: &Words, out: &mut [MaybeUninit<u16>]) -> usize;
    bitmap_count_runs(a: &Words) -> u32;
    array_intersect(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize;
    array_intersect_card(a: &[u16], b: &[u16]) -> usize;
    array_intersects(a: &[u16], b: &[u16]) -> bool;
    array_union(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize;
    array_difference(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize;
    array_xor(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize;
}
