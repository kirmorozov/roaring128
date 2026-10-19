//! Port of the unit tests of `src/bitmap/inherent.rs`.
//!
//! Assertions on the private `containers` vector use `statistics().n_containers` and the
//! iteration order instead.

use proptest::collection::vec;
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

fn insert_range_body(off: u128, lo: u32, hi: u32, checks: &[u32]) {
    let r = off + lo as u128..off + hi as u128;
    let mut b = RoaringBitmap128::new();
    let inserted = b.insert_range(r.clone());
    if r.end > r.start {
        assert_eq!(inserted, (r.end - r.start) as u64);
    } else {
        assert_eq!(inserted, 0);
    }
    assert_valid(&b);

    // Assert all values in the range are present
    for i in r.clone() {
        assert!(b.contains(i), "does not contain {i}");
    }

    // Run the check values looking for any false positives
    for &i in checks {
        let i = off + i as u128;
        let bitmap_has = b.contains(i);
        let range_has = r.contains(&i);
        assert_eq!(
            bitmap_has, range_has,
            "value {i} in bitmap={bitmap_has} and range={range_has}"
        );
    }
}

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn insert_range(
        lo in 0u32..=65535, hi in 65536u32..=131071,
        checks in vec(0u32..=262143, 1000)
    ){
        insert_range_body(0, lo, hi, &checks);
    }

    #[test]
    fn insert_range_u128(
        off in prop::sample::select(OFFSETS.iter().map(|o| o.1).collect::<Vec<_>>()),
        lo in 0u32..=65535, hi in 65536u32..=131071,
        checks in vec(0u32..=262143, 1000)
    ){
        insert_range_body(off, lo, hi, &checks);
    }
}

fn test_insert_remove_range_same_container(off: u128) {
    let mut b = RoaringBitmap128::new();
    let inserted = b.insert_range(off + 1..off + 5);
    assert_eq!(inserted, 4);

    for i in 1..5 {
        assert!(b.contains(off + i));
    }

    let removed = b.remove_range(off + 2..off + 10);
    assert_eq!(removed, 3);
    assert!(b.contains(off + 1));
    for i in 2..5 {
        assert!(!b.contains(off + i));
    }
    assert_valid(&b);
}

fn test_insert_remove_range_pre_populated(off: u128) {
    let mut b = RoaringBitmap128::new();
    let inserted = b.insert_range(off + 1..off + 20_000);
    assert_eq!(inserted, 19_999);

    let removed = b.remove_range(off + 10_000..off + 21_000);
    assert_eq!(removed, 10_000);

    let inserted = b.insert_range(off + 1..off + 20_000);
    assert_eq!(inserted, 10_000);
    assert_valid(&b);
}

/// `off` must be key aligned for the container counts to hold.
fn test_insert_remove_across_container(off: u128) {
    let off = off & !0xFFFF;
    let m = u16::MAX as u128;
    let mut b = RoaringBitmap128::new();
    let inserted = b.insert_range(off + m..=off + m + 1);
    assert_eq!(inserted, 2);

    assert_eq!(b.statistics().n_containers, 2);

    let removed = b.remove_range(off + m + 1..=off + m + 1);
    assert_eq!(removed, 1);

    assert_eq!(b.statistics().n_containers, 1);
    assert_valid(&b);
}

fn test_insert_remove_single_element(off: u128) {
    let off = off & !0xFFFF;
    let m = u16::MAX as u128;
    let mut b = RoaringBitmap128::new();
    let inserted = b.insert_range(off + m + 1..=off + m + 1);
    assert_eq!(inserted, 1);

    let stats = b.statistics();
    assert_eq!(stats.cardinality, 1);
    assert_eq!(stats.n_containers, 1);

    let removed = b.remove_range(off + m + 1..=off + m + 1);
    assert_eq!(removed, 1);

    assert_eq!(b.statistics().n_containers, 0);
    assert!(b.is_empty());
    assert_valid(&b);
}

fn test_insert_remove_range_multi_container(off: u128) {
    let off = off & !0xFFFF;
    let k = 1u128 << 16;
    let mut bitmap = RoaringBitmap128::new();
    assert_eq!(bitmap.insert_range(off..off + k + 1), (1_u64 << 16) + 1);
    assert_eq!(bitmap.statistics().n_containers, 2);
    // keys 0 and 1 (relative to the offset)
    assert_eq!(bitmap.min(), Some(off));
    assert_eq!(bitmap.max(), Some(off + k));
    assert_eq!(bitmap.insert_range(off..off + k + 1), 0);

    assert!(bitmap.insert(off + k * 4));
    assert_eq!(bitmap.statistics().n_containers, 3);
    assert_eq!(bitmap.max(), Some(off + k * 4));

    assert_eq!(bitmap.remove_range(off + k * 3..=off + k * 4), 1);
    assert_eq!(bitmap.statistics().n_containers, 2);
    assert_valid(&bitmap);
}

