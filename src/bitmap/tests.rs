//! Unit tests of the top-level bitmap logic: key boundaries, strategy selection of the set
//! operations, iterator seek / size-hint bookkeeping, append / extend fast paths and the
//! serialization header parsing (including hostile inputs). Every bitmap produced by an operation
//! is checked against a `BTreeSet<u128>` model and `internal_validate`.

use alloc::collections::BTreeSet;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::{Bound, RangeBounds, RangeInclusive};

use proptest::prelude::*;

use super::{SKEW_RATIO, UPDATE_SKEW_RATIO, is_skewed, is_skewed_update, prefer_point_inserts};
use crate::util::{MAX_KEY, join};
use crate::{MultiOps, RoaringBitmap128};

type Model = BTreeSet<u128>;

const K: u128 = 1 << 16;

#[track_caller]
fn check(rb: &RoaringBitmap128, model: &Model) {
    rb.internal_validate().unwrap();
    assert_eq!(rb.len(), model.len() as u64);
    assert_eq!(rb.is_empty(), model.is_empty());
    assert_eq!(rb.min(), model.first().copied());
    assert_eq!(rb.max(), model.last().copied());
    assert!(rb.iter().eq(model.iter().copied()));
}

fn build(model: &Model) -> RoaringBitmap128 {
    let rb: RoaringBitmap128 = model.iter().copied().collect();
    check(&rb, model);
    rb
}

/// Values around the interesting key boundaries.
fn value() -> impl Strategy<Value = u128> {
    prop_oneof![
        3 => (0u128..4, any::<u16>()).prop_map(|(k, i)| join(k, i)),
        2 => (0u128..4, any::<u16>()).prop_map(|(k, i)| join(MAX_KEY - k, i)),
        1 => (0u128..64, 0u16..8).prop_map(|(k, i)| join(k * 3, i)),
        1 => any::<u128>(),
        1 => (0u128..32).prop_map(|d| u128::MAX - d),
    ]
}

/// A bitmap made of a few points and ranges (dense, sparse and run containers, keys at both ends
/// of the key space).
fn model() -> impl Strategy<Value = Model> {
    let piece = prop_oneof![
        4 => value().prop_map(|v| (v, 1u128)),
        2 => (value(), 1u128..300).prop_map(|(v, n)| (v, n)),
        1 => (value(), 4000u128..9000).prop_map(|(v, n)| (v, n)),
        1 => (value(), 60000u128..140000).prop_map(|(v, n)| (v, n)),
    ];
    proptest::collection::vec(piece, 0..8).prop_map(|pieces| {
        let mut model = Model::new();
        for (start, n) in pieces {
            let end = start.saturating_add(n - 1);
            model.extend(start..=end);
        }
        model
    })
}

fn range_strategy() -> impl Strategy<Value = (Bound<u128>, Bound<u128>)> {
    let bound = prop_oneof![
        value().prop_map(Bound::Included),
        value().prop_map(Bound::Excluded),
        Just(Bound::Unbounded),
    ];
    (bound.clone(), bound)
}

/// Whether a range is accepted by `range()` (otherwise it panics).
fn valid_iter_range(range: &(Bound<u128>, Bound<u128>)) -> bool {
    use Bound::*;
    match *range {
        (Excluded(s), Excluded(e)) if s == e => false,
        (Included(s) | Excluded(s), Included(e) | Excluded(e)) => s <= e,
        _ => true,
    }
}

fn model_range(model: &Model, range: &(Bound<u128>, Bound<u128>)) -> Vec<u128> {
    let in_range = |v: &u128| range.contains(v);
    model.iter().copied().filter(in_range).collect()
}

// ---------------------------------------------------------------------------------------------
// Logic that does not need containers
// ---------------------------------------------------------------------------------------------

#[test]
fn strategy_thresholds() {
    assert!(is_skewed(0, 1));
    assert!(!is_skewed(0, 0));
    assert!(is_skewed(1, SKEW_RATIO + 1));
    assert!(!is_skewed(1, SKEW_RATIO));
    assert!(!is_skewed(100, 100));
    assert!(!is_skewed(usize::MAX, usize::MAX));
    assert!(is_skewed_update(1, UPDATE_SKEW_RATIO + 1));
    assert!(!is_skewed_update(1, UPDATE_SKEW_RATIO));
    // Between the two ratios: in-place updates use point entries, queries still merge.
    assert!(is_skewed_update(150, 2000) && !is_skewed(150, 2000));
    assert!(prefer_point_inserts(1, 100));
    assert!(!prefer_point_inserts(50, 100));
    assert!(!prefer_point_inserts(usize::MAX, usize::MAX));
}

#[test]
fn empty_bitmap_queries() {
    let rb = RoaringBitmap128::new();
    rb.internal_validate().unwrap();
    assert_eq!(rb.len(), 0);
    assert!(rb.is_empty());
    assert!(!rb.is_full());
    assert_eq!(rb.min(), None);
    assert_eq!(rb.max(), None);
    assert_eq!(rb.rank(u128::MAX), 0);
    assert_eq!(rb.rank(0), 0);
    assert_eq!(rb.select(0), None);
    assert!(!rb.contains(0));
    assert!(!rb.contains(u128::MAX));
    assert!(rb.contains_range(5..5));
    assert!(!rb.contains_range(..));
    assert_eq!(rb.range_cardinality(..), 0);
    assert_eq!(rb.iter().size_hint(), (0, Some(0)));
    assert_eq!(rb.iter().next(), None);
    assert_eq!(rb.iter().next_back(), None);
    assert_eq!(rb.iter().next_range(), None);
    assert_eq!(rb.iter().next_range_back(), None);
    assert_eq!(rb.range(..).count(), 0);
    let mut iter = rb.iter();
    iter.advance_to(u128::MAX);
    iter.advance_back_to(0);
    assert_eq!(iter.len(), 0);
    assert_eq!(rb.clone().into_iter().len(), 0);
    assert_eq!(rb.clone().into_range(3..).next(), None);
    assert_eq!(RoaringBitmap128::from_lsb0_bytes(u128::MAX, &[]), rb);
}

