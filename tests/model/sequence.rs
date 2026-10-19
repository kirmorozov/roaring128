//! State machine test: random sequences of every mutating operation applied to one bitmap and to
//! its model; return values are checked, then `internal_validate()` and a full comparison with
//! the model run after every step.

use proptest::collection::vec;
use proptest::prelude::*;
use roaring128::{IntegerTooSmall, RoaringBitmap128};

use crate::common::*;

#[derive(Debug, Clone)]
enum Op {
    Insert(u128),
    Remove(u128),
    TryPush(u128),
    InsertRange(RangeSpec),
    RemoveRange(RangeSpec),
    Extend(Vec<u128>),
    AppendSorted(Vec<u128>),
    RemoveSmallest(u64),
    RemoveBiggest(u64),
    Optimize,
    RemoveRunCompression,
    Binary(SetOp, Form, SetSpec),
    MultiUnion(Vec<SetSpec>),
    CloneFrom(SetSpec),
    #[cfg(feature = "std")]
    SerializeRoundTrip,
    Clear,
}

/// A serialization round trip (an empty `Extend` without `std`).
fn serialize_op() -> Op {
    #[cfg(feature = "std")]
    return Op::SerializeRoundTrip;
    #[cfg(not(feature = "std"))]
    return Op::Extend(Vec::new());
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => value().prop_map(Op::Insert),
        6 => value().prop_map(Op::Remove),
        2 => value().prop_map(Op::TryPush),
        4 => insertable_range().prop_map(Op::InsertRange),
        4 => any_range().prop_map(Op::RemoveRange),
        2 => vec(value(), 0..50).prop_map(Op::Extend),
        1 => vec(value(), 0..50).prop_map(Op::AppendSorted),
        1 => prop_oneof![0..100u64, 0..100_000u64].prop_map(Op::RemoveSmallest),
        1 => prop_oneof![0..100u64, 0..100_000u64].prop_map(Op::RemoveBiggest),
        2 => Just(Op::Optimize),
        2 => Just(Op::RemoveRunCompression),
        4 => (set_op(), form(), set_spec_with(3)).prop_map(|(o, f, s)| Op::Binary(o, f, s)),
        1 => vec(set_spec_with(2), 0..3).prop_map(Op::MultiUnion),
        1 => set_spec_with(2).prop_map(Op::CloneFrom),
        1 => Just(serialize_op()),
        1 => Just(Op::Clear),
    ]
}

fn apply(rb: &mut RoaringBitmap128, model: &mut Model, op: &Op) {
    match op {
        Op::Insert(v) => assert_eq!(rb.insert(*v), model.insert(*v), "insert({v:#x})"),
        Op::Remove(v) => assert_eq!(rb.remove(*v), model.remove(v), "remove({v:#x})"),
        Op::TryPush(v) => {
            let ok = model.last().is_none_or(|m| v > m);
            assert_eq!(
                rb.try_push(*v),
                if ok { Ok(()) } else { Err(IntegerTooSmall) },
                "try_push({v:#x})"
            );
            if ok {
                model.insert(*v);
            }
        }
        Op::InsertRange(r) => {
            let expected = match r.bounds() {
                None => 0,
                Some((lo, hi)) => (hi - lo + 1) as u64 - r.model_count(model),
            };
            assert_eq!(rb.insert_range(*r), expected, "insert_range({r:x?})");
            if let Some((lo, hi)) = r.bounds() {
                model.extend(lo..=hi);
            }
        }
        Op::RemoveRange(r) => {
            let gone: Vec<u128> = r.model_values(model).collect();
            assert_eq!(
                rb.remove_range(*r),
                gone.len() as u64,
                "remove_range({r:x?})"
            );
            for v in gone {
                model.remove(&v);
            }
        }
        Op::Extend(values) => {
            rb.extend(values.iter());
            model.extend(values.iter().copied());
        }
        Op::AppendSorted(values) => {
            // shift the values above the current maximum when possible
            let base = model.last().map_or(0, |m| m.saturating_add(1));
            let mut s: Vec<u128> = values
                .iter()
                .map(|v| base.saturating_add(v & 0xF_FFFF))
                .collect();
            s.sort_unstable();
            s.dedup();
            let valid = match (s.first(), model.last()) {
                (Some(f), Some(m)) if f <= m => 0, // only possible at u128::MAX
                _ => s.len(),
            };
            let res = rb.append(s.iter().copied());
            if valid == s.len() {
                assert_eq!(res, Ok(s.len() as u64), "append");
            } else {
                assert_eq!(res.unwrap_err().valid_until(), valid as u64, "append error");
            }
            model.extend(s[..valid].iter().copied());
        }
        Op::RemoveSmallest(n) => {
            rb.remove_smallest(*n);
            for _ in 0..*n {
                if model.pop_first().is_none() {
                    break;
                }
            }
        }
        Op::RemoveBiggest(n) => {
            rb.remove_biggest(*n);
            for _ in 0..*n {
                if model.pop_last().is_none() {
                    break;
                }
            }
        }
        Op::Optimize => {
            rb.optimize();
            assert!(!rb.optimize(), "second optimize must return false");
        }
        Op::RemoveRunCompression => {
            rb.remove_run_compression();
            assert!(
                !rb.remove_run_compression(),
                "second remove_run_compression must return false"
            );
            assert_eq!(rb.statistics().n_run_containers, 0);
        }
        Op::Binary(o, f, spec) => {
            let (other, other_model) = spec.build();
            *rb = apply_op(*o, *f, rb, &other);
            *model = model_op(*o, model, &other_model);
        }
        Op::MultiUnion(specs) => {
            let others: Vec<(RoaringBitmap128, Model)> = specs.iter().map(SetSpec::build).collect();
            let mut all: Vec<&RoaringBitmap128> = vec![&*rb];
            all.extend(others.iter().map(|o| &o.0));
            let res = roaring128::MultiOps::union(all);
            for (_, m) in &others {
                model.extend(m.iter().copied());
            }
            *rb = res;
        }
        Op::CloneFrom(spec) => {
            let (other, other_model) = spec.build();
            rb.clone_from(&other);
            *model = other_model;
        }
        #[cfg(feature = "std")]
        Op::SerializeRoundTrip => {
            *rb = round_trip(rb);
        }
        Op::Clear => {
            rb.clear();
            model.clear();
        }
    }
}

proptest! {
    #![proptest_config(config(96))]

    #[test]
    fn random_operation_sequences(start in set_spec_with(3), ops in vec(op(), 1..24)) {
        let (mut rb, mut model) = start.build();
        for (i, op) in ops.iter().enumerate() {
            apply(&mut rb, &mut model, op);
            assert_matches(&rb, &model);
            prop_assert_eq!(rb.statistics().n_containers, n_keys(&model), "step {}: {:?}", i, op);
        }
        // final deep checks
        let all = sorted(&model);
        for v in probes(&model, 32) {
            prop_assert_eq!(rb.contains(v), model.contains(&v));
            prop_assert_eq!(rb.rank(v), rank_in(&all, v));
        }
        #[cfg(feature = "std")]
        assert_matches(&round_trip(&rb), &model);
    }
}
