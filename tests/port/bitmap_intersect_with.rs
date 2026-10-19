//! Port of `tests/intersect_with.rs` (identical to `tests/treemap_intersect_with.rs`).

use roaring128::RoaringBitmap128;

use crate::common::*;

fn check(mut bitmap1: RoaringBitmap128, bitmap2: RoaringBitmap128, bitmap3: RoaringBitmap128) {
    bitmap1 &= bitmap2;
    assert_valid(&bitmap1);
    assert_eq!(bitmap1, bitmap3);
}

fn array(off: u128) {
    check(shifted(off, 0..2000), shifted(off, 1000..3000), shifted(off, 1000..2000));
}

fn no_intersection(off: u128) {
    check(shifted(off, 0..2), shifted(off, 3..4), RoaringBitmap128::new());
}

fn array_and_bitmap(off: u128) {
    check(shifted(off, 0..2000), shifted(off, 1000..8000), shifted(off, 1000..2000));
}

fn bitmap_to_bitmap(off: u128) {
    check(shifted(off, 0..12000), shifted(off, 6000..18000), shifted(off, 6000..12000));
}

fn bitmap_to_array(off: u128) {
    check(shifted(off, 0..6000), shifted(off, 3000..9000), shifted(off, 3000..6000));
}

fn bitmap_and_array(off: u128) {
    check(shifted(off, 0..12000), shifted(off, 7000..9000), shifted(off, 7000..9000));
}

fn arrays(off: u128) {
    check(
        shifted(off, (0..2000).chain(1_000_000..1_002_000).chain(3_000_000..3_001_000)),
        shifted(off, (1000..3000).chain(1_001_000..1_003_000).chain(2_000_000..2_001_000)),
        shifted(off, (1000..2000).chain(1_001_000..1_002_000)),
    );
}

fn bitmaps(off: u128) {
    check(
        shifted(off, (0..6000).chain(1_000_000..1_012_000).chain(3_000_000..3_010_000)),
        shifted(off, (3000..9000).chain(1_006_000..1_018_000).chain(2_000_000..2_010_000)),
        shifted(off, (3000..6000).chain(1_006_000..1_012_000)),
    );
}

offset_variants!(
    array,
    no_intersection,
    array_and_bitmap,
    bitmap_to_bitmap,
    bitmap_to_array,
    bitmap_and_array,
    arrays,
    bitmaps
);

#[test]
fn same_low_bits_different_high_keys_u128() {
    // Values sharing their low 64 bits but differing in the high bits must not intersect.
    let a: RoaringBitmap128 = (0..5000u128).map(|v| v | (1 << 64)).collect();
    let b: RoaringBitmap128 = (0..5000u128).map(|v| v | (1 << 65)).collect();
    let c: RoaringBitmap128 = (0..5000u128).collect();
    assert!((&a & &b).is_empty());
    assert!((&a & &c).is_empty());
    let mut x = a.clone();
    x &= &b;
    assert_valid(&x);
    assert!(x.is_empty());
}

#[test]
fn run_with_array_at_max_u128() {
    let mut a = RoaringBitmap128::new();
    a.insert_range(u128::MAX - 100_000..=u128::MAX);
    let b: RoaringBitmap128 = [u128::MAX, u128::MAX - 70_000, u128::MAX - 200_000, 7].into_iter().collect();
    let r = &a & &b;
    assert_valid(&r);
    assert!(r.iter().eq([u128::MAX - 70_000, u128::MAX]));
}
