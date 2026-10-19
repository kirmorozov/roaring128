//! Conformance with `FORMAT.md`: exact bytes against the reference encoder of `raw.rs`, sizes,
//! and decoding of hand-written valid inputs (including non-canonical ones the format allows).

use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;
use crate::raw::*;

#[test]
fn empty_bitmap_bytes() {
    let bytes = serialize(&RoaringBitmap128::new());
    // cookie "RR18", flags 0, count 0
    assert_eq!(
        bytes,
        [b'R', b'R', b'1', b'8', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    let back = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
    assert!(back.is_empty());
}

#[test]
fn single_value_bytes() {
    let v: u128 = 0x0192_7a3e_8f00_7cc1_9e3a_1b2c_3d4e_5f60;
    let bytes = serialize(&RoaringBitmap128::from([v]));
    let mut expected = vec![b'R', b'R', b'1', b'8', 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
    // descriptor = (key << 16) | (cardinality - 1) = (v >> 16) << 16
    expected.extend(((v >> 16) << 16).to_le_bytes());
    expected.extend((v as u16).to_le_bytes());
    assert_eq!(bytes, expected);
    assert_eq!(bytes.len(), 16 + 18);
}

#[test]
fn max_value_bytes() {
    let mut rb = RoaringBitmap128::from([u128::MAX]);
    let bytes = serialize(&rb);
    assert_eq!(&bytes[16..32], &(u128::MAX << 16).to_le_bytes());
    assert_eq!(&bytes[32..], &[0xFF, 0xFF]);
    // a run container ending at u128::MAX
    rb.insert_range(u128::MAX - 99..);
    rb.optimize();
    let bytes = serialize(&rb);
    let model: Model = rb.iter().collect();
    assert_eq!(bytes, Raw::from_model(&model, true).encode());
    // flags bit 0 and the run bitset
    assert_eq!(&bytes[4..8], &[1, 0, 0, 0]);
    assert_eq!(bytes[16], 1);
    let back = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
    assert_eq!(back, rb);
}

#[test]
fn bitmap_container_bytes() {
    // 4097 values: smallest bitmap container
    let rb: RoaringBitmap128 = (0..4097u128).map(|v| (1 << 64) + 2 * v).collect();
    let bytes = serialize(&rb);
    assert_eq!(bytes.len(), 16 + 16 + 8192);
    let model: Model = rb.iter().collect();
    assert_eq!(bytes, Raw::from_model(&model, false).encode());
    // 4096 values: largest array container
    let rb: RoaringBitmap128 = (0..4096u128).map(|v| (1 << 64) + 2 * v).collect();
    assert_eq!(serialize(&rb).len(), 16 + 16 + 2 * 4096);
}

proptest! {
    #![proptest_config(config(128))]

    /// After `remove_run_compression()` (and then `optimize()`) the representation is fully
    /// determined by the set, so the bytes are too.
    #[test]
    fn bytes_match_reference_encoder(spec in set_spec()) {
        let (mut rb, model) = spec.build();
        rb.remove_run_compression();
        let expected = Raw::from_model(&model, false).encode();
        let bytes = serialize(&rb);
        prop_assert!(bytes == expected, "without runs: {} bytes, expected {}", bytes.len(), expected.len());

        rb.optimize();
        let expected = Raw::from_model(&model, true).encode();
        let bytes = serialize(&rb);
        prop_assert!(bytes == expected, "with runs: {} bytes, expected {}", bytes.len(), expected.len());
    }

    /// Every state (including non-optimal runs) round trips with its container kinds.
    #[test]
    fn round_trip_preserves_values_and_kinds(spec in set_spec()) {
        let (rb, model) = spec.build();
        let back = round_trip(&rb);
        assert_matches(&back, &model);
        let (s1, s2) = (rb.statistics(), back.statistics());
        prop_assert_eq!(
            (s1.n_array_containers, s1.n_bitset_containers, s1.n_run_containers),
            (s2.n_array_containers, s2.n_bitset_containers, s2.n_run_containers)
        );
        // serializing again gives the same bytes
        prop_assert!(serialize(&back) == serialize(&rb));
    }

    /// The reference encoding of any set decodes to that set.
    #[test]
    fn reference_encoding_decodes(spec in set_spec(), runs in any::<bool>()) {
        let model = spec.model();
        let bytes = Raw::from_model(&model, runs).encode();
        let rb = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
        assert_matches(&rb, &model);
        let rb = RoaringBitmap128::deserialize_unchecked_from(&bytes[..]).unwrap();
        assert_matches(&rb, &model);
        prop_assert_eq!(rb.serialized_size(), bytes.len());
    }

    /// Any valid run list is accepted, even when another representation would be smaller.
    #[test]
    fn non_canonical_runs_decode(key in any::<u128>(), sorted_indices in proptest::collection::btree_set(any::<u16>(), 1..3000)) {
        let key = key >> 16;
        let indices: Vec<u16> = sorted_indices.into_iter().collect();
        let raw = Raw::new(vec![(key, Payload::Run(to_runs(&indices)))]);
        let bytes = raw.encode();
        let rb = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
        assert_matches(&rb, &raw.model());
        prop_assert_eq!(rb.statistics().n_run_containers, 1);
        prop_assert!(serialize(&rb) == bytes);
    }
}

#[test]
fn hand_written_valid_inputs() {
    let k = MAX_KEY;
    let cases: Vec<Raw> = vec![
        // single value at the last key
        Raw::new(vec![(k, Payload::Array(vec![0xFFFF]))]),
        // one run of one value
        Raw::new(vec![(0, Payload::Run(vec![(7, 0)]))]),
        // a full run container and a full bitmap container
        Raw::new(vec![
            (1, Payload::Run(vec![(0, 0xFFFF)])),
            (2, Payload::bitmap_of(0..=0xFFFF)),
        ]),
        // array of 4096 values, bitmap of 4097 values
        Raw::new(vec![
            (5, Payload::Array((0..4096).map(|i| i * 2).collect())),
            (6, Payload::bitmap_of((0..4097).map(|i| i * 2))),
        ]),
        // a run container with more runs than any other representation would need
        Raw::new(vec![(
            1 << 100,
            Payload::Run((0..20_000u16).map(|i| (i * 3, 0)).collect()),
        )]),
        // runs in containers 0 and 9 of 10 (run bitset of two bytes)
        Raw::new(
            (0..10u128)
                .map(|i| {
                    let p = if i == 0 || i == 9 {
                        Payload::Run(vec![(1, 2)])
                    } else {
                        Payload::Array(vec![3])
                    };
                    (i * 1000, p)
                })
                .collect(),
        ),
        // keys straddling 2^48 (values around 2^64) and 2^96 (values around 2^112)
        Raw::new(vec![
            ((1 << 48) - 1, Payload::Array(vec![0xFFFF])),
            (1 << 48, Payload::Array(vec![0])),
            ((1 << 96) - 1, Payload::Run(vec![(0xFFF0, 0xF)])),
            (1 << 96, Payload::Run(vec![(0, 0xF)])),
        ]),
    ];
    for raw in cases {
        let bytes = raw.encode();
        let rb = RoaringBitmap128::deserialize_from(&bytes[..])
            .unwrap_or_else(|e| panic!("valid input rejected ({e}): {raw:?}"));
        assert_matches(&rb, &raw.model());
        assert_eq!(rb.serialized_size(), bytes.len());
        let rb = RoaringBitmap128::deserialize_unchecked_from(&bytes[..]).unwrap();
        assert_matches(&rb, &raw.model());
    }
}

#[test]
fn run_bitset_without_runs_is_accepted() {
    // FORMAT.md: flags bit 0 means "a run-container bitset follows"; an all-zero bitset is
    // well formed.
    let raw = Raw {
        flags: Some(1),
        ..Raw::new(vec![(3, Payload::Array(vec![1, 2, 3]))])
    };
    let bytes = raw.encode();
    assert_eq!(bytes.len(), 16 + 1 + 16 + 6);
    let rb = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
    assert_matches(&rb, &raw.model());
}

#[test]
fn reading_stops_at_the_end_of_the_bitmap() {
    // two bitmaps back to back in one stream
    let a: RoaringBitmap128 = [1u128, 1 << 64, u128::MAX].into_iter().collect();
    let mut b = RoaringBitmap128::new();
    b.insert_range(1000..100_000);
    let mut stream = serialize(&a);
    stream.extend(serialize(&b));
    let mut reader = &stream[..];
    let a2 = RoaringBitmap128::deserialize_from(&mut reader).unwrap();
    let b2 = RoaringBitmap128::deserialize_from(&mut reader).unwrap();
    assert_eq!(a2, a);
    assert_eq!(b2, b);
    assert!(reader.is_empty());
}
