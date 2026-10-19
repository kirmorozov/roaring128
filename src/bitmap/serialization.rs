use std::io;

use crate::RoaringBitmap128;

impl RoaringBitmap128 {
    /// Return the size in bytes of the serialized output (see `FORMAT.md`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let mut bytes = Vec::with_capacity(rb1.serialized_size());
    /// rb1.serialize_into(&mut bytes).unwrap();
    /// let rb2 = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
    ///
    /// assert_eq!(rb1, rb2);
    /// ```
    pub fn serialized_size(&self) -> usize {
        todo!()
    }

    /// Serialize this bitmap into the native 128-bit Roaring format described in `FORMAT.md`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let mut bytes = vec![];
    /// rb1.serialize_into(&mut bytes).unwrap();
    /// let rb2 = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
    ///
    /// assert_eq!(rb1, rb2);
    /// ```
    pub fn serialize_into<W: io::Write>(&self, writer: W) -> io::Result<()> {
        let _ = writer;
        todo!()
    }

    /// Deserialize a bitmap from the native 128-bit Roaring format, validating the input.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let mut bytes = vec![];
    /// rb1.serialize_into(&mut bytes).unwrap();
    /// let rb2 = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
    ///
    /// assert_eq!(rb1, rb2);
    /// ```
    pub fn deserialize_from<R: io::Read>(reader: R) -> io::Result<RoaringBitmap128> {
        let _ = reader;
        todo!()
    }

    /// Deserialize a bitmap from the native 128-bit Roaring format without validating the
    /// container contents. Only use this on trusted input; invalid input results in an invalid
    /// bitmap (but never in undefined behaviour).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let mut bytes = vec![];
    /// rb1.serialize_into(&mut bytes).unwrap();
    /// let rb2 = RoaringBitmap128::deserialize_unchecked_from(&bytes[..]).unwrap();
    ///
    /// assert_eq!(rb1, rb2);
    /// ```
    pub fn deserialize_unchecked_from<R: io::Read>(reader: R) -> io::Result<RoaringBitmap128> {
        let _ = reader;
        todo!()
    }
}
