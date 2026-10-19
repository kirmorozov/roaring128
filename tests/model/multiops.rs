//! `MultiOps` over owned bitmaps, references and `Result`s of both, against folds of the model.
//!
//! Error semantics (same as `roaring`): `union` and `symmetric_difference` consume every item
//! and return the first error; `intersection` and `difference` return the first error unless the
//! result is already known to be empty before reaching it (they may short-circuit), so errors
//! are only required when the running result cannot be empty.

use proptest::prelude::*;
use roaring128::{MultiOps, RoaringBitmap128};

use crate::common::*;

fn fold_model(models: &[Model], op: SetOp) -> Model {
    let mut it = models.iter();
    match it.next() {
        None => Model::new(),
        Some(first) => it.fold(first.clone(), |acc, m| model_op(op, &acc, m)),
    }
}

fn check_all(bitmaps: &[RoaringBitmap128], models: &[Model]) {
    let refs: Vec<&RoaringBitmap128> = bitmaps.iter().collect();
    for (op, name) in [
        (SetOp::Or, "union"),
        (SetOp::And, "intersection"),
        (SetOp::Sub, "difference"),
        (SetOp::Xor, "symmetric_difference"),
    ] {
        let expected = fold_model(models, op);
        let run = |owned: Vec<RoaringBitmap128>| match op {
            SetOp::Or => owned.union(),
            SetOp::And => owned.intersection(),
            SetOp::Sub => owned.difference(),
            SetOp::Xor => owned.symmetric_difference(),
        };
        let run_ref = |r: Vec<&RoaringBitmap128>| match op {
            SetOp::Or => r.union(),
            SetOp::And => r.intersection(),
            SetOp::Sub => r.difference(),
            SetOp::Xor => r.symmetric_difference(),
        };
        let run_try = |owned: Vec<RoaringBitmap128>| {
            let it = owned.into_iter().map(Ok::<_, String>);
            match op {
                SetOp::Or => it.union(),
                SetOp::And => it.intersection(),
                SetOp::Sub => it.difference(),
                SetOp::Xor => it.symmetric_difference(),
            }
        };
        let run_try_ref = |r: Vec<&RoaringBitmap128>| {
            let it = r.into_iter().map(Ok::<_, String>);
            match op {
                SetOp::Or => it.union(),
                SetOp::And => it.intersection(),
                SetOp::Sub => it.difference(),
                SetOp::Xor => it.symmetric_difference(),
            }
        };
        let got = run(bitmaps.to_vec());
        assert_matches(&got, &expected);
        let got = run_ref(refs.clone());
        assert_matches(&got, &expected);
        let got =
            run_try(bitmaps.to_vec()).unwrap_or_else(|e| panic!("{name}: unexpected error {e}"));
        assert_matches(&got, &expected);
        let got =
            run_try_ref(refs.clone()).unwrap_or_else(|e| panic!("{name}: unexpected error {e}"));
        assert_matches(&got, &expected);
    }
}

/// The bitmaps as `Ok` items, with errors at positions `k` and `k + 2`.
fn items(bitmaps: &[RoaringBitmap128], k: usize) -> Vec<Result<RoaringBitmap128, usize>> {
    bitmaps
        .iter()
        .enumerate()
        .map(|(i, b)| {
            if i == k || i == k + 2 {
                Err(i)
            } else {
                Ok(b.clone())
            }
        })
        .collect()
}

fn ref_items(bitmaps: &[RoaringBitmap128], k: usize) -> Vec<Result<&RoaringBitmap128, usize>> {
    bitmaps
        .iter()
        .enumerate()
        .map(|(i, b)| if i == k || i == k + 2 { Err(i) } else { Ok(b) })
        .collect()
}

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn multiops_match_model(specs in spec_list(6)) {
        let built: Vec<(RoaringBitmap128, Model)> = specs.iter().map(SetSpec::build).collect();
        let (bitmaps, models): (Vec<_>, Vec<_>) = built.into_iter().unzip();
        check_all(&bitmaps, &models);
    }

    /// More bitmaps than the eager-collection thresholds of roaring's implementation (10 / 50).
    #[test]
    fn multiops_many_small_bitmaps(values in proptest::collection::vec(proptest::collection::vec(value(), 0..8), 0..70)) {
        let bitmaps: Vec<RoaringBitmap128> = values.iter().map(|v| v.iter().copied().collect()).collect();
        let models: Vec<Model> = values.iter().map(|v| v.iter().copied().collect()).collect();
        check_all(&bitmaps, &models);
    }

    #[test]
    fn multiops_errors(specs in spec_list(5), err_at in any::<prop::sample::Index>(), sentinel in value()) {
        prop_assume!(!specs.is_empty());
        let mut bitmaps: Vec<RoaringBitmap128> = specs.iter().map(SetSpec::bitmap).collect();
        let n = bitmaps.len();
        let k = err_at.index(n);
        // union / symmetric difference consume everything: the first error is returned
        prop_assert_eq!(items(&bitmaps, k).union().err(), Some(k));
        prop_assert_eq!(items(&bitmaps, k).symmetric_difference().err(), Some(k));
        prop_assert_eq!(ref_items(&bitmaps, k).union().err(), Some(k));
        prop_assert_eq!(ref_items(&bitmaps, k).symmetric_difference().err(), Some(k));

        // intersection: make every bitmap contain the sentinel so the result is never empty
        for b in &mut bitmaps {
            b.insert(sentinel);
        }
        prop_assert_eq!(items(&bitmaps, k).intersection().err(), Some(k));
        prop_assert_eq!(ref_items(&bitmaps, k).intersection().err(), Some(k));

        // difference: only the first bitmap contains the sentinel
        for b in bitmaps.iter_mut().skip(1) {
            b.remove(sentinel);
        }
        prop_assert_eq!(items(&bitmaps, k).difference().err(), Some(k));
        prop_assert_eq!(ref_items(&bitmaps, k).difference().err(), Some(k));
    }
}

#[test]
fn multiops_single_and_empty() {
    let a: RoaringBitmap128 = [0u128, 1 << 64, u128::MAX].into_iter().collect();
    for got in [
        [a.clone()].union(),
        [a.clone()].intersection(),
        [a.clone()].difference(),
        [a.clone()].symmetric_difference(),
        [&a].union(),
        [&a].intersection(),
        [&a].difference(),
        [&a].symmetric_difference(),
    ] {
        assert_valid(&got);
        assert_eq!(got, a);
    }
    let none: Vec<Result<RoaringBitmap128, ()>> = Vec::new();
    assert_eq!(none.clone().union(), Ok(RoaringBitmap128::new()));
    assert_eq!(none.clone().intersection(), Ok(RoaringBitmap128::new()));
    assert_eq!(none.clone().difference(), Ok(RoaringBitmap128::new()));
    assert_eq!(none.symmetric_difference(), Ok(RoaringBitmap128::new()));
    let empty_inside = [a.clone(), RoaringBitmap128::new(), a.clone()];
    assert!(empty_inside.clone().intersection().is_empty());
    assert_eq!(empty_inside.clone().union(), a);
    assert_eq!(empty_inside.clone().difference(), RoaringBitmap128::new());
    assert_eq!(empty_inside.symmetric_difference(), RoaringBitmap128::new());
}
