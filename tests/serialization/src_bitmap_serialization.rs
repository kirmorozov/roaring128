//! Port of the serialization tests of `src/bitmap/serialization.rs` and
//! `src/treemap/serialization.rs` (`test_serialization`, `test_deserialize_overflow_s_plus_len`).
//! The `from_lsb0_bytes` tests of the same file are in the `model` binary (`misc`).

use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use crate::common::*;
use crate::raw::*;

proptest! {
    #![proptest_config(config(128))]

    #[test]
    fn test_serialization(spec in set_spec()) {
        let bitmap = spec.bitmap();
        let mut buffer = Vec::new();
        bitmap.serialize_into(&mut buffer).unwrap();
        prop_assert_eq!(buffer.len(), bitmap.serialized_size());
        prop_assert_eq!(&bitmap, &RoaringBitmap128::deserialize_from(buffer.as_slice()).unwrap());
        prop_assert_eq!(&bitmap, &RoaringBitmap128::deserialize_unchecked_from(buffer.as_slice()).unwrap());
    }

    #[test]
    fn test_serialization_into_writer_variants(spec in set_spec_with(2)) {
        let bitmap = spec.bitmap();
        // through `&mut Vec`, a `Cursor` and a writer that accepts one byte per call
        let mut a = Vec::new();
        bitmap.serialize_into(&mut a).unwrap();
        let mut cursor = std::io::Cursor::new(Vec::new());
        bitmap.serialize_into(&mut cursor).unwrap();
        prop_assert!(cursor.into_inner() == a);
        struct OneByte(Vec<u8>);
        impl std::io::Write for OneByte {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                match buf.first() {
                    Some(&b) => { self.0.push(b); Ok(1) }
                    None => Ok(0),
                }
            }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        let mut w = OneByte(Vec::new());
        bitmap.serialize_into(&mut w).unwrap();
        prop_assert!(w.0 == a);
        // and a reader that returns one byte per call
        struct OneByteReader<'a>(&'a [u8]);
        impl std::io::Read for OneByteReader<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if buf.is_empty() || self.0.is_empty() { return Ok(0); }
                buf[0] = self.0[0];
                self.0 = &self.0[1..];
                Ok(1)
            }
        }
        let back = RoaringBitmap128::deserialize_from(OneByteReader(&a)).unwrap();
        prop_assert_eq!(back, bitmap);
    }
}

#[test]
fn serialize_into_failing_writer() {
    struct Full(usize);
    impl std::io::Write for Full {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::Error::other("full"));
            }
            let n = buf.len().min(self.0);
            self.0 -= n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let rb: RoaringBitmap128 = (0..10_000u128).map(|v| v * 7).collect();
    let size = rb.serialized_size();
    for cap in [0, 1, 15, 16, 17, size / 2, size - 1] {
        assert!(
            rb.serialize_into(Full(cap)).is_err(),
            "writer with room for {cap} of {size} bytes"
        );
    }
    assert!(rb.serialize_into(Full(size)).is_ok());
}

/// The original fed a crafted 32-bit input whose run `start + length` overflows.
#[test]
fn test_deserialize_overflow_s_plus_len() {
    let raw = Raw::new(vec![(0, Payload::Run(vec![(0xFF82, 0xFE3B)]))]);
    let res = RoaringBitmap128::deserialize_from(&raw.encode()[..]);
    assert!(res.is_err());
    // the original bytes (32-bit format) are rejected as well
    let data = vec![
        59, 48, 0, 0, 255, 130, 254, 59, 48, 2, 0, 41, 255, 255, 166, 197, 4, 0, 2,
    ];
    assert!(RoaringBitmap128::deserialize_from(data.as_slice()).is_err());
}
