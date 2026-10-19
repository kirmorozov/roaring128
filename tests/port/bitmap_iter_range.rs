//! Port of `tests/iter_range.rs`.

use std::ops::Bound;

use proptest::collection::btree_set;
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

fn range_array(off: u128) {
    let mut rb = RoaringBitmap128::new();
    rb.insert(off);
    rb.insert(off + 1);
    rb.insert(off + 10);
    rb.insert(off + 100_000);
    rb.insert(off + 999_999);
    rb.insert(off + 1_000_000);

    let expected = shift_vec(off, [1, 10, 100_000, 999_999]);
    let actual: Vec<u128> = rb.range(off + 1..=off + 999_999).collect();
    assert_eq!(expected, actual);
}

fn range_bitmap(off: u128) {
    let rb = RoaringBitmap128::from_sorted_iter((10..5000).map(|v| off + v)).unwrap();

    let expected = shift_vec(off, [10, 11, 12]);
    let actual: Vec<u128> = rb.range(off..off + 13).collect();
    assert_eq!(expected, actual);
}

fn empty_range(off: u128) {
    let rb = RoaringBitmap128::from_sorted_iter((10..5000).map(|v| off + v)).unwrap();

    let mut it = rb.range(off..off);
    assert_eq!(it.next(), None);
    let mut it = rb.range(..off);
    assert_eq!(it.next(), None);
    it = rb.range(off + 13..off + 13);
    assert_eq!(it.next(), None);
    it = rb.range((Bound::Excluded(off + 1), Bound::Included(off + 1)));
    assert_eq!(it.next(), None);
    it = rb.range(u128::MAX..u128::MAX);
    assert_eq!(it.next(), None);
    it = rb.range((Bound::Excluded(u128::MAX), Bound::Included(u128::MAX)));
    assert_eq!(it.next(), None);
    it = rb.range((Bound::Excluded(u128::MAX), Bound::Unbounded));
    assert_eq!(it.next(), None);
    assert_eq!(it.size_hint(), (0, Some(0)));
}

offset_variants!(range_array, range_bitmap, empty_range);

#[test]
fn empty_range_u32_max() {
    // the original used u32::MAX as the end of the space; in 128 bits it is an ordinary value
    let m = u32::MAX as u128;
    let rb = RoaringBitmap128::from([m, m + 1]);
    assert_eq!(rb.range(m..m).next(), None);
    assert!(rb.range((Bound::Excluded(m), Bound::Included(m + 1))).eq([m + 1]));
    assert!(rb.range((Bound::Excluded(m), Bound::Unbounded)).eq([m + 1]));
}

#[test]
fn range_ending_at_max_u128() {
    let rb = RoaringBitmap128::from([0, 1 << 64, u128::MAX - 1, u128::MAX]);
    assert!(rb.range(u128::MAX - 1..).eq([u128::MAX - 1, u128::MAX]));
    assert!(rb.range(u128::MAX..=u128::MAX).eq([u128::MAX]));
    assert!(rb.range(..=u128::MAX).eq([0, 1 << 64, u128::MAX - 1, u128::MAX]));
    assert!(rb.range(..u128::MAX).eq([0, 1 << 64, u128::MAX - 1]));
    assert!(rb.range(..).eq([0, 1 << 64, u128::MAX - 1, u128::MAX]));
    assert!(rb.range((Bound::Excluded(0), Bound::Excluded(u128::MAX))).eq([1 << 64, u128::MAX - 1]));
    assert!(rb.range((1 << 64)..(1 << 64) + 1).eq([1 << 64]));
    assert!(rb.clone().into_range(1..).eq([1 << 64, u128::MAX - 1, u128::MAX]));
    assert!(rb.clone().into_range(..).rev().eq([u128::MAX, u128::MAX - 1, 1 << 64, 0]));
    assert_eq!(rb.range(1..u128::MAX).len(), 2);
}

#[test]
#[should_panic(expected = "range start is greater than range end")]
fn invalid_range() {
    let rb = RoaringBitmap128::from_sorted_iter(10..5000).unwrap();
    #[allow(clippy::reversed_empty_ranges)]
    let _ = rb.range(13..0);
}

#[test]
#[should_panic(expected = "range start and end are equal and excluded")]
fn invalid_range_equal_excluded() {
    let rb = RoaringBitmap128::from_sorted_iter(10..5000).unwrap();
    let _ = rb.range((Bound::Excluded(13), Bound::Excluded(13)));
}

#[test]
#[should_panic(expected = "range start is greater than range end")]
fn into_invalid_range() {
    let rb = RoaringBitmap128::from_sorted_iter(10..5000).unwrap();
    #[allow(clippy::reversed_empty_ranges)]
    let _ = rb.into_range(13..0);
}

#[test]
#[should_panic(expected = "range start and end are equal and excluded")]
fn into_invalid_range_equal_excluded() {
    let rb = RoaringBitmap128::from_sorted_iter(10..5000).unwrap();
    let _ = rb.into_range((Bound::Excluded(13), Bound::Excluded(13)));
}

#[test]
#[should_panic(expected = "range start is greater than range end")]
fn invalid_range_u128() {
    let rb = RoaringBitmap128::from([1, u128::MAX]);
    let _ = rb.range((Bound::Included(u128::MAX), Bound::Included(1 << 64)));
}

#[test]
#[should_panic(expected = "range start and end are equal and excluded")]
fn invalid_range_equal_excluded_u128() {
    let rb = RoaringBitmap128::from([1, u128::MAX]);
    let _ = rb.range((Bound::Excluded(u128::MAX), Bound::Excluded(u128::MAX)));
}

#[test]
fn range_on_empty_bitmap_is_empty() {
    // valid ranges on an empty bitmap do not panic
    let rb = RoaringBitmap128::new();
    assert_eq!(rb.range(..).next(), None);
    assert_eq!(rb.range(5..=5).next_back(), None);
    assert_eq!(rb.clone().into_range(u128::MAX..).next(), None);
}

proptest! {
    #![proptest_config(config(128))]

    #[test]
    fn proptest_range(
        values in btree_set(..=262_143_u32, ..=1000),
        range_a in 0u32..262_143,
        range_b in 0u32..262_143,
    ){
        let range = range_a.min(range_b) as u128..=range_a.max(range_b) as u128;
        let values: Model = values.into_iter().map(u128::from).collect();

        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        let expected: Vec<u128> = values.range(range.clone()).copied().collect();
        let actual: Vec<u128> = bitmap.range(range.clone()).collect();

        assert_eq!(expected, actual);
    }

    #[test]
    fn proptest_range_u128(
        values in btree_set(value(), ..=1000),
        range_a in value(),
        range_b in value(),
    ){
        let range = range_a.min(range_b)..=range_a.max(range_b);

        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        let expected: Vec<u128> = values.range(range.clone()).copied().collect();
        let actual: Vec<u128> = bitmap.range(range.clone()).collect();
        prop_assert_eq!(&expected, &actual);
        let actual_rev: Vec<u128> = bitmap.range(range.clone()).rev().collect();
        prop_assert!(expected.iter().rev().eq(actual_rev.iter()));
        prop_assert_eq!(bitmap.range(range.clone()).len(), expected.len());
        let actual_into: Vec<u128> = bitmap.into_range(range).collect();
        prop_assert_eq!(expected, actual_into);
    }
}
