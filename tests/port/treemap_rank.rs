//! Port of `tests/treemap_rank.rs`.

use core::ops::RangeInclusive;

use proptest::collection::{btree_set, vec};
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

/// `BITMAP_MAX` of the original (`u32::MAX`), or another boundary for the 128-bit variants.
fn rank_roaring_bitmaps_at(bitmap_max: u128) {
    // The lower part contains one array container with the highest 1000 values below the
    // boundary, the higher part one bitmap container with the lowest 5000 values above it.
    let treemap = RoaringBitmap128::from_sorted_iter(bitmap_max - 1000..bitmap_max + 5000).unwrap();

    // start of treemap
    assert_eq!(treemap.rank(0), 0);

    // low boundary
    assert_eq!(treemap.rank(bitmap_max - 1002), 0);
    assert_eq!(treemap.rank(bitmap_max - 1001), 0);
    assert_eq!(treemap.rank(bitmap_max - 1000), 1);

    // middle range (spans two roaring bitmaps)
    assert_eq!(treemap.rank(bitmap_max - 1), 1000);
    assert_eq!(treemap.rank(bitmap_max), 1001);
    assert_eq!(treemap.rank(bitmap_max + 1), 1002);

    // high boundary
    assert_eq!(treemap.rank(bitmap_max + 4998), 5999);
    assert_eq!(treemap.rank(bitmap_max + 4999), 6000);
    assert_eq!(treemap.rank(bitmap_max + 5000), 6000);

    // end of treemap
    let m = u64::MAX as u128;
    let below_u64_max = (bitmap_max - 1000..bitmap_max + 5000)
        .filter(|&v| v <= m)
        .count() as u64;
    assert_eq!(treemap.rank(m), below_u64_max);
    assert_eq!(treemap.rank(u128::MAX), 6000);
}

variants!({
    orig = u32::MAX as u128,
    u64_max = u64::MAX as u128,
    p2_112 = (1u128 << 112) - 1,
    near_max = u128::MAX - 0x1_0000,
}; rank_roaring_bitmaps_at);

const BITMAP_MAX: u64 = u32::MAX as u64;

// A range that spans 2 roaring bitmaps with 2 containers each
const PROP_RANGE: RangeInclusive<u64> = BITMAP_MAX - (1 << 17)..=BITMAP_MAX + (1 << 17);

proptest! {
    #![proptest_config(config(128))]

    #[test]
    fn proptest_rank(
        values in btree_set(PROP_RANGE, ..=1000),
        checks in vec(PROP_RANGE, ..=100)
    ){
        let treemap = RoaringBitmap128::from_sorted_iter(values.iter().map(|&v| v as u128)).unwrap();
        for i in checks {
            let expected = values.iter().take_while(|&&x| x <= i).count() as u64;
            assert_eq!(treemap.rank(i as u128), expected);
        }
    }

    #[test]
    fn proptest_rank_u128(
        boundary in prop::sample::select(vec![1u128 << 64, 1u128 << 112, u128::MAX - (1 << 17)]),
        values in btree_set(0..=(1u128 << 18), ..=1000),
        checks in vec(0..=(1u128 << 18), ..=100)
    ){
        let base = boundary - (1 << 17);
        let values: Vec<u128> = values.into_iter().map(|v| base + v).collect();
        let treemap = RoaringBitmap128::from_sorted_iter(values.iter().copied()).unwrap();
        for i in checks.into_iter().map(|v| base + v) {
            prop_assert_eq!(treemap.rank(i), rank_in(&values, i));
        }
    }
}
