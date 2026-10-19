//! Port of the proptest of `src/bitmap/ops_with_serialized.rs`.

use std::io::{Cursor, Seek, SeekFrom};

use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

proptest! {
    #![proptest_config(config(128))]

    #[test]
    fn intersection_with_serialized_eq_materialized_intersection((a, b) in spec_pair()) {
        let (a, ma) = a.build();
        let (b, mb) = b.build();
        let mut serialized_bytes_b = Vec::new();
        b.serialize_into(&mut serialized_bytes_b).unwrap();
        let serialized_bytes_b = &serialized_bytes_b[..];

        let got = a.intersection_with_serialized_unchecked(Cursor::new(serialized_bytes_b)).unwrap();
        assert_matches(&got, &(&ma & &mb));
        prop_assert_eq!(got, a & b);
    }

    /// Both operands in the run-free and in the optimized representation.
    #[test]
    fn intersection_with_serialized_all_kinds((a, b) in spec_pair(), opt_a in any::<bool>(), opt_b in any::<bool>()) {
        let (mut a, ma) = a.build();
        let (mut b, mb) = b.build();
        a.remove_run_compression();
        b.remove_run_compression();
        if opt_a { a.optimize(); }
        if opt_b { b.optimize(); }
        let bytes = serialize(&b);
        let got = a.intersection_with_serialized_unchecked(Cursor::new(&bytes)).unwrap();
        assert_matches(&got, &(&ma & &mb));
    }
}

#[test]
fn intersection_with_serialized_edge_cases() {
    let empty = RoaringBitmap128::new();
    let a: RoaringBitmap128 = [0u128, 1 << 64, u128::MAX].into_iter().collect();
    let bytes_a = serialize(&a);
    let bytes_empty = serialize(&empty);
    assert!(
        empty
            .intersection_with_serialized_unchecked(Cursor::new(&bytes_a))
            .unwrap()
            .is_empty()
    );
    assert!(
        a.intersection_with_serialized_unchecked(Cursor::new(&bytes_empty))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        a.intersection_with_serialized_unchecked(Cursor::new(&bytes_a))
            .unwrap(),
        a
    );
    // truncated or foreign input is an error, not a panic
    assert!(
        a.intersection_with_serialized_unchecked(Cursor::new(&bytes_a[..10]))
            .is_err()
    );
    assert!(
        a.intersection_with_serialized_unchecked(Cursor::new(&[0u8; 16][..]))
            .is_err()
    );
}

/// The serialized bitmap does not have to start at position 0 of the stream (not specified by
/// the documentation; `deserialize_from` reads from the current position).
#[test]
fn intersection_with_serialized_at_stream_offset() {
    let a: RoaringBitmap128 = (0..10_000u128).map(|v| (1 << 64) + v * 5).collect();
    let mut b = RoaringBitmap128::new();
    b.insert_range((1 << 64) + 1000..(1 << 64) + 20_000);
    b.insert(u128::MAX);
    let mut stream = vec![0xAB; 37];
    stream.extend(serialize(&b));
    stream.extend([0xCD; 11]);
    let mut cursor = Cursor::new(&stream);
    cursor.seek(SeekFrom::Start(37)).unwrap();
    let got = a.intersection_with_serialized_unchecked(cursor).unwrap();
    assert_eq!(got, &a & &b);
}
