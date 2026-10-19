//! Port of `tests/range_checks.rs`.

use proptest::collection::hash_set;
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

#[test]
fn u32_max() {
    let m = u32::MAX as u128;
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert(m);
    assert!(bitmap.contains_range(m..=m));
    assert!(!bitmap.contains_range(m - 1..=m));

    bitmap.insert_range(4_000_000_000..=m);
    assert!(bitmap.contains_range(4_000_000_000..=m));
    assert!(bitmap.contains_range(4_000_000_000..m));
    assert!(bitmap.contains_range(4_100_000_000..=m));
    // in 128 bits `4_000_000_000..` does not stop at u32::MAX
    assert!(!bitmap.contains_range(4_000_000_000..));
    assert!(!bitmap.contains_range(4_000_000_000..=m + 1));
    assert_valid(&bitmap);
}

#[test]
fn u128_max() {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert(u128::MAX);
    assert!(bitmap.contains_range(u128::MAX..=u128::MAX));
    assert!(bitmap.contains_range(u128::MAX..));
    assert!(!bitmap.contains_range(u128::MAX - 1..=u128::MAX));
    assert!(!bitmap.contains_range(u128::MAX - 1..));

    let start = u128::MAX - 294_967_295; // as many values as 4_000_000_000..=u32::MAX
    assert_eq!(bitmap.insert_range(start..), 294_967_295);
    assert!(bitmap.contains_range(start..));
    assert!(bitmap.contains_range(start..u128::MAX));
    assert!(bitmap.contains_range(start..=u128::MAX));
    assert!(bitmap.contains_range(start + 100_000_000..=u128::MAX));
    assert!(!bitmap.contains_range(start - 1..));
    assert_eq!(bitmap.len(), 294_967_296);
    assert_eq!(bitmap.range_cardinality(start..), 294_967_296);
    assert_eq!(bitmap.range_cardinality(..), 294_967_296);
    assert_valid(&bitmap);
}

fn proptest_range_body(off: u128, start: u32, len: u32, extra: &std::collections::HashSet<u32>) {
    let start = off + start as u128;
    let end = start + len as u128;
    let range = start..end;
    let inverse_empty_range = (start + len as u128)..start;

    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(range.clone());
    assert!(bitmap.contains_range(range.clone()));
    assert!(bitmap.contains_range(inverse_empty_range.clone()));
    assert_eq!(bitmap.range_cardinality(range.clone()), len as u64);

    for &val in extra {
        bitmap.insert(off + val as u128);
        assert!(bitmap.contains_range(range.clone()));
        assert!(bitmap.contains_range(inverse_empty_range.clone()));
        assert_eq!(bitmap.range_cardinality(range.clone()), len as u64);
    }

    for (i, val) in extra.iter().map(|&x| off + x as u128).filter(|x| range.contains(x)).enumerate()
    {
        bitmap.remove(val);
        assert!(!bitmap.contains_range(range.clone()));
        assert!(bitmap.contains_range(inverse_empty_range.clone()));
        assert_eq!(bitmap.range_cardinality(range.clone()), len as u64 - i as u64 - 1);
    }
    assert_valid(&bitmap);
}

fn proptest_range_boundaries_body(off: u128, start: u32, len: u32) {
    let start = off + start as u128;
    let mut bitmap = RoaringBitmap128::new();
    let end = start + len as u128;
    let half = start + len as u128 / 2;
    bitmap.insert_range(start..end);

    assert!(bitmap.contains_range(start..end));

    assert!(bitmap.contains_range(start + 1..end));
    assert!(bitmap.contains_range(start..end - 1));
    assert!(bitmap.contains_range(start + 1..end - 1));

    assert!(!bitmap.contains_range(start - 1..end));
    assert!(!bitmap.contains_range(start - 1..end - 1));
    assert!(!bitmap.contains_range(start..end + 1));
    assert!(!bitmap.contains_range(start + 1..end + 1));
    assert!(!bitmap.contains_range(start - 1..end + 1));

    assert!(!bitmap.contains_range(start - 1..half));
    assert!(!bitmap.contains_range(half..end + 1));
}

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn proptest_range(
        start in ..=262_143_u32,
        len in ..=262_143_u32,
        extra in hash_set(..=462_143_u32, ..=100),
    ){
        proptest_range_body(0, start, len, &extra);
    }

    #[test]
    fn proptest_range_u128(
        off in prop::sample::select(OFFSETS.iter().map(|o| o.1).collect::<Vec<_>>()),
        start in ..=262_143_u32,
        len in ..=262_143_u32,
        extra in hash_set(..=462_143_u32, ..=100),
    ){
        proptest_range_body(off, start, len, &extra);
    }

    #[test]
    fn proptest_range_boundaries(
        // Ensure we can always subtract one from start
        start in 1..=262_143_u32,
        len in 0..=262_143_u32,
    ) {
        proptest_range_boundaries_body(0, start, len);
    }

    #[test]
    fn proptest_range_boundaries_u128(
        off in prop::sample::select(OFFSETS.iter().map(|o| o.1).collect::<Vec<_>>()),
        start in 1..=262_143_u32,
        len in 0..=262_143_u32,
    ) {
        proptest_range_boundaries_body(off, start, len);
    }
}
