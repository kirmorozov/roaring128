//! Port of the proptests of `src/bitmap/ops.rs` and `src/treemap/ops.rs` (identical, the
//! bitmap version adds the `Result` MultiOps variants): fast cardinality computations and the
//! equivalence of every way of writing a set operation.
//!
//! `RoaringBitmap::arbitrary()` is replaced by structured `SetSpec`s (see `common`).

use core::convert::Infallible;

use proptest::prelude::*;
use roaring128::{MultiOps, RoaringBitmap128};

use crate::common::*;

proptest! {
    #![proptest_config(config(64))]

    // fast count tests
    #[test]
    fn union_len_eq_len_of_materialized_union((a, b) in spec_pair()) {
        let (a, b) = (a.bitmap(), b.bitmap());
        prop_assert_eq!(a.union_len(&b), (a | b).len());
    }

    #[test]
    fn intersection_len_eq_len_of_materialized_intersection((a, b) in spec_pair()) {
        let (a, b) = (a.bitmap(), b.bitmap());
        prop_assert_eq!(a.intersection_len(&b), (a & b).len());
    }

    #[test]
    fn difference_len_eq_len_of_materialized_difference((a, b) in spec_pair()) {
        let (a, b) = (a.bitmap(), b.bitmap());
        prop_assert_eq!(a.difference_len(&b), (a - b).len());
    }

    #[test]
    fn symmetric_difference_len_eq_len_of_materialized_symmetric_difference((a, b) in spec_pair()) {
        let (a, b) = (a.bitmap(), b.bitmap());
        prop_assert_eq!(a.symmetric_difference_len(&b), (a ^ b).len());
    }

    #[test]
    fn all_union_give_the_same_result((a, b, c) in spec_triple()) {
        let (a, b, c) = (a.bitmap(), b.bitmap(), c.bitmap());
        let mut ref_assign = a.clone();
        ref_assign |= &b;
        ref_assign |= &c;

        let mut own_assign = a.clone();
        own_assign |= b.clone();
        own_assign |= c.clone();

        let ref_inline = &a | &b | &c;
        let own_inline = a.clone() | b.clone() | c.clone();

        let ref_multiop = [&a, &b, &c].union();
        let own_multiop = [a.clone(), b.clone(), c.clone()].union();

        let ref_multiop_try = [&a, &b, &c].map(Ok::<_, Infallible>).union().unwrap();
        let own_multiop_try = [a, b, c].map(Ok::<_, Infallible>).union().unwrap();

        assert_valid(&ref_assign);
        for roar in &[
            own_assign,
            ref_inline,
            own_inline,
            ref_multiop,
            own_multiop,
            ref_multiop_try,
            own_multiop_try,
        ] {
            assert_valid(roar);
            prop_assert_eq!(&ref_assign, roar);
        }
    }

    #[test]
    fn all_intersection_give_the_same_result((a, b, c) in spec_triple()) {
        let (a, b, c) = (a.bitmap(), b.bitmap(), c.bitmap());
        let mut ref_assign = a.clone();
        ref_assign &= &b;
        ref_assign &= &c;

        let mut own_assign = a.clone();
        own_assign &= b.clone();
        own_assign &= c.clone();

        let ref_inline = &a & &b & &c;
        let own_inline = a.clone() & b.clone() & c.clone();

        let ref_multiop = [&a, &b, &c].intersection();
        let own_multiop = [a.clone(), b.clone(), c.clone()].intersection();

        let ref_multiop_try = [&a, &b, &c].map(Ok::<_, Infallible>).intersection().unwrap();
        let own_multiop_try = [a, b, c].map(Ok::<_, Infallible>).intersection().unwrap();

        assert_valid(&ref_assign);
        for roar in &[
            own_assign,
            ref_inline,
            own_inline,
            ref_multiop,
            own_multiop,
            ref_multiop_try,
            own_multiop_try,
        ] {
            assert_valid(roar);
            prop_assert_eq!(&ref_assign, roar);
        }
    }

    #[test]
    fn all_difference_give_the_same_result((a, b, c) in spec_triple()) {
        let (a, b, c) = (a.bitmap(), b.bitmap(), c.bitmap());
        let mut ref_assign = a.clone();
        ref_assign -= &b;
        ref_assign -= &c;

        let mut own_assign = a.clone();
        own_assign -= b.clone();
        own_assign -= c.clone();

        let ref_inline = &a - &b - &c;
        let own_inline = a.clone() - b.clone() - c.clone();

        let ref_multiop = [&a, &b, &c].difference();
        let own_multiop = [a.clone(), b.clone(), c.clone()].difference();

        let ref_multiop_try = [&a, &b, &c].map(Ok::<_, Infallible>).difference().unwrap();
        let own_multiop_try = [a, b, c].map(Ok::<_, Infallible>).difference().unwrap();

        assert_valid(&ref_assign);
        for roar in &[
            own_assign,
            ref_inline,
            own_inline,
            ref_multiop,
            own_multiop,
            ref_multiop_try,
            own_multiop_try,
        ] {
            assert_valid(roar);
            prop_assert_eq!(&ref_assign, roar);
        }
    }

    #[test]
    fn all_symmetric_difference_give_the_same_result((a, b, c) in spec_triple()) {
        let (a, b, c) = (a.bitmap(), b.bitmap(), c.bitmap());
        let mut ref_assign = a.clone();
        ref_assign ^= &b;
        ref_assign ^= &c;

        let mut own_assign = a.clone();
        own_assign ^= b.clone();
        own_assign ^= c.clone();

        let ref_inline = &a ^ &b ^ &c;
        let own_inline = a.clone() ^ b.clone() ^ c.clone();

        let ref_multiop = [&a, &b, &c].symmetric_difference();
        let own_multiop = [a.clone(), b.clone(), c.clone()].symmetric_difference();

        let ref_multiop_try = [&a, &b, &c]
            .map(Ok::<_, Infallible>)
            .symmetric_difference()
            .unwrap();
        let own_multiop_try = [a, b, c]
            .map(Ok::<_, Infallible>)
            .symmetric_difference()
            .unwrap();

        assert_valid(&ref_assign);
        for roar in &[
            own_assign,
            ref_inline,
            own_inline,
            ref_multiop,
            own_multiop,
            ref_multiop_try,
            own_multiop_try,
        ] {
            assert_valid(roar);
            prop_assert_eq!(&ref_assign, roar);
        }
    }
}

#[test]
fn multiops_on_empty_input() {
    let none: [RoaringBitmap128; 0] = [];
    assert!(none.clone().union().is_empty());
    assert!(none.clone().intersection().is_empty());
    assert!(none.clone().difference().is_empty());
    assert!(none.symmetric_difference().is_empty());
    let none: Vec<&RoaringBitmap128> = Vec::new();
    assert!(none.clone().union().is_empty());
    assert!(none.clone().intersection().is_empty());
    assert!(none.clone().difference().is_empty());
    assert!(none.symmetric_difference().is_empty());
}
