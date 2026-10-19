//! Port of the proptests of `src/bitmap/serde.rs` and `src/treemap/serde.rs` (feature `serde`).

#![cfg(feature = "serde")]

use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn test_serde_json(spec in set_spec_with(3)) {
        let bitmap = spec.bitmap();
        let json = serde_json::to_vec(&bitmap).unwrap();
        let back: RoaringBitmap128 = serde_json::from_slice(&json).unwrap();
        assert_valid(&back);
        prop_assert_eq!(bitmap, back);
    }

    #[test]
    fn test_postcard(spec in set_spec()) {
        let bitmap = spec.bitmap();
        let buffer = postcard::to_allocvec(&bitmap).unwrap();
        let back: RoaringBitmap128 = postcard::from_bytes(&buffer).unwrap();
        assert_valid(&back);
        prop_assert_eq!(bitmap, back);
    }
}

#[test]
fn serde_payload_is_the_native_format() {
    let bitmap: RoaringBitmap128 = [1u128, 1 << 64, u128::MAX].into_iter().collect();
    let json = serde_json::to_string(&bitmap).unwrap();
    let bytes: Vec<u8> = serde_json::from_str(&json).unwrap();
    assert_eq!(bytes, serialize(&bitmap));
}

#[test]
fn serde_rejects_invalid_payload() {
    let bitmap: RoaringBitmap128 = [1u128, 1 << 64, u128::MAX].into_iter().collect();
    let mut bytes = serialize(&bitmap);
    bytes.truncate(bytes.len() - 1);
    let json = serde_json::to_string(&bytes).unwrap();
    assert!(serde_json::from_str::<RoaringBitmap128>(&json).is_err());
    let mut bytes = serialize(&bitmap);
    bytes[0] ^= 1;
    let json = serde_json::to_string(&bytes).unwrap();
    assert!(serde_json::from_str::<RoaringBitmap128>(&json).is_err());
    assert!(serde_json::from_str::<RoaringBitmap128>("\"not bytes\"").is_err());
}
