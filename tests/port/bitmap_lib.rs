//! Port of `tests/lib.rs`.

use roaring128::RoaringBitmap128;

use crate::common::*;

/// The original `smoke` test with `u32::MAX` replaced by `top`.
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
    assert_valid(&bitmap);
}

#[test]
fn smoke() {
    smoke_at(u32::MAX as u128);
}

#[test]
fn smoke_u64_max() {
    smoke_at(u64::MAX as u128);
}

#[test]
fn smoke_2p64() {
    smoke_at((1 << 64) + 1);
}

#[test]
fn smoke_2p112() {
    smoke_at((1 << 112) + 1);
}

#[test]
fn smoke_u128_max() {
    smoke_at(u128::MAX);
}

fn remove_range(off: u128) {
    let ranges = [
        0u64,
        1,
        63,
        64,
        65,
        100,
        4096 - 1,
        4096,
        4096 + 1,
        65536 - 1,
        65536,
        65536 + 1,
    ];
    let full = shifted(off, 0..=65536);
    for (i, &a) in ranges.iter().enumerate() {
        for &b in &ranges[i..] {
            let mut bitmap = full.clone();
            assert_eq!(bitmap.remove_range(off + a as u128..off + b as u128), b - a);
            assert_valid(&bitmap);
            assert_eq!(bitmap, shifted(off, (0..a).chain(b..=65536)));
        }
    }
}

#[allow(clippy::range_plus_one)] // remove_range needs an exclusive range
fn remove_range_array(off: u128) {
    let mut bitmap = shifted(off, 0..1000);
    for i in (0..1000).map(|i| off + i) {
        assert_eq!(bitmap.remove_range(i..i), 0);
        assert_eq!(bitmap.remove_range(i..i + 1), 1);
    }
    assert!(bitmap.is_empty());

    // insert 0, 2, 4, ..
    // remove [0, 2), [2, 4), ..
    let mut bitmap = shifted(off, (0..1000).map(|x| x * 2));
    for i in 0..1000 {
        assert_eq!(bitmap.remove_range(off + i * 2..off + (i + 1) * 2), 1);
    }

    // remove [0, 2), [2, 4), ..
    let mut bitmap = shifted(off, 0..1000);
    for i in 0..1000 / 2 {
        assert_eq!(bitmap.remove_range(off + i * 2..off + (i + 1) * 2), 2);
    }
    assert_valid(&bitmap);
}

#[allow(clippy::range_plus_one)] // remove_range needs an exclusive range
fn remove_range_bitmap(off: u128) {
    let mut bitmap = shifted(off, 0..4096 + 1000);
    for i in (0..1000).map(|i| off + i) {
        assert_eq!(bitmap.remove_range(i..i), 0);
        assert_eq!(bitmap.remove_range(i..i + 1), 1);
    }
    assert_valid(&bitmap);

    // insert 0, 2, 4, ..
    // remove [0, 2), [2, 4), ..
    let mut bitmap = shifted(off, (0..4096 + 1000).map(|x| x * 2));
    for i in 0..1000 {
        assert_eq!(bitmap.remove_range(off + i * 2..off + (i + 1) * 2), 1);
    }
    assert_valid(&bitmap);

    // remove [0, 2), [2, 4), ..
    let mut bitmap = shifted(off, 0..4096 + 1000);
    for i in 0..1000 / 2 {
        assert_eq!(bitmap.remove_range(off + i * 2..off + (i + 1) * 2), 2);
    }
    assert_valid(&bitmap);

    // remove [1, 3), [3, 5), ..
    let mut bitmap = shifted(off, 0..4096 + 1000);
    for i in 0..1000 / 2 {
        assert_eq!(
            bitmap.remove_range(off + i * 2 + 1..off + (i + 1) * 2 + 1),
            2
        );
    }
    assert_valid(&bitmap);
}

fn to_bitmap(off: u128) {
    let bitmap = shifted(off, 0..5000);
    assert_eq!(bitmap.len(), 5000);
    for i in 1..5000 {
        assert!(bitmap.contains(off + i));
    }
    assert!(!bitmap.contains(off + 5001));
}

fn to_array(off: u128) {
    let mut bitmap = shifted(off, 0..5000);
    for i in 3000..5000 {
        bitmap.remove(off + i);
    }
    assert_valid(&bitmap);
    assert_eq!(bitmap.len(), 3000);
    for i in 0..3000 {
        assert!(bitmap.contains(off + i));
    }
    for i in 3000..5000 {
        assert!(!bitmap.contains(off + i));
    }
}

fn optimize_array(off: u128) {
    let mut bitmap = shifted(off, 0..1000);
    assert!(bitmap.optimize());
    assert_valid(&bitmap);
    let mut bitmap = shifted(off, 0..5000);
    assert!(bitmap.optimize());
    assert_valid(&bitmap);
}

fn optimize_bitmap(off: u128) {
    let mut bitmap = shifted(off, 0..5000);
    assert!(bitmap.optimize());
}

fn remove_run_compression(off: u128) {
    let mut bitmap = shifted(off, 0..5000);
    assert!(bitmap.optimize());
    assert!(bitmap.remove_run_compression());
    assert_valid(&bitmap);
    assert_eq!(bitmap, shifted(off, 0..5000));
}

fn optimize_run(off: u128) {
    let mut bitmap = shifted(off, 0..1000);
    assert!(bitmap.optimize());
    // Calling optimize a second time should return false as no changes will be made
    assert!(!bitmap.optimize());
}

