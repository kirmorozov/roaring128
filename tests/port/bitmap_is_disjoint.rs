//! Port of `tests/is_disjoint.rs` (identical to `tests/treemap_is_disjoint.rs`).

use roaring128::RoaringBitmap128;

use crate::common::*;

fn array(off: u128) {
    let bitmap1 = shifted(off, 0..2000);
    let bitmap2 = shifted(off, 4000..6000);
    assert!(bitmap1.is_disjoint(&bitmap2));
}

fn array_not(off: u128) {
    let bitmap1 = shifted(off, 0..4000);
    let bitmap2 = shifted(off, 2000..6000);
    assert!(!bitmap1.is_disjoint(&bitmap2));
}

fn bitmap(off: u128) {
    let bitmap1 = shifted(off, 0..6000);
    let bitmap2 = shifted(off, 10000..16000);
    assert!(bitmap1.is_disjoint(&bitmap2));
}

fn bitmap_not(off: u128) {
    let bitmap1 = shifted(off, 0..10000);
    let bitmap2 = shifted(off, 5000..15000);
    assert!(!bitmap1.is_disjoint(&bitmap2));
}

fn arrays(off: u128) {
    let bitmap1 = shifted(off, (0..2000).chain(1_000_000..1_002_000).chain(2_000_000..2_002_000));
    let bitmap2 = shifted(off, (100_000..102_000).chain(1_100_000..1_102_000));
    assert!(bitmap1.is_disjoint(&bitmap2));
}

fn arrays_not(off: u128) {
    let bitmap1 = shifted(off, (0..2_000).chain(1_000_000..1_002_000).chain(2_000_000..2_002_000));
    let bitmap2 = shifted(off, (100_000..102_000).chain(1_001_000..1_003_000));
    assert!(!bitmap1.is_disjoint(&bitmap2));
}

fn bitmaps(off: u128) {
    let bitmap1 = shifted(off, (0..6000).chain(1_000_000..1_006_000).chain(2_000_000..2_006_000));
    let bitmap2 = shifted(off, (100_000..106_000).chain(1_100_000..1_106_000));
    assert!(bitmap1.is_disjoint(&bitmap2));
}

fn bitmaps_not(off: u128) {
    let bitmap1 = shifted(off, (0..6000).chain(1_000_000..1_006_000).chain(2_000_000..2_006_000));
    let bitmap2 = shifted(off, (100_000..106_000).chain(1_004_000..1_008_000));
    assert!(!bitmap1.is_disjoint(&bitmap2));
}

offset_variants!(array, array_not, bitmap, bitmap_not, arrays, arrays_not, bitmaps, bitmaps_not);

#[test]
fn disjoint_high_bits_u128() {
    let low: RoaringBitmap128 = (0..10_000u128).collect();
    let high: RoaringBitmap128 = (0..10_000u128).map(|v| v | (1 << 127)).collect();
    assert!(low.is_disjoint(&high));
    assert!(high.is_disjoint(&low));
    let empty = RoaringBitmap128::new();
    assert!(empty.is_disjoint(&empty));
    assert!(empty.is_disjoint(&high));
    assert!(high.is_disjoint(&empty));
    let both = &low | &high;
    assert!(!both.is_disjoint(&high));
    assert!(!RoaringBitmap128::from([u128::MAX]).is_disjoint(&RoaringBitmap128::from([0, u128::MAX])));
}
