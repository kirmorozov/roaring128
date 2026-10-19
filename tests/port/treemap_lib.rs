//! Port of `tests/treemap_lib.rs`.
//!
//! `to_bitmap` / `to_array` are identical to the ones of `tests/lib.rs` (see `bitmap_lib`).
//! The `optimize_*` tests built treemaps with empty partitions through `from_bitmaps`; the
//! 128-bit bitmap can never hold an empty container, which is checked instead.

use roaring128::RoaringBitmap128;

use crate::common::*;

fn smoke_at(top: u128) {
    let mut bitmap = RoaringBitmap128::new();
    assert_eq!(bitmap.len(), 0);
    assert!(bitmap.is_empty());
    bitmap.remove(0);
    assert_eq!(bitmap.len(), 0);
    assert!(bitmap.is_empty());
    bitmap.insert(1);
    assert!(bitmap.contains(1));
    assert_eq!(bitmap.len(), 1);
    assert!(!bitmap.is_empty());
    bitmap.insert(top - 2);
    assert!(bitmap.contains(top - 2));
    assert_eq!(bitmap.len(), 2);
    bitmap.insert(top);
    assert!(bitmap.contains(top));
    assert_eq!(bitmap.len(), 3);
    bitmap.insert(2);
    assert!(bitmap.contains(2));
    assert_eq!(bitmap.len(), 4);
    bitmap.remove(2);
    assert!(!bitmap.contains(2));
    assert_eq!(bitmap.len(), 3);
    assert!(!bitmap.contains(0));
    assert!(bitmap.contains(1));
    assert!(!bitmap.contains(100));
    assert!(bitmap.contains(top - 2));
    assert!(!bitmap.contains(top - 1));
    assert!(bitmap.contains(top));
}

#[test]
fn smoke() {
    smoke_at(u64::MAX as u128);
}

#[test]
fn smoke_u128_max() {
    smoke_at(u128::MAX);
}

/// The original with `SIGMA = u32::MAX` (spans 2^33 values, 131072 full containers).
fn insert_range_at(off: u128) {
    let ranges = off..off + 0x1000;
    const SIGMA: u128 = u32::MAX as u128;

    let mut bitmap = RoaringBitmap128::new();
    assert_eq!(bitmap.insert_range(ranges), 0x1000);
    assert_eq!(bitmap.len(), 0x1000);
    assert_eq!(bitmap.max(), Some(off + 0xFFF));

    assert_eq!(bitmap.insert_range(off + SIGMA - 1..off + SIGMA + 1), 2);
    assert!(bitmap.contains(off + 2));
    assert!(bitmap.contains(off + 0xFFF));
    assert!(!bitmap.contains(off + 0x1000));
    assert_valid(&bitmap);

    bitmap.clear();
    assert_eq!(bitmap.insert_range(off + 2 * SIGMA..=off + 4 * SIGMA), 2 * SIGMA as u64 + 1);

    assert_eq!(bitmap.min(), Some(off + 2 * SIGMA));
    assert_eq!(bitmap.max(), Some(off + 4 * SIGMA));
    assert_eq!(bitmap.len(), 2 * SIGMA as u64 + 1);

    assert!(bitmap.contains(off + 3 * SIGMA));
    assert!(!bitmap.contains(off + 4 * SIGMA + 1));
    assert!(!bitmap.contains(off + 2 * SIGMA - 1));
    assert_valid(&bitmap);
}

wide_offset_variants!(insert_range_at);

#[test]
fn insert_range_ending_at_u128_max() {
    const SIGMA: u128 = u32::MAX as u128;
    let mut bitmap = RoaringBitmap128::new();
    assert_eq!(bitmap.insert_range(u128::MAX - 2 * SIGMA..), 2 * SIGMA as u64 + 1);
    assert_eq!(bitmap.min(), Some(u128::MAX - 2 * SIGMA));
    assert_eq!(bitmap.max(), Some(u128::MAX));
    assert_eq!(bitmap.insert_range(u128::MAX - 2 * SIGMA..=u128::MAX), 0);
    assert_eq!(bitmap.insert_range(u128::MAX..), 0);
    assert_eq!(bitmap.len(), 2 * SIGMA as u64 + 1);
    assert_eq!(bitmap.remove_range(u128::MAX - SIGMA..), SIGMA as u64 + 1);
    assert_eq!(bitmap.max(), Some(u128::MAX - SIGMA - 1));
    assert_valid(&bitmap);
}

