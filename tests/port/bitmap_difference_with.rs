//! Port of `tests/difference_with.rs` (identical to `tests/treemap_difference_with.rs`).

use roaring128::RoaringBitmap128;

use crate::common::*;

fn check(mut bitmap1: RoaringBitmap128, bitmap2: RoaringBitmap128, bitmap3: RoaringBitmap128) {
    bitmap1 -= bitmap2;
    assert_valid(&bitmap1);
    assert_eq!(bitmap1, bitmap3);
}

fn array(off: u128) {
    check(
        shifted(off, 0..2000),
        shifted(off, 1000..3000),
        shifted(off, 0..1000),
    );
}

fn no_difference(off: u128) {
    check(
        shifted(off, 1..3),
        shifted(off, 1..3),
        RoaringBitmap128::new(),
    );
}

fn array_and_bitmap(off: u128) {
    check(
        shifted(off, 0..2000),
        shifted(off, 1000..8000),
        shifted(off, 0..1000),
    );
}

fn bitmap_to_bitmap(off: u128) {
    check(
        shifted(off, 0..12000),
        shifted(off, 6000..18000),
        shifted(off, 0..6000),
    );
}

fn bitmap_to_array(off: u128) {
    check(
        shifted(off, 0..6000),
        shifted(off, 3000..9000),
        shifted(off, 0..3000),
    );
}

fn bitmap_and_array_to_bitmap(off: u128) {
    check(
        shifted(off, 0..12000),
        shifted(off, 9000..12000),
        shifted(off, 0..9000),
    );
}

fn bitmap_and_array_to_array(off: u128) {
    check(
        shifted(off, 0..6000),
        shifted(off, 3000..6000),
        shifted(off, 0..3000),
    );
}

fn arrays(off: u128) {
    check(
        shifted(
            off,
            (0..2000)
                .chain(1_000_000..1_002_000)
                .chain(2_000_000..2_001_000),
        ),
        shifted(
            off,
            (1000..3000)
                .chain(1_001_000..1_003_000)
                .chain(2_000_000..2_001_000),
        ),
        shifted(off, (0..1000).chain(1_000_000..1_001_000)),
    );
}

fn arrays_removing_one_whole_container(off: u128) {
    check(
        shifted(
            off,
            (0..2000)
                .chain(1_000_000..1_002_000)
                .chain(2_000_000..2_001_000),
        ),
        shifted(
            off,
            (0..3000)
                .chain(1_001_000..1_003_000)
                .chain(2_000_000..2_001_000),
        ),
        shifted(off, 1_000_000..1_001_000),
    );
}

fn bitmaps(off: u128) {
    check(
        shifted(
            off,
            (0..6000)
                .chain(1_000_000..1_012_000)
                .chain(2_000_000..2_010_000),
        ),
        shifted(
            off,
            (3000..9000)
                .chain(1_006_000..1_018_000)
                .chain(2_000_000..2_010_000),
        ),
        shifted(off, (0..3000).chain(1_000_000..1_006_000)),
    );
}

offset_variants!(
    array,
    no_difference,
    array_and_bitmap,
    bitmap_to_bitmap,
    bitmap_to_array,
    bitmap_and_array_to_bitmap,
    bitmap_and_array_to_array,
    arrays,
    arrays_removing_one_whole_container,
    bitmaps
);

#[test]
fn difference_removes_every_container_u128() {
    let a: RoaringBitmap128 = [0u128, 1 << 64, (1 << 112) + 3, u128::MAX]
        .into_iter()
        .collect();
    let mut b = a.clone();
    b.insert(42);
    let mut x = a.clone();
    x -= &b;
    assert_valid(&x);
    assert!(x.is_empty());
    assert_eq!(x.len(), 0);
    assert_eq!(x.min(), None);
    assert_eq!(&b - &a, RoaringBitmap128::from([42]));
}
