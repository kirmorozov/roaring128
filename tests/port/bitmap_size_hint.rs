//! Port of `tests/size_hint.rs` (`tests/treemap_size_hint.rs` is a subset of it).

use roaring128::RoaringBitmap128;

use crate::common::*;

fn array(off: u128) {
    let bitmap = shifted(off, 0..2000);
    let mut iter = bitmap.iter();
    assert_eq!((2000, Some(2000)), iter.size_hint());
    iter.by_ref().take(1000).for_each(drop);
    assert_eq!((1000, Some(1000)), iter.size_hint());
    iter.by_ref().for_each(drop);
    assert_eq!((0, Some(0)), iter.size_hint());
}

fn bitmap(off: u128) {
    let bitmap = shifted(off, 0..6000);
    let mut iter = bitmap.iter();
    assert_eq!((6000, Some(6000)), iter.size_hint());
    iter.by_ref().take(3000).for_each(drop);
    assert_eq!((3000, Some(3000)), iter.size_hint());
    iter.by_ref().for_each(drop);
    assert_eq!((0, Some(0)), iter.size_hint());
}

fn run(off: u128) {
    let mut bitmap = shifted(off, 0..6000);
    bitmap.optimize();
    let mut iter = bitmap.iter();
    assert_eq!((6000, Some(6000)), iter.size_hint());
    iter.by_ref().take(3000).for_each(drop);
    assert_eq!((3000, Some(3000)), iter.size_hint());
    iter.by_ref().for_each(drop);
    assert_eq!((0, Some(0)), iter.size_hint());
}

fn arrays(off: u128) {
    let bitmap = shifted(off, (0..2000).chain(1_000_000..1_002_000).chain(2_000_000..2_001_000));
    let mut iter = bitmap.iter();
    assert_eq!((5000, Some(5000)), iter.size_hint());
    iter.by_ref().take(3000).for_each(drop);
    assert_eq!((2000, Some(2000)), iter.size_hint());
    iter.by_ref().for_each(drop);
    assert_eq!((0, Some(0)), iter.size_hint());
}

fn bitmaps(off: u128) {
    let bitmap = shifted(off, (0..6000).chain(1_000_000..1_012_000).chain(2_000_000..2_010_000));
    let mut iter = bitmap.iter();
    assert_eq!((28000, Some(28000)), iter.size_hint());
    iter.by_ref().take(2000).for_each(drop);
    assert_eq!((26000, Some(26000)), iter.size_hint());
    iter.by_ref().take(5000).for_each(drop);
    assert_eq!((21000, Some(21000)), iter.size_hint());
    iter.by_ref().take(20000).for_each(drop);
    assert_eq!((1000, Some(1000)), iter.size_hint());
    iter.by_ref().for_each(drop);
    assert_eq!((0, Some(0)), iter.size_hint());
}

fn runs(off: u128) {
    let mut bitmap =
        shifted(off, (0..2000).chain(1_000_000..1_002_000).chain(2_000_000..2_001_000));
    bitmap.optimize();
    let mut iter = bitmap.iter();
    assert_eq!((5000, Some(5000)), iter.size_hint());
    iter.by_ref().take(3000).for_each(drop);
    assert_eq!((2000, Some(2000)), iter.size_hint());
    iter.by_ref().for_each(drop);
    assert_eq!((0, Some(0)), iter.size_hint());
}

/// `into_iter` and back iteration keep an exact size hint too.
fn into_iter_and_back(off: u128) {
    let bitmap = shifted(off, (0..6000).chain(1_000_000..1_012_000).chain(2_000_000..2_010_000));
    let mut iter = bitmap.into_iter();
    assert_eq!(iter.len(), 28000);
    iter.by_ref().rev().take(15000).for_each(drop);
    assert_eq!((13000, Some(13000)), iter.size_hint());
    iter.by_ref().take(7000).for_each(drop);
    assert_eq!((6000, Some(6000)), iter.size_hint());
    assert_eq!(iter.len(), 6000);
}

offset_variants!(array, bitmap, run, arrays, bitmaps, runs, into_iter_and_back);

#[test]
fn size_hint_above_u32_max_u128() {
    // 2^33 values in 131072 full containers: the hint must not be truncated to 32 bits.
    let mut bitmap = RoaringBitmap128::new();
    let n = 1u128 << 33;
    assert_eq!(bitmap.insert_range(u128::MAX - n + 1..=u128::MAX), 1 << 33);
    assert_eq!(bitmap.len(), 1 << 33);
    let mut iter = bitmap.iter();
    assert_eq!(iter.size_hint(), (1 << 33, Some(1 << 33)));
    assert_eq!(iter.next(), Some(u128::MAX - n + 1));
    assert_eq!(iter.next_back(), Some(u128::MAX));
    assert_eq!(iter.len(), (1 << 33) - 2);
}
