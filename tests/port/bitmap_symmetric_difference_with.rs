//! Port of `tests/symmetric_difference_with.rs` (identical to
//! `tests/treemap_symmetric_difference_with.rs`).

use roaring128::RoaringBitmap128;

use crate::common::*;

fn check(mut bitmap1: RoaringBitmap128, bitmap2: RoaringBitmap128, bitmap3: RoaringBitmap128) {
    bitmap1 ^= bitmap2;
    assert_valid(&bitmap1);
    assert_eq!(bitmap1, bitmap3);
}

fn array(off: u128) {
    check(
        shifted(off, 0..2000),
        shifted(off, 1000..3000),
        shifted(off, (0..1000).chain(2000..3000)),
    );
}

fn no_symmetric_difference(off: u128) {
    check(
        shifted(off, 0..2),
        shifted(off, 0..2),
        RoaringBitmap128::new(),
    );
}

fn array_and_bitmap(off: u128) {
    check(
        shifted(off, 0..2000),
        shifted(off, 1000..8000),
        shifted(off, (0..1000).chain(2000..8000)),
    );
}

fn bitmap_to_bitmap(off: u128) {
    check(
        shifted(off, 0..12000),
        shifted(off, 6000..18000),
        shifted(off, (0..6000).chain(12000..18000)),
    );
}

fn bitmap_to_array(off: u128) {
    check(
        shifted(off, 0..6000),
        shifted(off, 2000..7000),
        shifted(off, (0..2000).chain(6000..7000)),
    );
}

fn bitmap_and_array_to_bitmap(off: u128) {
    check(
        shifted(off, 0..12000),
        shifted(off, 11000..14000),
        shifted(off, (0..11000).chain(12000..14000)),
    );
}

fn bitmap_and_array_to_array(off: u128) {
    check(
        shifted(off, 0..6000),
        shifted(off, 3000..7000),
        shifted(off, (0..3000).chain(6000..7000)),
    );
}

fn arrays(off: u128) {
    check(
        shifted(
            off,
            (0..2000)
                .chain(1_000_000..1_002_000)
                .chain(3_000_000..3_001_000),
        ),
        shifted(
            off,
            (1000..3000)
                .chain(1_001_000..1_003_000)
                .chain(2_000_000..2_000_001),
        ),
        shifted(
            off,
            (0..1000)
                .chain(1_000_000..1_001_000)
                .chain(2000..3000)
                .chain(1_002_000..1_003_000)
                .chain(2_000_000..2_000_001)
                .chain(3_000_000..3_001_000),
        ),
    );
}

fn bitmaps(off: u128) {
    check(
        shifted(
            off,
            (0..6000)
                .chain(1_000_000..1_012_000)
                .chain(3_000_000..3_010_000),
        ),
        shifted(
            off,
            (3000..7000)
                .chain(1_006_000..1_018_000)
                .chain(2_000_000..2_010_000),
        ),
        shifted(
            off,
            (0..3000)
                .chain(1_000_000..1_006_000)
                .chain(6000..7000)
                .chain(1_012_000..1_018_000)
                .chain(2_000_000..2_010_000)
                .chain(3_000_000..3_010_000),
        ),
    );
}

offset_variants!(
    array,
    no_symmetric_difference,
    array_and_bitmap,
    bitmap_to_bitmap,
    bitmap_to_array,
    bitmap_and_array_to_bitmap,
    bitmap_and_array_to_array,
    arrays,
    bitmaps
);

#[test]
fn xor_full_containers_u128() {
    // Two full containers (runs) at the very top cancel out; partial overlap leaves the rest.
    let mut a = RoaringBitmap128::new();
    a.insert_range(u128::MAX - 0x1_FFFF..=u128::MAX);
    let mut b = RoaringBitmap128::new();
    b.insert_range(u128::MAX - 0xFFFF..=u128::MAX);
    let x = &a ^ &b;
    assert_valid(&x);
    assert_eq!(x.len(), 0x1_0000);
    assert_eq!(x.min(), Some(u128::MAX - 0x1_FFFF));
    assert_eq!(x.max(), Some(u128::MAX - 0x1_0000));
    let mut y = a.clone();
    y ^= a;
    assert_valid(&y);
    assert!(y.is_empty());
}
