//! Port of `tests/iter_next_range.rs`.

use roaring128::RoaringBitmap128;

use crate::common::*;

/// `o + a..=o + b`
fn r(o: u128, a: u128, b: u128) -> Option<std::ops::RangeInclusive<u128>> {
    Some(o + a..=o + b)
}

fn next_range_basic(o: u128) {
    let bm = shifted(o, [1, 2, 4, 5]);
    let mut iter = bm.iter();

    // First consecutive range: 1..=2
    assert_eq!(iter.next_range(), r(o, 1, 2));

    // Iterator should now point at 4
    assert_eq!(iter.next(), Some(o + 4));

    // Second consecutive range: 5..=5 (single element)
    assert_eq!(iter.next_range(), r(o, 5, 5));

    // Iterator should now be exhausted
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_range(), None);
}

fn next_range_back_basic(o: u128) {
    let bm = shifted(o, [1, 2, 4, 5]);
    let mut iter = bm.iter();

    assert_eq!(iter.next_range_back(), r(o, 4, 5));
    assert_eq!(iter.next_back(), Some(o + 2));
    assert_eq!(iter.next_range_back(), r(o, 1, 1));
    assert_eq!(iter.next_back(), None);
    assert_eq!(iter.next_range_back(), None);
}

fn next_range_single_elements(o: u128) {
    let bm = shifted(o, [1, 3, 5, 7]);
    let mut iter = bm.iter();

    assert_eq!(iter.next_range(), r(o, 1, 1));
    assert_eq!(iter.next(), Some(o + 3));

    assert_eq!(iter.next_range(), r(o, 5, 5));
    assert_eq!(iter.next(), Some(o + 7));

    assert_eq!(iter.next_range(), None);
    assert_eq!(iter.next(), None);
}

fn next_range_long_consecutive(o: u128) {
    let bm = shifted(o, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    let mut iter = bm.iter();

    assert_eq!(iter.next_range(), r(o, 1, 10));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_range(), None);
}

fn next_range_partial_consumption(o: u128) {
    let bm = shifted(o, [1, 2, 3, 4, 5, 10, 11, 12]);
    let mut iter = bm.iter();

    assert_eq!(iter.next(), Some(o + 1));
    assert_eq!(iter.next(), Some(o + 2));
    assert_eq!(iter.next_range(), r(o, 3, 5));
    assert_eq!(iter.next(), Some(o + 10));
    assert_eq!(iter.next_range(), r(o, 11, 12));
}

fn next_range_back_partial_consumption(o: u128) {
    let bm = shifted(o, [1, 2, 3, 10, 11, 12]);
    let mut iter = bm.iter();

    assert_eq!(iter.next_back(), Some(o + 12));
    assert_eq!(iter.next_back(), Some(o + 11));
    assert_eq!(iter.next_range_back(), r(o, 10, 10));
    assert_eq!(iter.next_back(), Some(o + 3));
    assert_eq!(iter.next_range_back(), r(o, 1, 2));
}

fn next_range_empty_bitmap(_o: u128) {
    let bm = RoaringBitmap128::new();
    let mut iter = bm.iter();

    assert_eq!(iter.next_range(), None);
    assert_eq!(iter.next_range_back(), None);
}

fn next_range_single_element_bitmap(o: u128) {
    let bm = shifted(o, [42]);
    let mut iter = bm.iter();

    assert_eq!(iter.next_range(), r(o, 42, 42));
    assert_eq!(iter.next(), None);

    let mut iter = bm.iter();
    assert_eq!(iter.next_range_back(), r(o, 42, 42));
    assert_eq!(iter.next_back(), None);
}

fn next_range_mixed_operations(o: u128) {
    let bm = shifted(o, [1, 2, 3, 10, 11, 12, 20]);
    let mut iter = bm.iter();

    assert_eq!(iter.next(), Some(o + 1));
    assert_eq!(iter.next_back(), Some(o + 20));
    assert_eq!(iter.next_range(), r(o, 2, 3));
    assert_eq!(iter.next_range_back(), r(o, 10, 12));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
}

fn next_range_multi_container(o: u128) {
    let bm = shifted(o, [1, 2, 0x1_0000, 0x1_0001, 0x1_0002]);
    let mut iter = bm.iter();

    assert_eq!(iter.next_range(), r(o, 1, 2));
    assert_eq!(iter.next(), Some(o + 0x1_0000));
    assert_eq!(iter.next_range(), r(o, 0x1_0001, 0x1_0002));
    assert_eq!(iter.next(), None);
}

