//! Port of `tests/treemap_select.rs`.

use proptest::collection::btree_set;
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

#[test]
fn select() {
    let bitmap = (0..2000u128).collect::<RoaringBitmap128>();

    assert_eq!(bitmap.select(0), Some(0));
}

/// `far` is `u32::MAX` in the original.
fn select_multiple_bitmap(far: u128) {
    let mut bitmap = (0..100_000u128).collect::<RoaringBitmap128>();
    bitmap.append(far..=far + 99_999).expect("sorted integers");

    assert_eq!(bitmap.select(0), Some(0));
    assert_eq!(bitmap.select(99_999), Some(99_999));
    assert_eq!(bitmap.select(100_000), Some(far));
    assert_eq!(bitmap.select(199_999), Some(far + 99_999));
    assert_eq!(bitmap.select(200_000), None);
    assert_eq!(bitmap.select(u64::MAX), None);
}

variants!({
    orig = u32::MAX as u128,
    far_u64_max = u64::MAX as u128,
    far_2p100 = 1u128 << 100,
    far_max = u128::MAX - 99_999,
}; select_multiple_bitmap);

#[test]
fn select_empty() {
    let bitmap = RoaringBitmap128::new();

    assert_eq!(bitmap.select(0), None);
    assert_eq!(bitmap.select(1024), None);
    assert_eq!(bitmap.select(u64::MAX), None);
}

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn proptest_select(values in btree_set(any::<u64>(), 1000)) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().map(|&v| v as u128)).unwrap();
        for (i, value) in values.iter().cloned().enumerate() {
            prop_assert_eq!(bitmap.select(i as u64), Some(value as u128));
        }
    }

    #[test]
    fn proptest_select_u128(values in btree_set(any::<u128>(), 1000)) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        for (i, value) in values.iter().cloned().enumerate() {
            prop_assert_eq!(bitmap.select(i as u64), Some(value));
        }
        prop_assert_eq!(bitmap.select(1000), None);
    }
}
