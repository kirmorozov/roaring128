//! Construction and conversions: `insert`, `extend`, `insert_range`, `append`, `try_push`
//! (inside `SetSpec::build`), `FromIterator` / `Extend` (owned and borrowed), `From<[u128; N]>`,
//! `from_sorted_iter`, `Clone`, `clone_from`, `Eq`, `Default`, `Debug`, `IntoIterator`.

use proptest::collection::vec;
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

proptest! {
    #![proptest_config(config(128))]

    #[test]
    fn build_matches_model(spec in set_spec()) {
        let (rb, model) = spec.build();
        assert_matches(&rb, &model);
    }

    #[test]
    fn collect_and_extend_match_model(spec in set_spec(), extra in vec(value(), 0..64)) {
        let model = spec.model();
        // unsorted input with duplicates
        let mut values: Vec<u128> = model.iter().copied().rev().collect();
        values.extend(extra.iter().copied());
        values.extend(extra.iter().copied());
        let mut full = model.clone();
        full.extend(extra.iter().copied());

        let owned: RoaringBitmap128 = values.iter().copied().collect();
        assert_matches(&owned, &full);
        let borrowed: RoaringBitmap128 = values.iter().collect();
        assert_matches(&borrowed, &full);

        let mut ext = RoaringBitmap128::new();
        ext.extend(model.iter());
        ext.extend(extra.iter().copied());
        assert_matches(&ext, &full);

        let sorted = RoaringBitmap128::from_sorted_iter(full.iter().copied()).unwrap();
        assert_matches(&sorted, &full);
        prop_assert_eq!(&sorted, &owned);

        // IntoIterator for & and owned
        let mut n = 0usize;
        for v in &sorted {
            prop_assert!(full.contains(&v));
            n += 1;
        }
        prop_assert_eq!(n, full.len());
        assert_same_seq(sorted, full.iter().copied(), "into_iter");
    }

    #[test]
    fn from_sorted_iter_reports_the_valid_prefix(
        spec in set_spec_with(2),
        bad_at in any::<prop::sample::Index>(),
        bad_kind in 0..3u8,
    ) {
        let sorted: Vec<u128> = spec.model().into_iter().collect();
        prop_assume!(!sorted.is_empty());
        let i = bad_at.index(sorted.len());
        let mut input: Vec<u128> = sorted[..=i].to_vec();
        // the element after position i is not greater than the previous one
        let bad = match bad_kind {
            0 => sorted[i],                 // duplicate
            1 => sorted[i].saturating_sub(1), // smaller (or equal at 0)
            _ => sorted[0],                 // back to the minimum
        };
        input.push(bad);
        input.push(u128::MAX);
        let err = RoaringBitmap128::from_sorted_iter(input.iter().copied()).unwrap_err();
        prop_assert_eq!(err.valid_until(), i as u64 + 1);

        // append on a non-empty bitmap keeps the valid prefix
        let mut rb = RoaringBitmap128::new();
        let err = rb.append(input.iter().copied()).unwrap_err();
        prop_assert_eq!(err.valid_until(), i as u64 + 1);
        let prefix: Model = sorted[..=i].iter().copied().collect();
        assert_matches(&rb, &prefix);
        let err = rb.append(sorted[..=i].iter().copied()).unwrap_err();
        prop_assert_eq!(err.valid_until(), 0);
        assert_matches(&rb, &prefix);
        prop_assert_eq!(rb.append(std::iter::empty()), Ok(0));
    }

    #[test]
    fn clone_eq_and_debug((a, b) in spec_pair()) {
        let (ra, ma) = a.build();
        let (rb, mb) = b.build();

        // Eq is semantic: the same values built differently are equal
        let rebuilt = RoaringBitmap128::from_sorted_iter(ma.iter().copied()).unwrap();
        prop_assert_eq!(&rebuilt, &ra);
        let mut optimized = rebuilt.clone();
        optimized.optimize();
        prop_assert_eq!(&optimized, &ra);
        let mut unrun = ra.clone();
        unrun.remove_run_compression();
        prop_assert_eq!(&unrun, &optimized);
        prop_assert_eq!(ra == rb, ma == mb);
        prop_assert_eq!(ra != rb, ma != mb);

        let c = ra.clone();
        assert_matches(&c, &ma);
        let mut d = rb.clone();
        d.clone_from(&ra);
        assert_matches(&d, &ma);
        prop_assert_eq!(&d, &ra);

        // Debug
        let expected = if ma.len() < 16 {
            format!("RoaringBitmap128<{:?}>", ma.iter().collect::<Vec<_>>())
        } else {
            format!(
                "RoaringBitmap128<{:?} values between {:?} and {:?} in {:?} containers>",
                ma.len(),
                ma.first().unwrap(),
                ma.last().unwrap(),
                n_keys(&ma)
            )
        };
        prop_assert_eq!(format!("{ra:?}"), expected);
    }
}

#[test]
fn from_array() {
    let rb = RoaringBitmap128::from([u128::MAX, 0, 1 << 64, 0]);
    assert!(rb.iter().eq([0, 1 << 64, u128::MAX]));
    let rb = RoaringBitmap128::from([]);
    assert!(rb.is_empty());
    assert_eq!(RoaringBitmap128::default(), rb);
}