fn next_range_advance_to_integration(o: u128) {
    let bm = shifted(o, [1, 2, 3, 4, 5, 10, 11, 12, 13]);
    let mut iter = bm.iter();

    iter.advance_to(o + 3);
    assert_eq!(iter.next_range(), r(o, 3, 5));
    assert_eq!(iter.next(), Some(o + 10));
    assert_eq!(iter.next_range(), r(o, 11, 13));
}

fn next_range_advance_back_to_integration(o: u128) {
    let bm = shifted(o, [1, 2, 3, 4, 5, 10, 11, 12, 13]);
    let mut iter = bm.iter();

    iter.advance_back_to(o + 12);
    assert_eq!(iter.next_range_back(), r(o, 10, 12));
    assert_eq!(iter.next_back(), Some(o + 5));
    assert_eq!(iter.next_range_back(), r(o, 1, 4));
}

fn into_iter_next_range_basic(o: u128) {
    let bm = shifted(o, [1, 2, 4, 5]);
    let mut iter = bm.into_iter();

    assert_eq!(iter.next_range(), r(o, 1, 2));
    assert_eq!(iter.next(), Some(o + 4));
    assert_eq!(iter.next_range(), r(o, 5, 5));
}

fn into_iter_next_range_back_basic(o: u128) {
    let bm = shifted(o, [1, 2, 4, 5]);
    let mut iter = bm.into_iter();

    assert_eq!(iter.next_range_back(), r(o, 4, 5));
    assert_eq!(iter.next_back(), Some(o + 2));
    assert_eq!(iter.next_range_back(), r(o, 1, 1));
}

fn next_range_exhausted_iterator(o: u128) {
    let bm = shifted(o, [1, 2, 3]);
    let mut iter = bm.iter();

    iter.next();
    iter.next();
    iter.next();

    assert_eq!(iter.next_range(), None);
    assert_eq!(iter.next_range_back(), None);
}

fn next_range_overlapping_calls(o: u128) {
    let bm = shifted(o, [1, 2, 3, 10, 11]);
    let mut iter = bm.iter();

    assert_eq!(iter.next_range(), r(o, 1, 3));
    assert_eq!(iter.next_range(), r(o, 10, 11));
    assert_eq!(iter.next_range(), None);
}

fn next_range_very_sparse(o: u128) {
    let bm = shifted(o, [0, 1000, 2000, 3000]);
    let mut iter = bm.iter();

    assert_eq!(iter.next_range(), r(o, 0, 0));
    assert_eq!(iter.next(), Some(o + 1000));
    assert_eq!(iter.next_range(), r(o, 2000, 2000));
    assert_eq!(iter.next(), Some(o + 3000));
    assert_eq!(iter.next_range(), None);
}

fn next_range_dense_bitmap(o: u128) {
    let mut bm = RoaringBitmap128::new();
    for i in 0..100 {
        bm.insert(o + i);
    }
    for i in 200..300 {
        bm.insert(o + i);
    }
    for i in 500..600 {
        bm.insert(o + i);
    }

    let mut iter = bm.iter();

    assert_eq!(iter.next_range(), r(o, 0, 99));
    assert_eq!(iter.next(), Some(o + 200));
    assert_eq!(iter.next_range(), r(o, 201, 299));
    assert_eq!(iter.next(), Some(o + 500));
    assert_eq!(iter.next_range(), r(o, 501, 599));
    assert_eq!(iter.next(), None);
}

fn next_range_multi_container_range(o: u128) {
    let mut bm = RoaringBitmap128::new();
    bm.insert_range(o..=o + 0x4_0000);
    let mut iter = bm.iter();

    assert_eq!(iter.next(), Some(o));
    assert_eq!(iter.next(), Some(o + 1));
    assert_eq!(iter.next_range(), r(o, 2, 0x4_0000));
    assert_eq!(iter.next_range(), None);
    assert_eq!(iter.next(), None);
}

fn next_range_bitmap_store_forced(o: u128) {
    let mut bm = RoaringBitmap128::new();
    for i in (0..20000).step_by(4) {
        bm.insert(o + i);
        bm.insert(o + i + 1);
    }
    bm.remove_run_compression();

    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), r(o, 0, 1));
    assert_eq!(iter.next(), Some(o + 4));
    assert_eq!(iter.next_range(), r(o, 5, 5));
}

fn next_range_back_bitmap_store_forced(o: u128) {
    let mut bm = RoaringBitmap128::new();
    for i in (0..20000).step_by(4) {
        bm.insert(o + i);
        bm.insert(o + i + 1);
    }
    bm.remove_run_compression();

    let mut iter = bm.iter();
    assert_eq!(iter.next_range_back(), r(o, 19996, 19997));
}