fn insert_range_single(off: u128) {
    let off = off & !0xFFFF;
    let k = 1u128 << 16;
    let mut bitmap = RoaringBitmap128::new();
    assert_eq!(bitmap.insert_range(off + k..off + 2 * k), 1_u64 << 16);
    let stats = bitmap.statistics();
    assert_eq!(stats.n_containers, 1);
    // a full container is stored as a single run
    assert_eq!(stats.n_run_containers, 1);
    assert_eq!(bitmap.min(), Some(off + k));
    assert_eq!(bitmap.max(), Some(off + 2 * k - 1));
}

fn remove_smallest_for_vec(off: u128) {
    let mut bitmap = shifted(off, [1, 2, 3, 7, 9, 11]);
    bitmap.remove_smallest(3);
    assert_eq!(bitmap.len(), 3);
    assert_eq!(bitmap, shifted(off, [7, 9, 11]));

    bitmap = shifted(off, [1, 2, 5, 7, 9, 11]);
    bitmap.remove_smallest(3);
    assert_eq!(bitmap.len(), 3);
    assert_eq!(bitmap, shifted(off, [7, 9, 11]));

    bitmap = shifted(off, [1, 3]);
    bitmap.remove_smallest(2);
    assert_eq!(bitmap.len(), 0);

    bitmap = shifted(off, [1, 2, 3, 7, 9, 11]);
    bitmap.remove_smallest(0);
    assert_eq!(bitmap.len(), 6);
    assert_eq!(bitmap, shifted(off, [1, 2, 3, 7, 9, 11]));

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + (1 << 16) + 5);
    bitmap.remove_smallest(65537);
    assert_eq!(bitmap.len(), 4);
    assert_eq!(bitmap, shifted(off, [65537, 65538, 65539, 65540]));
    assert_valid(&bitmap);

    bitmap = shifted(off, [1, 2, 5, 7, 9, 11]);
    bitmap.remove_smallest(7);
    assert_eq!(bitmap, RoaringBitmap128::default());
    assert_valid(&bitmap);
}

fn remove_smallest_for_bit(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 4098);
    bitmap.remove_smallest(4095);
    assert_eq!(bitmap.len(), 3);
    // removed bit to vec
    assert_eq!(bitmap, shifted(off, [4095, 4096, 4097]));
    assert_valid(&bitmap);

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 6000);
    bitmap.remove_smallest(999);
    assert_eq!(bitmap.len(), 5001);

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 8000);
    bitmap.remove_smallest(10);
    assert_eq!(bitmap.len(), 7990);

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 200000);
    bitmap.remove_smallest(2000);
    assert_eq!(bitmap.len(), 198000);
    assert_eq!(bitmap, shifted(off, 2000..200000));
    assert_valid(&bitmap);

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 2);
    bitmap.insert_range(off + 4..off + 7);
    bitmap.insert_range(off + 1000..off + 6000);
    bitmap.remove_smallest(30);
    assert_eq!(bitmap.len(), 4975);
    assert_valid(&bitmap);

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 65535);
    bitmap.remove_smallest(0);
    assert_eq!(bitmap.len(), 65535);
}

fn remove_biggest_for_bit(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 5000);
    bitmap.remove_biggest(1000);
    assert_eq!(bitmap.len(), 4000);

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 6000);
    bitmap.remove_biggest(1000);
    assert_eq!(bitmap.len(), 5000);

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 200000);
    bitmap.remove_biggest(196000);
    assert_eq!(bitmap.len(), 4000);
    assert_valid(&bitmap);

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 200000);
    bitmap.remove_biggest(2000);
    assert_eq!(bitmap.len(), 198000);
    assert_eq!(bitmap, shifted(off, 0..198000));
    assert_valid(&bitmap);

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 65535);
    bitmap.remove_biggest(0);
    assert_eq!(bitmap.len(), 65535);
}

