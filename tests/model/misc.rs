//! `optimize`, `remove_run_compression`, `remove_smallest`, `remove_biggest`, `from_lsb0_bytes`,
//! `statistics`, `clear`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use proptest::collection::vec;
use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

proptest! {
    #![proptest_config(config(128))]

    /// Values are preserved; container kinds follow the roaring rules; return values report
    /// whether something changed; both operations are idempotent.
    #[test]
    fn optimize_and_remove_run_compression(spec in set_spec()) {
        let (mut rb, model) = spec.build();
        let had_runs = rb.statistics().n_run_containers > 0;
        prop_assert_eq!(rb.remove_run_compression(), had_runs);
        assert_matches(&rb, &model);
        prop_assert!(!rb.remove_run_compression());
        let (a, b, r) = expected_kinds(&model, false);
        let s = rb.statistics();
        prop_assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (a, b, r));

        // from a run-free state, optimize converts exactly the containers predicted
        let (a2, b2, r2) = expected_kinds(&model, true);
        prop_assert_eq!(rb.optimize(), r2 > 0);
        assert_matches(&rb, &model);
        let s = rb.statistics();
        prop_assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (a2, b2, r2));
        prop_assert_eq!(s.n_values_run_containers + s.n_values_array_containers + s.n_values_bitset_containers, model.len() as u64);
        prop_assert!(!rb.optimize(), "optimize is idempotent");

        prop_assert_eq!(rb.remove_run_compression(), r2 > 0);
        let s = rb.statistics();
        prop_assert_eq!((s.n_array_containers, s.n_bitset_containers, s.n_run_containers), (a, b, r));
        assert_matches(&rb, &model);
    }

    /// Whatever the history, a second `optimize` returns false and values are preserved.
    #[test]
    fn optimize_twice_after_mutations(spec in set_spec(), removals in vec(value(), 0..32), ranges in vec(insertable_range(), 0..3)) {
        let (mut rb, mut model) = spec.build();
        for v in removals {
            rb.remove(v);
            model.remove(&v);
        }
        for r in ranges {
            rb.insert_range(r);
            if let Some((lo, hi)) = r.bounds() {
                model.extend(lo..=hi);
            }
        }
        rb.optimize();
        assert_matches(&rb, &model);
        prop_assert!(!rb.optimize());
        rb.remove_run_compression();
        prop_assert!(!rb.remove_run_compression());
        assert_matches(&rb, &model);
    }

    #[test]
    fn remove_smallest_and_biggest(spec in set_spec(), n in prop_oneof![0..300u64, 0..200_000u64, Just(u64::MAX)]) {
        let (rb, model) = spec.build();
        let all = sorted(&model);
        let k = (n as usize).min(all.len());

        let mut a = rb.clone();
        a.remove_smallest(n);
        let expected: Model = all[k..].iter().copied().collect();
        assert_matches(&a, &expected);
        prop_assert_eq!(a.statistics().n_containers, n_keys(&expected));

        let mut b = rb.clone();
        b.remove_biggest(n);
        let expected: Model = all[..all.len() - k].iter().copied().collect();
        assert_matches(&b, &expected);
        prop_assert_eq!(b.statistics().n_containers, n_keys(&expected));
    }

    #[test]
    fn from_lsb0_bytes_matches_model(
        offset in value(),
        bytes in prop_oneof![
            vec(any::<u8>(), 0..64),
            vec(prop_oneof![Just(0u8), Just(0xFF), any::<u8>()], 0..20_000),
            vec(Just(0xFFu8), 8192..=8200),
        ],
    ) {
        let bits = bytes.len() as u128 * 8;
        // offset + bits must not exceed 2^128
        let fits = bits == 0 || offset.checked_add(bits - 1).is_some();
        if fits {
            let rb = RoaringBitmap128::from_lsb0_bytes(offset, &bytes);
            assert_matches(&rb, &lsb0_model(offset, &bytes));
        } else {
            let res = catch_unwind(AssertUnwindSafe(|| RoaringBitmap128::from_lsb0_bytes(offset, &bytes)));
            prop_assert!(res.is_err(), "from_lsb0_bytes({:#x}, {} bytes) must panic", offset, bytes.len());
        }
    }

    #[test]
    fn from_lsb0_bytes_at_top(bytes in vec(any::<u8>(), 1..10_000)) {
        // the last bit lands exactly on u128::MAX
        let bits = bytes.len() as u128 * 8;
        let offset = u128::MAX - (bits - 1);
        let rb = RoaringBitmap128::from_lsb0_bytes(offset, &bytes);
        assert_matches(&rb, &lsb0_model(offset, &bytes));
        let res = catch_unwind(AssertUnwindSafe(|| RoaringBitmap128::from_lsb0_bytes(offset + 1, &bytes)));
        prop_assert!(res.is_err());
    }

    #[test]
    fn clear_resets(spec in set_spec()) {
        let (mut rb, _) = spec.build();
        rb.clear();
        assert_matches(&rb, &Model::new());
        prop_assert_eq!(rb.statistics().n_containers, 0);
        prop_assert_eq!(rb, RoaringBitmap128::new());
    }
}