fn next_range_bitmap_store_dense_with_gaps(o: u128) {
    let mut bm = RoaringBitmap128::new();
    for i in 0..10000 {
        if i % 3 != 0 {
            bm.insert(o + i);
        }
    }
    bm.remove_run_compression();

    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), r(o, 1, 2));
    assert_eq!(iter.next(), Some(o + 4));
    assert_eq!(iter.next_range(), r(o, 5, 5));
}

fn next_range_bitmap_store_partial_consumption(o: u128) {
    let mut bm = RoaringBitmap128::new();
    for i in (1000..8000).step_by(3) {
        bm.insert(o + i);
        bm.insert(o + i + 1);
    }
    bm.remove_run_compression();

    let mut iter = bm.iter();
    assert_eq!(iter.next(), Some(o + 1000));
    assert_eq!(iter.next(), Some(o + 1001));
    assert_eq!(iter.next_range(), r(o, 1003, 1004));
}

fn next_range_bitmap_store_mixed_operations(o: u128) {
    let mut bm = RoaringBitmap128::new();
    for i in (0..10000).step_by(3) {
        bm.insert(o + i);
        bm.insert(o + i + 1);
    }
    bm.remove_run_compression();

    let last_element = bm.iter().next_back().unwrap();
    let mut iter = bm.iter();

    assert_eq!(iter.next(), Some(o));
    assert_eq!(iter.next_back(), Some(last_element));
    assert_eq!(iter.next_range(), r(o, 1, 1));
    assert_eq!(iter.next(), Some(o + 3));
    assert_eq!(iter.next_range(), r(o, 4, 4));
}

fn next_range_bitmap_store_single_elements(o: u128) {
    let mut bm = RoaringBitmap128::new();
    for i in (0..20000).step_by(5) {
        bm.insert(o + i);
    }
    bm.remove_run_compression();

    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), r(o, 0, 0));
    assert_eq!(iter.next(), Some(o + 5));
    assert_eq!(iter.next_range(), r(o, 10, 10));
    assert_eq!(iter.next(), Some(o + 15));
    assert_eq!(iter.next_range(), r(o, 20, 20));
}

fn next_range_bitmap_store_alternating_pattern(o: u128) {
    let mut bm = RoaringBitmap128::new();
    for i in (0..10000).step_by(2) {
        bm.insert(o + i);
    }
    bm.remove_run_compression();

    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), r(o, 0, 0));
    assert_eq!(iter.next(), Some(o + 2));
    assert_eq!(iter.next_range(), r(o, 4, 4));
    assert_eq!(iter.next(), Some(o + 6));
    assert_eq!(iter.next_range(), r(o, 8, 8));
}

fn next_range_bitmap_store_with_small_clusters(o: u128) {
    let mut bm = RoaringBitmap128::new();
    for base in (0..15000).step_by(8) {
        bm.insert(o + base);
        bm.insert(o + base + 1);
        bm.insert(o + base + 2);
    }
    bm.remove_run_compression();

    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), r(o, 0, 2));
    assert_eq!(iter.next(), Some(o + 8));
    assert_eq!(iter.next_range(), r(o, 9, 10));
    assert_eq!(iter.next(), Some(o + 16));
    assert_eq!(iter.next_range(), r(o, 17, 18));
}

fn range_partial_consume(o: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(o..=o + 0x3FFF);
    let mut iter = bitmap.iter();
    iter.next();
    assert_eq!(iter.next_range_back(), r(o, 1, 0x3FFF));
}

fn range_with_initial_next(o: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(o + 69311..=o + 180090);
    let mut iter = bitmap.iter();
    assert_eq!(iter.next(), Some(o + 69311));
    assert_eq!(iter.next_range_back(), r(o, 69312, 180090));
}

fn range_with_gap(o: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(o + 0x2_0000..=o + 0x2_FFFF);
    bitmap.remove(o + 0x2_1000);
    bitmap.remove_run_compression();
    let mut iter = bitmap.iter();
    assert_eq!(iter.next_range(), r(o, 0x2_0000, 0x2_0FFF));
    assert_eq!(iter.next(), Some(o + 0x2_1001));
}

fn range_back_after_next(o: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(o..=o + 0x3_FFFF);
    bitmap.remove(o + 0x0_3000);
    let mut iter = bitmap.iter();
    assert_eq!(iter.next(), Some(o));
    assert_eq!(iter.next_range_back(), r(o, 0x0_3001, 0x3_FFFF));
}

