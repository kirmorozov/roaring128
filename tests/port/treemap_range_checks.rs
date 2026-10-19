//! Port of `tests/treemap_range_checks.rs`.
//!
//! The treemap bucket boundary (`BUCKET = 2^32`) is a parameter: `orig` uses 2^32, the variants
//! use 2^64, 2^112 and buckets just below `u128::MAX`. Containers of `RoaringBitmap128` are
//! 2^16 wide, so every "bucket" spans 65536 containers here.

use proptest::collection::hash_set;
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

const BUCKET: u128 = 1 << 32;

macro_rules! base_variants {
    ($($f:ident),* $(,)?) => {
        variants!({
            orig = 0u128,
            base_2p64 = 1u128 << 64,
            base_2p100 = (1u128 << 100) + (5 << 16),
            base_2p112 = (1u128 << 112) - 2 * (1u128 << 32),
            base_max = u128::MAX - 4 * (1u128 << 32) + 1,
        }; $($f),*);
    };
}

#[test]
fn empty_range_always_contained() {
    let rb = RoaringBitmap128::new();
    assert!(rb.contains_range(7..7));
    assert!(rb.contains_range(0..0));
    assert!(rb.contains_range(u64::MAX as u128..u64::MAX as u128));
    assert!(rb.contains_range(u128::MAX..u128::MAX));
    #[allow(clippy::reversed_empty_ranges)]
    {
        assert!(rb.contains_range(10..5));
        assert!(rb.contains_range(u128::MAX..0));
    }
    assert!(rb.contains_range((std::ops::Bound::Excluded(3), std::ops::Bound::Excluded(3))));
    assert!(rb.contains_range((
        std::ops::Bound::Excluded(u128::MAX),
        std::ops::Bound::Unbounded
    )));
}

#[test]
fn empty_range_cardinality_is_zero() {
    let rb = RoaringBitmap128::new();
    assert_eq!(rb.range_cardinality(7..7), 0);
    assert_eq!(rb.range_cardinality(u64::MAX as u128..u64::MAX as u128), 0);
    assert_eq!(rb.range_cardinality(u128::MAX..u128::MAX), 0);
    let full = RoaringBitmap128::from([0, 7, u128::MAX]);
    assert_eq!(full.range_cardinality(7..7), 0);
    #[allow(clippy::reversed_empty_ranges)]
    {
        assert_eq!(full.range_cardinality(u128::MAX..0), 0);
    }
    assert_eq!(
        full.range_cardinality((
            std::ops::Bound::Excluded(u128::MAX),
            std::ops::Bound::Unbounded
        )),
        0
    );
}

#[test]
fn empty_treemap_not_contained() {
    let rb = RoaringBitmap128::new();
    assert!(!rb.contains_range(0..1));
    assert!(!rb.contains_range(0..=u64::MAX as u128));
    assert!(!rb.contains_range(0..=u128::MAX));
    assert!(!rb.contains_range(..));
    assert_eq!(rb.range_cardinality(0..=u64::MAX as u128), 0);
    assert_eq!(rb.range_cardinality(..), 0);
}

fn single_bucket_contained(base: u128) {
    let mut rb = RoaringBitmap128::new();
    rb.insert_range(base + 10..base + 20);
    assert!(rb.contains_range(base + 10..base + 20));
    assert!(rb.contains_range(base + 11..base + 19));
    assert!(!rb.contains_range(base + 9..base + 20));
    assert!(!rb.contains_range(base + 10..base + 21));
    assert_eq!(rb.range_cardinality(base + 10..base + 20), 10);
    assert_eq!(rb.range_cardinality(base + 10..base + 15), 5);
    assert_eq!(rb.range_cardinality(base..base + 10), 0);
    assert_eq!(rb.range_cardinality(base + 20..base + 30), 0);
}

fn cross_bucket_boundary(base: u128) {
    // Values straddle the hi=0 / hi=1 bucket boundary
    let lo_max = base + BUCKET - 1;
    let hi_min = base + BUCKET;

    let mut rb = RoaringBitmap128::new();
    rb.insert(lo_max);
    rb.insert(hi_min);

    assert!(rb.contains_range(lo_max..=lo_max));
    assert!(rb.contains_range(hi_min..=hi_min));
    assert!(rb.contains_range(lo_max..=hi_min));
    assert!(!rb.contains_range(lo_max - 1..=hi_min));
    assert!(!rb.contains_range(lo_max..=hi_min + 1));

    assert_eq!(rb.range_cardinality(lo_max..=hi_min), 2);
    assert_eq!(rb.range_cardinality(lo_max..=lo_max), 1);
    assert_eq!(rb.range_cardinality(hi_min..=hi_min), 1);
    assert_eq!(rb.range_cardinality(lo_max - 1..lo_max), 0);

    rb.insert_range(lo_max..=hi_min);
    assert!(rb.contains_range(lo_max..=hi_min));
    assert_eq!(rb.range_cardinality(lo_max..=hi_min), 2);
}

