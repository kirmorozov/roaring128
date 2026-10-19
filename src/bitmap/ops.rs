use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Sub, SubAssign};

use crate::RoaringBitmap128;

impl RoaringBitmap128 {
    /// Computes the len of the union with the specified other bitmap without creating a new
    /// bitmap.
    ///
    /// This is faster and more space efficient when you're only interested in the cardinality of
    /// the union.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let rb2: RoaringBitmap128 = (3..5).collect();
    ///
    /// assert_eq!(rb1.union_len(&rb2), (rb1 | rb2).len());
    /// ```
    pub fn union_len(&self, other: &RoaringBitmap128) -> u64 {
        self.len
            .wrapping_add(other.len)
            .wrapping_sub(self.intersection_len(other))
    }

    /// Computes the len of the intersection with the specified other bitmap without creating a
    /// new bitmap.
    ///
    /// This is faster and more space efficient when you're only interested in the cardinality of
    /// the intersection.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let rb2: RoaringBitmap128 = (3..5).collect();
    ///
    /// assert_eq!(rb1.intersection_len(&rb2), (rb1 & rb2).len());
    /// ```
    pub fn intersection_len(&self, other: &RoaringBitmap128) -> u64 {
        self.intersection_len_impl(other)
    }

    /// Computes the len of the difference with the specified other bitmap without creating a new
    /// bitmap.
    ///
    /// This is faster and more space efficient when you're only interested in the cardinality of
    /// the difference.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let rb2: RoaringBitmap128 = (3..5).collect();
    ///
    /// assert_eq!(rb1.difference_len(&rb2), (rb1 - rb2).len());
    /// ```
    pub fn difference_len(&self, other: &RoaringBitmap128) -> u64 {
        self.len - self.intersection_len(other)
    }

    /// Computes the len of the symmetric difference with the specified other bitmap without
    /// creating a new bitmap.
    ///
    /// This is faster and more space efficient when you're only interested in the cardinality of
    /// the symmetric difference.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let rb2: RoaringBitmap128 = (3..5).collect();
    ///
    /// assert_eq!(rb1.symmetric_difference_len(&rb2), (rb1 ^ rb2).len());
    /// ```
    pub fn symmetric_difference_len(&self, other: &RoaringBitmap128) -> u64 {
        let intersection_len = self.intersection_len(other);
        self.len
            .wrapping_add(other.len)
            .wrapping_sub(intersection_len)
            .wrapping_sub(intersection_len)
    }
}

impl BitOr<RoaringBitmap128> for RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// An `union` between two sets.
    fn bitor(mut self, rhs: RoaringBitmap128) -> RoaringBitmap128 {
        BitOrAssign::bitor_assign(&mut self, rhs);
        self
    }
}

impl BitOr<&RoaringBitmap128> for RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// An `union` between two sets.
    fn bitor(mut self, rhs: &RoaringBitmap128) -> RoaringBitmap128 {
        BitOrAssign::bitor_assign(&mut self, rhs);
        self
    }
}

impl BitOr<RoaringBitmap128> for &RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// An `union` between two sets.
    fn bitor(self, rhs: RoaringBitmap128) -> RoaringBitmap128 {
        BitOr::bitor(rhs, self)
    }
}

impl BitOr<&RoaringBitmap128> for &RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// An `union` between two sets.
    fn bitor(self, rhs: &RoaringBitmap128) -> RoaringBitmap128 {
        self.union_ref(rhs)
    }
}

impl BitOrAssign<RoaringBitmap128> for RoaringBitmap128 {
    /// An `union` between two sets.
    fn bitor_assign(&mut self, rhs: RoaringBitmap128) {
        self.union_assign_owned(rhs)
    }
}

impl BitOrAssign<&RoaringBitmap128> for RoaringBitmap128 {
    /// An `union` between two sets.
    fn bitor_assign(&mut self, rhs: &RoaringBitmap128) {
        self.union_assign_ref(rhs)
    }
}

impl BitAnd<RoaringBitmap128> for RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// An `intersection` between two sets.
    fn bitand(mut self, rhs: RoaringBitmap128) -> RoaringBitmap128 {
        BitAndAssign::bitand_assign(&mut self, rhs);
        self
    }
}