#[test]
fn from_lsb0_bytes_edge_cases() {
    // empty input never panics, even at the very top
    assert!(RoaringBitmap128::from_lsb0_bytes(u128::MAX, &[]).is_empty());
    assert!(RoaringBitmap128::from_lsb0_bytes(0, &[0; 100]).is_empty());
    // the last byte of the space
    let rb = RoaringBitmap128::from_lsb0_bytes(u128::MAX - 7, &[0x80]);
    assert!(rb.iter().eq([u128::MAX]));
    // a full container at the top, from 8192 bytes of ones
    let rb = RoaringBitmap128::from_lsb0_bytes(u128::MAX - 0xFFFF, &[0xFF; 8192]);
    assert_eq!(rb.len(), 0x1_0000);
    assert!(rb.contains(u128::MAX));
    assert_valid(&rb);
    // one byte past the end panics even if its bits are zero
    let res = catch_unwind(|| RoaringBitmap128::from_lsb0_bytes(u128::MAX - 6, &[0x01]));
    assert!(res.is_err());
}

/// The original `test_from_lsb0_bytes` of `src/bitmap/serialization.rs`, at 128-bit offsets.
fn lsb0_original(base: u128) {
    const CONTAINER_OFFSET: u128 = 1 << 16;
    const CONTAINER_OFFSET_IN_BYTES: usize = (CONTAINER_OFFSET / 8) as usize;
    let mut bytes = vec![0xff; CONTAINER_OFFSET_IN_BYTES];
    bytes.extend([0x00; CONTAINER_OFFSET_IN_BYTES]);
    bytes.extend([0b00000001, 0b00000010, 0b00000011, 0b00000100]);

    let offset = base + 32;
    let rb = RoaringBitmap128::from_lsb0_bytes(offset, &bytes);
    assert_valid(&rb);
    for i in base..offset {
        assert!(!rb.contains(i), "{i} should not be in the bitmap");
    }
    for i in offset..offset + CONTAINER_OFFSET {
        assert!(rb.contains(i), "{i} should be in the bitmap");
    }
    for i in offset + CONTAINER_OFFSET..offset + CONTAINER_OFFSET * 2 {
        assert!(!rb.contains(i), "{i} should not be in the bitmap");
    }
    for bit in [0, 9, 16, 17, 26] {
        let i = bit + offset + CONTAINER_OFFSET * 2;
        assert!(rb.contains(i), "{i} should be in the bitmap");
    }

    assert_eq!(rb.len(), CONTAINER_OFFSET as u64 + 5);

    // Ensure the empty container is not created
    let mut bytes = vec![0x00u8; CONTAINER_OFFSET_IN_BYTES];
    bytes.extend([0xff]);
    let rb = RoaringBitmap128::from_lsb0_bytes(base, &bytes);
    assert_eq!(rb.min(), Some(base + CONTAINER_OFFSET));
    assert_eq!(rb.statistics().n_containers, 1 + u64::from(base % CONTAINER_OFFSET > CONTAINER_OFFSET - 8));

    let rb = RoaringBitmap128::from_lsb0_bytes(base + 8, &bytes);
    assert_eq!(rb.min(), Some(base + CONTAINER_OFFSET + 8));
}

/// The original `test_from_lsb0_bytes_not_multiple_of_8`.
fn lsb0_not_multiple_of_8(base: u128) {
    const CONTAINER_OFFSET: u128 = 1 << 16;
    const CONTAINER_OFFSET_IN_BYTES: usize = (CONTAINER_OFFSET / 8) as usize;

    let mut bytes = vec![0b0101_1001];
    bytes.extend([0x00; CONTAINER_OFFSET_IN_BYTES]);
    bytes.extend([0b00000001, 0b00000010, 0b00000011, 0b00000100]);

    let mut indices = vec![0, 3, 4, 6];
    indices.extend([0, 9, 16, 17, 26].map(|i| 8 + CONTAINER_OFFSET + i));

    for offset in 0..8 {
        let rb = RoaringBitmap128::from_lsb0_bytes(base + offset, &bytes);
        for i in indices.iter().map(|&i| base + i + offset) {
            assert!(rb.contains(i), "{i} should be in the bitmap");
        }
        assert_eq!(rb.len(), indices.len() as u64);
    }
}

offset_variants!(lsb0_original, lsb0_not_multiple_of_8);

#[test]
#[should_panic]
fn test_from_lsb0_bytes_overflow() {
    // roaring panics with "<= 2^32"; the 128-bit message is not specified
    let bytes = [0x01, 0x01];
    RoaringBitmap128::from_lsb0_bytes(u128::MAX - 7, &bytes);
}
