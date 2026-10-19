//! Binary set operations in all owned / borrowed / assign forms, the `*_len` helpers,
//! `is_disjoint`, `is_subset`, `is_superset` and `==`.

use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

proptest! {
    #![proptest_config(config(96))]

    #[test]
    fn binary_ops_match_model((a, b) in spec_pair()) {
        let (ra, ma) = a.build();
        let (rb, mb) = b.build();
        for op in SET_OPS {
            let expected = model_op(op, &ma, &mb);
            for form in FORMS {
                let got = apply_op(op, form, &ra, &rb);
                assert_matches(&got, &expected);
            }
        }
        // the operands are untouched by the borrowed forms
        assert_matches(&ra, &ma);
        assert_matches(&rb, &mb);
    }

    #[test]
    fn len_helpers_match_model((a, b) in spec_pair()) {
        let (ra, ma) = a.build();
        let (rb, mb) = b.build();
        prop_assert_eq!(ra.union_len(&rb), (&ma | &mb).len() as u64);
        prop_assert_eq!(ra.intersection_len(&rb), (&ma & &mb).len() as u64);
        prop_assert_eq!(ra.difference_len(&rb), (&ma - &mb).len() as u64);
        prop_assert_eq!(ra.symmetric_difference_len(&rb), (&ma ^ &mb).len() as u64);
        prop_assert_eq!(rb.difference_len(&ra), (&mb - &ma).len() as u64);
    }

    #[test]
    fn comparisons_match_model((a, b) in spec_pair()) {
        let (ra, ma) = a.build();
        let (rb, mb) = b.build();
        prop_assert_eq!(ra.is_disjoint(&rb), ma.is_disjoint(&mb));
        prop_assert_eq!(rb.is_disjoint(&ra), ma.is_disjoint(&mb));
        prop_assert_eq!(ra.is_subset(&rb), ma.is_subset(&mb));
        prop_assert_eq!(rb.is_subset(&ra), mb.is_subset(&ma));
        prop_assert_eq!(ra.is_superset(&rb), ma.is_superset(&mb));
        prop_assert_eq!(rb.is_superset(&ra), mb.is_superset(&ma));
        prop_assert_eq!(ra == rb, ma == mb);

        // constructed relations
        let union = &ra | &rb;
        prop_assert!(ra.is_subset(&union) && rb.is_subset(&union));
        prop_assert!(union.is_superset(&ra));
        let inter = &ra & &rb;
        prop_assert!(inter.is_subset(&ra) && inter.is_subset(&rb));
        let diff = &ra - &rb;
        prop_assert!(diff.is_disjoint(&rb));
        prop_assert_eq!(diff.is_subset(&rb), diff.is_empty());
        // a strict subset with one value less
        if let Some(m) = ra.max() {
            let mut smaller = ra.clone();
            smaller.remove(m);
            prop_assert!(smaller.is_subset(&ra));
            prop_assert!(!ra.is_subset(&smaller));
            prop_assert!(ra.is_superset(&smaller));
            prop_assert!(!smaller.is_disjoint(&ra) || smaller.is_empty());
        }
    }

    /// Operations with the empty set and with itself, in every form.
    #[test]
    fn ops_with_empty_and_self(a in set_spec()) {
        let (ra, ma) = a.build();
        let empty = RoaringBitmap128::new();
        let me = Model::new();
        for op in SET_OPS {
            for form in FORMS {
                assert_matches(&apply_op(op, form, &ra, &empty), &model_op(op, &ma, &me));
                assert_matches(&apply_op(op, form, &empty, &ra), &model_op(op, &me, &ma));
                assert_matches(&apply_op(op, form, &ra, &ra), &model_op(op, &ma, &ma));
            }
        }
        prop_assert!(ra.is_disjoint(&empty));
        prop_assert!(empty.is_subset(&ra));
        prop_assert_eq!(ra.is_disjoint(&ra), ra.is_empty());
    }
}
