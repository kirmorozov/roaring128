//! Range operations: `insert_range`, `remove_range`, `contains_range`, `range_cardinality`,
//! `range`, `into_range`, with every kind of bounds (inclusive, exclusive, unbounded, empty,
//! reversed, equal and excluded, the full `u128` range).

use std::panic::{AssertUnwindSafe, catch_unwind};

use proptest::collection::vec;
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

/// Whether every value of the range is in the model (`true` for empty ranges).
fn model_contains_range(r: &RangeSpec, model: &Model) -> bool {
    match r.width() {
        Some(0) => true,
        Some(w) => r.model_count(model) as u128 == w,
        None => false, // 2^128 values
    }
}

proptest! {
    #![proptest_config(config(128))]

    #[test]
    fn insert_range_matches_model(spec in set_spec(), ranges in vec(insertable_range(), 1..4)) {
        let (mut rb, mut model) = spec.build();
        for r in ranges {
            let (expected, width) = match r.bounds() {
                None => (0, 0),
                Some((lo, hi)) => ((hi - lo + 1) as u64 - r.model_count(&model), hi - lo + 1),
            };
            prop_assert_eq!(rb.insert_range(r), expected, "insert_range({:x?})", r);
            if let Some((lo, hi)) = r.bounds() {
                model.extend(lo..=hi);
                prop_assert_eq!(width as u64, r.model_count(&model));
            }
            assert_matches(&rb, &model);
            prop_assert!(rb.contains_range(r));
            prop_assert_eq!(rb.range_cardinality(r), r.model_count(&model));
        }
    }

    #[test]
    fn remove_range_matches_model(spec in set_spec(), ranges in vec(any_range(), 1..4)) {
        let (mut rb, mut model) = spec.build();
        for r in ranges {
            let gone: Vec<u128> = r.model_values(&model).collect();
            prop_assert_eq!(rb.remove_range(r), gone.len() as u64, "remove_range({:x?})", r);
            for v in gone {
                model.remove(&v);
            }
            assert_matches(&rb, &model);
            prop_assert_eq!(rb.range_cardinality(r), 0);
            prop_assert_eq!(rb.contains_range(r), r.width() == Some(0));
            prop_assert_eq!(rb.statistics().n_containers, n_keys(&model));
        }
    }

    #[test]
    fn contains_range_and_cardinality_match_model(spec in set_spec(), ranges in vec(any_range(), 1..16)) {
        let (rb, model) = spec.build();
        // also ranges built from the content: maximal runs and their neighbourhoods
        let sorted = sorted(&model);
        let mut all = ranges;
        for r in intervals(&sorted).into_iter().take(16) {
            let (lo, hi) = (*r.start(), *r.end());
            all.push(RangeSpec(std::ops::Bound::Included(lo), std::ops::Bound::Included(hi)));
            all.push(RangeSpec(std::ops::Bound::Included(lo.saturating_sub(1)), std::ops::Bound::Included(hi)));
            all.push(RangeSpec(std::ops::Bound::Included(lo), std::ops::Bound::Excluded(hi)));
            all.push(RangeSpec(std::ops::Bound::Excluded(lo), std::ops::Bound::Included(hi.saturating_add(1))));
            all.push(RangeSpec(std::ops::Bound::Unbounded, std::ops::Bound::Included(hi)));
            all.push(RangeSpec(std::ops::Bound::Included(lo), std::ops::Bound::Unbounded));
        }
        for r in all {
            prop_assert_eq!(rb.contains_range(r), model_contains_range(&r, &model), "contains_range({:x?})", r);
            prop_assert_eq!(rb.range_cardinality(r), r.model_count(&model), "range_cardinality({:x?})", r);
        }
    }

    #[test]
    fn range_iter_matches_model(spec in set_spec(), ranges in vec(any_range(), 1..8)) {
        let (rb, model) = spec.build();
        for r in ranges {
            if r.panics_as_iter_range() {
                let res = catch_unwind(AssertUnwindSafe(|| rb.range(r).count()));
                prop_assert!(res.is_err(), "range({:x?}) must panic", r);
                let owned = rb.clone();
                let res = catch_unwind(AssertUnwindSafe(move || owned.into_range(r).count()));
                prop_assert!(res.is_err(), "into_range({:x?}) must panic", r);
                continue;
            }
            let expected: Vec<u128> = r.model_values(&model).collect();
            let it = rb.range(r);
            prop_assert_eq!(it.len(), expected.len(), "range({:x?}).len()", r);
            assert_same_seq(it, expected.iter().copied(), "range");
            assert_same_seq(rb.range(r).rev(), expected.iter().rev().copied(), "range rev");
            assert_same_seq(outside_in(rb.range(r)), outside_in(expected.iter().copied()), "range outside in");
            assert_same_seq(rb.clone().into_range(r), expected.iter().copied(), "into_range");
            assert_same_seq(rb.clone().into_range(r).rev(), expected.iter().rev().copied(), "into_range rev");
        }
    }
}

#[test]
fn full_range_operations() {
    let mut rb: RoaringBitmap128 = [0u128, 1 << 64, u128::MAX].into_iter().collect();
    assert!(!rb.contains_range(..));
    assert_eq!(rb.range_cardinality(..), 3);
    assert!(rb.range(..).eq([0, 1 << 64, u128::MAX]));
    assert_eq!(rb.remove_range(..), 3);
    assert!(rb.is_empty());
    assert_valid(&rb);
}

#[test]
fn reversed_and_empty_ranges_are_noops() {
    use std::ops::Bound::*;
    let mut rb: RoaringBitmap128 = [5u128, 1 << 64, u128::MAX].into_iter().collect();
    let before = rb.clone();
    for r in [
        RangeSpec(Included(10), Included(5)),
        RangeSpec(Included(u128::MAX), Included(0)),
        RangeSpec(Excluded(7), Excluded(7)),
        RangeSpec(Excluded(7), Included(7)),
        RangeSpec(Excluded(7), Excluded(8)),
        RangeSpec(Included(7), Excluded(7)),
        RangeSpec(Excluded(u128::MAX), Unbounded),
        RangeSpec(Unbounded, Excluded(0)),
    ] {
        assert_eq!(rb.insert_range(r), 0, "insert_range({r:?})");
        assert_eq!(rb.remove_range(r), 0, "remove_range({r:?})");
        assert!(rb.contains_range(r), "contains_range({r:?})");
        assert_eq!(rb.range_cardinality(r), 0, "range_cardinality({r:?})");
        assert_eq!(rb, before);
    }
    assert_valid(&rb);
}
