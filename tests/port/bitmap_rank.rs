//! Port of `tests/rank.rs`.

use proptest::collection::{btree_set, vec};
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

fn rank(off: u128) {
    let mut bitmap = RoaringBitmap128::from_sorted_iter((0..2000).map(|v| off + v)).unwrap();
    bitmap.insert_range(off + 200_000..off + 210_000);

    // No matching container
    assert_eq!(bitmap.rank(off + 80_000), 2000);
    assert_eq!(bitmap.rank(off.saturating_add(u32::MAX as u128)), 12_000);
    assert_eq!(bitmap.rank(u128::MAX), 12_000);

    // Array container at key
    assert_eq!(bitmap.rank(off), 1);
    assert_eq!(bitmap.rank(off + 100), 101);
    assert_eq!(bitmap.rank(off + 2000), 2000);

    // Bitmap container at key
    assert_eq!(bitmap.rank(off + 200_000), 2001);
    assert_eq!(bitmap.rank(off + 210_000), 12_000);
}

fn rank_array(off: u128) {
    let bitmap = RoaringBitmap128::from_sorted_iter((0..2000).map(|v| off + v)).unwrap();

    // No matching container
    assert_eq!(bitmap.rank(off.saturating_add(u32::MAX as u128)), 2000);
    assert_eq!(bitmap.rank(u128::MAX), 2000);

    // Has container (array)
    assert_eq!(bitmap.rank(off), 1);
    assert_eq!(bitmap.rank(off + 100), 101);
    assert_eq!(bitmap.rank(off + 2000), 2000);
    assert_eq!(bitmap.rank(off + 3000), 2000);
}

fn rank_bitmap(off: u128) {
    let bitmap = RoaringBitmap128::from_sorted_iter((0..5000).map(|v| off + v)).unwrap();

    // key: 0, bit: 0
    assert_eq!(bitmap.rank(off), 1);

    // key: 0, bit: 63 (mask of all ones)
    assert_eq!(bitmap.rank(off + 63), 64);

    // key: 1023, bit: 0
    assert_eq!(bitmap.rank(off + 65535), 5000);

    // key: 1023, bit: 63 (mask of all ones)
    assert_eq!(bitmap.rank(off + 65472), 5000);

    assert_eq!(bitmap.rank(off + 1), 2);
    assert_eq!(bitmap.rank(off + 100), 101);
    assert_eq!(bitmap.rank(off + 1000), 1001);
    assert_eq!(bitmap.rank(off + 4999), 5000);
}

/// Values below the offset have rank 0.
fn rank_below(off: u128) {
    let bitmap = RoaringBitmap128::from_sorted_iter((0..5000).map(|v| off + v)).unwrap();
    if off > 0 {
        assert_eq!(bitmap.rank(off - 1), 0);
        assert_eq!(bitmap.rank(0), 0);
    }
    assert_eq!(bitmap.rank(u128::MAX), 5000);
}

offset_variants!(rank, rank_array, rank_bitmap, rank_below);

#[test]
fn rank_u128_max() {
    let mut bitmap = RoaringBitmap128::new();
    assert_eq!(bitmap.rank(u128::MAX), 0);
    bitmap.insert_range(u128::MAX - 99_999..);
    bitmap.insert(0);
    assert_eq!(bitmap.rank(u128::MAX), 100_001);
    assert_eq!(bitmap.rank(u128::MAX - 1), 100_000);
    assert_eq!(bitmap.rank(u128::MAX - 99_999), 2);
    assert_eq!(bitmap.rank(u128::MAX - 100_000), 1);
    assert_eq!(bitmap.rank(0), 1);
    assert_eq!(bitmap.rank(u128::MAX), bitmap.len());
}

proptest! {
    #![proptest_config(config(128))]

    #[test]
    fn proptest_rank(
        values in btree_set(..=262_143_u32, ..=1000),
        checks in vec(..=262_143_u32, ..=100)
    ){
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().map(|&v| v as u128)).unwrap();
        for i in checks {
            let expected = values.iter().take_while(|&&x| x <= i).count() as u64;
            assert_eq!(bitmap.rank(i as u128), expected);
        }
    }

    #[test]
    fn proptest_rank_u128(
        values in btree_set(value(), ..=1000),
        checks in vec(value(), ..=100)
    ){
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        let sorted: Vec<u128> = values.iter().copied().collect();
        for i in checks.into_iter().chain(sorted.iter().copied()) {
            prop_assert_eq!(bitmap.rank(i), rank_in(&sorted, i), "rank({:#x})", i);
        }
    }
}
