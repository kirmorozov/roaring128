//! Port of `tests/iter.rs`.

use proptest::arbitrary::any;
use proptest::collection::btree_set;
use proptest::prelude::*;
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
    let original = shifted(off, 0..100_000);
    let clone = RoaringBitmap128::from_iter(&original);
    let clone2 = RoaringBitmap128::from_iter(original.clone());

    assert_eq!(clone, original);
    assert_eq!(clone2, original);
}

fn arrays(off: u128) {
    let original = shifted(off, (0..2000).chain(1_000_000..1_002_000).chain(2_000_000..2_001_000));
    let clone = RoaringBitmap128::from_iter(&original);
    let clone2 = RoaringBitmap128::from_iter(original.clone());

    assert_eq!(clone, original);
    assert_eq!(clone2, original);
}

fn bitmaps(off: u128) {
    let original =
        shifted(off, (0..100_000).chain(1_000_000..1_012_000).chain(2_000_000..2_010_000));
    let clone = RoaringBitmap128::from_iter(&original);
    let clone2 = RoaringBitmap128::from_iter(original.clone());

    assert_eq!(clone, original);
    assert_eq!(clone2, original);
}

fn rev_array(off: u128) {
    let values = (0..100).map(|v| off + v);
    let bitmap = values.clone().collect::<RoaringBitmap128>();

    assert!(values.rev().eq(bitmap.iter().rev()));
}

fn rev_bitmap(off: u128) {
    let values = (0..=100_000).map(|v| off + v);
    let bitmap = values.clone().collect::<RoaringBitmap128>();

    assert!(values.rev().eq(bitmap.iter().rev()));
}

fn interleaved_array(off: u128) {
    let values = (0..100).map(|v| off + v);
    let bitmap = values.clone().collect::<RoaringBitmap128>();

    assert!(outside_in(values).eq(outside_in(bitmap)));
}

fn interleaved_bitmap(off: u128) {
    let values = (0..=4097).map(|v| off + v);
    let bitmap = values.clone().collect::<RoaringBitmap128>();

    assert!(outside_in(values).eq(outside_in(bitmap)));
}

fn run_nth_max(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 0x1_0000);
    let mut iter = bitmap.iter();
    assert_eq!(iter.nth(0x0_FFFF), Some(off + 0x0_FFFF));
    assert_eq!(iter.len(), 0);
    #[allow(clippy::iter_nth_zero)]
    {
        assert_eq!(iter.nth(0), None);
    }
    assert_eq!(iter.next(), None);
}

fn run_nth_back_max(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 0x1_0000);
    let mut iter = bitmap.iter();
    assert_eq!(iter.nth_back(0x0_FFFF), Some(off));
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.nth_back(0), None);
    assert_eq!(iter.next_back(), None);
}

offset_variants!(
    range,
    array,
    bitmap,
    arrays,
    bitmaps,
    rev_array,
    rev_bitmap,
    interleaved_array,
    interleaved_bitmap,
    run_nth_max,
    run_nth_back_max
);

#[test]
fn huge_nth() {
    let bitmap = RoaringBitmap128::new();
    let mut iter = bitmap.iter();
    assert_eq!(None, iter.nth(usize::MAX));
}

#[test]
fn huge_nth_non_empty_u128() {
    let mut bitmap = RoaringBitmap128::from([0, 1 << 64, u128::MAX]);
    bitmap.insert_range(u128::MAX - 100_000..);
    let mut iter = bitmap.iter();
    assert_eq!(iter.nth(usize::MAX), None);
    assert_eq!(iter.next(), None);
    let mut iter = bitmap.iter();
    assert_eq!(iter.nth_back(usize::MAX), None);
    assert_eq!(iter.next_back(), None);
    let mut iter = bitmap.clone().into_iter();
    assert_eq!(iter.nth(usize::MAX), None);
    assert_eq!(iter.len(), 0);
}

#[test]
fn from_iter() {
    // This test verifies that the public API allows conversion from iterators
    // with u128 as well as &u128 elements.
    let vals = vec![1, 5, 1_000_000_000_000_000, u128::MAX];
    let a = RoaringBitmap128::from_iter(vals.iter());
    let b = RoaringBitmap128::from_iter(vals);
    assert_eq!(a, b);
}

// Sanity check that outside_in does what we expect
#[test]
fn outside_in_iterator() {
    let values = 0..10;
    assert!(outside_in(values).eq(vec![0, 9, 1, 8, 2, 7, 3, 6, 4, 5]));
}

#[test]
fn run_nth_whole_space_end_u128() {
    // nth across several full containers ending at u128::MAX
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(u128::MAX - 0x3_FFFF..=u128::MAX);
    let mut iter = bitmap.iter();
    assert_eq!(iter.nth(0x3_FFFF), Some(u128::MAX));
    assert_eq!(iter.next(), None);
    let mut iter = bitmap.iter();
    assert_eq!(iter.nth(0x1_0000), Some(u128::MAX - 0x2_FFFF));
    assert_eq!(iter.nth_back(0x1_0000), Some(u128::MAX - 0x1_0000));
    assert_eq!(iter.len(), 0x1_FFFE);
}

