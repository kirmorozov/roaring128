//! Point queries and updates: `contains`, `rank`, `select`, `min`, `max`, `len`, `is_empty`,
//! `insert`, `remove`, `try_push`, `push`.

use proptest::collection::vec;
use proptest::prelude::*;
use roaring128::{IntegerTooSmall, RoaringBitmap128};

use crate::common::*;

proptest! {
    #![proptest_config(config(128))]

    #[test]
    fn queries_match_model(spec in set_spec(), extra in vec(value(), 0..32), sel in vec(any::<u64>(), 0..8)) {
        let (rb, model) = spec.build();
        let sorted = sorted(&model);
        for v in probes(&model, 64).into_iter().chain(extra) {
            prop_assert_eq!(rb.contains(v), model.contains(&v), "contains({:#x})", v);
            prop_assert_eq!(rb.rank(v), rank_in(&sorted, v), "rank({:#x})", v);
        }
        prop_assert_eq!(rb.rank(u128::MAX), rb.len());
        let n = sorted.len() as u64;
        let step = (n / 64).max(1);
        let mut idx: Vec<u64> = (0..n).step_by(step as usize).collect();
        idx.extend([n.saturating_sub(1), n, n + 1, u64::MAX]);
        idx.extend(sel.iter().map(|&s| if n == 0 { s } else { s % (n + 2) }));
        for i in idx {
            let expected = sorted.get(i as usize).copied().filter(|_| i < n);
            prop_assert_eq!(rb.select(i), expected, "select({})", i);
            if let Some(v) = expected {
                prop_assert_eq!(rb.rank(v), i + 1);
            }
        }
        prop_assert_eq!(rb.min(), sorted.first().copied());
        prop_assert_eq!(rb.max(), sorted.last().copied());
        prop_assert_eq!(rb.len(), n);
        prop_assert_eq!(rb.is_empty(), n == 0);
        prop_assert!(!rb.is_full());
    }

    #[test]
    fn insert_remove_match_model(spec in set_spec(), ops in vec((any::<bool>(), value()), 1..200)) {
        let (mut rb, mut model) = spec.build();
        for (insert, v) in ops {
            if insert {
                prop_assert_eq!(rb.insert(v), model.insert(v), "insert({:#x})", v);
            } else {
                prop_assert_eq!(rb.remove(v), model.remove(&v), "remove({:#x})", v);
            }
            prop_assert_eq!(rb.len(), model.len() as u64);
            prop_assert_eq!(rb.contains(v), model.contains(&v));
            prop_assert_eq!(rb.min(), model.first().copied());
            prop_assert_eq!(rb.max(), model.last().copied());
            assert_valid(&rb);
        }
        assert_matches(&rb, &model);
    }

    /// Removing every value one by one (in random order) empties every container.
    #[test]
    fn remove_everything(spec in set_spec_with(3), seed in any::<u64>()) {
        let (mut rb, model) = spec.build();
        let mut values: Vec<u128> = model.iter().copied().collect();
        // cheap deterministic shuffle
        let mut s = seed | 1;
        for i in (1..values.len()).rev() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            values.swap(i, (s % (i as u64 + 1)) as usize);
        }
        let mut left = model.clone();
        for (k, v) in values.into_iter().enumerate() {
            prop_assert!(rb.remove(v));
            left.remove(&v);
            if k % 4096 == 0 {
                assert_valid(&rb);
                prop_assert_eq!(rb.statistics().n_containers, n_keys(&left));
            }
        }
        prop_assert!(rb.is_empty());
        prop_assert_eq!(rb.statistics().n_containers, 0);
        assert_valid(&rb);
    }

    #[test]
    #[allow(deprecated)]
    fn try_push_and_push_match_model(spec in set_spec(), pushes in vec(value(), 1..100)) {
        let (mut rb, mut model) = spec.build();
        for (i, v) in pushes.into_iter().enumerate() {
            let ok = model.last().is_none_or(|&m| v > m);
            if i % 2 == 0 {
                let res = rb.try_push(v);
                prop_assert_eq!(res, if ok { Ok(()) } else { Err(IntegerTooSmall) }, "try_push({:#x})", v);
            } else {
                prop_assert_eq!(rb.push(v), ok, "push({:#x})", v);
            }
            if ok {
                model.insert(v);
            }
            prop_assert_eq!(rb.len(), model.len() as u64);
            prop_assert_eq!(rb.max(), model.last().copied());
        }
        assert_matches(&rb, &model);
    }
}

#[test]
fn queries_on_empty() {
    let rb = RoaringBitmap128::new();
    for v in SPECIAL_VALUES {
        assert!(!rb.contains(*v));
        assert_eq!(rb.rank(*v), 0);
    }
    assert_eq!(rb.select(0), None);
    assert_eq!(rb.min(), None);
    assert_eq!(rb.max(), None);
    assert_eq!(rb.len(), 0);
    assert!(rb.is_empty());
    assert!(!rb.is_full());
}
