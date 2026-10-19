//! Port of `tests/select.rs`.

use proptest::collection::btree_set;
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

fn select(off: u128) {
    let bitmap = shifted(off, 0..2000);

    assert_eq!(bitmap.select(0), Some(off));
}

fn select_array(off: u128) {
    let bitmap = shifted(off, 0..2000);

    assert_eq!(bitmap.select(0), Some(off));
    assert_eq!(bitmap.select(100), Some(off + 100));
    assert_eq!(bitmap.select(1000), Some(off + 1000));
    assert_eq!(bitmap.select(1999), Some(off + 1999));
    assert_eq!(bitmap.select(2000), None);
}

fn select_bitmap(off: u128) {
    let bitmap = shifted(off, 0..100_000);

    assert_eq!(bitmap.select(0), Some(off));
    assert_eq!(bitmap.select(63), Some(off + 63));
    assert_eq!(bitmap.select(1000), Some(off + 1000));
    assert_eq!(bitmap.select(65535), Some(off + 65535));
    assert_eq!(bitmap.select(65536), Some(off + 65536));
    assert_eq!(bitmap.select(99_999), Some(off + 99_999));
    assert_eq!(bitmap.select(100_000), None);
}

fn select_run(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 200_000);
    assert_eq!(bitmap.select(0), Some(off));
    assert_eq!(bitmap.select(65_535), Some(off + 65_535));
    assert_eq!(bitmap.select(65_536), Some(off + 65_536));
    assert_eq!(bitmap.select(199_999), Some(off + 199_999));
    assert_eq!(bitmap.select(200_000), None);
}

offset_variants!(select, select_array, select_bitmap, select_run);

#[test]
fn select_empty() {
    let bitmap = RoaringBitmap128::new();

    assert_eq!(bitmap.select(0), None);
    assert_eq!(bitmap.select(1024), None);
    assert_eq!(bitmap.select(u32::MAX as u64), None);
    assert_eq!(bitmap.select(u64::MAX), None);
}

#[test]
fn select_u128_max() {
    let bitmap = RoaringBitmap128::from([0, 1 << 64, u128::MAX]);
    assert_eq!(bitmap.select(0), Some(0));
    assert_eq!(bitmap.select(1), Some(1 << 64));
    assert_eq!(bitmap.select(2), Some(u128::MAX));
    assert_eq!(bitmap.select(3), None);
    assert_eq!(bitmap.select(u64::MAX), None);
}

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn proptest_select(values in btree_set(any::<u32>(), 1000)) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().map(|&v| v as u128)).unwrap();
        for (i, value) in values.iter().cloned().enumerate() {
            prop_assert_eq!(bitmap.select(i as u64), Some(value as u128));
        }
    }

    #[test]
    fn proptest_select_u128(values in proptest::collection::vec(value(), 1000)) {
        // `value()` has repeated special values: collect instead of asking for exactly 1000
        let values: Model = values.into_iter().collect();
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        for (i, value) in values.iter().cloned().enumerate() {
            prop_assert_eq!(bitmap.select(i as u64), Some(value));
            prop_assert_eq!(bitmap.rank(value), i as u64 + 1);
        }
        prop_assert_eq!(bitmap.select(values.len() as u64), None);
    }
}