offset_variants!(
    remove_range,
    remove_range_array,
    remove_range_bitmap,
    to_bitmap,
    to_array,
    optimize_array,
    optimize_bitmap,
    remove_run_compression,
    optimize_run
);

// -------------------------------------------------------------------------------------------------
// 128-bit specific
// -------------------------------------------------------------------------------------------------

#[test]
fn insert_and_remove_u128_max() {
    let mut bitmap = RoaringBitmap128::new();
    assert!(bitmap.insert(u128::MAX));
    assert!(!bitmap.insert(u128::MAX));
    assert!(bitmap.contains(u128::MAX));
    assert!(!bitmap.contains(u128::MAX - 1));
    assert_eq!(bitmap.min(), Some(u128::MAX));
    assert_eq!(bitmap.max(), Some(u128::MAX));
    assert_eq!(bitmap.rank(u128::MAX), 1);
    assert_eq!(bitmap.rank(u128::MAX - 1), 0);
    assert_eq!(bitmap.select(0), Some(u128::MAX));
    assert_valid(&bitmap);
    assert!(bitmap.remove(u128::MAX));
    assert!(!bitmap.remove(u128::MAX));
    assert!(bitmap.is_empty());
    assert_valid(&bitmap);
}

#[test]
fn values_differing_only_in_high_bits() {
    // Same low 16 / 64 bits, different keys
    let values: Vec<u128> = (0..128).map(|bit| (1u128 << bit) | 0xABCD).collect();
    let bitmap: RoaringBitmap128 = values.iter().copied().collect();
    let model: Model = values.iter().copied().collect();
    assert_matches(&bitmap, &model);
    for &v in &values {
        assert!(bitmap.contains(v));
        assert!(!bitmap.contains(v ^ 1));
    }
    // 0xABCD | (1 << bit) for bit < 16 shares the key 0 and maybe even the value
    assert_eq!(bitmap.len(), model.len() as u64);
}

#[test]
fn optimize_at_top_of_the_space() {
    let mut bitmap: RoaringBitmap128 = (u128::MAX - 999..=u128::MAX).collect();
    assert!(bitmap.optimize());
    assert!(!bitmap.optimize());
    assert_valid(&bitmap);
    assert_eq!(bitmap.len(), 1000);
    assert_eq!(bitmap.max(), Some(u128::MAX));
    assert!(bitmap.remove_run_compression());
    assert!(!bitmap.remove_run_compression());
    assert_valid(&bitmap);
    assert!(bitmap.iter().eq(u128::MAX - 999..=u128::MAX));
}

#[test]
fn optimize_sparse_uuids_is_noop() {
    let mut bitmap: RoaringBitmap128 = (0..1000u128)
        .map(|i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835))
        .collect();
    let before = bitmap.clone();
    assert!(!bitmap.optimize());
    assert!(!bitmap.remove_run_compression());
    assert_eq!(bitmap, before);
}

#[test]
fn remove_range_unbounded_u128() {
    let mut bitmap: RoaringBitmap128 = [0u128, 1, 1 << 64, 1 << 100, u128::MAX - 1, u128::MAX]
        .into_iter()
        .collect();
    assert_eq!(bitmap.remove_range(u128::MAX..), 1);
    assert_eq!(bitmap.remove_range(..1), 1);
    assert_eq!(bitmap.remove_range((1 << 64) + 1..(1 << 100)), 0);
    assert_eq!(bitmap.remove_range((1 << 64)..=(1 << 100)), 2);
    assert_valid(&bitmap);
    assert!(bitmap.iter().eq([1, u128::MAX - 1]));
    assert_eq!(bitmap.remove_range(..), 2);
    assert!(bitmap.is_empty());
    assert_valid(&bitmap);
    assert_eq!(bitmap.remove_range(..), 0);
}

#[test]
fn is_full_is_false() {
    assert!(!RoaringBitmap128::new().is_full());
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(0..1 << 20);
    bitmap.insert_range(u128::MAX - (1 << 20)..);
    assert!(!bitmap.is_full());
}

#[test]
fn default_and_clear() {
    let mut bitmap = RoaringBitmap128::default();
    assert!(bitmap.is_empty());
    assert_eq!(bitmap, RoaringBitmap128::new());
    bitmap.insert_range(u128::MAX - 70_000..=u128::MAX);
    bitmap.insert(1 << 64);
    bitmap.clear();
    assert!(bitmap.is_empty());
    assert_eq!(bitmap.len(), 0);
    assert_eq!(bitmap.min(), None);
    assert_eq!(bitmap.iter().next(), None);
    assert_valid(&bitmap);
    assert_eq!(bitmap, RoaringBitmap128::new());
}

#[test]
fn debug_format() {
    let small = RoaringBitmap128::from([1, 2, u128::MAX]);
    assert_eq!(
        format!("{small:?}"),
        format!("RoaringBitmap128<[1, 2, {}]>", u128::MAX)
    );
    assert_eq!(
        format!("{:?}", RoaringBitmap128::new()),
        "RoaringBitmap128<[]>"
    );
    let mut big: RoaringBitmap128 = (0..16u128).collect();
    big.insert(1 << 64);
    assert_eq!(
        format!("{big:?}"),
        format!(
            "RoaringBitmap128<17 values between 0 and {} in 2 containers>",
            1u128 << 64
        )
    );
}
