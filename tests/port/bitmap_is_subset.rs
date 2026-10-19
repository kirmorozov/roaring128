//! Port of `tests/is_subset.rs` (identical to `tests/treemap_is_subset.rs`).

use roaring128::RoaringBitmap128;

use crate::common::*;

fn array_not(off: u128) {
    let sup = shifted(off, 0..2000);
    let sub = shifted(off, 1000..3000);
    assert!(!sub.is_subset(&sup));
}

fn array(off: u128) {
    let sup = shifted(off, 0..4000);
    let sub = shifted(off, 2000..3000);
    assert!(sub.is_subset(&sup));
}

fn array_bitmap_not(off: u128) {
    let sup = shifted(off, 0..2000);
    let sub = shifted(off, 1000..15000);
    assert!(!sub.is_subset(&sup));
}

fn bitmap_not(off: u128) {
    let sup = shifted(off, 0..6000);
    let sub = shifted(off, 4000..10000);
    assert!(!sub.is_subset(&sup));
}

fn bitmap(off: u128) {
    let sup = shifted(off, 0..20000);
    let sub = shifted(off, 5000..15000);
    assert!(sub.is_subset(&sup));
}

fn bitmap_array_not(off: u128) {
    let sup = shifted(off, 0..20000);
    let sub = shifted(off, 19000..21000);
    assert!(!sub.is_subset(&sup));
}

fn bitmap_array(off: u128) {
    let sup = shifted(off, 0..20000);
    let sub = shifted(off, 18000..20000);
    assert!(sub.is_subset(&sup));
}

fn arrays_not(off: u128) {
    let sup = shifted(off, (0..2000).chain(1_000_000..1_002_000));
    let sub = shifted(off, (100_000..102_000).chain(1_100_000..1_102_000));
    assert!(!sub.is_subset(&sup));
}

fn arrays(off: u128) {
    let sup = shifted(off, (0..3000).chain(100_000..103_000));
    let sub = shifted(off, (0..2000).chain(100_000..102_000));
    assert!(sub.is_subset(&sup));
}

fn bitmaps_not(off: u128) {
    let sup = shifted(
        off,
        (0..6000)
            .chain(1_000_000..1_006_000)
            .chain(2_000_000..2_010_000),
    );
    let sub = shifted(off, (100_000..106_000).chain(1_100_000..1_106_000));
    assert!(!sub.is_subset(&sup));
}

fn bitmaps(off: u128) {
    let sup = shifted(off, (0..1_000_000).chain(2_000_000..2_010_000));
    let sub = shifted(off, (0..10_000).chain(500_000..510_000));
    assert!(sub.is_subset(&sup));
}

offset_variants!(
    array_not,
    array,
    array_bitmap_not,
    bitmap_not,
    bitmap,
    bitmap_array_not,
    bitmap_array,
    arrays_not,
    arrays,
    bitmaps_not,
    bitmaps
);

#[test]
fn subset_superset_u128() {
    let empty = RoaringBitmap128::new();
    let mut sup = RoaringBitmap128::new();
    sup.insert_range(u128::MAX - 300_000..=u128::MAX);
    let sub: RoaringBitmap128 = [u128::MAX, u128::MAX - 300_000, u128::MAX - 65_536]
        .into_iter()
        .collect();
    assert!(empty.is_subset(&empty));
    assert!(empty.is_subset(&sup));
    assert!(!sup.is_subset(&empty));
    assert!(sub.is_subset(&sup));
    assert!(sup.is_superset(&sub));
    assert!(!sub.is_superset(&sup));
    assert!(sup.is_subset(&sup));
    assert!(sup.is_superset(&sup));
    let mut not_sub = sub.clone();
    not_sub.insert(u128::MAX - 300_001);
    assert!(!not_sub.is_subset(&sup));
    // same low bits in another key
    let other: RoaringBitmap128 = sub.iter().map(|v| v ^ (1 << 100)).collect();
    assert!(!other.is_subset(&sup));
}