fn remove_range(off: u128) {
    let ranges = [0u64, 1, 63, 64, 65, 100, 4096 - 1, 4096, 4096 + 1, 65536 - 1];
    let full = shifted(off, 0..=65536);
    for (i, &a) in ranges.iter().enumerate() {
        for &b in &ranges[i..] {
            let mut bitmap = full.clone();
            assert_eq!(bitmap.remove_range(off + a as u128..off + b as u128), b - a);
            assert_eq!(bitmap, shifted(off, (0..a).chain(b..=65536)));
        }
    }
}

offset_variants!(remove_range);

#[test]
fn test_max() {
    let mut bitmap = RoaringBitmap128::new();
    assert_eq!(bitmap.max(), None);
    bitmap.insert(0);
    assert_eq!(bitmap.max(), Some(0));
    bitmap.insert(1);
    assert_eq!(bitmap.max(), Some(1));
    bitmap.insert(u64::MAX as u128);
    assert_eq!(bitmap.max(), Some(u64::MAX as u128));
    bitmap.insert(1 << 100);
    assert_eq!(bitmap.max(), Some(1 << 100));
    bitmap.insert(u128::MAX);
    assert_eq!(bitmap.max(), Some(u128::MAX));
    bitmap.remove(u128::MAX);
    assert_eq!(bitmap.max(), Some(1 << 100));
}

#[test]
fn test_min() {
    let mut bitmap = RoaringBitmap128::new();
    assert_eq!(bitmap.min(), None);
    bitmap.insert(u128::MAX);
    assert_eq!(bitmap.min(), Some(u128::MAX));
    bitmap.insert(u64::MAX as u128);
    assert_eq!(bitmap.min(), Some(u64::MAX as u128));
    bitmap.insert(1);
    assert_eq!(bitmap.min(), Some(1));
    bitmap.insert(0);
    assert_eq!(bitmap.min(), Some(0));
    bitmap.remove(0);
    assert_eq!(bitmap.min(), Some(1));
}

/// Replaces `optimize_prunes_empty_bitmaps_and_returns_true`: removing every value of a
/// container drops the container immediately, there is nothing left for `optimize` to prune.
fn optimize_after_emptying_containers(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert(off + 7);
    bitmap.insert(off + (1 << 32) + 1337);
    bitmap.insert(off + (2 << 32) + 9);
    assert_eq!(bitmap.statistics().n_containers, 3);

    bitmap.remove(off + 7);
    bitmap.remove(off + (2 << 32) + 9);
    assert_valid(&bitmap);
    let stats = bitmap.statistics();
    assert_eq!(stats.n_containers, 1);
    assert_eq!(stats.cardinality, 1);

    assert!(!bitmap.optimize());
    assert_eq!(bitmap.iter().collect::<Vec<_>>(), vec![off + (1 << 32) + 1337]);
}

fn optimize_works_for_single_shard(off: u128) {
    // Mirrors `optimize_run` from `tests/lib.rs`
    let values = (1000..10000).map(|v| off + v);
    let expected = values.clone().collect::<Vec<_>>();

    let mut bitmap = RoaringBitmap128::from_iter(values);

    assert!(bitmap.optimize());
    assert_eq!(bitmap.iter().collect::<Vec<_>>(), expected);
    assert!(!bitmap.optimize());
    assert_valid(&bitmap);
}

fn optimize_returns_false_when_nothing_changes(off: u128) {
    let mut bitmap = shifted(off, [1, 5, 9]);

    assert!(!bitmap.optimize());
    assert_eq!(bitmap.iter().collect::<Vec<_>>(), shift_vec(off, [1, 5, 9]));
}

wide_offset_variants!(optimize_after_emptying_containers);
offset_variants!(optimize_works_for_single_shard, optimize_returns_false_when_nothing_changes);