#[test]
fn count_and_fold_on_runs_u128() {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range((1 << 64) - 100_000..(1 << 64) + 100_000);
    assert_eq!(bitmap.iter().count(), 200_000);
    assert_eq!(bitmap.clone().into_iter().count(), 200_000);
    let sum = bitmap.iter().fold(0u128, |acc, v| acc + (v - ((1 << 64) - 100_000)));
    assert_eq!(sum, (0..200_000u128).sum::<u128>());
    let rsum = bitmap.iter().rfold(0u128, |acc, v| acc + (v - ((1 << 64) - 100_000)));
    assert_eq!(rsum, sum);
    assert_eq!(bitmap.iter().last(), Some((1 << 64) + 99_999));
    assert_eq!(Iterator::max(bitmap.iter()), Some((1 << 64) + 99_999));
    assert_eq!(Iterator::min(bitmap.iter()), Some((1 << 64) - 100_000));
}

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn iter(values in btree_set(any::<u32>(), ..=10_000)) {
        let values: Vec<u128> = values.into_iter().map(u128::from).collect();
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        // Iterator::eq != PartialEq::eq - cannot use assert_eq macro
        assert!(values.into_iter().eq(bitmap));
    }

    #[test]
    fn iter_u128(values in btree_set(any::<u128>(), ..=10_000)) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        assert!(values.into_iter().eq(bitmap));
    }

    #[test]
    fn iter_anchored_u128(values in btree_set(value(), ..=10_000)) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        assert!(values.into_iter().eq(bitmap));
    }

    #[test]
    fn fold(values in btree_set(any::<u32>(), ..=10_000)) {
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
    fn fold_u128(values in btree_set(value(), ..=10_000)) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        let mut val_iter = values.iter().copied();
        #[allow(clippy::unnecessary_fold)]
        let r = bitmap.iter().fold(true, |b, i| b && i == val_iter.next().unwrap());
        assert!(r);
        let mut val_iter = values.iter().copied().rev();
        let r = bitmap.into_iter().rfold(true, |b, i| b && i == val_iter.next().unwrap());
        assert!(r);
    }

    #[test]
    fn nth(values in btree_set(any::<u32>(), ..=10_000), nth in 0..10_005usize) {
        let values: Vec<u128> = values.into_iter().map(u128::from).collect();
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        let mut orig_iter = bitmap.iter().fuse();
        let mut iter = bitmap.iter();

        for _ in 0..nth {
            if orig_iter.next().is_none() {
                break;
            }
        }
        let expected = orig_iter.next();
        assert_eq!(expected, iter.nth(nth));
        let expected_next = orig_iter.next();
        assert_eq!(expected_next, iter.next());

        let mut val_iter = values.into_iter();
        assert_eq!(expected, val_iter.nth(nth));
        assert_eq!(expected_next, val_iter.next());
    }

    #[test]
    fn nth_u128(values in btree_set(value(), ..=10_000), nth in 0..10_005usize) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        let sorted: Vec<u128> = values.iter().copied().collect();
        let mut iter = bitmap.iter();
        assert_eq!(iter.nth(nth), sorted.get(nth).copied());
        assert_eq!(iter.next(), sorted.get(nth + 1).copied());
        let mut iter = bitmap.iter();
        let back = sorted.len().checked_sub(nth + 1).map(|i| sorted[i]);
        assert_eq!(iter.nth_back(nth), back);
        let mut into = bitmap.into_iter();
        assert_eq!(into.nth(nth), sorted.get(nth).copied());
    }

    #[test]
    fn count(values in btree_set(any::<u32>(), ..=10_000), skip in 0..10_005usize) {
        let values: Vec<u128> = values.into_iter().map(u128::from).collect();
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        let mut iter = bitmap.iter();

        if let Some(n) = skip.checked_sub(1) {
            iter.nth(n);
        }
        let expected_count = values.len().saturating_sub(skip);
        let size_hint = iter.size_hint();
        assert_eq!(expected_count, size_hint.0);
        assert_eq!(Some(expected_count), size_hint.1);
        assert_eq!(expected_count, iter.count());
    }

    #[test]
    fn count_u128(values in btree_set(value(), ..=10_000), skip in 0..10_005usize) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();
        let mut iter = bitmap.iter();
        if let Some(n) = skip.checked_sub(1) {
            iter.nth_back(n);
        }
        let expected_count = values.len().saturating_sub(skip);
        assert_eq!(iter.size_hint(), (expected_count, Some(expected_count)));
        assert_eq!(expected_count, iter.count());
    }

    #[test]
    fn rev_iter(values in btree_set(any::<u32>(), ..=10_000)) {
        let values: Vec<u128> = values.into_iter().map(u128::from).collect();
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();

        assert!(values.into_iter().rev().eq(bitmap.iter().rev()));
    }

    #[test]
    fn rev_iter_u128(values in btree_set(value(), ..=10_000)) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();

        assert!(values.iter().copied().rev().eq(bitmap.iter().rev()));
        assert!(values.into_iter().rev().eq(bitmap.into_iter().rev()));
    }
}

proptest! {
    #![proptest_config(config(16))]

    #[test]
    fn interleaved_iter(values in btree_set(any::<u32>(), 50_000..=100_000)) {
        let values: Vec<u128> = values.into_iter().map(u128::from).collect();
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();

        assert!(outside_in(values).eq(outside_in(bitmap)));
    }

    #[test]
    fn interleaved_iter_u128(values in btree_set(value(), 50_000..=100_000)) {
        let bitmap = RoaringBitmap128::from_sorted_iter(values.iter().cloned()).unwrap();

        assert!(outside_in(values.iter().copied()).eq(outside_in(&bitmap)));
        assert!(outside_in(values).eq(outside_in(bitmap)));
    }
}