#[test]
#[should_panic(expected = "range start is greater than range end")]
fn range_start_greater_than_end_panics() {
    let rb = RoaringBitmap128::new();
    let _ = rb.range((Bound::Included(5), Bound::Included(4)));
}

#[test]
#[should_panic(expected = "range start and end are equal and excluded")]
fn range_equal_excluded_panics() {
    let rb = RoaringBitmap128::new();
    let _ = rb.range((Bound::Excluded(5), Bound::Excluded(5)));
}

#[test]
#[should_panic(expected = "range start is greater than range end")]
fn into_range_start_greater_than_end_panics() {
    let _ = RoaringBitmap128::new().into_range((Bound::Included(u128::MAX), Bound::Included(0)));
}

#[test]
#[should_panic(expected = "range start and end are equal and excluded")]
fn into_range_equal_excluded_panics() {
    let _ = RoaringBitmap128::new().into_range((Bound::Excluded(0), Bound::Excluded(0)));
}

#[test]
#[should_panic(expected = "offset + bytes.len() * 8 must be <= 2^128")]
fn from_lsb0_bytes_overflow_panics() {
    let _ = RoaringBitmap128::from_lsb0_bytes(u128::MAX - 6, &[0]);
}

#[test]
fn empty_ranges_are_no_ops() {
    let mut rb = RoaringBitmap128::new();
    #[allow(clippy::reversed_empty_ranges)]
    {
        assert_eq!(rb.insert_range(5..5), 0);
        assert_eq!(rb.insert_range(6..5), 0);
        assert_eq!(rb.remove_range(6..5), 0);
        assert_eq!(rb.range_cardinality(6..5), 0);
        assert!(rb.contains_range(6..5));
    }
    assert_eq!(
        rb.insert_range((Bound::Excluded(u128::MAX), Bound::Unbounded)),
        0
    );
    assert!(rb.is_empty());
}

// ---------------------------------------------------------------------------------------------
// Serialization header parsing and hostile inputs
// ---------------------------------------------------------------------------------------------

#[cfg(feature = "std")]
mod serialization {
    use super::*;
    use crate::bitmap::serialization::{COOKIE, FLAG_RUN_BITSET};
    use std::io::{self, Cursor};

