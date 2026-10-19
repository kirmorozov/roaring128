//! Port of `tests/union_with.rs` (identical to `tests/treemap_union_with.rs`).

use roaring128::RoaringBitmap128;

use crate::common::*;

fn check(mut bitmap1: RoaringBitmap128, bitmap2: RoaringBitmap128, bitmap3: RoaringBitmap128) {
    bitmap1 |= bitmap2;
    assert_valid(&bitmap1);
    assert_eq!(bitmap1, bitmap3);
}

fn array_to_array(off: u128) {
    check(shifted(off, 0..2000), shifted(off, 1000..3000), shifted(off, 0..3000));
}

fn array_to_bitmap(off: u128) {
    check(shifted(off, 0..4000), shifted(off, 4000..8000), shifted(off, 0..8000));
}

fn array_and_bitmap(off: u128) {
    check(shifted(off, 0..2000), shifted(off, 1000..8000), shifted(off, 0..8000));
}

fn bitmap(off: u128) {
    check(shifted(off, 0..12000), shifted(off, 6000..18000), shifted(off, 0..18000));
}

fn bitmap_and_array(off: u128) {
    check(shifted(off, 0..12000), shifted(off, 10000..13000), shifted(off, 0..13000));
}

fn arrays(off: u128) {
    check(
        shifted(off, (0..2000).chain(1_000_000..1_002_000).chain(3_000_000..3_001_000)),
        shifted(off, (1000..3000).chain(1_001_000..1_003_000).chain(2_000_000..2_001_000)),
        shifted(
            off,
            (0..3000)
                .chain(1_000_000..1_003_000)
                .chain(2_000_000..2_001_000)
                .chain(3_000_000..3_001_000),
        ),
    );
}

fn bitmaps(off: u128) {
    check(
        shifted(off, (0..6000).chain(1_000_000..1_012_000).chain(3_000_000..3_010_000)),
        shifted(off, (3000..9000).chain(1_006_000..1_018_000).chain(2_000_000..2_010_000)),
        shifted(
            off,
            (0..9000)
                .chain(1_000_000..1_018_000)
                .chain(2_000_000..2_010_000)
                .chain(3_000_000..3_010_000),
        ),
    );
}

offset_variants!(array_to_array, array_to_bitmap, array_and_bitmap, bitmap, bitmap_and_array, arrays, bitmaps);

#[test]
fn runs_and_sparse_keys_u128() {
    // run containers (insert_range) united with arrays/bitmaps in far apart keys
    let mut a = RoaringBitmap128::new();
    a.insert_range(u128::MAX - 200_000..=u128::MAX);
    a.insert_range((1 << 64) - 10..(1 << 64) + 10);
    let b: RoaringBitmap128 =
        [0u128, 1 << 100, u128::MAX - 300_000, u128::MAX].into_iter().chain((1 << 64) - 100..(1 << 64)).collect();
    let mut expected: Model = (u128::MAX - 200_000..=u128::MAX).collect();
    expected.extend((1 << 64) - 10..(1 << 64) + 10);
    expected.extend([0u128, 1 << 100, u128::MAX - 300_000]);
    expected.extend((1 << 64) - 100..(1 << 64));
    a |= &b;
    assert_matches(&a, &expected);
}