fn remove_biggest_for_vec(off: u128) {
    let mut bitmap = shifted(off, [1, 2, 3, 7, 9, 11]);
    bitmap.remove_biggest(2);
    assert_eq!(bitmap, shifted(off, [1, 2, 3, 7]));

    bitmap = shifted(off, [1, 2, 3, 7, 9, 11]);
    bitmap.remove_biggest(6);
    assert_eq!(bitmap.len(), 0);

    bitmap = shifted(off, [1, 2, 3, 7, 9, 11]);
    bitmap.remove_biggest(0);
    assert_eq!(bitmap.len(), 6);
    assert_eq!(bitmap, shifted(off, [1, 2, 3, 7, 9, 11]));

    bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + (1 << 16) + 5);
    bitmap.remove_biggest(65537);
    assert_eq!(bitmap.len(), 4);
    assert_eq!(bitmap, shifted(off, [0, 1, 2, 3]));
    assert_valid(&bitmap);

    let mut bitmap = shifted(off, [1, 2, 3]);
    bitmap.remove_biggest(4);
    assert_eq!(bitmap, RoaringBitmap128::default());
}

offset_variants!(
    test_insert_remove_range_same_container,
    test_insert_remove_range_pre_populated,
    test_insert_remove_across_container,
    test_insert_remove_single_element,
    test_insert_remove_range_multi_container,
    insert_range_single,
    remove_smallest_for_vec,
    remove_smallest_for_bit,
    remove_biggest_for_bit,
    remove_biggest_for_vec
);

#[test]
fn test_insert_max_u32() {
    let mut b = RoaringBitmap128::new();
    let inserted = b.insert(u32::MAX as u128);
    // We are allowed to add u32::MAX
    assert!(inserted);
}

#[test]
fn test_insert_max_u128() {
    let mut b = RoaringBitmap128::new();
    assert!(b.insert(u128::MAX));
    assert_eq!(b.insert_range(u128::MAX..=u128::MAX), 0);
    assert_eq!(b.insert_range(u128::MAX - 1..=u128::MAX), 1);
    assert_eq!(b.len(), 2);
    assert_valid(&b);
}

#[test]
fn remove_smallest_biggest_huge_n_u128() {
    let mut bitmap: RoaringBitmap128 = [0u128, 1 << 64, u128::MAX].into_iter().collect();
    bitmap.remove_smallest(u64::MAX);
    assert!(bitmap.is_empty());
    assert_valid(&bitmap);
    let mut bitmap: RoaringBitmap128 = [0u128, 1 << 64, u128::MAX].into_iter().collect();
    bitmap.remove_biggest(u64::MAX);
    assert!(bitmap.is_empty());
    let mut empty = RoaringBitmap128::new();
    empty.remove_smallest(1);
    empty.remove_biggest(1);
    assert!(empty.is_empty());
}

#[test]
fn remove_smallest_biggest_across_far_keys_u128() {
    let values = [
        0u128,
        5,
        1 << 64,
        (1 << 64) + 1,
        1 << 112,
        u128::MAX - 1,
        u128::MAX,
    ];
    for n in 0..=values.len() as u64 + 1 {
        let mut a: RoaringBitmap128 = values.into_iter().collect();
        a.remove_smallest(n);
        assert_valid(&a);
        assert!(
            a.iter().eq(values.iter().copied().skip(n as usize)),
            "remove_smallest({n})"
        );
        let mut b: RoaringBitmap128 = values.into_iter().collect();
        b.remove_biggest(n);
        assert_valid(&b);
        let keep = values.len().saturating_sub(n as usize);
        assert!(
            b.iter().eq(values.iter().copied().take(keep)),
            "remove_biggest({n})"
        );
    }
}

#[test]
fn remove_smallest_biggest_whole_containers_u128() {
    // 3 full containers ending at u128::MAX: remove exactly one container's worth
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(u128::MAX - 0x2_FFFF..);
    bitmap.remove_smallest(0x1_0000);
    assert_eq!(bitmap.len(), 0x2_0000);
    assert_eq!(bitmap.min(), Some(u128::MAX - 0x1_FFFF));
    assert_eq!(bitmap.statistics().n_containers, 2);
    bitmap.remove_biggest(0x1_0000);
    assert_eq!(bitmap.len(), 0x1_0000);
    assert_eq!(bitmap.max(), Some(u128::MAX - 0x1_0000));
    assert_eq!(bitmap.statistics().n_containers, 1);
    bitmap.remove_biggest(0xFFFF);
    assert_eq!(bitmap.len(), 1);
    assert_eq!(bitmap.max(), Some(u128::MAX - 0x1_FFFF));
    assert_valid(&bitmap);
}
