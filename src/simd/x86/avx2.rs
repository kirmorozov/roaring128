//! avx2 kernels.
//!
//! Placeholder: every kernel currently re-instantiates the scalar reference with the tier's
//! target features. Specialized implementations replace these one by one.

use core::mem::MaybeUninit;

use crate::simd::{BitOp, Words};

super::scalar_kernels! { "avx2,bmi1,bmi2,popcnt,sse4.2" =>
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
