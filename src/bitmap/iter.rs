use core::iter::FusedIterator;
use core::ops::{RangeBounds, RangeInclusive};

use crate::{NonSortedIntegers, RoaringBitmap128};

/// An iterator for `RoaringBitmap128`.
#[derive(Clone)]
pub struct Iter<'a> {
    _todo: core::marker::PhantomData<&'a RoaringBitmap128>,
}

/// An iterator for `RoaringBitmap128`.
#[derive(Clone)]
pub struct IntoIter {
    _todo: (),
}

macro_rules! iter_api {
    ($ty:ty) => {
        impl $ty {
            /// Advance the iterator to the first position where the item has a value >= `n`.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use roaring128::RoaringBitmap128;
            ///
            /// let bitmap = (1..3).collect::<RoaringBitmap128>();
            /// let mut iter = bitmap.iter();
            /// iter.advance_to(2);
            ///
            /// assert_eq!(iter.next(), Some(2));
            /// assert_eq!(iter.next(), None);
            /// ```
            pub fn advance_to(&mut self, n: u128) {
                todo!("{n}")
            }

            /// Advance the back of the iterator to the first position where the item has a
            /// value <= `n`.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use roaring128::RoaringBitmap128;
            ///
            /// let bitmap = (1..3).collect::<RoaringBitmap128>();
            /// let mut iter = bitmap.iter();
            /// iter.advance_back_to(1);
            ///
            /// assert_eq!(iter.next_back(), Some(1));
            /// assert_eq!(iter.next_back(), None);
            /// ```
            pub fn advance_back_to(&mut self, n: u128) {
                todo!("{n}")
            }

            /// Returns the range of consecutive set bits from the current position to the end
            /// of the current run.
            ///
            /// After this call, the iterator will be positioned at the first item after the
            /// returned range. Returns `None` if the iterator is exhausted.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use roaring128::RoaringBitmap128;
            ///
            /// let bm = RoaringBitmap128::from([1, 2, 4, 5]);
            /// let mut iter = bm.iter();
            /// assert_eq!(iter.next_range(), Some(1..=2));
            /// assert_eq!(iter.next(), Some(4));
            /// assert_eq!(iter.next_range(), Some(5..=5));
            /// ```
            pub fn next_range(&mut self) -> Option<RangeInclusive<u128>> {
                todo!()
            }

            /// Returns the range of consecutive set bits from the start of the current run to
            /// the current back position.
            ///
            /// After this call, the back of the iterator will be positioned at the last item
            /// before the returned range. Returns `None` if the iterator is exhausted.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use roaring128::RoaringBitmap128;
            ///
            /// let bm = RoaringBitmap128::from([1, 2, 4, 5]);
            /// let mut iter = bm.iter();
            /// assert_eq!(iter.next_range_back(), Some(4..=5));
            /// assert_eq!(iter.next_back(), Some(2));
            /// assert_eq!(iter.next_range_back(), Some(1..=1));
            /// ```
            pub fn next_range_back(&mut self) -> Option<RangeInclusive<u128>> {
                todo!()
            }
        }

        impl Iterator for $ty {
            type Item = u128;

            fn next(&mut self) -> Option<u128> {
                todo!()
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                todo!()
            }
        }

        impl DoubleEndedIterator for $ty {
            fn next_back(&mut self) -> Option<u128> {
                todo!()
            }
        }

        #[cfg(target_pointer_width = "64")]
        impl ExactSizeIterator for $ty {}

        impl FusedIterator for $ty {}
    };
}