impl BitAnd<&RoaringBitmap128> for RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// An `intersection` between two sets.
    fn bitand(mut self, rhs: &RoaringBitmap128) -> RoaringBitmap128 {
        BitAndAssign::bitand_assign(&mut self, rhs);
        self
    }
}

impl BitAnd<RoaringBitmap128> for &RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// An `intersection` between two sets.
    fn bitand(self, rhs: RoaringBitmap128) -> RoaringBitmap128 {
        BitAnd::bitand(rhs, self)
    }
}

impl BitAnd<&RoaringBitmap128> for &RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// An `intersection` between two sets.
    fn bitand(self, rhs: &RoaringBitmap128) -> RoaringBitmap128 {
        self.intersection_ref(rhs)
    }
}

impl BitAndAssign<RoaringBitmap128> for RoaringBitmap128 {
    /// An `intersection` between two sets.
    fn bitand_assign(&mut self, rhs: RoaringBitmap128) {
        self.intersect_assign_owned(rhs)
    }
}

impl BitAndAssign<&RoaringBitmap128> for RoaringBitmap128 {
    /// An `intersection` between two sets.
    fn bitand_assign(&mut self, rhs: &RoaringBitmap128) {
        self.intersect_assign_ref(rhs)
    }
}

impl Sub<RoaringBitmap128> for RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// A `difference` between two sets.
    fn sub(mut self, rhs: RoaringBitmap128) -> RoaringBitmap128 {
        SubAssign::sub_assign(&mut self, &rhs);
        self
    }
}

impl Sub<&RoaringBitmap128> for RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// A `difference` between two sets.
    fn sub(mut self, rhs: &RoaringBitmap128) -> RoaringBitmap128 {
        SubAssign::sub_assign(&mut self, rhs);
        self
    }
}

impl Sub<RoaringBitmap128> for &RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// A `difference` between two sets.
    fn sub(self, rhs: RoaringBitmap128) -> RoaringBitmap128 {
        Sub::sub(self, &rhs)
    }
}

impl Sub<&RoaringBitmap128> for &RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// A `difference` between two sets.
    fn sub(self, rhs: &RoaringBitmap128) -> RoaringBitmap128 {
        self.difference_ref(rhs)
    }
}

impl SubAssign<RoaringBitmap128> for RoaringBitmap128 {
    /// A `difference` between two sets.
    fn sub_assign(&mut self, rhs: RoaringBitmap128) {
        SubAssign::sub_assign(self, &rhs)
    }
}

impl SubAssign<&RoaringBitmap128> for RoaringBitmap128 {
    /// A `difference` between two sets.
    fn sub_assign(&mut self, rhs: &RoaringBitmap128) {
        self.difference_assign_ref(rhs)
    }
}

impl BitXor<RoaringBitmap128> for RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// A `symmetric difference` between two sets.
    fn bitxor(mut self, rhs: RoaringBitmap128) -> RoaringBitmap128 {
        BitXorAssign::bitxor_assign(&mut self, rhs);
        self
    }
}

impl BitXor<&RoaringBitmap128> for RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// A `symmetric difference` between two sets.
    fn bitxor(mut self, rhs: &RoaringBitmap128) -> RoaringBitmap128 {
        BitXorAssign::bitxor_assign(&mut self, rhs);
        self
    }
}

impl BitXor<RoaringBitmap128> for &RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// A `symmetric difference` between two sets.
    fn bitxor(self, rhs: RoaringBitmap128) -> RoaringBitmap128 {
        BitXor::bitxor(rhs, self)
    }
}

impl BitXor<&RoaringBitmap128> for &RoaringBitmap128 {
    type Output = RoaringBitmap128;

    /// A `symmetric difference` between two sets.
    fn bitxor(self, rhs: &RoaringBitmap128) -> RoaringBitmap128 {
        self.symmetric_difference_ref(rhs)
    }
}

impl BitXorAssign<RoaringBitmap128> for RoaringBitmap128 {
    /// A `symmetric difference` between two sets.
    fn bitxor_assign(&mut self, rhs: RoaringBitmap128) {
        self.symmetric_difference_assign_owned(rhs)
    }
}

impl BitXorAssign<&RoaringBitmap128> for RoaringBitmap128 {
    /// A `symmetric difference` between two sets.
    fn bitxor_assign(&mut self, rhs: &RoaringBitmap128) {
        self.symmetric_difference_assign_ref(rhs)
    }
}