/// A range spanning several containers built from different representations (array, bitmap,
/// run) is returned as one range.
fn next_range_across_mixed_containers(o: u128) {
    let mut bm = RoaringBitmap128::new();
    let key = o & !0xFFFF;
    // array part ending at the end of a key, bitmap part in the next key (ending with the odd
    // value 0x1_FFFF), run in the third key
    bm.extend(key + 0xFF00..=key + 0xFFFF);
    bm.extend(key + 0x1_0000..key + 0x1_0000 + 5000);
    bm.extend((key + 0x1_0000 + 5001..key + 0x2_0000).step_by(2));
    bm.insert_range(key + 0x2_0000..key + 0x2_0100);
    assert_valid(&bm);
    let mut iter = bm.iter();
    iter.advance_to(key + 0x1_FFFE);
    assert_eq!(iter.next_range(), Some(key + 0x1_FFFF..=key + 0x2_00FF));
    assert_eq!(iter.next_range(), None);
    let mut iter = bm.iter();
    assert_eq!(iter.next_range_back(), Some(key + 0x1_FFFF..=key + 0x2_00FF));
    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), Some(key + 0xFF00..=key + 0x1_0000 + 4999));
    assert_eq!(iter.next_range(), Some(key + 0x1_0000 + 5001..=key + 0x1_0000 + 5001));
    let mut iter = bm.iter();
    iter.advance_back_to(key + 0x1_0000 + 4000);
    assert_eq!(iter.next_range_back(), Some(key + 0xFF00..=key + 0x1_0000 + 4000));
    assert_eq!(iter.next_range_back(), None);
}

offset_variants!(
    next_range_basic,
    next_range_back_basic,
    next_range_single_elements,
    next_range_long_consecutive,
    next_range_partial_consumption,
    next_range_back_partial_consumption,
    next_range_empty_bitmap,
    next_range_single_element_bitmap,
    next_range_mixed_operations,
    next_range_multi_container,
    next_range_advance_to_integration,
    next_range_advance_back_to_integration,
    into_iter_next_range_basic,
    into_iter_next_range_back_basic,
    next_range_exhausted_iterator,
    next_range_overlapping_calls,
    next_range_very_sparse,
    next_range_dense_bitmap,
    next_range_multi_container_range,
    next_range_bitmap_store_forced,
    next_range_back_bitmap_store_forced,
    next_range_bitmap_store_dense_with_gaps,
    next_range_bitmap_store_partial_consumption,
    next_range_bitmap_store_mixed_operations,
    next_range_bitmap_store_single_elements,
    next_range_bitmap_store_alternating_pattern,
    next_range_bitmap_store_with_small_clusters,
    range_partial_consume,
    range_with_initial_next,
    range_with_gap,
    range_back_after_next,
    next_range_across_mixed_containers
);

#[test]
fn next_range_u32_max_boundary() {
    let m = u32::MAX as u128;
    let bm = RoaringBitmap128::from([m - 2, m - 1, m]);
    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), Some((m - 2)..=m));
    assert_eq!(iter.next(), None);
}

#[test]
fn next_range_u32_max_boundary_continues_u128() {
    // u32::MAX is not special for 128-bit keys: the range continues into the next key.
    let m = u32::MAX as u128;
    let bm = RoaringBitmap128::from([m - 2, m - 1, m, m + 1]);
    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), Some((m - 2)..=m + 1));
    assert_eq!(iter.next(), None);
}

#[test]
fn next_range_u128_max_boundary() {
    let m = u128::MAX;
    let bm = RoaringBitmap128::from([m - 2, m - 1, m]);
    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), Some((m - 2)..=m));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_range(), None);
    let mut iter = bm.iter();
    assert_eq!(iter.next_range_back(), Some((m - 2)..=m));
    assert_eq!(iter.next_range_back(), None);
}

#[test]
fn next_range_full_tail_of_the_space_u128() {
    let mut bm = RoaringBitmap128::new();
    bm.insert_range(u128::MAX - 0x2_FFFF..);
    bm.insert(u128::MAX - 0x3_0001);
    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), Some(u128::MAX - 0x3_0001..=u128::MAX - 0x3_0001));
    assert_eq!(iter.next_range(), Some(u128::MAX - 0x2_FFFF..=u128::MAX));
    assert_eq!(iter.next_range(), None);
    let mut iter = bm.clone().into_iter();
    assert_eq!(iter.next_back(), Some(u128::MAX));
    assert_eq!(iter.next_range_back(), Some(u128::MAX - 0x2_FFFF..=u128::MAX - 1));
    assert_eq!(iter.next_range_back(), Some(u128::MAX - 0x3_0001..=u128::MAX - 0x3_0001));
    assert_eq!(iter.next_range_back(), None);
}

#[test]
fn next_range_u64_boundary_u128() {
    // A run crossing 2^64 (key 2^48) is one range.
    let mut bm = RoaringBitmap128::new();
    bm.insert_range((1u128 << 64) - 3..(1u128 << 64) + 3);
    let mut iter = bm.iter();
    assert_eq!(iter.next_range(), Some((1u128 << 64) - 3..=(1u128 << 64) + 2));
    assert_eq!(iter.next_range(), None);
}
