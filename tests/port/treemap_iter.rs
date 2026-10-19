//! Port of `tests/treemap_iter.rs`.
//!
//! `bitmaps_iterator` (`RoaringTreemap::bitmaps` / `from_bitmaps`) has no equivalent: the 128-bit
//! bitmap is not made of 32-bit bitmaps.

use proptest::arbitrary::any;
use proptest::collection::btree_set;
use proptest::proptest;
use roaring128::RoaringBitmap128;

use crate::common::*;

fn range(off: u128) {
    let original = shifted(off, 0..2000);
    let clone = RoaringBitmap128::from_iter(&original);
    let clone2 = RoaringBitmap128::from_iter(original.clone());

    assert_eq!(clone, original);
    assert_eq!(clone2, original);
}

fn array(off: u128) {
    let original = shifted(off, 0..5);
    let clone = RoaringBitmap128::from([off, off + 1, off + 2, off + 3, off + 4]);

    assert_eq!(clone, original);
}

fn bitmap(off: u128) {
    let original = shifted(off, 0..6000);
    let clone = RoaringBitmap128::from_iter(&original);
    let clone2 = RoaringBitmap128::from_iter(original.clone());

    assert_eq!(clone, original);
    assert_eq!(clone2, original);
}

fn arrays(off: u128) {
    let original = shifted(
        off,
        (0..2000)
            .chain(1_000_000..1_002_000)
            .chain(2_000_000..2_001_000),
    );
    let clone = RoaringBitmap128::from_iter(&original);
    let clone2 = RoaringBitmap128::from_iter(original.clone());

    assert_eq!(clone, original);
    assert_eq!(clone2, original);
}

fn bitmaps(off: u128) {
    let original = shifted(
        off,
        (0..6000)
            .chain(1_000_000..1_012_000)
            .chain(2_000_000..2_010_000),
    );
    let clone = RoaringBitmap128::from_iter(&original);
    let clone2 = RoaringBitmap128::from_iter(original.clone());

    assert_eq!(clone, original);
    assert_eq!(clone2, original);
}

offset_variants!(range, array, bitmap, arrays, bitmaps);

fn rev_values(off: u128) -> impl DoubleEndedIterator<Item = u128> + Clone {
    (1..3u128)
        .chain(1_000_000..1_012_003)
        .chain(2_000_001..2_000_003)
        .chain(2_000_000_000_001..2_000_000_000_003)
        .map(move |v| off + v)
}

fn rev(off: u128) {
    let values = rev_values(off);
    let bitmap = RoaringBitmap128::from_iter(values.clone());

    assert!(values.rev().eq(bitmap.iter().rev()));
}

fn interleaved(off: u128) {
    let values = rev_values(off);
    let bitmap = RoaringBitmap128::from_iter(values.clone());

    assert!(outside_in(values).eq(outside_in(bitmap)));
}

wide_offset_variants!(rev, interleaved);

#[test]
fn from_iter() {
    // This test verifies that the public API allows conversion from iterators
    // with u128 as well as &u128 elements.
    let vals = vec![1, 5, 1_000_000_000_000_000, 1 << 64, u128::MAX];
    let a = RoaringBitmap128::from_iter(vals.iter());
    let b = RoaringBitmap128::from_iter(vals);
    assert_eq!(a, b);
}

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn iter(values in btree_set(any::<u64>(), ..=10_000)) {
        let values: Vec<u128> = values.into_iter().map(u128::from).collect();
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();

        assert!(values.into_iter().eq(bitmap));
    }

    #[test]
    fn fold(values in btree_set(any::<u64>(), ..=10_000)) {
        let values: Vec<u128> = values.into_iter().map(u128::from).collect();
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        let mut val_iter = values.into_iter();
        // `Iterator::all` uses currently unimplementable `try_fold`, we test `fold`
        #[allow(clippy::unnecessary_fold)]
        let r = bitmap.into_iter().fold(true, |b, i| {
            b && i == val_iter.next().unwrap()
        });
        assert!(r)
    }

    #[test]
    fn rev_iter(values in btree_set(any::<u64>(), ..=10_000)) {
        let values: Vec<u128> = values.into_iter().map(u128::from).collect();
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();

        assert!(values.into_iter().rev().eq(bitmap.iter().rev()));
    }

    #[test]
    fn rev_iter_u128(values in btree_set(any::<u128>(), ..=10_000)) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();

        assert!(values.into_iter().rev().eq(bitmap.iter().rev()));
    }
}

proptest! {
    #![proptest_config(config(16))]

    #[test]
    fn interleaved_iter(values in btree_set(any::<u64>(), 50_000..=100_000)) {
        let values: Vec<u128> = values.into_iter().map(u128::from).collect();
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();

        assert!(outside_in(values).eq(outside_in(bitmap)));
    }

    #[test]
    fn interleaved_iter_u128(values in btree_set(any::<u128>(), 50_000..=100_000)) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();

        assert!(outside_in(values).eq(outside_in(bitmap)));
    }
}