fn multi_bucket_gap_not_contained(base: u128) {
    // Insert values in buckets 0 and 2, leaving bucket 1 empty.
    let mut rb = RoaringBitmap128::new();
    rb.insert_range(base..base + BUCKET); // hi=0, full
    rb.insert_range(base + 2 * BUCKET..base + 3 * BUCKET); // hi=2, full

    assert!(!rb.contains_range(base..base + 3 * BUCKET));
    assert!(rb.contains_range(base..base + BUCKET));
    assert!(rb.contains_range(base + 2 * BUCKET..base + 3 * BUCKET));
    assert_eq!(
        rb.range_cardinality(base..base + 3 * BUCKET),
        2 * BUCKET as u64
    );
    assert_eq!(rb.len(), 2 * BUCKET as u64);
    assert_valid(&rb);
}

base_variants!(
    single_bucket_contained,
    cross_bucket_boundary,
    multi_bucket_gap_not_contained
);

#[test]
fn u64_max_boundary() {
    let m = u64::MAX as u128;
    let mut rb = RoaringBitmap128::new();
    rb.insert(m);
    assert!(rb.contains_range(m..=m));
    assert!(!rb.contains_range(m - 1..=m));
    assert_eq!(rb.range_cardinality(m..=m), 1);
    assert_eq!(rb.range_cardinality(m - 1..m), 0);

    rb.insert(m - 1);
    assert!(rb.contains_range(m - 1..=m));
    assert_eq!(rb.range_cardinality(m - 1..=m), 2);
    // not the end of the space any more
    assert!(!rb.contains_range(m - 1..=m + 1));
    rb.insert(m + 1);
    assert!(rb.contains_range(m - 1..=m + 1));
    assert_eq!(rb.range_cardinality(m - 1..), 3);
}

#[test]
fn u128_max_boundary() {
    let m = u128::MAX;
    let mut rb = RoaringBitmap128::new();
    rb.insert(m);
    assert!(rb.contains_range(m..=m));
    assert!(rb.contains_range(m..));
    assert!(!rb.contains_range(m - 1..=m));
    assert_eq!(rb.range_cardinality(m..=m), 1);
    assert_eq!(rb.range_cardinality(m..), 1);
    assert_eq!(rb.range_cardinality(m - 1..m), 0);

    rb.insert(m - 1);
    assert!(rb.contains_range(m - 1..=m));
    assert!(rb.contains_range(m - 1..));
    assert_eq!(rb.range_cardinality(m - 1..=m), 2);
    assert_eq!(rb.range_cardinality(..), 2);
}

#[test]
fn unbounded_range() {
    // Start in the last 2^32 values so that the range stays small.
    let last_bucket_start = u128::MAX - (BUCKET - 1);
    let mut rb = RoaringBitmap128::new();
    assert_eq!(rb.insert_range(last_bucket_start..), BUCKET as u64);
    assert!(rb.contains_range(last_bucket_start..));
    assert!(rb.contains_range(last_bucket_start + 1..=u128::MAX));
    assert!(!rb.contains_range(last_bucket_start - 1..=u128::MAX));
    assert!(!rb.contains_range(..));
    assert_eq!(rb.range_cardinality(..), BUCKET as u64);
    assert_eq!(rb.range_cardinality(..last_bucket_start), 0);
}

#[test]
fn unbounded_range_u64_bucket() {
    // the original: `(u32::MAX << 32)..` is the last 2^32 values of u64, not of u128
    let last_bucket_start = (u32::MAX as u128) << 32;
    let mut rb = RoaringBitmap128::new();
    rb.insert_range(last_bucket_start..=u64::MAX as u128);
    assert!(rb.contains_range(last_bucket_start..=u64::MAX as u128));
    assert!(!rb.contains_range(last_bucket_start..));
    assert!(rb.contains_range(last_bucket_start + 1..=u64::MAX as u128));
    assert!(!rb.contains_range(last_bucket_start - 1..=u64::MAX as u128));
}

