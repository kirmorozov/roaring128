//! Rejection of corrupted input by `deserialize_from`: truncation at every byte offset, bad
//! cookie and flags, unsorted / duplicate keys, invalid arrays, bitmaps whose popcount does not
//! match the descriptor, overlapping / adjacent / unsorted / overflowing runs.
//!
//! `deserialize_unchecked_from` only skips the validation of container contents: it must still
//! reject truncated input and a bad cookie, and must not crash on the rest.

use std::panic::{AssertUnwindSafe, catch_unwind};

use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;
use crate::raw::*;

#[track_caller]
fn assert_rejected(bytes: &[u8], what: &str) {
    if let Ok(rb) = RoaringBitmap128::deserialize_from(bytes) {
        panic!("{what}: invalid input accepted: {rb:?}");
    }
}

/// `deserialize_unchecked_from` may accept invalid container contents, but must neither crash nor
/// hang; whatever it returns is not used further.
fn unchecked_does_not_crash(bytes: &[u8]) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _ = RoaringBitmap128::deserialize_unchecked_from(bytes);
    }));
}

/// Valid encodings covering every container kind, the run bitset and several keys.
fn samples() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut rb = RoaringBitmap128::new();
    rb.insert(u128::MAX);
    out.push(serialize(&rb));
    rb.insert_range((1 << 64) - 10..(1 << 64) + 10); // runs across a key boundary
    rb.extend((0..5000u128).map(|v| (1 << 100) + 3 * v)); // bitmap
    rb.extend([5u128, 7, 1 << 112]); // arrays
    out.push(serialize(&rb));
    let mut rb2 = rb.clone();
    rb2.optimize();
    out.push(serialize(&rb2));
    rb2.remove_run_compression();
    out.push(serialize(&rb2));
    let model: Model = (0..20u128).map(|i| i << 20).collect();
    out.push(Raw::from_model(&model, true).encode());
    out
}

#[test]
fn truncation_at_every_offset() {
    for bytes in samples() {
        assert!(RoaringBitmap128::deserialize_from(&bytes[..]).is_ok());
        for len in 0..bytes.len() {
            let cut = &bytes[..len];
            assert!(
                RoaringBitmap128::deserialize_from(cut).is_err(),
                "truncated to {len} of {} bytes: accepted",
                bytes.len()
            );
            assert!(
                RoaringBitmap128::deserialize_unchecked_from(cut).is_err(),
                "unchecked, truncated to {len} of {} bytes: accepted",
                bytes.len()
            );
        }
    }
}

#[test]
fn bad_cookie() {
    for bytes in samples() {
        for i in 0..4 {
            for bit in 0..8 {
                let mut b = bytes.clone();
                b[i] ^= 1 << bit;
                assert_rejected(&b, "cookie");
                assert!(RoaringBitmap128::deserialize_unchecked_from(&b[..]).is_err(), "unchecked: cookie");
            }
        }
    }
    // the 32-bit Roaring cookies are not accepted either
    for cookie in [12346u32, 12347] {
        let raw = Raw { cookie, ..Raw::new(vec![(0, Payload::Array(vec![1]))]) };
        assert_rejected(&raw.encode(), "32-bit cookie");
    }
}

#[test]
fn bad_flags() {
    let containers = vec![(0, Payload::Array(vec![1, 2])), (9, Payload::Run(vec![(0, 9)]))];
    for bit in 1..32 {
        for base in [0u32, 1] {
            let raw = Raw { flags: Some(base | (1 << bit)), write_bitset: Some(base == 1), ..Raw::new(containers.clone()) };
            assert_rejected(&raw.encode(), &format!("flags {:#x}", base | (1 << bit)));
            unchecked_does_not_crash(&raw.encode());
        }
    }
}

#[test]
fn unsorted_and_duplicate_keys() {
    let a = || Payload::Array(vec![1]);
    for keys in [
        vec![0u128, 0],
        vec![1, 0],
        vec![0, 2, 1],
        vec![MAX_KEY, 0],
        vec![MAX_KEY, MAX_KEY],
        vec![1 << 48, (1 << 48) - 1],
        vec![5, 6, 7, 7, 8],
    ] {
        let raw = Raw::new(keys.iter().map(|&k| (k, a())).collect());
        assert_rejected(&raw.encode(), &format!("keys {keys:x?}"));
        unchecked_does_not_crash(&raw.encode());
    }
    // equal keys with different kinds
    let raw = Raw::new(vec![(3, Payload::Array(vec![1])), (3, Payload::Run(vec![(5, 5)]))]);
    assert_rejected(&raw.encode(), "duplicate key array/run");
}

