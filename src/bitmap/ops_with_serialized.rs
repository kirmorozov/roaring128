use std::io;

use crate::RoaringBitmap128;

impl RoaringBitmap128 {
    /// Computes the intersection between a materialized [`RoaringBitmap128`] and a serialized
    /// one.
    ///
    /// This is faster and more space efficient when you only need the intersection result: only
    /// the containers whose keys are present in `self` are read from `other`.
    /// The serialized bitmap is not validated (see
    /// [`deserialize_unchecked_from`](Self::deserialize_unchecked_from)).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    /// use std::io::Cursor;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let rb2: RoaringBitmap128 = (3..5).collect();
    ///
    /// let mut serialized_bytes_rb2 = Vec::new();
    /// rb2.serialize_into(&mut serialized_bytes_rb2).unwrap();
    ///
    /// let intersection = rb1.intersection_with_serialized_unchecked(Cursor::new(serialized_bytes_rb2)).unwrap();
    ///
    /// assert_eq!(rb1 & rb2, intersection);
    /// ```
    pub fn intersection_with_serialized_unchecked<R>(
        &self,
        other: R,
    ) -> io::Result<RoaringBitmap128>
    where
        R: io::Read + io::Seek,
    {
        let _ = other;
        todo!()
    }
}