fn proptest_range_body(base: u128, start: u64, len: u64, extra: &std::collections::HashSet<u64>) {
    let start = base + start as u128;
    let end = start + len as u128;
    let range = start..end;
    let inverse_empty_range = end..start;

    let mut rb = RoaringBitmap128::new();
    rb.insert_range(range.clone());
    assert!(rb.contains_range(range.clone()));
    assert!(rb.contains_range(inverse_empty_range.clone()));
    assert_eq!(rb.range_cardinality(range.clone()), len);

    for &val in extra {
        rb.insert(base + val as u128);
        assert!(rb.contains_range(range.clone()));
        assert!(rb.contains_range(inverse_empty_range.clone()));
        assert_eq!(rb.range_cardinality(range.clone()), len);
    }

    for (i, val) in extra
        .iter()
        .map(|&x| base + x as u128)
        .filter(|x| range.contains(x))
        .enumerate()
    {
        rb.remove(val);
        assert!(!rb.contains_range(range.clone()));
        assert!(rb.contains_range(inverse_empty_range.clone()));
        assert_eq!(rb.range_cardinality(range.clone()), len - i as u64 - 1);
    }
}

fn proptest_range_boundaries_body(base: u128, start: u64, len: u64) {
    let mut rb = RoaringBitmap128::new();
    let start = base + start as u128;
    let end = start + len as u128;
    let half = start + len as u128 / 2;
    rb.insert_range(start..end);

    assert!(rb.contains_range(start..end));
    assert!(rb.contains_range(start + 1..end));
    assert!(rb.contains_range(start..end.saturating_sub(1)));
    assert!(rb.contains_range(start + 1..end.saturating_sub(1)));

    assert!(!rb.contains_range(start - 1..end));
    assert!(!rb.contains_range(start - 1..end.saturating_sub(1)));
    assert!(!rb.contains_range(start..end + 1));
    assert!(!rb.contains_range(start + 1..end + 1));
    assert!(!rb.contains_range(start - 1..end + 1));

    assert!(!rb.contains_range(start - 1..half));
    assert!(!rb.contains_range(half..end + 1));
}

fn proptest_cross_bucket_body(base: u128, start_lo: u32, end_lo: u32) {
    let start = base + start_lo as u128;
    let end = base + (BUCKET | end_lo as u128);

    let mut rb = RoaringBitmap128::new();
    rb.insert_range(start..=end);

    assert!(rb.contains_range(start..=end));
    assert_eq!(
        rb.range_cardinality(start..=end),
        (u32::MAX as u64 - start_lo as u64 + 1) + (end_lo as u64 + 1),
    );

    if end < u128::MAX {
        assert!(!rb.contains_range(start..=end + 1));
    }
    if start > 0 {
        assert!(!rb.contains_range(start - 1..=end));
    }
}

fn base() -> impl Strategy<Value = u128> {
    prop::sample::select(vec![
        0u128,
        1u128 << 64,
        (1u128 << 100) + (5 << 16),
        (1u128 << 112) - 2 * BUCKET,
        u128::MAX - 4 * BUCKET + 1,
    ])
}

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn proptest_range(
        // Keep values well within a single bucket to avoid very slow tests from
        // inserting billions of values across bucket boundaries.
        start in ..=262_143_u64,
        len in ..=262_143_u64,
        extra in hash_set(..=462_143_u64, ..=100),
    ) {
        proptest_range_body(0, start, len, &extra);
    }

    #[test]
    fn proptest_range_u128(
        base in base(),
        start in ..=262_143_u64,
        len in ..=262_143_u64,
        extra in hash_set(..=462_143_u64, ..=100),
    ) {
        proptest_range_body(base, start, len, &extra);
    }

    #[test]
    fn proptest_range_boundaries(
        start in 1..=262_143_u64,
        len in 0..=262_143_u64,
    ) {
        proptest_range_boundaries_body(0, start, len);
    }

    #[test]
    fn proptest_range_boundaries_u128(
        base in base(),
        start in 1..=262_143_u64,
        len in 0..=262_143_u64,
    ) {
        proptest_range_boundaries_body(base, start, len);
    }
}

proptest! {
    // every case inserts ~2^32 values (65536 full containers)
    #![proptest_config(config(8))]

    #[test]
    fn proptest_cross_bucket(
        // start_lo: low 32 bits of start value, in bucket hi=0
        start_lo in 0_u32..=u32::MAX / 2,
        // end_lo: low 32 bits of end value, in bucket hi=1
        end_lo in u32::MAX / 2..=u32::MAX,
    ) {
        proptest_cross_bucket_body(0, start_lo, end_lo);
    }

    #[test]
    fn proptest_cross_bucket_u128(
        base in base(),
        start_lo in 0_u32..=u32::MAX / 2,
        end_lo in u32::MAX / 2..=u32::MAX,
    ) {
        proptest_cross_bucket_body(base, start_lo, end_lo);
    }
}
