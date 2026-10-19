use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::de::SeqAccess;
use serde::de::Visitor;

use crate::RoaringBitmap128;

impl<'de> Deserialize<'de> for RoaringBitmap128 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BitmapVisitor;

        impl<'de> Visitor<'de> for BitmapVisitor {
            type Value = RoaringBitmap128;

            fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
                formatter.write_str("roaring128 bitmap")
            }

            fn visit_bytes<E>(self, bytes: &[u8]) -> Result<RoaringBitmap128, E>
            where
                E: serde::de::Error,
            {
                RoaringBitmap128::deserialize_from(bytes).map_err(serde::de::Error::custom)
            }

            // in some case bytes will be serialized as a sequence thus we need to accept both
            // even if it means non optimal performance
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut bytes: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
                while let Some(el) = seq.next_element()? {
                    bytes.push(el);
                }
                RoaringBitmap128::deserialize_from(&*bytes).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_bytes(BitmapVisitor)
    }
}

impl Serialize for RoaringBitmap128 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut buf = alloc::vec::Vec::new();
        self.serialize_into(&mut buf)
            .map_err(serde::ser::Error::custom)?;

        serializer.serialize_bytes(&buf)
    }
}