#[test]
fn invalid_arrays() {
    for values in [
        vec![0u16, 0],
        vec![1, 0],
        vec![0, 1, 2, 2],
        vec![0xFFFF, 0],
        vec![5, 4, 6],
        (0..4096).rev().collect::<Vec<u16>>(),
        (0..4096).map(|i| i / 2).collect(),
    ] {
        let raw = Raw::new(vec![(0, Payload::Array(vec![1])), (7, Payload::Array(values.clone()))]);
        assert_rejected(&raw.encode(), &format!("array {:?}", &values[..values.len().min(8)]));
        unchecked_does_not_crash(&raw.encode());
    }
}

#[test]
fn bitmap_popcount_mismatch() {
    let indices = || (0..5000u16).map(|i| i * 3);
    let Payload::Bitmap { words, card } = Payload::bitmap_of(indices()) else { unreachable!() };
    assert_eq!(card, 5000);
    let ok = Raw::new(vec![(1, Payload::Bitmap { words: words.clone(), card })]);
    assert!(RoaringBitmap128::deserialize_from(&ok.encode()[..]).is_ok());
    for declared in [4097u32, 4999, 5001, 0x1_0000] {
        let raw = Raw::new(vec![(1, Payload::Bitmap { words: words.clone(), card: declared })]);
        assert_rejected(&raw.encode(), &format!("bitmap declared {declared}, popcount 5000"));
        unchecked_does_not_crash(&raw.encode());
    }
    // a bitmap container with at most 4096 bits set is not a valid bitmap (descriptor > 4096)
    let Payload::Bitmap { words, .. } = Payload::bitmap_of(0..4096) else { unreachable!() };
    let raw = Raw::new(vec![(1, Payload::Bitmap { words, card: 4097 })]);
    assert_rejected(&raw.encode(), "4096 bits declared as 4097");
    let raw = Raw::new(vec![(1, Payload::Bitmap { words: vec![0; 1024], card: 4097 })]);
    assert_rejected(&raw.encode(), "empty bitmap");
}

#[test]
fn invalid_runs() {
    for runs in [
        vec![(0u16, 4u16), (1, 0)],     // overlapping (starts inside the first run)
        vec![(0, 0), (1, 0)],           // adjacent
        vec![(10, 5), (16, 0)],         // adjacent
        vec![(10, 5), (15, 3)],         // overlapping by one
        vec![(20, 0), (10, 0)],         // unsorted
        vec![(10, 0), (10, 0)],         // duplicate
        vec![(0xFFFE, 4)],              // overflow
        vec![(0xFFFF, 1)],              // overflow by one
        vec![(1, 0xFFFF)],              // overflow
        vec![(0, 0xFFFF), (0, 0)],      // full then duplicate
        vec![(100, 10), (50, 10)],      // unsorted, disjoint
    ] {
        let raw = Raw::new(vec![(0, Payload::Array(vec![1])), (1 << 90, Payload::Run(runs.clone()))]);
        assert_rejected(&raw.encode(), &format!("runs {runs:?}"));
        unchecked_does_not_crash(&raw.encode());
    }
}

#[test]
fn count_larger_than_content() {
    for bytes in samples() {
        let n = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        for extra in [1u64, 2, 100] {
            let mut b = bytes.clone();
            b[8..16].copy_from_slice(&(n + extra).to_le_bytes());
            assert_rejected(&b, "count + extra");
        }
    }
}

proptest! {
    #![proptest_config(config(64))]

    /// Random single-byte corruptions are either rejected or decode into a valid bitmap
    /// (`internal_validate` passes) whose re-serialization round trips. The container count
    /// (bytes 8..16) is left alone here, see the `serialization_limits` binary.
    #[test]
    fn random_corruption_never_yields_an_invalid_bitmap(
        spec in set_spec_with(3),
        runs in any::<bool>(),
        flips in proptest::collection::vec((any::<prop::sample::Index>(), 1..=255u8), 1..4),
    ) {
        let model = spec.model();
        let mut bytes = Raw::from_model(&model, runs).encode();
        for (i, x) in &flips {
            let i = i.index(bytes.len());
            if !(8..16).contains(&i) {
                bytes[i] ^= x;
            }
        }
        if let Ok(rb) = RoaringBitmap128::deserialize_from(&bytes[..]) {
            assert_valid(&rb);
            let back = round_trip(&rb);
            prop_assert_eq!(back, rb);
        }
        unchecked_does_not_crash(&bytes);
    }

    /// Truncation of random sets at random offsets is always rejected.
    #[test]
    fn random_truncation_is_rejected(spec in set_spec_with(3), cut in any::<prop::sample::Index>()) {
        let (rb, _) = spec.build();
        let bytes = serialize(&rb);
        let len = cut.index(bytes.len());
        prop_assert!(RoaringBitmap128::deserialize_from(&bytes[..len]).is_err());
        prop_assert!(RoaringBitmap128::deserialize_unchecked_from(&bytes[..len]).is_err());
    }
}