    fn header(cookie: u32, flags: u32, count: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&cookie.to_le_bytes());
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes
    }

    fn descriptor(key: u128, low: u16) -> [u8; 16] {
        ((key << 16) | low as u128).to_le_bytes()
    }

    fn kind(bytes: &[u8]) -> io::ErrorKind {
        let checked = RoaringBitmap128::deserialize_from(bytes)
            .unwrap_err()
            .kind();
        let unchecked = RoaringBitmap128::deserialize_unchecked_from(bytes)
            .unwrap_err()
            .kind();
        assert_eq!(checked, unchecked);
        checked
    }

    #[test]
    fn cookie_is_rr18() {
        assert_eq!(&COOKIE.to_le_bytes(), b"RR18");
    }

    #[test]
    fn empty_bitmap_round_trip() {
        let rb = RoaringBitmap128::new();
        let mut bytes = Vec::new();
        rb.serialize_into(&mut bytes).unwrap();
        assert_eq!(bytes, header(COOKIE, 0, 0));
        assert_eq!(rb.serialized_size(), bytes.len());
        assert_eq!(RoaringBitmap128::deserialize_from(&bytes[..]).unwrap(), rb);
        // A run bitset flag without containers is valid too.
        let bytes = header(COOKIE, FLAG_RUN_BITSET, 0);
        assert!(
            RoaringBitmap128::deserialize_from(&bytes[..])
                .unwrap()
                .is_empty()
        );
        let other = RoaringBitmap128::new();
        assert!(
            other
                .intersection_with_serialized_unchecked(Cursor::new(&bytes))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_bad_headers() {
        assert_eq!(kind(&[]), io::ErrorKind::UnexpectedEof);
        assert_eq!(
            kind(&header(COOKIE, 0, 0)[..15]),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(kind(&header(COOKIE + 1, 0, 0)), io::ErrorKind::InvalidData);
        assert_eq!(kind(&header(12346, 0, 0)), io::ErrorKind::InvalidData);
        assert_eq!(kind(&header(COOKIE, 2, 0)), io::ErrorKind::InvalidData);
        assert_eq!(
            kind(&header(COOKIE, 0x8000_0001, 0)),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn hostile_counts_do_not_allocate() {
        // Announced counts far beyond the input: must fail with EOF quickly instead of
        // allocating terabytes.
        for count in [u64::MAX, u64::MAX / 16, 1 << 40, 1 << 33, 4097, 1] {
            for flags in [0, FLAG_RUN_BITSET] {
                let bytes = header(COOKIE, flags, count);
                assert_eq!(kind(&bytes), io::ErrorKind::UnexpectedEof);
                let mut rb = RoaringBitmap128::new();
                // An empty `self` reads no descriptor at all.
                assert!(
                    rb.intersection_with_serialized_unchecked(Cursor::new(&bytes))
                        .unwrap()
                        .is_empty()
                );
                rb.insert(join(9, 9));
                assert_eq!(
                    rb.intersection_with_serialized_unchecked(Cursor::new(&bytes))
                        .unwrap_err()
                        .kind(),
                    io::ErrorKind::UnexpectedEof
                );
            }
        }
        // A run bitset that is present but truncated.
        let mut bytes = header(COOKIE, FLAG_RUN_BITSET, 1 << 20);
        bytes.extend_from_slice(&[0; 100]);
        assert_eq!(kind(&bytes), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn rejects_unsorted_or_duplicate_keys() {
        for (a, b) in [(5u128, 5u128), (6, 5), (MAX_KEY, 0)] {
            let mut bytes = header(COOKIE, 0, 2);
            bytes.extend_from_slice(&descriptor(a, 0));
            bytes.extend_from_slice(&descriptor(b, 0));
            // Payloads would follow, but the descriptors are rejected first.
            assert_eq!(kind(&bytes), io::ErrorKind::InvalidData);
        }
    }

    #[test]
    fn rejects_run_bitset_padding() {
        let mut bytes = header(COOKIE, FLAG_RUN_BITSET, 1);
        bytes.push(0b10);
        bytes.extend_from_slice(&descriptor(0, 0));
        bytes.extend_from_slice(&7u16.to_le_bytes());
        assert_eq!(
            RoaringBitmap128::deserialize_from(&bytes[..])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn rejects_overflowing_runs() {
        let mut bytes = header(COOKIE, FLAG_RUN_BITSET, 1);
        bytes.push(1);
        bytes.extend_from_slice(&descriptor(3, 0));
        bytes.extend_from_slice(&0xFFF0u16.to_le_bytes());
        bytes.extend_from_slice(&0x0010u16.to_le_bytes());
        assert_eq!(kind(&bytes), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_invalid_containers() {
        // Unsorted array.
        let mut bytes = header(COOKIE, 0, 1);
        bytes.extend_from_slice(&descriptor(1, 1));
        bytes.extend_from_slice(&9u16.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        // Array order is checked by both readers (no debug assertion can fire).
        assert_eq!(kind(&bytes), io::ErrorKind::InvalidData);
        // Duplicate array values.
        let mut bytes = header(COOKIE, 0, 1);
        bytes.extend_from_slice(&descriptor(1, 1));
        bytes.extend_from_slice(&[7, 0, 7, 0]);
        assert_eq!(kind(&bytes), io::ErrorKind::InvalidData);
        // Bitmap with a wrong cardinality.
        let mut bytes = header(COOKIE, 0, 1);
        bytes.extend_from_slice(&descriptor(1, 5000));
        bytes.extend_from_slice(&[0xFF; 8192]);
        assert_eq!(
            RoaringBitmap128::deserialize_from(&bytes[..])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        // Unchecked: an invalid bitmap (the announced cardinality), no panic even in debug.
        let rb = RoaringBitmap128::deserialize_unchecked_from(&bytes[..]).unwrap();
        assert_eq!(rb.len(), 5001);
        assert!(rb.internal_validate().is_err());
        // The payloads read by the serialized intersection are validated.
        let ours: RoaringBitmap128 = [join(1, 3)].into();
        assert_eq!(
            ours.intersection_with_serialized_unchecked(Cursor::new(&bytes))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        // A correct bitmap payload read in place, checked and unchecked.
        let mut bytes = header(COOKIE, 0, 1);
        bytes.extend_from_slice(&descriptor(MAX_KEY, 5119));
        bytes.extend_from_slice(&[0xFF; 640]);
        bytes.extend_from_slice(&[0x00; 8192 - 640]);
        for rb in [
            RoaringBitmap128::deserialize_from(&bytes[..]).unwrap(),
            RoaringBitmap128::deserialize_unchecked_from(&bytes[..]).unwrap(),
        ] {
            rb.internal_validate().unwrap();
            assert!(rb.iter().eq((0..5120u16).map(|i| join(MAX_KEY, i))));
        }
        // Overlapping runs.
        let mut bytes = header(COOKIE, FLAG_RUN_BITSET, 1);
        bytes.push(1);
        bytes.extend_from_slice(&descriptor(1, 1));
        for v in [10u16, 5, 12, 1] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(kind(&bytes), io::ErrorKind::InvalidData);
        // Adjacent runs.
        let mut bytes = header(COOKIE, FLAG_RUN_BITSET, 1);
        bytes.push(1);
        bytes.extend_from_slice(&descriptor(1, 1));
        for v in [10u16, 5, 16, 1] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(kind(&bytes), io::ErrorKind::InvalidData);
        // Truncated payload.
        let mut bytes = header(COOKIE, 0, 1);
        bytes.extend_from_slice(&descriptor(1, 3));
        bytes.extend_from_slice(&[1, 0, 2, 0]);
        assert_eq!(kind(&bytes), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn layout_matches_format() {
        let mut rb = RoaringBitmap128::new();
        rb.insert(join(7, 3));
        rb.insert(join(7, 9));
        rb.insert_range(join(MAX_KEY, 10)..=join(MAX_KEY, 20));
        rb.optimize();
        let mut bytes = Vec::new();
        rb.serialize_into(&mut bytes).unwrap();
        assert_eq!(bytes.len(), rb.serialized_size());

        let mut expected = header(COOKIE, FLAG_RUN_BITSET, 2);
        expected.push(0b10);
        expected.extend_from_slice(&descriptor(7, 1));
        expected.extend_from_slice(&descriptor(MAX_KEY, 0));
        for v in [3u16, 9, 10, 10] {
            expected.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(bytes, expected);
        assert_eq!(RoaringBitmap128::deserialize_from(&bytes[..]).unwrap(), rb);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "needs more than 4096 containers: too slow for the CI Miri step (`container` filter)"
    )]
    fn many_containers_cross_chunk_boundaries() {
        // More descriptors than one read/write chunk, mixed kinds.
        let mut rb = RoaringBitmap128::new();
        for k in 0..10_000u128 {
            rb.insert(join(k * 5, (k % 7) as u16));
        }
        rb.insert_range(join(70_000, 0)..join(70_002, 0));
        rb.insert_range(join(80_000, 0)..join(80_000, 6000));
        rb.remove_run_compression();
        rb.optimize();
        let mut bytes = Vec::new();
        rb.serialize_into(&mut bytes).unwrap();
        assert_eq!(bytes.len(), rb.serialized_size());
        let back = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
        back.internal_validate().unwrap();
        assert_eq!(back, rb);
        let mut other = RoaringBitmap128::new();
        for k in (0..60_000u128).step_by(3) {
            other.insert(join(k, (k % 11) as u16));
        }
        other.insert_range(join(70_001, 5)..join(80_000, 10));
        let expected = &other & &rb;
        let got = other
            .intersection_with_serialized_unchecked(Cursor::new(&bytes))
            .unwrap();
        got.internal_validate().unwrap();
        assert_eq!(got, expected);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn round_trip(model in model(), optimize in any::<bool>()) {
            let mut rb = build(&model);
            if optimize {
                rb.optimize();
            }
            let mut bytes = Vec::new();
            rb.serialize_into(&mut bytes).unwrap();
            prop_assert_eq!(bytes.len(), rb.serialized_size());
            let back = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
            check(&back, &model);
            let back = RoaringBitmap128::deserialize_unchecked_from(&bytes[..]).unwrap();
            check(&back, &model);
            // Any truncation is an error.
            if !bytes.is_empty() {
                let cut = bytes.len() / 2;
                prop_assert!(RoaringBitmap128::deserialize_from(&bytes[..cut]).is_err());
            }
        }

        #[test]
        fn intersection_with_serialized(a in model(), b in model()) {
            let ra = build(&a);
            let rb = build(&b);
            let mut bytes = Vec::new();
            rb.serialize_into(&mut bytes).unwrap();
            // A prefix in front of the bitmap: seeks must be relative.
            let mut stream = vec![0xAB; 3];
            stream.extend_from_slice(&bytes);
            let mut cursor = Cursor::new(stream);
            cursor.set_position(3);
            let got = ra.intersection_with_serialized_unchecked(cursor).unwrap();
            let expected: Model = a.intersection(&b).copied().collect();
            check(&got, &expected);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Key boundaries
// ---------------------------------------------------------------------------------------------

#[test]
fn key_space_ends() {
    let mut rb = RoaringBitmap128::new();
    assert!(rb.insert(u128::MAX));
    assert!(rb.insert(0));
    assert!(!rb.insert(u128::MAX));
    assert_eq!(rb.min(), Some(0));
    assert_eq!(rb.max(), Some(u128::MAX));
    assert_eq!(rb.rank(u128::MAX), 2);
    assert_eq!(rb.rank(u128::MAX - 1), 1);
    assert_eq!(rb.select(1), Some(u128::MAX));
    assert!(rb.iter().eq([0, u128::MAX]));
    assert!(rb.iter().rev().eq([u128::MAX, 0]));
    assert_eq!(rb.range(1..).collect::<Vec<_>>(), vec![u128::MAX]);
    assert_eq!(rb.range(..u128::MAX).collect::<Vec<_>>(), vec![0]);
    assert_eq!(rb.try_push(u128::MAX), Err(crate::IntegerTooSmall));

    // Full last container and ranges ending at u128::MAX.
    let mut rb = RoaringBitmap128::new();
    assert_eq!(rb.insert_range(u128::MAX - 0x1_FFFF..), 0x2_0000);
    assert_eq!(rb.len(), 0x2_0000);
    assert!(rb.contains_range(u128::MAX - 0x1_FFFF..));
    assert!(!rb.contains_range(u128::MAX - 0x2_0000..));
    assert_eq!(rb.range_cardinality(..), 0x2_0000);
    assert_eq!(rb.range_cardinality(u128::MAX - 5..), 6);
    assert_eq!(
        rb.iter().next_range(),
        Some(u128::MAX - 0x1_FFFF..=u128::MAX)
    );
    assert_eq!(
        rb.iter().next_range_back(),
        Some(u128::MAX - 0x1_FFFF..=u128::MAX)
    );
    assert_eq!(rb.iter().next_back(), Some(u128::MAX));
    assert_eq!(rb.remove_range(u128::MAX..), 1);
    assert_eq!(rb.max(), Some(u128::MAX - 1));
    rb.remove_biggest(0xFFFF);
    assert_eq!(rb.max(), Some(u128::MAX - 0x1_0000));
    rb.internal_validate().unwrap();
    assert_eq!(rb.remove_range(..), 0x1_0000);
    assert!(rb.is_empty());
    rb.internal_validate().unwrap();
}

#[test]
fn ranges_across_many_keys() {
    let mut model = Model::new();
    let mut rb = RoaringBitmap128::new();

    // Empty index: the middle containers are bulk-built.
    let r = join(3, 100)..join(100, 7);
    assert_eq!(rb.insert_range(r.clone()), (r.end - r.start) as u64);
    model.extend(r.clone());
    check(&rb, &model);
    assert_eq!(rb.statistics().n_containers, 98);

    // Overlapping range with existing (partially filled) containers.
    let r2 = join(50, 1)..=join(160, 2);
    let expected = r2.clone().filter(|v| !model.contains(v)).count() as u64;
    assert_eq!(rb.insert_range(r2.clone()), expected);
    model.extend(r2);
    check(&rb, &model);

    // Big index, short span: point path.
    for k in 1000..3000u128 {
        rb.insert(join(k, 1));
        model.insert(join(k, 1));
    }
    let r3 = join(1990, 0)..join(2030, 0);
    let expected = r3.clone().filter(|v| !model.contains(v)).count() as u64;
    assert_eq!(rb.insert_range(r3.clone()), expected);
    model.extend(r3);
    check(&rb, &model);

    assert!(rb.contains_range(join(3, 100)..join(100, 7)));
    assert!(!rb.contains_range(join(3, 99)..join(100, 7)));
    assert!(!rb.contains_range(join(3, 100)..=join(161, 0)));
    for range in [
        join(0, 0)..=join(10, 0),
        join(60, 5)..=join(1995, 3),
        join(2, 0)..=join(5000, 0),
        join(1500, 1)..=join(1500, 1),
        0..=u128::MAX,
    ] {
        let expected = model.range(range.clone()).count() as u64;
        assert_eq!(rb.range_cardinality(range.clone()), expected, "{range:?}");
    }

    let removed = rb.remove_range(join(20, 5)..join(1500, 0));
    let before = model.len();
    model.retain(|v| !(join(20, 5)..join(1500, 0)).contains(v));
    assert_eq!(removed, (before - model.len()) as u64);
    check(&rb, &model);
}

/// Like `roaring`, a range insertion stores every container it fills as a single run, including
/// a middle container that was already full as a bitmap.
#[test]
fn insert_range_turns_full_bitmaps_into_runs() {
    let mut rb = RoaringBitmap128::new();
    for k in [2u128, 3] {
        for i in 0..=u16::MAX {
            rb.insert(join(k, i));
        }
    }
    let stats = rb.statistics();
    assert_eq!((stats.n_bitset_containers, stats.n_run_containers), (2, 0));
    // Key 2 is a middle key (`fill_keys`), key 3 the last one (`Container::insert_range`).
    assert_eq!(rb.insert_range(join(1, 7)..=join(3, 0xFFFF)), (1 << 16) - 7);
    let stats = rb.statistics();
    assert_eq!((stats.n_bitset_containers, stats.n_run_containers), (0, 3));
    assert_eq!(rb.len(), 3 * (1 << 16) - 7);
    rb.internal_validate().unwrap();
}

#[test]
fn rank_and_select_from_both_ends() {
    let mut model = Model::new();
    for k in 0..200u128 {
        for i in 0..(k % 5) as u16 {
            model.insert(join(k * 2, i * 3));
        }
    }
    let rb = build(&model);
    let values: Vec<u128> = model.iter().copied().collect();
    for (n, &v) in values.iter().enumerate() {
        assert_eq!(rb.select(n as u64), Some(v));
        assert_eq!(rb.rank(v), n as u64 + 1);
        if v > 0 {
            assert_eq!(rb.rank(v - 1), n as u64);
        }
    }
    assert_eq!(rb.select(values.len() as u64), None);
    assert_eq!(rb.rank(u128::MAX), values.len() as u64);
    assert_eq!(rb.rank(join(1000, 0)), values.len() as u64);
}

#[test]
fn remove_smallest_and_biggest() {
    let model: Model = (0..10u128)
        .flat_map(|k| (0..(k + 1) as u16).map(move |i| join(k, i)))
        .collect();
    for n in 0..=model.len() as u64 + 2 {
        let mut rb = build(&model);
        rb.remove_smallest(n);
        let expected: Model = model.iter().copied().skip(n as usize).collect();
        check(&rb, &expected);

        let mut rb = build(&model);
        rb.remove_biggest(n);
        let keep = model.len().saturating_sub(n as usize);
        let expected: Model = model.iter().copied().take(keep).collect();
        check(&rb, &expected);
    }
}

#[test]
fn from_lsb0_bytes_offsets() {
    let bytes: Vec<u8> = (0..20_000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 7) as u8)
        .collect();
    for offset in [
        0u128,
        1,
        7,
        8,
        9,
        K - 3,
        K * 5 + 12_345,
        u128::MAX - bytes.len() as u128 * 8 + 1,
        u128::MAX - bytes.len() as u128 * 8 - 4,
    ] {
        let rb = RoaringBitmap128::from_lsb0_bytes(offset, &bytes);
        let mut model = Model::new();
        for (i, byte) in bytes.iter().enumerate() {
            for bit in 0..8 {
                if byte >> bit & 1 == 1 {
                    model.insert(offset + i as u128 * 8 + bit);
                }
            }
        }
        check(&rb, &model);
    }
    // The last bit of the key space.
    let rb = RoaringBitmap128::from_lsb0_bytes(u128::MAX - 7, &[0x80]);
    assert!(rb.iter().eq([u128::MAX]));
}

// ---------------------------------------------------------------------------------------------
// Append / extend
// ---------------------------------------------------------------------------------------------

#[test]
fn append_into_last_container_and_new_keys() {
    let mut rb = RoaringBitmap128::from_sorted_iter([1, 2, join(5, 7)]).unwrap();
    assert_eq!(rb.append([join(5, 8), join(5, 9), join(6, 0)]), Ok(3));
    assert_eq!(
        rb.append([join(7, 0), join(7, 0)])
            .unwrap_err()
            .valid_until(),
        1
    );
    assert_eq!(rb.append([join(7, 0)]).unwrap_err().valid_until(), 0);
    let model: Model = [
        1,
        2,
        join(5, 7),
        join(5, 8),
        join(5, 9),
        join(6, 0),
        join(7, 0),
    ]
    .into();
    check(&rb, &model);
    assert_eq!(rb.try_push(join(7, 0)), Err(crate::IntegerTooSmall));
    assert_eq!(rb.try_push(join(7, 1)), Ok(()));
    assert_eq!(rb.try_push(join(9, 1)), Ok(()));
    assert_eq!(rb.len(), 9);
    rb.internal_validate().unwrap();

    // 12k containers are too slow for the CI Miri step, which selects this test through its
    // `container` filter; the part above covers the container code.
    if cfg!(miri) {
        return;
    }

    // Many new keys appended to a large index (linear append path) and to a small one.
    let mut big: RoaringBitmap128 = (0..5000u128).map(|k| join(k, 1)).collect();
    assert_eq!(big.append((5000..12_000u128).map(|k| join(k, 3))), Ok(7000));
    let model: Model = (0..5000u128)
        .map(|k| join(k, 1))
        .chain((5000..12_000u128).map(|k| join(k, 3)))
        .collect();
    check(&big, &model);
}

#[test]
fn extend_sorted_then_unsorted() {
    let mut model = Model::new();
    let mut rb = RoaringBitmap128::new();
    let values: Vec<u128> = (0..3000u128)
        .map(|k| join(k / 3, (k * 7) as u16))
        .chain([5, 5, 1, join(10_000, 1), 3, 3, u128::MAX, 0])
        .chain((0..2000u128).map(|k| join(k * 11 % 1500, k as u16)))
        .collect();
    rb.extend(values.iter().copied());
    model.extend(values.iter().copied());
    check(&rb, &model);

    // Extending above the maximum with duplicates stays on the sorted path.
    let tail = [u128::MAX - 3, u128::MAX - 3, u128::MAX - 1];
    let mut rb = build(&(0..10).collect());
    rb.extend(tail);
    let mut model: Model = (0..10).collect();
    model.extend(tail);
    check(&rb, &model);
}

/// A deterministic pseudo-random sequence (splitmix64).
fn scrambled(seed: u64, n: usize) -> impl Iterator<Item = u128> {
    let mut state = seed;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    (0..n).map(move |_| ((next() as u128) << 64) | next() as u128)
}

#[test]
fn extend_unsorted_batches() {
    // Sparse random values: several sorted batches merged into a growing index.
    let values: Vec<u128> = scrambled(1, 30_000).collect();
    let rb: RoaringBitmap128 = values.iter().copied().collect();
    let model: Model = values.iter().copied().collect();
    check(&rb, &model);

    // Random values landing in existing containers (unions with owned batch containers), with
    // duplicates, into a pre-populated index.
    let mut rb = build(&model);
    let mut model = model;
    let clustered: Vec<u128> = scrambled(2, 20_000)
        .map(|v| join((v % 500) * 7, (v >> 64) as u16 % 3000))
        .chain(values.iter().copied().step_by(5))
        .collect();
    rb.extend(clustered.iter().copied());
    model.extend(clustered.iter().copied());
    check(&rb, &model);

    // Dense unsorted values (keys rarely change): direct insertion path.
    let dense: Vec<u128> = (0..40_000u128)
        .map(|i| join(i / 10_000 + 3, ((i * 7919) % 10_000) as u16))
        .collect();
    let mut rb2 = build(&(0..5).collect());
    let mut model2: Model = (0..5).collect();
    rb2.extend(dense.iter().rev().copied());
    model2.extend(dense.iter().copied());
    check(&rb2, &model2);

    // A few values into a big index: point updates.
    rb.extend([5u128, u128::MAX, 3, join(77, 1)]);
    model.extend([5u128, u128::MAX, 3, join(77, 1)]);
    check(&rb, &model);
}

#[cfg(feature = "std")]
#[test]
fn extend_is_panic_safe() {
    let mut rb: RoaringBitmap128 = [1u128, 2, 3].into();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rb.extend((4..100u128).map(|v| if v == 50 { panic!("boom") } else { v * K }));
    }));
    assert!(result.is_err());
    rb.internal_validate().unwrap();
    assert_eq!(rb.len(), 3 + 46);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rb.append((100..200u128).map(|v| if v == 150 { panic!("boom") } else { v * K }))
    }));
    assert!(result.is_err());
    rb.internal_validate().unwrap();
    assert_eq!(rb.len(), 3 + 46 + 50);
}

// ---------------------------------------------------------------------------------------------
// Set operations: every strategy against the model
// ---------------------------------------------------------------------------------------------

fn sparse(keys: impl Iterator<Item = u128>, seed: u128) -> Model {
    keys.flat_map(|k| {
        let n = (k * 31 + seed) % 5;
        (0..n).map(move |i| join(k, ((k * 7 + i * 13 + seed) % 64) as u16))
    })
    .collect()
}

fn dense(keys: impl Iterator<Item = u128>, seed: u128) -> Model {
    keys.flat_map(|k| {
        let start = ((k + seed) % 3) as u16 * 1000;
        (start..start + 5000).map(move |i| join(k, i))
    })
    .collect()
}

#[track_caller]
fn check_all_ops(a: &Model, b: &Model) {
    let ra = build(a);
    let rb = build(b);
    let or: Model = a.union(b).copied().collect();
    let and: Model = a.intersection(b).copied().collect();
    let sub: Model = a.difference(b).copied().collect();
    let xor: Model = a.symmetric_difference(b).copied().collect();

    check(&(&ra | &rb), &or);
    check(&(ra.clone() | &rb), &or);
    check(&(&ra | rb.clone()), &or);
    check(&(ra.clone() | rb.clone()), &or);
    check(&(&ra & &rb), &and);
    check(&(ra.clone() & &rb), &and);
    check(&(&ra & rb.clone()), &and);
    check(&(ra.clone() & rb.clone()), &and);
    check(&(&ra - &rb), &sub);
    check(&(ra.clone() - &rb), &sub);
    check(&(&ra - rb.clone()), &sub);
    check(&(ra.clone() - rb.clone()), &sub);
    check(&(&ra ^ &rb), &xor);
    check(&(ra.clone() ^ &rb), &xor);
    check(&(&ra ^ rb.clone()), &xor);
    check(&(ra.clone() ^ rb.clone()), &xor);

    assert_eq!(ra.union_len(&rb), or.len() as u64);
    assert_eq!(ra.intersection_len(&rb), and.len() as u64);
    assert_eq!(ra.difference_len(&rb), sub.len() as u64);
    assert_eq!(ra.symmetric_difference_len(&rb), xor.len() as u64);
    assert_eq!(ra.is_disjoint(&rb), and.is_empty());
    assert_eq!(ra.is_subset(&rb), a.is_subset(b));
    assert_eq!(ra.is_superset(&rb), a.is_superset(b));
    assert_eq!(rb.is_subset(&ra), b.is_subset(a));
    assert_eq!(ra == rb, a == b);
}

#[test]
fn set_ops_balanced_and_skewed() {
    let few = |seed| sparse([3u128, 700, 701, 5000].into_iter(), seed);
    let many = |seed| sparse(0..2000u128, seed);
    let many_shifted = |seed| sparse(1000..3000u128, seed);
    let cases = [
        (many(1), many(2)),
        (many(1), many_shifted(2)),
        (few(1), many(2)),
        (many(1), few(2)),
        (few(1), few(1)),
        (many(3), many(3)),
        (Model::new(), many(1)),
        (many(1), Model::new()),
        (dense(0..20u128, 1), dense(10..30u128, 2)),
        (dense(0..3u128, 1), sparse(0..1000u128, 4)),
        (sparse(0..1000u128, 4), dense(0..3u128, 1)),
        // Container count ratio between UPDATE_SKEW_RATIO and SKEW_RATIO.
        (sparse(0..150u128, 6), many(7)),
        (many(7), sparse(1900..2050u128, 6)),
    ];
    for (a, b) in &cases {
        check_all_ops(a, b);
    }
    // Subset through both strategies.
    let big = many(5);
    let small: Model = big.iter().copied().step_by(97).collect();
    check_all_ops(&small, &big);
    let half: Model = big.iter().copied().step_by(2).collect();
    check_all_ops(&half, &big);
}

#[test]
fn multiops_strategies() {
    let models = [
        sparse(0..500u128, 1),
        sparse(250..750u128, 2),
        dense(0..4u128, 3),
        sparse(0..40u128, 4),
        Model::new(),
        sparse(100..120u128, 5),
    ];
    let bitmaps: Vec<RoaringBitmap128> = models.iter().map(build).collect();
    let check_multi = |subset: &[usize]| {
        let ms: Vec<&Model> = subset.iter().map(|&i| &models[i]).collect();
        let bs: Vec<&RoaringBitmap128> = subset.iter().map(|&i| &bitmaps[i]).collect();
        let union: Model = ms.iter().flat_map(|m| m.iter().copied()).collect();
        let mut xor = Model::new();
        for m in &ms {
            xor = xor.symmetric_difference(m).copied().collect();
        }
        let intersection: Model = match ms.split_first() {
            None => Model::new(),
            Some((first, rest)) => first
                .iter()
                .copied()
                .filter(|v| rest.iter().all(|m| m.contains(v)))
                .collect(),
        };
        let difference: Model = match ms.split_first() {
            None => Model::new(),
            Some((first, rest)) => first
                .iter()
                .copied()
                .filter(|v| rest.iter().all(|m| !m.contains(v)))
                .collect(),
        };
        let owned = || bs.iter().map(|b| (*b).clone()).collect::<Vec<_>>();
        check(&bs.iter().copied().union(), &union);
        check(&owned().union(), &union);
        check(&bs.iter().copied().symmetric_difference(), &xor);
        check(&owned().symmetric_difference(), &xor);
        check(&bs.iter().copied().intersection(), &intersection);
        check(&owned().intersection(), &intersection);
        check(&bs.iter().copied().difference(), &difference);
        check(&owned().difference(), &difference);
        let ok_ref = |b| Ok::<_, ()>(b);
        let ok = |b| Ok::<_, ()>(b);
        check(&bs.iter().copied().map(ok_ref).union().unwrap(), &union);
        check(
            &owned().into_iter().map(ok).intersection().unwrap(),
            &intersection,
        );
        check(
            &bs.iter().copied().map(ok_ref).difference().unwrap(),
            &difference,
        );
        check(
            &owned().into_iter().map(ok).symmetric_difference().unwrap(),
            &xor,
        );
    };
    check_multi(&[]);
    check_multi(&[0]);
    check_multi(&[0, 1]);
    check_multi(&[0, 1, 2, 3]);
    check_multi(&[0, 3, 5]);
    check_multi(&[3, 5, 0]);
    check_multi(&[0, 1, 2, 3, 4, 5]);
    check_multi(&[4, 0]);
    check_multi(&[0, 0, 0]);

    // Errors are propagated.
    let with_error = [Ok(&bitmaps[0]), Err(7), Ok(&bitmaps[1])];
    assert_eq!(with_error.into_iter().union(), Err(7));
    let with_error = [Ok(bitmaps[0].clone()), Err(7)];
    assert_eq!(with_error.into_iter().symmetric_difference(), Err(7));
    let with_error = [Ok(&bitmaps[0]), Err(7)];
    assert_eq!(with_error.into_iter().intersection(), Err(7));
}

// ---------------------------------------------------------------------------------------------
// Iterators: seeks, ranges and size-hint bookkeeping against a model cursor
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum IterOp {
    Next,
    NextBack,
    AdvanceTo(u128),
    AdvanceBackTo(u128),
    NextRange,
    NextRangeBack,
    Nth(usize),
    NthBack(usize),
}

fn iter_op() -> impl Strategy<Value = IterOp> {
    prop_oneof![
        3 => Just(IterOp::Next),
        3 => Just(IterOp::NextBack),
        2 => value().prop_map(IterOp::AdvanceTo),
        2 => value().prop_map(IterOp::AdvanceBackTo),
        1 => Just(IterOp::NextRange),
        1 => Just(IterOp::NextRangeBack),
        1 => (0usize..70_000).prop_map(IterOp::Nth),
        1 => (0usize..70_000).prop_map(IterOp::NthBack),
    ]
}

/// The remaining values of an iterator: `values[lo..hi]`.
struct Cursor<'a> {
    values: &'a [u128],
    lo: usize,
    hi: usize,
}

impl Cursor<'_> {
    fn apply(&mut self, op: &IterOp) -> Option<RangeInclusive<u128>> {
        let single = |v: u128| v..=v;
        match *op {
            IterOp::Next => (self.lo < self.hi).then(|| {
                self.lo += 1;
                single(self.values[self.lo - 1])
            }),
            IterOp::NextBack => (self.lo < self.hi).then(|| {
                self.hi -= 1;
                single(self.values[self.hi])
            }),
            IterOp::AdvanceTo(v) => {
                let p = self.values.partition_point(|&x| x < v);
                self.lo = self.lo.max(p).min(self.hi);
                None
            }
            IterOp::AdvanceBackTo(v) => {
                let p = self.values.partition_point(|&x| x <= v);
                self.hi = self.hi.min(p).max(self.lo);
                None
            }
            IterOp::NextRange => (self.lo < self.hi).then(|| {
                let start = self.values[self.lo];
                let mut end = start;
                self.lo += 1;
                while self.lo < self.hi && end.checked_add(1) == Some(self.values[self.lo]) {
                    end += 1;
                    self.lo += 1;
                }
                start..=end
            }),
            IterOp::NextRangeBack => (self.lo < self.hi).then(|| {
                let end = self.values[self.hi - 1];
                let mut start = end;
                self.hi -= 1;
                while self.lo < self.hi && start.checked_sub(1) == Some(self.values[self.hi - 1]) {
                    start -= 1;
                    self.hi -= 1;
                }
                start..=end
            }),
            IterOp::Nth(n) => {
                if self.lo + n < self.hi {
                    self.lo += n + 1;
                    Some(single(self.values[self.lo - 1]))
                } else {
                    self.lo = self.hi;
                    None
                }
            }
            IterOp::NthBack(n) => {
                if self.lo + n < self.hi {
                    self.hi -= n + 1;
                    Some(single(self.values[self.hi]))
                } else {
                    self.hi = self.lo;
                    None
                }
            }
        }
    }
}

macro_rules! run_iter_ops {
    ($iter:expr, $values:expr, $ops:expr) => {{
        let mut iter = $iter;
        let mut cursor = Cursor {
            values: $values,
            lo: 0,
            hi: $values.len(),
        };
        for op in $ops {
            let expected = cursor.apply(op);
            let got = match *op {
                IterOp::Next => iter.next().map(|v| v..=v),
                IterOp::NextBack => iter.next_back().map(|v| v..=v),
                IterOp::AdvanceTo(v) => {
                    iter.advance_to(v);
                    None
                }
                IterOp::AdvanceBackTo(v) => {
                    iter.advance_back_to(v);
                    None
                }
                IterOp::NextRange => iter.next_range(),
                IterOp::NextRangeBack => iter.next_range_back(),
                IterOp::Nth(n) => iter.nth(n).map(|v| v..=v),
                IterOp::NthBack(n) => iter.nth_back(n).map(|v| v..=v),
            };
            prop_assert_eq!(got, expected, "{:?}", op);
            let remaining = cursor.hi - cursor.lo;
            prop_assert_eq!(iter.size_hint(), (remaining, Some(remaining)), "{:?}", op);
        }
        let rest = &cursor.values[cursor.lo..cursor.hi];
        prop_assert_eq!(iter.clone().count(), rest.len());
        prop_assert!(iter.clone().eq(rest.iter().copied()));
        prop_assert!(iter.clone().rev().eq(rest.iter().rev().copied()));
        prop_assert_eq!(iter.clone().fold(0usize, |n, _| n + 1), rest.len());
        prop_assert_eq!(iter.clone().last(), rest.last().copied());
        let reversed = iter.rfold(Vec::new(), |mut acc, x| {
            acc.push(x);
            acc
        });
        prop_assert!(reversed.into_iter().eq(rest.iter().rev().copied()));
    }};
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn iterators_follow_the_model(
        model in model(),
        ops in proptest::collection::vec(iter_op(), 0..12),
    ) {
        let rb = build(&model);
        let values: Vec<u128> = model.iter().copied().collect();
        run_iter_ops!(rb.iter(), &values[..], &ops);
        run_iter_ops!(rb.clone().into_iter(), &values[..], &ops);
    }

    #[test]
    fn range_iterators(model in model(), range in range_strategy(), ops in proptest::collection::vec(iter_op(), 0..6)) {
        prop_assume!(valid_iter_range(&range));
        let rb = build(&model);
        let values = model_range(&model, &range);
        run_iter_ops!(rb.range(range), &values[..], &ops);
        run_iter_ops!(rb.clone().into_range(range), &values[..], &ops);
        prop_assert_eq!(rb.range_cardinality(range), values.len() as u64);
        let full = values.first().is_some_and(|&first| {
            values.len() as u128 - 1 == values.last().unwrap() - first
        });
        if let (Bound::Included(s), Bound::Included(e)) = range {
            prop_assert_eq!(
                rb.contains_range(s..=e),
                full && values.first() == Some(&s) && values.last() == Some(&e)
            );
        }
    }

    #[test]
    fn point_and_range_mutations(
        model in model(),
        ops in proptest::collection::vec((0u8..6, value(), 0u128..140_000), 0..16),
    ) {
        let mut model = model;
        let mut rb = build(&model);
        for (op, v, n) in ops {
            let end = v.saturating_add(n);
            match op {
                0 => prop_assert_eq!(rb.insert(v), model.insert(v)),
                1 => prop_assert_eq!(rb.remove(v), model.remove(&v)),
                2 => {
                    let before = model.len();
                    model.extend(v..=end);
                    prop_assert_eq!(rb.insert_range(v..=end), (model.len() - before) as u64);
                }
                3 => {
                    let before = model.len();
                    model.retain(|x| !(v..=end).contains(x));
                    prop_assert_eq!(rb.remove_range(v..=end), (before - model.len()) as u64);
                }
                4 => {
                    prop_assert_eq!(rb.rank(v), model.range(..=v).count() as u64);
                    let n = n as u64;
                    prop_assert_eq!(rb.select(n), model.iter().nth(n as usize).copied());
                    prop_assert_eq!(rb.contains(v), model.contains(&v));
                }
                _ => {
                    let pushed = rb.try_push(v).is_ok();
                    let expected = model.last().is_none_or(|&m| v > m);
                    prop_assert_eq!(pushed, expected);
                    if expected {
                        model.insert(v);
                    }
                }
            }
            check(&rb, &model);
        }
        let changed = rb.optimize();
        check(&rb, &model);
        let _ = changed;
        rb.remove_run_compression();
        check(&rb, &model);
    }

    #[test]
    fn set_ops_follow_the_model(a in model(), b in model()) {
        check_all_ops(&a, &b);
    }
}
