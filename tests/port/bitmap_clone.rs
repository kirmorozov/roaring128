//! Port of `tests/clone.rs` (and `tests/treemap_clone.rs`, a subset of it).

use roaring128::RoaringBitmap128;

use crate::common::*;

#[allow(clippy::redundant_clone)]
fn array(off: u128) {
    let original = shifted(off, 0..2000);
    let clone = original.clone();

    assert_eq!(clone, original);
}

#[allow(clippy::redundant_clone)]
fn bitmap(off: u128) {
    let original = shifted(off, 0..6000);
    let clone = original.clone();

    assert_eq!(clone, original);
}

#[allow(clippy::redundant_clone)]
fn arrays(off: u128) {
    let original =
        shifted(off, (0..2000).chain(1_000_000..1_002_000).chain(2_000_000..2_001_000));
    let clone = original.clone();

    assert_eq!(clone, original);
}

#[allow(clippy::redundant_clone)]
fn bitmaps(off: u128) {
    let original =
        shifted(off, (0..6000).chain(1_000_000..1_012_000).chain(2_000_000..2_010_000));
    let clone = original.clone();

    assert_eq!(clone, original);
}

#[allow(clippy::redundant_clone)]
fn runs(off: u128) {
    let mut original =
        shifted(off, (0..6000).chain(1_000_000..1_012_000).chain(2_000_000..2_010_000));
    original.optimize();
    let clone = original.clone();

    assert_eq!(clone, original);
}

offset_variants!(array, bitmap, arrays, bitmaps, runs);

#[test]
fn clone_from_replaces_content_u128() {
    let source: RoaringBitmap128 = [0, 1 << 64, u128::MAX].into_iter().collect();
    let mut target = shifted(OFF_STRADDLE_2P112, 0..10_000);
    target.clone_from(&source);
    assert_valid(&target);
    assert_eq!(target, source);
    assert_eq!(target.len(), 3);

    let mut target = source.clone();
    target.clone_from(&RoaringBitmap128::new());
    assert!(target.is_empty());
    assert_valid(&target);
}

#[test]
fn clone_is_independent_u128() {
    let original: RoaringBitmap128 = (u128::MAX - 100..=u128::MAX).collect();
    let mut clone = original.clone();
    clone.insert(5);
    clone.remove(u128::MAX);
    assert_eq!(original.len(), 101);
    assert!(original.contains(u128::MAX));
    assert!(!original.contains(5));
    assert_ne!(clone, original);
}
