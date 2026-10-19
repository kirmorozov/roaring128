//! The `uuid` feature: `insert_uuid` / `contains_uuid` / `remove_uuid` / `iter_uuids` and
//! `FromIterator` / `Extend` of `Uuid` and `&Uuid`.

#![cfg(feature = "uuid")]

mod common;

use proptest::collection::vec;
use proptest::prelude::*;
use roaring128::RoaringBitmap128;
use uuid::Uuid;

use common::*;

#[test]
fn uuid_round_trip() {
    let id = Uuid::from_u128(0x0192_7a3e_8f00_7cc1_9e3a_1b2c_3d4e_5f60);
    let mut set = RoaringBitmap128::new();
    assert!(set.insert_uuid(id));
    assert!(!set.insert_uuid(id));
    assert!(set.contains_uuid(id));
    assert!(set.contains(id.as_u128()));
    assert!(!set.contains_uuid(Uuid::nil()));
    assert_eq!(set.iter_uuids().next(), Some(id));
    assert_eq!(set.iter_uuids().next_back(), Some(id));
    assert!(set.remove_uuid(id));
    assert!(!set.remove_uuid(id));
    assert!(set.is_empty());
    let mut set = RoaringBitmap128::new();
    assert!(set.insert_uuid(Uuid::max()));
    assert!(set.contains(u128::MAX));
    assert!(set.insert_uuid(Uuid::nil()));
    assert!(set.iter_uuids().eq([Uuid::nil(), Uuid::max()]));
}

proptest! {
    #![proptest_config(config(128))]

    /// Iteration order is the `Uuid` (byte) order; collecting / extending with owned and borrowed
    /// UUIDs gives the same set as their `u128` values.
    #[test]
    fn uuids_match_model(raw in vec(prop_oneof![any::<u128>(), value()], 0..2000)) {
        let ids: Vec<Uuid> = raw.iter().map(|&v| Uuid::from_u128(v)).collect();
        let model: Model = raw.iter().copied().collect();
        let mut sorted_ids = ids.clone();
        sorted_ids.sort();
        sorted_ids.dedup();

        let owned: RoaringBitmap128 = ids.iter().copied().collect();
        let borrowed: RoaringBitmap128 = ids.iter().collect();
        let mut extended = RoaringBitmap128::new();
        extended.extend(ids.iter().copied());
        let mut extended_ref = RoaringBitmap128::new();
        extended_ref.extend(ids.iter());
        for rb in [&owned, &borrowed, &extended, &extended_ref] {
            assert_matches(rb, &model);
            prop_assert!(rb.iter_uuids().eq(sorted_ids.iter().copied()));
            prop_assert!(rb.iter_uuids().rev().eq(sorted_ids.iter().rev().copied()));
        }
        for id in &ids {
            prop_assert!(owned.contains_uuid(*id));
        }
        let mut rb = owned.clone();
        let mut m = model.clone();
        for id in ids.iter().step_by(2) {
            prop_assert_eq!(rb.remove_uuid(*id), m.remove(&id.as_u128()));
        }
        assert_matches(&rb, &m);
    }

    /// Time-ordered UUIDs (v7 layout: 48-bit millisecond timestamp first) iterate in creation
    /// order and share containers when created in the same millisecond window.
    #[test]
    fn v7_like_uuids_iterate_in_time_order(start_ms in 0u64..(1 << 47), rand_bits in vec(any::<u64>(), 1..500)) {
        let ids: Vec<Uuid> = rand_bits
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let ms = (start_ms + i as u64) as u128;
                let v = (ms << 80) | (0x7 << 76) | ((*r as u128) & ((1 << 76) - 1));
                Uuid::from_u128(v)
            })
            .collect();
        let rb: RoaringBitmap128 = ids.iter().collect();
        prop_assert!(rb.iter_uuids().eq(ids.iter().copied()));
        assert_valid(&rb);
    }
}
