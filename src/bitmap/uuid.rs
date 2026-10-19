//! Conversions between [`uuid::Uuid`] and the `u128` values of the bitmap.
//!
//! A UUID is stored as its big-endian 128-bit integer (`Uuid::as_u128`), so iteration order is
//! the byte order of the UUIDs; time-ordered UUIDs (v6, v7) iterate in creation order.

use uuid::Uuid;

use crate::RoaringBitmap128;

impl RoaringBitmap128 {
    /// Adds a UUID to the set. Returns `true` if it was not already present.
    ///
    /// ```
    /// use roaring128::RoaringBitmap128;
    /// use uuid::Uuid;
    ///
    /// let id = Uuid::from_u128(0x0192_7a3e_8f00_7cc1_9e3a_1b2c_3d4e_5f60);
    /// let mut set = RoaringBitmap128::new();
    /// assert!(set.insert_uuid(id));
    /// assert!(set.contains_uuid(id));
    /// assert_eq!(set.iter_uuids().next(), Some(id));
    /// ```
    pub fn insert_uuid(&mut self, uuid: Uuid) -> bool {
        self.insert(uuid.as_u128())
    }

    /// Removes a UUID from the set. Returns `true` if it was present.
    pub fn remove_uuid(&mut self, uuid: Uuid) -> bool {
        self.remove(uuid.as_u128())
    }

    /// Returns `true` if the set contains the UUID.
    pub fn contains_uuid(&self, uuid: Uuid) -> bool {
        self.contains(uuid.as_u128())
    }

    /// Iterates over the set as UUIDs, in increasing order.
    pub fn iter_uuids(&self) -> impl DoubleEndedIterator<Item = Uuid> + '_ {
        self.iter().map(Uuid::from_u128)
    }
}

impl FromIterator<Uuid> for RoaringBitmap128 {
    fn from_iter<I: IntoIterator<Item = Uuid>>(iterator: I) -> Self {
        iterator.into_iter().map(|u| u.as_u128()).collect()
    }
}

impl<'a> FromIterator<&'a Uuid> for RoaringBitmap128 {
    fn from_iter<I: IntoIterator<Item = &'a Uuid>>(iterator: I) -> Self {
        iterator.into_iter().map(|u| u.as_u128()).collect()
    }
}

impl Extend<Uuid> for RoaringBitmap128 {
    fn extend<I: IntoIterator<Item = Uuid>>(&mut self, iterator: I) {
        self.extend(iterator.into_iter().map(|u| u.as_u128()))
    }
}

impl<'a> Extend<&'a Uuid> for RoaringBitmap128 {
    fn extend<I: IntoIterator<Item = &'a Uuid>>(&mut self, iterator: I) {
        self.extend(iterator.into_iter().map(|u| u.as_u128()))
    }
}