iter_api!(Iter<'_>);
iter_api!(IntoIter);

impl RoaringBitmap128 {
    /// Iterator over each value stored in the RoaringBitmap128, guarantees values are ordered by
    /// value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    /// use core::iter::FromIterator;
    ///
    /// let bitmap = (1..3).collect::<RoaringBitmap128>();
    /// let mut iter = bitmap.iter();
    ///
    /// assert_eq!(iter.next(), Some(1));
    /// assert_eq!(iter.next(), Some(2));
    /// assert_eq!(iter.next(), None);
    /// ```
    pub fn iter(&self) -> Iter<'_> {
        todo!()
    }

    /// Iterator over values within a range stored in the RoaringBitmap128.
    ///
    /// # Panics
    ///
    /// Panics if the range start is greater than the range end, or if both bounds are equal and
    /// excluded.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use core::ops::Bound;
    /// use roaring128::RoaringBitmap128;
    ///
    /// let bitmap = RoaringBitmap128::from([0, 1, 2, 3, 4, 5, 10, 11, 12, 20, 21, u128::MAX]);
    ///
    /// // Exclusive ranges still step forward
    /// assert!(bitmap.range(1..10).eq([1, 2, 3, 4, 5]));
    ///
    /// // Inclusive ranges include the end
    /// assert!(bitmap.range(1..=10).eq([1, 2, 3, 4, 5, 10]));
    ///
    /// // Ranges can be unbounded
    /// assert!(bitmap.range(..3).eq([0, 1, 2]));
    /// assert!(bitmap.range(13..).eq([20, 21, u128::MAX]));
    ///
    /// // Ranges can be empty
    /// assert_eq!(bitmap.range(13..13).next(), None);
    /// assert_eq!(bitmap.range((Bound::Excluded(13), Bound::Included(13))).next(), None);
    /// ```
    pub fn range<R: RangeBounds<u128>>(&self, range: R) -> Iter<'_> {
        let _ = range;
        todo!()
    }

    /// Iterator over values within a range stored in the RoaringBitmap128, consuming it.
    ///
    /// # Panics
    ///
    /// Panics if the range start is greater than the range end, or if both bounds are equal and
    /// excluded.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let bitmap = RoaringBitmap128::from([0, 1, 2, 3, 4, 5, 10, 11, 12, 20, 21, u128::MAX]);
    /// assert!(bitmap.into_range(1..=10).eq([1, 2, 3, 4, 5, 10]));
    /// ```
    pub fn into_range<R: RangeBounds<u128>>(self, range: R) -> IntoIter {
        let _ = range;
        todo!()
    }

    /// Create the set from a sorted iterator. Values must be sorted and deduplicated.
    ///
    /// The values of the iterator must be ordered and strictly greater than the greatest value
    /// in the set. If a value in the iterator doesn't satisfy this requirement, it is not added
    /// and the append operation is stopped.
    ///
    /// Returns `Ok` with the requested `RoaringBitmap128`, `Err` with the number of elements
    /// that were correctly appended before failure.
    ///
    /// # Example: Create a set from an ordered list of integers.
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::from_sorted_iter(0..10).unwrap();
    ///
    /// assert!(rb.iter().eq(0..10));
    /// ```
    ///
    /// # Example: Try to create a set from a non-ordered list of integers.
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let integers = 0..10u128;
    /// let error = RoaringBitmap128::from_sorted_iter(integers.rev()).unwrap_err();
    ///
    /// assert_eq!(error.valid_until(), 1);
    /// ```
    pub fn from_sorted_iter<I: IntoIterator<Item = u128>>(
        iterator: I,
    ) -> Result<RoaringBitmap128, NonSortedIntegers> {
        let mut rb = RoaringBitmap128::new();
        rb.append(iterator).map(|_| rb)
    }

    /// Extend the set with a sorted iterator.
    ///
    /// The values of the iterator must be ordered and strictly greater than the greatest value
    /// in the set. If a value in the iterator doesn't satisfy this requirement, it is not added
    /// and the append operation is stopped.
    ///
    /// Returns `Ok` with the number of elements appended to the set, `Err` with the number of
    /// elements we effectively appended before an error occurred.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// assert_eq!(rb.append(0..10), Ok(10));
    ///
    /// assert!(rb.iter().eq(0..10));
    /// ```
    pub fn append<I: IntoIterator<Item = u128>>(
        &mut self,
        iterator: I,
    ) -> Result<u64, NonSortedIntegers> {
        let _ = iterator;
        todo!()
    }
}

impl<'a> IntoIterator for &'a RoaringBitmap128 {
    type Item = u128;
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

impl IntoIterator for RoaringBitmap128 {
    type Item = u128;
    type IntoIter = IntoIter;

    fn into_iter(self) -> IntoIter {
        todo!()
    }
}

impl<const N: usize> From<[u128; N]> for RoaringBitmap128 {
    fn from(arr: [u128; N]) -> Self {
        RoaringBitmap128::from_iter(arr)
    }
}

impl FromIterator<u128> for RoaringBitmap128 {
    fn from_iter<I: IntoIterator<Item = u128>>(iterator: I) -> RoaringBitmap128 {
        let mut rb = RoaringBitmap128::new();
        rb.extend(iterator);
        rb
    }
}

impl<'a> FromIterator<&'a u128> for RoaringBitmap128 {
    fn from_iter<I: IntoIterator<Item = &'a u128>>(iterator: I) -> RoaringBitmap128 {
        let mut rb = RoaringBitmap128::new();
        rb.extend(iterator);
        rb
    }
}

impl Extend<u128> for RoaringBitmap128 {
    fn extend<I: IntoIterator<Item = u128>>(&mut self, iterator: I) {
        let _ = iterator;
        todo!()
    }
}

impl<'a> Extend<&'a u128> for RoaringBitmap128 {
    fn extend<I: IntoIterator<Item = &'a u128>>(&mut self, iterator: I) {
        self.extend(iterator.into_iter().copied())
    }
}
