//! Iterators: `iter`, `into_iter`, `range`, `into_range` driven by random sequences of `next`,
//! `next_back`, `nth`, `nth_back`, `advance_to`, `advance_back_to`, `next_range`,
//! `next_range_back` and clones, checked against a model cursor with exact `size_hint` / `len`
//! after every step; plus `count`, `fold`, `rfold`, `last`, interleaved front/back iteration.

use proptest::collection::vec;
use proptest::prelude::*;

use crate::common::*;

proptest! {
    #![proptest_config(config(128))]

    #[test]
    fn iter_ops_match_model(spec in set_spec(), ops in vec(iter_op(), 0..40)) {
        let (rb, model) = spec.build();
        let all = sorted(&model);
        check_iter_ops(rb.iter(), &all, &all, &ops);
        check_iter_ops((&rb).into_iter(), &all, &all, &ops);
    }

    #[test]
    fn into_iter_ops_match_model(spec in set_spec(), ops in vec(iter_op(), 0..40)) {
        let (rb, model) = spec.build();
        let all = sorted(&model);
        check_iter_ops(rb.into_iter(), &all, &all, &ops);
    }

    #[test]
    fn range_iter_ops_match_model(spec in set_spec(), r in any_range(), ops in vec(iter_op(), 0..40)) {
        prop_assume!(!r.panics_as_iter_range());
        let (rb, model) = spec.build();
        let all = sorted(&model);
        let expected: Vec<u128> = r.model_values(&model).collect();
        check_iter_ops(rb.range(r), &expected, &all, &ops);
        check_iter_ops(rb.into_range(r), &expected, &all, &ops);
    }

    #[test]
    fn whole_iteration_helpers(spec in set_spec(), skip in 0..20_000usize) {
        let (rb, model) = spec.build();
        let all = sorted(&model);
        prop_assert_eq!(rb.iter().count(), all.len());
        prop_assert_eq!(rb.clone().into_iter().count(), all.len());
        // `last` is part of the iterator API under test
        #[allow(clippy::double_ended_iterator_last)]
        let (last, last_owned) = (rb.iter().last(), rb.clone().into_iter().last());
        prop_assert_eq!(last, all.last().copied());
        prop_assert_eq!(last_owned, all.last().copied());
        let folded = rb.iter().fold(Vec::with_capacity(all.len()), |mut acc, v| { acc.push(v); acc });
        prop_assert!(folded == all);
        let rfolded = rb.iter().rfold(Vec::with_capacity(all.len()), |mut acc, v| { acc.push(v); acc });
        prop_assert!(rfolded.iter().rev().eq(all.iter()));
        let folded = rb.clone().into_iter().fold(0usize, |n, v| { assert_eq!(v, all[n]); n + 1 });
        prop_assert_eq!(folded, all.len());
        assert_same_seq(outside_in(&rb), outside_in(all.iter().copied()), "outside in");
        assert_same_seq(outside_in(rb.clone()), outside_in(all.iter().copied()), "outside in (owned)");

        // skip/nth based helpers
        let mut it = rb.iter();
        if let Some(n) = skip.checked_sub(1) {
            prop_assert_eq!(it.nth(n), all.get(n).copied());
        }
        let rest = all.len().saturating_sub(skip);
        prop_assert_eq!(it.size_hint(), (rest, Some(rest)));
        prop_assert_eq!(it.clone().count(), rest);
        let mut it = rb.iter();
        if let Some(n) = skip.checked_sub(1) {
            prop_assert_eq!(it.nth_back(n), all.len().checked_sub(skip).map(|i| all[i]));
        }
        prop_assert_eq!(it.len(), rest);
        assert_same_seq(it, all.iter().copied().take(rest), "after nth_back");
        // Iterator::skip / step_by / by_ref
        assert_same_seq(rb.iter().skip(skip), all.iter().copied().skip(skip), "skip");
        assert_same_seq(rb.iter().rev().skip(skip), all.iter().rev().copied().skip(skip), "rev skip");
        assert_same_seq(rb.iter().step_by(skip % 97 + 1), all.iter().copied().step_by(skip % 97 + 1), "step_by");
    }

    /// `next_range` over the whole bitmap returns the maximal intervals of the set (in both
    /// directions), whatever the container boundaries and representations.
    #[test]
    fn next_range_enumerates_intervals(spec in set_spec()) {
        let (rb, model) = spec.build();
        let expected = intervals(&sorted(&model));
        let mut it = rb.iter();
        let mut got = Vec::new();
        while let Some(r) = it.next_range() {
            got.push(r);
        }
        prop_assert_eq!(&got, &expected);
        let mut it = rb.clone().into_iter();
        let mut got = Vec::new();
        while let Some(r) = it.next_range_back() {
            got.push(r);
        }
        got.reverse();
        prop_assert_eq!(&got, &expected);
    }
}
