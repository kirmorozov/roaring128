//! Port of the proptests of `src/treemap/inherent.rs`.
//!
//! `optimize_prunes_empty_partitions_from_bitmaps` built treemaps with empty partitions through
//! `from_bitmaps`; the 128-bit bitmap cannot hold empty containers, so the property checked is
//! that no operation ever leaves one behind (`n_containers` equals the number of distinct keys).

use proptest::prelude::*;

use crate::common::*;

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn optimize_preserves_values(spec in set_spec()) {
        let (mut bitmap, model) = spec.build();
        let before = bitmap.iter().collect::<Vec<_>>();

        let changed = bitmap.optimize();

        let after = bitmap.iter().collect::<Vec<_>>();
        prop_assert_eq!(&before, &after);
        assert_matches(&bitmap, &model);
        // a second optimize never changes anything
        prop_assert!(!bitmap.optimize(), "second optimize returned true (first: {})", changed);

        let changed = bitmap.remove_run_compression();
        prop_assert_eq!(bitmap.iter().collect::<Vec<_>>(), before);
        prop_assert!(!bitmap.remove_run_compression(), "second remove_run_compression returned true (first: {})", changed);
        prop_assert_eq!(bitmap.statistics().n_run_containers, 0);
        assert_matches(&bitmap, &model);
    }

    #[test]
    fn no_empty_containers_after_removals(
        spec in set_spec(),
        removals in proptest::collection::vec(insertable_range(), 0..8),
    ) {
        let (mut bitmap, mut model) = spec.build();
        for r in &removals {
            let expected = r.model_count(&model);
            prop_assert_eq!(bitmap.remove_range(*r), expected);
            let gone: Vec<u128> = r.model_values(&model).collect();
            for v in gone {
                model.remove(&v);
            }
            prop_assert_eq!(bitmap.statistics().n_containers, n_keys(&model));
        }
        bitmap.optimize();
        prop_assert_eq!(bitmap.statistics().n_containers, n_keys(&model));
        assert_matches(&bitmap, &model);
    }
}
