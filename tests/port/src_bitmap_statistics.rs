//! `src/bitmap/statistics.rs` has no unit tests upstream, only the documentation example; it is
//! ported here together with container-kind checks at the array/bitmap boundary and for runs.

use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

/// The documentation example of `statistics()`.
fn doc_example(off: u128) {
    let off = off & !0xFFFF;
    let bitmap: RoaringBitmap128 = (1..100).map(|v| off + v).collect();
    let statistics = bitmap.statistics();

    assert_eq!(statistics.n_containers, 1);
    assert_eq!(statistics.n_array_containers, 1);
    assert_eq!(statistics.n_run_containers, 0);
    assert_eq!(statistics.n_bitset_containers, 0);
    assert_eq!(statistics.n_values_array_containers, 99);
    assert_eq!(statistics.n_values_run_containers, 0);
    assert_eq!(statistics.n_values_bitset_containers, 0);
    assert_eq!(statistics.max_value, Some(off + 99));
    assert_eq!(statistics.min_value, Some(off + 1));
    assert_eq!(statistics.cardinality, 99);
}

/// 4096 values is the largest array, 4097 the smallest bitmap (one key, no runs).
fn array_bitmap_boundary(off: u128) {
    let off = off & !0xFFFF;
    let mut bitmap: RoaringBitmap128 = (0..4096).map(|v| off + 2 * v).collect();
    let s = bitmap.statistics();
    assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (1, 0, 0));
    assert_eq!(s.n_values_array_containers, 4096);

    bitmap.insert(off + 2 * 4096);
    let s = bitmap.statistics();
    assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (0, 1, 0));
    assert_eq!(s.n_values_bitset_containers, 4097);
    assert!(s.n_bytes_bitset_containers >= 8192);

    bitmap.remove(off);
    let s = bitmap.statistics();
    assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (1, 0, 0));
    assert_eq!(s.cardinality, 4096);
    assert_valid(&bitmap);
}

/// `insert_range` creates runs, `optimize` and `remove_run_compression` switch kinds.
fn runs(off: u128) {
    let off = off & !0xFFFF;
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off + 10..off + 1000);
    let s = bitmap.statistics();
    assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (0, 0, 1));
    assert_eq!(s.n_values_run_containers, 990);

    assert!(bitmap.remove_run_compression());
    let s = bitmap.statistics();
    assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (1, 0, 0));
    assert_eq!(s.n_values_array_containers, 990);

    assert!(bitmap.optimize());
    let s = bitmap.statistics();
    assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (0, 0, 1));

    // a big dense container becomes a bitmap without runs
    bitmap.insert_range(off + 0x1_0000..off + 0x1_8000);
    assert!(bitmap.remove_run_compression());
    let s = bitmap.statistics();
    assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (1, 1, 0));
    assert_eq!(s.n_values_bitset_containers, 0x8000);
    assert_eq!(s.n_containers, 2);
    assert_eq!(s.cardinality, 990 + 0x8000);
}

offset_variants!(doc_example, array_bitmap_boundary, runs);

#[test]
fn empty_statistics() {
    let s = RoaringBitmap128::new().statistics();
    assert_eq!(s.n_containers, 0);
    assert_eq!(s.n_array_containers + s.n_bitset_containers + s.n_run_containers, 0);
    assert_eq!(s.cardinality, 0);
    assert_eq!(s.max_value, None);
    assert_eq!(s.min_value, None);
    assert_eq!(s.n_bytes_array_containers + s.n_bytes_bitset_containers + s.n_bytes_run_containers, 0);
}

#[test]
fn sparse_uuids_one_container_each() {
    let values: Vec<u128> =
        (1..=1000u128).map(|i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835)).collect();
    let bitmap: RoaringBitmap128 = values.iter().copied().collect();
    let s = bitmap.statistics();
    assert_eq!(s.n_containers, 1000);
    assert_eq!(s.n_array_containers, 1000);
    assert_eq!(s.n_values_array_containers, 1000);
    assert_eq!(s.cardinality, 1000);
    assert_eq!(s.max_value, values.iter().max().copied());
    assert_eq!(s.min_value, values.iter().min().copied());
}

proptest! {
    #![proptest_config(config(64))]

    /// Statistics are consistent with the content, and container kinds follow the roaring rules
    /// after `remove_run_compression` (and then `optimize`).
    #[test]
    fn statistics_match_model(spec in set_spec()) {
        let (mut rb, model) = spec.build();
        let s = rb.statistics();
        prop_assert_eq!(s.cardinality, model.len() as u64);
        prop_assert_eq!(s.n_containers, n_keys(&model));
        prop_assert_eq!(s.n_containers, s.n_array_containers + s.n_bitset_containers + s.n_run_containers);
        prop_assert_eq!(
            s.cardinality,
            s.n_values_array_containers + s.n_values_bitset_containers + s.n_values_run_containers
        );
        prop_assert_eq!(s.max_value, model.last().copied());
        prop_assert_eq!(s.min_value, model.first().copied());
        prop_assert!(s.n_values_array_containers <= 4096 * s.n_array_containers);
        prop_assert!(s.n_values_bitset_containers > 4096 * s.n_bitset_containers || s.n_bitset_containers == 0);
        prop_assert!(s.n_bytes_bitset_containers >= 8192 * s.n_bitset_containers);
        prop_assert!(s.n_bytes_array_containers >= 2 * s.n_values_array_containers);

        rb.remove_run_compression();
        let s = rb.statistics();
        let (a, b, r) = expected_kinds(&model, false);
        prop_assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (a, b, r));

        rb.optimize();
        let s = rb.statistics();
        let (a, b, r) = expected_kinds(&model, true);
        prop_assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (a, b, r));
        assert_matches(&rb, &model);
    }
}
